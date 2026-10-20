"""The CCTV console: a dark low slab with a glass top screen, SIT IN, KILL under a flip cover, WORK
HARDER -> cctv_console.glb.

Frame: the origin is on the floor under the console's centre; the operator's side faces -Y (glTF +Z,
towards the doorway), the far side faces the monitor wall. The slab is 2.6 m wide and 0.8 m deep; its
top slopes up away from the operator, from 0.80 m at the front edge to 0.95 m at the back
(TILT = atan(0.15 / 0.8), about 10.6 degrees), with a cyan light strip along both top edges.

Scene `cctv_console`, root `cctv_console` (an empty) with these children:

  console_body               slab, pedestal and edge strips (material `cctv_trim` for the glow)
  console_screen             the glass top screen listing flagged agents, 1.24 x 0.66 m, material
                             `cctv_screen` with UVs filling 0..1 on its face (the top of the image
                             towards the wall)
  console_sit_in             the cyan SIT IN bar between the glass and the hazard marks, 0.17 x 0.13 m
                             (agent-screens spec, "SIT IN on the CCTV console")
  console_hazard             the yellow and black hazard marks around the button
  console_kill_button        the red KILL button, 0.17 m across; origin at its base
  console_cover              the red flip cover, closed over the button; origin on its hinge, the back
                             edge (towards the wall). It opens by a negative turn about its local X
                             axis, lifting towards the wall (about -1.9 rad is fully open).
  console_work_harder        the yellow WORK HARDER bar, 0.40 x 0.13 m
  console_sit_in_label       empties marking the centre of each label's place in front of its control
  console_kill_label         (0.36 x 0.06 m usable each; SIT IN's 0.20 x 0.06 m, between the glass
  console_work_harder_label  and KILL's)

Every part on the top (screen, bars, hazard, button, cover, labels) has its own origin on the top
surface and is turned by TILT about X, so in its local frame the top is flat: glTF local X across,
local +Y out of the surface, local -Z up the slope. Clickable parts are separate nodes so the world can
pick them by their boxes and dim or light them by their materials (`kill_button`, `kill_cover`,
`work_harder`; `sit_in` is never dimmed).
Run: blender -b --factory-startup --python cctv_console.py -- <out-dir> [--rebake]
"""
import math
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import wa_common as wc  # noqa: E402
import wa_materials as wm  # noqa: E402

out, _ = wc.start([])
metal = wm.plain("console_metal", (0.03, 0.03, 0.035), 0.25, 0.7)
trim = wm.plain("cctv_trim", (0.0, 0.85, 1.0), 0.3, emit=(0.0, 0.85, 1.0), strength=8.0)
screen = wm.plain("cctv_screen", (0.01, 0.012, 0.015), 0.08, emit=(0.35, 0.75, 0.85), strength=0.15)
red = wm.plain("kill_button", (0.8, 0.03, 0.02), 0.35, emit=(1.0, 0.04, 0.02), strength=4.0)
cover_red = wm.plain("kill_cover", (0.55, 0.03, 0.02), 0.25, emit=(1.0, 0.1, 0.08), strength=0.6)
cyan = wm.plain("sit_in", (0.0, 0.6, 0.75), 0.4, emit=(0.0, 0.85, 1.0), strength=6.0)
yellow = wm.plain("work_harder", (1.0, 0.8, 0.1), 0.4, emit=(1.0, 0.85, 0.1), strength=6.0)
hazard = wm.plain("hazard_yellow", (0.9, 0.65, 0.05), 0.5, emit=(1.0, 0.75, 0.05), strength=1.5)

W, D = 2.6, 0.8
FRONT_H, BACK_H = 0.80, 0.95
TILT = math.atan2(BACK_H - FRONT_H, D)
SLOPE = math.hypot(BACK_H - FRONT_H, D)
BUTTON = (0.45, 0.42)  # (across, up the slope from the front edge)
BAR = (0.92, 0.42)
SIT_IN = (0.17, 0.42)  # midway between the glass's right edge (0.02) and the hazard marks' left (0.30)


def on_top(u, v, h=0.0):
    """A point on the top: u across, v up the slope from the front edge, h out of the surface."""
    c, s = math.cos(TILT), math.sin(TILT)
    return (u, -D / 2 + v * c - h * s, FRONT_H + v * s + h * c)


def top_part(name, parts, pivot):
    """Join `parts` (built flat around a pivot at the origin) and lay them on the top at `pivot` (u, v)."""
    o = wc.join(parts, name)
    wc.uv_world(o)
    o.location = on_top(*pivot)
    o.rotation_euler = (TILT, 0, 0)
    return o


