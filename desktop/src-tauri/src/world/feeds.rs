//! Live camera feeds on the CCTV room's monitor wall (management-rooms ticket 15; spec:
//! "Camera feeds"). The CCTV state machine (`cctv_state`) says which rooms are on the
//! wall, where, and which feeds refresh each frame; this draws it.
//!
//! - **A wall camera in every project room:** the `wall_camera` piece hangs inside over
//!   the doorway, its head turned to the room's framing (`layout::wall_camera`), which
//!   snaps to a new one when the room grows or shrinks.
//! - **A feed camera per room** ([`FeedCamera`], also a [`PortalCamera`] so the main
//!   camera's systems leave it alone): a `Camera3d` at the lens, rendering only its room's
//!   layer into a 320 × 180 image of its own (`order: -1`, `Msaa::Off`, no haze).
//! - **The monitor wall:** one `cctv_feed_monitor` per feed, laid out by the state machine,
//!   its screen drawn by [`MonitorMaterial`] (`monitor.wesl`): the feed, monochrome cyan,
//!   with scanlines and grain on the world's clock, the room's name, its CAM number (from
//!   `lettering`) and a recording dot. With no open project, one NO SIGNAL screen.
//! - **Cheap on purpose:** every feed camera is off unless the CCTV room is seen (the
//!   player is in it, or sees into it through its live doorway) while the world draws;
//!   then only the state machine's turn of feeds draws each frame (one: a feed of a full
//!   room costs about 5 ms on the owner's iGPU; every other frame while the room is seen
//!   only through its doorway; and no feed more often than every fourth frame, so a short
//!   wall rests between turns), and the others keep their last image. Fed rooms' lights
//!   are on without shadows meanwhile (`portals`).
//! - **Flagged feeds**: a feed whose room has a flagged agent gets the
//!   `cctv_feed_flag` frame with its tag bar, glowing in the flag's colour ([`FlagTint`],
//!   red over amber as the state machine says), the flag's words on the tag, and its
//!   picture drawn in that colour with a wash of it.

use std::collections::HashMap;

use agents::FlagKind;
use bevy::app::Propagate;
use bevy::asset::{embedded_asset, embedded_path, AssetPath};
use bevy::camera::visibility::RenderLayers;
use bevy::camera::RenderTarget;
use bevy::gltf::GltfMaterialName;
use bevy::light::NotShadowCaster;
use bevy::pbr::{Material, MaterialPlugin};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, TextureFormat};
use bevy::render::view::{Msaa, NoIndirectDrawing};
use bevy::shader::ShaderRef;

use super::aim::Clickable;
use super::cctv_geometry;
use super::cctv_state::{self, CctvState, Target, Wall};
use super::collision::Space;
use super::corridor::Plan;
use super::floor_plan::layout::{self, WallCamera};
use super::lettering::{Lettered, Lettering, Overlay, Wording};
use super::marker::Marker;
use super::pieces::Kit;
use super::portals::{FollowMain, IndirectDrawing, Live, LiveDoorway, PortalCamera};
use super::strings::{self, WorldStrings};
use super::surfaces::{Bright, SwapMaterials};

/// A wall feed's image, in pixels.
const FEED_IMAGE: UVec2 = UVec2::new(320, 180);
/// A feed screen's size (`cctv_monitors.py`'s `cctv_feed_screen`), and its face in front
/// of its monitor's back, with the picture a millimetre in front of it.
const SCREEN: Vec2 = Vec2::new(1.2, 0.675);
const SCREEN_FRONT: f32 = 0.073;
/// How far a feed camera sees: the deepest room's far corner, with room to spare.
const FEED_FAR: f32 = 60.0;

/// A camera feed's camera. It also carries [`PortalCamera`], so the world's own camera
/// systems (rect, look, drawing on/off, the first-frame count) leave it alone.
#[derive(Component, Clone, Copy, Debug)]
pub struct FeedCamera;

