"""Clutter: loose A4 sheets on the floor -> clutter_loose_sheets.glb (origin on the floor).
Run: blender -b --factory-startup --python clutter_loose_sheets.py -- <out-dir> [--rebake]
"""
import os
import random
import sys

import bpy

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import wa_common as wc  # noqa: E402

out, M = wc.start(["paper"])
rnd = random.Random(33)
parts = []
for i, (x, y) in enumerate(((0.0, 0.0), (0.18, 0.1), (-0.1, 0.25))):
    bpy.ops.mesh.primitive_grid_add(x_subdivisions=6, y_subdivisions=8, size=1)
    o = bpy.context.object
    o.scale = (0.21, 0.297, 1)
    o.data.materials.append(M["paper"])
    wc.apply(o)
    for v in o.data.vertices:  # a gentle curl at the ends and small creases
        v.co.z = 0.0015 + 0.08 * max(0.0, abs(v.co.y) - 0.1) ** 1.5 + rnd.uniform(0, 0.002)
    sol = o.modifiers.new("thickness", "SOLIDIFY")
    sol.thickness = 0.0008
    o.rotation_euler = (0, 0, rnd.uniform(0, 6.283))
    o.location = (x, y, i * 0.0012)
    parts.append(wc.apply(o))
wc.clutter(parts, "loose_sheets")
wc.export(out, "clutter_loose_sheets")
