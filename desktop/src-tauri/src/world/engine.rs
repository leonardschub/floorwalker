//! The real world launcher: Bevy 0.20 on a `bevy-world` thread in the Tauri process,
//! presenting into a Wayland subsurface below the transparent webview (spec: "The real
//! world launcher"; ADR-0009; the spike's `start_world` and `run_world`).
//!
//! Bevy owns no window and no event loop. The primary `Window` entity is spawned by hand
//! with the subsurface's raw handles, and a custom runner drains a message channel and
//! calls `app.update()` in a loop. GTK `size-allocate` on the webview moves the
//! subsurface and sends the new surface size to Bevy; the page's world view rectangle
//! becomes the camera viewport. Keys from GTK pass the [`KeyGate`] and are written as
//! Bevy `KeyboardInput` before each update.

use std::panic::{self, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use bevy::app::PluginsState;
use bevy::camera::RenderTarget;
use bevy::core_pipeline::core_3d::Opaque3d;
use bevy::input::keyboard::{Key, KeyboardInput, NativeKey};
use bevy::prelude::*;
use bevy::render::camera::ExtractedCamera;
use bevy::render::render_phase::ViewBinnedRenderPhases;
use bevy::render::render_resource::PipelineCache;
use bevy::render::view::screenshot::{save_to_disk, Screenshot};
use bevy::render::view::ExtractedView;
use bevy::render::{Render, RenderApp, RenderSystems};
use bevy::window::{
    ExitCondition, PresentMode, PrimaryWindow, RawHandleWrapper, RawHandleWrapperHolder,
    WindowCreated, WindowPlugin, WindowResized, WindowResolution, WindowWrapper,
};
use raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle};
use tauri::{AppHandle, Manager, WebviewWindow};

use super::aim;
use super::cctv::CctvPlugin;
use super::clutter::ClutterPlugin;
use super::collision::Space;
use super::controller::{
    Button, FromWorld, Launcher, Rect, Reports, StartFailure, ToWorld, WorldLink,
};
use super::controls::Controls;
use super::corridor::{self, CorridorPlugin, START_LOOK};
use super::floor_plan::DeskId;
use super::go_to_desk;
use super::keys::{Change, KeyGate};
use super::lettering::LetteringPlugin;
use super::look::Look;
use super::pc_screens::{self, OnScreen, PcScreensPlugin, SessionScreens};
use super::pieces::{Loading, PiecesPlugin};
use super::portals::{FollowMain, IndirectDrawing, Live, PortalCamera, PortalsPlugin};
use super::quality::{self, Presenter, ScaledView, ViewQuality};
use super::rooms::{self, RoomsPlugin};
use super::seat::{self, SeatOutbox, SeatPlugin};
use super::strings::WorldStrings;
use super::surface::{FrameClock, Geometry, Subsurface, SurfaceHandles};
use super::surfaces::SurfacesPlugin;
use super::viewport;
use super::walk::{WalkPlugin, Whereabouts};

/// How often the world thread logs its tick count, frames drawn, yaw/pitch and where the
/// player stands.
const LOG_EVERY: Duration = Duration::from_secs(5);

/// The simulation's pace while the world draws nothing: not shown, or suspended. While
/// it draws, the compositor's frame callbacks pace it instead.
const TICK: Duration = Duration::from_micros(16_667);

/// How long a shown world waits for the frame callback of the frame it drew last before
/// it simulates a tick without drawing. A little more than one tick, so a callback that
/// comes a moment late does not cost a frame.
const FRAME_WAIT: Duration = Duration::from_micros(25_000);

/// How long after starting a frame the world may start the next one while the last is
/// still to be shown ([`FrameClock::wait`]; ticket 27): one refresh at 60 Hz. Sooner, a
/// world faster than the display would draw frames the compositor never shows.
const PIPELINE_AFTER: Duration = TICK;

/// How long a frame callback may stay away while the world is shown before the world
/// counts as suspended (for the log; drawing waits for the callback either way).
const SUSPEND_AFTER: Duration = Duration::from_millis(500);

/// How long a world stays suspended before its live screens are hung up: a compositor
/// that holds a frame callback back for a moment (seen in a nested KWin as the cursor is
/// captured) is no reason to restart attaches that take seconds to start. Back and
/// keep warm hang up at once.
const HANG_UP_SUSPENDED_AFTER: Duration = Duration::from_secs(2);

/// Whether a world has installed Bevy's logger. The logger is process-global: a fresh
/// world after a stop leaves `LogPlugin` out instead of failing to install it again.
static LOGGING: AtomicBool = AtomicBool::new(false);

/// How long a stop waits for the main thread to destroy the world's surface. The main
/// thread only queues behind GTK work, so this bounds a stop, not a normal wait.
const DESTROY_WAIT: Duration = Duration::from_secs(5);

/// Messages to the world thread.
enum Msg {
    /// From the controller (the page).
    World(ToWorld),
    /// The drawable surface's size in physical pixels, and the scale factor.
    Surface { width: u32, height: u32, scale: f32 },
    /// Leave the frame loop; the `App` and its render resources drop with it.
    Stop,
}

/// The world attached to the main window: its surface and its channel. Read on the main
/// thread by the `size-allocate` handler. `world` tells launched worlds apart, so a stop
/// takes only its own world's surface.
struct Attached {
    world: u64,
    subsurface: Subsurface,
    tx: mpsc::Sender<Msg>,
}

/// The attached world, if any: one slot for the whole webview.
type Slot = Arc<Mutex<Option<Attached>>>;

pub struct BevyLauncher {
    app: AppHandle,
    attached: Slot,
    /// The `size-allocate` handler is connected once, for the life of the webview, and
    /// reaches whichever world is attached at the time: none after a stop.
    geometry_hooked: Arc<AtomicBool>,
    launched: AtomicU64,
}

impl BevyLauncher {
    pub fn new(app: AppHandle) -> Self {
        Self {
            app,
            attached: Arc::default(),
            geometry_hooked: Arc::default(),
            launched: AtomicU64::new(0),
        }
    }
}

/// One launched world's lifecycle, shared by its attachment (main thread) and its stop.
#[derive(Default)]
struct Life {
    /// Set by stop. An attachment that has not run yet then never runs.
    stopped: bool,
    thread: Option<JoinHandle<()>>,
}