/// The CCTV room's state machine, fed every new floor plan.
#[derive(Resource, Default)]
pub struct Cctv(pub CctvState);

/// For measuring what the feeds cost, a debug build's `WORLD_DEBUG_FEEDS=<n>|off`: how
/// many wall feeds refresh each frame (`off` is none: the wall keeps black screens).
fn debug_feeds_per_frame() -> Option<usize> {
    if !cfg!(debug_assertions) {
        return None;
    }
    match std::env::var("WORLD_DEBUG_FEEDS").ok()?.as_str() {
        "off" => Some(0),
        n => n.parse().ok(),
    }
}

/// Where the feed cameras are turned on or off for this frame, after `portals` has
/// worked out which doorway is live.
#[derive(SystemSet, Clone, Debug, PartialEq, Eq, Hash)]
pub struct RefreshFeeds;

/// For measuring, a debug build's `WORLD_DEBUG_DOORWAY_EVERY=<n>`: through the CCTV
/// doorway, the wall's turns go every n frames (1 is every frame, as inside).
fn debug_doorway_every() -> Option<usize> {
    if !cfg!(debug_assertions) {
        return None;
    }
    std::env::var("WORLD_DEBUG_DOORWAY_EVERY")
        .ok()?
        .parse()
        .ok()
}

/// How many feed cameras draw this frame, for the world's log.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FeedsDrawing(pub usize);

/// What a monitor's screen shows: a feed, tinted and overlaid (`monitor.wesl`).
#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
pub struct MonitorMaterial {
    /// x: 1 once the feed has drawn; y: the feed's grain seed.
    #[uniform(0)]
    params: Vec4,
    #[texture(1)]
    #[sampler(2)]
    feed: Handle<Image>,
    #[texture(3)]
    #[sampler(4)]
    name: Option<Handle<Image>>,
    #[texture(5)]
    #[sampler(6)]
    cam: Option<Handle<Image>>,
    /// rgb: the flag's colour; a: 1 while the feed is flagged.
    #[uniform(7)]
    flag: Vec4,
}

/// A flag's colour as the CCTV room's screens show it, linear: the markers' own red and
/// amber (`marker`), which the tone mapping keeps red and amber.
pub fn flag_colour(kind: FlagKind) -> Vec3 {
    Vec3::from_array(Marker { kind }.colour())
}

/// A screen's flag uniform: the flag's colour, and whether there is one.
pub fn flag_uniform(flag: Option<FlagKind>) -> Vec4 {
    flag.map_or(Vec4::ZERO, |kind| flag_colour(kind).extend(1.0))
}

/// A flag frame piece (`cctv_feed_flag`, `cctv_focus_flag`): its glow takes the flag's
/// colour and its tag bar the flag's words, once its scene spawns.
#[derive(Component, Clone, Copy, Debug)]
pub struct FlagTint(pub FlagKind);

/// How strongly a flag frame glows: like the markers', low enough that the tone mapping
/// keeps red red.
const FRAME_GLOW: f32 = 2.0;
/// The tag bars' text markers (`cctv_monitors.py`), and how wide the words are on each.
const FEED_TAG: (&str, f32) = ("cctv_feed_flag_text", 0.86);
const FOCUS_TAG: (&str, f32) = ("cctv_focus_flag_text", 1.1);

impl Material for MonitorMaterial {
    // A screen is a glowing quad that casts no shadow and needs no depth prepass: no
    // pipelines for either, so the world's first frame waits for fewer to compile.
    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }

    fn fragment_shader() -> ShaderRef {
        ShaderRef::Path(
            AssetPath::from_path_buf(embedded_path!("monitor.wesl")).with_source("embedded"),
        )
    }
}

/// One open room's feed: its wall camera, its feed camera and image, and its screen's
/// material, with what that material's overlays say.
struct Feed {
    slot: usize,
    framing: WallCamera,
    rig: Entity,
    head: Option<Entity>,
    camera: Entity,
    material: Handle<MonitorMaterial>,
    drawn: bool,
    lettered: Option<(String, String)>,
}

