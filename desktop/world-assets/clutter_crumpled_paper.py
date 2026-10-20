"""Clutter: three balls of crumpled paper -> clutter_crumpled_paper.glb (origin on the floor, centre).
Run: blender -b --factory-startup --python clutter_crumpled_paper.py -- <out-dir> [--rebake]
"""
import os
import random
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import wa_common as wc  # noqa: E402

out, M = wc.start(["paper"])
rnd = random.Random(14)
parts = []
for x, y, r in ((0.0, 0.0, 0.05), (0.22, 0.12, 0.04), (-0.15, 0.2, 0.035)):
    o = wc.jitter(wc.ico(r, (0, 0, 0), M["paper"]), rnd, r * 0.3)
    o.scale = (1.0, rnd.uniform(0.75, 1.15), 0.75)
    o.rotation_euler = (0, 0, rnd.uniform(0, 6.283))
    o.location = (x, y, r * 0.65)
    parts.append(wc.apply(o))
wc.clutter(parts, "crumpled_paper")
wc.export(out, "clutter_crumpled_paper")
