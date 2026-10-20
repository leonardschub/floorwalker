//! The corridor (sweatshop-rooms ticket 16): one doorway per room of the floor plan, built
//! from the corridor kit, lit like the accepted look, with the player's camera at the
//! start pose. The rooms behind the doorways (`rooms`) stand elsewhere in world space and
//! are seen through their portals (`portals`).
//!
//! The engine hands each agent tree to [`apply_tree`], which works out the next
//! [`FloorPlan`]; [`build_corridor`] then makes the corridor match it. Every change snaps:
//! segments are added or removed at the far end, and the far end wall moves.

use bevy::core_pipeline::prepass::DepthPrepass;
use bevy::prelude::*;
use bevy::render::view::{NoIndirectDrawing, Tonemapping};

use agents::AgentTreeSnapshot;

use super::clutter;
use super::collision;
use super::floor_plan::{self, layout, FloorPlan};
use super::look::Look;
use super::pieces::{FirstPiecesSpawned, Kit};
use super::portals::IndirectDrawing;
use super::quality::ViewQuality;
use super::walk::PlayerEye;

/// A fresh world's turn: facing the doorway wall, level.
pub const START_LOOK: Look = Look {
    yaw: 0.0,
    pitch: 0.0,
};

/// The greenish fluorescent light of the look (`review_renders.py`'s `LIGHT`).
pub const LIGHT: Color = Color::srgb(0.88, 1.0, 0.78);

/// One tube light per segment, in the middle of the corridor's ceiling, and the point
/// light under it.
const TUBE: Vec3 = Vec3::new(layout::SEGMENT / 2.0, 3.0, layout::WIDTH / 2.0);
const TUBE_LUMENS: f32 = 30_000.0;

/// Debug builds only (`WORLD_DEBUG_EYE`): where the camera starts instead of the start
/// eye, in the corridor.
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct DebugEye(pub Vec3);

/// The current floor plan. Changed only when the plan does.
#[derive(Resource, Default)]
pub struct Plan(pub FloorPlan);

/// Works out the next floor plan from `tree`.
pub fn apply_tree(world: &mut World, tree: &AgentTreeSnapshot) {
    let next = floor_plan::next(tree, &world.resource::<Plan>().0);
    let mut plan = world.resource_mut::<Plan>();
    if plan.0 != next {
        let rooms: Vec<String> = next
            .rooms
            .iter()
            .map(|r| {
                let deepest = r.desks.iter().map(|d| d.depth).max().unwrap_or(0);
                format!(
                    "{:?} slot {}: {} desks (depth to {deepest}), {} rows",
                    r.project,
                    r.slot,
                    r.desks.len(),
                    r.rows
                )
            })
            .collect();
        let (start, far) = layout::end_walls(&next);
        eprintln!(
            "[world] floor plan: {} rooms [{}], end walls at x {start:.2} and {far:.2}",
            rooms.len(),
            rooms.join("; ")
        );
        plan.0 = next;
    }
}

/// What has been built of the corridor.
#[derive(Resource, Default)]
struct Built {
    /// Each segment: whether it has a doorway, and its entity (the scene, its tube and
    /// its light as children).
    segments: Vec<(bool, Entity)>,
    far_end: Option<Entity>,
    /// Each segment's far wall: which part, and its entity.
    far_walls: Vec<(layout::FarWall, Entity)>,
    /// The rooms it was built for, in order.
    rooms: Vec<String>,
}

pub struct CorridorPlugin;

impl Plugin for CorridorPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Plan>()
            .init_resource::<ViewQuality>()
            .init_resource::<Built>()
            .insert_resource(GlobalAmbientLight {
                color: LIGHT,
                brightness: 40.0,
                ..default()
            })
            .add_systems(Startup, spawn_camera)
            .add_systems(Update, build_corridor);
    }
}

fn spawn_camera(
    mut commands: Commands,
    indirect: Res<IndirectDrawing>,
    quality: Res<ViewQuality>,
    debug_eye: Option<Res<DebugEye>>,
) {
    let eye = debug_eye.map_or(Vec3::from_array(layout::start_eye()), |eye| eye.0);
    let mut camera = commands.spawn((
        Camera3d::default(),
        PlayerEye,
        Tonemapping::AgX,
        // The dusty haze, while Clutter is on (`clutter`).
        clutter::haze(),
        Transform::from_translation(eye),
        // Near enough that walking through a doorway never cuts its opening.
        Projection::Perspective(PerspectiveProjection {
            near: collision::NEAR,
            ..default()
        }),
    ));
    camera.insert((quality.msaa, super::quality::clusters()));
    if quality.prepass {
        camera.insert(DepthPrepass);
    }
    // Bevy's multi-draw-indirect path drops whole batches of meshes (a room's walls,
    // every desk top) on the owner's Intel iGPU; direct draws show every one.
    if !indirect.0 {
        camera.insert(NoIndirectDrawing);
    }
}

