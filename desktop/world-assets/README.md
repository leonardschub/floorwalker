# World assets

Headless Blender scripts that generate the world's models and their baked textures
([ADR-0012](../../docs/adr/0012-world-assets-are-generated-by-blender-at-build-time.md)). Only these
scripts are committed; their output goes to an ignored folder (`out/` here by convention).

Needs Blender 4.3.x and numpy (Debian: `python3-numpy`).

## Regeneration (part of the build)

`regenerate.py` (plain Python 3) runs every piece script into `out/`, which the desktop app bundles
as its `world-assets/` resource folder. It runs in `tauri dev` and `tauri build` before cargo
builds the app: `cargo-runner.sh` is Tauri's `build.runner` (`../src-tauri/tauri.conf.json`), so it
runs after the frontend is up and a first regeneration cannot run out `tauri dev`'s 180 s wait for
the dev server. Or by hand:

```sh
pnpm --filter desktop world-assets        # skipped when out/.fingerprint matches
pnpm --filter desktop world-assets:force  # empty out/ and rebake everything
```

It first checks for Blender 4.3.x (`blender` on PATH, or the command in `BLENDER`) and numpy, and
stops with what to install. The fingerprint hashes every `.py` here and the Blender version; a run
that stops halfway leaves no fingerprint, so the next one starts again. Blender's output goes to
`regenerate.log` beside `out/`. Every run, skipped or not, also copies the committed third-party
files into `out/` (below).

Run one piece:

```sh
env -u WAYLAND_DISPLAY -u DISPLAY blender -b --factory-startup --python desktop/world-assets/<piece>.py -- <out-dir>
```

Each piece script bakes the textures it needs into `<out-dir>/textures/` (reusing ones already baked
by this exact version of `wa_materials.py`; `--rebake` forces it), then writes `<out-dir>/<piece>.glb`.
A first build of everything bakes 24 texture sets and takes about 15 minutes on a laptop CPU.

## Pieces

| Script | Output | glTF scenes / root nodes |
| --- | --- | --- |
| `desk_set.py` | `desk_set.glb` | `desk_set` > `desk`, `chair`, `pc`, `keyboard`, `tower`, `mouse`, `robot_seat` (empty) |
| `robot.py` | `robot.glb` | `robot` (empty, the scalable node) > `robot_body`, `robot_face` |
| `room_kit.py` | `room_kit.glb` | `room_doorway_wall`, `room_row_segment`, `room_back_wall`, `room_pipes`, `room_tube_light`, `room_tube_light_dead` |
| `corridor_kit.py` | `corridor_kit.glb` | `corridor_segment`, `corridor_segment_plain`, `corridor_far_wall`, `corridor_far_wall_doorway`, `corridor_far_wall_doorway_end`, `corridor_far_wall_doorway_start`, `corridor_end_start`, `corridor_end_far` |
| `sign_plate.py` | `sign_plate.glb` | `sign_plate` > `sign_text` (empty) |
| `clutter_<item>.py` | `clutter_<item>.glb` | `clutter_<item>`, for `crumpled_paper`, `rusty_cans`, `cardboard_crates`, `loose_sheets`, `floor_cables`, `bucket`, `broken_chair` |
| `cctv_room_kit.py` | `cctv_room_kit.glb` | `cctv_doorway_wall`, `cctv_floor`, `cctv_ceiling`, `cctv_back_wall`, `cctv_side_wall_left`, `cctv_side_wall_right`, `cctv_extension`; each wall's glow is its child `<part>_lights` |
| `cctv_monitors.py` | `cctv_monitors.glb` | `cctv_feed_monitor` > `cctv_feed_screen`, `cctv_focus_monitor` > `cctv_focus_screen`, `cctv_feed_flag` > `cctv_feed_flag_text` (empty), `cctv_focus_flag` > `cctv_focus_flag_text` (empty), `cctv_ticker` > `cctv_ticker_screen` |
| `cctv_console.py` | `cctv_console.glb` | `cctv_console` (empty) > `console_body`, `console_screen`, `console_sit_in`, `console_hazard`, `console_kill_button`, `console_cover`, `console_work_harder`, `console_sit_in_label`, `console_kill_label`, `console_work_harder_label` (empties) |
| `cctv_sign_plate.py` | `cctv_sign_plate.glb` | `cctv_sign_plate` > `cctv_sign_text` (empty) |
| `wall_camera.py` | `wall_camera.glb` | `wall_camera` (empty) > `wall_camera_mount`, `wall_camera_head` > `wall_camera_lens` (empty) |
| `squad_robot.py` | `squad_robot.glb` | `squad_robot` (empty) > `squad_pelvis` > `squad_torso` (> `squad_head`, `squad_arm_upper_{l,r}` > `squad_arm_lower_{l,r}`), `squad_leg_upper_{l,r}` > `squad_leg_lower_{l,r}`; `squad_charge` > `squad_charge_led` |
| `wreck_parts.py` | `wreck_parts.glb` | `wreck_torso`, `wreck_head`, `wreck_arm`, `wreck_leg`, `wreck_chair`, `wreck_debris`, `scorch_floor`, `scorch_desk` |
| `effects.py` | `effects.glb` | `fx_fireball`, `fx_smoke_puff`, `fx_bolt`, `fx_arc`, `fx_spark` |

