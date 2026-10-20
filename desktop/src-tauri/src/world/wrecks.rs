//! Draws the wrecks the CCTV state machine says there are (management-rooms ticket 20;
//! spec: "Punishments in motion", the wreck). From a kill's blast on, and for as long as
//! the state machine keeps it ([`CctvState::wrecked`](super::cctv_state::CctvState::wrecked):
//! until the agent's state or flag changes, it finishes or its desk goes), the
//! agent's desk shows a wreck
//! instead of its robot:
//!
//! - the seated robot (`rooms::Figure`) and the desk set's chair are hidden; the desk,
//!   the PC and the rest of the desk set stay standing, and nothing about the desk set's
//!   collision changes (desks never block);
//! - the `wreck_parts` pieces lie at the desk: a scorch on the floor and on the desk top,
//!   the robot's charred torso, head, arms and legs scattered about, its chair knocked
//!   over; a few faint sparks flicker among them and a thin wisp of smoke rises.
//!
//! The wreck is a child of the desk entity, so it moves with the desk if desks repack
//! and takes its room's render layer. A wreck the state machine no longer lists is taken
//! away at once and the robot and chair shown again, with no animation.

use std::collections::HashMap;

use bevy::app::PropagateSet;
use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;

use super::feeds::Cctv;
use super::floor_plan::DeskId;
use super::pieces::Kit;
use super::punishments::random;
use super::rooms::{Figure, Seated};
use super::surfaces::SwapMaterials;

/// The desk set's chair, by its node name (`desk_set.py`), which a wreck knocks over.
const CHAIR: &str = "chair";
/// The parts a wreck leaves, in its desk's frame (the desk set's origin under the seat,
/// facing the doorway, +Z): each piece, where it lies and its turn about the vertical.
/// The scorch on the desk top goes where `wreck_parts.py` says.
const PARTS: [(&str, [f32; 3], f32); 10] = [
    ("scorch_floor", [0.0, 0.0, 0.0], 0.0),
    ("scorch_desk", [0.0, 0.76, 0.57], 0.0),
    ("wreck_chair", [-0.1, 0.0, -0.62], 0.3),
    ("wreck_torso", [0.45, 0.0, -0.35], 0.8),
    ("wreck_head", [-0.6, 0.0, -0.25], 2.1),
    ("wreck_arm", [0.8, 0.0, 0.15], -0.4),
    ("wreck_arm", [-0.35, 0.0, -1.05], 1.3),
    ("wreck_leg", [0.55, 0.0, -0.95], 2.6),
    ("wreck_leg", [-0.75, 0.0, 0.2], -1.1),
    ("wreck_debris", [0.2, 0.0, 0.35], 0.7),
];
/// Where the sparks flicker, low among the parts, and where the wisp rises from.
const SPARKS: [[f32; 3]; 3] = [[0.45, 0.12, -0.35], [-0.6, 0.1, -0.25], [0.1, 0.25, -0.55]];
const WISP_FROM: [f32; 3] = [0.45, 0.2, -0.35];
/// How long one wisp of smoke takes to rise, in milliseconds.
const WISP_EVERY: i64 = 3_000;

pub struct WrecksPlugin;

/// Where the wrecks are laid in `PostUpdate`.
#[derive(SystemSet, Clone, Debug, PartialEq, Eq, Hash)]
pub struct LayWrecks;

impl Plugin for WrecksPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Laid>().add_systems(
            PostUpdate,
            (lay_wrecks, smoulder)
                .chain()
                .in_set(LayWrecks)
                .before(PropagateSet::<RenderLayers>::default())
                .before(SwapMaterials)
                .before(bevy::transform::TransformSystems::Propagate),
        );
    }
}

/// One wreck at its agent's desk.
struct Wreck {
    desk: Entity,
    root: Entity,
    /// What it hides: the robot and the desk set's chair.
    hidden: Vec<Entity>,
    sparks: [Entity; SPARKS.len()],
    wisp: Entity,
    /// Where its desk is, as the state machine last said (room, row, column): it moves
    /// with the desk, which only the log needs to know.
    at: (String, usize, usize),
}

/// The wrecks laid, by agent.
#[derive(Resource, Default)]
struct Laid(HashMap<DeskId, Wreck>);

