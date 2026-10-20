"""The sign plate over each doorway: a rusty, blank steel plate -> sign_plate.glb.

Frame: the origin is the centre of the plate's back, which mounts on the corridor face of the
doorway wall; the plate faces -Y (glTF +Z, into the corridor). It is 1.8 x 0.32 m. The empty
`sign_text` marks the centre of the front face, where the world puts the project name's image
(1.6 x 0.24 m usable). In the corridor frame, the doorway's sign goes at (doorway centre, 0, 2.45).
Run: blender -b --factory-startup --python sign_plate.py -- <out-dir> [--rebake]
"""
import math
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import wa_common as wc  # noqa: E402

out, M = wc.start(["rusty_sign"])
W, D, H = 1.8, 0.04, 0.32
parts = [wc.box((W, D, H), (0, -D / 2, 0), M["rusty_sign"], bevel=0.006)]
for x in (-W / 2 + 0.05, W / 2 - 0.05):
    for z in (-H / 2 + 0.05, H / 2 - 0.05):
        parts.append(wc.cylinder(0.012, 0.012, (x, -D - 0.004, z), M["rusty_sign"], rot=(math.pi / 2, 0, 0),
                                 vertices=8))
plate = wc.join(parts, "sign_plate")
wc.uv_world(plate)
wc.empty("sign_text", (0, -D - 0.001, 0), parent=plate)
wc.part_scene("sign_plate", [plate])
wc.export(out, "sign_plate")
