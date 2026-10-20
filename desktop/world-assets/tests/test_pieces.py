"""Checks of the world pieces the build generates. Plain Python 3, no Blender:

  python3 -m unittest discover -s desktop/world-assets/tests

The first check reads the scripts only. The others read the exported `.glb` files (`out/` beside
the scripts, or the folder in WORLD_ASSETS_OUT) and are skipped when a file is missing, so run
`regenerate.py` first. They pin what the world relies on: the scene and node names it spawns and
moves by name, and the sizes the spec sets (management-rooms spec, "Assets", "The CCTV room:
layout", "The console", "Punishments in motion"; agent-screens spec, "The PC screen").
"""
import json
import math
import os
import re
import struct
import sys
import unittest

HERE = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.environ.get("WORLD_ASSETS_OUT") or os.path.join(HERE, "out")
sys.path.insert(0, HERE)
import regenerate  # noqa: E402

# Every scene of the CCTV pieces, with each node the world finds by name and that node's parent
# (None for the scene's root). Later tickets load these by name, so a rename must be deliberate.
CCTV = {
    "cctv_room_kit": {
        "cctv_doorway_wall": {"cctv_doorway_wall": None, "cctv_doorway_wall_lights": "cctv_doorway_wall"},
        "cctv_floor": {"cctv_floor": None},
        "cctv_ceiling": {"cctv_ceiling": None},
        "cctv_back_wall": {"cctv_back_wall": None, "cctv_back_wall_lights": "cctv_back_wall"},
        "cctv_side_wall_left": {"cctv_side_wall_left": None,
                                "cctv_side_wall_left_lights": "cctv_side_wall_left"},
        "cctv_side_wall_right": {"cctv_side_wall_right": None,
                                 "cctv_side_wall_right_lights": "cctv_side_wall_right"},
        "cctv_extension": {"cctv_extension": None, "cctv_extension_lights": "cctv_extension"},
    },
    "cctv_monitors": {
        "cctv_feed_monitor": {"cctv_feed_monitor": None, "cctv_feed_screen": "cctv_feed_monitor"},
        "cctv_focus_monitor": {"cctv_focus_monitor": None, "cctv_focus_screen": "cctv_focus_monitor"},
        "cctv_feed_flag": {"cctv_feed_flag": None, "cctv_feed_flag_text": "cctv_feed_flag"},
        "cctv_focus_flag": {"cctv_focus_flag": None, "cctv_focus_flag_text": "cctv_focus_flag"},
        "cctv_ticker": {"cctv_ticker": None, "cctv_ticker_screen": "cctv_ticker"},
    },
    "cctv_console": {
        "cctv_console": {
            "cctv_console": None,
            "console_body": "cctv_console",
            "console_screen": "cctv_console",
            "console_hazard": "cctv_console",
            "console_kill_button": "cctv_console",
            "console_cover": "cctv_console",
            "console_work_harder": "cctv_console",
            "console_kill_label": "cctv_console",
            "console_work_harder_label": "cctv_console",
            "console_sit_in": "cctv_console",
            "console_sit_in_label": "cctv_console",
        },
    },
    "cctv_sign_plate": {"cctv_sign_plate": {"cctv_sign_plate": None, "cctv_sign_text": "cctv_sign_plate"}},
    "wall_camera": {
        "wall_camera": {"wall_camera": None, "wall_camera_mount": "wall_camera",
                        "wall_camera_head": "wall_camera", "wall_camera_lens": "wall_camera_head"},
    },
    "squad_robot": {
        "squad_robot": {
            "squad_robot": None,
            "squad_pelvis": "squad_robot",
            "squad_torso": "squad_pelvis",
            "squad_head": "squad_torso",
            "squad_arm_upper_l": "squad_torso",
            "squad_arm_lower_l": "squad_arm_upper_l",
            "squad_arm_upper_r": "squad_torso",
            "squad_arm_lower_r": "squad_arm_upper_r",
            "squad_leg_upper_l": "squad_pelvis",
            "squad_leg_lower_l": "squad_leg_upper_l",
            "squad_leg_upper_r": "squad_pelvis",
            "squad_leg_lower_r": "squad_leg_upper_r",
        },
        "squad_charge": {"squad_charge": None, "squad_charge_led": "squad_charge"},
    },
    "wreck_parts": {
        name: {name: None}
        for name in ("wreck_torso", "wreck_head", "wreck_arm", "wreck_leg", "wreck_chair", "wreck_debris",
                     "scorch_floor", "scorch_desk")
    },
    "effects": {
        name: {name: None}
        for name in ("fx_fireball", "fx_smoke_puff", "fx_bolt", "fx_arc", "fx_spark")
    },
}

