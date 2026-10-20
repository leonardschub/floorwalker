"""The corridor kit: the pieces a corridor of any length is assembled from -> corridor_kit.glb.

One glTF scene (and one root node of the same name) per part, all in the corridor's frame: X along
the corridor from its start end, Y across it (the doorway wall's corridor face at y = 0, the rooms
behind it at +Y, the far wall's face at y = -2.4, so 2.4 m wide), Z up (ceiling at 3.0 m).

  corridor_segment        one doorway's 2.5 m (x 0..2.5): floor, ceiling and the brick doorway wall
                          with its 1.2 x 2.1 m opening (x 0.65..1.85). Segment n goes at x = 2.5 n,
                          with one of the far-wall parts below at the same place.
  corridor_segment_plain  the same 2.5 m without a doorway (the corridor with no project open)
  corridor_far_wall       a segment's 2.5 m of the peeling-paint far wall (x 0..2.5)
  corridor_far_wall_doorway        the same with the CCTV doorway's 1.2 x 2.1 m opening in its middle
                                   (x 0.65..1.85), opposite the segment's own doorway
  corridor_far_wall_doorway_end    the same with the opening's first 0.6 m at its end (x 1.9..2.5)
  corridor_far_wall_doorway_start  and with the opening's other 0.6 m at its start (x 0..0.6): two
                                   segments' far walls meet in the middle of the CCTV doorway when the
                                   corridor has an even number of segments
  corridor_end_start      the start end: 0.85 m of corridor (x -0.85..0) and the end wall, so the
                          wall stands 1.5 m before the first doorway. Place at x = 0.
  corridor_end_far        the far end: 0.85 m (x 0..0.85) and the end wall, 1.5 m after the last
                          doorway. Place at x = 2.5 n after the last segment.

The textures are 2.4 m tiles; along the corridor they are mapped 2.5 m to the tile (4% wider bricks)
so every segment boundary falls on a whole tile and segments meet without a seam. The tube light
and the sign plate come from room_kit.glb and sign_plate.glb.
Run: blender -b --factory-startup --python corridor_kit.py -- <out-dir> [--rebake]
"""
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import wa_common as wc  # noqa: E402

out, M = wc.start(["brick", "peeling_paint", "concrete", "ceiling"])
SEG, CW, H, T = 2.5, 2.4, 3.0, 0.2
DOOR = (0.65, 1.85, 2.1)
END = 0.85
STRETCH = (2.4 / SEG, 1.0, 1.0)


def run(x0, x1, doorway=False, far_wall=True):
    """Floor, ceiling, doorway-side wall (brick) and far wall (peeling paint) between x0 and x1."""
    objs = [wc.box_between((x0, -CW - T, -T), (x1, T, 0), M["concrete"]),
            wc.box_between((x0, -CW - T, H), (x1, T, H + T), M["ceiling"])]
    if far_wall:
        objs.append(wc.box_between((x0, -CW - T, 0), (x1, -CW, H), M["peeling_paint"]))
    if doorway:
        dl, dr, dh = DOOR
        objs += [wc.box_between((x0, 0, 0), (dl, T, H), M["brick"]),
                 wc.box_between((dr, 0, 0), (x1, T, H), M["brick"]),
                 wc.box_between((dl, 0, dh), (dr, T, H), M["brick"])]
    else:
        objs.append(wc.box_between((x0, 0, 0), (x1, T, H), M["brick"]))
    return objs


def far_wall(x0, x1, opening=None):
    """The far wall between x0 and x1, open from opening[0] to opening[1] (up to 2.1 m) if given."""
    paint = M["peeling_paint"]
    if opening is None:
        return [wc.box_between((x0, -CW - T, 0), (x1, -CW, H), paint)]
    open_left, open_right = opening
    objs = [wc.box_between((open_left, -CW - T, DOOR[2]), (open_right, -CW, H), paint)]
    if open_left > x0:
        objs.append(wc.box_between((x0, -CW - T, 0), (open_left, -CW, H), paint))
    if open_right < x1:
        objs.append(wc.box_between((open_right, -CW - T, 0), (x1, -CW, H), paint))
    return objs


def part(name, objs):
    o = wc.join(objs, name)
    wc.uv_world(o, scale=STRETCH)
    wc.part_scene(name, [o])


part("corridor_segment", run(0, SEG, doorway=True, far_wall=False))
part("corridor_segment_plain", run(0, SEG, far_wall=False))
part("corridor_far_wall", far_wall(0, SEG))
part("corridor_far_wall_doorway", far_wall(0, SEG, DOOR[:2]))
half = (DOOR[1] - DOOR[0]) / 2
part("corridor_far_wall_doorway_end", far_wall(0, SEG, (SEG - half, SEG)))
part("corridor_far_wall_doorway_start", far_wall(0, SEG, (0, half)))
part("corridor_end_start", run(-END, 0) + [wc.box_between((-END - T, -CW - T, 0), (-END, T, H), M["peeling_paint"])])
part("corridor_end_far", run(0, END) + [wc.box_between((END, -CW - T, 0), (END + T, T, H), M["peeling_paint"])])
wc.export(out, "corridor_kit")
