//! Doorways (sweatshop-rooms ticket 19; spec: "Doorways"; ADR-0011): each room's doorway,
//! and the CCTV doorway in the corridor's far wall, is a
//! two-way render-to-texture portal. From the corridor, the doorway shows the space behind
//! it; from inside, its doorway shows the corridor. `collision::doorways` says where each
//! one is and what carries the corridor through it.
//!
//! Every doorway has a portal camera ([`PortalCamera`]) and an opening on each side: one
//! in the corridor (layer 0) at the corridor wall's far face, and one behind it (its
//! space's layer) at that wall's corridor face. While the doorway is live, its camera
//! stands at the player's pose carried through the doorway (`Doorway::through`, a pure
//! translation), clips at the doorway plane with `near_clip_plane`, and renders into an
//! image the opening shows with [`PortalMaterial`], read at each fragment's pixel.
//!
//! **One doorway is live** ([`Live`]): from the corridor, the nearest opening in the main
//! camera's view, never one behind the player; inside a room or the CCTV room, its own
//! doorway while it is in view. The live doorway has all four knobs of ticket 10: its room's
//! shadows off, its camera cropped to the opening's on-screen rectangle (with the culling
//! fix [`crop_frusta`]), `Msaa::Off`, and a half-resolution image. Every other opening
//! shows a dim flat colour, and the lights of rooms that are neither live nor the
//! player's are removed ([`RoomLight`] keeps their settings), not hidden.
//!
//! The world's own camera systems (`engine`) leave portal cameras alone; these follow the
//! main camera's drawing on/off, pose, tonemapping, exposure and haze instead.

use std::collections::HashMap;

use bevy::asset::{embedded_asset, embedded_path, AssetPath};
use bevy::camera::primitives::Frustum;
use bevy::camera::visibility::{RenderLayers, VisibilitySystems};
use bevy::camera::{CameraProjection, Exposure, RenderTarget, SubCameraView, Viewport};
use bevy::core_pipeline::prepass::DepthPrepass;
use bevy::light::NotShadowCaster;
use bevy::math::{Affine3A, Rect};
use bevy::pbr::{Material, MaterialPlugin};
use bevy::prelude::*;
use bevy::render::extract_component::{ExtractComponent, ExtractComponentPlugin};
use bevy::render::render_resource::{AsBindGroup, TextureFormat};
use bevy::render::view::{Msaa, NoIndirectDrawing, Tonemapping};
use bevy::render::RenderApp;
use bevy::shader::ShaderRef;
use bevy::shape::ViewFrustum;
use bevy::window::WindowRef;

use super::collision::{self, Space};
use super::corridor::Plan;
use super::feeds::FeedCamera;
use super::floor_plan::{layout, FloorPlan};
use super::quality::ViewQuality;
use super::rooms::Detail;

/// The portal images are this many times smaller than the world view, each way.
const IMAGE_DIVISOR: u32 = 2;

/// The live doorway, if any: the space behind it (a room or the CCTV room), and whether
/// it is seen from inside that space (showing the corridor) rather than from the
/// corridor (showing the space).
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Live(pub Option<LiveDoorway>);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LiveDoorway {
    pub behind: Space,
    pub from_inside: bool,
}

impl Live {
    /// How many doorways are live: none or one.
    pub fn count(&self) -> usize {
        usize::from(self.0.is_some())
    }
}

/// Whether the world's cameras use Bevy's GPU-driven indirect drawing. Off by default:
/// on the owner's Intel iGPU (Mesa) it drops whole batches of meshes, the room's walls in
/// ticket 17 and the desks and robots seen through a doorway in ticket 19, so every
/// camera carries `NoIndirectDrawing`.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IndirectDrawing(pub bool);

/// Whether the room the player stands in has shadows. A room seen through its doorway
/// never has.
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq)]
pub struct RoomShadows(pub bool);

impl Default for RoomShadows {
    fn default() -> Self {
        Self(ROOM_SHADOWS)
    }
}

/// Shadows in the player's room: off. The room's tube lights were unshadowed already,
/// and shadowing them cost far beyond the budget (ticket 19's comments): a 12-desk room
/// has 5 point lights, 6 shadow faces each, and a 40-desk room 19.
const ROOM_SHADOWS: bool = false;

/// A camera other than the player's view: a doorway's portal camera, the warm-up camera,
/// or a camera feed's (`feeds`). The world's own camera systems leave each alone. The
/// marker reaches the render world too, where the first-frame check skips it.
#[derive(Component, ExtractComponent, Clone, Copy, Debug)]
#[extract_app(RenderApp)]
pub struct PortalCamera;

/// A light in a room or the CCTV room, with its settings: its `PointLight` is taken off
/// while its space is neither live nor the player's, and put back from these.
#[derive(Component, Clone, Debug)]
pub struct RoomLight {
    pub space: Space,
    pub light: PointLight,
}

/// Orders the portal cameras after everything that moves, turns or frames the main
/// camera in `Update`.
#[derive(SystemSet, Clone, Debug, PartialEq, Eq, Hash)]
pub struct FollowMain;

/// What an opening shows: the dim colour, or (while live) the portal image read at the
/// fragment's world view pixel.
#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
pub struct PortalMaterial {
    /// rgb: the dim colour, linear.
    #[uniform(0)]
    dim: Vec4,
    /// x: image pixels per world view pixel; y: 1 while live.
    #[uniform(1)]
    frame: Vec4,
    #[texture(2)]
    image: Handle<Image>,
}

impl Material for PortalMaterial {
    fn fragment_shader() -> ShaderRef {
        ShaderRef::Path(
            AssetPath::from_path_buf(embedded_path!("portal.wesl")).with_source("embedded"),
        )
    }
}

/// Which doorway: a room's, by its project id, or the CCTV room's.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Key {
    Project(String),
    Cctv,
}

/// One doorway: where it is, its camera, image and material, and its two openings.
struct Doorway {
    at: collision::Doorway,
    camera: Entity,
    image: Handle<Image>,
    size: UVec2,
    material: Handle<PortalMaterial>,
    corridor_side: Entity,
    inner_side: Entity,
    live: bool,
}

/// Every doorway, and the openings' shared mesh.
#[derive(Resource, Default)]
struct Doorways {
    by_key: HashMap<Key, Doorway>,
    opening: Option<Handle<Mesh>>,
}

