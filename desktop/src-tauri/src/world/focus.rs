//! The CCTV room's two focus monitors and its ticker (management-rooms ticket 17; spec:
//! "Focus monitors and the ticker"). The CCTV state machine (`cctv_state`) says whom
//! each focus monitor shows, when its camera draws, and what the ticker lists; this
//! draws it.
//!
//! - **Focus monitors:** two `cctv_focus_monitor`s above the monitor wall, centred on the
//!   room. Each has its own close-up camera ([`FocusCamera`], also a feed and portal
//!   camera, so the main camera's systems leave it alone and it takes the main camera's
//!   tone mapping without the haze) standing in front of its agent's robot face
//!   (`cctv_geometry::focus_camera`), on that room's render layer, rendering into its own
//!   320 × 180 image with `Msaa::Off`. Its screen ([`FocusMaterial`], `focus.wesl`)
//!   shows the picture in the flag's colour with targeting brackets, the CAM number
//!   top-left, the model and agent ("OPUS-5.5 / root") and the room's name bottom-left,
//!   and the flag with its timer ("WAITING ON YOU 04:12") bottom-right. Its
//!   `cctv_focus_flag` frame glows in the flag's colour, with the flag's words on its
//!   tag. A killed agent (a kill playing, or a wreck) has neither colour nor frame, and
//!   its status reads the kill's progress or WRECKED. With nobody to show, it reads NO
//!   SIGNAL and its camera never draws.
//! - **Cheap on purpose:** each camera draws every eighth frame (7.5 times a second), the
//!   two never within four frames of each other, and only while the CCTV room is seen and
//!   the world draws, like the wall feeds.
//! - **A punishment playing** on a focus monitor's agent (focus monitor 1 holds the newest)
//!   is shown wide, the whole desk with the ceiling over it and the aisles round it
//!   (`cctv_geometry::punishment_camera`), and drawn every frame until it is over: the
//!   bolt lasts 300 ms, which a turn every eighth frame missed at the CCTV room's
//!   20-odd frames a second (owner-run-1).
//! - **The ticker** under the wall's lowest row scrolls every flagged agent: the room,
//!   the model, the flag and its timer, most urgent first ([`TickerMaterial`],
//!   `ticker.wesl`).
//!
//! They hang once the world has drawn its first frame, so they never hold its start up.
//!
//! Timers and the ticker are live texts (`lettering::Live`), redrawn when their words
//! change, about once a second. Project and model names are data, never translated; the
//! flag words, "root", the timer's units and NO SIGNAL come from the page (`strings`).

use agents::FlagKind;
use bevy::app::Propagate;
use bevy::asset::{embedded_asset, embedded_path, AssetPath};
use bevy::camera::visibility::RenderLayers;
use bevy::camera::RenderTarget;
use bevy::light::NotShadowCaster;
use bevy::pbr::{Material, MaterialPlugin};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, TextureFormat};
use bevy::render::view::{Msaa, NoIndirectDrawing};
use bevy::shader::ShaderRef;

use super::aim::Clickable;
use super::cctv_geometry;
use super::cctv_state::{FocusView, Target, Words};
use super::clock::WorldClock;
use super::collision::Space;
use super::engine::FirstFrameDrawn;
use super::feeds::{self, Cctv, FeedCamera, FeedsDrawing, FlagTint, RefreshFeeds};
use super::floor_plan::layout;
use super::lettering::{Lettered, Lettering, Live, LiveTexts, Overlay, Wording};
use super::pieces::Kit;
use super::portals::{IndirectDrawing, PortalCamera};
use super::strings::{self, WorldStrings};

/// A focus monitor's picture, in pixels: the wall feeds' size, about what its screen
/// covers of the view from the console. 640 × 360 cost the owner's iGPU 8–10 ms a frame
///; see `cctv_state`'s `FOCUS_EVERY` for the rate.
const FOCUS_IMAGE: UVec2 = UVec2::new(320, 180);
/// The focus screen's size (`cctv_monitors.py`'s `cctv_focus_screen`), and the picture's
/// place in front of its monitor's back (its glass at 0.071).
const FOCUS_SCREEN: Vec2 = Vec2::new(1.52, 0.855);
const FOCUS_FRONT: f32 = 0.073;
/// The ticker's screen (`cctv_ticker_screen`) and its picture in front of the bar's back
/// (its glass at 0.052).
const TICKER_SCREEN: Vec2 = Vec2::new(5.1, 0.09);
const TICKER_FRONT: f32 = 0.053;
/// What separates the ticker's fields, and its entries.
const TICKER_FIELD: &str = " :: ";
const TICKER_ENTRY: &str = "     ///     ";
/// How far a focus camera sees: past the robot, into the room behind it.
const FOCUS_FAR: f32 = 12.0;

