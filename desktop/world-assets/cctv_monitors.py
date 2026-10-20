"""The CCTV room's screens: feed and focus monitors, their flag frames and the ticker -> cctv_monitors.glb.

Thin-bezel black monitors (direction A, branch `prototype/cctv-look`). Every part's origin is the
centre of its back, which mounts on the back wall (or wherever the world puts it on the curved wall);
it faces -Y (glTF +Z, towards the doorway). A monitor is 0.07 m deep; its screen's face is 0.071 m in
front of the origin.

  cctv_feed_monitor    a wall feed: bezel 1.236 x 0.711 m > `cctv_feed_screen`, the 1.20 x 0.675 m
                       (16:9) screen
  cctv_focus_monitor   a focus monitor: bezel 1.58 x 0.915 m > `cctv_focus_screen`, 1.52 x 0.855 m
  cctv_feed_flag       a flagged feed's glowing frame (1.292 m wide, fits a 4/3 m column) and its tag
                       bar over the top edge; `cctv_feed_flag_text` marks the centre of the tag bar's
                       front, where the flag's words go (0.86 x 0.045 m usable). Spawned with the same
                       transform as the monitor it frames.
  cctv_focus_flag      the same for a focus monitor (tag text 1.1 x 0.06 m usable)
  cctv_ticker          the slim bar under the wall, 5.2 x 0.14 m > `cctv_ticker_screen`, 5.1 x 0.09 m

Screens use the material `cctv_screen` (a dark, faintly cyan glass) with UVs filling 0..1 on their
front only, so the world can give each screen its own feed or text image; their sides and bevels sit
on one corner of it. Frames and tag bars use `cctv_flag`, a white glow the world tints with the flag's
colour (amber for waiting on you, red for asking you something).
Run: blender -b --factory-startup --python cctv_monitors.py -- <out-dir> [--rebake]
"""
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import wa_common as wc  # noqa: E402
import wa_materials as wm  # noqa: E402

out, _ = wc.start([])
bezel = wm.plain("monitor_bezel", (0.018, 0.018, 0.022), 0.28, 0.6)
screen = wm.plain("cctv_screen", (0.01, 0.012, 0.015), 0.08, emit=(0.35, 0.75, 0.85), strength=0.15)
flag = wm.plain("cctv_flag", (1.0, 1.0, 1.0), 0.4, emit=(1.0, 1.0, 1.0), strength=3.0)

BODY, FACE = 0.04, 0.03  # body depth behind the bezel, bezel depth
FRONT = BODY + FACE  # 0.07: the bezel's front face


def monitor(name, w, h, border):
    """A monitor whose screen is w x h, with a bezel `border` wide on every side."""
    bw, bh = w + 2 * border, h + 2 * border
    body = wc.join([
        wc.box((bw - 0.06, BODY, bh - 0.06), (0, -BODY / 2, 0), bezel, bevel=0.01),
        wc.box((bw, FACE, bh), (0, -BODY - FACE / 2, 0), bezel, bevel=0.004),
    ], name)
    wc.uv_world(body)
    glass = wc.join([wc.box((w, 0.002, h), (0, -FRONT - 0.001, 0), screen)], name.replace("monitor", "screen"))
    wc.uv_world(glass)
    wc.uv_fit(glass, "cctv_screen", front=(0, -1, 0))
    wc.move_origin(glass, (0, -FRONT - 0.001, 0))
    wc.parent(glass, body)
    wc.part_scene(name, [body])
    return bw, bh


def flag_frame(name, bw, bh, gap, th, tag_w, tag_h):
    """A glowing outline `gap` outside a bezel of bw x bh, bars `th` thick, and a tag bar centred on its top."""
    y0, y1 = -FRONT + 0.01, -FRONT - 0.005
    ow, oh = bw + 2 * (gap + th), bh + 2 * (gap + th)
    bars = []
    for sz in (-1, 1):
        z = sz * (oh / 2 - th / 2)
        bars.append(wc.box_between((-ow / 2, y1, z - th / 2), (ow / 2, y0, z + th / 2), flag))
    for sx in (-1, 1):
        x = sx * (ow / 2 - th / 2)
        bars.append(wc.box_between((x - th / 2, y1, -oh / 2 + th), (x + th / 2, y0, oh / 2 - th), flag))
    top = oh / 2 - th / 2
    tag = y1 - 0.002  # just in front of the bars
    bars.append(wc.box_between((-tag_w / 2, tag, top - tag_h / 2), (tag_w / 2, y0, top + tag_h / 2), flag))
    o = wc.join(bars, name)
    wc.uv_world(o)
    wc.empty(f"{name}_text", (0, tag - 0.001, top), parent=o)
    wc.part_scene(name, [o])


fw, fh = monitor("cctv_feed_monitor", 1.20, 0.675, 0.018)
Fw, Fh = monitor("cctv_focus_monitor", 1.52, 0.855, 0.03)
flag_frame("cctv_feed_flag", fw, fh, 0.012, 0.016, 0.9, 0.06)
flag_frame("cctv_focus_flag", Fw, Fh, 0.02, 0.025, 1.16, 0.08)

ticker = wc.join([wc.box((5.2, 0.05, 0.14), (0, -0.025, 0), bezel, bevel=0.006)], "cctv_ticker")
wc.uv_world(ticker)
strip = wc.join([wc.box((5.1, 0.002, 0.09), (0, -0.051, 0), screen)], "cctv_ticker_screen")
wc.uv_world(strip)
wc.uv_fit(strip, "cctv_screen", front=(0, -1, 0))
wc.move_origin(strip, (0, -0.052, 0))
wc.parent(strip, ticker)
wc.part_scene("cctv_ticker", [ticker])

wc.export(out, "cctv_monitors")
