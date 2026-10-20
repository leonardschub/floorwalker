"""The CCTV room kit: the shell of the CCTV room, which widens a feed column at a time -> cctv_room_kit.glb.

The look is direction A, "black site", accepted on branch `prototype/cctv-look`: near-black glass
and carbon walls, a glossy dark floor, thin cold-cyan light strips along the floor and ceiling edges
and vertical light ribs on the side walls. Glow is emissive only (`cctv_trim`).

One glTF scene (and one root node of the same name) per part, all in the room's frame: X across the
room, centred on the doorway (inner side walls at x = -4 and +4, so 8 m wide), Y from the doorway
wall's inner face (y = 0) into the room (glTF -Z; the back wall's inner face at y = 7), Z up from the
floor (ceiling at 4.2 m). Walls and slabs are 0.2 m thick. Each part's light strips are a child mesh
`<part>_lights`, so the world can treat the glow apart from the shell.

  cctv_doorway_wall     the front wall, x -4..4, with the 1.2 x 2.1 m doorway centred (x -0.6..0.6)
  cctv_floor            the floor, x -4..4, y 0..7
  cctv_ceiling          the ceiling, x -4..4, y 0..7
  cctv_back_wall        the back wall (the monitor wall), x -4..4
  cctv_side_wall_left   the left side wall seen from the doorway (x -4.2..-4), closing both corners,
                        with its floor and ceiling strips and three light ribs (y 1.5, 3.5, 5.5)
  cctv_side_wall_right  the same on the right (x 4..4.2)
  cctv_extension        floor, ceiling, front and back wall for a strip half a feed column wide
                        (x 0..2/3 in its own frame), with its strips

Widening: a feed column is 4/3 m. Past 4 columns the room gains one column at a time, half on each
side: with n extra columns, extension k (0..n-1) goes at x = 4 + 2k/3 on the right and at
x = -4 - 2(k+1)/3 on the left, and the side walls move out by 2n/3 m. Depth and height never change.
Textures are mapped from these coordinates on 2/3 m tiles, so the strips continue the pattern.
Run: blender -b --factory-startup --python cctv_room_kit.py -- <out-dir> [--rebake]
"""
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import wa_common as wc  # noqa: E402
import wa_materials as wm  # noqa: E402

out, M = wc.start(["cctv_wall", "cctv_floor"])
W, L, H, T = 8.0, 7.0, 4.2, 0.2
X0, X1 = -W / 2, W / 2
DOOR_W, DOOR_H = 1.2, 2.1
EXT = 2.0 / 3.0  # half a feed column
S, GAP = 0.03, 0.01  # light strips: 3 cm square, 1 cm off the wall
RIBS = (1.5, 3.5, 5.5)

wall, floor = M["cctv_wall"], M["cctv_floor"]
ceiling = wm.plain("cctv_ceiling", (0.010, 0.010, 0.012), 0.85)
trim = wm.plain("cctv_trim", (0.0, 0.85, 1.0), 0.3, emit=(0.0, 0.85, 1.0), strength=8.0)


def part(name, objs, lights=(), offset=(0, 0, 0)):
    """A part's shell mesh, with its light strips as the child `<name>_lights`."""
    o = wc.join(objs, name)
    wc.uv_world(o, offset=offset)
    if lights:
        glow = wc.join(list(lights), f"{name}_lights")
        wc.uv_world(glow)
        wc.parent(glow, o)
    wc.part_scene(name, [o])
    return o


def edge_strips(x0, x1, y0, y1, floor=True):
    """Light strips along X (x0..x1) at y0..y1, on the floor and under the ceiling."""
    out = [wc.box_between((x0, y0, H - S), (x1, y1, H), trim)]
    if floor:
        out.append(wc.box_between((x0, y0, 0), (x1, y1, S), trim))
    return out


FRONT = (GAP, GAP + S)  # strips along the doorway wall
BACK = (L - GAP - S, L - GAP)  # strips along the back wall

dl, dr = -DOOR_W / 2, DOOR_W / 2
part("cctv_doorway_wall", [
    wc.box_between((X0, -T, -T), (dl, 0, H + T), wall),
    wc.box_between((dr, -T, -T), (X1, 0, H + T), wall),
    wc.box_between((dl, -T, DOOR_H), (dr, 0, H + T), wall),
    wc.box_between((dl, -T, -T), (dr, 0, 0), floor),  # threshold
], lights=edge_strips(X0, X1, *FRONT, floor=False)
    + [wc.box_between((X0, FRONT[0], 0), (dl - 0.1, FRONT[1], S), trim),
       wc.box_between((dr + 0.1, FRONT[0], 0), (X1, FRONT[1], S), trim)])

part("cctv_floor", [wc.box_between((X0, 0, -T), (X1, L, 0), floor)])
part("cctv_ceiling", [wc.box_between((X0, 0, H), (X1, L, H + T), ceiling)])
part("cctv_back_wall", [wc.box_between((X0, L, -T), (X1, L + T, H + T), wall)], lights=edge_strips(X0, X1, *BACK))


def side_wall(name, sign):
    inner = sign * W / 2
    outer = inner + sign * T
    lights = []
    face = inner - sign * GAP
    strip = sorted((face, face - sign * S))
    for z0, z1 in ((0, S), (H - S, H)):
        lights.append(wc.box_between((strip[0], 0, z0), (strip[1], L, z1), trim))
    rib = sorted((inner - sign * 0.005, inner - sign * 0.035))
    for y in RIBS:
        lights.append(wc.box_between((rib[0], y - 0.02, 0.35), (rib[1], y + 0.02, H - 0.35), trim))
    lo, hi = sorted((inner, outer))
    # Seams every 2/3 m from y = 1/6, so each rib sits on one.
    part(name, [wc.box_between((lo, -T, -T), (hi, L + T, H + T), wall)], lights=lights, offset=(0, -1 / 6, 0))


side_wall("cctv_side_wall_left", -1)
side_wall("cctv_side_wall_right", 1)

part("cctv_extension", [
    wc.box_between((0, 0, -T), (EXT, L, 0), floor),
    wc.box_between((0, 0, H), (EXT, L, H + T), ceiling),
    wc.box_between((0, -T, -T), (EXT, 0, H + T), wall),
    wc.box_between((0, L, -T), (EXT, L + T, H + T), wall),
], lights=edge_strips(0, EXT, *FRONT) + edge_strips(0, EXT, *BACK))

wc.export(out, "cctv_room_kit")