/// Every open room's feed by project, and the wall as last hung.
#[derive(Resource, Default)]
struct Feeds {
    by_project: HashMap<String, Feed>,
    hung: Option<Wall>,
    wall_root: Option<Entity>,
    monitors: Vec<Entity>,
    screen: Option<Handle<Mesh>>,
    /// Each flagged feed's frame, by project: its piece, its flag and where it hangs.
    frames: HashMap<String, (Entity, FlagKind, cctv_geometry::Screen)>,
}

/// A wall camera piece, by its room's project.
#[derive(Component)]
struct Rig(String);

pub struct FeedsPlugin;

impl Plugin for FeedsPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "monitor.wesl");
        app.add_plugins(MaterialPlugin::<MonitorMaterial>::default())
            .insert_resource({
                let mut cctv = Cctv::default();
                if let Some(count) = debug_feeds_per_frame() {
                    cctv.0.set_refresh_per_frame(count);
                    eprintln!("[world] debug: {count} feeds refresh each frame");
                }
                if let Some(every) = debug_doorway_every() {
                    cctv.0.set_doorway_every(every);
                    eprintln!(
                        "[world] debug: through the doorway, feeds refresh every {every} frames"
                    );
                }
                cctv
            })
            .init_resource::<Feeds>()
            .init_resource::<FeedsDrawing>()
            .add_systems(
                Update,
                (
                    (follow_plan, turn_heads, letter_feeds, hang_wall, flag_feeds).chain(),
                    refresh_feeds.after(FollowMain).in_set(RefreshFeeds),
                ),
            )
            .add_systems(PostUpdate, (tint_frames, letter_tags).before(SwapMaterials));
    }
}

/// Gives the state machine each new plan, and every open room its wall camera and feed;
/// a room that grew or shrank gets its new framing, and a closed room's feed goes.
#[allow(clippy::too_many_arguments)]
fn follow_plan(
    mut commands: Commands,
    kit: Option<Res<Kit>>,
    plan: Res<Plan>,
    indirect: Res<IndirectDrawing>,
    mut cctv: ResMut<Cctv>,
    mut feeds: ResMut<Feeds>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<MonitorMaterial>>,
    mut transforms: Query<&mut Transform>,
    mut projections: Query<&mut Projection>,
) {
    let Some(kit) = kit else { return };
    if !plan.is_changed() && !kit.is_added() {
        return;
    }
    cctv.0.plan(&plan.0);
    feeds.by_project.retain(|project, feed| {
        let open = plan
            .0
            .rooms
            .iter()
            .any(|room| &room.project == project && room.slot == feed.slot);
        if !open {
            commands.entity(feed.rig).despawn();
            commands.entity(feed.camera).despawn();
        }
        open
    });
    for room in &plan.0.rooms {
        let framing = layout::wall_camera(room.rows);
        let origin = Vec3::from_array(layout::interior_origin(room.slot));
        if let Some(feed) = feeds.by_project.get_mut(&room.project) {
            if feed.framing != framing {
                feed.framing = framing;
                if let Ok(mut at) = transforms.get_mut(feed.camera) {
                    *at = camera_pose(origin, framing);
                }
                if let Ok(mut projection) = projections.get_mut(feed.camera) {
                    *projection = projection_of(framing);
                }
                if let Some(head) = feed.head {
                    if let Ok(mut at) = transforms.get_mut(head) {
                        at.rotation = turn(framing);
                    }
                }
                eprintln!(
                    "[world] wall camera of {} reframed for {} rows: fov {:.0}°",
                    room.project,
                    room.rows,
                    framing.fov.to_degrees()
                );
            }
            continue;
        }
        let layer = RenderLayers::from_layers(&layout::seen_layers(room.slot));
        let rig = commands
            .spawn((
                Transform::from_translation(origin),
                Visibility::default(),
                Propagate(layer.clone()),
            ))
            .id();
        commands.spawn((
            kit.piece("wall_camera"),
            Rig(room.project.clone()),
            Transform::from_translation(Vec3::from_array(layout::WALL_CAMERA_MOUNT)),
            ChildOf(rig),
        ));
        let image = images.add(Image::new_target_texture(
            FEED_IMAGE.x,
            FEED_IMAGE.y,
            TextureFormat::Bgra8UnormSrgb,
            None,
        ));
        let mut camera = commands.spawn((
            Camera3d::default(),
            Camera {
                order: -1,
                is_active: false,
                ..default()
            },
            RenderTarget::Image(image.clone().into()),
            projection_of(framing),
            camera_pose(origin, framing),
            Msaa::Off,
            super::quality::clusters(),
            layer,
            PortalCamera,
            FeedCamera,
        ));
        if !indirect.0 {
            camera.insert(NoIndirectDrawing);
        }
        let camera = camera.id();
        let material = materials.add(MonitorMaterial {
            params: Vec4::new(0.0, room.slot as f32 * 7.31, 0.0, 0.0),
            feed: image,
            name: None,
            cam: None,
            flag: Vec4::ZERO,
        });
        feeds.by_project.insert(
            room.project.clone(),
            Feed {
                slot: room.slot,
                framing,
                rig,
                head: None,
                camera,
                material,
                drawn: false,
                lettered: None,
            },
        );
        eprintln!(
            "[world] wall camera and feed for {} (slot {}), fov {:.0}°",
            room.project,
            room.slot,
            framing.fov.to_degrees()
        );
    }
}

