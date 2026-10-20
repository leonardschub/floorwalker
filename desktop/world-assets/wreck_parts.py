"""What a kill leaves at the desk: the robot's charred parts, a knocked-over chair, debris and scorch
marks -> wreck_parts.glb.

From the management-rooms spec, "Punishments in motion": the robot's parts (torso, head, limbs) are
scattered in charred paint (`charred_paint`), the chair is knocked over, the floor and the desk carry a
scorch mark. One glTF scene (and root node) per part, so the world scatters each one on its own.

  wreck_torso   the torso, dented, lying on its back
  wreck_head    the head on its side, its face plate cracked (`wreck_glass`)
  wreck_arm     an arm bent at the elbow, lying flat (the world uses it twice, mirrored or turned)
  wreck_leg     a leg bent at the knee, lying flat (likewise)
  wreck_chair   the desk set's folding chair, tipped over onto its back
  wreck_debris  a small torn plate in the robot's own paint (`robot_paint`), for the debris the blast
                throws; origin at its centre, about 9 x 6 cm
  scorch_floor  a ragged black scorch about 2 m across, 2-3 mm above its origin; centre it on the
                desk set's origin (the seat point on the floor)
  scorch_desk   a ragged scorch for the desk top, about 0.9 x 0.45 m, 1 mm above its origin; it goes
                at (0, -0.57, 0.76) in the desk set's frame

Each lying part and the scorch on the floor has its origin on the floor under its middle, so the world
puts it at floor height and turns it about the vertical as it likes. Sizes are the seated robot's (full
size; the world scales them with the robot).
Run: blender -b --factory-startup --python wreck_parts.py -- <out-dir> [--rebake]
"""
import math
import os
import random
import sys

import bmesh
import bpy
from mathutils import Matrix

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import wa_common as wc  # noqa: E402
import wa_materials as wm  # noqa: E402

out, M = wc.start(["charred_paint", "robot_joint", "robot_paint", "rusty_steel_dark"])
char, joint, paint, steel = M["charred_paint"], M["robot_joint"], M["robot_paint"], M["rusty_steel_dark"]
glass = wm.plain("wreck_glass", (0.01, 0.011, 0.013), 0.35)
scorch = wm.plain("scorch", (0.014, 0.012, 0.010), 0.95)
core = wm.plain("scorch_core", (0.004, 0.004, 0.004), 1.0)


def rest(o, rot):
    """Turn `o` by `rot`, then move it so it rests on z = 0 with its middle over the origin."""
    turn = Matrix.Rotation(rot[2], 4, "Z") @ Matrix.Rotation(rot[1], 4, "Y") @ Matrix.Rotation(rot[0], 4, "X")
    o.data.transform(turn)
    vs = [v.co for v in o.data.vertices]
    lo = [min(v[k] for v in vs) for k in range(3)]
    hi = [max(v[k] for v in vs) for k in range(3)]
    o.data.transform(Matrix.Translation((-(lo[0] + hi[0]) / 2, -(lo[1] + hi[1]) / 2, -lo[2])))
    return o


def part(name, objs, rot):
    o = rest(wc.join(objs, name), rot)
    wc.uv_world(o)
    wc.part_scene(name, [o])
    return o


rnd = random.Random(12)

# The torso as the seated robot has it (robot.py, facing -Y), dented, with torn sockets.
torso = [wc.box((0.30, 0.19, 0.28), (0, 0.04, 0.32), char, bevel=0.03),
         wc.box((0.18, 0.02, 0.12), (0, -0.06, 0.34), joint, bevel=0.01),
         wc.rod((0, 0.04, 0.46), (0.01, 0.05, 0.50), 0.03, joint),
         wc.rod((0, 0.03, 0.18), (-0.01, 0.03, 0.13), 0.035, joint)]
for i in range(4):
    torso.append(wc.box((0.16, 0.012, 0.012), (0, -0.071, 0.30 + i * 0.026), char, bevel=0.003))
for sx in (-1, 1):
    torso.append(wc.ball(0.035, (0.15 * sx, 0.04, 0.42), joint))
wc.jitter(torso[0], rnd, 0.006)
part("wreck_torso", torso, (1.45, 0.2, 0.7))

head = [wc.box((0.21, 0.19, 0.18), (0, 0.03, 0.64), char, bevel=0.035),
        wc.box((0.16, 0.01, 0.13), (0, -0.065, 0.64), glass, bevel=0.008),
        wc.rod((-0.06, 0.06, 0.73), (-0.07, 0.09, 0.78), 0.006, joint),
        wc.rod((-0.07, 0.09, 0.78), (-0.13, 0.11, 0.80), 0.006, joint),  # the antenna, bent
        wc.rod((0, 0.03, 0.55), (0.01, 0.03, 0.52), 0.03, joint)]  # neck stub
