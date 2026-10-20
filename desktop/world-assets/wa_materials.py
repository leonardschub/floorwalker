"""Surface materials of the sweatshop world: procedural recipes, their bake, and the baked materials.

Each recipe is a Blender node graph (ported from the accepted look, branch `prototype/sweatshop-look`,
direction C) that is evaluated on a UV square covering one texture tile of real size `tile` (metres).
Noise and Voronoi are sampled on a torus (4D: cos/sin of each UV axis), and bricks, bands and key grids
are built from whole periods, so every bake repeats without a seam. Strip textures (walls) repeat only
along U; their V runs from the floor (0 m) to the ceiling (3 m), so rising damp and soot stay put.

Each recipe bakes, with Cycles on the CPU, into three 1024x1024 PNGs in `<out>/textures/`:
  <name>_color.png   base colour, sRGB
  <name>_normal.png  tangent-space normal (OpenGL / glTF convention), from the baked height
  <name>_orm.png     glTF metallic-roughness layout: R = 1 (no occlusion), G = roughness, B = metallic
"""
import hashlib
import math
import os
import struct
import zlib

import bpy
import numpy as np

SIZE = 1024
SAMPLES = 8
TAU = 2.0 * math.pi
WALL_H = 3.0

RECIPES = {}


def recipe(tile, periodic_v=True, emissive=None):
    """Register a recipe. tile = (u, v) in metres; emissive = strength when the colour map glows."""

    def wrap(fn):
        RECIPES[fn.__name__] = dict(fn=fn, tile=tile, periodic_v=periodic_v, emissive=emissive)
        return fn

    return wrap


def tile_of(name):
    r = RECIPES.get(name)
    return r["tile"] if r else (1.0, 1.0)