/// A focus monitor's close-up camera.
#[derive(Component, Clone, Copy, Debug)]
pub struct FocusCamera;

/// What a focus monitor's screen shows (`focus.wesl`).
#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
pub struct FocusMaterial {
    /// x: 1 once the camera has drawn its agent; y: the grain seed.
    #[uniform(0)]
    params: Vec4,
    /// rgb: the flag's colour; a: 1 while its agent is flagged (0 for a killed agent,
    /// drawn in the wall feeds' plain tint).
    #[uniform(1)]
    flag: Vec4,
    #[texture(2)]
    #[sampler(3)]
    feed: Handle<Image>,
    #[texture(4)]
    #[sampler(5)]
    cam: Option<Handle<Image>>,
    #[texture(6)]
    #[sampler(7)]
    name: Option<Handle<Image>>,
    #[texture(8)]
    #[sampler(9)]
    who: Option<Handle<Image>>,
    #[texture(10)]
    #[sampler(11)]
    status: Option<Handle<Image>>,
}

impl Material for FocusMaterial {
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
            AssetPath::from_path_buf(embedded_path!("focus.wesl")).with_source("embedded"),
        )
    }
}

/// What the ticker's screen shows (`ticker.wesl`): its line of entries, scrolled.
#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
pub struct TickerMaterial {
    /// x: the line's width in its image, in pixels, padding included (0: nothing to
    /// show); y, z: the image's size in pixels.
    #[uniform(0)]
    params: Vec4,
    #[texture(1)]
    #[sampler(2)]
    line: Option<Handle<Image>>,
}

impl Material for TickerMaterial {
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
            AssetPath::from_path_buf(embedded_path!("ticker.wesl")).with_source("embedded"),
        )
    }
}

/// One focus monitor: its camera, its picture's quad and material, its NO SIGNAL quad,
/// its flag frame, whom it shows and what its overlays say.
struct Monitor {
    camera: Entity,
    picture: Entity,
    no_signal: Entity,
    material: Handle<FocusMaterial>,
    frame: Option<(Entity, FlagKind)>,
    showing: Option<FocusView>,
    lettered: Option<[String; 3]>,
    status: Option<String>,
}

/// The ticker: its bar, its material, the rows it hangs under and its line.
struct Ticker {
    bar: Entity,
    material: Handle<TickerMaterial>,
    rows: usize,
    line: Option<String>,
    /// Its entries as last logged.
    entries: Option<Vec<String>>,
}

#[derive(Resource, Default)]
struct Focus {
    root: Option<Entity>,
    monitors: Vec<Monitor>,
    ticker: Option<Ticker>,
}

pub struct FocusPlugin;

impl Plugin for FocusPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "focus.wesl");
        embedded_asset!(app, "ticker.wesl");
        app.add_plugins((
            MaterialPlugin::<FocusMaterial>::default(),
            MaterialPlugin::<TickerMaterial>::default(),
        ))
        .init_resource::<Focus>()
        .add_systems(
            Update,
            (
                (hang_focus, follow_focus, letter_focus, tick)
                    .chain()
                    .in_set(LiveTexts),
                refresh_focus.after(RefreshFeeds),
            ),
        );
    }
}

/// For measuring, a debug build's `WORLD_DEBUG_FOCUS_IMAGE=<w>x<h>`: the focus monitors'
/// picture size instead of [`FOCUS_IMAGE`].
fn focus_image() -> UVec2 {
    let debug = cfg!(debug_assertions)
        .then(|| std::env::var("WORLD_DEBUG_FOCUS_IMAGE").ok())
        .flatten()
        .and_then(|size| {
            let (w, h) = size.split_once('x')?;
            Some(UVec2::new(w.parse().ok()?, h.parse().ok()?))
        });
    if let Some(size) = debug {
        eprintln!("[world] debug: focus monitors draw {}x{}", size.x, size.y);
    }
    debug.unwrap_or(FOCUS_IMAGE)
}

