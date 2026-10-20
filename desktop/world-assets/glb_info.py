"""Print a .glb's scenes, nodes (with extras), materials, images and extensions. Plain Python 3, no Blender.
Run: python3 glb_info.py <file.glb> ...
"""
import json
import struct
import sys


def read(path):
    with open(path, "rb") as f:
        magic, version, _ = struct.unpack("<4sII", f.read(12))
        assert magic == b"glTF" and version == 2, path
        n, kind = struct.unpack("<I4s", f.read(8))
        assert kind == b"JSON"
        return json.loads(f.read(n))


for path in sys.argv[1:]:
    doc = read(path)
    print(f"== {path}")
    nodes = doc.get("nodes", [])
    for s in doc.get("scenes", []):
        print(f"scene {s.get('name')}: roots {[nodes[i]['name'] for i in s['nodes']]}")
    for nd in nodes:
        extra = f" extras={nd['extras']}" if "extras" in nd else ""
        print(f"  node {nd['name']} mesh={nd.get('mesh')} children={[nodes[c]['name'] for c in nd.get('children', [])]}"
              f" t={nd.get('translation')} s={nd.get('scale')}{extra}")
    print("materials:", [m["name"] for m in doc.get("materials", [])])
    print("images:", [i.get("uri") for i in doc.get("images", [])])
    print("extensionsUsed:", doc.get("extensionsUsed", []))