class G:
    """Node-graph helper. Every pattern it makes repeats over one tile."""

    def __init__(self, nt, tile, periodic_v):
        self.nt = nt
        self.tu, self.tv = tile
        self.periodic_v = periodic_v
        self.seed = 0
        tc = self.n("ShaderNodeTexCoord")
        sep = self.n("ShaderNodeSeparateXYZ")
        self.l(tc.outputs["UV"], sep.inputs[0])
        self.u, self.v = sep.outputs[0], sep.outputs[1]
        self.um = self.math("MULTIPLY", self.u, self.tu)  # metres along U
        self.vm = self.math("MULTIPLY", self.v, self.tv)  # metres along V (height for strips)
        au = self.math("MULTIPLY", self.u, TAU)
        self.cu, self.su = self.math("COSINE", au), self.math("SINE", au)
        av = self.math("MULTIPLY", self.v, TAU)
        self.cv, self.sv = self.math("COSINE", av), self.math("SINE", av)

    # plumbing
    def n(self, kind, **props):
        node = self.nt.nodes.new(kind)
        for k, v in props.items():
            setattr(node, k, v)
        return node

    def l(self, a, b):
        self.nt.links.new(a, b)

    def put(self, sock, val):
        if isinstance(val, (int, float)):
            sock.default_value = val
        elif isinstance(val, tuple):
            sock.default_value = (*val, 1) if len(val) == 3 else val
        else:
            self.l(val, sock)

    def math(self, op, a, b=None, c=None):
        m = self.n("ShaderNodeMath", operation=op)
        for i, val in enumerate((a, b, c)):
            if val is not None:
                self.put(m.inputs[i], val)
        return m.outputs[0]

    def combine(self, x, y, z):
        c = self.n("ShaderNodeCombineXYZ")
        for i, val in enumerate((x, y, z)):
            self.put(c.inputs[i], val)
        return c.outputs[0]

    # periodic coordinates
    def coords(self, scale, su=1.0, sv=1.0):
        """4D coordinates whose features are `scale` per metre (times su/sv per axis), periodic per tile."""
        self.seed += 1
        k = self.seed
        ru = scale * su * self.tu / TAU
        x = self.math("MULTIPLY_ADD", self.cu, ru, 17.31 * k)
        y = self.math("MULTIPLY", self.su, ru)
        if self.periodic_v:
            rv = scale * sv * self.tv / TAU
            z = self.math("MULTIPLY", self.cv, rv)
            w = self.math("MULTIPLY_ADD", self.sv, rv, 5.77 * k)
        else:
            z = self.math("MULTIPLY", self.vm, scale * sv)
            w = 5.77 * k
        return self.combine(x, y, z), w

    def noise(self, scale, detail=8.0, rough=0.6, su=1.0, sv=1.0, out="Fac", distortion=0.0):
        vec, w = self.coords(scale, su, sv)
        t = self.n("ShaderNodeTexNoise", noise_dimensions="4D")
        t.inputs["Scale"].default_value = 1.0
        t.inputs["Detail"].default_value = detail
        t.inputs["Roughness"].default_value = rough
        t.inputs["Distortion"].default_value = distortion
        self.l(vec, t.inputs["Vector"])
        self.put(t.inputs["W"], w)
        return t.outputs[out]

    def voronoi_edges(self, scale, warp=0.0, warp_scale=2.0):
        vec, w = self.coords(scale)
        if warp:
            jitter = self.noise(warp_scale, 2.0, 0.5, out="Color")
            # recentre the noise colour around zero so the warp does not drift the cells
            centred = self.n("ShaderNodeVectorMath", operation="MULTIPLY_ADD")
            self.l(jitter, centred.inputs[0])
            centred.inputs[1].default_value = (warp, warp, warp)
            centred.inputs[2].default_value = (-warp / 2, -warp / 2, -warp / 2)
            add = self.n("ShaderNodeVectorMath", operation="ADD")
            self.l(vec, add.inputs[0])
            self.l(centred.outputs[0], add.inputs[1])
            vec = add.outputs[0]
        t = self.n("ShaderNodeTexVoronoi", voronoi_dimensions="4D", feature="DISTANCE_TO_EDGE")
        t.inputs["Scale"].default_value = 1.0
        self.l(vec, t.inputs["Vector"])
        self.put(t.inputs["W"], w)
        return t.outputs["Distance"]

    def ramp(self, fac, stops):
        r = self.n("ShaderNodeValToRGB")
        els = r.color_ramp.elements
        while len(els) < len(stops):
            els.new(0.5)
        for e, (pos, col) in zip(els, stops):
            e.position = pos
            e.color = (*col, 1) if len(col) == 3 else col
        self.put(r.inputs["Fac"], fac)
        return r.outputs["Color"]

    def mix(self, fac, a, b, blend="MIX"):
        m = self.n("ShaderNodeMix", data_type="RGBA", blend_type=blend)
        for sock, val in ((m.inputs[0], fac), (m.inputs[6], a), (m.inputs[7], b)):
            self.put(sock, val)
        return m.outputs[2]

    def maprange(self, val, a, b, c, d):
        m = self.n("ShaderNodeMapRange")
        self.put(m.inputs["Value"], val)
        for i, x in zip((1, 2, 3, 4), (a, b, c, d)):
            m.inputs[i].default_value = x
        return m.outputs[0]

    def bricks(self, width, height, offset, mortar, c1, c2, cm, along_v=None):
        """A brick or tile grid in metres. Returns (colour, mortar mask: 1 in the joints).

        Per-brick shade comes from a white noise of the wrapped brick index, so it repeats too.
        """
        ncols = round(self.tu / width)
        nrows = round(self.tv / height)
        assert abs(ncols * width - self.tu) < 1e-6, "bricks must fit the tile along U"
        if self.periodic_v:
            assert abs(nrows * height - self.tv) < 1e-6, "bricks must fit the tile along V"
        yv = self.math("DIVIDE", self.vm, height)
        row = self.math("FLOOR", yv)
        shift = self.math("FRACT", self.math("MULTIPLY", row, offset))
        xs = self.math("ADD", self.math("DIVIDE", self.um, width), shift)
        col = self.math("FLOORED_MODULO", self.math("FLOOR", xs), ncols)
        rowk = self.math("FLOORED_MODULO", row, nrows) if self.periodic_v else row
        wn = self.n("ShaderNodeTexWhiteNoise", noise_dimensions="3D")
        self.l(self.combine(col, rowk, 0.37), wn.inputs["Vector"])
        shade = self.mix(wn.outputs["Value"], c1, c2)
        fx, fy = self.math("FRACT", xs), self.math("FRACT", yv)
        dx = self.math("MULTIPLY", self.math("MINIMUM", fx, self.math("SUBTRACT", 1.0, fx)), width)
        dy = self.math("MULTIPLY", self.math("MINIMUM", fy, self.math("SUBTRACT", 1.0, fy)), height)
        d = self.math("MINIMUM", dx, dy)
        joint = self.maprange(d, mortar * 0.35, mortar * 0.5, 1.0, 0.0)
        return self.mix(joint, shade, cm), joint

    def bands(self, count, distortion=None, distortion_amount=0.0):
        """Bands across V: `count` per tile, optionally wobbled by a noise (in band periods)."""
        phase = self.math("MULTIPLY", self.v, float(count))
        if distortion is not None:
            phase = self.math("MULTIPLY_ADD", distortion, distortion_amount, phase)
        s = self.math("SINE", self.math("MULTIPLY", phase, TAU))
        return self.math("MULTIPLY_ADD", s, 0.5, 0.5)


# ---------------------------------------------------------------------------------------------
# Recipes. Each returns dict(color, rough, metal, height, depth) where height is 0..1 and depth is
# the relief in metres that `height` spans (for the normal map).


