"""Review renders of the exported pieces, for the owner to approve (ticket 14).

Imports the .glb files from <out-dir> (so it shows what was exported, baked textures and all), lays
them out as the world would, and renders PNGs with Eevee into <renders-dir>, lit like the accepted
look: greenish fluorescent tubes, a dim ambient and, in the room shots, a dusty haze.
Run after every piece script:
  blender -b --factory-startup --python review_renders.py -- <out-dir> <renders-dir> [shot ...]
With no shot named it renders the sweatshop's shots; `cctv` renders the CCTV room's and the
punishments', kept in `.scratch/management-rooms/designs/pieces/`; `sit-in` the console with SIT IN,
kept in `.scratch/agent-screens/verification/16/`.
"""
import math
import os
import random
import sys

import bpy
import numpy as np
from mathutils import Matrix, Vector

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import wa_materials as wm  # noqa: E402

argv = sys.argv[sys.argv.index("--") + 1:]
OUT, RENDERS = os.path.abspath(argv[0]), os.path.abspath(argv[1])
WANT = argv[2:]
os.makedirs(RENDERS, exist_ok=True)

LIGHT = (0.88, 1.0, 0.78)
ROWS_X = (-3.3, -1.1, 1.1, 3.3)  # desk columns, 2.2 m apart
CLUTTER = ["crumpled_paper", "rusty_cans", "cardboard_crates", "loose_sheets", "floor_cables", "bucket",
           "broken_chair"]


# ---------------------------------------------------------------------------------------------
# Scene plumbing


def fresh():
    bpy.ops.wm.read_factory_settings(use_empty=True)
    sc = bpy.context.scene
    sc.render.engine = "BLENDER_EEVEE_NEXT"
    sc.eevee.taa_render_samples = 32
    sc.render.resolution_x, sc.render.resolution_y = 1280, 720
    sc.render.image_settings.file_format = "PNG"
    sc.render.image_settings.color_mode = "RGB"
    sc.view_settings.view_transform = "AgX"
    sc.view_settings.look = "AgX - Medium High Contrast"
    sc.world = bpy.data.worlds.new("w")
    sc.world.use_nodes = True
    bg = sc.world.node_tree.nodes["Background"]
    bg.inputs["Color"].default_value = (*LIGHT, 1)
    bg.inputs["Strength"].default_value = 0.03
    _cache.clear()
    return sc


_cache = {}


def piece(name):
    """Import <name>.glb once per scene; return its objects by name, hidden as templates."""
    if name not in _cache:
        before = set(bpy.data.objects)
        bpy.ops.import_scene.gltf(filepath=os.path.join(OUT, f"{name}.glb"))
        objs = {o.name: o for o in set(bpy.data.objects) - before}
        for o in objs.values():
            o.hide_render = True
            o.hide_viewport = True
        _cache[name] = objs
    return _cache[name]


def place(piece_name, node, loc=(0, 0, 0), rot_z=0.0, scale=1.0):
    """A copy of node `node` of a piece (with its children) at loc, turned rot_z about Z."""
    src = piece(piece_name)[node]

    def clone(o, parent):
        c = o.copy()
        bpy.context.scene.collection.objects.link(c)
        c.hide_render = c.hide_viewport = False
        c.parent = parent
        for ch in o.children:
            clone(ch, c)
        return c

    root = clone(src, None)
    root.location = loc
    root.rotation_mode = "XYZ"
    root.rotation_euler = (0, 0, rot_z)
    root.scale = (scale, scale, scale)
    return root


def area(loc, energy, size=(1.3, 0.1), colour=LIGHT):
    bpy.ops.object.light_add(type="AREA", location=loc)
    L = bpy.context.object.data
    L.shape, L.size, L.size_y = "RECTANGLE", *size
    L.energy, L.color = energy, colour
    return bpy.context.object


def tube(loc, dead=False, energy=63.0):
    place("room_kit", "room_tube_light_dead" if dead else "room_tube_light", loc)
    if not dead:
        area((loc[0], loc[1], loc[2] - 0.12), energy)


def haze(lo, hi, density=0.035):
    bpy.ops.mesh.primitive_cube_add(size=1, location=tuple((a + b) / 2 for a, b in zip(lo, hi)))
    o = bpy.context.object
    o.scale = tuple(b - a for a, b in zip(lo, hi))
    m = bpy.data.materials.new("haze")
    m.use_nodes = True
    nt = m.node_tree
    nt.nodes.remove(nt.nodes["Principled BSDF"])
    v = nt.nodes.new("ShaderNodeVolumePrincipled")
    v.inputs["Density"].default_value = density
    v.inputs["Color"].default_value = (0.8, 0.85, 0.7, 1)
    nt.links.new(v.outputs[0], nt.nodes["Material Output"].inputs["Volume"])
    o.data.materials.append(m)


def camera(eye, look, lens=24):
    bpy.ops.object.camera_add(location=eye)
    cam = bpy.context.object
    cam.data.lens = lens
    cam.data.clip_start = 0.05
    cam.rotation_euler = (Vector(look) - Vector(eye)).to_track_quat("-Z", "Y").to_euler()
    bpy.context.scene.camera = cam


def render(name):
    sc = bpy.context.scene
    sc.render.filepath = os.path.join(RENDERS, f"{name}.png")
    bpy.ops.render.render(write_still=True)
    print("RENDERED", sc.render.filepath)


# ---------------------------------------------------------------------------------------------
# Layouts


def room(rows=2, lights=True, dead=True):
    """A room interior in the room kit's frame: doorway wall, `rows` row segments, back wall, pipes, tubes."""
    place("room_kit", "room_doorway_wall")
    backdrop((-3.5, -0.6, 1.05))
    for r in range(rows):
        place("room_kit", "room_row_segment", (0, 0.2 + 2.4 * r, 0))
        place("room_kit", "room_pipes", (0, 0.2 + 2.4 * r, 0))
    place("room_kit", "room_back_wall", (0, 0.2 + 2.4 * rows, 0))
    if lights:
        for r in range(rows):
            for x in (-2.4, 2.4):
                tube((x, 1.1 + 2.4 * r, 3.0), dead=dead and r == rows - 1 and x > 0)
    return 0.2 + 2.4 * rows