/// The CCTV room's dim doorway colour: its cold cyan, dark.
const CCTV_DIM: Vec4 = Vec4::new(0.0, 0.012, 0.018, 1.0);

/// The warm-up camera: after each change of the plan, and whenever new meshes are
/// spawned (a room's pieces, a desk) or the world settings change (Textures swaps the
/// materials), it draws every room for a few frames, one room a frame, with the main
/// camera's settings and target, into a corner of the world view that the main camera
/// then clears and draws over. The pipelines the main camera needs
/// inside a room are then compiled before the player first walks in. A doorway's view
/// (MSAA off, an image's format) compiles others; without this, the first
/// crossing into a room drew it for a second or so without the meshes whose pipelines
/// were still compiling (the tube lights, the robots' antenna bulbs). It carries the
/// [`PortalCamera`] marker, so the main camera's systems leave it alone and
/// `match_main_view` gives it the main camera's tonemapping, exposure and haze.
#[derive(Resource, Default)]
struct WarmUp {
    camera: Option<Entity>,
    frames: usize,
    next: usize,
}

/// Frames each room is drawn by the warm-up camera after a change: pipelines compile in
/// the background over a few frames.
const WARM_FRAMES_PER_ROOM: usize = 20;
/// The warm-up view's size: only the pipelines matter.
const WARM_SIZE: u32 = 64;

/// Set when room lights were added, removed or changed, for the check in `PostUpdate`.
#[derive(Resource, Default)]
struct LightsChanged(bool);

pub struct PortalsPlugin;

impl Plugin for PortalsPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "portal.wesl");
        app.add_plugins((
            MaterialPlugin::<PortalMaterial>::default(),
            ExtractComponentPlugin::<PortalCamera>::default(),
        ))
        .init_resource::<Live>()
        .init_resource::<IndirectDrawing>()
        .init_resource::<RoomShadows>()
        .init_resource::<Doorways>()
        .init_resource::<LightsChanged>()
        .init_resource::<WarmUp>()
        .init_resource::<ViewQuality>()
        .add_systems(
            Update,
            (
                build_doorways,
                follow_space,
                follow_main,
                warm_up,
                match_main_view,
                apply_lights,
            )
                .chain()
                .in_set(FollowMain),
        )
        .add_systems(
            PostUpdate,
            (
                crop_frusta
                    .after(VisibilitySystems::UpdateFrusta)
                    .before(VisibilitySystems::CheckVisibility),
                check_lights,
            ),
        );
    }
}

/// Gives every room of the plan its doorway and the CCTV room its own, keeps each
/// corridor opening at its doorway as rooms come and go (the CCTV doorway snaps to the
/// corridor's middle), and takes a closed room's doorway away.
fn build_doorways(
    mut commands: Commands,
    plan: Res<Plan>,
    indirect: Res<IndirectDrawing>,
    mut doorways: ResMut<Doorways>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<PortalMaterial>>,
) {
    if !plan.is_changed() {
        return;
    }
    let plan = &plan.0;
    let doorways = &mut *doorways;
    // Each doorway of the plan by its key: the rooms' come first, in corridor order.
    let wanted: Vec<(Key, collision::Doorway)> = collision::doorways(plan)
        .into_iter()
        .map(|at| match at.behind {
            Space::Room(slot) => {
                let room = plan.rooms.iter().find(|r| r.slot == slot);
                let project = room.map_or_else(String::new, |r| r.project.clone());
                (Key::Project(project), at)
            }
            Space::Cctv | Space::Corridor => (Key::Cctv, at),
        })
        .collect();
    doorways.by_key.retain(|key, doorway| {
        let open = wanted
            .iter()
            .any(|(k, at)| k == key && at.behind == doorway.at.behind);
        if !open {
            for gone in [doorway.camera, doorway.corridor_side, doorway.inner_side] {
                commands.entity(gone).despawn();
            }
        }
        open
    });
    let opening = doorways
        .opening
        .get_or_insert_with(|| {
            meshes.add(Rectangle::new(
                layout::DOORWAY_WIDTH,
                layout::DOORWAY_HEIGHT,
            ))
        })
        .clone();
    // An opening's quad faces +Z as built: turned half round to face -Z.
    let side_at = |centre: [f32; 3], facing: f32| {
        let at = Transform::from_translation(Vec3::from_array(centre));
        if facing < 0.0 {
            at.with_rotation(Quat::from_rotation_y(std::f32::consts::PI))
        } else {
            at
        }
    };
    for (key, at) in wanted {
        let corridor_side = side_at(at.corridor_side, at.facing_corridor());
        if let Some(doorway) = doorways.by_key.get_mut(&key) {
            if doorway.at != at {
                if doorway.at.corridor_side != at.corridor_side {
                    commands.entity(doorway.corridor_side).insert(corridor_side);
                }
                doorway.at = at;
            }
            continue;
        }
        let image = images.add(target_image(UVec2::ONE));
        let dim = match &key {
            Key::Project(project) => dim_colour(project),
            Key::Cctv => CCTV_DIM,
        };
        let material = materials.add(PortalMaterial {
            dim,
            frame: Vec4::new(1.0 / IMAGE_DIVISOR as f32, 0.0, 0.0, 0.0),
            image: image.clone(),
        });
        let behind = at.behind.seen_layers();
        let mut camera = commands.spawn((
            Camera3d::default(),
            Camera {
                order: -1,
                is_active: false,
                ..default()
            },
            RenderTarget::Image(image.clone().into()),
            Projection::Perspective(PerspectiveProjection::default()),
            Msaa::Off,
            super::quality::clusters(),
            behind,
            PortalCamera,
        ));
        if !indirect.0 {
            camera.insert(NoIndirectDrawing);
        }
        let camera = camera.id();
        let side = |at: Transform, layer: usize| {
            (
                Mesh3d(opening.clone()),
                MeshMaterial3d(material.clone()),
                at,
                RenderLayers::from_layers(&[layer]),
                NotShadowCaster,
            )
        };
        let corridor_side = commands.spawn(side(corridor_side, 0)).id();
        // Facing into the space behind.
        let inner_side = side_at(at.inner_side, -at.facing_corridor());
        let inner_side = commands
            .spawn(side(inner_side, at.behind.render_layer()))
            .id();
        doorways.by_key.insert(
            key,
            Doorway {
                at,
                camera,
                image,
                size: UVec2::ONE,
                material,
                corridor_side,
                inner_side,
                live: false,
            },
        );
    }
}

