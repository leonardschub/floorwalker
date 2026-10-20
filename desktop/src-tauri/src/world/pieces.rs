//! The world's pieces: the `.glb` files `desktop/world-assets` generates (ADR-0012),
//! loaded from the asset root the engine chooses, and the named scenes the world builds
//! from them.
//!
//! The sign font loads beside them and counts the same.
//!
//! Loading is two steps: each file loads with every texture it points at, then every
//! scene the world uses is looked up, so a missing one fails the start rather than a
//! later change. [`Loading`] reports where that stands, so the engine can hold *ready*
//! until the first pieces are in and turn a load failure into *failed*.

use std::collections::HashMap;

use bevy::asset::{LoadState, RecursiveDependencyLoadState};
use bevy::gltf::Gltf;
use bevy::prelude::*;
use bevy::world_serialization::{WorldAsset, WorldInstanceReady};

/// Every generated file the world uses, with the scenes it takes from each.
const FILES: [(&str, &[&str]); 20] = [
    (
        "corridor_kit.glb",
        &[
            "corridor_segment",
            "corridor_segment_plain",
            "corridor_far_wall",
            "corridor_far_wall_doorway",
            "corridor_far_wall_doorway_end",
            "corridor_far_wall_doorway_start",
            "corridor_end_start",
            "corridor_end_far",
        ],
    ),
    (
        "room_kit.glb",
        &[
            "room_doorway_wall",
            "room_row_segment",
            "room_back_wall",
            "room_pipes",
            "room_tube_light",
            "room_tube_light_dead",
        ],
    ),
    ("desk_set.glb", &["desk_set"]),
    ("robot.glb", &["robot"]),
    ("sign_plate.glb", &["sign_plate"]),
    // Each clutter file holds one scene, named like it (`clutter::Piece::scene`).
    ("clutter_crumpled_paper.glb", &["clutter_crumpled_paper"]),
    ("clutter_rusty_cans.glb", &["clutter_rusty_cans"]),
    (
        "clutter_cardboard_crates.glb",
        &["clutter_cardboard_crates"],
    ),
    ("clutter_loose_sheets.glb", &["clutter_loose_sheets"]),
    ("clutter_floor_cables.glb", &["clutter_floor_cables"]),
    ("clutter_bucket.glb", &["clutter_bucket"]),
    ("clutter_broken_chair.glb", &["clutter_broken_chair"]),
    // The CCTV room.
    (
        "cctv_room_kit.glb",
        &[
            "cctv_doorway_wall",
            "cctv_floor",
            "cctv_ceiling",
            "cctv_back_wall",
            "cctv_side_wall_left",
            "cctv_side_wall_right",
            "cctv_extension",
        ],
    ),
    (
        "cctv_monitors.glb",
        &[
            "cctv_feed_monitor",
            // The flagged feeds' frames, the focus monitors and the ticker.
            "cctv_feed_flag",
            "cctv_focus_monitor",
            "cctv_focus_flag",
            "cctv_ticker",
        ],
    ),
    ("cctv_console.glb", &["cctv_console"]),
    ("cctv_sign_plate.glb", &["cctv_sign_plate"]),
    // Each project room's wall camera.
    ("wall_camera.glb", &["wall_camera"]),
    // The punishments' effect shapes.
    (
        "effects.glb",
        &[
            "fx_bolt",
            "fx_arc",
            "fx_spark",
            "fx_smoke_puff",
            "fx_fireball",
        ],
    ),
    // The kill's squad and what it leaves.
    ("squad_robot.glb", &["squad_robot", "squad_charge"]),
    (
        "wreck_parts.glb",
        &[
            "wreck_torso",
            "wreck_head",
            "wreck_arm",
            "wreck_leg",
            "wreck_chair",
            "wreck_debris",
            "scorch_floor",
            "scorch_desk",
        ],
    ),
];

/// The font of the signs and the model names (`desktop/world-assets/third-party`).
const FONT: &str = "fonts/oswald.ttf";

/// How far loading the pieces has come.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub enum Loading {
    /// Files loading, or the first pieces spawning.
    #[default]
    Pending,
    /// The first pieces are spawned and ready to draw.
    Done,
    /// A file, a texture or a scene could not be loaded.
    Failed(String),
}