struct BevyLink {
    app: AppHandle,
    world: u64,
    tx: mpsc::Sender<Msg>,
    life: Arc<Mutex<Life>>,
    attached: Slot,
}

impl WorldLink for BevyLink {
    fn send(&self, msg: ToWorld) {
        // Messages queue until the world thread drains them; a gone thread drops them.
        let _ = self.tx.send(Msg::World(msg));
    }

    /// The stop order: tell the world thread to leave its loop, join it (Bevy's render
    /// resources drop with the `App`), then destroy the subsurface on the main thread.
    fn stop(self: Box<Self>) {
        let begun = Instant::now();
        let thread = {
            let mut life = self.life.lock().unwrap();
            life.stopped = true;
            life.thread.take()
        };
        let _ = self.tx.send(Msg::Stop);
        if let Some(thread) = thread {
            if thread.join().is_err() {
                eprintln!("[world] the world thread could not be joined");
            }
            eprintln!(
                "[world] world {} thread joined {} ms after the stop",
                self.world,
                begun.elapsed().as_millis()
            );
        }
        let (world, attached) = (self.world, self.attached.clone());
        let (done, destroyed) = mpsc::channel();
        // Runs inline when the stop is already on the main thread.
        let scheduled = self.app.run_on_main_thread(move || {
            let mut slot = attached.lock().unwrap();
            if slot.as_ref().is_some_and(|a| a.world == world) {
                if let Some(attached) = slot.take() {
                    attached.subsurface.destroy();
                    eprintln!("[world] world {world} subsurface destroyed");
                }
            }
            let _ = done.send(());
        });
        if scheduled.is_err() || destroyed.recv_timeout(DESTROY_WAIT).is_err() {
            eprintln!("[world] world {world}: the main thread did not destroy the surface");
        }
    }
}

impl Launcher for BevyLauncher {
    fn launch(&self, reports: Reports) -> Box<dyn WorldLink> {
        let (tx, rx) = mpsc::channel();
        let world = self.launched.fetch_add(1, Ordering::SeqCst) + 1;
        let life = Arc::new(Mutex::new(Life::default()));
        let link = Box::new(BevyLink {
            app: self.app.clone(),
            world,
            tx: tx.clone(),
            life: life.clone(),
            attached: self.attached.clone(),
        });
        let Some(window) = self.app.get_webview_window("main") else {
            eprintln!("[world] no main window to attach to");
            reports.failed(StartFailure::NoWindow);
            return link;
        };
        let force = debug::forced(world);
        let assets = asset_root(&self.app);
        let screens = self
            .app
            .try_state::<crate::terminals::SessionTerminals>()
            .and_then(|terminals| terminals.screens());
        let failing = reports.clone();
        let unreached = reports.clone();
        let attached = self.attached.clone();
        let hooked = self.geometry_hooked.clone();
        let main = window.clone();
        // GTK and Wayland work happens on the main thread, inside with_webview.
        let scheduled = window.with_webview(move |platform| {
            // Held until the thread is spawned, so a stop either comes first and nothing
            // attaches, or comes after and finds the thread to join.
            let mut life = life.lock().unwrap();
            if life.stopped {
                eprintln!("[world] world {world} stopped before it attached");
                return;
            }
            let webview = platform.inner();
            if force == Some(debug::Force::FailStart) {
                eprintln!("[world] world {world}: forced start failure (debug build)");
                reports.failed(StartFailure::Forced);
                return;
            }
            let (subsurface, geometry) = match attach(&main, &webview) {
                Ok(attachment) => attachment,
                Err(error) => {
                    eprintln!("[world] could not attach the world's surface: {error}");
                    reports.failed(StartFailure::AttachFailed);
                    return;
                }
            };
            let handles = subsurface.handles();
            let frames = subsurface.frames();
            *attached.lock().unwrap() = Some(Attached {
                world,
                subsurface,
                tx,
            });
            if !hooked.swap(true, Ordering::SeqCst) {
                follow_geometry(&webview, attached);
            }
            let (width, height) = geometry.physical();
            let surface = (width, height, geometry.scale as f32);
            let spawned = thread::Builder::new()
                .name("bevy-world".into())
                .spawn(move || {
                    // However the world ends, its live screens are hung up.
                    let _hang_up = HangUp(screens.clone());
                    let panicked = reports.clone();
                    let run = AssertUnwindSafe(|| {
                        run_world(
                            handles, frames, surface, assets, rx, reports, force, screens,
                        )
                    });
                    // The panic message has been printed by the panic hook already.
                    if panic::catch_unwind(run).is_err() {
                        eprintln!("[world] world {world}: the world thread panicked");
                        panicked.crashed();
                    }
                });
            match spawned {
                Ok(thread) => life.thread = Some(thread),
                Err(error) => {
                    eprintln!("[world] could not spawn the world thread: {error}");
                    failing.failed(StartFailure::ThreadFailed);
                }
            }
        });
        if let Err(error) = scheduled {
            eprintln!("[world] could not reach the main webview: {error}");
            unreached.failed(StartFailure::NoWebview);
        }
        link
    }
}

/// Hangs up every attach when the world ends: a stop (quitting stops the world first), a
/// crash or a restart (spec: "The terminals crate", shutdown).
struct HangUp(Option<Arc<dyn SessionScreens>>);

impl Drop for HangUp {
    fn drop(&mut self) {
        if let Some(screens) = &self.0 {
            screens.shutdown();
        }
    }
}

/// Where the world's generated assets are (ADR-0012): the regeneration output in
/// `tauri dev`, and the bundled resources in a packaged build, where Bevy's default lookup
/// beside the executable misses them.
fn asset_root(app: &AppHandle) -> PathBuf {
    let root = if tauri::is_dev() {
        PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../world-assets/out"))
    } else {
        match app.path().resource_dir() {
            Ok(resources) => resources.join("world-assets"),
            Err(error) => {
                // The load then fails, and the start with it.
                eprintln!("[world] no resource folder: {error}");
                PathBuf::from("world-assets")
            }
        }
    };
    eprintln!("[world] assets from {}", root.display());
    root
}