# The CCTV room's frame and sizes (spec: about 8 x 7 x 4.2 m, a 1.2 x 2.1 m doorway; it widens by
# one feed column at a time, half a column on each side).
WIDTH, DEPTH, HEIGHT = 8.0, 7.0, 4.2
DOOR_W, DOOR_H = 1.2, 2.1
COLUMN = 4.0 / 3.0
MM = 0.002


# ---------------------------------------------------------------------------------------------
# A small glTF reader: nodes with their world matrices, and each mesh's positions.


def _mat(node):
    if "matrix" in node:
        m = node["matrix"]
        return [[m[c * 4 + r] for c in range(4)] for r in range(4)]
    tx, ty, tz = node.get("translation", (0, 0, 0))
    x, y, z, w = node.get("rotation", (0, 0, 0, 1))
    sx, sy, sz = node.get("scale", (1, 1, 1))
    r = [[1 - 2 * (y * y + z * z), 2 * (x * y - z * w), 2 * (x * z + y * w)],
         [2 * (x * y + z * w), 1 - 2 * (x * x + z * z), 2 * (y * z - x * w)],
         [2 * (x * z - y * w), 2 * (y * z + x * w), 1 - 2 * (x * x + y * y)]]
    s = (sx, sy, sz)
    return [[r[i][0] * s[0], r[i][1] * s[1], r[i][2] * s[2], t] for i, t in enumerate((tx, ty, tz))] + [[0, 0, 0, 1]]


def _mul(a, b):
    return [[sum(a[i][k] * b[k][j] for k in range(4)) for j in range(4)] for i in range(4)]


def _apply(m, p):
    return tuple(m[i][0] * p[0] + m[i][1] * p[1] + m[i][2] * p[2] + m[i][3] for i in range(3))