def desk(x, row, depth=0, robot=True):
    seat_y = 1.6 + 2.4 * row
    place("desk_set", "desk_set", (x, seat_y, 0))
    if robot:
        place("robot", "robot", (x, seat_y, 0.46), scale=max(0.75 ** depth, 0.4))


def corridor(segments, lights=True, plain=()):
    place("corridor_kit", "corridor_end_start")
    for i in range(segments):
        place("corridor_kit", "corridor_segment_plain" if i in plain else "corridor_segment", (2.5 * i, 0, 0))
        place("corridor_kit", "corridor_far_wall", (2.5 * i, 0, 0))
        if i not in plain:
            place("sign_plate", "sign_plate", (2.5 * i + 1.25, 0, 2.45))
            backdrop((2.5 * i + 1.25, 0.6, 1.05))
        if lights:
            tube((2.5 * i + 1.25, -1.2, 3.0), energy=45.0)
    place("corridor_kit", "corridor_end_far", (2.5 * segments, 0, 0))


def backdrop(loc):
    """A dark card behind a doorway: the room seen through it is the world's portal, not a piece."""
    bpy.ops.mesh.primitive_plane_add(size=1, location=loc, rotation=(math.pi / 2, 0, 0))
    o = bpy.context.object
    o.scale = (1.6, 2.4, 1)
    o.data.materials.append(wm.plain("backdrop", (0.004, 0.005, 0.004), 1.0))


def scatter(item, loc, rot_z=0.0):
    place(f"clutter_{item}", f"clutter_{item}", loc, rot_z)


# ---------------------------------------------------------------------------------------------
# Shots


def shot_desk_set():
    fresh()
    room(2)
    desk(-1.1, 0, robot=False)
    desk(1.1, 0, robot=False)
    haze((-4.75, 0, 0), (4.75, 5.0, 3.0), 0.02)
    camera((0.15, 2.75, 1.75), (-1.1, 1.0, 0.65), 24)
    render("desk-set")


def shot_robot():
    fresh()
    room(2)
    desk(-1.1, 0)
    haze((-4.75, 0, 0), (4.75, 5.0, 3.0), 0.02)
    camera((-0.35, 0.95, 1.3), (-1.1, 1.6, 0.95), 28)
    render("robot")


def shot_room_kit():
    fresh()
    room(2)
    haze((-4.75, 0, 0), (4.75, 5.0, 3.0), 0.03)
    camera((0.6, 4.7, 1.9), (-4.4, 0.4, 1.9), 16)
    render("room-kit")


def shot_corridor_kit():
    fresh()
    corridor(4, plain=(2,))
    haze((-0.85, -2.4, 0), (10.85, 0, 3.0), 0.03)
    camera((-0.6, -2.1, 1.6), (6.0, -0.2, 1.3), 18)
    render("corridor-kit")


def shot_sign_plate():
    fresh()
    corridor(2)
    camera((0.6, -1.6, 1.6), (1.25, 0, 2.4), 32)
    render("sign-plate")


def shot_clutter(item):
    fresh()
    room(2)
    desk(3.3, 1, robot=False)
    scatter(item, (3.6, 3.0, 0), 0.4)
    haze((-4.75, 0, 0), (4.75, 5.0, 3.0), 0.02)
    eye, look = {"cardboard_crates": ((2.45, 1.75, 1.45), (3.7, 3.1, 0.35)),
                 "floor_cables": ((2.2, 1.6, 1.5), (3.6, 3.0, 0.0))}.get(item, ((2.75, 2.1, 1.05), (3.65, 3.05, 0.1)))
    camera(eye, look, 28)
    render(f"clutter-{item.replace('_', '-')}")


def shot_assembled():
    """Comparable with designs/look/look-C-brick-textured-robot-2-beside-desk.png."""
    fresh()
    depth = room(2)
    for r, row in enumerate(((0, 1, 1, 2), (0, 1))):
        for x, d in zip(ROWS_X, row):
            desk(x, r, d)
    for item, loc, rz in (("crumpled_paper", (-2.2, 0.9, 0), 0.3), ("rusty_cans", (2.4, 3.2, 0), 1.1),
                          ("cardboard_crates", (4.1, 4.5, 0), 0.0), ("loose_sheets", (-3.9, 2.4, 0), 0.7),
                          ("floor_cables", (-2.2, 2.0, 0), 0.0), ("bucket", (2.4, 0.8, 0), 0.0),
                          ("broken_chair", (-4.0, 4.5, 0), 0.6), ("crumpled_paper", (0.2, 3.3, 0), 2.0),
                          ("floor_cables", (2.2, 4.4, 0), 0.0)):
        scatter(item, loc, rz)
    haze((-4.75, 0, 0), (4.75, depth, 3.0), 0.04)
    camera((-0.05, 1.25, 1.6), (-1.6, 3.3, 0.6), 18)
    render("assembled-beside-desk")
    camera((4.3, 4.6, 2.6), (-1.6, 0.6, 0.4), 16)
    render("assembled-room")


def shot_tiling():
    """Two room row segments and corridor segments, lit flat so the joins can be judged."""
    halves = []
    for side in ("room", "corridor"):
        fresh()
        sc = bpy.context.scene
        sc.world.node_tree.nodes["Background"].inputs["Strength"].default_value = 0.25
        sc.render.resolution_x = 640
        if side == "room":
            room(2, lights=False)
            for y in (1.4, 3.8):
                area((0, y, 2.9), 300, (8, 2))
            camera((4.4, 5.0, 2.7), (-2.0, 0.6, 0.9), 16)
        else:
            corridor(3, lights=False, plain=(1,))
            for x in (1.25, 3.75, 6.25):
                area((x, -1.2, 2.9), 120, (2, 2))
            camera((-0.7, -2.2, 2.6), (5.5, -0.1, 0.8), 16)
        render(f"_tiling-{side}")
        img = bpy.data.images.load(os.path.join(RENDERS, f"_tiling-{side}.png"))
        px = np.empty(640 * 720 * 4, np.float32)
        img.pixels.foreach_get(px)
        halves.append(px.reshape(720, 640, 4)[..., :3])
        os.remove(os.path.join(RENDERS, f"_tiling-{side}.png"))
    both = np.concatenate(halves, 1)
    both[:, 638:642] = 0.0
    # pixels from a loaded PNG are already display-encoded values
    wm._png(os.path.join(RENDERS, "tiling.png"), wm._u8(both)[::-1])
    print("RENDERED tiling.png")


