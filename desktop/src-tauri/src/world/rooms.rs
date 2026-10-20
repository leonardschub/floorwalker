//! The rooms (sweatshop-rooms ticket 17): each room of the floor plan as an interior
//! built from the room kit, holding one desk set with its seated robot per desk of the
//! plan. Every interior stands in its own part of world space ([`layout::interior_origin`]
//! for its slot) on its own render layer, meshes and lights alike, so it grows towards -Z
//! without touching the corridor or another room, and the corridor's camera does not
//! draw it. A room is seen from the corridor through its doorway (`portals`), which
//! also decides which rooms' lights are on; a debug build can put the camera inside one
//! ([`InRoom`]).
//!
//! Every change snaps, as [`build_rooms`] makes the interiors match the plan: rows are
//! added or removed at the back and the back wall moves, desks keep their entities by
//! [`DeskId`] and move seats, and [`restyle_desks`] scales each robot by its named
//! `robot` node. Only running agents have desks (`floor_plan`), so a desk goes, robot
//! and all, the moment its agent finishes. Each row segment also carries its clutter
//! (`clutter`).

use std::collections::{HashMap, HashSet};

use bevy::app::{HierarchyPropagatePlugin, Propagate, PropagateSet};
use bevy::camera::visibility::{RenderLayers, VisibilitySystems};
use bevy::gltf::GltfMaterialName;
use bevy::light::SimulationLightSystems;
use bevy::prelude::*;

use super::clutter;
use super::collision::Space;
use super::controller::WorldSettings;
use super::controls::Controls;
use super::corridor::{Plan, LIGHT};
use super::feeds::Cctv;
use super::floor_plan::{self, layout, DeskId};
use super::lettering::{self, FacePlate};
use super::names::Face;
use super::pieces::{Kit, Unready};
use super::portals::{PortalCamera, RoomLight};
use super::quality::ViewQuality;
use super::surfaces::SwapMaterials;

/// How much smaller each level of subagent's robot is: the Subagent size setting
/// (ticket 23), ×0.75 by default.
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct SubagentScale(pub f32);

impl Default for SubagentScale {
    fn default() -> Self {
        Self(0.75)
    }
}

/// The Textures and Clutter settings (ticket 23), both on by default. Changed only when
/// one of them changes: `surfaces` then swaps the flat materials in or out, and
/// `clutter` hides or shows the clutter and the haze (ticket 24).
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Detail {
    pub textures: bool,
    pub clutter: bool,
}

impl Default for Detail {
    fn default() -> Self {
        Self {
            textures: true,
            clutter: true,
        }
    }
}

/// Applies the world options from Settings. Only a change marks a resource changed, so
/// resending the same settings restyles nothing; a new size restyles every desk on the
/// next frame (`restyle_desks`), where [`floor_plan::robot_scale`]'s floor holds.
pub fn apply_settings(world: &mut World, settings: WorldSettings) {
    let scale = SubagentScale(settings.subagent_scale);
    let detail = Detail {
        textures: settings.textures,
        clutter: settings.clutter,
    };
    let rescaled = world.resource_mut::<SubagentScale>().set_if_neq(scale);
    let redetailed = world.resource_mut::<Detail>().set_if_neq(detail);
    let recontrolled = world
        .resource_mut::<Controls>()
        .set_if_neq(settings.controls);
    // Punishment effects: the CCTV state machine's input,
    // applied at once to punishments already playing.
    let reeffected = world.get_resource_mut::<Cctv>().is_some_and(|mut cctv| {
        let changed = cctv.0.effects() != settings.punishment_effects;
        if changed {
            cctv.0.set_effects(settings.punishment_effects);
        }
        changed
    });
    if rescaled || redetailed || recontrolled || reeffected {
        eprintln!(
            "[world] settings: subagent size ×{:.2}, textures {}, clutter {}, punishment effects {}, walking {:.1} m/s (running {:.1}), mouse sensitivity {:.1} ({:.5} rad/px)",
            settings.subagent_scale,
            if settings.textures { "on" } else { "off" },
            if settings.clutter { "on" } else { "off" },
            if settings.punishment_effects { "on" } else { "off" },
            settings.controls.walk_speed,
            settings.controls.run_speed(),
            settings.controls.look_sensitivity,
            settings.controls.radians_per_pixel(),
        );
    }
}