/// Every opening the player in `space` may see live, by the space behind its doorway and
/// whether it is seen from inside: from the corridor, each doorway's corridor side; from
/// a room or the CCTV room, its own doorway's inner side.
fn openings(plan: &FloorPlan, space: Space) -> Vec<((Space, bool), Opening)> {
    collision::doorways(plan)
        .into_iter()
        .filter_map(|d| match space {
            Space::Corridor => Some((
                (d.behind, false),
                Opening {
                    centre: Vec3::from_array(d.corridor_side),
                    facing: Vec3::Z * d.facing_corridor(),
                },
            )),
            _ if d.behind == space => Some((
                (d.behind, true),
                Opening {
                    centre: Vec3::from_array(d.inner_side),
                    facing: Vec3::NEG_Z * d.facing_corridor(),
                },
            )),
            _ => None,
        })
        .collect()
}

/// The main camera sees only the space the player is in.
fn follow_space(
    mut commands: Commands,
    space: Res<Space>,
    main: Single<Entity, (With<Camera3d>, Without<PortalCamera>)>,
) {
    if !space.is_changed() {
        return;
    }
    let mut main = commands.entity(*main);
    main.insert(RenderLayers::from_layers(&[space.render_layer()]));
    match cctv_ambient(*space) {
        Some(ambient) => main.insert(ambient),
        None => main.remove::<AmbientLight>(),
    };
}

/// Picks the live doorway and puts its camera at the player's pose through the doorway,
/// clipped at the doorway and cropped to its opening on screen. Every other portal camera
/// is off, and every camera is off while the main camera does not draw.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn follow_main(
    mut commands: Commands,
    doorway_ambient: Query<(), (With<AmbientLight>, With<PortalCamera>)>,
    plan: Res<Plan>,
    space: Res<Space>,
    mut live: ResMut<Live>,
    mut doorways: ResMut<Doorways>,
    main: Single<(&Transform, &Camera, &Projection), (With<Camera3d>, Without<PortalCamera>)>,
    mut cameras: Query<
        (
            &mut Camera,
            &mut Transform,
            &mut Projection,
            &mut RenderLayers,
            &mut RenderTarget,
        ),
        With<PortalCamera>,
    >,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<PortalMaterial>>,
) {
    let (eye, main_camera, Projection::Perspective(main_projection)) = *main else {
        return;
    };
    let view = main_camera.viewport.as_ref().map(|v| v.physical_size);

    // Each portal image covers the world view at half its resolution.
    if let Some(view) = view {
        let size = view.map(|v| v.div_ceil(IMAGE_DIVISOR).max(1));
        for doorway in doorways.by_key.values_mut() {
            if doorway.size == size {
                continue;
            }
            doorway.size = size;
            doorway.image = images.add(target_image(size));
            if let Some(mut material) = materials.get_mut(&doorway.material) {
                material.image = doorway.image.clone();
            }
            if let Ok((_, _, _, _, mut target)) = cameras.get_mut(doorway.camera) {
                *target = RenderTarget::Image(doorway.image.clone().into());
            }
        }
    }

    // The live doorway: the nearest opening in view on the player's side.
    let mut chosen = None;
    if let Some(view) = view.filter(|v| v.x > 0 && v.y > 0) {
        let size = view.as_vec2();
        let clip_from_world = clip_from_view(main_projection, size) * view_from_world(eye);
        let openings = openings(&plan.0, *space);
        chosen = nearest_in_view(eye.translation, clip_from_world, size, openings)
            .map(|(key, rect)| (key, rect, view));
    }
    let want = Live(chosen.map(|((behind, from_inside), _, _)| LiveDoorway {
        behind,
        from_inside,
    }));
    if *live != want {
        let name = |behind: Space| match behind {
            Space::Room(slot) => {
                let project = plan
                    .0
                    .rooms
                    .iter()
                    .find(|r| r.slot == slot)
                    .map_or("?", |r| r.project.as_str());
                format!("{project} (slot {slot})")
            }
            other => other.describe(),
        };
        match want.0 {
            Some(LiveDoorway {
                behind,
                from_inside,
            }) => eprintln!(
                "[world] live doorway: {}, seen from {}",
                name(behind),
                if from_inside {
                    "inside"
                } else {
                    "the corridor"
                }
            ),
            None => eprintln!("[world] live doorway: none in view"),
        }
        *live = want;
    }

    for doorway in doorways.by_key.values_mut() {
        let Ok((mut camera, mut at, mut projection, mut layers, _)) =
            cameras.get_mut(doorway.camera)
        else {
            continue;
        };
        let this = chosen.filter(|((behind, _), _, _)| *behind == doorway.at.behind);
        let active = main_camera.is_active && this.is_some();
        if camera.is_active != active {
            camera.is_active = active;
        }
        if doorway.live != this.is_some() {
            doorway.live = this.is_some();
            if let Some(mut material) = materials.get_mut(&doorway.material) {
                material.frame.y = if doorway.live { 1.0 } else { 0.0 };
            }
        }
        let Some(((behind, from_inside), rect, view)) = this else {
            continue;
        };
        let through = Vec3::from_array(doorway.at.through);
        // The pose through the doorway, the space it sees, and the doorway plane there
        // (the far face of the opening, as seen from the player's side).
        let (offset, want_layers, plane) = if from_inside {
            let face = Vec3::from_array(doorway.at.inner_side) - through;
            (-through, Space::Corridor.seen_layers(), face)
        } else {
            let face = Vec3::from_array(doorway.at.corridor_side) + through;
            (through, behind.seen_layers(), face)
        };
        let pose = Transform {
            translation: eye.translation + offset,
            rotation: eye.rotation,
            scale: Vec3::ONE,
        };
        if *at != pose {
            *at = pose;
        }
        if *layers != want_layers {
            *layers = want_layers;
        }
        // The CCTV room's own dark ambient, seen through its doorway.
        let ambient = if from_inside {
            None
        } else {
            cctv_ambient(behind)
        };
        if ambient.is_some() != doorway_ambient.contains(doorway.camera) {
            match ambient {
                Some(ambient) => commands.entity(doorway.camera).insert(ambient),
                None => commands.entity(doorway.camera).remove::<AmbientLight>(),
            };
        }
        let want_projection = PerspectiveProjection {
            near_clip_plane: clip_plane(&pose, plane, Vec3::Z),
            aspect_ratio: match &*projection {
                Projection::Perspective(p) => p.aspect_ratio,
                _ => main_projection.aspect_ratio,
            },
            ..main_projection.clone()
        };
        if !matches!(&*projection, Projection::Perspective(p) if same_projection(p, &want_projection))
        {
            *projection = Projection::Perspective(want_projection);
        }
        let (viewport, sub_view) = crop(rect, view, doorway.size);
        if camera
            .viewport
            .as_ref()
            .map(|v| (v.physical_position, v.physical_size))
            != Some((viewport.physical_position, viewport.physical_size))
        {
            camera.viewport = Some(viewport);
        }
        if camera.sub_camera_view != Some(sub_view) {
            camera.sub_camera_view = Some(sub_view);
        }
    }
}

