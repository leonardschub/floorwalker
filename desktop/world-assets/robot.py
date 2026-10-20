"""The robot: a humanoid built from blocks, seated and typing, static -> robot.glb.

Frame: the origin is the seat point (the robot's hips), the node the world scales. The robot faces
-Y (glTF +Z), towards its desk and the doorway; its feet reach the floor 0.46 m below the origin.
Nodes: `robot` (empty, the scalable node) > `robot_body` (paint and joints) and `robot_face`
(the blank smudged black glass plate, material `robot_face`, UVs filling 0..1 on its front only and
its sides and bevels on one blank corner, so ticket 22's face pattern and model name show on the
front alone, never squashed on the edges). No emblem or logo.
Run: blender -b --factory-startup --python robot.py -- <out-dir> [--rebake]
"""
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import wa_common as wc  # noqa: E402
import wa_materials as wm  # noqa: E402

out, M = wc.start(["robot_paint", "robot_joint", "robot_face"])
paint, joint = M["robot_paint"], M["robot_joint"]
tip = wm.plain("robot_antenna_tip", (0.9, 0.88, 0.82), 0.3, emit=(1.0, 0.95, 0.85), strength=1.0)


def P(x, y, z):
    """Prototype coordinates (robot facing +Y) turned to face -Y."""
    return (-x, -y, z)


parts = [
    wc.box((0.26, 0.17, 0.10), P(0, 0, 0.03), joint, bevel=0.012),  # pelvis
    wc.rod(P(0, -0.03, 0.08), P(0, -0.04, 0.18), 0.035, joint),  # waist
    wc.box((0.30, 0.19, 0.28), P(0, -0.04, 0.32), paint, bevel=0.03),  # torso
    wc.box((0.18, 0.02, 0.12), P(0, 0.06, 0.34), joint, bevel=0.01),  # chest vent
    wc.rod(P(0, -0.04, 0.46), P(0, -0.04, 0.54), 0.03, joint),  # neck
    wc.box((0.21, 0.19, 0.18), P(0, -0.03, 0.64), paint, bevel=0.035),  # head
    wc.rod(P(0.06, -0.06, 0.73), P(0.07, -0.08, 0.80), 0.006, joint),  # antenna
    wc.ball(0.012, P(0.07, -0.08, 0.80), tip),
]
for i in range(4):  # chest vent slats
    parts.append(wc.box((0.16, 0.012, 0.012), P(0, 0.071, 0.30 + i * 0.026), paint, bevel=0.003))
for side in (-1, 1):
    sh, el, wr = (0.18 * side, -0.04, 0.42), (0.21 * side, 0.16, 0.25), (0.13 * side, 0.42, 0.30)
    parts += [
        wc.ball(0.05, P(*sh), joint),
        wc.rod(P(*sh), P(*el), 0.032, paint),
        wc.ball(0.035, P(*el), joint),
        wc.rod(P(*el), P(*wr), 0.028, paint),
        wc.box((0.06, 0.07, 0.025), P(wr[0], wr[1] + 0.04, wr[2] - 0.01), joint, bevel=0.008),  # hand
    ]
    hip, knee, ankle = (0.09 * side, 0.02, 0.0), (0.09 * side, 0.42, 0.0), (0.09 * side, 0.45, -0.42)
    parts += [
        wc.rod(P(*hip), P(*knee), 0.042, paint),
        wc.ball(0.045, P(*knee), joint),
        wc.rod(P(*knee), P(*ankle), 0.036, paint),
        wc.box((0.09, 0.17, 0.05), P(ankle[0], ankle[1] + 0.04, ankle[2] - 0.02), joint, bevel=0.01),  # foot
    ]
body = wc.join(parts, "robot_body")
wc.uv_world(body)

face = wc.join([wc.box((0.16, 0.01, 0.13), P(0, 0.065, 0.64), M["robot_face"], bevel=0.008)], "robot_face")
wc.uv_world(face)
# Only the front (-Y) shows the face; its 1 cm sides and bevels get a blank corner of it.
wc.uv_fit(face, "robot_face", front=(0, -1, 0))

root = wc.empty("robot")
wc.parent(body, root)
wc.parent(face, root)
wc.part_scene("robot", [root])
wc.export(out, "robot")