@recipe((2.4, WALL_H), periodic_v=False)
def brick(g):
    col, joint = g.bricks(0.24, 0.075, 0.5, 0.010, (0.32, 0.12, 0.08), (0.22, 0.08, 0.05), (0.30, 0.28, 0.25))
    speck = g.ramp(g.noise(60, 4), [(0.4, (0.7, 0.7, 0.7)), (0.7, (1.0, 1.0, 1.0))])
    col = g.mix(1.0, col, speck, "MULTIPLY")
    soot = g.ramp(g.noise(1.2, 10, 0.7), [(0.25, (1, 1, 1)), (0.7, (0.18, 0.17, 0.15))])
    col = g.mix(1.0, col, soot, "MULTIPLY")
    streak = g.ramp(g.noise(1.0, 6, 0.6, su=9.0, sv=0.35), [(0.5, (1, 1, 1)), (0.72, (0.30, 0.27, 0.20))])
    col = g.mix(1.0, col, streak, "MULTIPLY")
    salt = g.ramp(g.noise(5, 10, 0.7), [(0.68, (0, 0, 0)), (0.75, (1, 1, 1))])
    col = g.mix(salt, col, (0.42, 0.40, 0.35))
    # rising damp: darker and greener in the lowest 1.5 m, with a ragged edge
    damp = g.maprange(g.vm, 0.0, 1.5, 0.85, 0.0)
    edge = g.ramp(g.noise(3, 6), [(0.3, (0, 0, 0)), (0.6, (1, 1, 1))])
    dfac = g.mix(damp, (0, 0, 0), edge)
    col = g.mix(dfac, col, (0.06, 0.07, 0.04))
    pits = g.noise(30, 6, 0.6)
    height = g.math("MULTIPLY_ADD", pits, 0.15, g.math("SUBTRACT", 0.85, g.math("MULTIPLY", joint, 0.85)))
    return dict(color=col, rough=0.92, metal=0.0, height=height, depth=0.006)


@recipe((2.4, 2.4))
def concrete(g):
    base = g.ramp(g.noise(6, 12, 0.65), [(0.3, (0.13, 0.13, 0.13)), (0.7, (0.22, 0.21, 0.20))])
    edges = g.voronoi_edges(0.45, warp=0.2)
    crack = g.ramp(edges, [(0.0, (0.03, 0.03, 0.03)), (0.004, (1, 1, 1))])
    col = g.mix(1.0, base, crack, "MULTIPLY")
    stains = g.ramp(g.noise(0.6, 6, 0.7), [(0.45, (1, 1, 1)), (0.65, (0.45, 0.40, 0.30))])
    col = g.mix(1.0, col, stains, "MULTIPLY")
    oil = g.ramp(g.noise(0.42, 2), [(0.62, (1, 1, 1)), (0.68, (0.25, 0.25, 0.25))])
    col = g.mix(1.0, col, oil, "MULTIPLY")
    grime = g.ramp(g.noise(2.2, 12, 0.75), [(0.3, (1, 1, 1)), (0.6, (0.30, 0.26, 0.20))])
    col = g.mix(1.0, col, grime, "MULTIPLY")
    puddle = g.ramp(g.noise(0.42, 3, 0.5), [(0.64, (0, 0, 0)), (0.67, (1, 1, 1))])
    col = g.mix(puddle, col, (0.03, 0.03, 0.03))
    rough = g.ramp(g.noise(0.6, 6, 0.7), [(0.45, (0.9, 0.9, 0.9)), (0.7, (0.5, 0.5, 0.5))])
    rough = g.mix(puddle, rough, (0.05, 0.05, 0.05))
    crack_h = g.ramp(edges, [(0.0, (0, 0, 0)), (0.004, (1, 1, 1))])
    height = g.math("MULTIPLY_ADD", g.noise(20, 8, 0.6), 0.25, g.math("MULTIPLY", crack_h, 0.75))
    return dict(color=col, rough=rough, metal=0.0, height=height, depth=0.004)


@recipe((2.4, 2.4))
def ceiling(g):
    col = g.ramp(g.noise(1.5, 10, 0.7), [(0.3, (0.05, 0.05, 0.05)), (0.7, (0.14, 0.13, 0.12))])
    tiles, joint = g.bricks(0.6, 0.6, 0.0, 0.016, (1, 1, 1), (0.8, 0.75, 0.6), (0.2, 0.2, 0.2))
    col = g.mix(1.0, col, tiles, "MULTIPLY")
    water = g.ramp(g.noise(0.8, 5, 0.6), [(0.55, (1, 1, 1)), (0.6, (0.45, 0.30, 0.15)), (0.66, (0.75, 0.6, 0.4))])
    col = g.mix(1.0, col, water, "MULTIPLY")
    height = g.math("MULTIPLY_ADD", g.noise(40, 4, 0.5), 0.2, g.math("SUBTRACT", 0.8, g.math("MULTIPLY", joint, 0.8)))
    return dict(color=col, rough=0.95, metal=0.0, height=height, depth=0.004)


@recipe((2.4, WALL_H), periodic_v=False)
def peeling_paint(g):
    peel = g.noise(1.6, 14, 0.75)
    mask = g.ramp(peel, [(0.52, (0, 0, 0)), (0.55, (1, 1, 1))])
    paint = g.ramp(g.noise(8, 4, 0.5), [(0.3, (0.15, 0.18, 0.15)), (0.7, (0.20, 0.23, 0.19))])
    under = g.ramp(g.noise(12, 8, 0.6), [(0.3, (0.07, 0.065, 0.06)), (0.7, (0.11, 0.10, 0.09))])
    col = g.mix(mask, paint, under)
    drip = g.ramp(g.noise(1, 3, 0.5, su=6.0, sv=0.25), [(0.55, (1, 1, 1)), (0.75, (0.55, 0.5, 0.42))])
    col = g.mix(1.0, col, drip, "MULTIPLY")
    damp = g.maprange(g.vm, 0.0, 1.2, 0.6, 0.0)
    col = g.mix(damp, col, (0.08, 0.09, 0.07))
    height = g.math("SUBTRACT", 1.0, mask)
    return dict(color=col, rough=0.85, metal=0.0, height=height, depth=0.0015)


