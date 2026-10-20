"""Clutter: a stack of soggy cardboard crates -> clutter_cardboard_crates.glb (origin on the floor).
Run: blender -b --factory-startup --python clutter_cardboard_crates.py -- <out-dir> [--rebake]
"""
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import wa_common as wc  # noqa: E402

out, M = wc.start(["cardboard"])


def crate(size, loc, turn, flaps=True):
    w, d, h = size
    parts = [wc.box(size, (0, 0, h / 2), M["cardboard"], bevel=0.006)]
    for sx in (-1, 1) if flaps else ():  # the two top flaps, slightly lifted
        flap = wc.box((w / 2 - 0.01, d - 0.01, 0.004), (0, 0, 0), M["cardboard"])
        flap.rotation_euler = (0, sx * -0.12, 0)
        flap.location = (sx * w / 4, 0, h + 0.004 + 0.012)
        parts.append(wc.apply(flap))
    o = wc.join(parts, "crate")
    o.rotation_euler = (0, 0, turn)
    o.location = loc
    return wc.apply(o)


parts = [crate((0.5, 0.45, 0.42), (0, 0, 0), 0.05, flaps=False),
         crate((0.4, 0.36, 0.32), (0.02, 0.01, 0.42), -0.12),
         crate((0.4, 0.36, 0.34), (0.52, 0.05, 0), 0.25)]
wc.clutter(parts, "cardboard_crates")
wc.export(out, "clutter_cardboard_crates")