# ---------------------------------------------------------------------------------------------
# The CCTV room and the punishments, compared with the accepted look on
# branch `prototype/cctv-look` (`.scratch/management-rooms/designs/look` and `.../punishments`).
# Render them into `.scratch/management-rooms/designs/pieces/`. What the world draws by code (the feed
# images and their overlay, the text) is stood in for here: the feeds show the sweatshop's review
# renders, tinted and scanned like the accepted look, and the words are Blender text in Oswald. No
# bloom: the world has none, so glow is the emissive colour alone.

FEEDS = os.path.normpath(os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", ".scratch",
                                      "sweatshop-rooms", "designs", "pieces"))
CYAN, TINT = (0.0, 0.85, 1.0), (0.55, 0.9, 1.0)
WAITING, ASKING = (1.0, 0.65, 0.05), (1.0, 0.06, 0.08)
COLUMN, FEED_PITCH, BOTTOM_ROW, CONSOLE_Y = 4.0 / 3.0, 0.80, 1.12, 4.4
FOCUS_Z = BOTTOM_ROW + 2 * FEED_PITCH + 0.711 / 2 + 0.08 + 0.915 / 2
TICKER_Z = BOTTOM_ROW - 0.711 / 2 - 0.10 - 0.07


def find(root, base):
    """The clone of node `base` under `root` (clones are named `base`, `base.001`, ...)."""
    stack = [root]
    while stack:
        o = stack.pop()
        if o.name.split(".")[0] == base:
            return o
        stack.extend(o.children)
    raise KeyError(base)


def glow_mat(name, colour, strength):
    m = bpy.data.materials.new(name)
    m.use_nodes = True
    nt = m.node_tree
    nt.nodes.clear()
    e = nt.nodes.new("ShaderNodeEmission")
    e.inputs["Color"].default_value = (*colour, 1)
    e.inputs["Strength"].default_value = strength
    nt.links.new(e.outputs[0], nt.nodes.new("ShaderNodeOutputMaterial").inputs[0])
    return m


def recolour(o, slot_name, colour, strength=None):
    """Give `o` its own copy of material `slot_name`, glowing in `colour` (the world's tint)."""
    o.data = o.data.copy()
    for i, m in enumerate(o.data.materials):
        if m and m.name.split(".")[0] == slot_name:
            c = m.copy()
            b = c.node_tree.nodes["Principled BSDF"]
            b.inputs["Base Color"].default_value = (*colour, 1)
            b.inputs["Emission Color"].default_value = (*colour, 1)
            if strength is not None:
                b.inputs["Emission Strength"].default_value = strength
            o.data.materials[i] = c


def feed_mat(image, flag=None, mono=True):
    """A camera feed as the accepted look shows it: the still, cyan monochrome, scanlines, grain."""
    m = bpy.data.materials.new("feed")
    m.use_nodes = True
    nt = m.node_tree
    nt.nodes.clear()
    N = nt.nodes.new
    tc = N("ShaderNodeTexCoord")
    img = N("ShaderNodeTexImage")
    path = os.path.join(FEEDS, image + ".png")
    img.image = bpy.data.images.load(path, check_existing=True) if os.path.exists(path) else None
    nt.links.new(tc.outputs["UV"], img.inputs["Vector"])
    bw = N("ShaderNodeRGBToBW")
    nt.links.new(img.outputs["Color"], bw.inputs[0])
    mul = N("ShaderNodeMix")
    mul.data_type, mul.blend_type = "RGBA", "MULTIPLY"
    mul.inputs["Factor"].default_value = 1.0
    nt.links.new(bw.outputs[0], mul.inputs["A"])
    mul.inputs["B"].default_value = (*[c * 1.6 for c in (flag or TINT)], 1)
    col = mul.outputs["Result"]
    sep = N("ShaderNodeSeparateXYZ")
    nt.links.new(tc.outputs["UV"], sep.inputs[0])
    sl = N("ShaderNodeMath")
    sl.operation = "MULTIPLY"
    sl.inputs[1].default_value = 240.0
    nt.links.new(sep.outputs["Y"], sl.inputs[0])
    sn = N("ShaderNodeMath")
    sn.operation = "SINE"
    nt.links.new(sl.outputs[0], sn.inputs[0])
    sm = N("ShaderNodeMapRange")
    sm.inputs["From Min"].default_value = -1
    sm.inputs["To Min"].default_value = 0.75
    nt.links.new(sn.outputs[0], sm.inputs["Value"])
    lines = N("ShaderNodeMix")
    lines.data_type, lines.blend_type = "RGBA", "MULTIPLY"
    lines.inputs["Factor"].default_value = 1.0
    nt.links.new(col, lines.inputs["A"])
    nt.links.new(sm.outputs[0], lines.inputs["B"])
    col = lines.outputs["Result"]
    if flag:
        wash = N("ShaderNodeMix")
        wash.data_type, wash.blend_type = "RGBA", "SCREEN"
        wash.inputs["Factor"].default_value = 0.18
        nt.links.new(col, wash.inputs["A"])
        wash.inputs["B"].default_value = (*flag, 1)
        col = wash.outputs["Result"]
    e = N("ShaderNodeEmission")
    e.inputs["Strength"].default_value = 1.4
    nt.links.new(col, e.inputs["Color"])
    nt.links.new(e.outputs[0], N("ShaderNodeOutputMaterial").inputs[0])
    return m


def text(body, parent, loc, size, colour, align="LEFT", strength=4.0, rot=(math.pi / 2, 0, 0)):
    """Emissive Oswald text, a child of `parent` in its local frame (the default faces -Y)."""
    path = os.path.join(OUT, "fonts", "oswald.ttf")
    bpy.ops.object.text_add(location=loc, rotation=rot)
    t = bpy.context.object
    t.data.body = body
    t.data.size = size
    if os.path.exists(path):
        t.data.font = bpy.data.fonts.load(path, check_existing=True)
    t.data.align_x, t.data.align_y = align, "CENTER"
    t.data.materials.append(glow_mat("ink", colour, strength))
    t.parent = parent
    return t


def screen_image(monitor_root, screen_node, image, flag=None):
    s = find(monitor_root, screen_node)
    s.data = s.data.copy()
    s.data.materials[0] = feed_mat(image, flag)
    return s


