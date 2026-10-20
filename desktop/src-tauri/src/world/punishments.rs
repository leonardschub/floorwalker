//! Draws the punishments the CCTV state machine says are playing (management-rooms
//! ticket 19; spec: "Punishments in motion", "Punishment and wreck rules over time").
//! The state machine (`cctv_state`) decides everything: which agent, which phase and how
//! far through it, on the world's clock, and keep or clear. This only puts effects where
//! it says, frame by frame:
//!
//! - **The clock:** every update gives the state machine the time
//!   ([`CctvState::advance`](super::cctv_state::CctvState::advance)), whether or not the
//!   CCTV room is seen and while the world is kept warm or suspended; a punishment's
//!   phase is a function of that clock alone, so it always ends the same way.
//! - **A rig per punishment,** a child of its agent's desk (`rooms::Seated`): it moves
//!   with the desk if desks repack, and takes its room's render layer from the
//!   interior's root, so it shows in the room, through its doorway, on its feed and on
//!   the focus monitors. A punishment that ends or is cleared takes its rig with it at
//!   once.
//! - **Work harder:** the `effects` pieces (ADR-0012) moved, turned, shown and hidden by
//!   code: blue arcs (`fx_arc`) crackling on the ceiling over the robot; the forked bolt
//!   (`fx_bolt`) from the ceiling to its head, flickering, with a glowing ball where it
//!   hits and a blue-white point light that floods the room; the robot charred black
//!   (`surfaces::Charred`, its face too) with sparks (`fx_spark`) and smoke puffs
//!   (`fx_smoke_puff`); then its paint back and one wisp of smoke. The seated robot
//!   never moves, nothing collides, and the player is never touched.
//!
//! - **Kill**: the squad (`squad_robot`, three of them) placed and posed
//!   where `squad` says for each moment, each limb swung about its joint by code (no
//!   animation clips): running in from the doorway along the aisles, two pointing and
//!   one crouched planting the charge (`squad_charge`, its light blinking), running
//!   back out and vanishing at the doorway. Then the blast: a fireball (`fx_fireball`)
//!   swelling and collapsing, dark smoke (`fx_smoke_puff`) rising, debris in the
//!   robot's paint (`wreck_debris`) thrown about the room, and a strong orange point
//!   light. The wreck the robot becomes from the blast on is `wrecks`'. The squad
//!   never collides, and the player is never touched.

use std::collections::HashMap;
use std::f32::consts::TAU;

use bevy::app::PropagateSet;
use bevy::camera::visibility::RenderLayers;
use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::prelude::*;

use super::cctv_state::{Kind, Phase, Punishment};
use super::clock::WorldClock;
use super::feeds::Cctv;
use super::floor_plan::{layout, DeskId};
use super::lettering::FacePlate;
use super::pieces::Kit;
use super::rooms::{Figure, Seated};
use super::squad::{self, Pose, SQUAD};
use super::surfaces::{Char, Charred, SwapMaterials};