/// Draws each room in turn with the warm-up camera for a while after the plan changes,
/// from just inside its doorway, while the main camera draws ([`WarmUp`]).
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn warm_up(
    mut commands: Commands,
    plan: Res<Plan>,
    indirect: Res<IndirectDrawing>,
    mut warm: ResMut<WarmUp>,
    main: Single<(&Camera, &RenderTarget), (With<Camera3d>, Without<PortalCamera>)>,
    mut cameras: Query<
        (
            &mut Camera,
            &mut Transform,
            &mut RenderLayers,
            &mut RenderTarget,
            &mut Projection,
        ),
        With<PortalCamera>,
    >,
    added: Query<(), Added<Mesh3d>>,
    detail: Res<Detail>,
    quality: Res<ViewQuality>,
) {
    // Every room, then the CCTV room: each from just inside its doorway, looking in.
    let spaces: Vec<(Space, Transform)> = plan
        .0
        .rooms
        .iter()
        .map(|room| {
            let [x, _, z] = layout::interior_origin(room.slot);
            let at = Vec3::new(x + layout::ROOM_DOORWAY_X, layout::EYE_HEIGHT, z - 0.3);
            (Space::Room(room.slot), Transform::from_translation(at))
        })
        .chain(std::iter::once({
            let at = layout::cctv_to_world([0.0, layout::EYE_HEIGHT, -0.3]);
            let turned = Quat::from_rotation_y(std::f32::consts::PI);
            (
                Space::Cctv,
                Transform::from_translation(Vec3::from_array(at)).with_rotation(turned),
            )
        }))
        .collect();
    // New meshes (a room's pieces spawned, a desk added) or materials (Textures) may
    // need new pipelines.
    let changed = plan.is_changed() || !added.is_empty() || detail.is_changed();
    if changed {
        if warm.frames == 0 {
            eprintln!(
                "[world] warm-up: {} frames after a change",
                WARM_FRAMES_PER_ROOM * spaces.len()
            );
        }
        warm.frames = WARM_FRAMES_PER_ROOM * spaces.len();
    }
    let camera = *warm.camera.get_or_insert_with(|| {
        // The window, as the main camera's target: its format is part of each pipeline.
        let mut camera = commands.spawn((
            Camera3d::default(),
            Camera {
                order: -2,
                is_active: false,
                ..default()
            },
            RenderTarget::Window(WindowRef::Primary),
            // Wide enough to see the whole room, the ceiling's tubes too, from its doorway.
            Projection::Perspective(PerspectiveProjection {
                fov: 2.8,
                near: super::collision::NEAR,
                ..default()
            }),
            RenderLayers::none(),
            PortalCamera,
            // The main camera's, so it compiles the main camera's pipelines.
            quality.msaa,
            super::quality::clusters(),
        ));
        if quality.prepass {
            camera.insert(DepthPrepass);
        }
        if !indirect.0 {
            camera.insert(NoIndirectDrawing);
        }
        camera.id()
    });
    let (main, main_target) = *main;
    let Ok((mut camera, mut at, mut layers, target, projection)) = cameras.get_mut(camera) else {
        return;
    };
    let space = spaces.get(warm.next % spaces.len());
    let (Some(&(space, pose)), true, 1..) = (space, main.is_active, warm.frames) else {
        if camera.is_active {
            camera.is_active = false;
        }
        return;
    };
    warm.frames -= 1;
    warm.next += 1;
    // The main camera's target: the window, or its scaled image.
    share_target(target, projection, main_target);
    // A corner of the world view, which the main camera clears and draws over after.
    if let Some(view) = &main.viewport {
        let want = (
            view.physical_position,
            view.physical_size.min(UVec2::splat(WARM_SIZE)),
        );
        if camera
            .viewport
            .as_ref()
            .map(|v| (v.physical_position, v.physical_size))
            != Some(want)
        {
            camera.viewport = Some(Viewport {
                physical_position: want.0,
                physical_size: want.1,
                ..default()
            });
        }
    }
    *at = pose;
    *layers = RenderLayers::from_layers(&[space.render_layer()]);
    camera.is_active = true;
}

/// Points a camera at `want`, the target another camera already draws into (the warm-up
/// camera at the main camera's window or scaled image), unless it is there already; true
/// when it was. Bevy works out a camera's target size only when its target's image is
/// added or changed, its viewport changes or its projection does, never when the target
/// alone changes: a camera turned to an image added some frames before kept the old
/// image's size, and its depth prepass copied the main camera's depth at that size, which
/// wgpu refuses for a depth texture ("Partial copy … Depth32Float", owner-run-1), and the
/// world ended. Marking the projection changed makes Bevy size it afresh.
pub(super) fn share_target(
    mut target: Mut<RenderTarget>,
    mut projection: Mut<Projection>,
    want: &RenderTarget,
) -> bool {
    let same = match (&*target, want) {
        (RenderTarget::Image(a), RenderTarget::Image(b)) => a.handle == b.handle,
        (RenderTarget::Window(_), RenderTarget::Window(_)) => true,
        _ => false,
    };
    if !same {
        *target = want.clone();
        projection.set_changed();
    }
    same
}

