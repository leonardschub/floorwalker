"""Shared helpers for the world-asset scripts: arguments, building blocks, UV mapping and export.

Every piece script runs in headless Blender and writes `<out>/<piece>.glb`, baking the textures it
needs into `<out>/textures/` first (see wa_materials.py and README.md).
"""
import json
import math
import os
import shutil
import struct
import sys

import bmesh
import bpy
from mathutils import Matrix, Vector

import wa_materials as wm


def out_dir():
    """The output folder: the first argument after `--`."""
    argv = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []
    if not argv:
        sys.exit("usage: blender -b --factory-startup --python <piece>.py -- <out-dir> [--rebake]")
    os.makedirs(argv[0], exist_ok=True)
    return os.path.abspath(argv[0])


def rebake():
    return "--rebake" in sys.argv


def tex_dir(out):
    return os.path.join(out, "textures")


def start(recipes):
    """Empty the file, bake (or reuse) the textures for `recipes`, return (out, materials by name)."""
    out = out_dir()
    bpy.ops.wm.read_factory_settings(use_empty=True)
    td = tex_dir(out)
    wm.ensure_baked(recipes, td, force=rebake())
    return out, {name: wm.surface(name, td) for name in recipes}


# ---------------------------------------------------------------------------------------------
# Building blocks. Each returns a mesh object with its transform applied, so its vertices are in
# the piece's own frame (metres, Blender Z up; the piece's front faces -Y, which is glTF +Z).


def apply(o):
    bpy.ops.object.select_all(action="DESELECT")
    o.select_set(True)
    bpy.context.view_layer.objects.active = o
    for mod in list(o.modifiers):
        bpy.ops.object.modifier_apply(modifier=mod.name)
    bpy.ops.object.transform_apply(location=True, rotation=True, scale=True)
    return o


def _bevel(o, width, segments=2):
    if width:
        b = o.modifiers.new("bevel", "BEVEL")
        b.width, b.segments = width, segments
        b.limit_method = "ANGLE"
    return o


def box(size, loc, mat, rot=(0, 0, 0), bevel=0.0):
    bpy.ops.mesh.primitive_cube_add(size=1, location=loc, rotation=rot)
    o = bpy.context.object
    o.scale = size
    o.data.materials.append(mat)
    bpy.ops.object.transform_apply(scale=True)
    return apply(_bevel(o, bevel))


def box_between(lo, hi, mat, bevel=0.0):
    """An axis-aligned box from corner `lo` to corner `hi`."""
    size = tuple(h - l for l, h in zip(lo, hi))
    return box(size, tuple((l + h) / 2 for l, h in zip(lo, hi)), mat, bevel=bevel)


def cylinder(r, depth, loc, mat, rot=(0, 0, 0), vertices=16, smooth=True):
    bpy.ops.mesh.primitive_cylinder_add(radius=r, depth=depth, vertices=vertices, location=loc, rotation=rot)
    o = bpy.context.object
    o.data.materials.append(mat)
    if smooth:
        _smooth_sides(o)
    return apply(o)


def rod(a, b, r, mat, vertices=14):
    """A cylinder from point a to point b."""
    a, b = Vector(a), Vector(b)
    d = b - a
    bpy.ops.mesh.primitive_cylinder_add(radius=r, depth=d.length, vertices=vertices, location=(a + b) / 2)
    o = bpy.context.object
    o.rotation_mode = "QUATERNION"
    o.rotation_quaternion = d.to_track_quat("Z", "Y")
    o.data.materials.append(mat)
    _smooth_sides(o)
    return apply(o)


def ball(r, loc, mat, segments=16, rings=10):
    bpy.ops.mesh.primitive_uv_sphere_add(radius=r, location=loc, segments=segments, ring_count=rings)
    o = bpy.context.object
    o.data.materials.append(mat)
    for p in o.data.polygons:
        p.use_smooth = True
    return apply(o)


def _smooth_sides(o):
    """Smooth the round sides of a cylinder, keep its caps flat."""
    for p in o.data.polygons:
        p.use_smooth = abs(p.normal.z) < 0.5