/// The room's ceiling, where the arcs crackle and the bolt starts, and the top of a
/// full-size robot's head, where it strikes (`robot.py`: the head's centre 0.64 over the
/// seat, 0.18 high), in its desk's frame; the head's centre.
const CEILING: f32 = 2.96;
const HEAD_TOP: f32 = layout::ROBOT_SEAT[1] + 0.64 + 0.09;
const HEAD: Vec3 = Vec3::new(0.0, layout::ROBOT_SEAT[1] + 0.64, 0.03);
/// How many arcs, sparks and smoke puffs a rig has.
const ARCS: usize = 3;
const SPARKS: usize = 6;
const PUFFS: usize = 3;
/// The flash: a blue-white point light over the robot, about forty tube lights strong
/// at its peak, reaching every corner of the deepest room.
const FLASH_LUMENS: f32 = 3_500_000.0;
const FLASH_RANGE: f32 = 30.0;
const FLASH_AT: Vec3 = Vec3::new(0.0, 2.4, 0.6);
/// The glowing ball where the bolt hits, and its glow.
const GLOW_RADIUS: f32 = 0.09;
const GLOW: LinearRgba = LinearRgba::rgb(3.0, 3.6, 5.0);
/// The kill: where the charge goes on a full-size robot's chest (`robot.py`: the
/// torso's front face, in its desk's frame), the blast's centre, smoke puffs and debris.
const CHEST: Vec3 = Vec3::new(0.0, layout::ROBOT_SEAT[1] + 0.34, 0.135);
const SMOKE: usize = 5;
const DEBRIS: usize = 10;
/// The blast's light: orange, stronger than the lightning's flash, fading over about
/// a second and a half.
const BLAST_LUMENS: f32 = 6_000_000.0;
const BLAST_AT: Vec3 = Vec3::new(0.0, 1.3, 0.3);
/// The squad robot's joints, by node name (`squad_robot.py`).
const JOINTS: [&str; 11] = [
    "squad_pelvis",
    "squad_torso",
    "squad_head",
    "squad_arm_upper_l",
    "squad_arm_lower_l",
    "squad_arm_upper_r",
    "squad_arm_lower_r",
    "squad_leg_upper_l",
    "squad_leg_lower_l",
    "squad_leg_upper_r",
    "squad_leg_lower_r",
];
const CHARGE_LED: &str = "squad_charge_led";

pub struct PunishmentsPlugin;

impl Plugin for PunishmentsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Rigs>()
            .add_systems(Startup, make_look)
            .add_systems(PreUpdate, tick_clock)
            // After this update's plan (`Update`), so a cleared punishment's effects go
            // in the frame it is cleared; before the render layers, materials and
            // transforms are propagated, so a new rig shows in its room from its first
            // frame.
            .add_systems(
                PostUpdate,
                (follow_punishments, play_effects, play_kills)
                    .chain()
                    .before(PropagateSet::<RenderLayers>::default())
                    .before(SwapMaterials)
                    .before(bevy::transform::TransformSystems::Propagate),
            );
    }
}

/// Gives the state machine the time, every update: punishments move on to their phase
/// for now, and those over end.
fn tick_clock(clock: Res<WorldClock>, mut cctv: ResMut<Cctv>) {
    cctv.0.advance(clock.0);
}

/// The impact glow's mesh and material, shared by every rig.
#[derive(Resource)]
struct Look {
    ball: Handle<Mesh>,
    glow: Handle<StandardMaterial>,
}

fn make_look(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.insert_resource(Look {
        ball: meshes.add(Sphere::new(1.0).mesh().ico(2).expect("a small ico sphere")),
        glow: materials.add(StandardMaterial {
            base_color: GLOW.into(),
            unlit: true,
            fog_enabled: false,
            ..default()
        }),
    });
}

/// One punishment's effects on its agent's desk.
struct Rig {
    /// The desk it hangs on, and its own root, a child of the desk.
    desk: Entity,
    root: Entity,
    /// Work harder's effects, or the kill's.
    lightning: Option<Lightning>,
    kill: Option<KillParts>,
    /// Its punishment's kind and start, to tell one that ended from one cleared.
    kind: Kind,
    began: i64,
    /// Whether its robot is charred now, and the phase last logged.
    charred: bool,
    phase: Phase,
}

/// Work harder's effects.
#[derive(Clone, Copy)]
struct Lightning {
    arcs: [Entity; ARCS],
    bolt: Entity,
    glow: Entity,
    flash: Entity,
    sparks: [Entity; SPARKS],
    puffs: [Entity; PUFFS],
}

/// The kill's squad and blast.
struct KillParts {
    /// The desk's seat it was last routed to, as (row, column).
    seat: (usize, usize),
    squad: [Squaddie; SQUAD],
    charge: Entity,
    /// The charge's light, once its scene has spawned.
    led: Option<Entity>,
    fireball: Entity,
    smoke: [Entity; SMOKE],
    debris: [Entity; DEBRIS],
    blast: Entity,
}