def feed(loc, yaw, image, label, cam, flag=None):
    """A wall feed monitor with its overlay (and flag frame) at `loc`, turned `yaw` about Z."""
    m = place("cctv_monitors", "cctv_feed_monitor", loc, yaw)
    screen_image(m, "cctv_feed_screen", image, flag and (WAITING if flag == "waiting" else ASKING))
    y = -0.075
    text(f"CAM {cam:02d}", m, (-0.56, y, 0.29), 0.045, TINT)
    text(label.upper(), m, (-0.56, y, -0.29), 0.05, TINT)
    bpy.ops.mesh.primitive_cylinder_add(radius=0.013, depth=0.002, location=(0.55, y, 0.29),
                                        rotation=(math.pi / 2, 0, 0))
    dot = bpy.context.object  # the recording dot
    dot.data.materials.append(glow_mat("rec", (1.0, 0.05, 0.05), 8))
    dot.parent = m
    if flag:
        f = place("cctv_monitors", "cctv_feed_flag", loc, yaw)
        colour = WAITING if flag == "waiting" else ASKING
        recolour(f, "cctv_flag", colour)
        anchor = find(f, "cctv_feed_flag_text")
        text("WAITING ON YOU" if flag == "waiting" else "ASKING YOU SOMETHING", f,
             (anchor.location.x, anchor.location.y - 0.002, anchor.location.z), 0.04, (0.05, 0.03, 0.0),
             align="CENTER", strength=0.0)
    return m


def focus(loc, image, flag, who, status, label, cam):
    m = place("cctv_monitors", "cctv_focus_monitor", loc)
    colour = WAITING if flag == "waiting" else ASKING
    screen_image(m, "cctv_focus_screen", image, colour)
    y = -0.075
    text(f"CAM {cam:02d}", m, (-0.72, y, 0.37), 0.055, TINT)
    text(who, m, (-0.72, y, -0.30), 0.06, TINT)
    text(label.upper(), m, (-0.72, y, -0.37), 0.065, TINT)
    text(status, m, (0.72, y, -0.30), 0.06, colour, align="RIGHT", strength=6)
    for sx in (-1, 1):  # the targeting brackets, drawn by the world over the focus image
        for sz in (-1, 1):
            arm, c = 0.08, (0.05 + sx * 0.16, 0.10 + sz * 0.16)
            bpy.ops.mesh.primitive_cube_add(size=1, location=(c[0] - sx * arm / 2, y, c[1]))
            h = bpy.context.object
            h.scale = (arm, 0.004, 0.012)
            h.data.materials.append(glow_mat("bracket", colour, 6))
            h.parent = m
            bpy.ops.mesh.primitive_cube_add(size=1, location=(c[0], y, c[1] - sz * arm / 2))
            v = bpy.context.object
            v.scale = (0.012, 0.004, arm)
            v.data.materials.append(glow_mat("bracket", colour, 6))
            v.parent = m
    f = place("cctv_monitors", "cctv_focus_flag", loc)
    recolour(f, "cctv_flag", colour)
    anchor = find(f, "cctv_focus_flag_text")
    text("WAITING ON YOU" if flag == "waiting" else "ASKING YOU SOMETHING", f,
         (anchor.location.x, anchor.location.y - 0.002, anchor.location.z), 0.05, (0.05, 0.03, 0.0),
         align="CENTER", strength=0.0)


CCTV_FEEDS = [("AgentGamified", "assembled-room", None), ("lighthouse", "corridor-kit", None),
              ("orchard", "desk-set", "waiting"), ("api-gateway", "robot", None),
              ("dotfiles", "room-kit", None), ("kestrel", "assembled-beside-desk", "asking"),
              ("thesis", "clutter-cardboard-crates", None), ("ledger", "desk-set", None)]


def cctv_room(columns=4, lights=True):
    """The kit assembled for `columns` feed columns (4 up to 12 rooms, one more per 3 rooms past it)."""
    extra = columns - 4
    for part in ("cctv_doorway_wall", "cctv_floor", "cctv_ceiling", "cctv_back_wall"):
        place("cctv_room_kit", part)
    for k in range(extra):
        place("cctv_room_kit", "cctv_extension", (4 + k * 2 / 3, 0, 0))
        place("cctv_room_kit", "cctv_extension", (-4 - (k + 1) * 2 / 3, 0, 0))
    place("cctv_room_kit", "cctv_side_wall_left", (-extra * 2 / 3, 0, 0))
    place("cctv_room_kit", "cctv_side_wall_right", (extra * 2 / 3, 0, 0))
    backdrop((0, -0.6, 1.05))
    if lights:  # a few dim cool downlights and the screens' glow, as the accepted look lights it
        for x, y in ((-2.0, 2.8), (2.0, 2.8), (0.0, 1.05)):
            bpy.ops.object.light_add(type="SPOT", location=(x, y, 4.1))
            lo = bpy.context.object
            lo.data.energy, lo.data.color, lo.data.spot_size = 250, (0.6, 0.85, 1.0), 0.9
        lo = area((0, 6.2, 1.8), 120, (5.5, 2.5), TINT)
        lo.rotation_euler = (-math.pi / 2, 0, 0)
    sc = bpy.context.scene
    sc.world.node_tree.nodes["Background"].inputs["Color"].default_value = (0.6, 0.85, 1.0, 1)
    sc.world.node_tree.nodes["Background"].inputs["Strength"].default_value = 0.01
    sc.eevee.use_raytracing = True


def monitor_wall(feeds, columns=4):
    """The feeds hang from the top row down (under the focus monitors), the ticker under the last row."""
    rows = max(1, min(3, math.ceil(len(feeds) / columns)))
    top = BOTTOM_ROW + 2 * FEED_PITCH
    for i, (label, image, flag) in enumerate(feeds):
        r, c = divmod(i, columns)
        off = c - (columns - 1) / 2
        x = off * COLUMN
        yaw = -off * 0.12
        y = 7.0 - abs(off) ** 2 * 0.09
        z = top - r * FEED_PITCH
        feed((x, y, z), yaw, image, label, i + 1, flag)
    focus((-0.94, 7.0, FOCUS_Z), "robot", "waiting", "OPUS-5.5 / root", "WAITING ON YOU  04:12", "orchard", 3)
    focus((0.94, 7.0, FOCUS_Z), "assembled-beside-desk", "asking", "SONNET-5.5 / root",
          "ASKING YOU SOMETHING  00:41", "kestrel", 6)
    t = place("cctv_monitors", "cctv_ticker", (0, 7.0, TICKER_Z + (3 - rows) * FEED_PITCH))
    text("ORCHARD :: OPUS-5.5 :: WAITING ON YOU 04:12   ///   KESTREL :: SONNET-5.5 :: ASKING YOU SOMETHING 00:41",
         t, (0, -0.055, 0), 0.06, TINT, align="CENTER")