/// The wall camera's head's node (`wall_camera.py`).
const HEAD: &str = "wall_camera_head";

/// The head's turn, and the feed camera's: `framing`'s yaw, then its pitch.
fn turn(framing: WallCamera) -> Quat {
    Quat::from_euler(EulerRot::YXZ, framing.yaw, framing.pitch, 0.0)
}

/// The feed camera at the lens of the wall camera in the room at `origin`.
fn camera_pose(origin: Vec3, framing: WallCamera) -> Transform {
    Transform::from_translation(origin + Vec3::from_array(framing.lens))
        .with_rotation(turn(framing))
}

fn projection_of(framing: WallCamera) -> Projection {
    Projection::Perspective(PerspectiveProjection {
        fov: framing.fov,
        near: super::collision::NEAR,
        far: FEED_FAR,
        ..default()
    })
}

/// Turns each wall camera's head to its room's framing once its piece is spawned.
fn turn_heads(
    mut feeds: ResMut<Feeds>,
    heads: Query<(Entity, &Name), Added<Name>>,
    parents: Query<&ChildOf>,
    rigs: Query<&Rig>,
    mut transforms: Query<&mut Transform>,
) {
    for (head, name) in &heads {
        if name.as_str() != HEAD {
            continue;
        }
        let Some(project) = parents
            .iter_ancestors(head)
            .find_map(|up| rigs.get(up).ok())
            .map(|rig| rig.0.clone())
        else {
            continue;
        };
        let Some(feed) = feeds.by_project.get_mut(&project) else {
            continue;
        };
        feed.head = Some(head);
        if let Ok(mut at) = transforms.get_mut(head) {
            at.rotation = turn(feed.framing);
        }
    }
}