/// For measuring, a debug build's `WORLD_DEBUG_FOCUS_TOGGLE=<seconds>`: the focus cameras
/// are off and on in turns of that long, so one launch compares both under the same load.
fn focus_toggle() -> Option<f32> {
    if !cfg!(debug_assertions) {
        return None;
    }
    std::env::var("WORLD_DEBUG_FOCUS_TOGGLE")
        .ok()?
        .parse()
        .ok()
        .filter(|s: &f32| *s > 0.0)
}

/// For measuring, a debug build's `WORLD_DEBUG_FOCUS_FAR=<metres>`: how far a focus camera
/// sees, instead of [`FOCUS_FAR`].
fn focus_far() -> f32 {
    let debug = cfg!(debug_assertions)
        .then(|| std::env::var("WORLD_DEBUG_FOCUS_FAR").ok())
        .flatten()
        .and_then(|far| far.parse::<f32>().ok())
        .filter(|far| *far > 0.5);
    if let Some(far) = debug {
        eprintln!("[world] debug: focus cameras see {far} m");
    }
    debug.unwrap_or(FOCUS_FAR)
}

/// Hangs the two focus monitors with their cameras, and the ticker, once the world has
/// drawn its first frame: their screens' pipelines and the focus cameras' views then
/// never hold the world's start up (it waits for every queued pipeline).
#[allow(clippy::too_many_arguments)]
fn hang_focus(
    mut commands: Commands,
    kit: Option<Res<Kit>>,
    drawn: Option<Res<FirstFrameDrawn>>,
    indirect: Res<IndirectDrawing>,
    mut focus: ResMut<Focus>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<FocusMaterial>>,
    mut tickers: ResMut<Assets<TickerMaterial>>,
) {
    let (Some(kit), Some(_)) = (kit, drawn) else {
        return;
    };
    if focus.root.is_some() {
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
    focus.root = Some(root);
    let picture = meshes.add(Rectangle::new(FOCUS_SCREEN.x, FOCUS_SCREEN.y));
    let size = focus_image();
    let far = focus_far();
    for n in 0..2 {
        let at = cctv_geometry::focus_screen(n);
        let monitor = commands
            .spawn((
                kit.piece("cctv_focus_monitor"),
                Transform::from_translation(Vec3::from_array(at.centre))
                    .with_rotation(Quat::from_rotation_y(at.yaw)),
                ChildOf(root),
            ))
            .id();
        let image = images.add(Image::new_target_texture(
            size.x,
            size.y,
            TextureFormat::Bgra8UnormSrgb,
            None,
        ));
        let material = materials.add(FocusMaterial {
            params: Vec4::new(0.0, 3.7 + n as f32 * 11.3, 0.0, 0.0),
            flag: Vec4::ZERO,
            feed: image.clone(),
            cam: None,
            name: None,
            who: None,
            status: None,
        });
        let quad = commands
            .spawn((
                Mesh3d(picture.clone()),
                MeshMaterial3d(material.clone()),
                Transform::from_xyz(0.0, 0.0, FOCUS_FRONT),
                NotShadowCaster,
                Visibility::Hidden,
                // A click on it selects the agent it shows.
                Clickable(Some(Target::Focus(n))),
                ChildOf(monitor),
            ))
            .id();
        let no_signal = commands
            .spawn((
                Lettered(Wording::NoSignal),
                Mesh3d(picture.clone()),
                Transform::from_xyz(0.0, 0.0, FOCUS_FRONT),
                NotShadowCaster,
                ChildOf(monitor),
            ))
            .id();
        let mut camera = commands.spawn((
            Camera3d::default(),
            Camera {
                order: -1,
                is_active: false,
                ..default()
            },
            RenderTarget::Image(image.into()),
            Projection::Perspective(PerspectiveProjection {
                near: super::collision::NEAR,
                far,
                ..default()
            }),
            Msaa::Off,
            super::quality::clusters(),
            RenderLayers::none(),
            PortalCamera,
            FeedCamera,
            FocusCamera,
        ));
        if !indirect.0 {
            camera.insert(NoIndirectDrawing);
        }
        focus.monitors.push(Monitor {
            camera: camera.id(),
            picture: quad,
            no_signal,
            material,
            frame: None,
            showing: None,
            lettered: None,
            status: None,
        });
    }
    let material = tickers.add(TickerMaterial {
        params: Vec4::ZERO,
        line: None,
    });
    let bar = commands
        .spawn((
            kit.piece("cctv_ticker"),
            Transform::from_translation(Vec3::from_array(cctv_geometry::ticker_screen(1).centre)),
            ChildOf(root),
        ))
        .with_children(|bar| {
            bar.spawn((
                Mesh3d(meshes.add(Rectangle::new(TICKER_SCREEN.x, TICKER_SCREEN.y))),
                MeshMaterial3d(material.clone()),
                Transform::from_xyz(0.0, 0.0, TICKER_FRONT),
                NotShadowCaster,
            ));
        })
        .id();
    focus.ticker = Some(Ticker {
        bar,
        material,
        rows: 1,
        line: None,
        entries: None,
    });
    eprintln!("[world] focus monitors and ticker hung");
}

/// Points each focus monitor's camera at its agent when the state machine's choice
/// changes, shows NO SIGNAL for nobody, gives it its flag's frame and colour, and
/// keeps the ticker under the wall's lowest row.
#[allow(clippy::too_many_arguments)]
fn follow_focus(
    mut commands: Commands,
    kit: Option<Res<Kit>>,
    cctv: Res<Cctv>,
    mut focus: ResMut<Focus>,
    mut materials: ResMut<Assets<FocusMaterial>>,
    mut cameras: Query<(&mut Transform, &mut Projection), With<FocusCamera>>,
    mut transforms: Query<&mut Transform, Without<FocusCamera>>,
) {
    let Some(kit) = kit else { return };
    let Some(root) = focus.root else { return };
    let chosen = cctv.0.focus_views();
    for (n, (monitor, want)) in focus.monitors.iter_mut().zip(chosen).enumerate() {
        if monitor.showing == want {
            continue;
        }
        let moved = match (&monitor.showing, &want) {
            (Some(had), Some(now)) => {
                let (had_wide, now_wide) = (had.wide, now.wide);
                let (had, now) = (&had.agent, &now.agent);
                (&had.desk, had.slot, had.row, had.column, had_wide)
                    != (&now.desk, now.slot, now.row, now.column, now_wide)
            }
            _ => true,
        };
        if moved {
            commands.entity(monitor.camera).insert(match &want {
                Some(view) => RenderLayers::from_layers(&layout::seen_layers(view.agent.slot)),
                None => RenderLayers::none(),
            });
            if let Some(FocusView { agent, wide, .. }) = &want {
                // While a punishment plays on it, the whole show; else the face.
                let pose = if *wide {
                    cctv_geometry::punishment_camera(agent.row, agent.column)
                } else {
                    cctv_geometry::focus_camera(agent.row, agent.column)
                };
                let origin = Vec3::from_array(layout::interior_origin(agent.slot));
                if let Ok((mut at, mut projection)) = cameras.get_mut(monitor.camera) {
                    *at = Transform::from_translation(origin + Vec3::from_array(pose.eye))
                        .with_rotation(Quat::from_euler(EulerRot::YXZ, pose.yaw, pose.pitch, 0.0));
                    if let Projection::Perspective(lens) = &mut *projection {
                        lens.fov = pose.fov;
                    }
                }
            }
            if let Some(mut material) = materials.get_mut(&monitor.material) {
                // Static until the camera has drawn its new agent.
                material.params.x = 0.0;
            }
        }
        // A killed agent's view has no flag: no colour and no frame.
        let kind = want.as_ref().and_then(|view| view.flag);
        let reflagged = monitor.showing.as_ref().and_then(|view| view.flag) != kind;
        let reworded =
            monitor.showing.as_ref().map(|view| view.words) != want.as_ref().map(|view| view.words);
        if let Some(mut material) = materials.get_mut(&monitor.material) {
            material.flag = feeds::flag_uniform(kind);
        }
        commands.entity(monitor.picture).insert(if want.is_some() {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
        commands
            .entity(monitor.no_signal)
            .insert(if want.is_some() {
                Visibility::Hidden
            } else {
                Visibility::Inherited
            });
        if monitor.frame.map(|(_, had)| had) != kind {
            if let Some((frame, _)) = monitor.frame.take() {
                commands.entity(frame).despawn();
            }
            if let Some(kind) = kind {
                let at = cctv_geometry::focus_screen(n);
                let frame = commands
                    .spawn((
                        kit.piece("cctv_focus_flag"),
                        FlagTint(kind),
                        Transform::from_translation(Vec3::from_array(at.centre))
                            .with_rotation(Quat::from_rotation_y(at.yaw)),
                        ChildOf(root),
                    ))
                    .id();
                monitor.frame = Some((frame, kind));
            }
        }
        if moved || reflagged || reworded {
            eprintln!(
                "[world] focus monitor {}: {}",
                n + 1,
                want.as_ref()
                    .map_or("NO SIGNAL".to_string(), |view| format!(
                        "{} in {} ({}, {:?}, since {}), {}",
                        view.agent.desk.agent,
                        view.agent.project,
                        view.flag
                            .map_or("no flag".to_string(), |kind| format!("{kind:?}")),
                        view.words,
                        view.agent.since,
                        if view.wide {
                            "wide on the punishment, every frame"
                        } else {
                            "close-up"
                        }
                    ))
            );
        }
        monitor.showing = want;
        monitor.lettered = None;
        monitor.status = None;
    }
    let rows = cctv.0.wall().rows;
    if let Some(ticker) = &mut focus.ticker {
        if ticker.rows != rows {
            ticker.rows = rows;
            if let Ok(mut at) = transforms.get_mut(ticker.bar) {
                at.translation = Vec3::from_array(cctv_geometry::ticker_screen(rows).centre);
            }
        }
    }
}

/// Letters each focus monitor's fixed overlays for its agent: its room's CAM number and
/// name (the wall feeds' own images) and its model and agent, once the font is read.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn letter_focus(
    mut commands: Commands,
    kit: Option<Res<Kit>>,
    strings: Res<WorldStrings>,
    mut focus: ResMut<Focus>,
    mut lettering: ResMut<Lettering>,
    mut stores: (
        ResMut<Assets<Image>>,
        ResMut<Assets<Mesh>>,
        ResMut<Assets<ColorMaterial>>,
        ResMut<Assets<FocusMaterial>>,
    ),
) {
    let Some(kit) = kit else { return };
    let (images, meshes, colours, materials) = &mut stores;
    let reworded = strings.is_changed();
    for monitor in &mut focus.monitors {
        let Some(FocusView { agent, .. }) = &monitor.showing else {
            continue;
        };
        let want = [
            feeds::cam_text(strings.get(strings::CAM), agent.cam),
            agent.name.clone(),
            who(&agent.model, &strings),
        ];
        if !reworded && monitor.lettered.as_ref() == Some(&want) {
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
        let (Some(cam), Some(name), Some(who)) = (
            letter(Overlay::FeedCam, &want[0]),
            letter(Overlay::FeedName, &want[1]),
            letter(Overlay::FocusWho, &want[2]),
        ) else {
            return;
        };
        if let Some(mut material) = materials.get_mut(&monitor.material) {
            material.cam = Some(cam);
            material.name = Some(name);
            material.who = Some(who);
        }
        monitor.lettered = Some(want);
    }
}

/// The model and agent a focus monitor shows, in the page's words: "OPUS-5.5 / root".
/// Only root agents are flagged, so the agent is always the page's word for a root agent.
fn who(model: &str, strings: &WorldStrings) -> String {
    let root = strings.get(strings::ROOT_AGENT);
    if model.is_empty() {
        root.to_string()
    } else {
        strings::fill(
            strings.get(strings::FOCUS_WHO),
            &[("model", model), ("agent", root)],
        )
    }
}

/// Rewrites the focus monitors' flag timers and the ticker's line as the clock moves
/// (about once a second, when their words change).
#[allow(clippy::too_many_arguments)]
fn tick(
    mut commands: Commands,
    kit: Option<Res<Kit>>,
    cctv: Res<Cctv>,
    clock: Res<WorldClock>,
    strings: Res<WorldStrings>,
    mut focus: ResMut<Focus>,
    mut lettering: ResMut<Lettering>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<FocusMaterial>>,
    mut tickers: ResMut<Assets<TickerMaterial>>,
) {
    let Some(kit) = kit else { return };
    let status = |words, timer| strings.status(words, timer, clock.0);
    for (n, monitor) in focus.monitors.iter_mut().enumerate() {
        let Some(view) = &monitor.showing else {
            continue;
        };
        let words = status(view.words, view.timer);
        if monitor.status.as_ref() == Some(&words) {
            continue;
        }
        let Some(live) = lettering.live(
            Live::FocusStatus(n),
            &words,
            &kit,
            &mut commands,
            &mut images,
        ) else {
            return;
        };
        if let Some(mut material) = materials.get_mut(&monitor.material) {
            if material.status.as_ref() != Some(&live.image) {
                material.status = Some(live.image);
            }
        }
        monitor.status = Some(words);
    }
    let Some(ticker) = &mut focus.ticker else {
        return;
    };
    let line = cctv
        .0
        .ticker()
        .iter()
        .map(|agent| {
            let status = status(Words::Flag(agent.kind), Some(agent.since));
            [agent.name.as_str(), agent.model.as_str(), &status]
                .into_iter()
                .filter(|field| !field.is_empty())
                .collect::<Vec<_>>()
                .join(TICKER_FIELD)
        })
        .collect::<Vec<_>>()
        .join(TICKER_ENTRY);
    if ticker.line.as_ref() == Some(&line) {
        return;
    }
    let Some(live) = lettering.live(Live::Ticker, &line, &kit, &mut commands, &mut images) else {
        return;
    };
    if let Some(mut material) = tickers.get_mut(&ticker.material) {
        let size = images.get(&live.image).map_or(Vec2::ONE, |i| i.size_f32());
        material.params = Vec4::new(
            if line.is_empty() {
                0.0
            } else {
                live.width + 2.0 * TICKER_PAD
            },
            size.x,
            size.y,
            0.0,
        );
        material.line = Some(live.image);
    }
    let entries: Vec<String> = cctv
        .0
        .ticker()
        .iter()
        .map(|agent| format!("{} {:?}", agent.name, agent.kind))
        .collect();
    if ticker.entries.as_ref() != Some(&entries) {
        eprintln!(
            "[world] ticker: {} flagged{}{}",
            entries.len(),
            if entries.is_empty() { "" } else { ": " },
            entries.join(", ")
        );
        ticker.entries = Some(entries);
    }
    ticker.line = Some(line);
}

/// The live texts' padding at their image's edge (`lettering`'s overlay padding).
const TICKER_PAD: f32 = 4.0;

/// Turns on this frame's focus cameras while the CCTV room is seen and the world draws,
/// every eighth frame each, and keeps the rest off.
#[allow(clippy::too_many_arguments)]
fn refresh_focus(
    space: Res<Space>,
    live: Res<super::portals::Live>,
    main: Single<&Camera, (With<Camera3d>, Without<PortalCamera>)>,
    mut cctv: ResMut<Cctv>,
    focus: Res<Focus>,
    mut drawing: ResMut<FeedsDrawing>,
    mut cameras: Query<&mut Camera, (With<FocusCamera>, With<PortalCamera>)>,
    mut materials: ResMut<Assets<FocusMaterial>>,
    time: Res<Time<Real>>,
    mut toggle: Local<Option<Option<f32>>>,
    mut toggled: Local<Option<bool>>,
) {
    let seen = main.is_active && feeds::cctv_seen(*space, *live);
    let mut due = if seen {
        cctv.0.refresh_focus()
    } else {
        [false, false]
    };
    if let Some(period) = *toggle.get_or_insert_with(focus_toggle) {
        let on = (time.elapsed_secs() / period) as u64 % 2 == 1;
        if *toggled != Some(on) {
            *toggled = Some(on);
            eprintln!(
                "[world] debug: focus cameras {}",
                if on { "on" } else { "off" }
            );
        }
        if !on {
            due = [false, false];
        }
    }
    for (monitor, on) in focus.monitors.iter().zip(due) {
        if let Ok(mut camera) = cameras.get_mut(monitor.camera) {
            if camera.is_active != on {
                camera.is_active = on;
            }
        }
        if on
            && materials
                .get(&monitor.material)
                .is_some_and(|m| m.params.x < 0.5)
        {
            if let Some(mut material) = materials.get_mut(&monitor.material) {
                material.params.x = 1.0;
            }
        }
    }
    let count = due.iter().filter(|on| **on).count();
    if count > 0 {
        drawing.0 += count;
    }
}
