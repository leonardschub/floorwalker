"""The sign over the CCTV doorway: a black glass plate with a cyan outline -> cctv_sign_plate.glb.

Frame: like `sign_plate.py`, the origin is the centre of the plate's back, which mounts on the corridor
face of the doorway wall; the plate faces -Y (glTF +Z, into the corridor). It is 1.1 x 0.30 m. The empty
`cctv_sign_text` marks the centre of the front face, where the world puts the "CCTV" image in the
room's cyan style (0.9 x 0.18 m usable inside the outline). In the corridor frame it goes at
(doorway centre, 0, 2.45), like the project doorways' signs.
Run: blender -b --factory-startup --python cctv_sign_plate.py -- <out-dir> [--rebake]
"""
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import wa_common as wc  # noqa: E402
import wa_materials as wm  # noqa: E402

out, _ = wc.start([])
glass = wm.plain("cctv_sign_glass", (0.012, 0.014, 0.018), 0.1)
trim = wm.plain("cctv_trim", (0.0, 0.85, 1.0), 0.3, emit=(0.0, 0.85, 1.0), strength=8.0)

W, D, H = 1.1, 0.04, 0.30
INSET, TH = 0.025, 0.012
parts = [wc.box((W, D, H), (0, -D / 2, 0), glass, bevel=0.004)]
ow, oh, y = W - 2 * INSET, H - 2 * INSET, -D - 0.001
for sz in (-1, 1):
    parts.append(wc.box((ow, 0.004, TH), (0, y, sz * (oh - TH) / 2), trim))
for sx in (-1, 1):
    parts.append(wc.box((TH, 0.004, oh - 2 * TH), (sx * (ow - TH) / 2, y, 0), trim))
plate = wc.join(parts, "cctv_sign_plate")
wc.uv_world(plate)
wc.empty("cctv_sign_text", (0, -D - 0.001, 0), parent=plate)
wc.part_scene("cctv_sign_plate", [plate])
wc.export(out, "cctv_sign_plate")