/// The portal cameras' tonemapping, exposure and haze are the main camera's, so a
/// doorway's view matches the view around it. The opening's shader applies none of them.
/// Camera feeds take the tonemapping and exposure, never the haze.
#[allow(clippy::type_complexity)]
fn match_main_view(
    mut commands: Commands,
    main: Single<
        (Ref<Tonemapping>, Ref<Exposure>, Option<Ref<DistanceFog>>),
        (With<Camera3d>, Without<PortalCamera>),
    >,
    mut fog_removed: RemovedComponents<DistanceFog>,
    cameras: Query<(Entity, Has<FeedCamera>), With<PortalCamera>>,
    added: Query<(), Added<PortalCamera>>,
) {
    let (tonemapping, exposure, fog) = &*main;
    let removed = fog_removed.read().count() > 0;
    let changed = tonemapping.is_changed()
        || exposure.is_changed()
        || fog.as_ref().is_some_and(|f| f.is_changed())
        || removed
        || !added.is_empty();
    if !changed {
        return;
    }
    for (camera, feed) in &cameras {
        let mut camera = commands.entity(camera);
        camera.insert((**tonemapping, **exposure));
        // A camera feed has no haze: its picture is cheap and plain.
        match fog.as_ref().filter(|_| !feed) {
            Some(fog) => camera.insert((**fog).clone()),
            None => camera.remove::<DistanceFog>(),
        };
    }
}

/// Lights are on in the player's room (with its shadows, if [`RoomShadows`]) and in the
/// room seen through the live doorway (without shadows). While the CCTV room is seen,
/// every project room is fed to its monitor wall and keeps its lights, without
/// shadows. Every other room's lights are taken off.
fn apply_lights(
    mut commands: Commands,
    live: Res<Live>,
    space: Res<Space>,
    shadows: Res<RoomShadows>,
    lights: Query<(Entity, &RoomLight, Option<&PointLight>)>,
    mut changed: ResMut<LightsChanged>,
) {
    let fed = super::feeds::cctv_seen(*space, *live);
    let want = |behind: Space| {
        if *space == behind {
            Some(shadows.0)
        } else if live.0
            == Some(LiveDoorway {
                behind,
                from_inside: false,
            })
            || (fed && matches!(behind, Space::Room(_)))
        {
            Some(false)
        } else {
            None
        }
    };
    for (entity, room_light, light) in &lights {
        match (want(room_light.space), light) {
            (None, None) => {}
            (None, Some(_)) => {
                commands.entity(entity).remove::<PointLight>();
                changed.0 = true;
            }
            (Some(shadowed), light) => {
                if light.is_some_and(|l| l.shadow_maps_enabled == shadowed) {
                    continue;
                }
                commands.entity(entity).insert(PointLight {
                    shadow_maps_enabled: shadowed,
                    ..room_light.light
                });
                changed.0 = true;
            }
        }
    }
}

/// The light check: after room lights changed, logs how many lights each room has on.
fn check_lights(
    mut changed: ResMut<LightsChanged>,
    lights: Query<(&RoomLight, Option<&PointLight>)>,
) {
    if !std::mem::take(&mut changed.0) {
        return;
    }
    let mut rooms: Vec<(String, usize, usize, usize)> = Vec::new();
    for (room_light, light) in &lights {
        let name = match room_light.space {
            Space::Room(slot) => format!("slot {slot}"),
            other => other.describe(),
        };
        let at = match rooms.iter().position(|r| r.0 == name) {
            Some(at) => at,
            None => {
                rooms.push((name, 0, 0, 0));
                rooms.len() - 1
            }
        };
        rooms[at].1 += 1;
        if let Some(light) = light {
            rooms[at].2 += 1;
            rooms[at].3 += usize::from(light.shadow_maps_enabled);
        }
    }
    rooms.sort();
    let rooms: Vec<String> = rooms
        .iter()
        .map(|(name, all, on, shadowed)| {
            format!("{name} {on} of {all} PointLights ({shadowed} shadowed)")
        })
        .collect();
    eprintln!("[world] room lights: {}", rooms.join(", "));
}

/// Bevy builds a cropped camera's culling frustum from its projection with the cropped
/// viewport's aspect ratio, centred on its view axis; it ignores the sub-view, and so
/// culls meshes that are in the doorway. Each cropped portal camera culls with the
/// frustum of what it draws instead.
fn crop_frusta(
    mut cameras: Query<(&Camera, &Projection, &GlobalTransform, &mut Frustum), With<PortalCamera>>,
) {
    for (camera, projection, at, mut frustum) in &mut cameras {
        let (true, Some(sub_view)) = (camera.is_active, camera.sub_camera_view.as_ref()) else {
            continue;
        };
        let clip_from_world =
            projection.get_clip_from_view_for_sub(sub_view) * Mat4::from(at.affine().inverse());
        *frustum = Frustum(ViewFrustum::from_clip_from_world_custom_far(
            &clip_from_world,
            &at.translation(),
            &at.back(),
            projection.far(),
        ));
    }
}

/// The ambient light of what a camera sees in `space`, if not the world's: the CCTV
/// room's is dark and cold (`cctv`), the others' the corridor's greenish light.
fn cctv_ambient(space: Space) -> Option<AmbientLight> {
    (space == Space::Cctv).then(super::cctv::ambient)
}

/// A portal image of `size` pixels, in the world view's format.
fn target_image(size: UVec2) -> Image {
    Image::new_target_texture(size.x, size.y, TextureFormat::Bgra8UnormSrgb, None)
}

/// A room's dim doorway colour: a hue picked from its project's id, so a room keeps its
/// colour as it moves along the corridor.
fn dim_colour(project: &str) -> Vec4 {
    // FNV-1a: stable across runs and builds, unlike std's hasher.
    let hash = project
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
        });
    Color::hsl((hash % 360) as f32, 0.45, 0.12)
        .to_linear()
        .to_vec4()
}

fn same_projection(a: &PerspectiveProjection, b: &PerspectiveProjection) -> bool {
    (a.fov, a.near, a.far, a.near_clip_plane) == (b.fov, b.near, b.far, b.near_clip_plane)
}

/// The main camera's projection over a world view of `size` pixels.
fn clip_from_view(projection: &PerspectiveProjection, size: Vec2) -> Mat4 {
    PerspectiveProjection {
        aspect_ratio: size.x / size.y,
        ..projection.clone()
    }
    .get_clip_from_view()
}