def console(cover_open=False):
    root = place("cctv_console", "cctv_console", (0, CONSOLE_Y, 0))
    glass = find(root, "console_screen")
    text("SELECT TARGET", glass, (-0.58, 0.27, 0.005), 0.045, TINT, rot=(0, 0, 0))
    for k, (row, colour) in enumerate((("ORCHARD   OPUS-5.5   WAITING ON YOU   04:12", WAITING),
                                       ("KESTREL   SONNET-5.5   ASKING YOU SOMETHING   00:41", ASKING))):
        y = 0.15 - k * 0.11
        if k == 0:  # the selected row, highlighted
            bpy.ops.mesh.primitive_cube_add(size=1, location=(0, y, 0.0045))
            b = bpy.context.object
            b.scale = (1.18, 0.085, 0.001)
            b.data.materials.append(glow_mat("row", colour, 3))
            b.parent = glass
            text(row, glass, (-0.56, y, 0.0062), 0.04, (0.03, 0.02, 0.0), rot=(0, 0, 0), strength=0.0)
        else:
            text(row, glass, (-0.56, y, 0.0062), 0.04, colour, rot=(0, 0, 0), strength=4)
    for node, word, colour in (("console_sit_in_label", "SIT IN", CYAN),
                               ("console_kill_label", "KILL", (1.0, 0.06, 0.04)),
                               ("console_work_harder_label", "WORK HARDER", (1.0, 0.85, 0.1))):
        label = find(root, node)
        text(word, label, (0, 0, 0), 0.05, colour, align="CENTER", rot=(0, 0, 0), strength=5)
    if cover_open:
        cover = find(root, "console_cover")
        cover.rotation_mode = "XYZ"
        cover.rotation_euler.x -= 1.9
    return root


def shot_cctv_room_kit():
    fresh()
    cctv_room()
    camera((0, 0.3, 1.7), (0, 7, 2.0), 16)
    render("cctv-room-kit")


def shot_cctv_assembled():
    fresh()
    cctv_room()
    monitor_wall(CCTV_FEEDS)
    console()
    camera((0, 0.3, 1.7), (0, 7, 2.0), 16)
    render("cctv-assembled-doorway")  # compare: designs/look/cctv-final-accepted-1-doorway.png
    camera((0, 3.4, 2.3), (0, 7, 2.5), 20)
    render("cctv-assembled-focus-wall")  # compare: cctv-final-accepted-2-focus-wall.png


def shot_cctv_widened():
    """Sixteen rooms: six columns of feeds, the room wider by two half-columns on each side."""
    fresh()
    cctv_room(columns=6)
    feeds = [CCTV_FEEDS[i % len(CCTV_FEEDS)] for i in range(16)]
    monitor_wall(feeds, columns=6)
    console()
    camera((3.9, 0.4, 3.6), (-0.8, 6.5, 1.5), 15)
    render("cctv-widened-16-rooms")


def shot_cctv_console():
    fresh()
    cctv_room()
    monitor_wall(CCTV_FEEDS)
    console()
    camera((0.2, CONSOLE_Y - 1.1, 1.55), (0.2, CONSOLE_Y + 0.1, 0.85), 16)
    render("cctv-console")  # compare: cctv-final-accepted-3-console.png
    fresh()
    cctv_room()
    monitor_wall(CCTV_FEEDS)
    console(cover_open=True)
    camera((0.75, CONSOLE_Y - 0.75, 1.35), (0.6, CONSOLE_Y, 0.85), 24)
    render("cctv-console-cover-open")


def shot_cctv_monitors():
    """The screens on their own: a plain feed, a waiting feed, an asking focus monitor, the ticker."""
    fresh()
    bpy.ops.mesh.primitive_plane_add(size=30, location=(0, 0.3, 0), rotation=(math.pi / 2, 0, 0))
    bpy.context.object.data.materials.append(wm.plain("studio", (0.012, 0.014, 0.018), 0.4))
    feed((-1.95, 0, 2.15), 0, "assembled-room", "agentgamified", 1)
    feed((-0.65, 0, 2.15), 0, "desk-set", "orchard", 3, "waiting")
    focus((1.25, 0, 2.25), "robot", "asking", "SONNET-5.5 / root", "ASKING YOU SOMETHING  00:41", "kestrel", 6)
    t = place("cctv_monitors", "cctv_ticker", (0, 0, 1.35))
    text("ORCHARD :: OPUS-5.5 :: WAITING ON YOU 04:12   ///   KESTREL :: SONNET-5.5 :: ASKING 00:41",
         t, (0, -0.055, 0), 0.06, TINT, align="CENTER")
    lo = area((0.3, -3, 3.2), 300, (4, 2), (0.8, 0.9, 1.0))
    lo.rotation_euler = (math.radians(60), 0, 0)
    bpy.context.scene.world.node_tree.nodes["Background"].inputs["Strength"].default_value = 0.02
    camera((0.6, -4.4, 2.4), (0.1, 0, 1.95), 30)
    render("cctv-monitors")


def shot_cctv_sign_plate():
    fresh()
    place("corridor_kit", "corridor_end_start")
    for i in range(2):
        place("corridor_kit", "corridor_segment", (2.5 * i, 0, 0))
        place("corridor_kit", "corridor_far_wall", (2.5 * i, 0, 0))
        backdrop((2.5 * i + 1.25, 0.6, 1.05))
        tube((2.5 * i + 1.25, -1.2, 3.0), energy=45.0)
    place("sign_plate", "sign_plate", (1.25, 0, 2.45))
    plate = place("cctv_sign_plate", "cctv_sign_plate", (3.75, 0, 2.45))
    anchor = find(plate, "cctv_sign_text")
    text("CCTV", plate, (0, anchor.location.y - 0.002, 0), 0.17, CYAN, align="CENTER", strength=2)
    place("corridor_kit", "corridor_end_far", (5.0, 0, 0))
    camera((2.4, -1.9, 1.7), (3.5, 0, 2.35), 28)
    render("cctv-sign-plate")


