//! The CCTV room (management-rooms ticket 14; spec: "The corridor's far side", "The CCTV
//! room: layout"): the first management room, behind the CCTV doorway in the corridor's
//! far wall, built from the CCTV kits (`desktop/world-assets/cctv_*.py`) with the
//! accepted "black site" look. Its monitor wall is `feeds`'.
//!
//! Like a project room, it stands elsewhere in world space ([`layout::CCTV_ORIGIN`],
//! turned half round to face the corridor) on a render layer of its own
//! ([`layout::CCTV_LAYER`]), meshes and lights alike, and is seen through its doorway's
//! portal (`portals`), which also puts its lights on while it is seen or stood in. Its
//! walls and console block (`collision`). The room never moves: when the corridor changes,
//! only its doorway and the sign over it snap to the corridor's new middle.
//!
//! **It widens past twelve rooms**: one `cctv_extension` strip, half a feed
//! column wide, on each side per column the monitor wall gains, with the side walls moved
//! out past them, so the doorway, console and wall stay centred
//! (`cctv_state::room_extra_columns`, which `collision` follows too). It narrows back as
//! rooms close; depth and height never change, and the corridor never sees it.
//!
//! Later tickets add to it here the console's clickable parts, found in the spawned
//! console by their node names.

use bevy::app::Propagate;
use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;

use super::cctv_geometry;
use super::cctv_state;
use super::collision::Space;
use super::corridor::Plan;
use super::floor_plan::layout;
use super::lettering::{Lettered, Wording};
use super::pieces::Kit;
use super::portals::RoomLight;
use super::quality::ViewQuality;

/// The shell of the room, as the kit's parts, each at its origin; the side walls move
/// out as it widens.
const SHELL: [&str; 4] = [
    "cctv_doorway_wall",
    "cctv_floor",
    "cctv_ceiling",
    "cctv_back_wall",
];
const SIDE_WALLS: [&str; 2] = ["cctv_side_wall_left", "cctv_side_wall_right"];
/// An extension strip's width: half a feed column (`cctv_room_kit.py`).
const EXTENSION: f32 = cctv_geometry::COLUMN_WIDTH / 2.0;

/// The sign's centre over the doorway, as over the project doorways, and its letters in
/// front of the plate's back (its front face at 0.04, `cctv_sign_text` at 0.041).
const SIGN_HEIGHT: f32 = 2.45;
const SIGN_FRONT: f32 = 0.043;

/// The room's few cold downlights (the accepted look: "little room light; the screens
/// carry it"), in the room's frame, hung below the ceiling so none makes a hot spot on
/// it, and one low between the console and the monitor wall standing in for the screens'
/// glow, below the screens so they stay dark glass.
const DOWNLIGHTS: [[f32; 3]; 3] = [[-2.0, 3.4, -2.8], [2.0, 3.4, -2.8], [0.0, 3.4, -1.05]];
const DOWNLIGHT: Color = Color::srgb(0.6, 0.85, 1.0);
const DOWNLIGHT_LUMENS: f32 = 9_000.0;
const SCREEN_GLOW_AT: [f32; 3] = [0.0, 0.9, -5.6];
const SCREEN_GLOW: Color = Color::srgb(0.55, 0.9, 1.0);
const SCREEN_GLOW_LUMENS: f32 = 4_000.0;

/// The CCTV room's own ambient light, dark and cold, for every camera that sees into it
/// (`portals`): the corridor's and the project rooms' greenish light is the world's.
pub fn ambient() -> AmbientLight {
    AmbientLight {
        color: DOWNLIGHT,
        brightness: 6.0,
        affects_lightmapped_meshes: true,
    }
}

/// What has been built: the room, its side walls (left and right as seen from the
/// doorway) and extension strips, and the sign over its doorway.
#[derive(Resource, Default)]
struct Built {
    room: Option<Entity>,
    side_walls: Vec<Entity>,
    extensions: Vec<Entity>,
    widened: Option<usize>,
    sign: Option<Entity>,
}

pub struct CctvPlugin;

impl Plugin for CctvPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Built>()
            .init_resource::<ViewQuality>()
            .add_systems(Update, ((build_room, widen_room).chain(), hang_sign));
    }
}