/// One robot of the squad: its piece, and its joints with their rest poses once its
/// scene has spawned.
struct Squaddie {
    root: Entity,
    joints: Option<[(Entity, Transform); JOINTS.len()]>,
}

/// The rigs, by the agent they punish.
#[derive(Resource, Default)]
struct Rigs(HashMap<DeskId, Rig>);

/// Spawns a rig on the desk of each new punishment, and takes away the rig of each one
/// that ended or was cleared, giving its robot its paint back.
#[allow(clippy::too_many_arguments)]
fn follow_punishments(
    mut commands: Commands,
    kit: Option<Res<Kit>>,
    look: Option<Res<Look>>,
    cctv: Res<Cctv>,
    mut rigs: ResMut<Rigs>,
    desks: Query<(Entity, &Seated)>,
    uncharring: Uncharring,
) {
    let (Some(kit), Some(look)) = (kit, look) else {
        return;
    };
    let playing = cctv.0.punishments();
    let now = cctv.0.now();
    rigs.0.retain(|desk, rig| {
        if playing.iter().any(|p| &p.agent.desk == desk) {
            return true;
        }
        if rig.charred {
            uncharring.char(&mut commands, rig.desk, false);
        }
        if let Ok(mut root) = commands.get_entity(rig.root) {
            root.try_despawn();
        }
        eprintln!(
            "[world] punishment on {}: {} after {:?} (last drawn)",
            desk.agent,
            if now - rig.began >= rig.kind.lasts() {
                "over"
            } else {
                "cleared"
            },
            rig.phase
        );
        false
    });
    for punishment in playing {
        if rigs.0.contains_key(&punishment.agent.desk) {
            continue;
        }
        let Some((desk, _)) = desks
            .iter()
            .find(|(_, seated)| seated.0 == punishment.agent.desk)
        else {
            continue;
        };
        let rig = spawn_rig(&mut commands, &kit, &look, desk, punishment);
        eprintln!(
            "[world] punishment: {:?} on {} in {} (CAM {:02}, row {} column {}), {:?}",
            punishment.kind,
            punishment.agent.desk.agent,
            punishment.agent.project,
            punishment.agent.cam,
            punishment.agent.row,
            punishment.agent.column,
            punishment.phase
        );
        rigs.0.insert(punishment.agent.desk.clone(), rig);
    }
}