@recipe((1.2, 1.2))
def laminate(g):
    """Worn wood-grain laminate with chipped patches showing chipboard, cup rings and scratches."""
    wobble = g.noise(2.1, 6, 0.6, sv=14 / 2.1)
    grain = g.bands(54, wobble, 0.5)
    wood = g.ramp(grain, [(0.2, (0.20, 0.11, 0.05)), (0.8, (0.36, 0.22, 0.11))])
    chip = g.ramp(g.noise(5, 10, 0.7), [(0.62, (0, 0, 0)), (0.65, (1, 1, 1))])
    chipboard = g.ramp(g.noise(80, 3), [(0.3, (0.38, 0.30, 0.20)), (0.7, (0.55, 0.45, 0.30))])
    col = g.mix(chip, wood, chipboard)
    grime = g.ramp(g.noise(6, 10, 0.7), [(0.3, (1, 1, 1)), (0.7, (0.45, 0.40, 0.34))])
    col = g.mix(1.0, col, grime, "MULTIPLY")
    ring = g.ramp(g.noise(0.85, 2), [(0.6, (1, 1, 1)), (0.62, (0.4, 0.28, 0.16)), (0.64, (1, 1, 1))])
    col = g.mix(1.0, col, ring, "MULTIPLY")
    scratch = g.ramp(g.noise(3, 10, 0.5, sv=35.0), [(0.66, (1, 1, 1)), (0.7, (1.6, 1.4, 1.2))])
    col = g.mix(1.0, col, scratch, "MULTIPLY")
    rough = g.mix(chip, (0.45, 0.45, 0.45), (0.95, 0.95, 0.95))
    return dict(color=col, rough=rough, metal=0.0, height=g.math("SUBTRACT", 1.0, chip), depth=0.0008)


def _rusty(g, base, coverage=0.45):
    rust_mask = g.ramp(g.noise(7, 12, 0.7), [(coverage, (0, 0, 0)), (coverage + 0.15, (1, 1, 1))])
    rust = g.ramp(g.noise(30, 6), [(0.3, (0.18, 0.07, 0.03)), (0.7, (0.35, 0.15, 0.05))])
    col = g.mix(rust_mask, base, rust)
    rough = g.mix(rust_mask, (0.45, 0.45, 0.45), (0.95, 0.95, 0.95))
    metal = g.math("SUBTRACT", 0.8, g.math("MULTIPLY", rust_mask, 0.8))
    height = g.math("MULTIPLY_ADD", g.noise(60, 4), 0.3, g.math("MULTIPLY", rust_mask, 0.7))
    return dict(color=col, rough=rough, metal=metal, height=height, depth=0.0008)


@recipe((1.0, 1.0))
def rusty_steel(g):
    """Desk legs."""
    return _rusty(g, (0.12, 0.12, 0.13))


@recipe((1.0, 1.0))
def rusty_steel_dark(g):
    """Folding chairs."""
    return _rusty(g, (0.07, 0.07, 0.08))


@recipe((1.0, 1.0))
def rusty_galvanised(g):
    """Pipes, the bucket, tube-light housings."""
    return _rusty(g, (0.22, 0.22, 0.22))


@recipe((1.0, 1.0))
def rusty_sign(g):
    """The plate over each doorway: dark steel eaten by rust."""
    return _rusty(g, (0.05, 0.06, 0.05), 0.40)


@recipe((0.5, 0.5))
def rusty_can(g):
    """Faded red paint on a can, mostly rust."""
    return _rusty(g, (0.35, 0.05, 0.04), 0.50)


@recipe((0.6, 0.6))
def yellowed_plastic(g):
    base = g.ramp(g.noise(3, 6), [(0.3, (0.50, 0.46, 0.34)), (0.7, (0.60, 0.56, 0.44))])
    dirt = g.ramp(g.noise(9, 10, 0.7), [(0.35, (1, 1, 1)), (0.7, (0.32, 0.28, 0.20))])
    col = g.mix(1.0, base, dirt, "MULTIPLY")
    residue = g.ramp(g.noise(2, 3), [(0.7, (1, 1, 1)), (0.72, (0.75, 0.72, 0.62))])
    col = g.mix(1.0, col, residue, "MULTIPLY")
    return dict(color=col, rough=0.55, metal=0.0, height=g.noise(50, 4), depth=0.0003)


@recipe((0.6, 0.6))
def bezel(g):
    """Dark worn plastic: CRT bezel, drive slots, badge."""
    col = g.ramp(g.noise(10, 6), [(0.3, (0.17, 0.16, 0.14)), (0.7, (0.20, 0.19, 0.16))])
    dirt = g.ramp(g.noise(9, 10, 0.7), [(0.35, (1, 1, 1)), (0.7, (0.55, 0.50, 0.42))])
    return dict(color=g.mix(1.0, col, dirt, "MULTIPLY"), rough=0.7, metal=0.0, height=g.noise(50, 4), depth=0.0003)