/// Debug builds only (`WORLD_DEBUG_IN_ROOM`): the camera stands in room `room` of the
/// plan (counted from the corridor's start end), or in the CCTV room for none, at `at`
/// (`[x, y, z]` in the room's frame), seeing only that room, while the room exists.
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct InRoom {
    pub room: Option<usize>,
    pub at: [f32; 3],
}

impl InRoom {
    /// Reads `<room>`, `<room>@<x>,<z>` or `<room>@<x>,<y>,<z>`, where `<room>` is a
    /// number or `cctv`: just inside the doorway at eye height by default, else at `x`,
    /// `z` in the room's frame (doorway wall at z = 0, the room towards -Z), at eye
    /// height or `y`. `world_look` turns the camera as usual; at zero yaw it faces into a
    /// project room, and the CCTV room starts turned to face into it too.
    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    pub fn parse(switch: &str) -> Option<Self> {
        let (room, at) = switch.split_once('@').unwrap_or((switch, ""));
        let numbers: Vec<f32> = at
            .split(',')
            .filter(|n| !n.trim().is_empty())
            .map(|n| n.trim().parse().ok())
            .collect::<Option<_>>()?;
        let room = match room.trim() {
            "cctv" => None,
            n => Some(n.parse().ok()?),
        };
        // The middle of the doorway (x -4.10..-2.90 in a room, -0.6..0.6 in the CCTV
        // room), 0.6 m in.
        let doorway = if room.is_some() {
            layout::ROOM_DOORWAY_X
        } else {
            0.0
        };
        let at = match numbers[..] {
            [] => [doorway, layout::EYE_HEIGHT, -0.6],
            [x, z] => [x, layout::EYE_HEIGHT, z],
            [x, y, z] => [x, y, z],
            _ => return None,
        };
        Some(Self { room, at })
    }
}

/// Two tube lights per row, across the room, and the light under each live one.
const TUBE_X: f32 = 2.4;
const TUBE_Z: f32 = -0.9;
const TUBE_LUMENS: f32 = 90_000.0;
/// The room's one dead tube: the right one of its second row, which every room has.
const DEAD_TUBE_ROW: usize = 1;

/// What has been built of the rooms, by project id.
#[derive(Resource, Default)]
struct Built(HashMap<String, Interior>);

struct Interior {
    /// The interior's root, at its slot's origin, carrying its render layer. Everything
    /// below is its child.
    root: Entity,
    slot: usize,
    /// Each row: its segment, pipes, tubes and lights.
    rows: Vec<Entity>,
    back_wall: Entity,
    desks: HashMap<DeskId, Entity>,
}

/// A desk: its desk set and robot pieces are its children.
#[derive(Component, Clone, PartialEq)]
struct DeskLook {
    depth: usize,
    /// What its robot's face shows.
    face: Face,
}

/// A desk whose robot size and face are still to be applied.
#[derive(Component)]
struct Restyle;

/// A desk's robot piece.
#[derive(Component)]
pub struct Figure;

/// Whose desk a desk entity is: it keeps its entity for as long as its agent has a desk
/// (`flag_markers` puts the agent's marker on it).
#[derive(Component, Clone, Debug, PartialEq)]
pub struct Seated(pub DeskId);

/// Where the interiors are made to match the plan in `Update`.
#[derive(SystemSet, Clone, Debug, PartialEq, Eq, Hash)]
pub struct BuildRooms;

/// Where the desks' looks are applied in `PostUpdate`; their faces are dressed after.
#[derive(SystemSet, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Restyled;

pub struct RoomsPlugin;