/// Builds the room once the kit is in. Only its width changes after (`widen_room`).
fn build_room(
    mut commands: Commands,
    kit: Option<Res<Kit>>,
    mut built: ResMut<Built>,
    quality: Res<ViewQuality>,
) {
    let Some(kit) = kit else { return };
    if built.room.is_some() {
        return;
    }
    let root = commands
        .spawn((
            Transform::from_translation(Vec3::from_array(layout::CCTV_ORIGIN))
                .with_rotation(Quat::from_rotation_y(std::f32::consts::PI)),
            Visibility::default(),
            Propagate(RenderLayers::from_layers(&[layout::CCTV_LAYER])),
        ))
        .id();
    for part in SHELL {
        commands.spawn((kit.piece(part), Transform::IDENTITY, ChildOf(root)));
    }
    built.side_walls = SIDE_WALLS
        .map(|part| {
            commands
                .spawn((kit.piece(part), Transform::IDENTITY, ChildOf(root)))
                .id()
        })
        .to_vec();
    commands.spawn((
        kit.piece("cctv_console"),
        Transform::from_xyz(0.0, 0.0, layout::CONSOLE_Z),
        ChildOf(root),
    ));
    let lights = DOWNLIGHTS
        .map(|at| (at, DOWNLIGHT, DOWNLIGHT_LUMENS))
        .into_iter()
        .chain([(SCREEN_GLOW_AT, SCREEN_GLOW, SCREEN_GLOW_LUMENS)]);
    for (at, color, intensity) in lights {
        commands.spawn((
            RoomLight {
                space: Space::Cctv,
                light: PointLight {
                    color,
                    intensity,
                    range: quality.light_range,
                    ..default()
                },
            },
            Transform::from_translation(Vec3::from_array(at)),
            Visibility::default(),
            ChildOf(root),
        ));
    }
    built.room = Some(root);
    eprintln!(
        "[world] the CCTV room is built at {:?}, on layer {}",
        layout::CCTV_ORIGIN,
        layout::CCTV_LAYER
    );
}

/// Widens the room to the plan's monitor wall, and narrows it back: an extension strip on
/// each side per extra column, the side walls just past them. A player a side wall lands
/// on is pushed aside by `walk`'s settling, which follows the same plan.
fn widen_room(
    mut commands: Commands,
    kit: Option<Res<Kit>>,
    plan: Res<Plan>,
    mut built: ResMut<Built>,
    mut transforms: Query<&mut Transform>,
) {
    let (Some(kit), Some(root)) = (kit, built.room) else {
        return;
    };
    let extra = cctv_state::room_extra_columns(plan.0.rooms.len());
    if built.widened == Some(extra) {
        return;
    }
    let shift = extra as f32 * EXTENSION;
    for (wall, side) in built.side_walls.clone().into_iter().zip([-1.0, 1.0]) {
        if let Ok(mut at) = transforms.get_mut(wall) {
            at.translation.x = side * shift;
        } else {
            // Spawned this frame: its transform lands with the spawn's commands.
            commands
                .entity(wall)
                .insert(Transform::from_xyz(side * shift, 0.0, 0.0));
        }
    }
    let narrow = layout::CCTV_HALF_WIDTH;
    while built.extensions.len() < 2 * extra {
        let k = built.extensions.len() / 2;
        for x in [
            narrow + k as f32 * EXTENSION,
            -narrow - (k + 1) as f32 * EXTENSION,
        ] {
            let strip = commands
                .spawn((
                    kit.piece("cctv_extension"),
                    Transform::from_xyz(x, 0.0, 0.0),
                    ChildOf(root),
                ))
                .id();
            built.extensions.push(strip);
        }
    }
    for gone in built.extensions.split_off(2 * extra) {
        commands.entity(gone).despawn();
    }
    if built.widened.is_some() || extra > 0 {
        eprintln!(
            "[world] the CCTV room is {:.2} m wide inside ({extra} extra columns)",
            2.0 * cctv_state::room_half_width(plan.0.rooms.len())
        );
    }
    built.widened = Some(extra);
}

/// Hangs the CCTV sign over the CCTV doorway once the kit is in, and moves it with the
/// doorway when the corridor changes. It faces the corridor (-Z), on the corridor's layer.
fn hang_sign(
    mut commands: Commands,
    kit: Option<Res<Kit>>,
    plan: Res<Plan>,
    mut built: ResMut<Built>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut transforms: Query<&mut Transform>,
) {
    let Some(kit) = kit else { return };
    let x = layout::cctv_doorway_x(&plan.0);
    let at = Transform::from_xyz(x, SIGN_HEIGHT, layout::WIDTH)
        .with_rotation(Quat::from_rotation_y(std::f32::consts::PI));
    match built.sign {
        Some(sign) => {
            if !plan.is_changed() {
                return;
            }
            if let Ok(mut placed) = transforms.get_mut(sign) {
                if *placed != at {
                    *placed = at;
                    eprintln!("[world] the CCTV doorway snapped to x {x:.2}");
                }
            }
        }
        None => {
            let size = Wording::CctvSign.size();
            let quad = meshes.add(Rectangle::new(size.x, size.y));
            let sign = commands
                .spawn((kit.piece("cctv_sign_plate"), at))
                .with_children(|sign| {
                    sign.spawn((
                        Lettered(Wording::CctvSign),
                        Mesh3d(quad),
                        Transform::from_xyz(0.0, 0.0, SIGN_FRONT),
                    ));
                })
                .id();
            built.sign = Some(sign);
            eprintln!("[world] the CCTV doorway at x {x:.2}");
        }
    }
}