@recipe((0.38, 0.38))
def keyboard(g):
    """Grubby beige keys in a staggered grid with dark gaps."""
    keys, joint = g.bricks(0.019, 0.019, 0.25, 0.005, (0.55, 0.51, 0.40), (0.48, 0.44, 0.34), (0.04, 0.04, 0.035))
    dirt = g.ramp(g.noise(14, 10, 0.7), [(0.3, (1, 1, 1)), (0.7, (0.40, 0.35, 0.26))])
    col = g.mix(1.0, keys, dirt, "MULTIPLY")
    return dict(color=col, rough=0.5, metal=0.0, height=g.math("SUBTRACT", 1.0, joint), depth=0.002)


@recipe((0.25, 0.25), emissive=1.4)
def pc_screen(g):
    """Bluish lit screen with scanlines: a PC showing no session terminal. The colour map is also the
    emissive map."""
    scan = g.ramp(g.bands(48), [(0.2, (0.55, 0.55, 0.55)), (0.8, (1, 1, 1))])
    flicker = g.ramp(g.noise(6, 3), [(0.3, (0.85, 0.85, 0.85)), (0.7, (1, 1, 1))])
    col = g.mix(1.0, g.mix(1.0, (0.45, 0.65, 1.0), scan, "MULTIPLY"), flicker, "MULTIPLY")
    return dict(color=col, rough=0.15, metal=0.0, height=0.5, depth=0.0)


@recipe((1.0, 1.0))
def robot_paint(g):
    """Chipped industrial yellow paint over steel, with panel seams, grime and rust spots."""
    chips = g.ramp(g.noise(9, 12, 0.7), [(0.60, (0, 0, 0)), (0.66, (1, 1, 1))])
    paint = g.ramp(g.noise(4, 6), [(0.3, (0.62, 0.40, 0.06)), (0.7, (0.75, 0.52, 0.10))])
    steel = g.ramp(g.noise(40, 4), [(0.3, (0.22, 0.22, 0.23)), (0.7, (0.32, 0.31, 0.30))])
    col = g.mix(chips, paint, steel)
    seams, seam_mask = g.bricks(0.125, 1 / 12, 0.0, 0.003, (1, 1, 1), (1, 1, 1), (0.15, 0.15, 0.15))
    col = g.mix(1.0, col, seams, "MULTIPLY")
    grime = g.ramp(g.noise(7, 10, 0.75), [(0.35, (1, 1, 1)), (0.7, (0.30, 0.26, 0.20))])
    col = g.mix(1.0, col, grime, "MULTIPLY")
    rust = g.ramp(g.noise(14, 10, 0.7), [(0.66, (0, 0, 0)), (0.72, (1, 1, 1))])
    col = g.mix(rust, col, (0.28, 0.12, 0.04))
    rough = g.mix(chips, (0.55, 0.55, 0.55), (0.35, 0.35, 0.35))
    metal = g.math("MULTIPLY", chips, 0.9)
    height = g.math("SUBTRACT", g.math("SUBTRACT", 1.0, g.math("MULTIPLY", chips, 0.5)), g.math("MULTIPLY", seam_mask, 0.5))
    return dict(color=col, rough=rough, metal=metal, height=height, depth=0.0008)


@recipe((1.0, 1.0))
def robot_joint(g):
    """Dark oily joints."""
    col = g.ramp(g.noise(20, 8), [(0.3, (0.05, 0.05, 0.055)), (0.7, (0.11, 0.11, 0.11))])
    oil = g.ramp(g.noise(5, 6), [(0.55, (1, 1, 1)), (0.7, (0.6, 0.5, 0.35))])
    return dict(color=g.mix(1.0, col, oil, "MULTIPLY"), rough=0.4, metal=0.6, height=g.noise(40, 4), depth=0.0003)


@recipe((0.2, 0.2))
def robot_face(g):
    """A blank, smudged black glass plate. Ticket 22 puts the face pattern and name on it."""
    smudge = g.ramp(g.noise(6, 8), [(0.4, (0.05, 0.05, 0.05)), (0.7, (0.4, 0.4, 0.4))])
    return dict(color=(0.01, 0.012, 0.015), rough=smudge, metal=0.0, height=0.5, depth=0.0)


@recipe((1.0, 1.0))
def cardboard(g):
    col = g.ramp(g.noise(4, 8), [(0.3, (0.30, 0.20, 0.10)), (0.7, (0.42, 0.30, 0.16))])
    water = g.ramp(g.noise(1.2, 4), [(0.6, (1, 1, 1)), (0.66, (0.6, 0.5, 0.4))])
    col = g.mix(1.0, col, g.ramp(g.noise(7, 10, 0.7), [(0.35, (1, 1, 1)), (0.7, (0.55, 0.48, 0.38))]), "MULTIPLY")
    return dict(color=g.mix(1.0, col, water, "MULTIPLY"), rough=0.95, metal=0.0, height=g.noise(30, 6), depth=0.0004)