/// Attaches a subsurface below the main window's toplevel and places it at the webview.
/// Wayland only: there is no other attachment.
fn attach(
    window: &WebviewWindow,
    webview: &webkit2gtk::WebView,
) -> Result<(Subsurface, Geometry), String> {
    let display = match window.display_handle().map(|h| h.as_raw()) {
        Ok(RawDisplayHandle::Wayland(display)) => display.display,
        other => return Err(format!("not a Wayland display: {other:?}")),
    };
    let toplevel = match window.window_handle().map(|h| h.as_raw()) {
        Ok(RawWindowHandle::Wayland(window)) => window.surface,
        other => return Err(format!("not a Wayland window: {other:?}")),
    };
    let mut subsurface = Subsurface::attach(display, toplevel)?;
    let geometry = Geometry::of(webview);
    subsurface.place(geometry.x, geometry.y, geometry.scale);
    // GTK's next commit of the toplevel applies the subsurface's position and stacking.
    if let Some(top) = gtk::prelude::WidgetExt::toplevel(webview) {
        gtk::prelude::WidgetExt::queue_draw(&top);
    }
    eprintln!(
        "[world] subsurface attached below the toplevel at ({},{}), {}x{} logical, scale {}",
        geometry.x, geometry.y, geometry.width, geometry.height, geometry.scale
    );
    Ok((subsurface, geometry))
}

/// Keeps the attached world's subsurface on the webview and its surface size current
/// across every re-layout, resize and scale change.
fn follow_geometry(webview: &webkit2gtk::WebView, attached: Slot) {
    gtk::prelude::WidgetExt::connect_size_allocate(webview, move |webview, _| {
        let geometry = Geometry::of(webview);
        if let Some(world) = attached.lock().unwrap().as_mut() {
            world
                .subsurface
                .place(geometry.x, geometry.y, geometry.scale);
            let (width, height) = geometry.physical();
            let _ = world.tx.send(Msg::Surface {
                width,
                height,
                scale: geometry.scale as f32,
            });
        }
    });
}

/// The latest world view rectangle, applied as the camera viewport.
#[derive(Resource, Default)]
pub struct WorldRect(pub Option<Rect>);

/// Frames drawn: counted in the render world once a frame has been presented with the
/// camera viewport set, every piece loaded and spawned, every room pipeline compiled and
/// the room's meshes queued, so the first count means the room is on screen and not a
/// bare clear colour or a half-loaded room.
#[derive(Resource, Clone, Default)]
struct DrawnFrames(Arc<AtomicU64>);

/// Set once every piece is loaded and spawned (an update before the frame that counts).
#[derive(Resource, Clone, Default)]
struct PiecesIn(Arc<AtomicBool>);

/// Frames the world view: the main camera draws into the window's world view rectangle,
/// or, below full render scale, into an image of the scaled size that the presenter
/// stretches over the rectangle ([`quality::frame_view`]).
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn apply_world_rect(
    mut commands: Commands,
    rect: Res<WorldRect>,
    quality: Res<ViewQuality>,
    mut scaled: ResMut<ScaledView>,
    mut images: ResMut<Assets<Image>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut cameras: Query<(&mut Camera, &mut RenderTarget), (With<Camera3d>, Without<PortalCamera>)>,
    mut presenters: Query<&mut Camera, (With<Presenter>, Without<Camera3d>)>,
) {
    let (Some(rect), Ok(window)) = (rect.0, windows.single()) else {
        return;
    };
    let target = &window.resolution;
    let Some(r) = viewport::clamp(rect, target.physical_width(), target.physical_height()) else {
        return;
    };
    let Ok((mut camera, mut camera_target)) = cameras.single_mut() else {
        return;
    };
    let rect = (UVec2::new(r.x, r.y), UVec2::new(r.w, r.h));
    let changed = quality::frame_view(
        &mut commands,
        &mut images,
        &mut scaled,
        quality.render_scale,
        rect,
        (&mut camera, &mut camera_target),
        presenters.single_mut().ok().as_deref_mut(),
    );
    if changed {
        eprintln!(
            "[world] camera viewport ({},{}) {}x{} on a {}x{} surface, drawn at {}x{} (render scale {})",
            r.x,
            r.y,
            r.w,
            r.h,
            target.physical_width(),
            target.physical_height(),
            scaled.drawn.x,
            scaled.drawn.y,
            quality.render_scale
        );
    }
}

/// The world has drawn its first frame and reported ready. What need not be there for the
/// first frame waits for it (the CCTV room's focus monitors and ticker, `focus`), so its
/// pipelines never hold the start up.
#[derive(Resource, Clone, Copy, Debug)]
pub struct FirstFrameDrawn;

/// Counts the main camera's frames only: a doorway's view is not the world view.
fn count_drawn_frames(
    drawn: Res<DrawnFrames>,
    pieces: Res<PiecesIn>,
    cameras: Query<(&ExtractedCamera, &ExtractedView), Without<PortalCamera>>,
    opaque: Res<ViewBinnedRenderPhases<Opaque3d>>,
    pipelines: Res<PipelineCache>,
) {
    let framed = cameras.iter().any(|(camera, _)| camera.viewport.is_some());
    let room_queued = cameras.iter().any(|(_, view)| {
        opaque
            .0
            .get(&view.retained_view_entity)
            .is_some_and(|phase| !phase.is_empty())
    });
    let compiled = pipelines.waiting_pipelines().next().is_none();
    if pieces.0.load(Ordering::Relaxed) && framed && room_queued && compiled {
        drawn.0.fetch_add(1, Ordering::Relaxed);
    }
}

