"""The desk set: desk, folding chair, PC, keyboard, tower and mouse -> desk_set.glb.

Frame: origin on the floor under the robot's seat point. The robot sits facing -Y (glTF +Z), so the
desk and the PC's lit screen are in front of it and the PC's back faces the doorway. The empty
`robot_seat` marks where robot.glb's origin goes. The PC's glass is a single quad with its own
material `pc_screen`, its UVs filling 0..1 upright and unmirrored as the robot sees it (u to its
right, glTF v down), so the world can show a session terminal on it.
Run: blender -b --factory-startup --python desk_set.py -- <out-dir> [--rebake]
"""
import math
import os
import sys

import bpy
from mathutils import Matrix, Vector

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import wa_common as wc  # noqa: E402
import wa_materials as wm  # noqa: E402

out, M = wc.start(["laminate", "rusty_steel", "rusty_steel_dark", "yellowed_plastic", "bezel", "keyboard",
                   "pc_screen"])
led = wm.plain("led_green", (0.1, 0.8, 0.2), 0.3, emit=(0.1, 1.0, 0.2), strength=8.0)
SEAT = 0.46

# Desk: laminate top on four rusty legs with a rail.
desk = [wc.box((1.2, 0.6, 0.04), (0, -0.57, 0.74), M["laminate"], bevel=0.004)]
for lx in (-0.55, 0.55):
    for ly in (-0.32, -0.82):
        desk.append(wc.box((0.04, 0.04, 0.72), (lx, ly, 0.36), M["rusty_steel"], bevel=0.003))
    desk.append(wc.box((0.025, 0.46, 0.025), (lx, -0.57, 0.12), M["rusty_steel"]))
desk.append(wc.box((1.06, 0.025, 0.06), (0, -0.82, 0.68), M["rusty_steel"]))
desk = wc.join(desk, "desk")

# Folding chair, seat top at the robot's seat height.
chair = [wc.box((0.40, 0.38, 0.03), (0, -0.02, SEAT - 0.02), M["rusty_steel_dark"], bevel=0.008),
         wc.box((0.38, 0.025, 0.22), (0, 0.19, 0.80), M["rusty_steel_dark"], bevel=0.008)]
for sx in (-0.19, 0.19):
    chair.append(wc.rod((sx, 0.24, 0.0), (sx, 0.18, 0.92), 0.012, M["rusty_steel_dark"]))  # back legs
    chair.append(wc.rod((sx, -0.21, 0.0), (sx, 0.12, SEAT - 0.03), 0.011, M["rusty_steel_dark"]))  # front legs
chair.append(wc.rod((-0.19, 0.20, 0.25), (0.19, 0.20, 0.25), 0.008, M["rusty_steel_dark"]))
chair.append(wc.rod((-0.19, -0.18, 0.06), (0.19, -0.18, 0.06), 0.008, M["rusty_steel_dark"]))
chair = wc.join(chair, "chair")

# PC: a thin black widescreen on a neck and base (screen B), gaffer tape over a cracked corner of
# the bezel. The 5:3 glass (100 x 30 cells of 6 x 12 px) is tilted about its centre to face a
# full-size robot's eyes squarely.
GLASS = (0.49, 0.294)
CENTRE = Vector((0, -0.60, 1.03))
EYE = Vector((0, -0.07, 1.10))  # robot.glb's face plate centre, seated
TILT = math.atan2(EYE.z - CENTRE.z, EYE.y - CENTRE.y)
FACING = Matrix.Rotation(TILT, 3, "X")  # the screen's frame: +Y towards the eyes, Z up the glass
black = wm.plain("pc_plastic", (0.025, 0.025, 0.028), 0.45)
tape = wm.plain("gaffer_tape", (0.32, 0.32, 0.30), 0.9)


def on_screen(size, loc, mat, spin=0.0, bevel=0.0):
    """A box placed in the screen's frame, turned `spin` about the glass's normal."""
    rot = (FACING @ Matrix.Rotation(spin, 3, "Y")).to_euler("XYZ")
    return wc.box(size, CENTRE + FACING @ Vector(loc), mat, rot=rot, bevel=bevel)


bpy.ops.mesh.primitive_plane_add(size=1, location=CENTRE + FACING @ Vector((0, -0.004, 0.006)),
                                 rotation=(FACING @ Matrix.Rotation(-math.pi / 2, 3, "X")).to_euler("XYZ"))
glass = bpy.context.object
glass.scale = (GLASS[0], GLASS[1], 1)
glass.data.materials.append(M["pc_screen"])
pc = [on_screen((0.53, 0.03, 0.33), (0, -0.02, 0), black, bevel=0.004),
      wc.apply(glass),
      on_screen((0.075, 0.004, 0.022), (-0.235, -0.0025, 0.14), tape, spin=0.75),
      on_screen((0.06, 0.004, 0.02), (-0.24, -0.001, 0.135), tape, spin=-0.6),
      wc.box((0.05, 0.04, 0.17), (0, -0.64, 0.86), black, bevel=0.003),
      wc.box((0.30, 0.18, 0.015), (0, -0.64, 0.7675), black, bevel=0.003)]
pc = wc.join(pc, "pc")

keyboard = wc.join([wc.box((0.40, 0.14, 0.018), (0, -0.37, 0.769), M["yellowed_plastic"], bevel=0.004),
                    wc.box((0.38, 0.12, 0.012), (0, -0.365, 0.783), M["keyboard"])], "keyboard")

tower = wc.join([wc.box((0.18, 0.40, 0.40), (-0.42, -0.62, 0.2), M["yellowed_plastic"], bevel=0.01),
                 wc.box((0.12, 0.005, 0.03), (-0.42, -0.418, 0.33), M["bezel"]),
                 wc.box((0.10, 0.005, 0.02), (-0.42, -0.418, 0.28), M["bezel"]),
                 wc.box((0.01, 0.005, 0.01), (-0.46, -0.418, 0.22), led)], "tower")

mouse = wc.join([wc.box((0.06, 0.10, 0.03), (-0.28, -0.37, 0.775), M["yellowed_plastic"], bevel=0.012),
                 wc.rod((-0.28, -0.42, 0.765), (-0.30, -0.62, 0.762), 0.003, M["bezel"], vertices=6)], "mouse")

for o in (desk, chair, pc, keyboard, tower, mouse):
    wc.uv_world(o)
wc.uv_fit(pc, "pc_screen", front=FACING @ Vector((0, 1, 0)))

root = wc.empty("desk_set")
for o in (desk, chair, pc, keyboard, tower, mouse):
    wc.parent(o, root)
wc.empty("robot_seat", (0, 0, SEAT), parent=root)
wc.part_scene("desk_set", [root])
wc.export(out, "desk_set")