def tube_path(points, r, mat, sides=8):
    """A smooth round tube through `points` (a cable)."""
    bm = bmesh.new()
    pts = [Vector(p) for p in points]
    rings = []
    for i, p in enumerate(pts):
        t = (pts[min(i + 1, len(pts) - 1)] - pts[max(i - 1, 0)]).normalized()
        side = t.cross(Vector((0, 0, 1)))
        if side.length < 1e-6:
            side = Vector((1, 0, 0))
        side.normalize()
        up = side.cross(t).normalized()
        rings.append([bm.verts.new(p + (side * math.cos(a) + up * math.sin(a)) * r)
                      for a in (k * math.tau / sides for k in range(sides))])
    for ra, rb in zip(rings, rings[1:]):
        for k in range(sides):
            f = bm.faces.new((ra[k], ra[(k + 1) % sides], rb[(k + 1) % sides], rb[k]))
            f.smooth = True
    bm.faces.new(list(reversed(rings[0])))
    bm.faces.new(rings[-1])
    me = bpy.data.meshes.new("tube")
    bm.to_mesh(me)
    bm.free()
    o = bpy.data.objects.new("tube", me)
    bpy.context.scene.collection.objects.link(o)
    me.materials.append(mat)
    return o


def jitter(o, rnd, amount):
    """Push every vertex in or out along its normal by up to `amount` (crumpling, dents)."""
    for v in o.data.vertices:
        v.co += v.normal * rnd.uniform(-amount, amount)
    return o


def ico(r, loc, mat, subdivisions=2):
    bpy.ops.mesh.primitive_ico_sphere_add(radius=r, subdivisions=subdivisions, location=loc)
    o = bpy.context.object
    o.data.materials.append(mat)
    return apply(o)


def clutter(parts, item):
    """Join a clutter item into its single tagged object and export it as clutter_<item>.glb."""
    name = f"clutter_{item}"
    o = join(parts, name)
    uv_world(o)
    tag_clutter(o)
    part_scene(name, [o])
    return o


def join(parts, name):
    """Join `parts` into one mesh object called `name` (origin at the piece origin)."""
    bpy.ops.object.select_all(action="DESELECT")
    for p in parts:
        p.select_set(True)
    bpy.context.view_layer.objects.active = parts[0]
    if len(parts) > 1:
        bpy.ops.object.join()
    o = bpy.context.object
    o.name = name
    o.data.name = name
    return o


def empty(name, loc=(0, 0, 0), parent=None):
    o = bpy.data.objects.new(name, None)
    o.location = loc
    o.empty_display_size = 0.1
    bpy.context.scene.collection.objects.link(o)
    if parent:
        o.parent = parent
    return o


def parent(child, root):
    """Parent keeping the child's world position (roots sit at their own origin)."""
    bpy.context.view_layer.update()  # so a root moved since the last evaluation is where it shows
    child.parent = root
    child.matrix_parent_inverse = root.matrix_world.inverted()


def move_origin(o, point):
    """Put the object's origin at `point` (piece frame) without moving its geometry."""
    p = Vector(point)
    o.data.transform(Matrix.Translation(-p))
    o.location = o.location + p


# ---------------------------------------------------------------------------------------------
# UV mapping at real-world scale


def uv_world(o, scale=(1.0, 1.0, 1.0), offset=(0.0, 0.0, 0.0)):
    """Box-project UVs from the piece-frame position, divided by each material's tile size.

    A face looking along Z gets (x, y), along X gets (y, z), along Y gets (x, z). Coordinates are
    first multiplied by `scale` and shifted by `offset`, so a piece can line its texture up with the
    pieces it tiles against (the corridor maps its 2.5 m segment onto a 2.4 m tile).
    """
    mw = o.matrix_world
    me = o.data
    if not me.uv_layers:
        me.uv_layers.new(name="UVMap")
    uv = me.uv_layers.active.data
    tiles = [wm.tile_of(m.name) if m else (1.0, 1.0) for m in me.materials]
    for p in me.polygons:
        n = (mw.to_3x3() @ p.normal)
        ax = max(range(3), key=lambda i: abs(n[i]))
        tu, tv = tiles[p.material_index] if tiles else (1.0, 1.0)
        for li in p.loop_indices:
            w = mw @ me.vertices[me.loops[li].vertex_index].co
            c = [w[i] * scale[i] + offset[i] for i in range(3)]
            a, b = {2: (c[0], c[1]), 0: (c[1], c[2]), 1: (c[0], c[2])}[ax]
            uv[li].uv = (a / tu, b / tv)