/// A rig for `punishment` on `desk`, every effect hidden until its phase.
fn spawn_rig(
    commands: &mut Commands,
    kit: &Kit,
    look: &Look,
    desk: Entity,
    punishment: &Punishment,
) -> Rig {
    let root = commands
        .spawn((Transform::IDENTITY, Visibility::Inherited, ChildOf(desk)))
        .id();
    let mut piece = |name: &str| {
        commands
            .spawn((
                kit.piece(name),
                Transform::IDENTITY,
                Visibility::Hidden,
                ChildOf(root),
            ))
            .id()
    };
    if punishment.kind == Kind::Kill {
        let squad = [(); SQUAD].map(|_| Squaddie {
            root: piece("squad_robot"),
            joints: None,
        });
        let charge = piece("squad_charge");
        let fireball = piece("fx_fireball");
        let smoke = [(); SMOKE].map(|_| piece("fx_smoke_puff"));
        let debris = [(); DEBRIS].map(|_| piece("wreck_debris"));
        let blast = commands
            .spawn((
                PointLight {
                    color: Color::srgb(1.0, 0.55, 0.2),
                    intensity: 0.0,
                    range: FLASH_RANGE,
                    radius: 0.3,
                    shadow_maps_enabled: false,
                    ..default()
                },
                Transform::from_translation(BLAST_AT),
                Visibility::Hidden,
                ChildOf(root),
            ))
            .id();
        return Rig {
            desk,
            root,
            lightning: None,
            kill: Some(KillParts {
                seat: (punishment.agent.row, punishment.agent.column),
                squad,
                charge,
                led: None,
                fireball,
                smoke,
                debris,
                blast,
            }),
            kind: punishment.kind,
            began: punishment.began,
            charred: false,
            phase: punishment.phase,
        };
    }
    let arcs = [(); ARCS].map(|_| piece("fx_arc"));
    let bolt = piece("fx_bolt");
    let sparks = [(); SPARKS].map(|_| piece("fx_spark"));
    let puffs = [(); PUFFS].map(|_| piece("fx_smoke_puff"));
    let glow = commands
        .spawn((
            Mesh3d(look.ball.clone()),
            MeshMaterial3d(look.glow.clone()),
            Transform::from_translation(HEAD).with_scale(Vec3::splat(GLOW_RADIUS)),
            Visibility::Hidden,
            NotShadowCaster,
            NotShadowReceiver,
            ChildOf(root),
        ))
        .id();
    let flash = commands
        .spawn((
            PointLight {
                color: Color::srgb(0.8, 0.88, 1.0),
                intensity: 0.0,
                range: FLASH_RANGE,
                radius: 0.1,
                shadow_maps_enabled: false,
                ..default()
            },
            Transform::from_translation(FLASH_AT),
            Visibility::Hidden,
            ChildOf(root),
        ))
        .id();
    Rig {
        desk,
        root,
        lightning: Some(Lightning {
            arcs,
            bolt,
            glow,
            flash,
            sparks,
            puffs,
        }),
        kill: None,
        kind: punishment.kind,
        began: punishment.began,
        charred: false,
        phase: punishment.phase,
    }
}

/// What charring a robot and giving its paint back needs: its parts below the desk, and
/// the face, whose own material is kept aside meanwhile.
#[derive(bevy::ecs::system::SystemParam)]
struct Uncharring<'w, 's> {
    children: Query<'w, 's, &'static Children>,
    figures: Query<'w, 's, (), With<Figure>>,
    faces: Query<
        'w,
        's,
        (
            &'static MeshMaterial3d<StandardMaterial>,
            Option<&'static Uncharred>,
        ),
        With<FacePlate>,
    >,
    char: Res<'w, Char>,
}

/// A charred face's own material, given back when its paint returns.
#[derive(Component)]
struct Uncharred(Handle<StandardMaterial>);

impl Uncharring<'_, '_> {
    /// Chars the robot at `desk` black, or gives it its paint back: every part of it,
    /// its face too.
    fn char(&self, commands: &mut Commands, desk: Entity, charred: bool) {
        let Ok(pieces) = self.children.get(desk) else {
            return;
        };
        for piece in pieces.iter().filter(|p| self.figures.contains(*p)) {
            for part in self.children.iter_descendants(piece) {
                if let Ok((material, uncharred)) = self.faces.get(part) {
                    match (charred, uncharred) {
                        (true, None) => {
                            commands.entity(part).insert((
                                Uncharred(material.0.clone()),
                                MeshMaterial3d(self.char.0.clone()),
                            ));
                        }
                        (false, Some(own)) => {
                            commands
                                .entity(part)
                                .insert(MeshMaterial3d(own.0.clone()))
                                .remove::<Uncharred>();
                        }
                        _ => {}
                    }
                    continue;
                }
                commands.entity(part).insert(Charred(charred));
            }
        }
    }
}

/// A repeatable random number in 0..1 for `seed` and `n` (a splitmix hash), so effects
/// jitter alike on every view of one frame.
pub(super) fn random(seed: u64, n: u64) -> f32 {
    let mut x = seed ^ n.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    x ^= x >> 33;
    x = x.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    x ^= x >> 33;
    x = x.wrapping_mul(0xC4CE_B9FE_1A85_EC53);
    x ^= x >> 33;
    (x >> 40) as f32 / (1u64 << 24) as f32
}