impl Plugin for RoomsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Built>()
            .init_resource::<ViewQuality>()
            .init_resource::<SubagentScale>()
            .init_resource::<Detail>()
            // Each interior's render layer reaches every entity below its root, the
            // glTF scenes' too, before Bevy decides what each camera and light sees.
            .add_plugins(HierarchyPropagatePlugin::<RenderLayers>::new(PostUpdate))
            .configure_sets(
                PostUpdate,
                PropagateSet::<RenderLayers>::default()
                    .before(VisibilitySystems::CheckVisibility)
                    .before(SimulationLightSystems::AssignLightsToClusters)
                    .before(SimulationLightSystems::CheckLightVisibility),
            )
            .add_systems(
                Update,
                (
                    build_rooms.in_set(BuildRooms),
                    place_in_room_eye.before(super::walk::settle),
                ),
            )
            // After the new desks' scenes are spawned, before anything is drawn: a new
            // or changed desk shows its look from its first frame.
            .add_systems(
                PostUpdate,
                restyle_desks
                    .in_set(Restyled)
                    .before(SwapMaterials)
                    .before(bevy::transform::TransformSystems::Propagate),
            );
    }
}

/// Makes the interiors match the plan, once the kit is in and whenever the plan changes.
fn build_rooms(
    mut commands: Commands,
    kit: Option<Res<Kit>>,
    plan: Res<Plan>,
    mut built: ResMut<Built>,
    looks: Query<(&DeskLook, &Transform)>,
    quality: Res<ViewQuality>,
) {
    let Some(kit) = kit else { return };
    if !kit.is_added() && !plan.is_changed() {
        return;
    }
    let plan = &plan.0;

    // A room that closed takes everything in it along.
    built.0.retain(|project, interior| {
        let open = plan
            .rooms
            .iter()
            .any(|room| &room.project == project && room.slot == interior.slot);
        if !open {
            commands.entity(interior.root).despawn();
        }
        open
    });

    for room in &plan.rooms {
        let interior = built
            .0
            .entry(room.project.clone())
            .or_insert_with(|| spawn_interior(&mut commands, &kit, room.slot));

        // Rows come and go at the back, and the back wall follows the last.
        if interior.rows.len() != room.rows {
            for gone in interior.rows.drain(room.rows.min(interior.rows.len())..) {
                commands.entity(gone).despawn();
            }
            for n in interior.rows.len()..room.rows {
                let row = spawn_row(
                    &mut commands,
                    &kit,
                    &room.project,
                    n,
                    interior.slot,
                    quality.light_range,
                );
                commands.entity(row).insert(ChildOf(interior.root));
                interior.rows.push(row);
            }
            commands
                .entity(interior.back_wall)
                .insert(Transform::from_xyz(
                    0.0,
                    0.0,
                    -layout::interior_depth(room.rows),
                ));
        }

        // Desks keep their entity by identity and move to their seat.
        let mut seated = HashSet::new();
        for desk in &room.desks {
            seated.insert(desk.id.clone());
            let [x, z] = layout::desk_position(desk.row, desk.column);
            let at = Transform::from_xyz(x, 0.0, z);
            let look = DeskLook {
                depth: desk.depth,
                face: Face::of(desk.model_id.as_deref(), desk.model.as_deref()),
            };
            match interior.desks.get(&desk.id) {
                Some(&entity) => {
                    let (have_look, have_at) = looks.get(entity).ok().unzip();
                    let mut set = commands.entity(entity);
                    if have_at != Some(&at) {
                        set.insert(at);
                    }
                    if have_look != Some(&look) {
                        set.insert((look, Restyle));
                    }
                }
                None => {
                    let entity = spawn_desk(&mut commands, &kit, look, at);
                    commands
                        .entity(entity)
                        .insert((Seated(desk.id.clone()), ChildOf(interior.root)));
                    interior.desks.insert(desk.id.clone(), entity);
                }
            }
        }
        interior.desks.retain(|id, entity| {
            let stays = seated.contains(id);
            if !stays {
                commands.entity(*entity).despawn();
            }
            stays
        });
    }
}