def uv_fit(o, mat_name, front=None, rest=(0.02, 0.02)):
    """Map the faces with material `mat_name` so their bounding box fills the 0..1 square (screens).

    Without `front`, each face fills the square on its own. With `front` (the outward normal of the
    face that shows the image, in the mesh's frame), only the faces turned that way are mapped, all
    to their common bounding box, upright and unmirrored as seen from in front (u to the viewer's
    right, v up Z, or up Y for a face looking along Z), however the face is tilted; every other face
    of the material (the sides and bevels) gets the single UV `rest`, a blank corner of the image, so
    the image never shows squashed on the edges.
    """
    me = o.data
    idx = [i for i, m in enumerate(me.materials) if m and m.name == mat_name]
    uv = me.uv_layers.active.data
    polys = [p for p in me.polygons if p.material_index in idx]
    if front is None:
        groups = [[p] for p in polys]
    else:
        front = Vector(front).normalized()
        shown = [p for p in polys if p.normal.dot(front) > 0.99]
        for p in polys:
            if p not in shown:
                for li in p.loop_indices:
                    uv[li].uv = rest
        groups = [shown] if shown else []
    for group in groups:
        if front is None:
            ax = max(range(3), key=lambda i: abs(group[0].normal[i]))
            axes = [Vector([float(i == k) for i in range(3)]) for k in range(3) if k != ax]
        else:
            up = Vector((0, 1, 0)) if abs(front.z) > 0.99 else Vector((0, 0, 1))
            right = up.cross(front).normalized()
            axes = [right, front.cross(right)]
        cos = [me.vertices[me.loops[li].vertex_index].co for p in group for li in p.loop_indices]
        lo = [min(c.dot(a) for c in cos) for a in axes]
        hi = [max(c.dot(a) for c in cos) for a in axes]
        for p in group:
            for li in p.loop_indices:
                c = me.vertices[me.loops[li].vertex_index].co
                uv[li].uv = tuple((c.dot(a) - lo[j]) / max(hi[j] - lo[j], 1e-9) for j, a in enumerate(axes))


# ---------------------------------------------------------------------------------------------
# Scenes and export


def part_scene(name, objs):
    """Make `objs` (and their children) one glTF scene called `name`.

    Everything is built in the start scene; the first part keeps it (renamed), later parts move to
    new scenes. So call this right after building each part, in order.
    """
    # The exporter reads each object's world matrix as last evaluated, and an object moved to a scene
    # that is not the active one is never evaluated again: so evaluate now, with every transform set.
    bpy.context.view_layer.update()
    first = bpy.data.scenes[0]
    if not first.get("used"):
        first.name = name
        first["used"] = True
        return first
    sc = bpy.data.scenes.new(name)
    sc["used"] = True
    stack = list(objs)
    while stack:
        o = stack.pop()
        for c in o.users_collection:
            c.objects.unlink(o)
        sc.collection.objects.link(o)
        stack.extend(o.children)
    return sc


def tag_clutter(o):
    """Clutter, and only clutter, carries extras {"clutter": true} and a `clutter_` name prefix."""
    assert o.name.startswith("clutter_"), o.name
    o["clutter"] = True