fn view_from_world(eye: &Transform) -> Mat4 {
    Mat4::from(eye.compute_affine().inverse())
}

/// The portal camera's viewport into its image and its sub-view of the main camera's
/// frustum, for the opening's on-screen rectangle `rect` (world view pixels) in a world
/// view of `view` pixels and an image of `image` pixels, [`IMAGE_DIVISOR`] times smaller:
/// the rectangle with a pixel to spare, on whole image pixels, so each image pixel lands
/// where the opening reads it.
fn crop(rect: Rect, view: UVec2, image: UVec2) -> (Viewport, SubCameraView) {
    let per = IMAGE_DIVISOR as f32;
    let lo = ((rect.min / per).floor() - Vec2::ONE)
        .max(Vec2::ZERO)
        .as_uvec2()
        .min(image - UVec2::ONE);
    let hi = ((rect.max / per).ceil() + Vec2::ONE)
        .as_uvec2()
        .min(image)
        .max(lo + UVec2::ONE);
    let size = hi - lo;
    (
        Viewport {
            physical_position: lo,
            physical_size: size,
            ..default()
        },
        SubCameraView {
            full_size: view,
            offset: lo.as_vec2() * per,
            size: size * IMAGE_DIVISOR,
        },
    )
}

/// The near clip plane for a camera at `camera` that must not draw anything between it
/// and the plane through `point` with normal `normal` (either sign): the plane in view
/// space, its normal pointing away from the camera, and minus its distance.
pub fn clip_plane(camera: &Transform, point: Vec3, normal: Vec3) -> Vec4 {
    let normal = if normal.dot(point - camera.translation) < 0.0 {
        -normal
    } else {
        normal
    };
    let view_from_world: Affine3A = camera.compute_affine().inverse();
    let n = view_from_world.transform_vector3(normal).normalize();
    let q = view_from_world.transform_point3(point);
    n.extend(-n.dot(q))
}

/// A doorway opening: its middle, and the side it is seen from. Openings stand upright
/// across the corridor's length (in an XY plane), the doorway's size.
#[derive(Clone, Copy, Debug)]
pub struct Opening {
    pub centre: Vec3,
    pub facing: Vec3,
}

impl Opening {
    /// A corridor-side opening of the doorway wall, seen from the corridor (+Z).
    #[cfg(test)]
    pub fn facing_corridor(centre: Vec3) -> Self {
        Self {
            centre,
            facing: Vec3::Z,
        }
    }

    /// A room-side opening of a room's doorway wall, seen from inside the room (-Z).
    #[cfg(test)]
    pub fn facing_room(centre: Vec3) -> Self {
        Self {
            centre,
            facing: Vec3::NEG_Z,
        }
    }

    fn half(&self) -> Vec2 {
        Vec2::new(layout::DOORWAY_WIDTH, layout::DOORWAY_HEIGHT) / 2.0
    }

    pub fn corners(&self) -> [Vec3; 4] {
        let h = self.half();
        [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
            .map(|(x, y)| self.centre + Vec3::new(x * h.x, y * h.y, 0.0))
    }

    /// How far `eye` is from the nearest point of the opening.
    pub fn distance(&self, eye: Vec3) -> f32 {
        let h = self.half();
        let nearest = Vec3::new(
            eye.x.clamp(self.centre.x - h.x, self.centre.x + h.x),
            eye.y.clamp(self.centre.y - h.y, self.centre.y + h.y),
            self.centre.z,
        );
        eye.distance(nearest)
    }
}

/// Where the quad with `corners` shows in a view of `size` pixels (y down) through
/// `clip_from_world`, cut to the view: the part in front of the camera only. None when
/// none of it is in view.
pub fn screen_rect(clip_from_world: Mat4, corners: [Vec3; 4], size: Vec2) -> Option<Rect> {
    const FRONT: f32 = 1e-3;
    let clip = corners.map(|c| clip_from_world * c.extend(1.0));
    let mut front = Vec::with_capacity(8);
    for (i, &a) in clip.iter().enumerate() {
        let b = clip[(i + 1) % clip.len()];
        if a.w > FRONT {
            front.push(a);
        }
        if (a.w > FRONT) != (b.w > FRONT) {
            front.push(a + (b - a) * ((FRONT - a.w) / (b.w - a.w)));
        }
    }
    let mut lo = Vec2::MAX;
    let mut hi = Vec2::MIN;
    for p in &front {
        let ndc = p.truncate().truncate() / p.w;
        let pixel = Vec2::new((ndc.x + 1.0) / 2.0 * size.x, (1.0 - ndc.y) / 2.0 * size.y);
        lo = lo.min(pixel);
        hi = hi.max(pixel);
    }
    let (lo, hi) = (lo.max(Vec2::ZERO), hi.min(size));
    (!front.is_empty() && lo.x < hi.x && lo.y < hi.y).then(|| Rect::from_corners(lo, hi))
}

/// The live doorway's choice: of `openings`, the one nearest `eye` that shows in the view
/// (`clip_from_world`, `size` pixels) and is seen from its own side, with where it shows.
/// An opening behind the player or edge-on is never chosen.
pub fn nearest_in_view<K>(
    eye: Vec3,
    clip_from_world: Mat4,
    size: Vec2,
    openings: impl IntoIterator<Item = (K, Opening)>,
) -> Option<(K, Rect)> {
    openings
        .into_iter()
        .filter(|(_, o)| o.facing.dot(eye - o.centre) > 0.0)
        .filter_map(|(key, o)| {
            let rect = screen_rect(clip_from_world, o.corners(), size)?;
            Some((o.distance(eye), key, rect))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, key, rect)| (key, rect))
}

#[cfg(test)]
mod tests {
    use super::super::floor_plan::{FloorPlan, Room};
    use super::*;

    const VIEW: Vec2 = Vec2::new(1600.0, 900.0);

    fn eye(at: [f32; 3], yaw: f32) -> Transform {
        Transform::from_translation(Vec3::from_array(at)).with_rotation(Quat::from_euler(
            EulerRot::YXZ,
            yaw,
            0.0,
            0.0,
        ))
    }

    fn clip_from_world(eye: &Transform) -> Mat4 {
        clip_from_view(&PerspectiveProjection::default(), VIEW) * view_from_world(eye)
    }

