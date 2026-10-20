"""The wall camera: a small CCTV camera hanging inside each project room above its doorway -> wall_camera.glb.

Frame: the room kit's (`room_kit.py`): the origin is the mount point on the doorway wall's inner face
(y = 0), and the camera looks into the room, +Y (glTF -Z). Place it at (doorway centre, 0, 2.55) in
the room frame, under the 3 m ceiling and over the 2.1 m doorway.

Scene `wall_camera`, root `wall_camera` (an empty) >
  wall_camera_mount   the wall plate, arm and ball joint (static)
  wall_camera_head    housing, sun hood, lens and red LED; origin on the ball joint (0, 0.17, -0.05),
                      tilted 0.45 rad down by default. The world may turn it to frame the room.
    > wall_camera_lens  an empty at the front of the lens, turned with the head: its glTF -Z is the
                        lens's axis and +Y is up, so a feed camera parented to it with no transform
                        of its own looks where the camera looks.
In the sweatshop's materials: yellowed plastic and rusty steel, a black lens and a red LED.
Run: blender -b --factory-startup --python wall_camera.py -- <out-dir> [--rebake]
"""
import math
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import wa_common as wc  # noqa: E402
import wa_materials as wm  # noqa: E402

out, M = wc.start(["yellowed_plastic", "rusty_steel"])
plastic, steel = M["yellowed_plastic"], M["rusty_steel"]
lens = wm.plain("camera_lens", (0.01, 0.01, 0.012), 0.05)
led = wm.plain("camera_led", (0.8, 0.02, 0.02), 0.3, emit=(1.0, 0.05, 0.05), strength=20.0)

JOINT = (0, 0.17, -0.05)
TILT = 0.45

mount = [wc.box((0.10, 0.015, 0.14), (0, 0.0075, 0), steel, bevel=0.003),
         wc.rod((0, 0.015, 0), (0, 0.155, -0.04), 0.011, steel),
         wc.ball(0.02, JOINT, steel)]
for sx in (-0.035, 0.035):
    for sz in (-0.05, 0.05):
        mount.append(wc.cylinder(0.006, 0.006, (sx, 0.017, sz), steel, rot=(math.pi / 2, 0, 0), vertices=8))
mount = wc.join(mount, "wall_camera_mount")
wc.uv_world(mount)

# The head, built around its joint at the origin, looking +Y.
LENS_Y, LENS_Z = 0.235, -0.06
head = [wc.box((0.03, 0.04, 0.025), (0, 0, -0.012), steel, bevel=0.004),  # clamp under the joint
        wc.box((0.10, 0.24, 0.09), (0, 0.10, LENS_Z), plastic, bevel=0.012),
        wc.box((0.13, 0.29, 0.006), (0, 0.125, -0.012), plastic, bevel=0.002),  # sun hood
        wc.cylinder(0.034, 0.012, (0, 0.224, LENS_Z), plastic, rot=(math.pi / 2, 0, 0), vertices=24),
        wc.cylinder(0.026, 0.02, (0, LENS_Y - 0.01, LENS_Z), lens, rot=(math.pi / 2, 0, 0), vertices=24),
        wc.box((0.012, 0.004, 0.012), (0.034, 0.221, LENS_Z - 0.03), led)]
for i in range(4):  # vents on the back
    head.append(wc.box((0.07, 0.004, 0.008), (0, -0.0215, LENS_Z - 0.025 + i * 0.016), steel))
head = wc.join(head, "wall_camera_head")
wc.uv_world(head)
head.location = JOINT
head.rotation_euler = (-TILT, 0, 0)

root = wc.empty("wall_camera")
wc.parent(mount, root)
wc.parent(head, root)
wc.empty("wall_camera_lens", (0, LENS_Y, LENS_Z), parent=head)
wc.part_scene("wall_camera", [root])
wc.export(out, "wall_camera")