def project_room(target=(-1.1, 1), robot_at_target=True):
    """A sweatshop room of three rows of four desks with robots, lit like its review renders."""
    depth = room(3)
    for r in range(3):
        for x in ROWS_X:
            desk(x, r, robot=robot_at_target or (x, r) != target)
    haze((-4.75, 0, 0), (4.75, depth, 3.0), 0.02)
    return depth


def wall_cam(pan=-0.5):
    cam = place("wall_camera", "wall_camera", (-3.5, 0, 2.55))
    head = find(cam, "wall_camera_head")
    head.rotation_mode = "XYZ"
    head.rotation_euler = (-0.45, 0, pan)
    return cam


def shot_wall_camera():
    fresh()
    project_room()
    cam = wall_cam()
    camera((-2.7, 1.25, 2.05), (-3.5, 0, 2.45), 30)
    render("wall-camera")
    bpy.context.view_layer.update()
    lens = find(cam, "wall_camera_lens")
    bpy.ops.object.camera_add()
    c = bpy.context.object
    c.matrix_world = lens.matrix_world @ Matrix.Rotation(math.pi / 2, 4, "X")
    c.data.lens = 13
    c.data.clip_start = 0.05
    bpy.context.scene.camera = c
    render("wall-camera-feed")  # compare: designs/punishments/strike-A-1-breach-feed.png


TARGET = (-1.1, 1)


def seat(target=TARGET):
    return target[0], 1.6 + 2.4 * target[1]


def squad(loc, yaw, pose):
    s = place("squad_robot", "squad_robot", loc, yaw)

    def turn(node, x):
        o = find(s, f"squad_{node}")
        o.rotation_mode = "XYZ"
        o.rotation_euler.x = x
        return o

    if pose == "point":
        turn("arm_upper_r", -1.5)
        turn("arm_upper_l", 0.15)
        turn("arm_lower_l", -0.5)
    elif pose == "crouch":
        p = find(s, "squad_pelvis")
        p.location.z -= 0.36
        turn("torso", 0.35)
        for side in ("l", "r"):
            turn(f"leg_upper_{side}", -1.45 if side == "r" else -0.6)
            turn(f"leg_lower_{side}", 2.2 if side == "r" else 1.2)
            turn(f"arm_upper_{side}", -1.1)
            turn(f"arm_lower_{side}", -0.6)
    elif pose == "run":
        turn("torso", 0.2)
        for side, k in (("l", 1), ("r", -1)):
            turn(f"leg_upper_{side}", -0.7 * k)
            turn(f"leg_lower_{side}", 0.9 if k < 0 else 0.3)
            turn(f"arm_upper_{side}", 0.6 * k)
            turn(f"arm_lower_{side}", -0.9)
    return s


def shot_squad_robot():
    fresh()
    project_room()
    tx, ty = seat()
    squad((tx - 0.8, ty - 0.05, 0), math.pi / 2, "point")
    squad((tx + 0.85, ty + 0.45, 0), -1.08, "point")
    squad((tx + 0.62, ty - 0.12, 0), -math.pi / 2, "crouch")
    squad((tx - 2.6, ty - 1.35, 0), math.pi / 2, "run")
    place("squad_robot", "squad_charge", (tx, ty - 0.075, 0.80))
    point = bpy.data.lights.new("charge", "POINT")
    point.energy, point.color = 2, (1, 0.1, 0.05)
    lo = bpy.data.objects.new("charge", point)
    lo.location = (tx + 0.03, ty - 0.15, 0.83)
    bpy.context.scene.collection.objects.link(lo)
    camera((tx + 1.6, ty - 1.7, 1.55), (tx - 0.1, ty, 0.95), 22)
    render("squad-robot")  # compare: designs/punishments/strike-A-3-surround-close.png
    camera((tx + 2.2, ty - 2.6, 2.4), (tx - 0.6, ty + 0.2, 0.7), 18)
    render("squad-robot-room")


def point_light(loc, energy, colour, soft=0.3):
    bpy.ops.object.light_add(type="POINT", location=loc)
    lo = bpy.context.object
    lo.data.energy, lo.data.color, lo.data.shadow_soft_size = energy, colour, soft
    return lo


def hide_desk_chair(target=TARGET):
    """The wreck's chair replaces the desk set's own."""
    tx, ty = seat(target)
    for o in bpy.data.objects:
        if o.name.split(".")[0] == "chair" and not o.hide_render and abs(o.matrix_world.translation.x - tx) < 0.01 \
                and abs(o.matrix_world.translation.y - ty) < 0.01:
            o.hide_render = True


def shot_wreck():
    fresh()
    project_room(robot_at_target=False)
    bpy.context.view_layer.update()
    hide_desk_chair()
    tx, ty = seat()
    rnd = random.Random(3)
    place("wreck_parts", "scorch_floor", (tx, ty - 0.2, 0))
    place("wreck_parts", "scorch_desk", (tx, ty - 0.57, 0.762), 0.3)
    for name, (dx, dy), rz in (("wreck_torso", (0.35, 0.35), 0.4), ("wreck_head", (-0.65, 0.45), 2.0),
                               ("wreck_arm", (0.75, -0.2), 1.0), ("wreck_arm", (-0.5, -0.05), 3.6),
                               ("wreck_leg", (0.1, 0.85), 0.2), ("wreck_leg", (-0.95, 0.15), 4.4),
                               ("wreck_chair", (0.45, 0.95), 0.5)):
        place("wreck_parts", name, (tx + dx, ty + dy, 0), rz)
    for k in range(9):
        a, d = rnd.uniform(0, math.tau), rnd.uniform(0.5, 1.1)
        place("wreck_parts", "wreck_debris", (tx + math.cos(a) * d, ty + math.sin(a) * d, 0.006),
              rnd.uniform(0, 6))
    for k in range(14):
        place("effects", "fx_spark",
              (tx + rnd.uniform(-0.35, 0.4), ty + rnd.uniform(-0.1, 0.5), rnd.uniform(0.05, 0.5)),
              rnd.uniform(0, 3), scale=0.8)
    for k in range(5):
        place("effects", "fx_smoke_puff", (tx + 0.25 + 0.03 * k, ty + 0.3, 0.3 + k * 0.17), k, scale=0.05 + 0.025 * k)
    point_light((tx + 0.2, ty + 0.3, 0.3), 25, (1.0, 0.4, 0.1))
    camera((tx + 1.4, ty + 1.9, 1.6), (tx, ty - 0.2, 0.5), 22)
    render("wreck")  # compare: designs/punishments/strike-A-6-wreck-close.png
    camera((tx + 1.7, ty - 1.9, 1.9), (tx, ty + 0.3, 0.2), 20)
    render("wreck-front")