    fn corridor(n: usize) -> Opening {
        Opening::facing_corridor(Vec3::from_array(layout::corridor_doorway(n)))
    }

    /// The live doorway for a player at `eye` in a corridor of `doorways` doorways.
    fn live(eye: &Transform, doorways: usize) -> Option<usize> {
        let openings = (0..doorways).map(|n| (n, corridor(n)));
        nearest_in_view(eye.translation, clip_from_world(eye), VIEW, openings).map(|(n, _)| n)
    }

    fn close(a: Vec4, b: Vec4) -> bool {
        (a - b).abs().max_element() < 1e-4
    }

    #[test]
    fn the_clip_plane_is_the_doorway_in_view_space_facing_away() {
        let start = eye(layout::start_eye(), 0.0);
        let doorway = Vec3::from_array(layout::corridor_doorway(0));
        // Straight ahead, 2.1 + 0.2 m away, whichever way the normal is given.
        let ahead = Vec4::new(0.0, 0.0, -1.0, -2.3);
        assert!(close(clip_plane(&start, doorway, Vec3::Z), ahead));
        assert!(close(clip_plane(&start, doorway, Vec3::NEG_Z), ahead));
        // Turned a quarter left, the doorway is to the camera's right (+X in view).
        let turned = eye(layout::start_eye(), std::f32::consts::FRAC_PI_2);
        assert!(close(
            clip_plane(&turned, doorway, Vec3::Z),
            Vec4::new(1.0, 0.0, 0.0, -2.3)
        ));
    }

    #[test]
    fn a_portal_camera_through_the_doorway_sees_the_doorway_where_the_player_does() {
        let through = Vec3::from_array(layout::through_doorway(3, 1));
        let player = eye([3.0, 1.6, 1.4], 0.4);
        let doorway = Vec3::from_array(layout::corridor_doorway(1));
        let portal = Transform {
            translation: player.translation + through,
            ..player
        };
        assert!(close(
            clip_plane(&portal, doorway + through, Vec3::Z),
            clip_plane(&player, doorway, Vec3::Z)
        ));
        // And it draws the room's doorway where the player sees the corridor's.
        let a = screen_rect(clip_from_world(&player), corridor(1).corners(), VIEW).unwrap();
        let moved = corridor(1).corners().map(|c| c + through);
        let b = screen_rect(clip_from_world(&portal), moved, VIEW).unwrap();
        assert!((a.min - b.min).abs().max_element() < 0.5, "{a:?} {b:?}");
        assert!((a.max - b.max).abs().max_element() < 0.5, "{a:?} {b:?}");
        // Coming out of the room is the same translation the other way.
        let inside = eye([-3.5 + 150.0, 1.6, -100.6], std::f32::consts::PI);
        let out = Transform {
            translation: inside.translation - through,
            ..inside
        };
        let room = Vec3::from_array(layout::room_doorway(3));
        assert!(close(
            clip_plane(&out, room - through, Vec3::Z),
            clip_plane(&inside, room, Vec3::Z)
        ));
    }

    #[test]
    fn a_doorway_straight_ahead_shows_in_the_middle_of_the_view() {
        let start = eye(layout::start_eye(), 0.0);
        let rect = screen_rect(clip_from_world(&start), corridor(0).corners(), VIEW).unwrap();
        assert!((rect.center().x - VIEW.x / 2.0).abs() < 0.5, "{rect:?}");
        assert!(rect.width() > 100.0 && rect.height() > 100.0 && rect.height() < VIEW.y);
    }

    #[test]
    fn a_doorway_behind_the_player_shows_nowhere_and_one_alongside_is_cut_to_the_view() {
        let facing_away = eye(layout::start_eye(), std::f32::consts::PI);
        assert_eq!(
            screen_rect(clip_from_world(&facing_away), corridor(0).corners(), VIEW),
            None
        );
        // Standing beside doorway 0, looking along the corridor: part of it is behind.
        let along = eye([0.6, 1.6, 0.3], -std::f32::consts::FRAC_PI_2);
        let rect = screen_rect(clip_from_world(&along), corridor(0).corners(), VIEW);
        let rect = rect.expect("partly in view");
        assert!(rect.min.x >= 0.0 && rect.max.x <= VIEW.x && rect.min.y >= 0.0);
    }

    #[test]
    fn the_live_doorway_is_the_nearest_in_view() {
        // At the start pose, facing the first doorway.
        assert_eq!(live(&eye(layout::start_eye(), 0.0), 6), Some(0));
        // Opposite doorway 2, it is the nearest.
        let at_two = [layout::doorway_x(2), 1.6, 1.2];
        assert_eq!(live(&eye(at_two, 0.0), 6), Some(2));
        // Turned to look down the corridor towards its far end (+X), doorway 2 is
        // behind on the left: the nearest one ahead takes its place.
        assert_eq!(live(&eye(at_two, -1.3), 6), Some(3));
        // And looking back towards the start end, doorway 1.
        assert_eq!(live(&eye(at_two, 1.3), 6), Some(1));
    }

    #[test]
    fn no_doorway_is_live_with_none_in_view() {
        // Facing the far wall.
        assert_eq!(
            live(&eye(layout::start_eye(), std::f32::consts::PI), 6),
            None
        );
        // Looking straight up.
        let up = Transform::from_translation(Vec3::from_array(layout::start_eye()))
            .with_rotation(Quat::from_euler(EulerRot::YXZ, 0.0, 1.4, 0.0));
        assert_eq!(live(&up, 6), None);
        // A doorway seen from behind (from inside its wall) is not live.
        let behind = eye([layout::doorway_x(0), 1.6, -1.0], 0.0);
        assert_eq!(live(&behind, 1), None);
        assert_eq!(live(&eye(layout::start_eye(), 0.0), 0), None);
    }

    #[test]
    fn inside_a_room_its_doorway_is_live_while_in_view() {
        let room = Opening::facing_room(Vec3::from_array(layout::room_doorway(1)));
        let [x, _, z] = layout::interior_origin(1);
        let pick = |eye: &Transform| {
            nearest_in_view(eye.translation, clip_from_world(eye), VIEW, [(1, room)])
                .map(|(slot, _)| slot)
        };
        // Facing the doorway from the second row (the room's doorway is towards +Z).
        let back = eye([x - 2.0, 1.6, z - 4.0], std::f32::consts::PI);
        assert_eq!(pick(&back), Some(1));
        // Facing the back wall.
        assert_eq!(pick(&eye([x - 2.0, 1.6, z - 4.0], 0.0)), None);
    }