/// Lays a wreck at each desk the state machine says is wrecked, and takes away each one
/// it no longer does, showing the robot and its chair again.
#[allow(clippy::too_many_arguments)]
fn lay_wrecks(
    mut commands: Commands,
    kit: Option<Res<Kit>>,
    cctv: Res<Cctv>,
    mut laid: ResMut<Laid>,
    desks: Query<(Entity, &Seated)>,
    children: Query<&Children>,
    figures: Query<(), With<Figure>>,
    names: Query<&Name>,
    mut visibilities: Query<&mut Visibility>,
) {
    let Some(kit) = kit else { return };
    let wrecked = cctv.0.wrecked();
    laid.0.retain(|agent, wreck| {
        let listed = wrecked.iter().find(|w| &w.desk == agent);
        let stays = listed.is_some() && desks.get(wreck.desk).is_ok_and(|(_, s)| &s.0 == agent);
        if let (true, Some(w)) = (stays, listed) {
            let at = (w.project.clone(), w.row, w.column);
            if wreck.at != at {
                eprintln!(
                    "[world] wreck at {} moved with its desk to {} (CAM {:02}, row {} column {})",
                    agent.agent, w.project, w.cam, w.row, w.column
                );
                wreck.at = at;
            }
            return true;
        }
        if let Ok(mut root) = commands.get_entity(wreck.root) {
            root.try_despawn();
        }
        for &part in &wreck.hidden {
            if let Ok(mut visibility) = visibilities.get_mut(part) {
                *visibility = Visibility::Inherited;
            }
        }
        eprintln!("[world] wreck at {}: cleared", agent.agent);
        false
    });
    for agent in wrecked {
        if laid.0.contains_key(&agent.desk) {
            continue;
        }
        let Some((desk, _)) = desks.iter().find(|(_, seated)| seated.0 == agent.desk) else {
            continue;
        };
        let robot = children
            .get(desk)
            .into_iter()
            .flatten()
            .copied()
            .filter(|piece| figures.contains(*piece));
        let chair = children
            .iter_descendants(desk)
            .filter(|part| names.get(*part).is_ok_and(|name| name.as_str() == CHAIR));
        let hidden: Vec<Entity> = robot.chain(chair).collect();
        for &part in &hidden {
            if let Ok(mut visibility) = visibilities.get_mut(part) {
                *visibility = Visibility::Hidden;
            }
        }
        let root = commands
            .spawn((
                Transform::IDENTITY,
                Visibility::Inherited,
                ChildOf(desk),
                // A wreck stays hidden from a player sat in its robot, like the robot.
                super::seat::HiddenWhenSeated,
            ))
            .id();
        let mut piece = |name: &str, at: Transform| {
            commands
                .spawn((kit.piece(name), at, Visibility::Inherited, ChildOf(root)))
                .id()
        };
        for (name, at, turn) in PARTS {
            piece(
                name,
                Transform::from_translation(Vec3::from_array(at))
                    .with_rotation(Quat::from_rotation_y(turn)),
            );
        }
        let sparks = SPARKS.map(|at| {
            piece(
                "fx_spark",
                Transform::from_translation(Vec3::from_array(at)).with_scale(Vec3::splat(0.5)),
            )
        });
        let wisp = piece("fx_smoke_puff", Transform::from_scale(Vec3::ZERO));
        eprintln!(
            "[world] wreck at {} in {} (CAM {:02}, row {} column {}), hiding {} parts",
            agent.desk.agent,
            agent.project,
            agent.cam,
            agent.row,
            agent.column,
            hidden.len()
        );
        laid.0.insert(
            agent.desk.clone(),
            Wreck {
                desk,
                root,
                hidden,
                sparks,
                wisp,
                at: (agent.project.clone(), agent.row, agent.column),
            },
        );
    }
}

/// Faint sparks flickering among the parts and a thin wisp of smoke rising, on the
/// world's clock.
fn smoulder(
    cctv: Res<Cctv>,
    laid: Res<Laid>,
    mut transforms: Query<&mut Transform>,
    mut visibilities: Query<&mut Visibility>,
) {
    let now = cctv.0.now();
    for (n, wreck) in laid.0.values().enumerate() {
        let tick = (now / 110) as u64;
        for (k, (&spark, at)) in wreck.sparks.iter().zip(SPARKS).enumerate() {
            let r = |j: u64| random(0, tick.wrapping_mul(31) ^ (n * 8 + k) as u64 ^ (j << 20));
            let on = r(0) < 0.3;
            if let Ok(mut visibility) = visibilities.get_mut(spark) {
                visibility.set_if_neq(if on {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                });
            }
            if on {
                if let Ok(mut transform) = transforms.get_mut(spark) {
                    transform.set_if_neq(
                        Transform::from_translation(
                            Vec3::from_array(at)
                                + Vec3::new(r(1) - 0.5, r(2) * 0.5, r(3) - 0.5) * 0.12,
                        )
                        .with_rotation(Quat::from_rotation_z((r(4) - 0.5) * 2.0))
                        .with_scale(Vec3::splat(0.35 + 0.3 * r(5))),
                    );
                }
            }
        }
        let t = (now.rem_euclid(WISP_EVERY)) as f32 / WISP_EVERY as f32;
        if let Ok(mut transform) = transforms.get_mut(wreck.wisp) {
            transform.set_if_neq(
                Transform::from_translation(
                    Vec3::from_array(WISP_FROM) + Vec3::new(0.03 * t, 0.9 * t, 0.0),
                )
                .with_scale(Vec3::splat(0.03 + 0.07 * t)),
            );
        }
    }
}