@recipe((0.5, 0.5))
def paper(g):
    """Grubby off-white paper."""
    col = g.ramp(g.noise(10, 6), [(0.3, (0.55, 0.53, 0.46)), (0.7, (0.66, 0.63, 0.55))])
    dirt = g.ramp(g.noise(4, 8, 0.7), [(0.45, (1, 1, 1)), (0.75, (0.6, 0.55, 0.45))])
    return dict(color=g.mix(1.0, col, dirt, "MULTIPLY"), rough=0.9, metal=0.0, height=g.noise(25, 8), depth=0.0006)


@recipe((1.0, 1.0))
def rubber(g):
    """Black cable sheath."""
    col = g.ramp(g.noise(12, 6), [(0.3, (0.025, 0.025, 0.025)), (0.7, (0.045, 0.043, 0.04))])
    dust = g.ramp(g.noise(5, 8, 0.7), [(0.5, (0, 0, 0)), (0.8, (1, 1, 1))])
    col = g.mix(dust, col, (0.12, 0.11, 0.10))
    return dict(color=col, rough=0.6, metal=0.0, height=g.noise(60, 4), depth=0.0002)


# The CCTV room (management-rooms spec, direction A "black site", from branch `prototype/cctv-look`).
# Its tiles are half a feed column (2/3 m) wide, so the strips it widens by continue the pattern.
CCTV_TILE = 2.0 / 3.0
CCTV_H = 4.2


@recipe((CCTV_TILE, CCTV_H), periodic_v=False)
def cctv_wall(g):
    """Near-black glass panels, one per tile, between carbon bands (under 0.3 m and over 3.9 m),
    with thin dark seams. The glass is glossy with faint smudges; the carbon a fine matte weave."""
    weave, weave_joint = g.bricks(CCTV_TILE / 32, CCTV_TILE / 32, 0.5, 0.004, (0.020, 0.021, 0.024),
                                  (0.034, 0.036, 0.040), (0.012, 0.012, 0.014))
    low = g.math("LESS_THAN", g.vm, 0.3)
    high = g.math("GREATER_THAN", g.vm, 3.9)
    carbon = g.math("MINIMUM", g.math("ADD", low, high), 1.0)
    glass = g.ramp(g.noise(1.5, 6, 0.6), [(0.3, (0.008, 0.010, 0.013)), (0.7, (0.014, 0.017, 0.021))])
    col = g.mix(carbon, glass, weave)
    fu = g.math("FRACT", g.u)
    d_u = g.math("MULTIPLY", g.math("MINIMUM", fu, g.math("SUBTRACT", 1.0, fu)), CCTV_TILE)
    d_v = g.math("MINIMUM", g.math("ABSOLUTE", g.math("SUBTRACT", g.vm, 0.3)),
                 g.math("ABSOLUTE", g.math("SUBTRACT", g.vm, 3.9)))
    seam = g.maprange(g.math("MINIMUM", d_u, d_v), 0.0025, 0.0045, 1.0, 0.0)
    col = g.mix(seam, col, (0.002, 0.002, 0.003))
    smudge = g.ramp(g.noise(4, 8, 0.7), [(0.45, (0.05, 0.05, 0.05)), (0.75, (0.22, 0.22, 0.22))])
    rough = g.mix(carbon, smudge, (0.42, 0.42, 0.42))
    rough = g.mix(seam, rough, (0.7, 0.7, 0.7))
    metal = g.math("MULTIPLY", carbon, 0.2)
    relief = g.math("SUBTRACT", 1.0, g.math("MULTIPLY", g.math("MULTIPLY", weave_joint, carbon), 0.3))
    height = g.math("SUBTRACT", relief, g.math("MULTIPLY", seam, 0.7))
    return dict(color=col, rough=rough, metal=metal, height=height, depth=0.002)


@recipe((CCTV_TILE, CCTV_TILE))
def cctv_floor(g):
    """Glossy near-black floor tiles, one per tile, with dark joints, scuffs and faint smudges."""
    col = g.ramp(g.noise(2.5, 8, 0.6), [(0.3, (0.009, 0.010, 0.012)), (0.7, (0.016, 0.017, 0.020))])
    tiles, joint = g.bricks(CCTV_TILE, CCTV_TILE, 0.0, 0.006, (1, 1, 1), (1, 1, 1), (0.25, 0.25, 0.25))
    col = g.mix(1.0, col, tiles, "MULTIPLY")
    scuff = g.ramp(g.noise(3, 10, 0.6, su=1.0, sv=12.0), [(0.62, (0, 0, 0)), (0.7, (1, 1, 1))])
    smudge = g.ramp(g.noise(1.2, 6, 0.7), [(0.4, (0.05, 0.05, 0.05)), (0.75, (0.2, 0.2, 0.2))])
    rough = g.mix(scuff, smudge, (0.35, 0.35, 0.35))
    rough = g.mix(joint, rough, (0.8, 0.8, 0.8))
    col = g.mix(g.math("MULTIPLY", scuff, 0.4), col, (0.03, 0.03, 0.035))
    height = g.math("SUBTRACT", 1.0, joint)
    return dict(color=col, rough=rough, metal=0.0, height=height, depth=0.002)