/// An interior's root, at its slot, with its doorway wall and its back wall (placed by
/// the caller once the rows are known).
fn spawn_interior(commands: &mut Commands, kit: &Kit, slot: usize) -> Interior {
    let layer = RenderLayers::layer(layout::render_layer(slot));
    let root = commands
        .spawn((
            Transform::from_translation(Vec3::from_array(layout::interior_origin(slot))),
            Visibility::default(),
            Propagate(layer),
        ))
        .id();
    commands.spawn((
        kit.piece("room_doorway_wall"),
        Transform::IDENTITY,
        ChildOf(root),
    ));
    let back_wall = commands
        .spawn((
            kit.piece("room_back_wall"),
            Transform::IDENTITY,
            ChildOf(root),
        ))
        .id();
    Interior {
        root,
        slot,
        rows: Vec::new(),
        back_wall,
        desks: HashMap::new(),
    }
}

/// Row segment `n` of the room of `project` with its pipes, its two tubes, the light
/// under each live one, and its clutter. The lights are the room in `slot`'s: `portals`
/// puts their `PointLight` on while the room is seen or stood in.
fn spawn_row(
    commands: &mut Commands,
    kit: &Kit,
    project: &str,
    n: usize,
    slot: usize,
    range: f32,
) -> Entity {
    commands
        .spawn((
            Transform::from_xyz(0.0, 0.0, layout::row_z(n)),
            Visibility::default(),
        ))
        .with_children(|row| {
            row.spawn((kit.piece("room_row_segment"), Transform::IDENTITY));
            row.spawn((kit.piece("room_pipes"), Transform::IDENTITY));
            clutter::spawn_row_clutter(row, kit, project, n);
            for x in [-TUBE_X, TUBE_X] {
                let at = Vec3::new(x, 3.0, TUBE_Z);
                if n == DEAD_TUBE_ROW && x > 0.0 {
                    row.spawn((
                        kit.piece("room_tube_light_dead"),
                        Transform::from_translation(at),
                    ));
                    continue;
                }
                row.spawn((
                    kit.piece("room_tube_light"),
                    Transform::from_translation(at),
                ));
                row.spawn((
                    RoomLight {
                        space: Space::Room(slot),
                        light: PointLight {
                            color: LIGHT,
                            intensity: TUBE_LUMENS,
                            range,
                            ..default()
                        },
                    },
                    Transform::from_translation(at - Vec3::Y * 0.15),
                    Visibility::default(),
                ));
            }
        })
        .id()
}

/// A desk: the desk set, and the robot on its seat, both facing the doorway (+Z).
fn spawn_desk(commands: &mut Commands, kit: &Kit, look: DeskLook, at: Transform) -> Entity {
    commands
        .spawn((look, Restyle, at, Visibility::default()))
        .with_children(|desk| {
            desk.spawn((kit.piece("desk_set"), Transform::IDENTITY));
            desk.spawn((
                kit.piece("robot"),
                Figure,
                super::seat::HiddenWhenSeated,
                Transform::from_translation(Vec3::from_array(layout::ROBOT_SEAT)),
            ));
        })
        .id()
}