class Glb:
    def __init__(self, path):
        with open(path, "rb") as f:
            magic, version, _ = struct.unpack("<4sII", f.read(12))
            assert magic == b"glTF" and version == 2, path
            n, _ = struct.unpack("<I4s", f.read(8))
            self.doc = json.loads(f.read(n))
            n, _ = struct.unpack("<I4s", f.read(8))
            self.bin = f.read(n)
        self.nodes = self.doc["nodes"]
        self.parent = {}
        for i, nd in enumerate(self.nodes):
            for c in nd.get("children", []):
                self.parent[c] = i
        self.by_name = {nd["name"]: i for i, nd in enumerate(self.nodes)}
        self.scenes = {s["name"]: s["nodes"] for s in self.doc["scenes"]}

    def world(self, i):
        m = _mat(self.nodes[i])
        while i in self.parent:
            i = self.parent[i]
            m = _mul(_mat(self.nodes[i]), m)
        return m

    def subtree(self, i):
        out = [i]
        for c in self.nodes[i].get("children", []):
            out += self.subtree(c)
        return out

    def scene_names(self, scene):
        return {self.nodes[j]["name"] for i in self.scenes[scene] for j in self.subtree(i)}

    def positions(self, name):
        """Every vertex of node `name`'s own mesh, in the scene's frame (glTF: Y up, front +Z)."""
        i = self.by_name[name]
        m = self.world(i)
        out = []
        for prim in self.doc["meshes"][self.nodes[i]["mesh"]]["primitives"]:
            out += [_apply(m, p) for p in self._read(prim["attributes"]["POSITION"])]
        return out

    def triangles(self, name):
        i = self.by_name[name]
        m = self.world(i)
        tris = []
        for prim in self.doc["meshes"][self.nodes[i]["mesh"]]["primitives"]:
            pos = [_apply(m, p) for p in self._read(prim["attributes"]["POSITION"])]
            idx = [k[0] for k in self._read(prim["indices"])]
            tris += [tuple(pos[idx[k + j]] for j in range(3)) for k in range(0, len(idx), 3)]
        return tris

    def surface(self, name, material):
        """Node `name`'s triangles drawn with `material`, each corner as (position, uv), in the
        scene's frame; uv as glTF has it (v down the image)."""
        i = self.by_name[name]
        m = self.world(i)
        tris = []
        for prim in self.doc["meshes"][self.nodes[i]["mesh"]]["primitives"]:
            if self.doc["materials"][prim["material"]]["name"] != material:
                continue
            pos = [_apply(m, p) for p in self._read(prim["attributes"]["POSITION"])]
            uv = self._read(prim["attributes"]["TEXCOORD_0"])
            idx = [k[0] for k in self._read(prim["indices"])]
            tris += [tuple((pos[idx[k + j]], uv[idx[k + j]]) for j in range(3)) for k in range(0, len(idx), 3)]
        return tris

    def _read(self, accessor):
        acc = self.doc["accessors"][accessor]
        view = self.doc["bufferViews"][acc["bufferView"]]
        fmt = {5126: "f", 5125: "I", 5123: "H", 5121: "B"}[acc["componentType"]]
        comps = {"SCALAR": 1, "VEC2": 2, "VEC3": 3, "VEC4": 4}[acc["type"]]
        start = view.get("byteOffset", 0) + acc.get("byteOffset", 0)
        size = struct.calcsize("<" + fmt * comps)
        stride = view.get("byteStride", size)
        return [struct.unpack_from("<" + fmt * comps, self.bin, start + k * stride) for k in range(acc["count"])]

    def bounds(self, name):
        ps = self.positions(name)
        return tuple(min(p[k] for p in ps) for k in range(3)), tuple(max(p[k] for p in ps) for k in range(3))

    def size(self, name):
        lo, hi = self.bounds(name)
        return tuple(h - l for l, h in zip(lo, hi))


def load(piece):
    path = os.path.join(OUT, f"{piece}.glb")
    if not os.path.exists(path):
        raise unittest.SkipTest(f"{path} is not generated; run regenerate.py")
    return Glb(path)


def covered(tris, x, y):
    """Whether any triangle, seen along Z, covers the point (x, y)."""
    for a, b, c in tris:
        d = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1])
        if abs(d) < 1e-12:
            continue
        l1 = ((b[1] - c[1]) * (x - c[0]) + (c[0] - b[0]) * (y - c[1])) / d
        l2 = ((c[1] - a[1]) * (x - c[0]) + (a[0] - c[0]) * (y - c[1])) / d
        if l1 >= 0 and l2 >= 0 and l1 + l2 <= 1:
            return True
    return False


# ---------------------------------------------------------------------------------------------


class TheBuildRegeneratesEveryPiece(unittest.TestCase):
    def test_every_script_that_exports_a_piece_is_in_the_build_and_named_like_its_file(self):
        exported = {}
        for name in sorted(os.listdir(HERE)):
            if name.endswith(".py"):
                with open(os.path.join(HERE, name)) as f:
                    found = re.findall(r'wc\.export\(out, "([a-z_]+)"\)', f.read())
                if found:
                    exported[name[:-3]] = found
        self.assertEqual({k: [k] for k in exported}, exported)
        self.assertEqual(sorted(exported), sorted(regenerate.PIECES))

    def test_every_cctv_piece_is_in_the_build(self):
        for piece in CCTV:
            self.assertIn(piece, regenerate.PIECES)

    def test_the_tests_do_not_change_the_fingerprint(self):
        self.assertIn("tests", regenerate.SKIP_DIRS)


