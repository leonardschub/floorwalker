"""The kill squad's robot and its charge -> squad_robot.glb.

A standing robot in matte-black armour with a red visor slit, a cyan chest trim light and a dark back
pack (management-rooms spec, "Punishments in motion"; the accepted squad A on `prototype/cctv-look`).
It is 1.74 m tall, standing in a neutral pose with its arms and legs hanging straight, so the world
swings each limb about its joint (procedural motion, no clips).

Frame: the origin is on the floor between the feet; it faces -Y (glTF +Z), like the seated robots.
`_l` is the robot's own left, +X. Each node's origin is its joint:

  squad_robot              (empty) the node the world moves along the path and turns
    squad_pelvis           hips, at (0, 0, 0.88)
      squad_torso          torso, chest trim (`cctv_trim`) and back pack, at the waist (0, 0, 0.96)
        squad_head         helmet and visor (`squad_visor`), at the neck (0, 0, 1.46)
        squad_arm_upper_l  shoulder and upper arm, at the shoulder (0.26, 0, 1.38)
          squad_arm_lower_l  forearm and hand, at the elbow (0.28, 0, 1.10)
        squad_arm_upper_r  (-0.26, 0, 1.38) > squad_arm_lower_r (-0.28, 0, 1.10)
      squad_leg_upper_l    thigh, at the hip (0.12, 0, 0.86)
        squad_leg_lower_l  shin and boot, at the knee (0.12, 0, 0.48)
      squad_leg_upper_r    (-0.12, 0, 0.86) > squad_leg_lower_r (-0.12, 0, 0.48)

Scene `squad_charge`: the charge a crouching robot plants on the target's chest, 0.14 x 0.05 x 0.10 m,
origin at the centre of its back (it faces -Y), with `squad_charge_led` the light the world blinks.
Run: blender -b --factory-startup --python squad_robot.py -- <out-dir> [--rebake]
"""
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import wa_common as wc  # noqa: E402
import wa_materials as wm  # noqa: E402

out, _ = wc.start([])
armour = wm.plain("squad_armour", (0.02, 0.02, 0.025), 0.35, 0.4)
joint = wm.plain("squad_joint", (0.07, 0.07, 0.08), 0.3, 0.8)
visor = wm.plain("squad_visor", (0.8, 0.02, 0.02), 0.3, emit=(1.0, 0.05, 0.03), strength=5.0)
trim = wm.plain("cctv_trim", (0.0, 0.85, 1.0), 0.3, emit=(0.0, 0.85, 1.0), strength=8.0)
led = wm.plain("charge_led", (0.8, 0.02, 0.02), 0.3, emit=(1.0, 0.05, 0.02), strength=8.0)

HIP, WAIST, NECK = 0.88, 0.96, 1.46


def node(name, parts, pivot):
    o = wc.join(parts, name)
    wc.uv_world(o)
    wc.move_origin(o, pivot)
    return o


pelvis = node("squad_pelvis", [
    wc.box((0.30, 0.18, 0.14), (0, 0, HIP), joint, bevel=0.02),
    wc.box((0.34, 0.20, 0.04), (0, 0, HIP + 0.06), armour, bevel=0.01),  # belt
], (0, 0, HIP))

torso = node("squad_torso", [
    wc.rod((0, 0, 0.92), (0, 0, 1.00), 0.07, joint),  # waist
    wc.box((0.40, 0.26, 0.48), (0, 0, 1.20), armour, bevel=0.03),
    wc.box((0.34, 0.03, 0.22), (0, -0.14, 1.31), armour, bevel=0.012),  # chest plate
    wc.box((0.30, 0.012, 0.025), (0, -0.158, 1.25), trim),  # chest trim light
    wc.box((0.30, 0.13, 0.32), (0, 0.195, 1.22), joint, bevel=0.02),  # back pack
    wc.box((0.22, 0.02, 0.04), (0, 0.27, 1.30), armour),
    wc.box((0.22, 0.02, 0.04), (0, 0.27, 1.16), armour),
], (0, 0, WAIST))

head = node("squad_head", [
    wc.rod((0, 0, 1.43), (0, 0, 1.53), 0.045, joint),  # neck
    wc.box((0.22, 0.24, 0.20), (0, -0.01, 1.64), armour, bevel=0.03),
    wc.box((0.23, 0.03, 0.03), (0, -0.125, 1.685), armour, bevel=0.006),  # brow
    wc.box((0.17, 0.012, 0.04), (0, -0.131, 1.635), visor),
], (0, 0, NECK))

limbs = {}
for side, sx in (("l", 1), ("r", -1)):
    s, e = (0.26 * sx, 0, 1.38), (0.28 * sx, 0, 1.10)
    w = (0.28 * sx, -0.02, 0.86)
    limbs[f"arm_upper_{side}"] = node(f"squad_arm_upper_{side}", [
        wc.ball(0.07, s, joint),
        wc.box((0.16, 0.18, 0.08), (s[0] + 0.02 * sx, 0, s[2] + 0.05), armour, bevel=0.02),  # pad
        wc.rod(s, e, 0.045, armour),
    ], s)
    limbs[f"arm_lower_{side}"] = node(f"squad_arm_lower_{side}", [
        wc.ball(0.05, e, joint),
        wc.rod(e, w, 0.04, armour),
        wc.ball(0.05, (w[0], w[1], 0.82), joint),
    ], e)
    h, k, a = (0.12 * sx, 0, 0.86), (0.12 * sx, 0, 0.48), (0.12 * sx, 0, 0.10)
    limbs[f"leg_upper_{side}"] = node(f"squad_leg_upper_{side}", [
        wc.ball(0.06, h, joint),
        wc.rod(h, k, 0.065, armour),
    ], h)
    limbs[f"leg_lower_{side}"] = node(f"squad_leg_lower_{side}", [
        wc.ball(0.065, k, joint),
        wc.box((0.12, 0.05, 0.12), (k[0], -0.07, k[2]), armour, bevel=0.015),  # knee pad
        wc.rod(k, a, 0.055, armour),
        wc.box((0.13, 0.26, 0.10), (a[0], -0.04, 0.05), joint, bevel=0.02),  # boot
    ], k)

root = wc.empty("squad_robot")
wc.parent(pelvis, root)
wc.parent(torso, pelvis)
wc.parent(head, torso)
for side in ("l", "r"):
    wc.parent(limbs[f"arm_upper_{side}"], torso)
    wc.parent(limbs[f"arm_lower_{side}"], limbs[f"arm_upper_{side}"])
    wc.parent(limbs[f"leg_upper_{side}"], pelvis)
    wc.parent(limbs[f"leg_lower_{side}"], limbs[f"leg_upper_{side}"])
wc.part_scene("squad_robot", [root])

charge = wc.join([
    wc.box((0.14, 0.05, 0.10), (0, -0.025, 0), joint, bevel=0.006),
    wc.box((0.16, 0.012, 0.02), (0, -0.002, 0.03), armour),  # straps
    wc.box((0.16, 0.012, 0.02), (0, -0.002, -0.03), armour),
    wc.box((0.08, 0.004, 0.03), (-0.02, -0.052, -0.015), armour),  # keypad
], "squad_charge")
wc.uv_world(charge)
light = wc.join([wc.box((0.025, 0.008, 0.018), (0.035, -0.054, 0.025), led)], "squad_charge_led")
wc.uv_world(light)
wc.move_origin(light, (0.035, -0.054, 0.025))
wc.parent(light, charge)
wc.part_scene("squad_charge", [charge])
wc.export(out, "squad_robot")