#[allow(clippy::too_many_arguments)]
fn run_world(
    handles: SurfaceHandles,
    mut frames: FrameClock,
    surface: (u32, u32, f32),
    assets: PathBuf,
    rx: mpsc::Receiver<Msg>,
    reports: Reports,
    force: Option<debug::Force>,
    screens: Option<Arc<dyn SessionScreens>>,
) {
    if force == Some(debug::Force::PanicStart) {
        panic!("forced panic while starting (debug build)");
    }
    let mut app = App::new();
    let strict = debug::strict_pacing();
    let in_flight = debug::in_flight().unwrap_or(super::surface::IN_FLIGHT);

    // The primary window entity, made by hand rather than by winit, carrying the
    // subsurface's raw handles.
    let wrapper = WindowWrapper::new(handles);
    let raw = RawHandleWrapper::new(&wrapper).expect("raw Wayland handles");
    let mut resolution = WindowResolution::new(surface.0.max(1), surface.1.max(1));
    resolution.set_scale_factor(surface.2);
    let window = app
        .world_mut()
        .spawn((
            Window {
                title: "world".into(),
                resolution,
                // Paced by the frame callbacks, never by a present that waits for one:
                // Mailbox does not block in Mesa's Wayland WSI, FIFO would (ticket 27).
                present_mode: if debug::unpaced() {
                    PresentMode::AutoNoVsync
                } else if strict {
                    PresentMode::AutoVsync
                } else {
                    PresentMode::Mailbox
                },
                ..default()
            },
            PrimaryWindow,
            raw.clone(),
            RawHandleWrapperHolder(Arc::new(Mutex::new(Some(raw)))),
        ))
        .id();

    let drawn = DrawnFrames::default();
    let pieces_in = PiecesIn::default();
    let mut plugins = DefaultPlugins
        .set(AssetPlugin {
            file_path: assets.to_string_lossy().into_owned(),
            ..default()
        })
        .set(WindowPlugin {
            primary_window: None,
            exit_condition: ExitCondition::DontExit,
            close_when_requested: false,
            ..default()
        })
        .disable::<bevy::app::TerminalCtrlCHandlerPlugin>()
        // Rendering in `app.update()` itself, so a frame asked to be drawn is presented
        // (and its frame callback committed) before the update returns. Rendering on
        // Bevy's own thread while the next frame is simulated gained nothing (ticket 27).
        .disable::<bevy::render::pipelined_rendering::PipelinedRenderingPlugin>();
    if LOGGING.swap(true, Ordering::SeqCst) {
        plugins = plugins.disable::<bevy::log::LogPlugin>();
    }
    app.add_plugins(plugins)
        .insert_resource(ClearColor(Color::srgb(0.043, 0.059, 0.075)))
        .insert_resource(START_LOOK)
        .insert_resource(ViewQuality::with_debug_overrides())
        .init_resource::<ScaledView>()
        .init_resource::<WorldRect>()
        .init_resource::<KeyGate>()
        .add_plugins((
            PiecesPlugin,
            super::mipmaps::MipmapsPlugin,
            CorridorPlugin,
            RoomsPlugin,
            WalkPlugin,
            PortalsPlugin,
            LetteringPlugin,
            CctvPlugin,
            super::feeds::FeedsPlugin,
            super::focus::FocusPlugin,
            SurfacesPlugin,
            ClutterPlugin,
            super::flag_markers::FlagMarkersPlugin,
        ))
        .add_plugins((
            super::clock::ClockPlugin,
            aim::AimPlugin,
            super::console::ConsolePlugin,
            super::punishments::PunishmentsPlugin,
            super::wrecks::WrecksPlugin,
            PcScreensPlugin,
            SeatPlugin,
        ))
        // The seat moves the camera before the look turns it.
        .add_systems(
            Update,
            (
                seat::keep_seat.before(apply_look),
                apply_world_rect,
                apply_look,
            )
                .before(FollowMain),
        );
    if let Some(screens) = screens {
        app.insert_resource(pc_screens::Screens(screens));
    }
    if let Some(in_room) = debug::in_room() {
        app.insert_resource(in_room);
    }
    if let Some(eye) = debug::eye() {
        app.insert_resource(corridor::DebugEye(eye));
    }
    let indirect = debug::indirect();
    app.insert_resource(IndirectDrawing(indirect));
    if let Some(shadows) = debug::room_shadows() {
        app.insert_resource(super::portals::RoomShadows(shadows));
    }
    let unpaced = debug::unpaced();
    let mut shots = debug::shots();
    let record = debug::record();
    let slow_frames = debug::slow_frames();
    let timed = debug::timing();
    if timed {
        super::timing::add(&mut app);
    }
    if let Some(render) = app.get_sub_app_mut(RenderApp) {
        render
            .insert_resource(drawn.clone())
            .insert_resource(pieces_in.clone())
            .add_systems(Render, count_drawn_frames.in_set(RenderSystems::Cleanup));
    }

    app.set_runner(move |mut app: App| {
        while app.plugins_state() == PluginsState::Adding {
            bevy::tasks::tick_global_task_pools_on_main_thread();
        }
        app.finish();
        app.cleanup();
        app.world_mut().write_message(WindowCreated { window });
        eprintln!("[world] plugins ready, entering the frame loop");

        let mut ready: Option<Instant> = None;
        let began = Instant::now();
        let mut loaded = false;
        let mut ticks: u64 = 0;
        let mut last_log = Instant::now();
        let mut shown = false;
        let mut suspended = false;
        let mut suspended_at = Instant::now();
        let mut last_tick = Instant::now();
        let mut drew = false;
        let mut last_draw = Instant::now();
        let mut trimmed = false;
        let mut stats = FrameStats::default();
        // Feed and focus camera draws, and frames drawn, since the last log line: the
        // feeds' count read at the log alone often falls on a tick that draws nothing.
        let (mut feed_draws, mut drawn_since) = (0usize, 0usize);
        let pacing = if unpaced {
            "unpaced (debug: no vsync, no frame callbacks)".to_string()
        } else if strict {
            "paced by frame callbacks, one frame at a time, FIFO (debug)".to_string()
        } else {
            format!("paced by frame callbacks, {in_flight} frames in flight, mailbox")
        };
        let quality = *app.world().resource::<ViewQuality>();
        eprintln!(
            "[world] indirect drawing {}, {pacing}; {quality:?}",
            if indirect { "on" } else { "off" },
        );
        loop {
            let was_shown = shown;
            if apply_messages(app.world_mut(), window, &rx, &mut shown) == Flow::Stop {
                eprintln!("[world] stop requested after {ticks} ticks, leaving the frame loop");
                return AppExit::Success;
            }
            if shown != was_shown {
                if shown {
                    eprintln!("[world] shown after {ticks} ticks: drawing");
                } else {
                    eprintln!("[world] not shown after {ticks} ticks: simulating without drawing");
                }
            }
            // Draw only while shown, and only once the compositor wants a frame: a
            // present that would wait for a frame callback is never reached.
            let wanted = if shown && unpaced {
                true
            } else if shown {
                // The next frame may start while the last is still to be shown, a
                // refresh after the last one started.
                let deadline = last_tick + if drew { FRAME_WAIT } else { TICK };
                let early = if strict {
                    deadline + LOG_EVERY
                } else {
                    last_draw + PIPELINE_AFTER
                };
                frames.wait(early, deadline, in_flight)
            } else {
                sleep_until(last_tick + TICK);
                false
            };
            last_tick = Instant::now();
            // The first update spawns the camera and gives the surface its first present.
            let draw = ticks == 0 || (shown && wanted);
            // Before the first frame a slow first present (shaders compiling) is not one.
            if ready.is_some()
                && shown
                && !wanted
                && !suspended
                && frames.awaited().is_some_and(|waited| waited >= SUSPEND_AFTER)
            {
                suspended = true;
                suspended_at = Instant::now();
                eprintln!("[world] suspended after {ticks} ticks: no frame callback, simulating without drawing");
            }
            if draw && suspended {
                suspended = false;
                eprintln!("[world] resumed drawing after {ticks} ticks");
            }
            drew = draw;
            set_drawing(app.world_mut(), draw);
            // Kept warm or suspended, no screen is live (spec: "Which PCs are live").
            let away = suspended && suspended_at.elapsed() >= HANG_UP_SUSPENDED_AFTER;
            let on_screen = OnScreen(shown && !away);
            if *app.world().resource::<OnScreen>() != on_screen {
                app.insert_resource(on_screen);
            }
            if draw {
                frames.ask();
                last_draw = last_tick;
            }
            // Debug builds: every frame drawn while a key is held, saved with the pose it
            // was drawn from, to check a doorway crossing frame by frame.
            let recorded = record.as_ref().filter(|_| {
                draw && app
                    .world()
                    .resource::<ButtonInput<KeyCode>>()
                    .get_pressed()
                    .next()
                    .is_some()
            });
            if let Some(dir) = recorded {
                let path = dir.join(format!("frame-{ticks:06}.png"));
                app.world_mut()
                    .spawn(Screenshot::primary_window())
                    .observe(save_to_disk(path));
            }
            // Taken by an update that draws: one that does not presents nothing, and its
            // screenshot came out black.
            if let (true, Some(since), Some((dir, at))) = (draw, ready, shots.as_mut()) {
                if at
                    .first()
                    .is_some_and(|ms| since.elapsed() >= Duration::from_millis(*ms))
                {
                    let ms = at.remove(0);
                    let path = dir.join(format!("world-{ms}.png"));
                    eprintln!("[world] debug: screenshot {}", path.display());
                    app.world_mut()
                        .spawn(Screenshot::primary_window())
                        .observe(save_to_disk(path));
                }
            }
            let update = Instant::now();
            if timed && draw {
                super::timing::begin();
            }
            app.update();
            if timed && draw {
                super::timing::end();
            }
            // What the CCTV room asked of real sessions this update (a kill's blast,
            // work harder's bolt): the shell does it (ADR-0013, ADR-0014). The world never
            // reaches into the OS itself.
            if let Some(mut cctv) = app.world_mut().get_resource_mut::<super::feeds::Cctv>() {
                for request in cctv.0.session_requests() {
                    reports.session_request(request);
                }
            }
            // The seat and the hints, for the page's overlay, top bar and hint pill.
            let seated = std::mem::take(&mut app.world_mut().resource_mut::<SeatOutbox>().0);
            for event in seated {
                match event {
                    FromWorld::Seat(seat) => reports.seat(seat),
                    FromWorld::Hint(hint) => reports.hint(hint),
                }
            }
            if recorded.is_some() {
                let eye = app.world().resource::<Whereabouts>().eye;
                eprintln!(
                    "[world] debug: frame {ticks:06} drawn from eye ({:.3}, {:.3}, {:.3}) in {}, live doorways {}",
                    eye.x,
                    eye.y,
                    eye.z,
                    app.world().resource::<Space>().describe(),
                    app.world().resource::<Live>().count()
                );
            }
            ticks += 1;
            if draw {
                feed_draws += app.world().resource::<super::feeds::FeedsDrawing>().0;
                drawn_since += 1;
            }
            if draw && ready.is_some() {
                if let Some(slow) = stats.drawn(update).filter(|_| slow_frames) {
                    eprintln!(
                        "[world] debug: slow frame at tick {ticks}: {:.1} ms since the last, update {:.1} ms{}",
                        slow.0,
                        slow.1,
                        if timed {
                            format!(" ({})", super::timing::last_frame())
                        } else {
                            String::new()
                        }
                    );
                }
            }

            if !loaded {
                match app.world().resource::<Loading>() {
                    Loading::Pending => {}
                    Loading::Done => {
                        loaded = true;
                        pieces_in.0.store(true, Ordering::Relaxed);
                        eprintln!(
                            "[world] pieces loaded and spawned after {ticks} ticks, {} ms",
                            began.elapsed().as_millis()
                        );
                    }
                    Loading::Failed(problem) => {
                        // The controller then releases the world, which stops this loop.
                        loaded = true;
                        eprintln!("[world] the world's assets failed to load: {problem}");
                        reports.failed(StartFailure::AssetsFailed);
                    }
                }
            }

            let frames = drawn.0.load(Ordering::Relaxed);
            if ready.is_none() && frames > 0 {
                ready = Some(Instant::now());
                eprintln!(
                    "[world] first frame drawn after {ticks} ticks, {} ms",
                    began.elapsed().as_millis()
                );
                reports.ready();
                app.world_mut().insert_resource(FirstFrameDrawn);
            }
            // The load's decoded files and textures are freed by now; hand the memory
            // they leave back to the system, which has little to spare (ticket 27).
            if !trimmed && ready.is_some_and(|since| since.elapsed() >= TRIM_AFTER) {
                trimmed = true;
                let (before, after) = give_back_memory();
                eprintln!("[world] memory given back: resident {before} kB before, {after} kB after");
            }
            if let (Some(debug::Force::PanicAfterReady(after)), Some(since)) = (force, ready) {
                if since.elapsed() >= after {
                    panic!("forced panic {after:?} after the first frame (debug build)");
                }
            }
            if last_log.elapsed() >= LOG_EVERY {
                let look = app.world().resource::<Look>();
                let player = app.world().resource::<Whereabouts>();
                eprintln!(
                    "[world] tick {ticks}, frames drawn {frames}, yaw {:.2} pitch {:.2}, eye ({:.2}, {:.2}, {:.2}) in {}",
                    look.yaw,
                    look.pitch,
                    player.eye.x,
                    player.eye.y,
                    player.eye.z,
                    app.world().resource::<Space>().describe()
                );
                if let Some(line) = stats.take(last_log.elapsed()) {
                    let live = app.world().resource::<Live>().count();
                    let indirect = if indirect { "on" } else { "off" };
                    eprintln!(
                        "[world] {line}, live doorways {live}, feed draws {feed_draws} in {drawn_since} frames, indirect drawing {indirect}"
                    );
                    if let Some(stages) = super::timing::take().filter(|_| timed) {
                        eprintln!("[world] {stages}");
                    }
                }
                last_log = Instant::now();
                (feed_draws, drawn_since) = (0, 0);
            }
            if let Some(exit) = app.should_exit() {
                return exit;
            }
        }
    });
    app.run();
    // The runner has returned and `run` has dropped the `App`, its render world, the
    // wgpu device and the surface presenting into the subsurface.
    eprintln!("[world] world ended, render resources dropped");
}