/// Makes the corridor match the plan, once the kit is in and whenever the plan changes.
fn build_corridor(
    mut commands: Commands,
    kit: Option<Res<Kit>>,
    plan: Res<Plan>,
    mut built: ResMut<Built>,
    mut transforms: Query<&mut Transform>,
) {
    let Some(kit) = kit else { return };
    let first = built.far_end.is_none();
    if !first && !plan.is_changed() {
        return;
    }
    let plan = &plan.0;
    // Only the rooms' order matters here: desks coming and going leave the corridor be.
    let order: Vec<String> = plan.rooms.iter().map(|r| r.project.clone()).collect();
    if !first && built.rooms == order {
        return;
    }
    built.rooms = order;
    let rooms = plan.rooms.len();
    let segments = layout::segments(plan);

    // Segments: one with a doorway per room, or the plain one.
    if built.segments.len() > segments {
        for (_, gone) in built.segments.drain(segments..) {
            commands.entity(gone).despawn();
        }
    }
    for n in 0..segments {
        let doorway = n < rooms;
        if let Some(&(had, entity)) = built.segments.get(n) {
            if had == doorway {
                continue;
            }
            commands.entity(entity).despawn();
        }
        let segment = spawn_segment(&mut commands, &kit, n, doorway);
        if n < built.segments.len() {
            built.segments[n] = (doorway, segment);
        } else {
            built.segments.push((doorway, segment));
        }
    }

    // Each segment's far wall, open where the CCTV doorway falls.
    let far_walls = layout::far_walls(plan);
    let kept = far_walls.len().min(built.far_walls.len());
    for (_, gone) in built.far_walls.drain(kept..) {
        commands.entity(gone).despawn();
    }
    for (n, wall) in far_walls.into_iter().enumerate() {
        if built.far_walls.get(n).is_some_and(|(had, _)| *had == wall) {
            continue;
        }
        let scene = match wall {
            layout::FarWall::Plain => "corridor_far_wall",
            layout::FarWall::Doorway => "corridor_far_wall_doorway",
            layout::FarWall::DoorwayEnd => "corridor_far_wall_doorway_end",
            layout::FarWall::DoorwayStart => "corridor_far_wall_doorway_start",
        };
        let entity = commands
            .spawn((
                kit.piece(scene),
                Transform::from_translation(Vec3::X * layout::segment_x(n)),
            ))
            .id();
        match built.far_walls.get_mut(n) {
            Some(slot) => {
                commands.entity(slot.1).despawn();
                *slot = (wall, entity);
            }
            None => built.far_walls.push((wall, entity)),
        }
    }

    // The far end follows the last segment.
    let far = Vec3::X * layout::segment_x(segments);
    match built.far_end {
        Some(end) => {
            if let Ok(mut at) = transforms.get_mut(end) {
                at.translation = far;
            }
        }
        None => {
            commands.spawn((kit.piece("corridor_end_start"), Transform::IDENTITY));
            built.far_end = Some(
                commands
                    .spawn((
                        kit.piece("corridor_end_far"),
                        Transform::from_translation(far),
                    ))
                    .id(),
            );
        }
    }

    if first {
        // Applied with the spawns above, so *ready* waits for these pieces.
        commands.insert_resource(FirstPiecesSpawned(true));
    }
}

/// Segment `n`, with or without a doorway, its tube light and the light under it.
fn spawn_segment(commands: &mut Commands, kit: &Kit, n: usize, doorway: bool) -> Entity {
    let scene = if doorway {
        "corridor_segment"
    } else {
        "corridor_segment_plain"
    };
    commands
        .spawn((
            Transform::from_translation(Vec3::X * layout::segment_x(n)),
            Visibility::default(),
        ))
        .with_children(|segment| {
            segment.spawn((kit.piece(scene), Transform::IDENTITY));
            segment.spawn((
                kit.piece("room_tube_light"),
                Transform::from_translation(TUBE),
            ));
            segment.spawn((
                PointLight {
                    color: LIGHT,
                    intensity: TUBE_LUMENS,
                    range: 10.0,
                    ..default()
                },
                Transform::from_translation(TUBE - Vec3::Y * 0.15),
            ));
        })
        .id()
}