/// Letters each feed's overlay (its room's name and CAM number) once the font is read,
/// and again when its place on the wall or the page's words change.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn letter_feeds(
    mut commands: Commands,
    kit: Option<Res<Kit>>,
    cctv: Res<Cctv>,
    strings: Res<WorldStrings>,
    mut feeds: ResMut<Feeds>,
    mut lettering: ResMut<Lettering>,
    mut stores: (
        ResMut<Assets<Image>>,
        ResMut<Assets<Mesh>>,
        ResMut<Assets<ColorMaterial>>,
        ResMut<Assets<MonitorMaterial>>,
    ),
) {
    let Some(kit) = kit else { return };
    let (images, meshes, colours, materials) = &mut stores;
    for on_wall in &cctv.0.wall().feeds {
        let Some(feed) = feeds.by_project.get_mut(&on_wall.project) else {
            continue;
        };
        let cam = cam_text(strings.get(strings::CAM), on_wall.cam);
        let want = (on_wall.name.clone(), cam);
        if feed.lettered.as_ref() == Some(&want) {
            continue;
        }
        let mut letter = |overlay, text: &str| {
            lettering.overlay(
                overlay,
                text,
                &kit,
                &mut commands,
                (&mut **images, &mut **meshes, &mut **colours),
            )
        };
        let (Some(name), Some(cam)) = (
            letter(Overlay::FeedName, &want.0),
            letter(Overlay::FeedCam, &want.1),
        ) else {
            return;
        };
        if let Some(mut material) = materials.get_mut(&feed.material) {
            material.name = Some(name);
            material.cam = Some(cam);
        }
        eprintln!("[world] feed lettered: {:?} {:?}", want.1, want.0);
        feed.lettered = Some(want);
    }
}

/// A feed's CAM number as its overlay shows it: the page's word and two digits.
pub(super) fn cam_text(word: &str, cam: usize) -> String {
    format!("{word} {cam:02}").trim().to_string()
}

/// Hangs the monitor wall as the state machine lays it out, whenever that changes: a
/// monitor per feed showing its feed, or one NO SIGNAL screen with no open project.
#[allow(clippy::too_many_arguments)]
fn hang_wall(
    mut commands: Commands,
    kit: Option<Res<Kit>>,
    cctv: Res<Cctv>,
    mut feeds: ResMut<Feeds>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let Some(kit) = kit else { return };
    // Flags only colour the feeds (`flag_feeds`); the monitors hang again only when
    // the wall's layout changes.
    let wall = &Wall {
        feeds: cctv
            .0
            .wall()
            .feeds
            .iter()
            .map(|feed| cctv_state::Feed {
                flag: None,
                ..feed.clone()
            })
            .collect(),
        ..cctv.0.wall().clone()
    };
    if feeds.hung.as_ref() == Some(wall) {
        return;
    }
    let root = *feeds.wall_root.get_or_insert_with(|| {
        commands
            .spawn((
                Transform::from_translation(Vec3::from_array(layout::CCTV_ORIGIN))
                    .with_rotation(Quat::from_rotation_y(std::f32::consts::PI)),
                Visibility::default(),
                Propagate(RenderLayers::from_layers(&[layout::CCTV_LAYER])),
            ))
            .id()
    });
    for gone in feeds.monitors.drain(..) {
        commands.entity(gone).despawn();
    }
    let screen = feeds
        .screen
        .get_or_insert_with(|| meshes.add(Rectangle::new(SCREEN.x, SCREEN.y)))
        .clone();
    let monitor = |commands: &mut Commands, at: cctv_geometry::Screen| {
        commands
            .spawn((
                kit.piece("cctv_feed_monitor"),
                Transform::from_translation(Vec3::from_array(at.centre))
                    .with_rotation(Quat::from_rotation_y(at.yaw)),
                ChildOf(root),
            ))
            .id()
    };
    let mut monitors = Vec::new();
    if wall.no_signal() {
        let at = cctv_geometry::screen(1, 0, 0);
        let size = Wording::NoSignal.size();
        let quad = meshes.add(Rectangle::new(size.x, size.y));
        let hung = monitor(&mut commands, at);
        commands.spawn((
            Lettered(Wording::NoSignal),
            Mesh3d(quad),
            Transform::from_xyz(0.0, 0.0, SCREEN_FRONT),
            NotShadowCaster,
            ChildOf(hung),
        ));
        monitors.push(hung);
    }
    for on_wall in &wall.feeds {
        let Some(feed) = feeds.by_project.get(&on_wall.project) else {
            continue;
        };
        let at = cctv_geometry::screen(wall.columns, on_wall.column, on_wall.row);
        let hung = monitor(&mut commands, at);
        commands.spawn((
            Mesh3d(screen.clone()),
            MeshMaterial3d(feed.material.clone()),
            Transform::from_xyz(0.0, 0.0, SCREEN_FRONT),
            NotShadowCaster,
            // A click on it selects its room's top-ranked flagged agent.
            Clickable(Some(Target::Feed(on_wall.project.clone()))),
            ChildOf(hung),
        ));
        monitors.push(hung);
    }
    feeds.monitors = monitors;
    eprintln!(
        "[world] monitor wall: {}",
        if wall.no_signal() {
            "NO SIGNAL".to_string()
        } else {
            format!(
                "{} feeds in {} columns and {} rows",
                wall.feeds.len(),
                wall.columns,
                wall.rows
            )
        }
    );
    feeds.hung = Some(wall.clone());
}