    fn plan(rooms: usize) -> FloorPlan {
        FloorPlan {
            rooms: (0..rooms)
                .map(|slot| Room {
                    project: format!("-p{slot}"),
                    name: format!("p{slot}"),
                    slot,
                    rows: layout::MIN_ROWS,
                    desks: Vec::new(),
                })
                .collect(),
        }
    }

    /// The live doorway for a player at `eye` in `space`, by the space behind it and
    /// whether it is seen from inside.
    fn live_in(plan: &FloorPlan, space: Space, eye: &Transform) -> Option<(Space, bool)> {
        let openings = openings(plan, space);
        nearest_in_view(eye.translation, clip_from_world(eye), VIEW, openings).map(|(k, _)| k)
    }

    #[test]
    fn the_cctv_doorway_is_live_when_nearest_in_view_and_from_inside_its_room() {
        let one = plan(1);
        let start = layout::start_eye();
        let corridor = |yaw: f32| live_in(&one, Space::Corridor, &eye(start, yaw));
        // At the start pose, facing the first doorway; turned round, the CCTV doorway
        // right behind the player in the far wall.
        assert_eq!(corridor(0.0), Some((Space::Room(0), false)));
        assert_eq!(corridor(std::f32::consts::PI), Some((Space::Cctv, false)));
        // In a corridor of five, from the middle, it is live when it is the nearest in
        // view: across the corridor, in front of it.
        let five = plan(5);
        let middle = [layout::cctv_doorway_x(&five), 1.6, 0.6];
        let across = live_in(&five, Space::Corridor, &eye(middle, std::f32::consts::PI));
        assert_eq!(across, Some((Space::Cctv, false)));
        // Inside the CCTV room, facing its doorway (room +Z is world -Z): its own
        // doorway, seen from inside; facing the monitor wall, none.
        let [x, y, z] = layout::cctv_to_world([1.0, 1.6, -3.0]);
        let inside = |yaw: f32| live_in(&five, Space::Cctv, &eye([x, y, z], yaw));
        assert_eq!(inside(0.0), Some((Space::Cctv, true)));
        assert_eq!(inside(std::f32::consts::PI), None);
        // Inside a project room, only its own doorway.
        let [x, _, z] = layout::interior_origin(2);
        let back = eye([x - 3.5, 1.6, z - 3.0], std::f32::consts::PI);
        assert_eq!(
            live_in(&five, Space::Room(2), &back),
            Some((Space::Room(2), true))
        );
    }

    #[test]
    fn the_crop_covers_the_opening_on_whole_half_resolution_pixels() {
        let view = UVec2::new(1601, 901);
        let image = UVec2::new(801, 451);
        let rect = Rect::new(300.4, 100.2, 701.0, 899.9);
        let (viewport, sub) = crop(rect, view, image);
        assert_eq!(viewport.physical_position, UVec2::new(149, 49));
        assert_eq!(viewport.physical_size, UVec2::new(352 - 149, 451 - 49));
        // The sub-view is the same rectangle in world view pixels, of the whole view.
        assert_eq!(sub.full_size, view);
        assert_eq!(sub.offset, Vec2::new(298.0, 98.0));
        assert_eq!(sub.size, UVec2::new(2 * 203, 2 * 402));
        // A rectangle at the view's edge stays inside the image.
        let (edge, _) = crop(Rect::new(0.0, 0.0, 1601.0, 901.0), view, image);
        assert_eq!(
            (edge.physical_position, edge.physical_size),
            (UVec2::ZERO, image)
        );
    }

    /// Bevy's own camera sizing (`camera_system`, in `PostUpdate` before the assets'
    /// events as in the engine), in an app of its own without a GPU.
    fn sizing() -> App {
        use bevy::asset::AssetEventSystems;
        use bevy::render::camera::camera_system;
        use bevy::render::texture::ManualTextureViews;
        use bevy::window::{WindowCreated, WindowResized, WindowScaleFactorChanged};
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default()))
            .init_asset::<Image>()
            .init_resource::<ManualTextureViews>()
            .add_message::<WindowResized>()
            .add_message::<WindowCreated>()
            .add_message::<WindowScaleFactorChanged>()
            .add_systems(PostUpdate, camera_system.before(AssetEventSystems));
        app
    }

    fn target(app: &mut App, width: u32, height: u32) -> RenderTarget {
        let image = Image::new_target_texture(width, height, TextureFormat::Bgra8UnormSrgb, None);
        let image = app.world_mut().resource_mut::<Assets<Image>>().add(image);
        RenderTarget::Image(image.into())
    }

    fn size(app: &App, camera: Entity) -> Option<UVec2> {
        app.world().get::<Camera>(camera)?.physical_target_size()
    }

    #[test]
    fn a_camera_turned_to_an_image_another_camera_drew_into_for_frames_takes_its_size() {
        // The owner's first world start: the window grew, the main camera turned to a
        // new, bigger scaled image, and only frames later did the warm-up camera follow it.
        let mut app = sizing();
        let small = target(&mut app, 660, 522);
        let warm = app
            .world_mut()
            .spawn((Camera3d::default(), Camera::default(), small))
            .id();
        app.update();
        assert_eq!(size(&app, warm), Some(UVec2::new(660, 522)));
        let big = target(&mut app, 1152, 708);
        for _ in 0..3 {
            app.update();
        }
        let mut camera = app.world_mut().entity_mut(warm);
        let (target, projection) = camera
            .get_components_mut::<(&mut RenderTarget, &mut Projection)>()
            .expect("a camera");
        assert!(!share_target(target, projection, &big));
        app.update();
        assert_eq!(size(&app, warm), Some(UVec2::new(1152, 708)));
        // Already there, it is left alone.
        let mut camera = app.world_mut().entity_mut(warm);
        let (target, projection) = camera
            .get_components_mut::<(&mut RenderTarget, &mut Projection)>()
            .expect("a camera");
        assert!(share_target(target, projection, &big));
    }
}