Kits hold one glTF scene per part, named like its root node, so the world can spawn a part by
scene name (`Gltf::named_scenes`) or find it by node name. Each script's docstring gives its frame
and where its parts go; in short:

- **Units and axes:** metres; Blender Z up and -Y front, exported with the default +Y up, so a
  piece's front is glTF **+Z**. Robots face +Z (towards their desk and the doorway); the PC's back
  faces +Z too.
- **Room frame:** X across (inner walls at ±4.75, 9.5 m wide), the doorway wall's inner face at
  z = 0 and the room running towards glTF -Z, ceiling at 3.0 m. A room of R rows is the doorway wall
  (with a 0.2 m strip of floor, ceiling and side walls), R row segments at depth 0.2 + 2.4 n, and the
  back wall at 0.2 + 2.4 R. The doorway is 1.2 × 2.1 m, at x -4.10..-2.90. `room_pipes` (along the left wall seen from the doorway) goes with
  each row segment; tube lights mount at their origin on the ceiling (y = 3.0).
- **Corridor frame:** X along the corridor, the doorway wall's corridor face at z = 0 with the
  corridor towards glTF +Z (2.4 m wide). Segment n (2.5 m, doorway at x 0.65..1.85) goes at
  x = 2.5 n; `corridor_end_start` at x = 0 and `corridor_end_far` after the last segment put the end
  walls 1.5 m from the outer doorways. `corridor_segment_plain` is the doorless corridor shown when no
  project is open. Each segment's far wall is a part of its own at the segment's place: plain, or
  open for the CCTV doorway in its middle (`_doorway`), or at its end or start (`_doorway_end`,
  `_doorway_start`) where two segments' far walls meet in the doorway's middle.
- **Desk set:** origin on the floor under the seat; `robot_seat` (0.46 m up) is where `robot.glb`'s
  origin goes. The robot's origin is its seat point, so scaling the `robot` node shrinks it onto
  its seat.