def shot_effects():
    fresh()
    project_room(robot_at_target=False)
    tx, ty = seat()
    place("effects", "fx_fireball", (tx, ty, 0.9), 0.0, scale=0.9)
    place("effects", "fx_fireball", (tx + 0.6, ty - 0.3, 1.5), 1.3, scale=0.55)
    for k in range(6):
        a = k * math.pi / 3
        place("effects", "fx_smoke_puff", (tx + math.cos(a) * 0.9, ty + math.sin(a) * 0.9, 1.9), a, scale=0.45)
    rnd = random.Random(1)
    for k in range(16):
        a, d = rnd.uniform(0, math.tau), rnd.uniform(1.0, 2.0)
        o = place("wreck_parts", "wreck_debris", (tx + math.cos(a) * d, ty + math.sin(a) * d, rnd.uniform(0.4, 2.4)),
                  rnd.uniform(0, 6), scale=1.4)
        o.rotation_euler.x = rnd.uniform(0, 3)
    point_light((tx, ty, 1.0), 4000, (1.0, 0.55, 0.2))
    camera((tx + 3.0, ty + 2.6, 2.0), (tx, ty, 1.0), 24)
    render("effects-boom")  # compare: designs/punishments/strike-A-5-boom-feed.png and -close

    fresh()
    project_room()
    head = (tx, ty, 0.46 + 0.64)
    b = place("effects", "fx_bolt", (tx, ty, 3.0))
    b.scale = (1, 1, 3.0 - head[2])
    for k in range(4):
        a = k * 1.4
        place("effects", "fx_arc", (tx + math.cos(a) * 0.3, ty + math.sin(a) * 0.3, 2.97), a)
    for k in range(10):
        s = place("effects", "fx_spark", (head[0] + rnd.uniform(-0.25, 0.25), head[1] + rnd.uniform(-0.25, 0.25),
                                          head[2] + rnd.uniform(-0.4, 0.1)), rnd.uniform(0, 3))
        recolour(s, "fx_spark", (0.7, 0.85, 1.0))
    point_light((head[0], head[1] - 0.3, head[2] + 0.3), 2500, (0.7, 0.8, 1.0))
    camera((tx + 1.4, ty + 1.9, 1.6), (tx, ty - 0.2, 1.3), 22)
    render("effects-bolt")  # compare: designs/punishments/harder-2-strike-close.png


def shot_effect_shapes():
    """Every effect shape on its own, on a dark card, at its own size."""
    fresh()
    bpy.ops.mesh.primitive_plane_add(size=30, location=(0, 1.5, 0), rotation=(math.pi / 2, 0, 0))
    bpy.context.object.data.materials.append(wm.plain("studio", (0.05, 0.05, 0.055), 0.6))
    bpy.ops.mesh.primitive_plane_add(size=30, location=(0, 0, 0))
    bpy.context.object.data.materials.append(wm.plain("studio", (0.05, 0.05, 0.055), 0.6))
    place("effects", "fx_fireball", (-2.3, 0, 1.0), 0.0)
    place("effects", "fx_smoke_puff", (0.1, 0, 1.0), 0.0)
    place("effects", "fx_bolt", (1.7, 0, 1.9))
    place("effects", "fx_arc", (1.7, 0, 0.6))
    for k in range(5):
        place("effects", "fx_spark", (2.6 + 0.1 * k, 0, 0.6 + 0.12 * k), 0.0, scale=3.0)
    lo = area((0, -3, 3), 250, (5, 3), (1, 1, 1))
    lo.rotation_euler = (math.radians(50), 0, 0)
    camera((0.1, -6.2, 1.5), (0.1, 0, 1.05), 30)
    render("effects-shapes")


def shot_cctv_sheet():
    """All the review renders above on one contact sheet, three across."""
    names = ["cctv-assembled-doorway", "cctv-assembled-focus-wall", "cctv-console", "cctv-console-cover-open",
             "cctv-room-kit", "cctv-widened-16-rooms", "cctv-monitors", "cctv-sign-plate", "wall-camera",
             "wall-camera-feed", "squad-robot", "squad-robot-room", "wreck", "wreck-front", "effects-boom",
             "effects-bolt", "effects-shapes"]
    w, h, across = 640, 360, 3
    rows = math.ceil(len(names) / across)
    sheet = np.zeros((rows * h, across * w, 3), np.float32)
    for i, n in enumerate(names):
        path = os.path.join(RENDERS, f"{n}.png")
        if not os.path.exists(path):
            continue
        img = bpy.data.images.load(path)
        img.colorspace_settings.name = "Non-Color"  # the stored values, not converted to linear
        W, H = img.size
        px = np.empty(W * H * 4, np.float32)
        img.pixels.foreach_get(px)
        px = px.reshape(H, W, 4)[..., :3]
        px = px.reshape(h, H // h, w, W // w, 3).mean(axis=(1, 3))  # halve by averaging, no banding
        r, c = divmod(i, across)
        sheet[(rows - 1 - r) * h:(rows - r) * h, c * w:(c + 1) * w] = px
    wm._png(os.path.join(RENDERS, "sheet.png"), wm._u8(sheet)[::-1])
    print("RENDERED sheet.png")


# ---------------------------------------------------------------------------------------------
# The PC (agent-screens ticket 09): the desk set's flat screen, compared with the accepted look on
# branch `prototype/agent-screens-look` (`.scratch/agent-screens/designs/seated-B.png` and
# `room-B.png`). Render them into `.scratch/agent-screens/verification/09/`. The session terminal the
# world paints is stood in for by the designs' terminal image on the glass's own UVs; a plain PC shows
# the exported `pc_screen` glow.

TERMINAL = os.path.normpath(os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", ".scratch",
                                         "agent-screens", "designs", "pc-texture-600x360.png"))
SEATED_W, SEATED_H = 1152, 746  # the world view in a 1440 x 810 window, as the designs frame it


