//! Aiming and clicking in the CCTV room (management-rooms ticket 18; spec: "Clicks reach
//! the world"). There is no console mode: the cursor stays captured, the player aims with
//! a small dot in the middle of the view and left-clicks.
//!
//! - **The centre dot** is a small unlit mark just in front of the eye, a child of the
//!   main camera on the layer of the space the player is in, shown while the player is
//!   in the CCTV room or a project room (to aim a right-click at a robot, agent
//!   screens), but not while seated.
//! - **Clicks** come from the page (`world_click`) as [`Clicks`]. Each one casts a ray from
//!   the eye through the middle of the view against the boxes of everything marked
//!   [`Clickable`] (the console's rows, cover and buttons, each wall feed, each focus
//!   monitor; a console row with nobody to list only blocks), with our own picking
//!   (`picking`); the nearest box wins, and the CCTV state machine hears what it was.
//!   SIT IN sits the user in the selected flagged agent (`seat::sit_in`: its seat, or
//!   the *terminal* hint) and leaves the state machine as it was. Outside the CCTV room
//!   clicks do nothing.

use bevy::camera::primitives::Aabb;
use bevy::camera::visibility::RenderLayers;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;

use super::cctv_state::Target;
use super::clock::WorldClock;
use super::collision::Space;
use super::feeds::Cctv;
use super::floor_plan::layout;
use super::picking::{self, Boxed, Ray};
use super::portals::{FollowMain, PortalCamera};
use super::seat::{self, PlayerSeat};
use super::walk::PlayerEye;

/// Primary-button presses the page sent while captured, not yet picked.
#[derive(Resource, Default)]
pub struct Clicks(pub u32);

/// Something a click can hit: what it stands for, or none for something that only
/// blocks the ray (an empty console row). Its box is its mesh's bounds, or its mesh
/// descendants' (a glTF node's), turned with it.
#[derive(Component, Clone, Debug, PartialEq)]
pub struct Clickable(pub Option<Target>);

/// How far in front of the eye the centre dot floats (past the camera's near plane,
/// `collision::NEAR`), and its radius there: about seven pixels across a 900-pixel-high
/// view at the camera's default 45° field of view.
const DOT_DISTANCE: f32 = 0.1;
const DOT_RADIUS: f32 = 0.00035;
/// The dot's colour: the CCTV room's cold cyan, near white.
const DOT_COLOUR: Color = Color::srgb(0.75, 0.97, 1.0);
/// Added to each box on every side, so a flat quad has some thickness to hit.
const PAD: f32 = 0.004;

/// The centre dot.
#[derive(Component)]
struct CentreDot;

pub struct AimPlugin;

impl Plugin for AimPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Clicks>().add_systems(
            Update,
            (
                spawn_dot,
                show_dot,
                pick_clicks.after(FollowMain),
                debug_clickables.after(FollowMain),
            ),
        );
    }
}

/// Puts the centre dot in front of the main camera, once.
fn spawn_dot(
    mut commands: Commands,
    eye: Single<Entity, (With<PlayerEye>, Without<PortalCamera>)>,
    dots: Query<(), With<CentreDot>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if !dots.is_empty() {
        return;
    }
    commands.spawn((
        CentreDot,
        Mesh3d(meshes.add(Circle::new(DOT_RADIUS))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: DOT_COLOUR,
            unlit: true,
            fog_enabled: false,
            ..default()
        })),
        Transform::from_xyz(0.0, 0.0, -DOT_DISTANCE),
        NotShadowCaster,
        RenderLayers::from_layers(&[layout::CCTV_LAYER]),
        Visibility::Hidden,
        ChildOf(*eye),
    ));
}

