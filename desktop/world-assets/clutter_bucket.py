"""Clutter: a rusty bucket with dirty water in it, for under a leak -> clutter_bucket.glb (origin on the floor).
Run: blender -b --factory-startup --python clutter_bucket.py -- <out-dir> [--rebake]
"""
import math
import os
import sys

import bpy

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import wa_common as wc  # noqa: E402
import wa_materials as wm  # noqa: E402

out, M = wc.start(["rusty_galvanised"])
m = M["rusty_galvanised"]
water = wm.plain("dirty_water", (0.03, 0.035, 0.025), 0.05)
H = 0.3

bpy.ops.mesh.primitive_cone_add(vertices=24, radius1=0.13, radius2=0.16, depth=H, location=(0, 0, H / 2),
                                end_fill_type="NOTHING")
pail = bpy.context.object
pail.data.materials.append(m)
bpy.ops.mesh.primitive_circle_add(vertices=24, radius=0.13, fill_type="NGON", location=(0, 0, 0))
bottom = bpy.context.object
bottom.data.materials.append(m)
pail = wc.join([pail, bottom], "pail")
for p in pail.data.polygons:
    p.use_smooth = abs(p.normal.z) < 0.5
pail.modifiers.new("wall", "SOLIDIFY").thickness = 0.006
pail = wc.apply(pail)

bpy.ops.mesh.primitive_torus_add(major_radius=0.16, minor_radius=0.006, location=(0, 0, H), major_segments=32,
                                 minor_segments=6)
rim = bpy.context.object
rim.data.materials.append(m)
# the wire handle, an arc fallen outwards to one side
tilt = 1.1
arc = [(0.162 * math.cos(a), -0.162 * math.sin(a) * math.sin(tilt), H + 0.162 * math.sin(a) * math.cos(tilt))
       for a in (i * math.pi / 16 for i in range(17))]
handle = wc.tube_path(arc, 0.004, m, sides=6)
water_disc = wc.cylinder(0.143, 0.004, (0, 0, 0.14), water, vertices=24, smooth=False)
wc.clutter([pail, wc.apply(rim), wc.apply(handle), water_disc], "bucket")
wc.export(out, "clutter_bucket")