for a, b in (((-0.06, -0.071, 0.68), (0.01, -0.071, 0.62)), ((0.01, -0.071, 0.62), (0.05, -0.071, 0.66)),
             ((0.01, -0.071, 0.62), (0.03, -0.071, 0.58))):  # cracks in the face plate
    head.append(wc.rod(a, b, 0.0025, char, vertices=6))
wc.jitter(head[0], rnd, 0.005)
part("wreck_head", head, (0.25, 1.35, 0.6))


def bent(a_len, b_len, r_a, r_b, angle, end):
    """A limb lying flat: a segment along X, a joint, a second segment turned by `angle`, then `end`."""
    s, e = (0, 0, 0), (a_len, 0, 0)
    w = (a_len + b_len * math.cos(angle), b_len * math.sin(angle), 0)
    return [wc.ball(r_a * 1.2, s, joint), wc.rod(s, e, r_a, char), wc.ball(r_a * 1.05, e, joint),
            wc.rod(e, w, r_b, char)] + end(w, angle)


part("wreck_arm", bent(0.27, 0.27, 0.032, 0.028, 1.2, lambda w, a: [
    wc.box((0.07, 0.06, 0.025), (w[0] + 0.03 * math.cos(a), w[1] + 0.03 * math.sin(a), 0), joint,
           rot=(0, 0, a), bevel=0.008)]), (0.1, 0, 0))
part("wreck_leg", bent(0.40, 0.42, 0.042, 0.036, -0.7, lambda w, a: [
    wc.box((0.17, 0.09, 0.05), (w[0] + 0.06 * math.cos(a), w[1] + 0.06 * math.sin(a), 0.03), joint,
           rot=(0, 0, a), bevel=0.01)]), (0, 0.15, 0))

# The desk set's folding chair (desk_set.py), tipped over backwards onto its back.
SEAT = 0.46
chair = [wc.box((0.40, 0.38, 0.03), (0, -0.02, SEAT - 0.02), steel, bevel=0.008),
         wc.box((0.38, 0.025, 0.22), (0, 0.19, 0.80), steel, bevel=0.008)]
for sx in (-0.19, 0.19):
    chair.append(wc.rod((sx, 0.24, 0.0), (sx, 0.18, 0.92), 0.012, steel))
    chair.append(wc.rod((sx, -0.21, 0.0), (sx, 0.12, SEAT - 0.03), 0.011, steel))
chair.append(wc.rod((-0.19, 0.20, 0.25), (0.19, 0.20, 0.25), 0.008, steel))
chair.append(wc.rod((-0.19, -0.18, 0.06), (0.19, -0.18, 0.06), 0.008, steel))
part("wreck_chair", chair, (-1.32, 0.12, 0.35))

debris = wc.box((0.09, 0.06, 0.012), (0, 0, 0), paint, bevel=0.003)
wc.jitter(debris, rnd, 0.004)
debris = wc.join([debris], "wreck_debris")
wc.uv_world(debris)
wc.part_scene("wreck_debris", [debris])


def ragged(name, rx, ry, z, mat, rnd, n=48, rough=0.14, spikes=7):
    """A flat ragged blob facing up: an ellipse rx x ry whose edge wavers and throws a few spikes."""
    bm = bmesh.new()
    phase = [rnd.uniform(0, math.tau) for _ in range(3)]
    spike_at = sorted(rnd.uniform(0, math.tau) for _ in range(spikes))
    verts = []
    for i in range(n):
        a = i * math.tau / n
        f = 1 + rough * (0.6 * math.sin(3 * a + phase[0]) + 0.3 * math.sin(7 * a + phase[1])
                         + 0.2 * math.sin(13 * a + phase[2])) + rnd.uniform(-0.04, 0.04)
        for s in spike_at:
            d = abs((a - s + math.pi) % math.tau - math.pi)
            f += 0.28 * max(0.0, 1 - d / 0.12)
        verts.append(bm.verts.new((rx * f * math.cos(a), ry * f * math.sin(a), z)))
    bm.faces.new(verts)
    me = bpy.data.meshes.new(name)
    bm.to_mesh(me)
    bm.free()
    o = bpy.data.objects.new(name, me)
    bpy.context.scene.collection.objects.link(o)
    me.materials.append(mat)
    return o


for name, (rx, ry), z in (("scorch_floor", (0.9, 0.85), 0.002), ("scorch_desk", (0.42, 0.2), 0.001)):
    blob = wc.join([ragged(f"{name}_outer", rx, ry, z, scorch, rnd),
                    ragged(f"{name}_core", rx * 0.5, ry * 0.5, z + 0.001, core, rnd, spikes=4)], name)
    wc.uv_world(blob)
    wc.part_scene(name, [blob])

wc.export(out, "wreck_parts")
