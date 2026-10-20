"""The punishments' effect shapes: fireball, smoke puff, lightning bolt, ceiling arc and spark -> effects.glb.

Simple meshes the world scales, moves and fades by code (management-rooms spec, "Punishments in
motion"); every glowing one is emissive only. One glTF scene (and root node) per shape:

  fx_fireball    a lumpy ball of radius about 1 m around its origin, `fx_fire` (baked turbulence,
                 orange to white-hot)
  fx_smoke_puff  a lumpy ball of radius about 1 m, dark smoke (`fx_smoke`)
  fx_bolt        a forked, jagged bolt hanging from its origin (the ceiling) 1 m straight down
                 (glTF -Y); the world scales it along Y to reach the robot's head (`fx_bolt`)
  fx_arc         a short crackling arc, 0.6 m along X around its origin, for the ceiling (`fx_bolt`)
  fx_spark       a tiny glowing streak, 4 cm along glTF Y (`fx_spark`, warm; the world may tint it)
Run: blender -b --factory-startup --python effects.py -- <out-dir> [--rebake]
"""
import math
import os
import random
import sys

import bmesh
import bpy

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import wa_common as wc  # noqa: E402
import wa_materials as wm  # noqa: E402

out, M = wc.start(["fx_fire"])
smoke = wm.plain("fx_smoke", (0.05, 0.05, 0.05), 1.0)
bolt = wm.plain("fx_bolt", (0.75, 0.85, 1.0), 0.5, emit=(0.75, 0.85, 1.0), strength=25.0)
spark = wm.plain("fx_spark", (1.0, 0.55, 0.15), 0.5, emit=(1.0, 0.55, 0.15), strength=6.0)


def lumpy(name, mat, subdivisions, lumps, rnd):
    """A ball of radius 1 whose surface swells and dips in a few broad lumps."""
    o = wc.ico(1.0, (0, 0, 0), mat, subdivisions=subdivisions)
    waves = [(rnd.uniform(1.5, 3.5), [rnd.uniform(-1, 1) for _ in range(3)], rnd.uniform(0, math.tau))
             for _ in range(5)]
    for v in o.data.vertices:
        p = v.co.normalized()
        f = 1.0
        for k, d, ph in waves:
            f += lumps / len(waves) * math.sin(k * (p.x * d[0] + p.y * d[1] + p.z * d[2]) * 2 + ph)
        v.co = p * f
    for poly in o.data.polygons:
        poly.use_smooth = True
    o = wc.join([o], name)
    wc.uv_world(o)
    wc.part_scene(name, [o])
    return o


def zigzag(points, rnd, jitter):
    """The points with every inner one pushed sideways at random by up to `jitter` (x, y, z)."""
    return [points[0]] + [tuple(c + rnd.uniform(-j, j) for c, j in zip(p, jitter)) for p in points[1:-1]] + [points[-1]]


rnd = random.Random(7)
lumpy("fx_fireball", M["fx_fire"], 3, 0.16, rnd)
lumpy("fx_smoke_puff", smoke, 3, 0.12, rnd)

n = 12
trunk = zigzag([(0, 0, -k / n) for k in range(n + 1)], rnd, (0.09, 0.09, 0.02))
fork = zigzag([trunk[4], (0.10, 0.03, -0.50), (0.18, 0.02, -0.62), (0.24, 0.06, -0.76)], rnd, (0.03, 0.03, 0.02))
twig = zigzag([trunk[8], (-0.07, -0.02, -0.78), (-0.13, 0.0, -0.86)], rnd, (0.02, 0.02, 0.01))
bolt_parts = [wc.tube_path(trunk, 0.014, bolt, sides=6), wc.tube_path(fork, 0.009, bolt, sides=6),
              wc.tube_path(twig, 0.006, bolt, sides=6)]
o = wc.join(bolt_parts, "fx_bolt")
wc.uv_world(o)
wc.part_scene("fx_bolt", [o])

arc = zigzag([(-0.3 + 0.6 * k / 10, 0, 0) for k in range(11)], rnd, (0.02, 0.05, 0.03))
o = wc.join([wc.tube_path(arc, 0.006, bolt, sides=6)], "fx_arc")
wc.uv_world(o)
wc.part_scene("fx_arc", [o])

bm = bmesh.new()
tips = [bm.verts.new((0, 0, 0.02)), bm.verts.new((0, 0, -0.02))]
ring = [bm.verts.new((0.004 * math.cos(a), 0.004 * math.sin(a), 0)) for a in (k * math.tau / 4 for k in range(4))]
for k in range(4):
    bm.faces.new((tips[0], ring[k], ring[(k + 1) % 4]))
    bm.faces.new((tips[1], ring[(k + 1) % 4], ring[k]))
me = bpy.data.meshes.new("fx_spark")
bm.to_mesh(me)
bm.free()
o = bpy.data.objects.new("fx_spark", me)
bpy.context.scene.collection.objects.link(o)
me.materials.append(spark)
wc.uv_world(o)
wc.part_scene("fx_spark", [o])

wc.export(out, "effects")