/// Gives each flagged feed its frame and its picture its flag's colour, as the state
/// machine says, and takes them away when its room's flag goes.
fn flag_feeds(
    mut commands: Commands,
    kit: Option<Res<Kit>>,
    cctv: Res<Cctv>,
    mut feeds: ResMut<Feeds>,
    mut materials: ResMut<Assets<MonitorMaterial>>,
) {
    let Some(kit) = kit else { return };
    if !cctv.is_changed() && !kit.is_added() {
        return;
    }
    let Some(root) = feeds.wall_root else { return };
    let wall = cctv.0.wall();
    let want: HashMap<&str, (FlagKind, cctv_geometry::Screen)> = wall
        .feeds
        .iter()
        .filter_map(|feed| {
            let at = cctv_geometry::screen(wall.columns, feed.column, feed.row);
            Some((feed.project.as_str(), (feed.flag?, at)))
        })
        .collect();
    let Feeds {
        by_project, frames, ..
    } = &mut *feeds;
    let mut changed = false;
    frames.retain(|project, (frame, kind, at)| {
        let keep = want.get(project.as_str()) == Some(&(*kind, *at));
        if !keep {
            commands.entity(*frame).despawn();
            changed = true;
        }
        keep
    });
    for (&project, &(kind, at)) in &want {
        if frames.contains_key(project) {
            continue;
        }
        let frame = commands
            .spawn((
                kit.piece("cctv_feed_flag"),
                FlagTint(kind),
                Transform::from_translation(Vec3::from_array(at.centre))
                    .with_rotation(Quat::from_rotation_y(at.yaw)),
                ChildOf(root),
            ))
            .id();
        frames.insert(project.to_string(), (frame, kind, at));
        changed = true;
    }
    if changed {
        let flagged: Vec<String> = wall
            .feeds
            .iter()
            .filter_map(|feed| Some(format!("{} {:?}", feed.name, feed.flag?)))
            .collect();
        eprintln!(
            "[world] feed flags: {}",
            if flagged.is_empty() {
                "none".to_string()
            } else {
                flagged.join(", ")
            }
        );
    }
    for on_wall in &wall.feeds {
        let Some(feed) = by_project.get(&on_wall.project) else {
            continue;
        };
        let flag = flag_uniform(on_wall.flag);
        if materials
            .get(&feed.material)
            .is_some_and(|m| m.flag != flag)
        {
            if let Some(mut material) = materials.get_mut(&feed.material) {
                material.flag = flag;
            }
        }
    }
}

/// Each flag frame's glowing material in its flag's colour, shared.
#[derive(Default)]
struct FrameLook([Option<Handle<StandardMaterial>>; 2]);

