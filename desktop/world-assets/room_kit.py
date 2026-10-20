"""The room kit: the pieces a room interior of any depth is assembled from -> room_kit.glb.

One glTF scene (and one root node of the same name) per part, all in the room's frame: X across the
room (inner side walls at x = -4.75 and +4.75, so 9.5 m wide), Y from the doorway wall's inner face
(y = 0) into the room (glTF -Z), Z up from the floor (ceiling at 3.0 m).

  room_doorway_wall  the wall with the 1.2 x 2.1 m doorway (x -4.10..-2.90, at the left seen from
                     the corridor) plus the first 0.2 m of floor, ceiling and side walls (y 0..0.2)
  room_row_segment   floor, ceiling and both side walls for one 2.4 m row, y 0..2.4 in its own frame;
                     row n goes at y = 0.2 + 2.4 n (with 1.6 m to the first desk row's seat line,
                     desks sit 1.4 m into their segment)
  room_back_wall     the back wall, inner face at y = 0 of its own frame; goes at y = 0.2 + 2.4 rows
  room_pipes         two rusty pipes along the left wall (seen from the doorway) near the ceiling, one row segment long
                     (same frame and placement as a row segment)
  room_tube_light    a fluorescent fixture, origin at its ceiling mount (place at z = 3.0), long axis X
  room_tube_light_dead  the same fixture with a dead tube

A room with R rows is: doorway wall, R row segments, the back wall: 0.2 + 2.4 R m deep, which is the
spec's 1.6 m margin + (R - 1) x 2.4 m + 1.0 m margin. Textures are mapped from these coordinates, so
segments placed at y = 0.2 + 2.4 n meet without a seam.
Run: blender -b --factory-startup --python room_kit.py -- <out-dir> [--rebake]
"""
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import wa_common as wc  # noqa: E402
import wa_materials as wm  # noqa: E402

out, M = wc.start(["brick", "concrete", "ceiling", "rusty_galvanised"])
W, H, ROW, T = 9.5, 3.0, 2.4, 0.2  # interior width, ceiling height, row depth, wall/slab thickness
X0, X1 = -W / 2, W / 2
DOOR = (-4.10, -2.90, 2.1)
STRIP = 0.2

glow = wm.plain("tube_glow", (0.88, 1.0, 0.78), 0.3, emit=(0.88, 1.0, 0.78), strength=6.0)
dead = wm.plain("tube_dead", (0.42, 0.42, 0.40), 0.35)


def shell(y0, y1):
    """Floor, ceiling and side walls between y0 and y1."""
    return [wc.box_between((X0 - T, y0, -T), (X1 + T, y1, 0), M["concrete"]),
            wc.box_between((X0 - T, y0, H), (X1 + T, y1, H + T), M["ceiling"]),
            wc.box_between((X0 - T, y0, 0), (X0, y1, H), M["brick"]),
            wc.box_between((X1, y0, 0), (X1 + T, y1, H), M["brick"])]


def part(name, objs, offset=(0, 0, 0)):
    o = wc.join(objs, name)
    wc.uv_world(o, offset=offset)
    wc.part_scene(name, [o])
    return o


# The doorway wall, with the 0.2 m strip in front of the first row. Its texture is shifted by -0.2 m
# along Y so it meets row segment 0 (placed at y = 0.2) on a whole tile.
dl, dr, dh = DOOR
part("room_doorway_wall", shell(0, STRIP) + [
    wc.box_between((X0 - T, -T, 0), (dl, 0, H), M["brick"]),
    wc.box_between((dr, -T, 0), (X1 + T, 0, H), M["brick"]),
    wc.box_between((dl, -T, dh), (dr, 0, H), M["brick"]),
    wc.box_between((dl, -T, -T), (dr, 0, 0), M["concrete"]),  # threshold
    wc.box_between((dl, -T, H), (dr, 0, H + T), M["ceiling"]),
], offset=(0, -STRIP, 0))

part("room_row_segment", shell(0, ROW))

part("room_back_wall", [wc.box_between((X0 - T, 0, -T), (X1 + T, T, H + T), M["brick"])])

pipes = []
for x, z, r in ((X0 + 0.14, 2.62, 0.06), (X0 + 0.30, 2.80, 0.045)):
    pipes.append(wc.cylinder(r, ROW, (x, ROW / 2, z), M["rusty_galvanised"], rot=(1.5708, 0, 0), vertices=16))
    pipes.append(wc.cylinder(r + 0.012, 0.05, (x, 0.3, z), M["rusty_galvanised"], rot=(1.5708, 0, 0)))  # collar
for y in (1.2,):
    pipes.append(wc.box_between((X0, y - 0.02, 2.70), (X0 + 0.38, y + 0.02, 2.72), M["rusty_galvanised"]))
    pipes.append(wc.box_between((X0, y - 0.02, 2.52), (X0 + 0.22, y + 0.02, 2.54), M["rusty_galvanised"]))
part("room_pipes", pipes)


def fixture(name, tube_mat):
    objs = [wc.box((1.36, 0.16, 0.05), (0, 0, -0.045), M["rusty_galvanised"], bevel=0.006),
            wc.box((1.30, 0.10, 0.03), (0, 0, -0.08), tube_mat, bevel=0.012)]
    for x in (-0.55, 0.55):
        objs.append(wc.rod((x, 0, 0), (x, 0, -0.02), 0.004, M["rusty_galvanised"], vertices=6))
    part(name, objs)


fixture("room_tube_light", glow)
fixture("room_tube_light_dead", dead)
wc.export(out, "room_kit")