/// Applies each desk's look once its pieces are in: its robot's size, by the robot
/// scene's `robot` node, and its face: the robot's face part is given its
/// [`FacePlate`], which `lettering` dresses. A changed Subagent size restyles every
/// desk.
#[allow(clippy::too_many_arguments)]
fn restyle_desks(
    mut commands: Commands,
    scale: Res<SubagentScale>,
    desks: Query<(Entity, &DeskLook, Has<Restyle>)>,
    children: Query<&Children>,
    unready: Query<(), With<Unready>>,
    figures: Query<(), With<Figure>>,
    names: Query<&Name>,
    materials_named: Query<&GltfMaterialName>,
    parents: Query<&ChildOf>,
    mut transforms: Query<&mut Transform>,
) {
    let all = scale.is_changed();
    let mut restyled = Vec::new();
    for (desk, look, restyle) in &desks {
        if !restyle && !all {
            continue;
        }
        let Ok(pieces) = children.get(desk) else {
            continue;
        };
        if pieces.iter().any(|piece| unready.contains(piece)) {
            // Restyled once its scenes are in.
            commands.entity(desk).insert(Restyle);
            continue;
        }
        let size = floor_plan::robot_scale(look.depth, scale.0);
        for piece in pieces.iter() {
            let figure = figures.contains(piece);
            for part in children.iter_descendants(piece) {
                // The scene's root entity carries the scene's name, `robot` too; the
                // node is its child.
                let named = |entity| names.get(entity).is_ok_and(|n| n.as_str() == "robot");
                let node = named(part) && parents.get(part).is_ok_and(|p| named(p.parent()));
                if figure && node {
                    if let Ok(mut transform) = transforms.get_mut(part) {
                        transform.scale = Vec3::splat(size);
                    }
                }
                if figure && lettering::is_face(&materials_named, part) {
                    let plate = FacePlate {
                        face: look.face.clone(),
                    };
                    commands.entity(part).insert(plate);
                }
            }
        }
        commands.entity(desk).remove::<Restyle>();
        restyled.push(format!("depth {} robot ×{size:.4}", look.depth));
    }
    if !restyled.is_empty() {
        eprintln!("[world] desks restyled: {}", restyled.join(", "));
    }
}

/// Debug builds: keeps the camera in the room [`InRoom`] names, the player's [`Space`],
/// wherever its interior is built.
fn place_in_room_eye(
    in_room: Option<Res<InRoom>>,
    kit: Option<Res<Kit>>,
    plan: Res<Plan>,
    mut space: ResMut<Space>,
    mut look: ResMut<super::look::Look>,
    mut cameras: Query<&mut Transform, (With<Camera3d>, Without<PortalCamera>)>,
    mut placed: Local<Option<Vec3>>,
) {
    let (Some(in_room), Some(kit)) = (in_room, kit) else {
        return;
    };
    if !kit.is_added() && !plan.is_changed() {
        return;
    }
    let Some(n) = in_room.room else {
        let eye = Vec3::from_array(layout::cctv_to_world(in_room.at));
        if placed.replace(eye) == Some(eye) {
            return;
        }
        for mut transform in &mut cameras {
            transform.translation = eye;
        }
        *space = Space::Cctv;
        look.yaw = std::f32::consts::PI;
        eprintln!(
            "[world] debug: the camera stands in the CCTV room at {:?} in the room",
            in_room.at
        );
        return;
    };
    let Some(room) = plan.0.rooms.get(n) else {
        eprintln!("[world] debug: no room {n} to stand in, the camera stays where it is");
        return;
    };
    let eye = Vec3::from_array(layout::interior_origin(room.slot)) + Vec3::from_array(in_room.at);
    if *placed == Some(eye) {
        return;
    }
    *placed = Some(eye);
    for mut transform in &mut cameras {
        transform.translation = eye;
    }
    *space = Space::Room(room.slot);
    eprintln!(
        "[world] debug: the camera stands in room {n} ({}) at {:?} in the room",
        room.project, in_room.at
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_debug_switch_reads_a_room_and_an_optional_place_in_it() {
        let at = |s: &str| InRoom::parse(s).map(|r| (r.room, r.at));
        assert_eq!(at("0"), Some((Some(0), [-3.5, 1.6, -0.6])));
        assert_eq!(at("2@-4.4,-2.4"), Some((Some(2), [-4.4, 1.6, -2.4])));
        assert_eq!(at("1@ 0, 2.8 ,-0.1"), Some((Some(1), [0.0, 2.8, -0.1])));
        // The CCTV room, just inside its doorway (x -0.6..0.6), or anywhere in its frame.
        assert_eq!(at("cctv"), Some((None, [0.0, 1.6, -0.6])));
        assert_eq!(at("cctv@2,-5.5"), Some((None, [2.0, 1.6, -5.5])));
        assert_eq!(at("x"), None);
        assert_eq!(at("0@1"), None);
        assert_eq!(at("0@1,2,3,4"), None);
        assert_eq!(at("0@a,b"), None);
    }
}