/// A punishment's seed: its agent and when it began.
fn seed_of(punishment: &Punishment) -> u64 {
    let mut seed = punishment.began as u64;
    for byte in punishment.agent.desk.agent.bytes() {
        seed = seed.wrapping_mul(31).wrapping_add(u64::from(byte));
    }
    seed
}

/// Places, shows and hides each rig's effects for its punishment's phase now, and chars
/// or restores its robot.
fn play_effects(
    mut commands: Commands,
    cctv: Res<Cctv>,
    mut rigs: ResMut<Rigs>,
    mut transforms: Query<&mut Transform>,
    mut visibilities: Query<&mut Visibility>,
    mut lights: Query<&mut PointLight>,
    uncharring: Uncharring,
) {
    let now = cctv.0.now();
    let charred_now: Vec<&DeskId> = cctv.0.charred().iter().map(|agent| &agent.desk).collect();
    for punishment in cctv.0.punishments() {
        let Some(rig) = rigs.0.get_mut(&punishment.agent.desk) else {
            continue;
        };
        let (phase, progress) = (punishment.phase, punishment.progress);
        if rig.phase != phase {
            eprintln!(
                "[world] punishment on {}: {phase:?}",
                punishment.agent.desk.agent
            );
            rig.phase = phase;
        }
        let Some(fx) = rig.lightning else {
            continue;
        };
        let seed = seed_of(punishment);
        let elapsed = (now - punishment.began).max(0) as u64;
        let mut show = |entity: Entity, on: bool| {
            if let Ok(mut visibility) = visibilities.get_mut(entity) {
                visibility.set_if_neq(if on {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                });
            }
        };
        let mut place = |entity: Entity, at: Transform| {
            if let Ok(mut transform) = transforms.get_mut(entity) {
                transform.set_if_neq(at);
            }
        };

        // Arcs crackle on the ceiling over the robot until the bolt is gone, each
        // jumping to a new place and turn every 70 ms and out about a third of the time.
        let crackling = matches!(phase, Phase::Crackle | Phase::Strike);
        let tick = elapsed / 70;
        for (n, &arc) in fx.arcs.iter().enumerate() {
            let r = |k: u64| random(seed ^ tick, (n as u64) * 8 + k);
            let on = crackling && r(0) < 0.7;
            show(arc, on);
            if on {
                place(
                    arc,
                    Transform::from_xyz(
                        (r(1) - 0.5) * 1.2,
                        CEILING - r(2) * 0.05,
                        (r(3) - 0.5) * 1.0,
                    )
                    .with_rotation(
                        Quat::from_rotation_y(r(4) * TAU)
                            * Quat::from_rotation_x((r(5) - 0.5) * 0.6),
                    )
                    .with_scale(Vec3::splat(0.7 + r(6) * 0.8)),
                );
            }
        }

        // The bolt, from the ceiling to the top of the head, flickering out one 50 ms
        // in three and taking a new turn each time; the ball of light where it hits.
        let striking = phase == Phase::Strike;
        let tick = elapsed / 50;
        let bolt_on = striking && tick % 3 != 2;
        show(fx.bolt, bolt_on);
        if bolt_on {
            place(
                fx.bolt,
                Transform::from_xyz(0.0, CEILING, HEAD.z)
                    .with_rotation(Quat::from_rotation_y(random(seed, tick) * TAU))
                    .with_scale(Vec3::new(1.0, CEILING - HEAD_TOP, 1.0)),
            );
        }
        show(fx.glow, bolt_on);
        if bolt_on {
            place(
                fx.glow,
                Transform::from_translation(HEAD)
                    .with_scale(Vec3::splat(GLOW_RADIUS * (1.0 - 0.5 * progress))),
            );
        }
        // The flash floods the room while the bolt is in, fading as it goes.
        show(fx.flash, bolt_on);
        if let Ok(mut light) = lights.get_mut(fx.flash) {
            let want = if bolt_on {
                FLASH_LUMENS * (1.0 - 0.6 * progress)
            } else {
                0.0
            };
            if light.intensity != want {
                light.intensity = want;
            }
        }

        // Charred for a second (with Punishment effects off too, as the state machine
        // says), sparking and smoking (only with them on: `Phase::Charred`).
        let charred = charred_now.contains(&&punishment.agent.desk);
        if rig.charred != charred {
            uncharring.char(&mut commands, rig.desk, charred);
            rig.charred = charred;
            eprintln!(
                "[world] punishment on {}: robot {}",
                punishment.agent.desk.agent,
                if charred { "charred" } else { "paint back" }
            );
        }
        let sparking = phase == Phase::Charred;
        let tick = elapsed / 90;
        for (n, &spark) in fx.sparks.iter().enumerate() {
            let r = |k: u64| random(seed ^ tick.wrapping_add(n as u64 * 7), (n as u64) * 8 + k);
            let on = sparking && r(0) < 0.75 - 0.4 * progress;
            show(spark, on);
            if on {
                place(
                    spark,
                    Transform::from_xyz(
                        (r(1) - 0.5) * 0.5,
                        0.75 + r(2) * 0.55,
                        (r(3) - 0.5) * 0.35,
                    )
                    .with_rotation(Quat::from_euler(EulerRot::XYZ, r(4) * TAU, r(5) * TAU, 0.0))
                    .with_scale(Vec3::splat(0.6 + r(6) * 0.7)),
                );
            }
        }
        // Smoke rising off the head while charred, one puff after another; then the
        // one wisp as its paint returns.
        for (n, &puff) in fx.puffs.iter().enumerate() {
            let rise = match phase {
                Phase::Charred => Some((progress + n as f32 / PUFFS as f32).fract()),
                Phase::Wisp if n == 0 => Some(progress),
                _ => None,
            };
            show(puff, rise.is_some());
            if let Some(t) = rise {
                place(
                    puff,
                    Transform::from_translation(
                        HEAD + Vec3::new(0.02 * n as f32, 0.12 + t * 0.6, 0.0),
                    )
                    .with_scale(Vec3::splat(0.035 + t * 0.06)),
                );
            }
        }
    }
}