def _canonical_triangles(doc, binary):
    """Put every primitive's triangles in one fixed order.

    The exporter emits the same triangles in a varying order from run to run (seen on UV spheres).
    Each triangle is rotated to start at its smallest index (keeping its winding), then the list is
    sorted, so the bytes only depend on the geometry.
    """
    import numpy as np

    dtypes = {5121: np.uint8, 5123: np.uint16, 5125: np.uint32}
    for mesh in doc.get("meshes", []):
        for prim in mesh["primitives"]:
            if prim.get("mode", 4) != 4 or "indices" not in prim:
                continue
            acc = doc["accessors"][prim["indices"]]
            view = doc["bufferViews"][acc["bufferView"]]
            dt = dtypes[acc["componentType"]]
            start = view.get("byteOffset", 0) + acc.get("byteOffset", 0)
            size = acc["count"] * np.dtype(dt).itemsize
            tris = np.frombuffer(bytes(binary[start:start + size]), dt).reshape(-1, 3)
            shift = np.argmin(tris, axis=1)
            rows = np.arange(len(tris))[:, None]
            rolled = tris[rows, (np.arange(3)[None, :] + shift[:, None]) % 3]
            rolled = rolled[np.lexsort(rolled.T[::-1])]
            binary[start:start + size] = rolled.astype(dt).tobytes()
    return bytes(binary)


def export(out, piece):
    """Export every scene to `<out>/<piece>.glb`; its images point at the shared `textures/` PNGs."""
    for sc in bpy.data.scenes:
        if "used" in sc:
            del sc["used"]
    for o in bpy.data.objects:  # split ngons ourselves, the same way every run
        if o.type == "MESH" and not any(m.type == "TRIANGULATE" for m in o.modifiers):
            o.modifiers.new("ngons", "TRIANGULATE").min_vertices = 5
    tmp = os.path.join(out, f".export-{piece}")
    shutil.rmtree(tmp, ignore_errors=True)
    os.makedirs(tmp)
    gltf_path = os.path.join(tmp, f"{piece}.gltf")
    bpy.ops.export_scene.gltf(
        filepath=gltf_path,
        export_format="GLTF_SEPARATE",
        export_texture_dir="img",
        export_image_format="AUTO",
        export_image_add_webp=False,
        export_draco_mesh_compression_enable=False,
        export_gpu_instances=False,
        export_yup=True,
        export_apply=True,
        export_extras=True,
        export_tangents=False,  # Bevy computes them (mikktspace); Blender's vary in the last bit run to run
        export_lights=False,
        export_cameras=False,
        export_animations=False,
        use_active_scene=False,
        use_selection=False,
        use_visible=False,
        export_unused_images=False,
    )
    with open(gltf_path) as f:
        doc = json.load(f)
    with open(os.path.join(tmp, doc["buffers"][0]["uri"]), "rb") as f:
        binary = f.read()
    shutil.rmtree(tmp)

    binary = _canonical_triangles(doc, bytearray(binary))

    # point every image at the shared baked texture of the same name
    for img in doc.get("images", []):
        path = os.path.join(tex_dir(out), img["name"] + ".png")
        assert os.path.exists(path), f"{piece}: image {img['name']} is not a baked texture"
        img["uri"] = "textures/" + img["name"] + ".png"
        img.pop("mimeType", None)
    assert len(doc["buffers"]) == 1
    del doc["buffers"][0]["uri"]
    doc["buffers"][0]["byteLength"] = len(binary)
    for banned in ("KHR_draco_mesh_compression", "EXT_meshopt_compression", "EXT_mesh_gpu_instancing",
                   "EXT_texture_webp"):
        assert banned not in doc.get("extensionsUsed", []), banned

    js = json.dumps(doc, separators=(",", ":"), ensure_ascii=False).encode()
    js += b" " * (-len(js) % 4)
    binary += b"\0" * (-len(binary) % 4)
    total = 12 + 8 + len(js) + 8 + len(binary)
    path = os.path.join(out, f"{piece}.glb")
    with open(path, "wb") as f:
        f.write(struct.pack("<4sII", b"glTF", 2, total))
        f.write(struct.pack("<I4s", len(js), b"JSON") + js)
        f.write(struct.pack("<I4s", len(binary), b"BIN\0") + binary)
    print(f"EXPORTED {path}")
    return path