/// A drawn frame this long after the last is logged with `WORLD_DEBUG_SLOW_FRAMES=1`.
const SLOW: Duration = Duration::from_millis(60);

/// Drawn frames since the last log: the time between them, and how long each drawing
/// update took (the render runs inside it).
#[derive(Default)]
struct FrameStats {
    last: Option<Instant>,
    intervals: Vec<f32>,
    updates: Vec<f32>,
}

impl FrameStats {
    /// A frame was drawn by the update that began at `began` and has just returned. The
    /// time since the last frame and the update's, in ms, when that is over [`SLOW`].
    fn drawn(&mut self, began: Instant) -> Option<(f32, f32)> {
        let now = Instant::now();
        let update = (now - began).as_secs_f32() * 1000.0;
        self.updates.push(update);
        let interval = self.last.map(|last| (now - last).as_secs_f32() * 1000.0);
        self.intervals.extend(interval);
        self.last = Some(now);
        interval
            .filter(|ms| *ms > SLOW.as_secs_f32() * 1000.0)
            .map(|ms| (ms, update))
    }

    /// The log line for the frames drawn over `over`, and a fresh start.
    fn take(&mut self, over: Duration) -> Option<String> {
        let count = self.updates.len();
        let interval = summary(&mut self.intervals)?;
        let update = summary(&mut self.updates)?;
        self.intervals.clear();
        self.updates.clear();
        Some(format!(
            "frame times: {:.1} fps; between frames mean {:.2} median {:.2} p95 {:.2} ms; update mean {:.2} p95 {:.2} ms",
            count as f32 / over.as_secs_f32(),
            interval.0,
            interval.1,
            interval.2,
            update.0,
            update.2
        ))
    }
}

