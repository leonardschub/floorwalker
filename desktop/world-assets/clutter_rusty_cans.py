"""Clutter: three rusty cans, one standing and two lying -> clutter_rusty_cans.glb (origin on the floor).
Run: blender -b --factory-startup --python clutter_rusty_cans.py -- <out-dir> [--rebake]
"""
import math
import os
import random
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import wa_common as wc  # noqa: E402

out, M = wc.start(["rusty_can"])
rnd = random.Random(21)
R, L = 0.033, 0.12


def can(loc, rot):
    body = wc.jitter(wc.cylinder(R, L, (0, 0, 0), M["rusty_can"], vertices=16), rnd, 0.002)
    rims = [wc.cylinder(R + 0.002, 0.006, (0, 0, z), M["rusty_can"], vertices=16) for z in (-L / 2, L / 2)]
    o = wc.join([body] + rims, "can")
    o.rotation_euler = rot
    o.location = loc
    return wc.apply(o)


parts = [can((0.0, 0.0, L / 2 + 0.003), (0, 0, 0.4)),
         can((0.20, 0.06, R + 0.002), (math.pi / 2, 0, rnd.uniform(0, 6.283))),
         can((-0.12, 0.18, R + 0.002), (math.pi / 2, 0, rnd.uniform(0, 6.283)))]
wc.clutter(parts, "rusty_cans")
wc.export(out, "clutter_rusty_cans")
