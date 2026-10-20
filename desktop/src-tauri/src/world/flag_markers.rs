//! Draws the flag markers (management-rooms ticket 13) that [`Markers`] asks for: HUD
//! corner brackets with a small ring, locked on each flagged robot's head, glowing in
//! its flag's colour, pulsing or steady as `marker` says.
//!
//! A marker is a child of its robot's desk, so it moves with the desk and takes its
//! room's render layer from the interior's root (`rooms`): it shows wherever the room is
//! drawn, in the room and through its doorway. It is built from its own meshes and
//! unlit materials, outside the glTF pieces, with no fog and no shadows, so Textures,
//! Clutter and the haze never touch it.
//!
//! [`Markers`] is the seam: [`want_markers`] fills it from the CCTV state machine
//! ([`CctvState::markers`](super::cctv_state::CctvState::markers)): every
//! flagged root agent's marker as the latest plan has it, but a killed robot's, from the
//! moment KILL fires until its wreck clears. It runs in `PostUpdate`, after the plan and
//! the clicks of this frame have reached the state machine, so a marker goes in the
//! frame KILL fires and comes back in the frame the wreck clears.

use std::collections::HashMap;

use bevy::app::PropagateSet;
use bevy::camera::visibility::RenderLayers;
use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::prelude::*;

use agents::FlagKind;

use super::feeds::Cctv;
use super::floor_plan::DeskId;
use super::marker::Marker;
use super::rooms::Seated;

/// Which robots carry a marker now, by desk, and how.
#[derive(Resource, Default, Clone, Debug, PartialEq)]
pub struct Markers(pub HashMap<DeskId, Marker>);

/// Where the marker's centre stands in its desk's frame: on a full-size robot's head
/// (its face at y 1.10, z 0.07), a little in front of the face, short of the PC's
/// glass (z 0.59 at its nearest). Only root agents are flagged and they are full size, so this never
/// scales.
const CENTRE: Vec3 = Vec3::new(0.0, 1.10, 0.25);
/// The brackets' square, the length of each bracket's arms, and their thickness: thick
/// enough to hold together in a doorway's half-resolution image across a room.
const SIDE: f32 = 0.42;
const ARM: f32 = 0.095;
const BAR: f32 = 0.022;
const DEPTH: f32 = 0.01;
/// The ring in the middle: its radius and its thickness.
const RING: f32 = 0.06;
const RING_BAR: f32 = 0.009;
/// How strongly a marker glows at full brightness: low enough that the tone mapping
/// keeps its red red (signs glow at about 2.4).
const GLOW: f32 = 1.1;

/// Where the markers are placed in `PostUpdate`.
#[derive(SystemSet, Clone, Debug, PartialEq, Eq, Hash)]
pub struct PlaceMarkers;

pub struct FlagMarkersPlugin;

impl Plugin for FlagMarkersPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Markers>()
            .add_systems(Startup, make_look)
            .add_systems(Update, pulse_markers)
            .add_systems(
                PostUpdate,
                (want_markers, place_markers)
                    .chain()
                    .in_set(PlaceMarkers)
                    .before(PropagateSet::<RenderLayers>::default())
                    .before(bevy::transform::TransformSystems::Propagate),
            );
    }
}

/// The meshes and materials every marker shares: one material per flag kind, so one
/// change pulses every *asking you something* marker together.
#[derive(Resource)]
struct Look {
    across: Handle<Mesh>,
    upright: Handle<Mesh>,
    ring: Handle<Mesh>,
    asking: Handle<StandardMaterial>,
    waiting: Handle<StandardMaterial>,
}

impl Look {
    fn material(&self, kind: FlagKind) -> Handle<StandardMaterial> {
        match kind {
            FlagKind::AskingYouSomething => self.asking.clone(),
            FlagKind::WaitingOnYou => self.waiting.clone(),
        }
    }
}

/// A desk's marker entity, and what it shows.
#[derive(Component)]
struct Carries {
    marker: Marker,
    entity: Entity,
}