class TheCctvPiecesKeepTheirNames(unittest.TestCase):
    def test_every_scene_has_its_nodes_under_their_parents(self):
        for piece, scenes in CCTV.items():
            glb = load(piece)
            self.assertEqual(sorted(glb.scenes), sorted(scenes), piece)
            for scene, nodes in scenes.items():
                roots = {glb.nodes[i]["name"] for i in glb.scenes[scene]}
                self.assertEqual(roots, {n for n, p in nodes.items() if p is None}, scene)
                self.assertLessEqual(set(nodes), glb.scene_names(scene), scene)
                for node, parent in nodes.items():
                    if parent:
                        got = glb.nodes[glb.parent[glb.by_name[node]]]["name"]
                        self.assertEqual(got, parent, node)


class TheCctvRoomKit(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.kit = load("cctv_room_kit")

    def test_the_room_is_8_by_7_by_4_2_metres_inside(self):
        k = self.kit
        self.assertAlmostEqual(k.bounds("cctv_floor")[1][1], 0.0, delta=MM)
        self.assertAlmostEqual(k.bounds("cctv_ceiling")[0][1], HEIGHT, delta=MM)
        self.assertAlmostEqual(k.bounds("cctv_doorway_wall")[0][2], 0.0, delta=MM)
        self.assertAlmostEqual(k.bounds("cctv_back_wall")[1][2], -DEPTH, delta=MM)
        self.assertAlmostEqual(k.bounds("cctv_side_wall_left")[1][0], -WIDTH / 2, delta=MM)
        self.assertAlmostEqual(k.bounds("cctv_side_wall_right")[0][0], WIDTH / 2, delta=MM)
        for part in ("cctv_floor", "cctv_ceiling", "cctv_doorway_wall", "cctv_back_wall"):
            lo, hi = k.bounds(part)
            self.assertAlmostEqual(lo[0], -WIDTH / 2, delta=MM, msg=part)
            self.assertAlmostEqual(hi[0], WIDTH / 2, delta=MM, msg=part)
        for part in ("cctv_floor", "cctv_ceiling"):
            lo, hi = k.bounds(part)
            self.assertAlmostEqual(hi[2], 0.0, delta=MM, msg=part)
            self.assertAlmostEqual(lo[2], -DEPTH, delta=MM, msg=part)

    def test_the_doorway_is_1_2_by_2_1_metres_and_centred(self):
        tris = self.kit.triangles("cctv_doorway_wall")
        e = 0.02
        for x, y in ((0, 1.0), (-DOOR_W / 2 + e, e), (DOOR_W / 2 - e, e), (-DOOR_W / 2 + e, DOOR_H - e),
                     (DOOR_W / 2 - e, DOOR_H - e)):
            self.assertFalse(covered(tris, x, y), (x, y))
        for x, y in ((-DOOR_W / 2 - e, 1.0), (DOOR_W / 2 + e, 1.0), (0, DOOR_H + e), (-3.0, 3.0), (3.0, 0.5)):
            self.assertTrue(covered(tris, x, y), (x, y))

    def test_side_walls_close_the_corners_and_span_the_height(self):
        for side in ("left", "right"):
            lo, hi = self.kit.bounds(f"cctv_side_wall_{side}")
            self.assertLessEqual(lo[1], 0.0)
            self.assertGreaterEqual(hi[1], HEIGHT)
            self.assertGreaterEqual(hi[2], 0.0)
            self.assertLessEqual(lo[2], -DEPTH)

    def test_an_extension_is_half_a_column_wide_and_as_deep_and_high_as_the_room(self):
        lo, hi = self.kit.bounds("cctv_extension")
        self.assertAlmostEqual(lo[0], 0.0, delta=MM)
        self.assertAlmostEqual(hi[0], COLUMN / 2, delta=MM)
        self.assertLessEqual(lo[1], 0.0)
        self.assertGreaterEqual(hi[1], HEIGHT)
        self.assertGreaterEqual(hi[2], 0.0)
        self.assertLessEqual(lo[2], -DEPTH)

    def test_lights_stay_inside_the_room(self):
        for part in ("cctv_doorway_wall_lights", "cctv_back_wall_lights", "cctv_side_wall_left_lights",
                     "cctv_side_wall_right_lights"):
            lo, hi = self.kit.bounds(part)
            self.assertGreaterEqual(lo[0], -WIDTH / 2 - MM, part)
            self.assertLessEqual(hi[0], WIDTH / 2 + MM, part)
            self.assertGreaterEqual(lo[1], -MM, part)
            self.assertLessEqual(hi[1], HEIGHT + MM, part)
            self.assertLessEqual(hi[2], MM, part)
            self.assertGreaterEqual(lo[2], -DEPTH - MM, part)


class TheMonitorsFitTheWall(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.kit = load("cctv_monitors")

    def test_screens_are_16_by_9_and_face_the_room(self):
        for screen in ("cctv_feed_screen", "cctv_focus_screen"):
            w, h, d = self.kit.size(screen)
            self.assertAlmostEqual(w / h, 16 / 9, delta=0.01, msg=screen)
            self.assertLess(d, 0.01, screen)

    def test_screens_sit_on_the_front_of_their_monitors(self):
        # Every part but the first lives in a scene of its own; its children must keep their place.
        for monitor, screen in (("cctv_feed_monitor", "cctv_feed_screen"), ("cctv_focus_monitor", "cctv_focus_screen"),
                                ("cctv_ticker", "cctv_ticker_screen")):
            self.assertAlmostEqual(self.kit.bounds(screen)[0][2], self.kit.bounds(monitor)[1][2], delta=MM, msg=screen)

    def test_focus_monitors_are_larger_than_feeds(self):
        self.assertGreater(self.kit.size("cctv_focus_screen")[0], self.kit.size("cctv_feed_screen")[0] * 1.1)

    def test_a_feed_monitor_with_its_flag_frame_fits_its_column(self):
        self.assertLess(self.kit.size("cctv_feed_flag")[0], COLUMN)

    def test_three_rows_two_focus_monitors_and_the_ticker_fit_under_the_ceiling(self):
        k = self.kit
        stack = 3 * k.size("cctv_feed_monitor")[1] + k.size("cctv_focus_monitor")[1] + k.size("cctv_ticker")[1]
        self.assertLess(stack, HEIGHT - 0.6)

    def test_two_focus_monitors_side_by_side_fit_four_columns(self):
        self.assertLess(2 * self.kit.size("cctv_focus_flag")[0], 4 * COLUMN)


class TheConsole(unittest.TestCase):
    def test_it_is_a_low_slab(self):
        w, h, d = load("cctv_console").size("console_body")
        self.assertGreater(w, 2.0)
        self.assertLess(h, 1.0)
        self.assertLess(d, 1.0)

    def test_the_cover_closes_over_the_button(self):
        k = load("cctv_console")
        (clo, chi), (blo, bhi) = k.bounds("console_cover"), k.bounds("console_kill_button")
        for a in (0, 2):
            self.assertLessEqual(clo[a], blo[a])
            self.assertGreaterEqual(chi[a], bhi[a])
        self.assertGreaterEqual(chi[1], bhi[1])

    def test_sit_in_is_a_cyan_bar_on_the_slope_between_the_glass_and_kill(self):
        # Agent-screens spec, "SIT IN on the CCTV console"; designs/console-bar.png.
        k = load("cctv_console")
        lo, hi = k.bounds("console_sit_in")
        self.assertGreater(lo[0], k.bounds("console_screen")[1][0])
        self.assertLess(hi[0], k.bounds("console_hazard")[0][0])
        # On the slope in the same row as WORK HARDER, which is as deep and as tall.
        for got, want in zip((lo, hi), k.bounds("console_work_harder")):
            for a in (1, 2):
                self.assertAlmostEqual(got[a], want[a], delta=0.001)
        self.assertTrue(k.surface("console_sit_in", "sit_in"))

    def test_sit_ins_label_is_under_its_bar_in_the_row_of_the_other_labels(self):
        k = load("cctv_console")
        at = _apply(k.world(k.by_name["console_sit_in_label"]), (0, 0, 0))
        kill = _apply(k.world(k.by_name["console_kill_label"]), (0, 0, 0))
        lo, hi = k.bounds("console_sit_in")
        self.assertAlmostEqual(at[0], (lo[0] + hi[0]) / 2, delta=0.001)
        self.assertAlmostEqual(at[1], kill[1], delta=1e-4)
        self.assertAlmostEqual(at[2], kill[2], delta=1e-4)
        # Its 0.20 m of text clears KILL's 0.36 m and the glass.
        self.assertLessEqual(at[0] + 0.10, kill[0] - 0.18 + MM)
        self.assertGreaterEqual(at[0] - 0.10, k.bounds("console_screen")[1][0] - MM)

    def test_the_other_parts_and_labels_stay_where_they_were(self):
        # Measured from the console before SIT IN.
        k = load("cctv_console")
        before = {"console_body": ((-1.3, 0.0, -0.407), (1.3, 0.953, 0.407)),
                  "console_screen": ((-1.22, 0.8129, -0.3175), (0.02, 0.9385, 0.3319)),
                  "console_hazard": ((0.3, 0.8498, -0.1602), (0.6, 0.908, 0.1352)),
                  "console_kill_button": ((0.345, 0.8581, -0.116), (0.555, 0.9393, 0.0926)),
                  "console_cover": ((0.34, 0.8571, -0.1262), (0.56, 0.9661, 0.1271)),
                  "console_work_harder": ((0.7, 0.8623, -0.0958), (1.14, 0.9341, 0.0724))}
        for part, box in before.items():
            for got, want in zip(k.bounds(part), box):
                for g, w in zip(got, want):
                    self.assertAlmostEqual(g, w, delta=0.0005, msg=part)
        for label, want in (("console_kill_label", (0.45, 0.8249, 0.2724)),
                            ("console_work_harder_label", (0.92, 0.8249, 0.2724))):
            for g, w in zip(_apply(k.world(k.by_name[label]), (0, 0, 0)), want):
                self.assertAlmostEqual(g, w, delta=0.0005, msg=label)


class TheWallCamera(unittest.TestCase):
    def test_its_lens_looks_into_the_room_and_down(self):
        k = load("wall_camera")
        m = k.world(k.by_name["wall_camera_lens"])
        forward = tuple(-m[i][2] for i in range(3))  # the lens node's -Z: where a camera on it looks
        up = tuple(m[i][1] for i in range(3))
        self.assertLess(forward[2], -0.8)  # into the room, glTF -Z
        self.assertLess(forward[1], -0.2)  # and down
        self.assertGreater(up[1], 0.8)
        self.assertLess(abs(forward[0]), 1e-6)


class TheSquadRobot(unittest.TestCase):
    def test_it_stands_about_1_8_metres_on_its_origin(self):
        k = load("squad_robot")
        tops = [p[1] for n in k.scene_names("squad_robot") if "mesh" in k.nodes[k.by_name[n]]
                for p in k.positions(n)]
        self.assertAlmostEqual(min(tops), 0.0, delta=0.01)
        self.assertTrue(1.7 < max(tops) < 1.95, max(tops))

    def test_limbs_hang_from_their_joints(self):
        k = load("squad_robot")
        for limb in ("arm_upper", "arm_lower", "leg_upper", "leg_lower"):
            for side in ("l", "r"):
                name = f"squad_{limb}_{side}"
                joint = _apply(k.world(k.by_name[name]), (0, 0, 0))
                lo, hi = k.bounds(name)
                self.assertGreater(joint[1], lo[1] + 0.2, name)  # the limb hangs below its joint
                self.assertLess(abs(joint[1] - hi[1]), 0.12, name)

    def test_left_is_the_robots_own_left(self):
        k = load("squad_robot")
        # The robot faces +Z (glTF), so its own left is +X.
        self.assertGreater(_apply(k.world(k.by_name["squad_arm_upper_l"]), (0, 0, 0))[0], 0.1)
        self.assertLess(_apply(k.world(k.by_name["squad_arm_upper_r"]), (0, 0, 0))[0], -0.1)


class TheWreck(unittest.TestCase):
    def test_parts_rest_on_the_floor_at_their_origin(self):
        k = load("wreck_parts")
        for part in ("wreck_torso", "wreck_head", "wreck_arm", "wreck_leg", "wreck_chair", "scorch_floor"):
            lo, hi = k.bounds(part)
            self.assertAlmostEqual(lo[1], 0.0, delta=0.006, msg=part)
            self.assertLess(abs((lo[0] + hi[0]) / 2), 0.3, part)

    def test_scorch_marks_are_flat(self):
        k = load("wreck_parts")
        self.assertLess(k.size("scorch_floor")[1], 0.01)
        self.assertLess(k.size("scorch_desk")[1], 0.01)
        self.assertTrue(1.6 < k.size("scorch_floor")[0] < 2.4)


class TheEffects(unittest.TestCase):
    def test_the_bolt_hangs_one_metre_down_from_its_origin(self):
        lo, hi = load("effects").bounds("fx_bolt")
        self.assertAlmostEqual(hi[1], 0.0, delta=0.03)
        self.assertAlmostEqual(lo[1], -1.0, delta=0.03)

    def test_round_effects_have_a_radius_of_about_one_metre(self):
        for name in ("fx_fireball", "fx_smoke_puff"):
            w, h, d = load("effects").size(name)
            self.assertTrue(1.7 < w < 2.4 and 1.7 < h < 2.4, (name, w, h))


class TheDeskSet(unittest.TestCase):
    """Screen B replaces the CRT (agent-screens spec, "The PC screen" and "Assets")."""

    @classmethod
    def setUpClass(cls):
        cls.kit = load("desk_set")

    def test_its_parts_are_the_desk_chair_pc_keyboard_tower_mouse_and_the_seat(self):
        k = self.kit
        self.assertEqual(sorted(k.scenes), ["desk_set"])
        self.assertEqual({k.nodes[i]["name"] for i in k.scenes["desk_set"]}, {"desk_set"})
        children = {k.nodes[c]["name"] for c in k.nodes[k.by_name["desk_set"]]["children"]}
        self.assertEqual(children, {"desk", "chair", "pc", "keyboard", "tower", "mouse", "robot_seat"})

    def glass(self):
        """The glass's corners, each (position, uv)."""
        return {c for t in self.kit.surface("pc", "pc_screen") for c in t}

    def test_the_glass_is_one_5_by_3_quad_of_its_own_material(self):
        self.assertEqual(len(self.kit.surface("pc", "pc_screen")), 2)
        corners = [p for p, _ in self.glass()]
        self.assertEqual(len(corners), 4)
        sides = sorted(math.dist(corners[0], c) for c in corners[1:])
        self.assertAlmostEqual(sides[0], 0.294, delta=0.001)
        self.assertAlmostEqual(sides[1], 0.49, delta=0.001)

    def test_the_glass_faces_a_full_size_robots_eyes_squarely(self):
        # floor_plan.rs: a full-size robot's face is at y 1.10, z 0.07 from the desk set's origin.
        eye = (0.0, 1.10, 0.07)
        (a, _), (b, _), (c, _) = self.kit.surface("pc", "pc_screen")[0]
        u, v = [q - p for p, q in zip(a, b)], [q - p for p, q in zip(a, c)]
        normal = (u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0])
        centre = [sum(p[k] for p, _ in self.glass()) / 4 for k in range(3)]
        to_eye = [e - c for e, c in zip(eye, centre)]
        cos = sum(n * t for n, t in zip(normal, to_eye)) / (math.hypot(*normal) * math.hypot(*to_eye))
        self.assertGreater(cos, math.cos(math.radians(1)))
        self.assertGreater(normal[1] / math.hypot(*normal), 0.05)  # tilted up towards the eyes, not upright

    def test_the_image_fills_the_glass_upright_and_unmirrored_for_the_robot(self):
        # The robot faces +Z, so its left is +X; glTF's v runs down the image.
        corners = sorted(self.glass(), key=lambda c: (c[0][1], c[0][0]))
        bottom_right, bottom_left, top_right, top_left = corners
        for (_, uv), want in ((top_left, (0, 0)), (top_right, (1, 0)), (bottom_left, (0, 1)), (bottom_right, (1, 1))):
            self.assertAlmostEqual(uv[0], want[0], delta=1e-4)
            self.assertAlmostEqual(uv[1], want[1], delta=1e-4)

    def test_the_other_parts_and_the_seat_stay_where_they_were(self):
        # Measured from the desk set with the CRT, before screen B.
        before = {"desk": ((-0.6, 0.0, 0.27), (0.6, 0.76, 0.87)),
                  "chair": ((-0.2017, -0.0067, -0.252), (0.2017, 0.9208, 0.2187)),
                  "keyboard": ((-0.2, 0.76, 0.3), (0.2, 0.789, 0.44)),
                  "tower": ((-0.51, 0.0, 0.4155), (-0.33, 0.4, 0.82)),
                  "mouse": ((-0.31, 0.759, 0.32), (-0.25, 0.79, 0.6203))}
        for part, box in before.items():
            for got, want in zip(self.kit.bounds(part), box):
                for g, w in zip(got, want):
                    self.assertAlmostEqual(g, w, delta=0.0005, msg=part)
        seat = _apply(self.kit.world(self.kit.by_name["robot_seat"]), (0, 0, 0))
        for g, w in zip(seat, (0.0, 0.46, 0.0)):
            self.assertAlmostEqual(g, w, delta=1e-6)

    def test_the_pc_stands_on_the_desk_behind_the_keyboard_inside_the_desk_sets_footprint(self):
        lo, hi = self.kit.bounds("pc")
        self.assertAlmostEqual(lo[1], self.kit.bounds("desk")[1][1], delta=0.001)
        self.assertGreater(lo[2], self.kit.bounds("keyboard")[1][2])
        # floor_plan.rs: DESK_SET_X and DESK_SET_Z.
        self.assertGreaterEqual(lo[0], -0.6)
        self.assertLessEqual(hi[0], 0.6)
        self.assertGreaterEqual(lo[2], -0.25)
        self.assertLessEqual(hi[2], 1.05)


if __name__ == "__main__":
    unittest.main()


class TheCorridorsFarWall(unittest.TestCase):
    """Each segment's far wall is a part of its own, open for the CCTV doorway in its middle or at
    either end, in the corridor's frame: the far wall at z 2.4..2.6."""

    @classmethod
    def setUpClass(cls):
        cls.kit = load("corridor_kit")

    def far(self, part):
        """The part's triangles at the far wall (z 2.4 and beyond)."""
        return [t for t in self.kit.triangles(part) if min(p[2] for p in t) > 2.39]

    def test_segments_leave_the_far_wall_to_its_own_parts(self):
        for part in ("corridor_segment", "corridor_segment_plain"):
            self.assertFalse(any(covered(self.far(part), x, 1.5) for x in (0.3, 1.25, 2.2)), part)
        self.assertTrue(covered(self.far("corridor_end_start"), -0.4, 1.5))
        self.assertTrue(covered(self.far("corridor_end_far"), 0.4, 1.5))

    def test_each_far_wall_part_is_open_where_the_cctv_doorway_falls(self):
        e = 0.02
        for part, (left, right) in (("corridor_far_wall", (None, None)),
                                    ("corridor_far_wall_doorway", (0.65, 1.85)),
                                    ("corridor_far_wall_doorway_end", (1.9, 2.5)),
                                    ("corridor_far_wall_doorway_start", (0.0, 0.6))):
            tris = self.kit.triangles(part)
            lo, hi = self.kit.bounds(part)
            self.assertAlmostEqual(lo[0], 0.0, delta=MM, msg=part)
            self.assertAlmostEqual(hi[0], 2.5, delta=MM, msg=part)
            self.assertAlmostEqual(lo[2], 2.4, delta=MM, msg=part)
            self.assertAlmostEqual(hi[2], 2.6, delta=MM, msg=part)
            for x in (0.3, 0.9, 1.25, 1.6, 2.2):
                open_here = left is not None and left < x < right
                self.assertEqual(covered(tris, x, 1.0), not open_here, (part, x))
            if left is not None:
                middle = (left + right) / 2
                self.assertTrue(covered(tris, middle, DOOR_H + e), part)
                self.assertFalse(covered(tris, middle, DOOR_H - e), part)