/// Gives a flag frame's glowing parts its flag's colour as its scene spawns. Before the
/// surfaces' swap, which then keeps it ([`Bright`]).
fn tint_frames(
    mut commands: Commands,
    parts: Query<(Entity, &GltfMaterialName), Added<GltfMaterialName>>,
    parents: Query<&ChildOf>,
    tints: Query<&FlagTint>,
    mut look: Local<FrameLook>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for (part, name) in &parts {
        if name.0 != "cctv_flag" {
            continue;
        }
        let Some(FlagTint(kind)) = parents
            .iter_ancestors(part)
            .find_map(|up| tints.get(up).ok())
            .copied()
        else {
            continue;
        };
        let material = look.0[kind as usize]
            .get_or_insert_with(|| {
                let colour = flag_colour(kind);
                materials.add(StandardMaterial {
                    base_color: Color::linear_rgb(colour.x, colour.y, colour.z),
                    emissive: LinearRgba::rgb(colour.x, colour.y, colour.z) * FRAME_GLOW,
                    perceptual_roughness: 0.4,
                    ..default()
                })
            })
            .clone();
        commands
            .entity(part)
            .insert((MeshMaterial3d(material.clone()), Bright(material)));
    }
}

/// Puts the flag's words on a flag frame's tag bar as its scene spawns.
fn letter_tags(
    mut commands: Commands,
    marks: Query<(Entity, &Name), Added<Name>>,
    parents: Query<&ChildOf>,
    tints: Query<&FlagTint>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    for (mark, name) in &marks {
        let Some((_, width)) = [FEED_TAG, FOCUS_TAG]
            .into_iter()
            .find(|(tag, _)| name.as_str() == *tag)
        else {
            continue;
        };
        let Some(FlagTint(kind)) = parents
            .iter_ancestors(mark)
            .find_map(|up| tints.get(up).ok())
            .copied()
        else {
            continue;
        };
        let wording = Wording::Flag(kind);
        let size = wording.size() * width / wording.size().x;
        commands.spawn((
            Lettered(wording),
            Mesh3d(meshes.add(Rectangle::new(size.x, size.y))),
            Transform::from_xyz(0.0, 0.0, 0.001),
            NotShadowCaster,
            ChildOf(mark),
        ));
    }
}

/// Whether the player sees the CCTV room: from inside it, or through its live doorway.
pub fn cctv_seen(space: Space, live: Live) -> bool {
    space == Space::Cctv
        || live.0
            == Some(LiveDoorway {
                behind: Space::Cctv,
                from_inside: false,
            })
}

/// Turns on this frame's turn of feed cameras while the CCTV room is seen and the world
/// draws (from the corridor through its doorway, every other frame), and every other
/// feed camera off.
#[allow(clippy::too_many_arguments)]
fn refresh_feeds(
    space: Res<Space>,
    live: Res<Live>,
    main: Single<&Camera, (With<Camera3d>, Without<PortalCamera>)>,
    mut cctv: ResMut<Cctv>,
    mut feeds: ResMut<Feeds>,
    mut drawing: ResMut<FeedsDrawing>,
    mut cameras: Query<&mut Camera, (With<FeedCamera>, With<PortalCamera>)>,
    mut materials: ResMut<Assets<MonitorMaterial>>,
) {
    let due = match (
        main.is_active,
        *space == Space::Cctv,
        cctv_seen(*space, *live),
    ) {
        (true, true, _) => cctv.0.refresh(),
        (true, false, true) => cctv.0.refresh_through_doorway(),
        _ => Vec::new(),
    };
    for (project, feed) in &mut feeds.by_project {
        let on = due.contains(project);
        if let Ok(mut camera) = cameras.get_mut(feed.camera) {
            if camera.is_active != on {
                camera.is_active = on;
            }
        }
        if on && !feed.drawn {
            feed.drawn = true;
            if let Some(mut material) = materials.get_mut(&feed.material) {
                material.params.x = 1.0;
            }
        }
    }
    drawing.set_if_neq(FeedsDrawing(due.len()));
}