/// The loaded scenes by name, once every file is in.
#[derive(Resource)]
pub struct Kit {
    scenes: HashMap<&'static str, Handle<WorldAsset>>,
    /// The sign font, loaded.
    pub font: Handle<Font>,
}

impl Kit {
    /// A piece to spawn: scene `name` (one of [`FILES`]'), not yet ready.
    pub fn piece(&self, name: &str) -> impl Bundle {
        let scene = self
            .scenes
            .get(name)
            .expect("a scene listed in FILES")
            .clone();
        (WorldAssetRoot(scene), Unready)
    }
}

/// Set by whoever has spawned the first pieces: until then *ready* waits.
#[derive(Resource, Default)]
pub struct FirstPiecesSpawned(pub bool);

/// A spawned piece whose scene is not in yet.
#[derive(Component)]
pub struct Unready;

/// The files, while they load, and the font.
#[derive(Resource)]
struct Files(Vec<(&'static str, Handle<Gltf>)>, Handle<Font>);

pub struct PiecesPlugin;

impl Plugin for PiecesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Loading>()
            .init_resource::<FirstPiecesSpawned>()
            .add_systems(Startup, load_files)
            .add_systems(Update, (look_up_scenes, first_pieces_in))
            .add_observer(scene_ready);
    }
}

fn load_files(mut commands: Commands, assets: Res<AssetServer>) {
    commands.insert_resource(Files(
        FILES
            .iter()
            .map(|&(file, _)| (file, assets.load(file)))
            .collect(),
        assets.load(FONT),
    ));
}

/// Once every file has loaded with its textures, looks up the scenes the world uses and
/// makes them the [`Kit`]; a failure anywhere fails the whole load.
fn look_up_scenes(
    mut commands: Commands,
    files: Option<Res<Files>>,
    mut loading: ResMut<Loading>,
    assets: Res<AssetServer>,
    gltfs: Res<Assets<Gltf>>,
) {
    let Some(files) = files else { return };
    if *loading != Loading::Pending {
        return;
    }
    for (file, handle) in &files.0 {
        if let LoadState::Failed(error) = assets.load_state(handle) {
            *loading = Loading::Failed(format!("{file}: {error}"));
            return;
        }
        match assets.recursive_dependency_load_state(handle) {
            RecursiveDependencyLoadState::Loaded => {}
            RecursiveDependencyLoadState::Failed(error) => {
                *loading = Loading::Failed(format!("{file} (a texture): {error}"));
                return;
            }
            _ => return,
        }
    }
    match assets.load_state(&files.1) {
        LoadState::Loaded => {}
        LoadState::Failed(error) => {
            *loading = Loading::Failed(format!("{FONT}: {error}"));
            return;
        }
        _ => return,
    }
    let mut kit = HashMap::new();
    for ((file, handle), (_, scenes)) in files.0.iter().zip(FILES) {
        let Some(gltf) = gltfs.get(handle) else {
            *loading = Loading::Failed(format!("{file} is not loaded"));
            return;
        };
        for &scene in scenes {
            let Some(found) = gltf.named_scenes.get(scene) else {
                *loading = Loading::Failed(format!("{file} has no scene {scene}"));
                return;
            };
            kit.insert(scene, found.clone());
        }
    }
    commands.insert_resource(Kit {
        scenes: kit,
        font: files.1.clone(),
    });
    commands.remove_resource::<Files>();
}

/// The load is done once the first pieces are spawned and every one of them is in.
fn first_pieces_in(
    spawned: Res<FirstPiecesSpawned>,
    unready: Query<(), With<Unready>>,
    mut loading: ResMut<Loading>,
) {
    if spawned.0 && *loading == Loading::Pending && unready.is_empty() {
        *loading = Loading::Done;
    }
}

/// A piece's scene is in. A piece despawned before its scene came in leaves no
/// [`Unready`] behind either.
fn scene_ready(ready: On<WorldInstanceReady>, mut commands: Commands) {
    if let Ok(mut piece) = commands.get_entity(ready.entity) {
        piece.remove::<Unready>();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::clutter::Piece;

    #[test]
    fn every_clutter_piece_is_loaded() {
        for piece in Piece::ALL {
            let file = format!("{}.glb", piece.scene());
            assert!(
                FILES
                    .iter()
                    .any(|(f, scenes)| *f == file && scenes.contains(&piece.scene())),
                "{piece:?}"
            );
        }
    }
}