def terminal_mat(look):
    """live: the terminal image, emissive; frozen: its last frame, greyed and dimmed (spec: "Three looks")."""
    name = f"terminal_{look}"
    if name in bpy.data.materials:
        return bpy.data.materials[name]
    m = bpy.data.materials.new(name)
    m.use_nodes = True
    nt = m.node_tree
    nt.nodes.clear()
    tex = nt.nodes.new("ShaderNodeTexImage")
    tex.image = bpy.data.images.load(TERMINAL, check_existing=True)
    col = tex.outputs["Color"]
    if look == "frozen":
        hs = nt.nodes.new("ShaderNodeHueSaturation")
        hs.inputs["Saturation"].default_value = 0.15
        hs.inputs["Value"].default_value = 0.45
        nt.links.new(col, hs.inputs["Color"])
        col = hs.outputs["Color"]
    e = nt.nodes.new("ShaderNodeEmission")
    e.inputs["Strength"].default_value = 1.6
    nt.links.new(col, e.inputs["Color"])
    nt.links.new(e.outputs[0], nt.nodes.new("ShaderNodeOutputMaterial").inputs[0])
    return m


def pc_desk(x, row, look, robot=True):
    """A desk set whose PC is live, frozen or plain; returns its `pc` object."""
    seat_y = 1.6 + 2.4 * row
    pc = find(place("desk_set", "desk_set", (x, seat_y, 0)), "pc")
    if look != "plain":
        for slot in pc.material_slots:
            if slot.material and slot.material.name.split(".")[0] == "pc_screen":
                slot.link = "OBJECT"
                slot.material = terminal_mat(look)
    if robot:
        place("robot", "robot", (x, seat_y, 0.46))
    return pc


def glass_of(pc):
    """The glass's centre, outward normal and width in the world, from the exported pc_screen face."""
    bpy.context.view_layer.update()
    me, mw = pc.data, pc.matrix_world
    slot = next(i for i, m in enumerate(me.materials) if m and m.name.split(".")[0] == "pc_screen")
    faces = [p for p in me.polygons if p.material_index == slot]  # the quad, as imported: two triangles
    corners = [mw @ me.vertices[v].co for v in {v for p in faces for v in p.vertices}]
    centre = sum(corners, Vector()) / len(corners)
    normal = (mw.to_3x3() @ faces[0].normal).normalized()
    return centre, normal, max((a - b).length for a in corners for b in corners if abs((a - b).z) < 0.01)


def pc_room(looks, hide=None):
    depth = room(3)
    pcs = {}
    for r in range(3):
        for x in ROWS_X:
            pcs[(x, r)] = pc_desk(x, r, looks.get((x, r), "frozen"), robot=(x, r) != hide)
    haze((-4.75, 0, 0), (4.75, depth, 3.0), 0.02)
    return pcs


def shot_pc_seated():
    """From the target robot's eyes (its robot hidden), squarely at the glass filling 72 % of the view."""
    sc = fresh()
    sc.render.resolution_x, sc.render.resolution_y = SEATED_W, SEATED_H
    pcs = pc_room({TARGET: "live", (1.1, 1): "live", (-3.3, 1): "plain"}, hide=TARGET)
    centre, normal, width = glass_of(pcs[TARGET])
    d = 0.5
    camera(tuple(centre + normal * d), tuple(centre))
    cam = bpy.context.scene.camera
    cam.data.sensor_fit = "HORIZONTAL"
    cam.data.lens = 0.72 * cam.data.sensor_width * d / width
    render("pc-seated")


def shot_pc_room():
    sc = fresh()
    sc.render.resolution_x, sc.render.resolution_y = SEATED_W, SEATED_H
    pc_room({(-1.1, 2): "live", (1.1, 2): "live", (3.3, 1): "live",
             (-3.3, 2): "plain", (3.3, 0): "plain", (-1.1, 0): "plain"})
    camera((2.1, 7.15, 1.6), (-0.9, 2.2, 0.95), 22)
    render("pc-room")


def shot_pc_close():
    """The desk set on its own, three-quarter from the front and from behind, every PC plain."""
    for name, eye in (("pc-close-front", (-0.4, 2.4, 1.45)), ("pc-close-back", (-0.4, 0.15, 1.5))):
        fresh()
        room(2)
        pc_desk(-1.1, 0, "plain", robot=False)
        haze((-4.75, 0, 0), (4.75, 5.0, 3.0), 0.02)
        camera(eye, (-1.1, 1.0, 0.95), 30)
        render(name)


PC_SHOTS = {"pc-seated": shot_pc_seated, "pc-room": shot_pc_room, "pc-close": shot_pc_close}


CCTV_SHOTS = {"cctv-room-kit": shot_cctv_room_kit, "cctv-assembled": shot_cctv_assembled,
              "cctv-widened": shot_cctv_widened, "cctv-console": shot_cctv_console,
              "cctv-monitors": shot_cctv_monitors, "cctv-sign-plate": shot_cctv_sign_plate,
              "wall-camera": shot_wall_camera, "squad-robot": shot_squad_robot, "wreck": shot_wreck,
              "effects": shot_effects, "effect-shapes": shot_effect_shapes, "cctv-sheet": shot_cctv_sheet}


SHOTS = {"desk-set": shot_desk_set, "robot": shot_robot, "room-kit": shot_room_kit,
         "corridor-kit": shot_corridor_kit, "sign-plate": shot_sign_plate, "tiling": shot_tiling,
         "assembled": shot_assembled}
for c in CLUTTER:
    SHOTS[f"clutter-{c.replace('_', '-')}"] = (lambda c=c: shot_clutter(c))
SHOTS.update(CCTV_SHOTS)
SHOTS.update(PC_SHOTS)


if WANT == ["cctv"]:  # every CCTV and punishment shot, into .scratch/management-rooms/designs/pieces/
    WANT = list(CCTV_SHOTS)
if WANT == ["pc"]:  # the PC's shots, into .scratch/agent-screens/verification/09/
    WANT = list(PC_SHOTS)
if WANT == ["sit-in"]:  # the console with SIT IN, into .scratch/agent-screens/verification/16/
    WANT = ["cctv-console", "cctv-assembled"]
for name, fn in SHOTS.items():
    if (not WANT and name not in CCTV_SHOTS and name not in PC_SHOTS) or name in WANT:
        fn()