/// Mean, median and 95th percentile.
fn summary(samples: &mut [f32]) -> Option<(f32, f32, f32)> {
    if samples.is_empty() {
        return None;
    }
    samples.sort_by(f32::total_cmp);
    let n = samples.len();
    let mean = samples.iter().sum::<f32>() / n as f32;
    Some((mean, samples[n / 2], samples[(n * 95 / 100).min(n - 1)]))
}

#[derive(PartialEq)]
enum Flow {
    Go,
    Stop,
}

/// Drains the channel into the world: the latest surface size, rectangle, agent tree,
/// settings and go to desk win (go to desk applies after the tree, to the plan it makes),
/// and keys, capture changes and focus losses pass the [`KeyGate`] in the order they
/// came. A stop, or a controller that let go of the world, ends the loop. Runs on every
/// pass, drawing or not, so a world kept warm or suspended keeps its floor plan and
/// settings current.
fn apply_messages(
    world: &mut World,
    window: Entity,
    rx: &mpsc::Receiver<Msg>,
    shown: &mut bool,
) -> Flow {
    let mut surface = None;
    let mut rect = None;
    let mut tree = None;
    let mut settings = None;
    let mut strings = None;
    let mut look = (0.0, 0.0);
    let mut desk = None;
    let mut keys: Vec<Change> = Vec::new();
    let mut clicks = 0;
    let mut right_clicks = 0;
    let mut sit_in = None;
    let mut leave = None;
    loop {
        let msg = match rx.try_recv() {
            Ok(msg) => msg,
            Err(mpsc::TryRecvError::Empty) => break,
            Err(mpsc::TryRecvError::Disconnected) => return Flow::Stop,
        };
        match msg {
            Msg::Stop => return Flow::Stop,
            Msg::Surface {
                width,
                height,
                scale,
            } => surface = Some((width, height, scale)),
            Msg::World(ToWorld::Rect(r)) => rect = Some(r),
            Msg::World(ToWorld::Shown(s)) => {
                *shown = s;
                // Back and keep warm close the seated view; the player is back where
                // they stood on the way back in.
                if !s {
                    leave = Some("the world screen went away");
                }
            }
            Msg::World(ToWorld::Look { dx, dy }) => {
                look.0 += dx;
                look.1 += dy;
            }
            Msg::World(ToWorld::Tree(t)) => tree = Some(t),
            Msg::World(ToWorld::Settings(s)) => settings = Some(s),
            Msg::World(ToWorld::Strings(s)) => strings = Some(s),
            Msg::World(ToWorld::GoToDesk {
                session_id,
                agent_id,
            }) => {
                desk = Some(DeskId {
                    session: session_id,
                    agent: agent_id,
                })
            }

            Msg::World(ToWorld::Captured(captured)) => {
                let mut gate = world.resource_mut::<KeyGate>();
                keys.extend(gate.captured(captured));
            }
            Msg::World(ToWorld::Key { code, pressed }) => {
                keys.extend(world.resource_mut::<KeyGate>().key(code, pressed));
            }
            Msg::World(ToWorld::FocusLost) => {
                keys.extend(world.resource_mut::<KeyGate>().focus_lost());
            }
            Msg::World(ToWorld::Click(Button::Primary)) => clicks += 1,
            Msg::World(ToWorld::Click(Button::Secondary)) => right_clicks += 1,
            Msg::World(ToWorld::SitIn { session_id }) => sit_in = Some(session_id),
            Msg::World(ToWorld::Leave) => leave = Some("Leave"),
        }
    }
    for (key_code, state) in keys {
        world.write_message(KeyboardInput {
            key_code,
            logical_key: Key::Unidentified(NativeKey::Unidentified),
            state,
            text: None,
            repeat: false,
            window,
        });
    }
    if let Some((width, height, scale)) = surface {
        let mut win = world.get_mut::<Window>(window).expect("primary window");
        let res = &mut win.resolution;
        if res.physical_width() != width
            || res.physical_height() != height
            || res.scale_factor() != scale
        {
            res.set_scale_factor(scale);
            res.set_physical_resolution(width.max(1), height.max(1));
            let (w, h) = (res.width(), res.height());
            world.write_message(WindowResized {
                window,
                width: w,
                height: h,
            });
        }
    }
    if let Some(rect) = rect {
        world.resource_mut::<WorldRect>().0 = Some(rect);
    }
    // Settings first, so a sensitivity sent with this drain turns its look deltas.
    if let Some(settings) = settings {
        rooms::apply_settings(world, settings);
    }
    if let Some(strings) = strings {
        let count = strings.len();
        let changed = world
            .resource_mut::<WorldStrings>()
            .set_if_neq(WorldStrings(strings));
        if changed {
            eprintln!("[world] strings: {count} from the page");
        }
    }
    if look != (0.0, 0.0) {
        let per_pixel = world.resource::<Controls>().radians_per_pixel();
        world.resource_mut::<Look>().turn(look.0, look.1, per_pixel);
    }
    if let Some(tree) = tree {
        corridor::apply_tree(world, &tree);
    }
    if let Some(why) = leave {
        seat::leave(world, why);
    }
    if let Some(desk) = desk {
        // Going to a desk while seated leaves the seat first.
        seat::leave(world, "go to desk");
        go_to_desk::go_to_desk(world, &desk);
    }
    // After the tree, so the seat is found in the newest plan; after the look, so a
    // right-click aims where the view turned to with it.
    if let Some(session) = sit_in {
        let root = DeskId {
            agent: session.clone(),
            session,
        };
        seat::sit_in(world, &root);
    }
    for _ in 0..right_clicks {
        seat::right_click(world);
    }
    // After the look, so a click aims where the view turned to with it.
    if clicks > 0 {
        world.resource_mut::<aim::Clicks>().0 += clicks;
    }
    Flow::Go
}