# The slab: flat underneath, its top sloped; then a recessed pedestal and the edge strips.
slab = wc.box_between((-W / 2, -D / 2, 0.60), (W / 2, D / 2, FRONT_H), metal)
for v in slab.data.vertices:
    if v.co.y > 0 and v.co.z > 0.7:
        v.co.z = BACK_H
body = [slab,
        wc.box_between((-1.05, -0.22, 0), (1.05, 0.30, 0.60), metal),
        wc.box_between((-1.09, -0.26, 0), (1.09, 0.34, 0.03), metal)]
for y, z in ((-D / 2, FRONT_H), (D / 2, BACK_H)):
    body.append(wc.box((W, 0.014, 0.014), (0, y, z - 0.004), trim))
body.append(wc.box((2.12, 0.012, 0.012), (0, -0.226, 0.036), trim))  # a faint line at the pedestal's foot
body = wc.join(body, "console_body")
wc.uv_world(body)

glass = top_part("console_screen", [wc.box((1.24, 0.66, 0.004), (0, 0, 0.002), screen)], (-0.60, 0.40))
wc.uv_fit(glass, "cctv_screen", front=(0, 0, 1))

# Hazard marks: a square ring of yellow and black stripes around the button.
stripes = []
OUT_R, BAND = 0.15, 0.03
n = 10
for side in range(4):
    for k in range(n):
        a = -OUT_R + (k + 0.5) * (2 * OUT_R / n)
        size = 2 * OUT_R / n
        mat = hazard if k % 2 == 0 else metal
        if side < 2:
            y = (OUT_R - BAND / 2) * (1 if side == 0 else -1)
            stripes.append(wc.box((size, BAND, 0.003), (a, y, 0.0015), mat))
        elif abs(a) < OUT_R - BAND:
            x = (OUT_R - BAND / 2) * (1 if side == 2 else -1)
            stripes.append(wc.box((BAND, size, 0.003), (x, a, 0.0015), mat))
ring = top_part("console_hazard", stripes, BUTTON)

button = top_part("console_kill_button", [
    wc.cylinder(0.105, 0.012, (0, 0, 0.006), metal, vertices=32),
    wc.cylinder(0.085, 0.035, (0, 0, 0.0295), red, vertices=32),
], BUTTON)

# The cover: an open-bottomed red box 0.22 x 0.22 x 0.07, hinged on its back edge.
CW, CH, T = 0.22, 0.07, 0.008
hinge = (0, CW / 2, 0)
cover_parts = [wc.box((CW, CW, T), (0, 0, CH - T / 2), cover_red, bevel=0.002)]
for sx in (-1, 1):
    cover_parts.append(wc.box((T, CW, CH - T), (sx * (CW - T) / 2, 0, (CH - T) / 2), cover_red))
for sy in (-1, 1):
    cover_parts.append(wc.box((CW - 2 * T, T, CH - T), (0, sy * (CW - T) / 2, (CH - T) / 2), cover_red))
cover_parts.append(wc.rod((-CW / 2, CW / 2, 0.004), (CW / 2, CW / 2, 0.004), 0.006, metal))  # hinge pin
cover_parts.append(wc.box((0.06, 0.02, 0.012), (0, -CW / 2 - 0.01, CH - 0.01), cover_red))  # finger tab
cover = wc.join(cover_parts, "console_cover")
wc.move_origin(cover, hinge)
cover = top_part("console_cover", [cover], (BUTTON[0], BUTTON[1] + CW / 2))

bar = top_part("console_work_harder", [
    wc.box((0.44, 0.17, 0.012), (0, 0, 0.006), metal, bevel=0.003),
    wc.box((0.40, 0.13, 0.035), (0, 0, 0.0295), yellow, bevel=0.008),
], BAR)

sit_in = top_part("console_sit_in", [
    wc.box((0.20, 0.17, 0.012), (0, 0, 0.006), metal, bevel=0.003),
    wc.box((0.17, 0.13, 0.035), (0, 0, 0.0295), cyan, bevel=0.008),
], SIT_IN)

root = wc.empty("cctv_console")
for o in (body, glass, sit_in, ring, button, cover, bar):
    wc.parent(o, root)
for name, (u, v) in (("console_sit_in_label", (SIT_IN[0], 0.13)), ("console_kill_label", (BUTTON[0], 0.13)),
                     ("console_work_harder_label", (BAR[0], 0.13))):
    label = wc.empty(name, on_top(u, v, 0.001), parent=root)
    label.rotation_euler = (TILT, 0, 0)
wc.part_scene("cctv_console", [root])
wc.export(out, "cctv_console")
