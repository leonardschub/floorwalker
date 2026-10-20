"""Clutter: black cables snaking across the floor, about one desk pitch (2.2 m) along X
-> clutter_floor_cables.glb (origin on the floor at the run's middle).
Run: blender -b --factory-startup --python clutter_floor_cables.py -- <out-dir> [--rebake]
"""
import math
import os
import random
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import wa_common as wc  # noqa: E402

out, M = wc.start(["rubber"])
rnd = random.Random(42)
parts = []
for r, dy, amp in ((0.007, 0.0, 0.06), (0.005, 0.05, 0.09), (0.004, -0.04, 0.05)):
    phase = rnd.uniform(0, 6.283)
    phase2 = rnd.uniform(0, 6.283)
    pts = [(x, dy + amp * math.sin(x * 2.3 + phase) + 0.25 * amp * math.sin(x * 7.1 + phase2), r)
           for x in (i * 0.05 - 1.1 for i in range(45))]
    parts.append(wc.tube_path(pts, r, M["rubber"]))
parts.append(wc.box((0.04, 0.025, 0.02), (1.12, 0.0, 0.01), M["rubber"], bevel=0.004))  # a plug
wc.clutter(parts, "floor_cables")
wc.export(out, "clutter_floor_cables")