@recipe((1.0, 1.0))
def charred_paint(g):
    """The robots' yellow paint burnt black: soot and blisters, with scraps of paint and bare steel."""
    soot = g.ramp(g.noise(6, 10, 0.65), [(0.3, (0.018, 0.016, 0.014)), (0.7, (0.050, 0.042, 0.034))])
    scraps = g.ramp(g.noise(5, 12, 0.7), [(0.60, (0, 0, 0)), (0.66, (1, 1, 1))])
    paint = g.ramp(g.noise(9, 6), [(0.3, (0.20, 0.12, 0.03)), (0.7, (0.34, 0.22, 0.05))])
    col = g.mix(scraps, soot, paint)
    bare = g.ramp(g.noise(14, 10, 0.7), [(0.68, (0, 0, 0)), (0.72, (1, 1, 1))])
    col = g.mix(bare, col, (0.16, 0.15, 0.14))
    blisters = g.noise(40, 6, 0.6)
    rough = g.mix(bare, (0.9, 0.9, 0.9), (0.45, 0.45, 0.45))
    metal = g.math("MULTIPLY", bare, 0.8)
    return dict(color=col, rough=rough, metal=metal, height=blisters, depth=0.0008)


@recipe((1.0, 1.0), emissive=3.0)
def fx_fire(g):
    """The fireball: turbulent orange to white-hot. The colour map is also the emissive map."""
    heat = g.noise(3, 8, 0.65, distortion=0.6)
    col = g.ramp(heat, [(0.25, (0.45, 0.05, 0.0)), (0.45, (1.0, 0.30, 0.02)), (0.65, (1.0, 0.68, 0.20)),
                        (0.85, (1.0, 0.95, 0.80))])
    return dict(color=col, rough=1.0, metal=0.0, height=0.5, depth=0.0)


# ---------------------------------------------------------------------------------------------
# Baking


def _source_hash():
    with open(__file__, "rb") as f:
        return hashlib.sha256(f.read()).hexdigest()


def _png(path, rgb):
    """Write an 8-bit RGB PNG deterministically (no timestamps, no metadata). rgb: (h, w, 3) uint8, top row first."""
    h, w, _ = rgb.shape
    up = rgb.astype(np.int16)
    up[1:] -= rgb[:-1].astype(np.int16)  # PNG filter 2 ("Up") on every row but the first
    rows = np.empty((h, 1 + w * 3), np.uint8)
    rows[:, 0] = 2
    rows[0, 0] = 0
    rows[0, 1:] = rgb[0].reshape(-1)
    rows[1:, 1:] = (up[1:] % 256).astype(np.uint8).reshape(h - 1, -1)

    def chunk(tag, data):
        return struct.pack(">I", len(data)) + tag + data + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)

    with open(path, "wb") as f:
        f.write(b"\x89PNG\r\n\x1a\n")
        f.write(chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0)))
        f.write(chunk(b"IDAT", zlib.compress(rows.tobytes(), 9)))
        f.write(chunk(b"IEND", b""))


def _to_srgb(x):
    x = np.clip(x, 0.0, 1.0)
    return np.where(x <= 0.0031308, x * 12.92, 1.055 * np.power(x, 1 / 2.4) - 0.055)


def _u8(x):
    return np.clip(np.rint(x * 255.0), 0, 255).astype(np.uint8)


def _bake_emission(obj, mat, socket_fn, img):
    """Bake one emission pass of `mat` (whatever socket_fn wires into the emission) into `img`."""
    nt = mat.node_tree
    for nd in [nd for nd in nt.nodes if nd.get("bake_tmp")]:
        nt.nodes.remove(nd)
    em = nt.nodes.new("ShaderNodeEmission")
    em["bake_tmp"] = True
    em.inputs["Strength"].default_value = 1.0
    socket_fn(em.inputs["Color"])
    out = next(nd for nd in nt.nodes if nd.bl_idname == "ShaderNodeOutputMaterial")
    nt.links.new(em.outputs[0], out.inputs["Surface"])
    tex = nt.nodes.new("ShaderNodeTexImage")
    tex["bake_tmp"] = True
    tex.image = img
    nt.nodes.active = tex
    bpy.ops.object.bake(type="EMIT", margin=0, use_clear=True)
    buf = np.empty(SIZE * SIZE * 4, np.float32)
    img.pixels.foreach_get(buf)
    return buf.reshape(SIZE, SIZE, 4)  # row 0 is v = 0 (bottom)