/// Turns the camera to the current look whenever it changes.
fn apply_look(
    look: Res<Look>,
    mut cameras: Query<&mut Transform, (With<Camera3d>, Without<PortalCamera>)>,
) {
    if !look.is_changed() {
        return;
    }
    for mut transform in &mut cameras {
        transform.rotation = Quat::from_euler(EulerRot::YXZ, look.yaw, look.pitch, 0.0);
    }
}

/// Turns the world's drawing on or off. A camera that is not active is not rendered, and
/// a window no active camera targets is neither acquired nor presented: the update
/// simulates without touching the surface. Portal cameras follow the main camera's state
/// (`portals`), so each doorway keeps its own.
fn set_drawing(world: &mut World, draw: bool) {
    let mut cameras = world.query_filtered::<&mut Camera, (
        Or<(With<Camera3d>, With<Presenter>)>,
        Without<PortalCamera>,
    )>();
    for mut camera in cameras.iter_mut(world) {
        if camera.is_active != draw {
            camera.is_active = draw;
        }
    }
}

/// How long after the first frame the memory the load freed is given back: by then every
/// texture has reached the GPU and dropped the pixels the world no longer keeps.
const TRIM_AFTER: Duration = Duration::from_secs(3);

/// Returns the heap's free memory to the system (glibc keeps it otherwise), with this
/// process's resident size in kB before and after.
fn give_back_memory() -> (u64, u64) {
    let resident = || {
        std::fs::read_to_string("/proc/self/status")
            .ok()
            .and_then(|status| {
                let line = status.lines().find(|l| l.starts_with("VmRSS:"))?;
                line.split_whitespace().nth(1)?.parse().ok()
            })
            .unwrap_or(0)
    };
    let before = resident();
    // Safety: malloc_trim only walks glibc's own arenas; it is thread-safe.
    unsafe {
        libc::malloc_trim(0);
    }
    (before, resident())
}

fn sleep_until(deadline: Instant) {
    let now = Instant::now();
    if deadline > now {
        thread::sleep(deadline - now);
    }
}

/// Forced failures for the acceptance demonstration. They exist only in debug builds
/// (`debug_assertions`): a release build reads no switch and forces nothing.
pub(super) mod debug {
    use std::time::Duration;

    #[derive(Clone, Copy, Debug, PartialEq)]
    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    pub enum Force {
        /// The start fails before the world attaches.
        FailStart,
        /// The world thread panics while starting.
        PanicStart,
        /// The world thread panics this long after its first frame.
        PanicAfterReady(Duration),
    }

    /// The failure forced on launched world `world`, if any. Only the first world of the
    /// process is forced, so **Try again** shows a healthy fresh start.
    /// `WORLD_DEBUG_FAIL_START=fail|panic` fails or panics the start;
    /// `WORLD_DEBUG_PANIC_AFTER_READY_MS=N` panics N ms after the first frame.
    #[cfg(debug_assertions)]
    pub fn forced(world: u64) -> Option<Force> {
        if world != 1 {
            return None;
        }
        match std::env::var("WORLD_DEBUG_FAIL_START").as_deref() {
            Ok("fail") => return Some(Force::FailStart),
            Ok("panic") => return Some(Force::PanicStart),
            _ => {}
        }
        let after = std::env::var("WORLD_DEBUG_PANIC_AFTER_READY_MS").ok()?;
        Some(Force::PanicAfterReady(Duration::from_millis(
            after.parse().ok()?,
        )))
    }

    #[cfg(not(debug_assertions))]
    pub fn forced(_world: u64) -> Option<Force> {
        None
    }

    /// Where the camera stands instead of the corridor, for checking a room before the
    /// player can walk in: `WORLD_DEBUG_IN_ROOM`, as [`InRoom::parse`] reads it.
    ///
    /// [`InRoom::parse`]: super::super::rooms::InRoom::parse
    #[cfg(debug_assertions)]
    pub fn in_room() -> Option<super::super::rooms::InRoom> {
        let switch = std::env::var("WORLD_DEBUG_IN_ROOM").ok()?;
        let in_room = super::super::rooms::InRoom::parse(&switch);
        if in_room.is_none() {
            eprintln!("[world] debug: WORLD_DEBUG_IN_ROOM={switch:?} is not <room>[@x,z|@x,y,z]");
        }
        in_room
    }

    #[cfg(not(debug_assertions))]
    pub fn in_room() -> Option<super::super::rooms::InRoom> {
        None
    }