/// Shows or hides `entity`.
fn show(visibilities: &mut Query<&mut Visibility>, entity: Entity, on: bool) {
    if let Ok(mut visibility) = visibilities.get_mut(entity) {
        visibility.set_if_neq(if on {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }
}

/// Puts `entity` at `at`.
fn place(transforms: &mut Query<&mut Transform>, entity: Entity, at: Transform) {
    if let Ok(mut transform) = transforms.get_mut(entity) {
        transform.set_if_neq(at);
    }
}

/// Each joint's turn about its own X (positive tilts a hanging limb backward, a rising
/// one forward) for a pose, in [`JOINTS`]' order, and how far the hips drop.
fn angles(pose: Pose) -> ([f32; JOINTS.len()], f32) {
    // pelvis, torso, head, upper arm l, forearm l, upper arm r, forearm r,
    // thigh l, shin l, thigh r, shin r.
    match pose {
        Pose::Run { stride } => {
            let swing = (stride * TAU).sin();
            let lift = (stride * TAU).cos();
            (
                [
                    0.0,
                    0.22,
                    -0.15,
                    0.65 * swing,
                    -1.1,
                    -0.65 * swing,
                    -1.1,
                    -0.75 * swing,
                    0.25 + 0.95 * lift.max(0.0),
                    0.75 * swing,
                    0.25 + 0.95 * (-lift).max(0.0),
                ],
                0.03 * swing.abs(),
            )
        }
        Pose::Point { raise } => (
            [
                0.0,
                0.05,
                0.0,
                -1.15 * raise,
                -0.45 * raise,
                -1.5 * raise,
                0.0,
                -0.15,
                0.1,
                0.15,
                0.05,
            ],
            0.0,
        ),
        Pose::Crouch { reach } => (
            [
                0.0,
                0.35 + 0.15 * reach,
                -0.25,
                -0.3,
                -0.6,
                -0.7 - 0.8 * reach,
                -0.5 * (1.0 - reach),
                -1.25,
                2.0,
                -1.0,
                1.75,
            ],
            0.4,
        ),
    }
}

/// Places, poses, shows and hides each kill's squad, charge and blast for its phase now.
#[allow(clippy::too_many_arguments)]
fn play_kills(
    cctv: Res<Cctv>,
    mut rigs: ResMut<Rigs>,
    mut transforms: Query<&mut Transform>,
    mut visibilities: Query<&mut Visibility>,
    mut lights: Query<&mut PointLight>,
    children: Query<&Children>,
    names: Query<&Name>,
) {
    let now = cctv.0.now();
    for punishment in cctv.0.punishments() {
        let Some(Rig {
            kill: Some(parts), ..
        }) = rigs.0.get_mut(&punishment.agent.desk)
        else {
            continue;
        };
        let (phase, progress) = (punishment.phase, punishment.progress);
        let agent = &punishment.agent;
        if parts.seat != (agent.row, agent.column) {
            parts.seat = (agent.row, agent.column);
            eprintln!(
                "[world] kill squad on {} re-routed to its desk at row {} column {} in {}, {:?}",
                agent.desk.agent, agent.row, agent.column, agent.project, phase
            );
        }
        let [dx, dz] = layout::desk_position(agent.row, agent.column);
        let desk = Vec3::new(dx, 0.0, dz);

        // The squad, where it is now, each limb swung about its joint.
        let squad = punishment.squad();
        for (n, robot) in parts.squad.iter_mut().enumerate() {
            let trooper = squad.iter().find(|t| t.robot == n);
            show(&mut visibilities, robot.root, trooper.is_some());
            let Some(trooper) = trooper else {
                continue;
            };
            place(
                &mut transforms,
                robot.root,
                Transform::from_translation(Vec3::new(trooper.at[0], 0.0, trooper.at[1]) - desk)
                    .with_rotation(Quat::from_rotation_y(trooper.yaw)),
            );
            if robot.joints.is_none() {
                robot.joints = find_joints(robot.root, &children, &names, &transforms);
            }
            let Some(joints) = &robot.joints else {
                continue;
            };
            let (turns, drop) = angles(trooper.pose);
            for (n, ((joint, rest), turn)) in joints.iter().zip(turns).enumerate() {
                let mut at = rest.with_rotation(rest.rotation * Quat::from_rotation_x(turn));
                if n == 0 {
                    at.translation.y -= drop;
                }
                place(&mut transforms, *joint, at);
            }
        }

        // The charge on the robot's chest, its light blinking faster towards the blast.
        let planted = squad::charge_planted(phase, progress);
        show(&mut visibilities, parts.charge, planted);
        place(
            &mut transforms,
            parts.charge,
            Transform::from_translation(CHEST),
        );
        if parts.led.is_none() {
            parts.led = children.iter_descendants(parts.charge).find(|part| {
                names
                    .get(*part)
                    .is_ok_and(|name| name.as_str() == CHARGE_LED)
            });
        }
        if let Some(led) = parts.led {
            let every = if phase == Phase::FallBack { 240 } else { 500 };
            let elapsed = (now - punishment.began).max(0);
            show(&mut visibilities, led, elapsed % every < every / 2);
        }

        // The blast: seconds since it went off.
        let boom = phase == Phase::Boom;
        let b = progress * 2.0;
        let seed = seed_of(punishment);
        // The fireball swells for a quarter of a second, burns, and collapses.
        let size = if !boom {
            0.0
        } else if b < 0.25 {
            0.95 * (1.0 - (1.0 - b / 0.25).powi(2))
        } else if b < 0.55 {
            0.95 - 0.3 * (b - 0.25) / 0.3
        } else {
            0.65 * (1.0 - (b - 0.55) / 0.35)
        };
        show(&mut visibilities, parts.fireball, size > 0.01);
        if size > 0.01 {
            place(
                &mut transforms,
                parts.fireball,
                Transform::from_translation(CHEST + Vec3::new(0.0, 0.2 + 0.3 * b, 0.0))
                    .with_rotation(Quat::from_rotation_y(b * 2.0))
                    .with_scale(Vec3::splat(size)),
            );
        }
        // Dark smoke, one puff after another, rising and growing.
        for (n, &puff) in parts.smoke.iter().enumerate() {
            let t = b - 0.1 - 0.1 * n as f32;
            let on = boom && t > 0.0;
            show(&mut visibilities, puff, on);
            if on {
                let r = |k: u64| random(seed, 100 + n as u64 * 4 + k) - 0.5;
                place(
                    &mut transforms,
                    puff,
                    Transform::from_translation(
                        CHEST + Vec3::new(r(0) * 0.6, 0.3 + 0.8 * t, r(1) * 0.6),
                    )
                    .with_scale(Vec3::splat((0.22 + 0.25 * t).min(0.6))),
                );
            }
        }
        // Debris in the robot's paint, thrown about, falling and coming to rest on the
        // floor, inside the room's walls.
        let depth = layout::interior_depth(agent.rows);
        for (n, &piece) in parts.debris.iter().enumerate() {
            show(&mut visibilities, piece, boom);
            if !boom {
                continue;
            }
            let r = |k: u64| random(seed, 200 + n as u64 * 8 + k);
            let heading = r(0) * TAU;
            let speed = 1.5 + 2.5 * r(1);
            let up = 1.5 + 3.0 * r(2);
            const G: f32 = 9.8;
            let start = CHEST.y;
            let landing = (up + (up * up + 2.0 * G * start).sqrt()) / G;
            let t = b.min(landing);
            let at = CHEST
                + Vec3::new(
                    heading.sin() * speed * t,
                    up * t - G * t * t / 2.0,
                    heading.cos() * speed * t,
                );
            let room = Vec3::new(
                (at.x + desk.x).clamp(-4.6, 4.6),
                at.y.clamp(0.02, 2.9),
                (at.z + desk.z).clamp(-depth + 0.1, -0.1),
            );
            place(
                &mut transforms,
                piece,
                Transform::from_translation(room - desk)
                    .with_rotation(Quat::from_euler(
                        EulerRot::XYZ,
                        r(3) * TAU + t * 9.0,
                        r(4) * TAU + t * 7.0,
                        0.0,
                    ))
                    .with_scale(Vec3::splat(1.6)),
            );
        }
        // A strong orange light, at once, fading.
        let light = if boom && b < 1.6 {
            BLAST_LUMENS * (b / 0.05).min(1.0) * (-2.5 * b).exp()
        } else {
            0.0
        };
        show(&mut visibilities, parts.blast, light > 0.0);
        if let Ok(mut blast) = lights.get_mut(parts.blast) {
            if blast.intensity != light {
                blast.intensity = light;
            }
        }
    }
}

/// A squad robot's joints and their rest poses, once its scene has spawned.
fn find_joints(
    root: Entity,
    children: &Query<&Children>,
    names: &Query<&Name>,
    transforms: &Query<&mut Transform>,
) -> Option<[(Entity, Transform); JOINTS.len()]> {
    let found: Vec<(&str, Entity)> = children
        .iter_descendants(root)
        .filter_map(|part| {
            let name = names.get(part).ok()?;
            let joint = JOINTS.iter().find(|j| **j == name.as_str())?;
            Some((*joint, part))
        })
        .collect();
    let mut joints = [(Entity::PLACEHOLDER, Transform::IDENTITY); JOINTS.len()];
    for (slot, joint) in joints.iter_mut().zip(JOINTS) {
        let (_, part) = found.iter().find(|(name, _)| *name == joint)?;
        *slot = (*part, *transforms.get(*part).ok()?);
    }
    Some(joints)
}
