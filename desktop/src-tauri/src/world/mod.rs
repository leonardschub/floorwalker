//! The world: Bevy drawing the corridor under the transparent webview (spec: "The world
//! contract", "The shell's world controller", "The real world launcher"; ADR-0009).
//!
//! - `controller`: the world's lifecycle, behind the narrow [`controller::Launcher`]
//!   seam; tested with a stand-in world, no GPU.
//! - `engine`: the real launcher, Bevy on a `bevy-world` thread.
//! - `surface`: the Wayland subsurface below the GTK toplevel that Bevy presents into.
//! - `floor_plan`: the world's rules, pure and free of Bevy: what the agent tree
//!   shows, and where.
//! - `go_to_desk`: the camera's jump beside an agent's desk, from the side panel.
//! - `viewport`, `pieces`, `corridor`, `rooms`: the camera viewport from the page's
//!   rectangle, the generated pieces (`desktop/world-assets`), and the corridor and the
//!   rooms' interiors with their desks, built from them to the floor plan.
//! - `keyboard`, `keys`, `walk`, `collision`: the main window's keys caught from GTK,
//!   the ones the world walks with while the cursor is captured, walking, and what
//!   blocks it.
//! - `portals`: each doorway as a two-way render-to-texture portal (ADR-0011): which one
//!   is live, its camera through the doorway, and which rooms keep their lights on.
//! - `surfaces`, `clutter`: the Textures and Clutter settings (ticket 24): every
//!   surface's textured, flat or dimmed (the console's dark controls) material, and the
//!   rooms' clutter and haze.
//! - `names`, pure, and `lettering`: what the signs and the robots' faces say (a model's
//!   company, names fitted to their plates), and drawing it as 2D text into images.
//! - `strings`: the world's fixed text, translated by the page and sent whole.
//! - `clock`: the world's clock, read once per frame for everything that tells time.
//! - `cctv`: the CCTV room behind the corridor's far wall.
//! - `cctv_state`, pure: everything the CCTV room decides (the monitor wall and its
//!   refresh turns, the focus monitors, the ticker, the console, punishments, wrecks and
//!   flag markers, and what punishments ask of real sessions, which the world hands to
//!   the shell through its reports: ADR-0013, ADR-0014); `cctv_geometry`, pure: where
//!   its screens hang and where a focus camera stands.
//! - `feeds`: the project rooms' live camera feeds on the monitor wall; `focus`: the
//!   focus monitors' close-ups of flagged agents and the ticker; `console`: the
//!   console's screen, buttons and flip cover.
//! - `aim`: the centre dot and clicks; `picking`, pure: which clickable thing a click's ray
//!   through the dot hits.
//! - `seating`, pure, and `seat`: sitting in an attachable root agent by right-click or
//!   by session (the side panel, New session), the seated view's camera and glass, and leaving.
//! - `marker`, pure, and `flag_markers`: which robots carry a flag marker, and drawing
//!   the corner brackets on their heads.
//! - `punishments` and `squad`, pure: work harder's and kill's effects in the project
//!   room, and the kill squad's path to the desk; `wrecks`: the wreck a kill leaves.
//!
//! The commands below are the world contract as the page sees it (`web/src/world`).
//! Each is declared in `build.rs`'s app manifest and granted only to `main`.

mod aim;
mod cctv;
mod cctv_geometry;
mod cctv_state;
mod clock;
mod clutter;
mod collision;
mod console;
mod controller;
mod controls;
mod corridor;
mod engine;
mod feeds;
mod flag_markers;
mod floor_plan;
mod focus;
mod go_to_desk;
mod keyboard;
mod keys;
mod lettering;
mod live_budget;
mod look;
mod marker;
mod mipmaps;
mod names;
mod pc_screens;
mod picking;
mod pieces;
mod pointer_lock;
mod portals;
mod punishments;
mod quality;
mod rooms;
mod seat;
mod seating;
mod squad;
mod strings;
mod surface;
mod surfaces;
mod timing;
mod viewport;
mod walk;
mod wrecks;

use tauri::{AppHandle, Emitter, Manager, State};

pub use cctv_state::{SessionAction, SessionRequest};
use controller::{Button, Close, FromWorld, Rect, StartFailure, Status, WorldSettings};
pub use controller::{Controller, SeatReport};
pub use keyboard::catch_on_main as catch_keys;
pub use pc_screens::{Screen, SessionScreens};
pub use pointer_lock::allow_on_main as allow_pointer_lock;

/// The event that carries every status change to the main window.
const STATUS_EVENT: &str = "world-status";