def bake(name, tex_dir):
    """Bake recipe `name` into its three PNGs."""
    r = RECIPES[name]
    sc = bpy.context.scene
    prev_engine = sc.render.engine
    sc.render.engine = "CYCLES"
    sc.cycles.device = "CPU"
    sc.cycles.samples = SAMPLES
    sc.cycles.use_denoising = False
    sc.cycles.use_adaptive_sampling = False
    sc.cycles.seed = 0
    sc.render.bake.margin = 0

    bpy.ops.mesh.primitive_plane_add(size=1.0)
    obj = bpy.context.object
    obj.name = f"bake_{name}"
    mat = bpy.data.materials.new(f"bake_{name}")
    mat.use_nodes = True
    nt = mat.node_tree
    nt.nodes.clear()
    nt.nodes.new("ShaderNodeOutputMaterial")
    obj.data.materials.append(mat)
    g = G(nt, r["tile"], r["periodic_v"])
    res = r["fn"](g)

    img = bpy.data.images.new(f"bake_{name}", SIZE, SIZE, alpha=False, float_buffer=True)
    img.colorspace_settings.name = "Non-Color"

    color = _bake_emission(obj, mat, lambda s: g.put(s, res["color"]), img)[..., :3]
    data = _bake_emission(
        obj, mat, lambda s: g.put(s, g.combine(res["rough"], res["metal"], res["height"])), img)

    tu, tv = r["tile"]
    height = data[..., 2].astype(np.float64) * res["depth"]
    dx = (np.roll(height, -1, 1) - np.roll(height, 1, 1)) / (2 * tu / SIZE)
    if r["periodic_v"]:
        dy = (np.roll(height, -1, 0) - np.roll(height, 1, 0)) / (2 * tv / SIZE)
    else:
        dy = np.gradient(height, tv / SIZE, axis=0)
    nrm = np.stack([-dx, -dy, np.ones_like(dx)], -1)
    nrm /= np.linalg.norm(nrm, axis=-1, keepdims=True)

    orm = np.stack([np.ones_like(data[..., 0]), data[..., 0], data[..., 1]], -1)
    os.makedirs(tex_dir, exist_ok=True)
    _png(os.path.join(tex_dir, f"{name}_color.png"), _u8(_to_srgb(color))[::-1])
    _png(os.path.join(tex_dir, f"{name}_normal.png"), _u8(nrm * 0.5 + 0.5)[::-1])
    _png(os.path.join(tex_dir, f"{name}_orm.png"), _u8(orm)[::-1])

    bpy.data.objects.remove(obj)
    bpy.data.materials.remove(mat)
    bpy.data.images.remove(img)
    sc.render.engine = prev_engine
    print(f"BAKED {name}")


def ensure_baked(names, tex_dir, force=False):
    """Bake each recipe unless its PNGs exist from this exact version of this file."""
    stamp = _source_hash()
    for name in sorted(set(names)):
        stamp_path = os.path.join(tex_dir, f".{name}.stamp")
        files = [os.path.join(tex_dir, f"{name}_{k}.png") for k in ("color", "normal", "orm")]
        if not force and all(map(os.path.exists, files)) and os.path.exists(stamp_path):
            with open(stamp_path) as f:
                if f.read().strip() == stamp:
                    continue
        bake(name, tex_dir)
        with open(stamp_path, "w") as f:
            f.write(stamp + "\n")


# ---------------------------------------------------------------------------------------------
# Materials used by the exported models


def surface(name, tex_dir):
    """The image-textured material for recipe `name`, made once per file."""
    if name in bpy.data.materials:
        return bpy.data.materials[name]
    r = RECIPES[name]
    m = bpy.data.materials.new(name)
    m.use_nodes = True
    nt = m.node_tree
    nt.nodes.clear()
    out = nt.nodes.new("ShaderNodeOutputMaterial")
    bsdf = nt.nodes.new("ShaderNodeBsdfPrincipled")
    nt.links.new(bsdf.outputs[0], out.inputs["Surface"])

    def image(kind, colour):
        path = os.path.join(tex_dir, f"{name}_{kind}.png")
        img = bpy.data.images.load(path, check_existing=True)
        img.name = f"{name}_{kind}"
        img.colorspace_settings.name = "sRGB" if colour else "Non-Color"
        t = nt.nodes.new("ShaderNodeTexImage")
        t.image = img
        t.interpolation = "Linear"
        t.extension = "REPEAT"
        return t

    col = image("color", True)
    if r["emissive"]:
        bsdf.inputs["Base Color"].default_value = (0.02, 0.02, 0.02, 1)
        nt.links.new(col.outputs["Color"], bsdf.inputs["Emission Color"])
        bsdf.inputs["Emission Strength"].default_value = r["emissive"]
    else:
        nt.links.new(col.outputs["Color"], bsdf.inputs["Base Color"])
    orm = image("orm", False)
    sep = nt.nodes.new("ShaderNodeSeparateColor")
    nt.links.new(orm.outputs["Color"], sep.inputs[0])
    nt.links.new(sep.outputs["Green"], bsdf.inputs["Roughness"])
    nt.links.new(sep.outputs["Blue"], bsdf.inputs["Metallic"])
    nrm = image("normal", False)
    nm = nt.nodes.new("ShaderNodeNormalMap")
    nt.links.new(nrm.outputs["Color"], nm.inputs["Color"])
    nt.links.new(nm.outputs["Normal"], bsdf.inputs["Normal"])
    return m


def plain(name, colour, rough=0.6, metal=0.0, emit=None, strength=0.0):
    """An untextured material: glowing bits (tubes, LEDs) and the dead tube."""
    if name in bpy.data.materials:
        return bpy.data.materials[name]
    m = bpy.data.materials.new(name)
    m.use_nodes = True
    b = m.node_tree.nodes["Principled BSDF"]
    b.inputs["Base Color"].default_value = (*colour, 1)
    b.inputs["Roughness"].default_value = rough
    b.inputs["Metallic"].default_value = metal
    if emit:
        b.inputs["Emission Color"].default_value = (*emit, 1)
        b.inputs["Emission Strength"].default_value = strength
    return m
