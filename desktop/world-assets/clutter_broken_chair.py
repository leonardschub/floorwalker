"""Clutter: a broken folding chair, tipped on its side with its back snapped off
-> clutter_broken_chair.glb (origin on the floor).
Run: blender -b --factory-startup --python clutter_broken_chair.py -- <out-dir> [--rebake]
"""
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import wa_common as wc  # noqa: E402

out, M = wc.start(["rusty_steel_dark"])
m = M["rusty_steel_dark"]
# The chair without its back, built upright around the seat, then laid on its side.
frame = [wc.box((0.40, 0.38, 0.03), (0, 0, 0), m, bevel=0.008)]
for sx in (-0.19, 0.19):
    frame.append(wc.rod((sx, -0.19, 0), (sx, -0.23, -0.44), 0.011, m))  # front legs
    frame.append(wc.rod((sx, 0.17, 0), (sx, 0.24, -0.45), 0.012, m))  # back legs
    frame.append(wc.rod((sx, 0.17, 0), (sx, 0.19, 0.16), 0.012, m))  # snapped stubs of the back
frame.append(wc.rod((-0.19, 0.22, -0.30), (0.19, 0.22, -0.30), 0.008, m))
seat = wc.join(frame, "frame")
seat.rotation_euler = (0, 1.5708, 0.3)
seat.location = (0, 0, 0.212)
seat = wc.apply(seat)
back = wc.box((0.38, 0.025, 0.22), (0, 0, 0), m, bevel=0.008)
back.rotation_euler = (1.5708, 0, 0.9)
back.location = (-0.45, 0.3, 0.0125)
wc.clutter([seat, wc.apply(back)], "broken_chair")
wc.export(out, "clutter_broken_chair")