fn make_look(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let mut glowing = |kind| {
        materials.add(StandardMaterial {
            base_color: glow(Marker { kind }, 0.0),
            unlit: true,
            fog_enabled: false,
            ..default()
        })
    };
    let asking = glowing(FlagKind::AskingYouSomething);
    let waiting = glowing(FlagKind::WaitingOnYou);
    commands.insert_resource(Look {
        across: meshes.add(Cuboid::new(ARM, BAR, DEPTH)),
        upright: meshes.add(Cuboid::new(BAR, ARM, DEPTH)),
        ring: meshes.add(Torus::new(RING - RING_BAR / 2.0, RING + RING_BAR / 2.0)),
        asking,
        waiting,
    });
}

/// The marker's unlit colour `secs` into the world.
fn glow(marker: Marker, secs: f32) -> Color {
    let [r, g, b] = marker.colour();
    let k = GLOW * marker.brightness(secs);
    Color::LinearRgba(LinearRgba::rgb(r * k, g * k, b * k))
}

/// The markers the CCTV state machine asks for now.
fn want_markers(cctv: Res<Cctv>, mut markers: ResMut<Markers>) {
    if cctv.is_changed() {
        markers.set_if_neq(Markers(cctv.0.markers()));
    }
}

/// Gives each seated robot the marker [`Markers`] asks for, or none, when that changes or
/// a desk is seated. A desk that goes takes its marker along.
fn place_markers(
    mut commands: Commands,
    look: Option<Res<Look>>,
    markers: Res<Markers>,
    desks: Query<(Entity, &Seated, Option<&Carries>)>,
    seated: Query<(), Added<Seated>>,
) {
    let Some(look) = look else { return };
    if !markers.is_changed() && seated.is_empty() {
        return;
    }
    let mut shown = Vec::new();
    for (desk, Seated(id), carries) in &desks {
        let want = markers.0.get(id).copied();
        if carries.map(|c| c.marker) == want {
            continue;
        }
        if let Some(old) = carries {
            commands.entity(old.entity).despawn();
            commands.entity(desk).remove::<Carries>();
        }
        let Some(marker) = want else {
            shown.push(format!("{} none", id.agent));
            continue;
        };
        let entity = spawn_marker(&mut commands, &look, marker);
        commands
            .entity(entity)
            .insert((ChildOf(desk), super::seat::HiddenWhenSeated));
        commands.entity(desk).insert(Carries { marker, entity });
        shown.push(format!("{} {:?}", id.agent, marker.kind));
    }
    if !shown.is_empty() {
        eprintln!("[world] flag markers: {}", shown.join(", "));
    }
}

/// One marker, facing the doorway (+Z) as its robot does: four corner brackets round a
/// small ring.
fn spawn_marker(commands: &mut Commands, look: &Look, marker: Marker) -> Entity {
    let material = MeshMaterial3d(look.material(marker.kind));
    let part = |mesh: &Handle<Mesh>, transform: Transform| {
        (
            Mesh3d(mesh.clone()),
            material.clone(),
            transform,
            NotShadowCaster,
            NotShadowReceiver,
        )
    };
    let half = SIDE / 2.0;
    commands
        .spawn((
            Name::new("flag_marker"),
            Transform::from_translation(CENTRE),
            Visibility::default(),
        ))
        .with_children(|parts| {
            for sx in [-1.0, 1.0] {
                for sy in [-1.0, 1.0] {
                    let corner = Vec2::new(sx * half, sy * half);
                    // Each arm runs inward from the corner, overlapping at it.
                    let across = Vec3::new(corner.x - sx * (ARM - BAR) / 2.0, corner.y, 0.0);
                    let upright = Vec3::new(corner.x, corner.y - sy * (ARM - BAR) / 2.0, 0.0);
                    parts.spawn(part(&look.across, Transform::from_translation(across)));
                    parts.spawn(part(&look.upright, Transform::from_translation(upright)));
                }
            }
            // The torus lies in XZ; stand it up to face +Z.
            parts.spawn(part(
                &look.ring,
                Transform::from_rotation(Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)),
            ));
        })
        .id()
}

/// Pulses the *asking you something* material; *waiting on you* stays as made.
fn pulse_markers(
    time: Res<Time>,
    look: Option<Res<Look>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let Some(look) = look else { return };
    let asking = Marker {
        kind: FlagKind::AskingYouSomething,
    };
    if let Some(mut material) = materials.get_mut(&look.asking) {
        material.base_color = glow(asking, time.elapsed_secs());
    }
}