/// Shows the centre dot while the player is in the CCTV room or a project room and not
/// seated, on that space's layer.
fn show_dot(
    space: Res<Space>,
    seat: Res<PlayerSeat>,
    mut dots: Query<(&mut Visibility, &mut RenderLayers), With<CentreDot>>,
) {
    let want = if *space != Space::Corridor && !seat.seated() {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    let layers = RenderLayers::from_layers(&[space.render_layer()]);
    for (mut dot, mut on) in &mut dots {
        if *on != layers {
            *on = layers.clone();
        }
        if *dot != want {
            *dot = want;
            eprintln!(
                "[world] centre dot {}",
                if want == Visibility::Hidden {
                    "hidden"
                } else {
                    "shown"
                }
            );
        }
    }
}

/// Picks what each click aims at, from the eye through the middle of the view, and hands
/// it to the CCTV state machine, or, for SIT IN, sits the user in the selected agent.
/// Outside the CCTV room a click does nothing.
#[allow(clippy::too_many_arguments)]
fn pick_clicks(
    mut commands: Commands,
    mut clicks: ResMut<Clicks>,
    space: Res<Space>,
    eye: Single<&Transform, (With<PlayerEye>, Without<PortalCamera>)>,
    clickables: Query<(Entity, &Clickable)>,
    children: Query<&Children>,
    bounds: Query<(&Aabb, &GlobalTransform)>,
    clock: Res<WorldClock>,
    mut cctv: ResMut<Cctv>,
) {
    if clicks.0 == 0 {
        return;
    }
    let count = std::mem::take(&mut clicks.0);
    if *space != Space::Cctv {
        eprintln!("[world] click outside the CCTV room: nothing");
        return;
    }
    let ray = Ray {
        origin: eye.translation,
        direction: *eye.forward(),
    };
    let boxes = clickables.iter().flat_map(|(entity, clickable)| {
        std::iter::once(entity)
            .chain(children.iter_descendants(entity))
            .filter_map(|part| bounds.get(part).ok())
            .map(|(aabb, at)| (clickable.0.clone(), boxed(aabb, at)))
            .collect::<Vec<_>>()
    });
    let hit = picking::nearest(&ray, boxes).flatten();
    for _ in 0..count {
        if let Some(target) = &hit {
            cctv.0.click(target, clock.0);
        }
        if hit == Some(Target::SitIn) {
            match cctv.0.sit_in().cloned() {
                Some(desk) => commands.queue(move |world: &mut World| seat::sit_in(world, &desk)),
                None => eprintln!("[world] SIT IN with nobody flagged selected: nothing"),
            }
        }
        eprintln!(
            "[world] click on {}: selected {}",
            hit.as_ref().map_or("nothing".to_string(), label),
            cctv.0
                .selected()
                .map_or("nobody".to_string(), |desk| desk.agent.clone())
        );
    }
}

/// A target as the world's log names it: `row:<agent>`, `feed:<project>`, `focus:<n>`
/// (from 1, as the user counts), `cover`, `kill`, `work-harder` or `sit-in`.
pub fn label(target: &Target) -> String {
    match target {
        Target::Row(desk) => format!("row:{}", desk.agent),
        Target::Feed(project) => format!("feed:{project}"),
        Target::Focus(n) => format!("focus:{}", n + 1),
        Target::Cover => "cover".into(),
        Target::Kill => "kill".into(),
        Target::WorkHarder => "work-harder".into(),
        Target::SitIn => "sit-in".into(),
    }
}

/// For driving clicks in a demonstration, a debug build's `WORLD_DEBUG_CLICKABLES=1`:
/// about every half second, if anything changed, the eye and its look, and where each
/// clickable thing's box is in the world, so a drive can turn the dot onto it.
#[allow(clippy::too_many_arguments)]
fn debug_clickables(
    eye: Single<&Transform, (With<PlayerEye>, Without<PortalCamera>)>,
    look: Res<super::look::Look>,
    clickables: Query<(Entity, &Clickable)>,
    children: Query<&Children>,
    bounds: Query<(&Aabb, &GlobalTransform)>,
    mut on: Local<Option<bool>>,
    mut frames: Local<u32>,
    mut logged: Local<String>,
) {
    let on = *on.get_or_insert_with(|| {
        cfg!(debug_assertions) && std::env::var("WORLD_DEBUG_CLICKABLES").is_ok_and(|v| v == "1")
    });
    *frames += 1;
    if !on || !frames.is_multiple_of(30) {
        return;
    }
    let at = eye.translation;
    let mut lines = vec![format!(
        "[world] debug: aim eye ({:.3}, {:.3}, {:.3}) yaw {:.4} pitch {:.4}",
        at.x, at.y, at.z, look.yaw, look.pitch
    )];
    let mut targets: Vec<(String, Vec3)> = clickables
        .iter()
        .filter_map(|(entity, clickable)| {
            let target = clickable.0.as_ref()?;
            let centres: Vec<Vec3> = std::iter::once(entity)
                .chain(children.iter_descendants(entity))
                .filter_map(|part| bounds.get(part).ok())
                .map(|(aabb, at)| boxed(aabb, at).centre)
                .collect();
            (!centres.is_empty()).then(|| {
                let centre = centres.iter().sum::<Vec3>() / centres.len() as f32;
                (label(target), centre)
            })
        })
        .collect();
    targets.sort_by(|a, b| a.0.cmp(&b.0));
    lines.extend(targets.into_iter().map(|(name, c)| {
        format!(
            "[world] debug: clickable {name} at ({:.3}, {:.3}, {:.3})",
            c.x, c.y, c.z
        )
    }));
    let text = lines.join("\n");
    if *logged != text {
        eprintln!("{text}");
        *logged = text;
    }
}

/// A mesh's bounds as a box in the world, turned and scaled with it, padded.
fn boxed(aabb: &Aabb, at: &GlobalTransform) -> Boxed {
    let affine = at.affine();
    let mut axes = [Vec3::X, Vec3::Y, Vec3::Z];
    let mut half = [0.0; 3];
    for i in 0..3 {
        let column = Vec3::from(affine.matrix3.col(i));
        let length = column.length();
        if length > 0.0 {
            axes[i] = column / length;
        }
        half[i] = aabb.half_extents[i] * length + PAD;
    }
    Boxed {
        centre: affine.transform_point3a(aabb.center).into(),
        axes,
        half,
    }
}