- **CCTV room frame** (`cctv_room_kit.py`): X across, centred on the doorway (inner side walls at
  ±4 m), the doorway wall's inner face at z = 0 and the room running towards glTF -Z (back wall at
  -7 m), ceiling at 4.2 m; the doorway is 1.2 × 2.1 m at x -0.6..0.6. A feed column is 4/3 m; past
  four columns each extra one adds a 2/3 m `cctv_extension` on each side and moves the side walls out
  by as much (the script's docstring has the placement). Monitors, the ticker and the console face
  glTF +Z (towards the doorway) with their origin at their back (the console's on the floor). Each
  CCTV and punishment script's docstring gives its sizes, pivots and anchors; the squad robot's limb
  nodes have their origin on their joints, and the wreck parts theirs on the floor under them.
- **Findable by name:** the PC's glass material `pc_screen` (emissive; one 0.49 × 0.294 m quad
  tilted to face a full-size robot's eyes, UVs filling 0..1 upright and unmirrored as the robot sees
  it, for the session terminal), the face material
  `robot_face` (UVs fill 0..1 on the plate's front, for ticket 22), the sign's `sign_text` empty
  (centre of the plate's front, 1.6 × 0.24 m usable), the glowing tube material `tube_glow`.

## Clutter tag

Clutter, and only clutter, is tagged two ways on its root node (each clutter file has exactly one
mesh node, its root):

- glTF node **extras `{"clutter": true}`** (a Blender custom property, exported with
  `export_extras`); Bevy puts it on the entity as `GltfExtras`.
- the node **name prefix `clutter_`**.

The dead tube and the pipes are room-kit parts and are not tagged.

## Textures

`wa_materials.py` holds one procedural node recipe per surface (ported from the accepted look on
branch `prototype/sweatshop-look`) and bakes each with Cycles on the CPU into three 1024×1024 PNGs:
`<name>_color.png` (sRGB), `<name>_normal.png` (tangent space, OpenGL/glTF convention) and
`<name>_orm.png` (glTF layout: G roughness, B metallic). Noise is sampled on a torus and brick and
key grids use whole periods, so every texture repeats without a seam. UVs are box-projected from the
piece's frame divided by the texture's tile size (`wa_common.uv_world`), so textures sit at
real-world scale and pieces placed on the grid above continue each other's pattern. Wall textures
(brick, peeling paint) are 2.4 m wide strips covering the full 3 m height, so rising damp stays at
the floor. The corridor maps its 2.5 m segment onto a 2.4 m tile (bricks 4% wider).

The `.glb` files do not embed their images: each points at `textures/<name>_<map>.png` beside it,
so the sets are shared by every model that uses them. The output folder is therefore
`<out>/*.glb` plus `<out>/textures/*.png`, and both ship together.

## Third-party files

`third-party/` holds the only files here that are not generated: the font of the room signs and the
robots' model names, Oswald (SIL Open Font License, `third-party/oswald/NOTICE.md` gives its source
and `OFL.txt` its licence). `regenerate.py` copies it to `<out>/fonts/oswald.ttf` on every run; no
piece is made from it, so it is not part of the fingerprint. The session terminals' font, JetBrains
Mono (the same licence, `third-party/jetbrains-mono/NOTICE.md`), is embedded in the app by the
terminals crate rather than copied. No company logo is committed anywhere:
robot faces show our own pattern in a company colour, drawn by the world.

A few small glowing or flat parts use untextured materials: `tube_glow`, `tube_dead`, `led_green`,
`robot_antenna_tip`, `dirty_water`, the PC's `pc_plastic` and `gaffer_tape`, and most of the CCTV room's and the punishments' (glass, screens,
glows, the squad's armour; the world tints `cctv_flag` with the flag's colour). The CCTV room's walls
and floor (`cctv_wall`, `cctv_floor`, on 2/3 m tiles), the wrecks' `charred_paint` and the
fireball's `fx_fire` are baked.

## Export rules

`wa_common.export` exports with Blender's glTF exporter (no Draco, no meshopt, no GPU instancing, no
WebP, modifiers applied, Blender object names kept) and packs the result into a `.glb`
itself, so the output is byte-identical from run to run. PNGs are written by our own writer (no
timestamps or metadata).

## Checking

- `python3 glb_info.py <out>/*.glb` lists each file's scenes, nodes, extras, materials and images.
- `python3 -m unittest discover -s tests` (plain Python, no Blender) checks that the build regenerates
  every piece script and, on the exported files in `out/` (or `WORLD_ASSETS_OUT`), the scene and node
  names and the sizes the world relies on. `tests/` is not part of the fingerprint.
- `blender -b --factory-startup --python review_renders.py -- <out-dir> <renders-dir> [shot ...]`
  imports the exported files and renders the review PNGs kept in
  `.scratch/sweatshop-rooms/designs/pieces/`; the shot `cctv` renders the CCTV room's and the
  punishments' pieces, kept in `.scratch/management-rooms/designs/pieces/`; the shot `pc` renders the
  desk set's PC seated, across a room and close up, kept in `.scratch/agent-screens/verification/09/`.