/// The event that asks the main window's page to confirm quitting.
const QUIT_EVENT: &str = "world-quit-requested";

/// The event that carries the seat, `{ sessionId, rect }` or null, to the main window
/// (agent-screens spec: "Controller and commands").
const SEAT_EVENT: &str = "world-seat";

/// The event that carries a sit-in's hint, `"terminal"` or `"nothing"`.
const HINT_EVENT: &str = "world-hint";

/// The world controller with the real launcher. Nothing starts until the page asks.
pub fn controller(app: &AppHandle) -> Controller {
    let events = app.clone();
    let agents = app.clone();
    let page = app.clone();
    Controller::new(
        engine::BevyLauncher::new(app.clone()),
        engine::debug::start_timeout(),
        move |status| {
            let _ = events.emit_to("main", STATUS_EVENT, status);
        },
        // The CCTV room asked something of a real session: the agents side does it.
        move |request| match agents.try_state::<crate::agents::Agents>() {
            Some(registry) => registry.session_request(request),
            None => eprintln!(
                "[agents] {:?} on session {}: no registry, nothing done",
                request.action, request.session
            ),
        },
        // The seat goes to the page (the overlay, the top bar, the panel's row) and to
        // the session terminals (which attach the overlay streams); a hint to the page.
        move |event| match event {
            FromWorld::Seat(seat) => {
                if let Some(terminals) = page.try_state::<crate::terminals::SessionTerminals>() {
                    terminals.seated(seat.as_ref());
                }
                let _ = page.emit_to("main", SEAT_EVENT, seat);
            }
            FromWorld::Hint(hint) => {
                let _ = page.emit_to("main", HINT_EVENT, hint);
            }
        },
    )
}

/// **start**: starts the world, joins a start in progress, or answers at once while a
/// world runs. Settles when the first frame has been drawn.
/// Entering the world also starts the agent registry's first build, if the panel has
/// not yet, so the corridor fills without the panel.
#[tauri::command]
pub async fn world_start(app: AppHandle, world: State<'_, Controller>) -> Result<(), StartFailure> {
    app.state::<crate::agents::Agents>().start_first_build(&app);
    world.start().await.unwrap_or(Err(StartFailure::Gone))
}

/// **stop**: stops the world completely: cancels a start in progress or ends a running
/// world, in the controller's stop order. Idempotent. Settles once the world thread has
/// been joined and its surface destroyed; the join blocks, so it runs off the async
/// runtime's workers.
#[tauri::command]
pub async fn world_stop(app: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || app.state::<Controller>().stop())
        .await
        .map_err(|_| "world-stop-failed".to_string())
}

/// **status**: the world's current state.
#[tauri::command]
pub fn world_status(world: State<'_, Controller>) -> Status {
    world.status()
}