    /// Where the camera starts in the corridor instead of the start eye:
    /// `WORLD_DEBUG_EYE=<x>,<z>` (eye height) or `<x>,<y>,<z>`.
    #[cfg(debug_assertions)]
    pub fn eye() -> Option<bevy::math::Vec3> {
        let switch = std::env::var("WORLD_DEBUG_EYE").ok()?;
        let eye = parse_eye(&switch);
        if eye.is_none() {
            eprintln!("[world] debug: WORLD_DEBUG_EYE={switch:?} is not <x>,<z> or <x>,<y>,<z>");
        }
        eye
    }

    #[cfg(not(debug_assertions))]
    pub fn eye() -> Option<bevy::math::Vec3> {
        None
    }

    /// Reads `<x>,<z>` (at eye height) or `<x>,<y>,<z>`.
    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    pub fn parse_eye(switch: &str) -> Option<bevy::math::Vec3> {
        let numbers: Vec<f32> = switch
            .split(',')
            .map(|n| n.trim().parse().ok())
            .collect::<Option<_>>()?;
        match numbers[..] {
            [x, z] => Some(bevy::math::Vec3::new(
                x,
                super::super::floor_plan::layout::EYE_HEIGHT,
                z,
            )),
            [x, y, z] => Some(bevy::math::Vec3::new(x, y, z)),
            _ => None,
        }
    }

    /// For measuring how long frames really take: `WORLD_DEBUG_UNPACED=1` draws every
    /// pass without waiting for frame callbacks, with no vsync.
    pub fn unpaced() -> bool {
        cfg!(debug_assertions) && std::env::var("WORLD_DEBUG_UNPACED").is_ok_and(|v| v == "1")
    }

    /// `WORLD_DEBUG_PACING=strict` draws the way the world did before ticket 27: FIFO,
    /// and each frame only once the last one's frame callback has come.
    pub fn strict_pacing() -> bool {
        cfg!(debug_assertions) && std::env::var("WORLD_DEBUG_PACING").is_ok_and(|v| v == "strict")
    }

    /// `WORLD_DEBUG_IN_FLIGHT=<n>`: how many drawn frames may await their frame callback
    /// at once, instead of `surface::IN_FLIGHT`.
    pub fn in_flight() -> Option<usize> {
        if !cfg!(debug_assertions) {
            return None;
        }
        let n = std::env::var("WORLD_DEBUG_IN_FLIGHT").ok()?.parse().ok()?;
        Some(usize::clamp(n, 1, 4))
    }

    /// `WORLD_DEBUG_TIMING=1` logs where each drawn frame's time goes (`timing`).
    pub fn timing() -> bool {
        cfg!(debug_assertions) && std::env::var("WORLD_DEBUG_TIMING").is_ok_and(|v| v == "1")
    }

    /// `WORLD_DEBUG_SLOW_FRAMES=1` logs every drawn frame that came over 60 ms after the
    /// last one.
    pub fn slow_frames() -> bool {
        cfg!(debug_assertions) && std::env::var("WORLD_DEBUG_SLOW_FRAMES").is_ok_and(|v| v == "1")
    }

    /// `WORLD_DEBUG_INDIRECT=1` leaves Bevy's GPU-driven indirect drawing on.
    pub fn indirect() -> bool {
        cfg!(debug_assertions) && std::env::var("WORLD_DEBUG_INDIRECT").is_ok_and(|v| v == "1")
    }

    /// `WORLD_DEBUG_ROOM_SHADOWS=on|off` overrides whether the player's room has shadows.
    pub fn room_shadows() -> Option<bool> {
        if !cfg!(debug_assertions) {
            return None;
        }
        match std::env::var("WORLD_DEBUG_ROOM_SHADOWS").as_deref() {
            Ok("on") => Some(true),
            Ok("off") => Some(false),
            _ => None,
        }
    }

    /// `WORLD_DEBUG_SHOTS=<dir>@<ms>,<ms>,...` saves the world's surface as
    /// `<dir>/world-<ms>.png` that long after the first frame.
    pub fn shots() -> Option<(std::path::PathBuf, Vec<u64>)> {
        if !cfg!(debug_assertions) {
            return None;
        }
        let switch = std::env::var("WORLD_DEBUG_SHOTS").ok()?;
        let (dir, at) = switch.rsplit_once('@')?;
        let mut at: Vec<u64> = at
            .split(',')
            .filter_map(|ms| ms.trim().parse().ok())
            .collect();
        at.sort_unstable();
        Some((dir.into(), at))
    }

    /// `WORLD_DEBUG_RECORD=<dir>` saves every frame drawn while a key is held as
    /// `<dir>/frame-<tick>.png`, and logs the eye and space it was drawn from.
    pub fn record() -> Option<std::path::PathBuf> {
        if !cfg!(debug_assertions) {
            return None;
        }
        std::env::var_os("WORLD_DEBUG_RECORD").map(Into::into)
    }

    /// Whether to log every key GTK hands the world, for checking GTK's quirks:
    /// `WORLD_DEBUG_KEYS=1`.
    #[cfg(debug_assertions)]
    pub fn log_keys() -> bool {
        std::env::var("WORLD_DEBUG_KEYS").is_ok_and(|on| on == "1")
    }

    #[cfg(not(debug_assertions))]
    pub fn log_keys() -> bool {
        false
    }

    /// The start timeout: about 15 seconds, or `WORLD_DEBUG_START_TIMEOUT_MS` in a debug
    /// build (every start).
    pub fn start_timeout() -> Duration {
        #[cfg(debug_assertions)]
        if let Some(ms) = std::env::var("WORLD_DEBUG_START_TIMEOUT_MS")
            .ok()
            .and_then(|ms| ms.parse().ok())
        {
            return Duration::from_millis(ms);
        }
        super::super::controller::START_TIMEOUT
    }

    #[cfg(test)]
    mod tests {
        #[test]
        fn the_debug_eye_reads_x_z_or_x_y_z() {
            let at = |s: &str| super::parse_eye(s).map(|v| v.to_array());
            assert_eq!(at("3.75,1.2"), Some([3.75, 1.6, 1.2]));
            assert_eq!(at("1, 2.5 ,0.4"), Some([1.0, 2.5, 0.4]));
            assert_eq!(at("1"), None);
            assert_eq!(at("a,b"), None);
        }
    }
}