/// The world view's rectangle as the page measures it: physical pixels relative to the
/// webview's top-left.
#[derive(serde::Deserialize)]
pub struct ReportedRect {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

/// **rect**: the world view's rectangle.
#[tauri::command]
pub fn world_rect(world: State<'_, Controller>, rect: ReportedRect) {
    let pixels = |value: f64| value.max(0.0).round() as u32;
    world.rect(Rect {
        x: pixels(rect.x),
        y: pixels(rect.y),
        w: pixels(rect.w),
        h: pixels(rect.h),
    });
}

/// **look**: look deltas while the cursor is captured, summed per animation frame.
#[tauri::command]
pub fn world_look(world: State<'_, Controller>, dx: f32, dy: f32) {
    if dx.is_finite() && dy.is_finite() {
        world.look(dx, dy);
    }
}

/// **click**: a button press while the cursor is captured, not the click that captured
/// it (management-rooms spec: "Clicks reach the world"). The world picks what the
/// middle of the view aims at: the primary button works the CCTV console, the secondary
/// (`button: "secondary"`: sit in) sits in the robot within reach. Without a
/// button it is the primary.
#[tauri::command]
pub fn world_click(world: State<'_, Controller>, button: Option<String>) {
    world.click(match button.as_deref() {
        Some("secondary") => Button::Secondary,
        _ => Button::Primary,
    });
}

/// **sit in**: sit in a session's root agent from anywhere (agent-screens spec: "The
/// side panel"): the camera goes to its desk and straight into the seat, from seat to
/// seat while seated. A terminal-started session gets the *terminal* hint instead
/// (`world-hint`). Only a world that exists hears it; it is never replayed to a world
/// launched later. The side panel's right-click sends it; so can New session and
/// anything else that names a session.
#[tauri::command]
pub fn world_sit_in(world: State<'_, Controller>, session_id: String) {
    world.sit_in(session_id);
}

/// **leave**: leave the seated view (Leave in the top bar, or a right-click on the
/// world view while seated). Nothing while not seated.
#[tauri::command]
pub fn world_leave(world: State<'_, Controller>) {
    world.leave();
}

/// The Subagent size slider's range on Settings (spec: "World settings").
const SUBAGENT_SCALE: std::ops::RangeInclusive<f32> = 0.5..=1.0;

/// **settings**: the world options from Settings, sent whole when the world starts and
/// whenever one changes. A size outside the slider's range is clamped to it, and a size
/// that is not a number is ignored. The walking speed and mouse sensitivity (ticket 28)
/// are clamped to their sliders' ranges; one that is not a number, or missing (a page
/// from before ticket 28), keeps its default. Punishment effects are on when
/// missing.
#[tauri::command]
pub fn world_settings(
    world: State<'_, Controller>,
    subagent_scale: f32,
    textures: bool,
    clutter: bool,
    punishment_effects: Option<bool>,
    walk_speed: Option<f32>,
    look_sensitivity: Option<f32>,
) {
    if !subagent_scale.is_finite() {
        return;
    }
    world.settings(WorldSettings {
        subagent_scale: subagent_scale.clamp(*SUBAGENT_SCALE.start(), *SUBAGENT_SCALE.end()),
        textures,
        clutter,
        punishment_effects: punishment_effects.unwrap_or(true),
        controls: controls::Controls::new(
            walk_speed.unwrap_or(f32::NAN),
            look_sensitivity.unwrap_or(f32::NAN),
        ),
    });
}

/// **strings**: the world's fixed text in the page's language, the whole table, sent
/// before the page first starts a world and whenever its language changes.
#[tauri::command]
pub fn world_strings(world: State<'_, Controller>, strings: controller::Strings) {
    world.strings(std::sync::Arc::new(strings));
}

/// **go to desk**: the side panel's press on an agent. The player jumps beside its
/// desk, or nothing changes when it has none; kept while the world starts.
#[tauri::command]
pub fn world_go_to_desk(world: State<'_, Controller>, session_id: String, agent_id: String) {
    world.go_to_desk(session_id, agent_id);
}

/// **captured**: whether the cursor is captured, on every Pointer Lock change and error.
/// The world walks only while it is, and lets go of every held key when it is not.
#[tauri::command]
pub fn world_captured(world: State<'_, Controller>, captured: bool) {
    world.captured(captured);
}

/// **shown**: whether the world screen shows the world. The world draws only while it
/// does; otherwise it keeps simulating without drawing.
#[tauri::command]
pub fn world_shown(world: State<'_, Controller>, shown: bool) {
    world.shown(shown);
}

/// A close request of the main window (title-bar close, the compositor's close action,
/// keyboard close). While a world exists the close is held and the page asked to
/// confirm quitting; otherwise it goes ahead at once (spec: "Quit prompt").
pub fn on_close_requested(window: &tauri::Window, api: &tauri::CloseRequestApi) {
    if window.label() != "main" {
        return;
    }
    match window.app_handle().state::<Controller>().close_requested() {
        Close::Now => {}
        Close::Ask => {
            eprintln!("[world] close held: asking the page to confirm quitting");
            api.prevent_close();
            if window.emit_to("main", QUIT_EVENT, ()).is_err() {
                // No page to ask: nothing would ever answer, so quit as asked.
                quit(window.app_handle().clone(), true);
            }
        }
        Close::Hold => {
            eprintln!("[world] close held: the page is already asking");
            api.prevent_close();
        }
    }
}

/// The main window's page starts loading: a prompt the old page showed can no longer
/// be answered, so the next close asks the new page.
pub fn on_page_loading(app: &AppHandle) {
    if app.state::<Controller>().page_loading() {
        eprintln!("[world] the page reloaded while asking: the next close asks the new page");
    }
}

/// **quit confirmation**: the page's answer to the quit prompt. Cancel changes nothing;
/// confirm stops the world in the controller's stop order, then exits the app.
#[tauri::command]
pub fn world_quit(app: AppHandle, confirmed: bool) {
    quit(app, confirmed);
}

/// The stop runs off the main thread: the real stop destroys the world's surface on the
/// main thread, so stopping from it would wait out the controller's stop backstop.
fn quit(app: AppHandle, confirmed: bool) {
    std::thread::spawn(move || {
        if app.state::<Controller>().quit_answered(confirmed) {
            eprintln!("[world] quit confirmed: the world stopped, exiting");
            app.exit(0);
        }
    });
}
