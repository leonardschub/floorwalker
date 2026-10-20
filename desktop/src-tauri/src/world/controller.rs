//! The shell's world controller: the one owner of the world's lifecycle (spec: "The
//! shell's world controller").
//!
//! It knows the world only through [`Launcher`], so the tests below drive it with a
//! stand-in world: no GPU, no Wayland, no Bevy. Nothing here starts at app launch; the
//! first [`Controller::start`] launches the world.

use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::Duration;

use agents::AgentTreeSnapshot;
use tokio::sync::oneshot;

use super::cctv_state::SessionRequest;
use super::controls::Controls;
use super::keys;
pub use super::seating::Hint;
pub use super::strings::Strings;

/// The world's state as the page reads it (spec: the world contract's **status**).
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    None,
    Starting,
    Running,
    /// The last start did not reach a first frame: the world failed or panicked while
    /// starting, or the start timed out.
    Failed,
    /// The world crashed after it was running. Kept until a fresh start or a stop.
    Stopped,
}

/// The world view's rectangle in physical pixels, relative to the webview's top-left
/// (spec: "Rectangle reporting").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// What the controller passes on to a launched world.
#[derive(Clone, Debug, PartialEq)]
pub enum ToWorld {
    Rect(Rect),
    /// Whether the world screen shows the world. The world draws only while it does,
    /// and simulates either way (spec: "Drawing only while shown").
    Shown(bool),
    /// Look deltas while the cursor is captured: mouse movement summed over one
    /// animation frame of the page.
    Look {
        dx: f32,
        dy: f32,
    },
    /// The whole agent tree, each time the registry pushes it (spec: "Data: from the
    /// registry to the world"). Shared, so the page's copy and the world's are one.
    Tree(Arc<AgentTreeSnapshot>),
    /// Go to desk: put the player beside this agent's desk, turned to its robot (spec:
    /// "Go to desk"). An agent with no desk changes nothing.
    GoToDesk {
        session_id: String,
        agent_id: String,
    },
    /// Whether the cursor is captured, as the page's Pointer Lock reports it (spec:
    /// "Keyboard input"). The world takes keys only while it is.
    Captured(bool),
    /// A key press or release on the main window, by physical key: GDK's
    /// `hardware_keycode`. The world names the keys it uses itself (`keys`).
    Key {
        code: u16,
        pressed: bool,
    },
    /// The main window lost focus: no releases come for the keys held now.
    FocusLost,
    /// A button press while the cursor is captured (management-rooms spec: "Clicks
    /// reach the world"); not the click that captured it. The world picks what the
    /// middle of the view aims at: a primary click works the CCTV console, a secondary
    /// one sits in the robot aimed at (agent-screens spec: "Right-click and picking").
    Click(Button),
    /// Sit in this session's root agent from anywhere (the side panel, New session):
    /// the player goes to its desk and straight into the seat, or gets its hint.
    #[cfg_attr(not(test), allow(dead_code))]
    SitIn {
        session_id: String,
    },
    /// Leave the seat: a right-click on the world view while seated, or Leave.
    Leave,
    /// The world options from Settings, whole, each time one changes (spec: "World
    /// settings").
    Settings(WorldSettings),
    /// The world's fixed text in the page's language, the whole table, before the first
    /// start and on each language change (management-rooms spec: "The console").
    Strings(Arc<Strings>),
}

/// Which mouse button a click in the world was.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Primary,
    Secondary,
}

/// The seated view's seat, as the world reports it (agent-screens spec: "The seated
/// view"): the session sat in and where its PC's glass is in the world view, for the
/// page's overlay. Reported once the camera settles and again whenever the rectangle
/// changes.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SeatReport {
    pub session_id: String,
    /// The session's short id: which attach the shell streams to the overlay. The page
    /// has no use for it.
    #[serde(skip)]
    pub short_id: String,
    pub rect: SeatRect,
}

/// The glass's rectangle in physical pixels relative to the webview's top-left, like
/// the world view's own ([`Rect`]), but to the fraction of a pixel.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize)]
pub struct SeatRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// What the world tells the page through the shell, besides its status.
#[derive(Clone, Debug, PartialEq)]
pub enum FromWorld {
    /// The seat, or none once the player has left it (or the world has ended).
    Seat(Option<SeatReport>),
    /// A sit-in that can't be: the hint to show under the centre dot.
    Hint(Hint),
}

/// The world options on Settings (spec: "World settings"). The page keeps them on the
/// device and sends them whole; the world never reads storage.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorldSettings {
    /// How much smaller each level of subagent's robot is (the Subagent size slider).
    pub subagent_scale: f32,
    pub textures: bool,
    pub clutter: bool,
    /// The Punishment effects switch: off, punishments
    /// play without their squad, bolt, flash or blast, on the same timings.
    pub punishment_effects: bool,
    /// The Walking speed and Mouse sensitivity sliders (ticket 28).
    pub controls: Controls,
}

/// What to do with a close request of the main window (spec: "Quit prompt").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Close {
    /// No world exists: close at once.
    Now,
    /// A world exists: hold the close and ask the page to confirm quitting.
    Ask,
    /// The page is already asking: hold the close and ask nothing more.
    Hold,
}

/// How long a stop waits for the world to be gone before letting it go.
#[cfg(not(test))]
const STOP_WAIT: Duration = Duration::from_secs(2);
#[cfg(test)]
const STOP_WAIT: Duration = Duration::from_millis(200);

/// How long a start may take to its first frame before it becomes *failed*.
pub const START_TIMEOUT: Duration = Duration::from_secs(15);

/// What a start settles to: `Ok` once the first frame is drawn, or why it did not.
pub type Outcome = Result<(), StartFailure>;

/// Why a start did not reach a first frame. The page sees it as a short token
/// (`web/src/world/client.ts`'s `StartFailure`), which the world log prints too.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartFailure {
    /// A stop cancelled the start. Not a failure: the status reads *none*.
    Cancelled,
    NoWindow,
    NoWebview,
    /// Attaching the world's surface failed, including the non-Wayland check.
    AttachFailed,
    ThreadFailed,
    /// A debug build's forced start failure.
    Forced,
    Panicked,
    /// No first frame within the start timeout.
    Timeout,
    /// A generated model or texture of the world could not be loaded.
    AssetsFailed,
    /// The start was dropped without an answer.
    Gone,
}

impl StartFailure {
    pub fn token(self) -> &'static str {
        match self {
            Self::Cancelled => "world-cancelled",
            Self::NoWindow => "world-no-window",
            Self::NoWebview => "world-no-webview",
            Self::AttachFailed => "world-attach-failed",
            Self::ThreadFailed => "world-thread-failed",
            Self::Forced => "world-forced-failure",
            Self::Panicked => "world-panicked",
            Self::Timeout => "world-timeout",
            Self::AssetsFailed => "world-assets-failed",
            Self::Gone => "world-gone",
        }
    }
}

impl std::fmt::Display for StartFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.token())
    }
}

impl serde::Serialize for StartFailure {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.token())
    }
}

/// Starts worlds. The real one is Bevy on a worker thread (`engine`).
pub trait Launcher: Send + Sync + 'static {
    /// Launches a world that reports back through `reports`.
    fn launch(&self, reports: Reports) -> Box<dyn WorldLink>;
}

/// The controller's handle on one launched world.
pub trait WorldLink: Send {
    /// Passes a message on; a world that is still starting receives it once it runs.
    fn send(&self, msg: ToWorld);

    /// Ends the world, started or still starting, and returns once it is gone. The real
    /// world's stop order: tell its thread to exit, join it, then destroy its surface.
    fn stop(self: Box<Self>);
}

struct State {
    status: Status,
    /// Everyone waiting for the current start to settle.
    waiting: Vec<oneshot::Sender<Outcome>>,
    world: Option<Box<dyn WorldLink>>,
    /// Counts launched worlds. A world's reports carry its number, so a stopped world
    /// that reports late is not mistaken for the current one.
    launched: u64,
    /// The latest world view rectangle, replayed to each newly launched world.
    rect: Option<Rect>,
    /// Whether the world screen shows the world, replayed to each newly launched world.
    shown: bool,
    /// The latest agent tree, replayed to each newly launched world.
    tree: Option<Arc<AgentTreeSnapshot>>,
    /// The latest world options, replayed to each newly launched world.
    settings: Option<WorldSettings>,
    /// The latest table of the world's text, replayed to each newly launched world.
    strings: Option<Arc<Strings>>,
    /// The latest go to desk asked for while the world is starting, handed over once it
    /// is running. Never kept for a world not yet launched.
    desk: Option<ToWorld>,
    /// Whether the cursor is captured, replayed to each newly launched world and read by
    /// the GTK key handler.
    captured: bool,
    /// Whether the page is asking to confirm quitting.
    asking: bool,
    /// Whether the current world last reported a seat.
    seated: bool,
}

struct Shared {
    launcher: Box<dyn Launcher>,
    start_timeout: Duration,
    on_status: Box<dyn Fn(Status) + Send + Sync>,
    on_session: Box<dyn Fn(SessionRequest) + Send + Sync>,
    on_world: Box<dyn Fn(FromWorld) + Send + Sync>,
    state: Mutex<State>,
}

impl Shared {
    /// Sets the status and tells the page. Called with the state lock held, so status
    /// events leave in the order the changes happened.
    fn set_status(&self, state: &mut State, status: Status) {
        if state.status != status {
            state.status = status;
            (self.on_status)(status);
        }
    }

    /// The world that held a seat is gone (stopped, failed, crashed): so is the seat.
    /// Called with the state lock held, like status changes.
    fn unseat(&self, state: &mut State) {
        if std::mem::take(&mut state.seated) {
            (self.on_world)(FromWorld::Seat(None));
        }
    }
}

pub struct Controller(Arc<Shared>);

impl Controller {
    /// `on_status` hears every status change (the world contract's **status event**).
    /// A start without a first frame within `start_timeout` fails. `on_session` hears each
    /// [`SessionRequest`] the CCTV room makes of a real session, for the shell to do: a
    /// kill's blast ends it (ADR-0013), work harder's bolt sends it a prompt (ADR-0014).
    /// `on_world` hears the seat and the hints for the page ([`FromWorld`]).
    pub fn new(
        launcher: impl Launcher,
        start_timeout: Duration,
        on_status: impl Fn(Status) + Send + Sync + 'static,
        on_session: impl Fn(SessionRequest) + Send + Sync + 'static,
        on_world: impl Fn(FromWorld) + Send + Sync + 'static,
    ) -> Self {
        Self(Arc::new(Shared {
            launcher: Box::new(launcher),
            start_timeout,
            on_status: Box::new(on_status),
            on_session: Box::new(on_session),
            on_world: Box::new(on_world),
            state: Mutex::new(State {
                status: Status::None,
                waiting: Vec::new(),
                world: None,
                launched: 0,
                rect: None,
                shown: false,
                tree: None,
                settings: None,
                strings: None,
                desk: None,
                captured: false,
                asking: false,
                seated: false,
            }),
        }))
    }

    pub fn status(&self) -> Status {
        self.0.state.lock().unwrap().status
    }

    /// Starts a world if none exists. A start during a start joins it, and a start while
    /// a world is running settles at once. The receiver settles when the world's first
    /// frame is drawn.
    pub fn start(&self) -> oneshot::Receiver<Outcome> {
        let (answer, outcome) = oneshot::channel();
        let mut state = self.0.state.lock().unwrap();
        match state.status {
            Status::Running => {
                let _ = answer.send(Ok(()));
                return outcome;
            }
            Status::Starting => {
                state.waiting.push(answer);
                return outcome;
            }
            Status::None | Status::Failed | Status::Stopped => {}
        }
        state.waiting.push(answer);
        state.launched += 1;
        let launched = state.launched;
        let reports = Reports {
            shared: self.0.clone(),
            world: launched,
        };
        self.0.set_status(&mut state, Status::Starting);
        // The launcher may report back on this thread, so it never runs under the lock.
        drop(state);
        time_out(reports.clone(), self.0.start_timeout);
        let world = self.0.launcher.launch(reports);
        let mut state = self.0.state.lock().unwrap();
        // While it launched, the world may have failed, or a stop may have come.
        let current = state.launched == launched
            && matches!(state.status, Status::Starting | Status::Running);
        if !current {
            drop(state);
            release(world);
            return outcome;
        }
        if let Some(rect) = state.rect {
            world.send(ToWorld::Rect(rect));
        }
        // A world starts not shown.
        if state.shown {
            world.send(ToWorld::Shown(true));
        }
        if let Some(strings) = &state.strings {
            world.send(ToWorld::Strings(strings.clone()));
        }
        if let Some(tree) = &state.tree {
            world.send(ToWorld::Tree(tree.clone()));
        }
        if let Some(settings) = state.settings {
            world.send(ToWorld::Settings(settings));
        }
        // A first frame drawn before the link was held leaves its go to desk here.
        if state.status == Status::Running {
            if let Some(desk) = state.desk.take() {
                world.send(desk);
            }
        }

        // A world starts released.
        if state.captured {
            world.send(ToWorld::Captured(true));
        }
        state.world = Some(world);
        outcome
    }

    /// Stops the world completely (the world contract's **stop**): cancels a start in
    /// progress, whose waiters settle with [`StartFailure::Cancelled`], or ends a running world. Returns
    /// once the world is gone, or after [`STOP_WAIT`] at most: a world whose thread is
    /// stuck (a present blocked on a hidden window) is let go and finishes stopping
    /// once it returns, so a stop never hangs. Idempotent.
    pub fn stop(&self) {
        let mut state = self.0.state.lock().unwrap();
        let world = state.world.take();
        state.desk = None;
        for answer in state.waiting.drain(..) {
            let _ = answer.send(Err(StartFailure::Cancelled));
        }
        self.0.set_status(&mut state, Status::None);
        self.0.unseat(&mut state);
        // Stopping joins the world's thread, so it never runs under the lock.
        drop(state);
        let Some(world) = world else { return };
        let (done, gone) = mpsc::channel();
        let stopping = thread::Builder::new()
            .name("world-stop".into())
            .spawn(move || {
                world.stop();
                let _ = done.send(());
            });
        if stopping.is_err() {
            eprintln!("[world] could not start the stop; the world is let go");
            return;
        }
        if gone.recv_timeout(STOP_WAIT).is_err() {
            eprintln!(
                "[world] the world did not stop within {STOP_WAIT:?}; it is let go and ends once its thread returns"
            );
        }
    }

    /// Whether the world screen shows the world (the world contract's **shown**). Kept
    /// for a world not yet launched, and passed on to the current one.
    pub fn shown(&self, shown: bool) {
        let mut state = self.0.state.lock().unwrap();
        state.shown = shown;
        if let Some(world) = &state.world {
            world.send(ToWorld::Shown(shown));
        }
    }

    /// The world view's rectangle (the world contract's **rect**). Kept for a world not
    /// yet launched, and passed on to the current one.
    pub fn rect(&self, rect: Rect) {
        let mut state = self.0.state.lock().unwrap();
        state.rect = Some(rect);
        if let Some(world) = &state.world {
            world.send(ToWorld::Rect(rect));
        }
    }

    /// The agent tree, as the registry pushes it. Kept for a world not yet launched,
    /// and passed on to the current one, starting, running, kept warm or suspended
    /// alike: a world that is not drawing still applies it.
    pub fn tree(&self, tree: Arc<AgentTreeSnapshot>) {
        let mut state = self.0.state.lock().unwrap();
        if let Some(world) = &state.world {
            world.send(ToWorld::Tree(tree.clone()));
        }
        state.tree = Some(tree);
    }

    /// The world options (the world contract's **settings**). Kept for a world not yet
    /// launched, since the page sends them whether or not a world exists, and passed on
    /// to the current one, drawing or not.
    pub fn settings(&self, settings: WorldSettings) {
        let mut state = self.0.state.lock().unwrap();
        state.settings = Some(settings);
        if let Some(world) = &state.world {
            world.send(ToWorld::Settings(settings));
        }
    }
}

impl Controller {
    /// The world's text in the page's language (the world contract's **strings**). Kept
    /// for a world not yet launched, since the page sends it before the first start, and
    /// passed on to the current one.
    pub fn strings(&self, strings: Arc<Strings>) {
        let mut state = self.0.state.lock().unwrap();
        if let Some(world) = &state.world {
            world.send(ToWorld::Strings(strings.clone()));
        }
        state.strings = Some(strings);
    }

    /// A close request of the main window.
    pub fn close_requested(&self) -> Close {
        let mut state = self.0.state.lock().unwrap();
        if !matches!(state.status, Status::Starting | Status::Running) {
            return Close::Now;
        }
        if state.asking {
            return Close::Hold;
        }
        state.asking = true;
        Close::Ask
    }

    /// The main window's page starts loading (a reload, or the first load). A prompt
    /// the old page was showing can no longer be answered, so the next close asks the
    /// new page instead of being held forever. Returns whether a prompt was out.
    pub fn page_loading(&self) -> bool {
        std::mem::take(&mut self.0.state.lock().unwrap().asking)
    }

    /// The page's answer to the quit prompt (the world contract's **quit
    /// confirmation**). A cancel changes nothing. A confirm stops the world (see
    /// [`Controller::stop`]: in the stop order, and never hanging on a suspended world)
    /// and returns `true`: the app may exit now.
    pub fn quit_answered(&self, confirmed: bool) -> bool {
        self.0.state.lock().unwrap().asking = false;
        if confirmed {
            self.stop();
        }
        confirmed
    }

    /// Go to desk (the world contract's **go to desk**): passed on to a running world.
    /// While the world is starting, only the latest request is kept, and handed over
    /// once it runs; with no world, or a failed or stopped one, it is dropped.
    pub fn go_to_desk(&self, session_id: String, agent_id: String) {
        let mut state = self.0.state.lock().unwrap();
        let desk = ToWorld::GoToDesk {
            session_id,
            agent_id,
        };
        match (state.status, &state.world) {
            (Status::Running, Some(world)) => world.send(desk),
            (Status::Starting, _) => state.desk = Some(desk),
            _ => {}
        }
    }

    /// Look deltas (the world contract's **look**). Only a world that exists turns:
    /// unlike the rectangle, movement is not kept for a world launched later.
    pub fn look(&self, dx: f32, dy: f32) {
        let state = self.0.state.lock().unwrap();
        if let Some(world) = &state.world {
            world.send(ToWorld::Look { dx, dy });
        }
    }

    /// Whether the cursor is captured (the world contract's **captured**), reported on
    /// every Pointer Lock change. Kept for a world not yet launched, and passed on to the
    /// current one.
    pub fn captured(&self, captured: bool) {
        let mut state = self.0.state.lock().unwrap();
        state.captured = captured;
        if let Some(world) = &state.world {
            world.send(ToWorld::Captured(captured));
        }
    }

    /// A key press or release on the main window, by physical key. Only a world that
    /// exists hears it, and none while seated. Returns whether to keep the key from the
    /// page: a key the world walks with, while the cursor is captured and nobody sits in.
    pub fn key(&self, code: u16, pressed: bool) -> bool {
        let state = self.0.state.lock().unwrap();
        // Seated, every key is the session's (agent-screens spec: "Keys, paste and
        // links"): the world neither walks nor keeps any key from the page.
        if state.seated {
            return false;
        }
        if let Some(world) = &state.world {
            world.send(ToWorld::Key { code, pressed });
        }
        state.captured && keys::walks(code)
    }

    /// A button press while captured (the world contract's **click**). Like look, only
    /// a world that exists hears it: never kept for a world launched later.
    pub fn click(&self, button: Button) {
        let state = self.0.state.lock().unwrap();
        if let Some(world) = &state.world {
            world.send(ToWorld::Click(button));
        }
    }

    /// Sit in a session's root agent from anywhere (agent-screens spec: "Controller and
    /// commands"). Only a world that exists hears it: a sit-in is never kept for a world
    /// launched later, which it would surprise. The side panel's right-click and New
    /// session reach it through `world_sit_in`.
    pub fn sit_in(&self, session_id: String) {
        let state = self.0.state.lock().unwrap();
        if let Some(world) = &state.world {
            world.send(ToWorld::SitIn { session_id });
        }
    }

    /// Leave the seat (the page's Leave and right-click, or the seated attach leaving
    /// its session). Nothing while the world reports no seat.
    pub fn leave(&self) {
        let state = self.0.state.lock().unwrap();
        if let (true, Some(world)) = (state.seated, &state.world) {
            world.send(ToWorld::Leave);
        }
    }

    /// The main window lost focus. Pointer Lock ends with it, so the cursor counts as
    /// released at once, before the page reports it.
    pub fn focus_lost(&self) {
        let mut state = self.0.state.lock().unwrap();
        state.captured = false;
        if let Some(world) = &state.world {
            world.send(ToWorld::FocusLost);
        }
    }
}

/// How a launched world reports back to the controller.
#[derive(Clone)]
pub struct Reports {
    shared: Arc<Shared>,
    /// Which launched world these reports come from.
    world: u64,
}

impl Reports {
    /// The world could not start, for `why`, which the start rejects with. The world
    /// becomes *failed* and is released. Only a starting world fails.
    pub fn failed(&self, why: StartFailure) {
        let mut state = self.shared.state.lock().unwrap();
        if state.launched != self.world || state.status != Status::Starting {
            return;
        }
        eprintln!("[world] world {} failed to start: {why}", self.world);
        let world = state.world.take();
        state.desk = None;
        for answer in state.waiting.drain(..) {
            let _ = answer.send(Err(why));
        }
        self.shared.set_status(&mut state, Status::Failed);
        self.shared.unseat(&mut state);
        drop(state);
        if let Some(world) = world {
            release(world);
        }
    }

    /// The world thread panicked. While starting the world becomes *failed* and the
    /// start rejects; after its first frame it becomes *stopped* until a fresh start.
    /// Either way the world is released and the app stays up.
    pub fn crashed(&self) {
        let mut state = self.shared.state.lock().unwrap();
        if state.launched != self.world {
            return;
        }
        match state.status {
            Status::Starting => {
                drop(state);
                self.failed(StartFailure::Panicked);
            }
            Status::Running => {
                eprintln!("[world] world {} crashed after it was running", self.world);
                let world = state.world.take();
                self.shared.set_status(&mut state, Status::Stopped);
                self.shared.unseat(&mut state);
                drop(state);
                if let Some(world) = world {
                    release(world);
                }
            }
            _ => {}
        }
    }

    /// A punishment in the CCTV room has reached its step and asks something of its
    /// real session: a kill's blast, to end it (ADR-0013); work harder's bolt, to send it
    /// a prompt (ADR-0014). Passed on to the shell whatever has happened to this world
    /// since, so a punishment that has reached its step always goes through; it never
    /// holds the state lock.
    pub fn session_request(&self, request: SessionRequest) {
        eprintln!(
            "[world] world {}: asking the shell for {:?} on session {}",
            self.world, request.action, request.session
        );
        (self.shared.on_session)(request);
    }

    /// The player sat in, the glass moved, or they left (none). Passed on to the shell
    /// for the page; a world stopped since reports nothing.
    pub fn seat(&self, seat: Option<SeatReport>) {
        let mut state = self.shared.state.lock().unwrap();
        if state.launched != self.world || state.status != Status::Running {
            return;
        }
        state.seated = seat.is_some();
        (self.shared.on_world)(FromWorld::Seat(seat));
    }

    /// A sit-in that can't be: the hint for the page. A world stopped since says nothing.
    pub fn hint(&self, hint: Hint) {
        let state = self.shared.state.lock().unwrap();
        if state.launched != self.world || state.status != Status::Running {
            return;
        }
        (self.shared.on_world)(FromWorld::Hint(hint));
    }

    /// The world has drawn its first frame. A world stopped since changes nothing.
    pub fn ready(&self) {
        let mut state = self.shared.state.lock().unwrap();
        if state.launched != self.world || state.status != Status::Starting {
            return;
        }
        self.shared.set_status(&mut state, Status::Running);
        for answer in state.waiting.drain(..) {
            let _ = answer.send(Ok(()));
        }
        // Without a link yet, `start` hands it over as soon as it holds one.
        let state = &mut *state;
        if let Some(world) = &state.world {
            if let Some(desk) = state.desk.take() {
                world.send(desk);
            }
        }
    }
}

/// Fails the start of the world `reports` speak for if it has drawn no first frame
/// after `timeout`. A world that is running, stopped or replaced by then is untouched.
fn time_out(reports: Reports, timeout: Duration) {
    let spawned = std::thread::Builder::new()
        .name("world-start-timeout".into())
        .spawn(move || {
            std::thread::sleep(timeout);
            reports.failed(StartFailure::Timeout);
        });
    if let Err(error) = spawned {
        eprintln!("[world] could not time the start: {error}");
    }
}

/// Releases a world that failed or crashed. Its reports may come from the world's own
/// thread, which a stop joins, so the stop runs on a thread of its own.
fn release(world: Box<dyn WorldLink>) {
    let spawned = std::thread::Builder::new()
        .name("world-release".into())
        .spawn(move || world.stop());
    if let Err(error) = spawned {
        eprintln!("[world] could not release the world: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::cctv_state::SessionAction;
    use std::sync::Condvar;
    use std::time::{Duration, Instant};

    /// A stand-in world: counts launches, records what each world was sent and whether
    /// it was stopped, and lets the test play the world's reports.
    #[derive(Clone, Default)]
    struct StandIn(Arc<Mutex<Vec<Launched>>>, Option<StartFailure>);

    #[derive(Clone)]
    struct Launched {
        reports: Reports,
        received: Arc<Mutex<Vec<ToWorld>>>,
        stopped: Arc<Mutex<bool>>,
        /// While set, the world's thread is stuck in a present that waits for a frame
        /// callback the compositor does not send (a suspended window under FIFO): its
        /// stop cannot finish until the present returns.
        in_present: Arc<(Mutex<bool>, Condvar)>,
    }

    struct StandInLink(Launched);
    impl WorldLink for StandInLink {
        fn send(&self, msg: ToWorld) {
            self.0.received.lock().unwrap().push(msg);
        }
        fn stop(self: Box<Self>) {
            let (blocked, returned) = &*self.0.in_present;
            let mut blocked = blocked.lock().unwrap();
            while *blocked {
                blocked = returned.wait(blocked).unwrap();
            }
            *self.0.stopped.lock().unwrap() = true;
        }
    }

    impl Launcher for StandIn {
        fn launch(&self, reports: Reports) -> Box<dyn WorldLink> {
            let launched = Launched {
                reports,
                received: Arc::default(),
                stopped: Arc::default(),
                in_present: Arc::default(),
            };
            self.0.lock().unwrap().push(launched.clone());
            if let Some(token) = self.1 {
                launched.reports.failed(token);
            }
            Box::new(StandInLink(launched))
        }
    }

    impl StandIn {
        /// A world that fails inside `launch`, before the controller holds its link.
        fn failing_at_launch(why: StartFailure) -> Self {
            Self(Arc::default(), Some(why))
        }
        fn launches(&self) -> usize {
            self.0.lock().unwrap().len()
        }
        fn nth(&self, n: usize) -> Launched {
            self.0.lock().unwrap()[n].clone()
        }
        fn latest(&self) -> Launched {
            self.0
                .lock()
                .unwrap()
                .last()
                .expect("a world was launched")
                .clone()
        }
        /// The latest launched world's reports.
        fn world(&self) -> Reports {
            self.latest().reports
        }
        /// What the latest launched world has been sent.
        fn received(&self) -> Vec<ToWorld> {
            self.latest().received.lock().unwrap().clone()
        }
    }

    impl Launched {
        fn stopped(&self) -> bool {
            *self.stopped.lock().unwrap()
        }
        /// The world's thread blocks in present until [`Launched::present_returns`].
        fn block_in_present(&self) {
            *self.in_present.0.lock().unwrap() = true;
        }
        fn present_returns(&self) {
            *self.in_present.0.lock().unwrap() = false;
            self.in_present.1.notify_all();
        }
    }

    fn controller() -> (Controller, StandIn, Arc<Mutex<Vec<Status>>>) {
        controller_timing_out_after(START_TIMEOUT)
    }

    fn controller_timing_out_after(
        timeout: Duration,
    ) -> (Controller, StandIn, Arc<Mutex<Vec<Status>>>) {
        let world = StandIn::default();
        let heard = Arc::new(Mutex::new(Vec::new()));
        let events = heard.clone();
        let controller = Controller::new(
            world.clone(),
            timeout,
            move |s| events.lock().unwrap().push(s),
            |_| {},
            |_| {},
        );
        (controller, world, heard)
    }

    #[test]
    fn nothing_is_launched_until_a_start() {
        let (controller, world, heard) = controller();
        assert_eq!(controller.status(), Status::None);
        assert_eq!(world.launches(), 0);
        assert!(heard.lock().unwrap().is_empty());
    }

    #[test]
    fn a_start_that_becomes_ready_resolves_and_reads_running() {
        let (controller, world, heard) = controller();
        let mut started = controller.start();
        assert_eq!(controller.status(), Status::Starting);
        assert!(
            started.try_recv().is_err(),
            "settled before the first frame"
        );

        world.world().ready();

        assert_eq!(started.blocking_recv().unwrap(), Ok(()));
        assert_eq!(controller.status(), Status::Running);
        assert_eq!(*heard.lock().unwrap(), [Status::Starting, Status::Running]);
    }

    #[test]
    fn concurrent_starts_launch_one_world_and_all_resolve_on_its_first_frame() {
        let (controller, world, heard) = controller();
        let controller = Arc::new(controller);
        let starts: Vec<_> = (0..8)
            .map(|_| {
                let controller = controller.clone();
                std::thread::spawn(move || controller.start())
            })
            .collect();
        let mut starts: Vec<_> = starts.into_iter().map(|t| t.join().unwrap()).collect();

        assert_eq!(world.launches(), 1);
        world.world().ready();
        for started in starts.drain(..) {
            assert_eq!(started.blocking_recv().unwrap(), Ok(()));
        }
        assert_eq!(*heard.lock().unwrap(), [Status::Starting, Status::Running]);
    }

    #[test]
    fn a_start_while_running_resolves_at_once_without_a_second_world() {
        let (controller, world, _) = controller();
        let _first = controller.start();
        world.world().ready();

        let mut again = controller.start();

        assert_eq!(again.try_recv().unwrap(), Ok(()));
        assert_eq!(world.launches(), 1);
        assert_eq!(controller.status(), Status::Running);
    }

    const VIEW: Rect = Rect {
        x: 0,
        y: 32,
        w: 880,
        h: 696,
    };
    const SMALLER: Rect = Rect {
        x: 0,
        y: 32,
        w: 600,
        h: 400,
    };

    #[test]
    fn a_rect_reported_before_the_start_reaches_the_launched_world() {
        let (controller, world, _) = controller();
        controller.rect(VIEW);
        let _started = controller.start();
        assert_eq!(world.received(), [ToWorld::Rect(VIEW)]);
    }

    #[test]
    fn a_rect_reported_while_running_reaches_the_world() {
        let (controller, world, _) = controller();
        let _started = controller.start();
        world.world().ready();
        controller.rect(VIEW);
        controller.rect(SMALLER);
        assert_eq!(
            world.received(),
            [ToWorld::Rect(VIEW), ToWorld::Rect(SMALLER)]
        );
    }

    #[test]
    fn stop_during_a_start_cancels_it_and_returns_to_none() {
        let (controller, world, heard) = controller();
        let started = controller.start();

        controller.stop();

        assert!(
            world.latest().stopped(),
            "the starting world was not stopped"
        );
        assert_eq!(controller.status(), Status::None);
        assert_eq!(
            started.blocking_recv().unwrap(),
            Err(StartFailure::Cancelled),
            "the cancelled start must settle"
        );
        assert_eq!(*heard.lock().unwrap(), [Status::Starting, Status::None]);
    }

    #[test]
    fn a_start_after_a_stop_launches_a_fresh_world() {
        let (controller, world, heard) = controller();
        let _first = controller.start();
        world.world().ready();
        controller.stop();

        let mut started = controller.start();

        assert_eq!(world.launches(), 2, "the stopped world was reused");
        assert_eq!(controller.status(), Status::Starting);
        assert!(
            started.try_recv().is_err(),
            "settled before the fresh world's first frame"
        );
        world.world().ready();
        assert_eq!(started.blocking_recv().unwrap(), Ok(()));
        assert_eq!(
            *heard.lock().unwrap(),
            [
                Status::Starting,
                Status::Running,
                Status::None,
                Status::Starting,
                Status::Running
            ]
        );
    }

    #[test]
    fn stop_ends_a_running_world_and_is_idempotent() {
        let (controller, world, heard) = controller();
        controller.stop();
        let _started = controller.start();
        world.world().ready();

        controller.stop();
        controller.stop();

        assert!(world.latest().stopped());
        assert_eq!(world.launches(), 1);
        assert_eq!(controller.status(), Status::None);
        assert_eq!(
            *heard.lock().unwrap(),
            [Status::Starting, Status::Running, Status::None]
        );
    }

    #[test]
    fn a_stopped_world_that_reports_late_changes_nothing() {
        let (controller, world, heard) = controller();
        let _first = controller.start();
        let stopped = world.world();
        controller.stop();
        let mut fresh = controller.start();

        stopped.ready();

        assert_eq!(controller.status(), Status::Starting);
        assert!(
            fresh.try_recv().is_err(),
            "the stopped world settled the fresh start"
        );
        world.world().ready();
        assert_eq!(fresh.blocking_recv().unwrap(), Ok(()));
        assert_eq!(
            *heard.lock().unwrap(),
            [
                Status::Starting,
                Status::None,
                Status::Starting,
                Status::Running
            ]
        );
    }

    #[test]
    fn a_stopped_world_is_sent_nothing_more() {
        let (controller, world, _) = controller();
        let _started = controller.start();
        world.world().ready();
        controller.stop();

        controller.rect(VIEW);

        assert_eq!(world.nth(0).received.lock().unwrap().len(), 0);
        let _again = controller.start();
        assert_eq!(
            world.nth(1).received.lock().unwrap().clone(),
            [ToWorld::Rect(VIEW)]
        );
    }

    /// Waits up to two seconds for a start to settle, so a missing timeout fails the
    /// test instead of hanging it.
    fn settled(mut started: oneshot::Receiver<Outcome>) -> Outcome {
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        loop {
            match started.try_recv() {
                Ok(outcome) => return outcome,
                Err(oneshot::error::TryRecvError::Empty)
                    if std::time::Instant::now() < deadline =>
                {
                    std::thread::sleep(Duration::from_millis(5))
                }
                Err(error) => panic!("the start did not settle: {error:?}"),
            }
        }
    }

    /// Waits for a world's release, which happens off the reporting thread.
    fn released(launched: &Launched) -> bool {
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while std::time::Instant::now() < deadline {
            if launched.stopped() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        false
    }

    #[test]
    fn a_world_that_fails_while_starting_reads_failed_rejects_the_start_and_is_released() {
        let (controller, world, heard) = controller();
        let started = controller.start();

        world.world().failed(StartFailure::AttachFailed);

        assert_eq!(controller.status(), Status::Failed);
        assert_eq!(
            started.blocking_recv().unwrap(),
            Err(StartFailure::AttachFailed)
        );
        assert!(
            released(&world.latest()),
            "the failed world was not released"
        );
        assert_eq!(*heard.lock().unwrap(), [Status::Starting, Status::Failed]);
    }

    #[test]
    fn a_world_that_panics_while_starting_reads_failed_and_rejects_the_start() {
        let (controller, world, heard) = controller();
        let started = controller.start();

        world.world().crashed();

        assert_eq!(controller.status(), Status::Failed);
        assert_eq!(
            started.blocking_recv().unwrap(),
            Err(StartFailure::Panicked)
        );
        assert!(released(&world.latest()));
        assert_eq!(*heard.lock().unwrap(), [Status::Starting, Status::Failed]);
    }

    #[test]
    fn a_start_without_a_first_frame_in_time_fails_and_releases_the_world() {
        let (controller, world, heard) = controller_timing_out_after(Duration::from_millis(30));
        let started = controller.start();

        assert_eq!(settled(started), Err(StartFailure::Timeout));
        assert_eq!(controller.status(), Status::Failed);
        assert!(
            released(&world.latest()),
            "the half-started world was not released"
        );
        world.world().ready();
        assert_eq!(
            controller.status(),
            Status::Failed,
            "a late first frame revived it"
        );
        assert_eq!(*heard.lock().unwrap(), [Status::Starting, Status::Failed]);
    }

    #[test]
    fn a_start_with_a_zero_timeout_fails_at_once() {
        let (controller, world, _) = controller_timing_out_after(Duration::ZERO);
        let started = controller.start();
        assert_eq!(settled(started), Err(StartFailure::Timeout));
        assert!(released(&world.latest()));
    }

    #[test]
    fn a_first_frame_in_time_is_not_failed_by_the_timeout() {
        let (controller, world, _) = controller_timing_out_after(Duration::from_millis(30));
        let started = controller.start();
        world.world().ready();
        assert_eq!(started.blocking_recv().unwrap(), Ok(()));
        std::thread::sleep(Duration::from_millis(80));
        assert_eq!(controller.status(), Status::Running);
        assert!(!world.latest().stopped());
    }

    #[test]
    fn a_world_that_crashes_after_ready_reads_stopped_until_a_fresh_start() {
        let (controller, world, heard) = controller();
        let _started = controller.start();
        world.world().ready();

        world.world().crashed();

        assert_eq!(controller.status(), Status::Stopped);
        assert!(
            released(&world.latest()),
            "the crashed world was not released"
        );
        let mut joined = controller.start();
        assert_eq!(
            world.launches(),
            2,
            "Try again did not launch a fresh world"
        );
        assert!(joined.try_recv().is_err());
        world.world().ready();
        assert_eq!(joined.blocking_recv().unwrap(), Ok(()));
        assert_eq!(
            *heard.lock().unwrap(),
            [
                Status::Starting,
                Status::Running,
                Status::Stopped,
                Status::Starting,
                Status::Running
            ]
        );
    }

    #[test]
    fn a_failed_start_is_retried_by_a_fresh_start() {
        let (controller, world, _) = controller();
        let _first = controller.start();
        world.world().failed(StartFailure::AttachFailed);

        let again = controller.start();

        assert_eq!(world.launches(), 2);
        assert_eq!(controller.status(), Status::Starting);
        world.world().ready();
        assert_eq!(again.blocking_recv().unwrap(), Ok(()));
    }

    #[test]
    fn stop_clears_a_failed_or_stopped_world_to_none() {
        let (controller, world, _) = controller();
        let _first = controller.start();
        world.world().failed(StartFailure::AttachFailed);
        controller.stop();
        assert_eq!(controller.status(), Status::None);

        let _second = controller.start();
        world.world().ready();
        world.world().crashed();
        controller.stop();
        assert_eq!(controller.status(), Status::None);
    }

    #[test]
    fn a_session_request_from_the_world_reaches_the_shell_even_from_a_world_stopped_since() {
        let world = StandIn::default();
        let heard = Arc::new(Mutex::new(Vec::new()));
        let shell = heard.clone();
        let controller = Controller::new(
            world.clone(),
            START_TIMEOUT,
            |_| {},
            move |request| shell.lock().unwrap().push(request),
            |_| {},
        );
        let request = |session: &str, action| SessionRequest {
            session: session.into(),
            action,
        };
        let _started = controller.start();
        let running = world.world();
        running.ready();
        assert!(heard.lock().unwrap().is_empty());
        running.session_request(request("s-1", SessionAction::End));
        running.session_request(request("s-2", SessionAction::WorkHarder));
        assert_eq!(
            *heard.lock().unwrap(),
            [
                request("s-1", SessionAction::End),
                request("s-2", SessionAction::WorkHarder)
            ]
        );
        // Once a kill has blown or a bolt has struck, it goes through, whatever has
        // happened to the world since.
        controller.stop();
        running.session_request(request("s-3", SessionAction::WorkHarder));
        assert_eq!(heard.lock().unwrap().len(), 3);
        assert_eq!(
            heard.lock().unwrap()[2],
            request("s-3", SessionAction::WorkHarder)
        );
        // Nothing else the controller does asks anything of a session.
        let _started = controller.start();
        world.world().ready();
        controller.click(Button::Primary);
        controller.stop();
        assert_eq!(heard.lock().unwrap().len(), 3);
    }

    #[test]
    fn a_world_that_fails_during_its_launch_is_released() {
        let world = StandIn::failing_at_launch(StartFailure::NoWindow);
        let controller = Controller::new(world.clone(), START_TIMEOUT, |_| {}, |_| {}, |_| {});
        let started = controller.start();
        assert_eq!(
            started.blocking_recv().unwrap(),
            Err(StartFailure::NoWindow)
        );
        assert_eq!(controller.status(), Status::Failed);
        assert!(released(&world.latest()));
    }

    #[test]
    fn shown_reaches_the_world_and_a_fresh_world_learns_it_is_shown() {
        let (controller, world, _) = controller();
        controller.shown(true);
        let _first = controller.start();
        world.world().ready();
        controller.shown(false);
        controller.shown(true);
        assert_eq!(
            world.received(),
            [
                ToWorld::Shown(true),
                ToWorld::Shown(false),
                ToWorld::Shown(true)
            ]
        );

        controller.stop();
        controller.rect(VIEW);
        let _second = controller.start();
        assert_eq!(
            world.received(),
            [ToWorld::Rect(VIEW), ToWorld::Shown(true)]
        );

        // A world starts not shown, so a world launched while hidden is told nothing.
        controller.shown(false);
        controller.stop();
        let _third = controller.start();
        assert_eq!(world.received(), [ToWorld::Rect(VIEW)]);
    }

    fn tree(projects: &[&str]) -> Arc<AgentTreeSnapshot> {
        Arc::new(AgentTreeSnapshot {
            status: agents::TreeStatus::Ready,
            projects: projects
                .iter()
                .map(|id| agents::Project {
                    id: id.to_string(),
                    name: id.to_string(),
                    sessions: Vec::new(),
                })
                .collect(),
        })
    }

    #[test]
    fn the_latest_tree_is_replayed_to_a_newly_launched_world() {
        let (controller, world, _) = controller();
        controller.tree(tree(&["-a"]));
        controller.tree(tree(&["-a", "-b"]));
        controller.rect(VIEW);
        let _first = controller.start();
        assert_eq!(
            world.received(),
            [ToWorld::Rect(VIEW), ToWorld::Tree(tree(&["-a", "-b"]))],
            "only the latest tree, once"
        );

        world.world().ready();
        controller.tree(tree(&["-b"]));
        controller.stop();
        let _second = controller.start();
        assert_eq!(
            world.received(),
            [ToWorld::Rect(VIEW), ToWorld::Tree(tree(&["-b"]))],
            "a fresh world after a stop gets the latest tree"
        );
    }

    const SMALL_PLAIN: WorldSettings = WorldSettings {
        subagent_scale: 0.5,
        textures: false,
        clutter: false,
        punishment_effects: false,
        controls: Controls {
            walk_speed: 3.5,
            look_sensitivity: 0.7,
        },
    };
    const DEFAULTS: WorldSettings = WorldSettings {
        subagent_scale: 0.75,
        textures: true,
        clutter: true,
        punishment_effects: true,
        controls: Controls {
            walk_speed: crate::world::controls::DEFAULT_WALK_SPEED,
            look_sensitivity: crate::world::controls::DEFAULT_LOOK_SENSITIVITY,
        },
    };

    #[test]
    fn the_latest_settings_are_replayed_to_a_newly_launched_world() {
        let (controller, world, _) = controller();
        controller.settings(DEFAULTS);
        assert_eq!(world.launches(), 0, "settings launch no world");
        controller.settings(SMALL_PLAIN);
        controller.rect(VIEW);
        let _first = controller.start();
        assert_eq!(
            world.received(),
            [ToWorld::Rect(VIEW), ToWorld::Settings(SMALL_PLAIN)],
            "only the latest settings, once"
        );

        world.world().ready();
        controller.settings(DEFAULTS);
        assert_eq!(
            world.received(),
            [
                ToWorld::Rect(VIEW),
                ToWorld::Settings(SMALL_PLAIN),
                ToWorld::Settings(DEFAULTS)
            ],
            "a change reaches the running world"
        );

        controller.stop();
        controller.settings(SMALL_PLAIN);
        let _second = controller.start();
        assert_eq!(
            world.received(),
            [ToWorld::Rect(VIEW), ToWorld::Settings(SMALL_PLAIN)],
            "a fresh world after a stop gets the settings changed while none existed"
        );
        // Punishment effects travel with the rest: a world
        // launched after the switch went off starts with them off.
        controller.stop();
        let effects_off = WorldSettings {
            punishment_effects: false,
            ..DEFAULTS
        };
        controller.settings(effects_off);
        let _third = controller.start();
        match world.received().as_slice() {
            [ToWorld::Rect(_), ToWorld::Settings(settings)] => {
                assert!(!settings.punishment_effects, "replayed with effects off");
                assert_eq!(*settings, effects_off);
            }
            other => panic!("expected the rect and the settings, got {other:?}"),
        }
    }

    fn strings(pairs: &[(&str, &str)]) -> Arc<Strings> {
        Arc::new(
            pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        )
    }

    #[test]
    fn the_latest_world_strings_are_replayed_to_a_newly_launched_world() {
        let (controller, world, _) = controller();
        let english = strings(&[("world.text.cctv-sign", "CCTV")]);
        let other = strings(&[("world.text.cctv-sign", "VIDÉO")]);
        controller.strings(english.clone());
        assert_eq!(world.launches(), 0, "strings launch no world");
        controller.strings(other.clone());
        controller.rect(VIEW);
        let _first = controller.start();
        assert_eq!(
            world.received(),
            [ToWorld::Rect(VIEW), ToWorld::Strings(other.clone())],
            "only the latest table, once"
        );

        world.world().ready();
        controller.strings(english.clone());
        assert_eq!(
            world.received(),
            [
                ToWorld::Rect(VIEW),
                ToWorld::Strings(other.clone()),
                ToWorld::Strings(english.clone()),
            ],
            "a language change reaches the running world"
        );

        controller.stop();
        controller.strings(other.clone());
        let _second = controller.start();
        assert_eq!(
            world.received(),
            [ToWorld::Rect(VIEW), ToWorld::Strings(other)],
            "a fresh world after a stop gets the table changed while none existed"
        );
    }

    #[test]
    fn trees_reach_a_starting_running_or_hidden_world_as_they_come() {
        let (controller, world, _) = controller();
        let _started = controller.start();
        controller.tree(tree(&["-a"]));
        world.world().ready();
        controller.tree(tree(&["-a", "-b"]));
        // Kept warm, and suspended: the world still applies what it is sent.
        controller.shown(false);
        world.latest().block_in_present();
        controller.tree(tree(&["-b"]));
        world.latest().present_returns();
        assert_eq!(
            world.received(),
            [
                ToWorld::Tree(tree(&["-a"])),
                ToWorld::Tree(tree(&["-a", "-b"])),
                ToWorld::Shown(false),
                ToWorld::Tree(tree(&["-b"])),
            ]
        );
    }

    #[test]
    fn no_tree_is_sent_when_no_world_exists() {
        let (controller, world, _) = controller();
        controller.tree(tree(&["-a"]));
        assert_eq!(world.launches(), 0, "a tree launches no world");

        let _first = controller.start();
        world.world().ready();
        controller.stop();
        controller.tree(tree(&["-b"]));
        assert_eq!(
            world.nth(0).received.lock().unwrap().clone(),
            [ToWorld::Tree(tree(&["-a"]))],
            "the stopped world was sent a tree"
        );

        let _second = controller.start();
        world.world().failed(StartFailure::AttachFailed);
        assert!(released(&world.latest()));
        controller.tree(tree(&["-c"]));
        assert_eq!(
            world.nth(1).received.lock().unwrap().clone(),
            [ToWorld::Tree(tree(&["-b"]))],
            "the failed world was sent a tree"
        );
    }

    #[test]
    fn stop_completes_while_the_world_is_blocked_in_present() {
        let (controller, world, heard) = controller();
        let _first = controller.start();
        world.world().ready();
        let suspended = world.latest();
        suspended.block_in_present();

        let begun = Instant::now();
        controller.stop();

        assert!(
            begun.elapsed() < Duration::from_secs(2),
            "stop hung on the blocked world for {:?}",
            begun.elapsed()
        );
        assert_eq!(controller.status(), Status::None);
        let fresh = controller.start();
        assert_eq!(world.launches(), 2, "no fresh world after the stop");
        world.world().ready();
        assert_eq!(fresh.blocking_recv().unwrap(), Ok(()));
        assert_eq!(
            *heard.lock().unwrap(),
            [
                Status::Starting,
                Status::Running,
                Status::None,
                Status::Starting,
                Status::Running
            ]
        );

        // Once the present returns, the let-go world finishes stopping by itself.
        suspended.present_returns();
        let waited = Instant::now();
        while !suspended.stopped() && waited.elapsed() < Duration::from_secs(2) {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(
            suspended.stopped(),
            "the blocked world never finished stopping"
        );
    }

    #[test]
    fn look_reaches_the_running_world_and_is_not_kept_for_the_next_one() {
        let (controller, world, _) = controller();
        controller.look(3.0, -2.0);
        let _started = controller.start();
        world.world().ready();
        controller.look(12.5, 4.0);
        assert_eq!(world.received(), [ToWorld::Look { dx: 12.5, dy: 4.0 }]);
    }

    #[test]
    fn a_click_reaches_the_running_world_and_is_dropped_without_one_and_never_replayed() {
        let (controller, world, _) = controller();
        let click = ToWorld::Click(Button::Primary);
        controller.click(Button::Primary);
        let _started = controller.start();
        world.world().ready();
        assert!(
            !world.received().contains(&click),
            "a click before the world was dropped"
        );
        controller.click(Button::Primary);
        controller.click(Button::Primary);
        assert_eq!(world.received(), [click.clone(), click.clone()]);

        // A world launched later hears none of them.
        controller.stop();
        let _again = controller.start();
        world.world().ready();
        assert_eq!(world.launches(), 2);
        assert!(!world.received().contains(&click));
    }

    #[test]
    fn a_right_click_carries_its_button_to_the_running_world() {
        let (controller, world, _) = controller();
        let _started = controller.start();
        world.world().ready();
        controller.click(Button::Secondary);
        controller.click(Button::Primary);
        assert_eq!(
            world.received(),
            [
                ToWorld::Click(Button::Secondary),
                ToWorld::Click(Button::Primary)
            ]
        );
    }

    fn sit_in(session: &str) -> ToWorld {
        ToWorld::SitIn {
            session_id: session.into(),
        }
    }

    #[test]
    fn a_sit_in_reaches_the_running_world_and_is_dropped_without_one_and_never_replayed() {
        let (controller, world, _) = controller();
        controller.sit_in("before".into());
        let _started = controller.start();
        world.world().ready();
        assert!(
            world.received().is_empty(),
            "a sit-in before the world was dropped"
        );
        controller.sit_in("s1".into());
        assert_eq!(world.received(), [sit_in("s1")]);

        // A world launched later is not sat in: it would surprise.
        controller.stop();
        let _again = controller.start();
        world.world().ready();
        assert_eq!(world.launches(), 2);
        assert!(world
            .received()
            .iter()
            .all(|msg| !matches!(msg, ToWorld::SitIn { .. })));
    }

    /// A controller whose shell records every seat and hint the world reports.
    fn controller_hearing() -> (Controller, StandIn, Arc<Mutex<Vec<FromWorld>>>) {
        let world = StandIn::default();
        let heard = Arc::new(Mutex::new(Vec::new()));
        let shell = heard.clone();
        let controller = Controller::new(
            world.clone(),
            START_TIMEOUT,
            |_| {},
            |_| {},
            move |event| shell.lock().unwrap().push(event),
        );
        (controller, world, heard)
    }

    fn seated(session: &str, x: f32) -> SeatReport {
        SeatReport {
            session_id: session.into(),
            short_id: format!("{session}-short"),
            rect: SeatRect {
                x,
                y: 40.0,
                w: 900.0,
                h: 540.0,
            },
        }
    }

    #[test]
    fn leave_reaches_the_world_only_while_it_reports_a_seat() {
        let (controller, world, _) = controller_hearing();
        let _started = controller.start();
        let running = world.world();
        running.ready();
        controller.leave();
        assert!(world.received().is_empty(), "no seat: nothing to leave");
        running.seat(Some(seated("s1", 100.0)));
        controller.leave();
        assert_eq!(world.received(), [ToWorld::Leave]);
        running.seat(None);
        controller.leave();
        assert_eq!(world.received(), [ToWorld::Leave]);
    }

    #[test]
    fn while_seated_the_world_takes_no_key_and_every_key_reaches_the_page() {
        let (controller, world, _) = controller_hearing();
        let _started = controller.start();
        let running = world.world();
        running.ready();
        running.seat(Some(seated("s1", 100.0)));
        // Even a capture report (none should come while seated) gives the world nothing.
        controller.captured(true);
        let before = world.received().len();
        for code in [W, UP, Q, ESCAPE] {
            assert!(
                !controller.key(code, true),
                "the seated terminal gets {code}"
            );
            assert!(!controller.key(code, false));
        }
        assert!(
            world.received()[before..]
                .iter()
                .all(|msg| !matches!(msg, ToWorld::Key { .. })),
            "no key reaches the world"
        );

        // Up again, walking keys are the world's once more.
        running.seat(None);
        assert!(controller.key(W, true));
    }

    #[test]
    fn the_seat_and_hints_reach_the_shell_and_a_world_that_ends_leaves_its_seat() {
        let (controller, world, heard) = controller_hearing();
        let _started = controller.start();
        let first = world.world();
        first.ready();
        first.hint(Hint::Terminal);
        first.seat(Some(seated("s1", 100.0)));
        // The rectangle changed: reported again.
        first.seat(Some(seated("s1", 120.0)));
        assert_eq!(
            *heard.lock().unwrap(),
            [
                FromWorld::Hint(Hint::Terminal),
                FromWorld::Seat(Some(seated("s1", 100.0))),
                FromWorld::Seat(Some(seated("s1", 120.0))),
            ]
        );
        // Stopping the world (quitting, a restart) ends the seat, once.
        controller.stop();
        controller.stop();
        assert_eq!(heard.lock().unwrap().len(), 4);
        assert_eq!(heard.lock().unwrap()[3], FromWorld::Seat(None));
        // A stopped world reports nothing more.
        first.seat(Some(seated("s2", 100.0)));
        first.hint(Hint::Nothing);
        assert_eq!(heard.lock().unwrap().len(), 4);

        // A world that crashes while seated leaves its seat too.
        let _again = controller.start();
        let second = world.world();
        second.ready();
        second.seat(Some(seated("s3", 100.0)));
        second.crashed();
        assert_eq!(
            heard.lock().unwrap()[4..],
            [
                FromWorld::Seat(Some(seated("s3", 100.0))),
                FromWorld::Seat(None)
            ]
        );
        // Without a seat, ending the world says nothing.
        let _third = controller.start();
        world.world().ready();
        controller.stop();
        assert_eq!(heard.lock().unwrap().len(), 6);
    }

    fn desk(agent: &str) -> ToWorld {
        ToWorld::GoToDesk {
            session_id: "s".into(),
            agent_id: agent.into(),
        }
    }

    #[test]
    fn go_to_desk_reaches_a_running_world_at_once() {
        let (controller, world, _) = controller();
        let _started = controller.start();
        world.world().ready();
        controller.go_to_desk("s".into(), "a".into());
        controller.go_to_desk("s".into(), "b".into());
        assert_eq!(world.received(), [desk("a"), desk("b")]);
    }

    #[test]
    fn go_to_desk_while_starting_keeps_the_latest_and_hands_it_over_once_running() {
        let (controller, world, _) = controller();
        let _started = controller.start();
        controller.go_to_desk("s".into(), "a".into());
        controller.go_to_desk("s".into(), "b".into());
        assert!(world.received().is_empty(), "sent before the world runs");

        world.world().ready();

        assert_eq!(world.received(), [desk("b")], "only the latest, once");
        controller.go_to_desk("s".into(), "c".into());
        assert_eq!(world.received(), [desk("b"), desk("c")]);
    }

    #[test]
    fn go_to_desk_is_sent_nowhere_without_a_world() {
        let (controller, world, _) = controller();
        controller.go_to_desk("s".into(), "before".into());
        assert_eq!(world.launches(), 0, "go to desk launches no world");
        let _first = controller.start();
        world.world().ready();
        assert!(
            world.received().is_empty(),
            "kept for a world launched later"
        );

        // Kept while starting, then the start is stopped, or fails, or the world
        // crashes: the request goes with it.
        controller.stop();
        let _second = controller.start();
        controller.go_to_desk("s".into(), "stopped".into());
        controller.stop();
        controller.go_to_desk("s".into(), "after a stop".into());
        let _third = controller.start();
        controller.go_to_desk("s".into(), "failed".into());
        world.world().failed(StartFailure::AttachFailed);
        assert!(released(&world.latest()));
        controller.go_to_desk("s".into(), "after a failure".into());
        let _fourth = controller.start();
        world.world().ready();
        world.world().crashed();
        controller.go_to_desk("s".into(), "after a crash".into());
        let _fifth = controller.start();
        world.world().ready();

        for n in 0..5 {
            let got = world.nth(n).received.lock().unwrap().clone();
            assert!(
                !got.iter().any(|m| matches!(m, ToWorld::GoToDesk { .. })),
                "world {n} was sent {got:?}"
            );
        }
    }

    const W: u16 = 25;
    const UP: u16 = 111;
    const Q: u16 = 24;
    const ESCAPE: u16 = 9;

    #[test]
    fn capture_reaches_the_world_and_a_fresh_world_learns_it_is_captured() {
        let (controller, world, _) = controller();
        controller.captured(true);
        let _first = controller.start();
        world.world().ready();
        controller.captured(false);
        assert_eq!(
            world.received(),
            [ToWorld::Captured(true), ToWorld::Captured(false)]
        );

        // A world starts released, so a world launched while released is told nothing.
        controller.stop();
        let _second = controller.start();
        assert_eq!(world.received(), []);
    }

    #[test]
    fn keys_reach_a_world_that_exists_and_are_not_kept_for_the_next_one() {
        let (controller, world, _) = controller();
        controller.key(W, true);
        assert_eq!(world.launches(), 0, "a key launches no world");
        let _started = controller.start();
        controller.key(W, true);
        controller.key(W, false);
        controller.focus_lost();
        assert_eq!(
            world.received(),
            [
                ToWorld::Key {
                    code: W,
                    pressed: true
                },
                ToWorld::Key {
                    code: W,
                    pressed: false
                },
                ToWorld::FocusLost
            ]
        );
    }

    #[test]
    fn only_the_walking_keys_are_kept_from_the_page_and_only_while_captured() {
        let (controller, world, _) = controller();
        let _started = controller.start();
        world.world().ready();
        assert!(!controller.key(W, true), "released: the page gets W");
        controller.captured(true);
        assert!(controller.key(W, true));
        assert!(
            controller.key(UP, false),
            "the arrows do not scroll the panel"
        );
        assert!(!controller.key(Q, true), "other keys pass");
        assert!(!controller.key(ESCAPE, true), "Esc always passes");
        controller.captured(false);
        assert!(!controller.key(UP, true));

        // A focus loss releases at once, before the page reports it.
        controller.captured(true);
        controller.focus_lost();
        assert!(!controller.key(W, true));
    }

    #[test]
    fn closing_with_no_world_quits_at_once() {
        let (controller, world, _) = controller();
        assert_eq!(controller.close_requested(), Close::Now, "no world started");

        let _first = controller.start();
        world.world().failed(StartFailure::AttachFailed);
        assert_eq!(
            controller.close_requested(),
            Close::Now,
            "after a failed start"
        );

        let _second = controller.start();
        world.world().ready();
        world.world().crashed();
        assert_eq!(
            controller.close_requested(),
            Close::Now,
            "after the world stopped"
        );

        let _third = controller.start();
        world.world().ready();
        controller.stop();
        assert_eq!(controller.close_requested(), Close::Now, "after a stop");
    }

    #[test]
    fn closing_while_a_world_exists_asks_once_and_holds_further_closes() {
        let (controller, world, _) = controller();
        let _started = controller.start();
        assert_eq!(controller.close_requested(), Close::Ask, "while starting");
        assert_eq!(
            controller.close_requested(),
            Close::Hold,
            "a second close while the prompt is out must not prompt again"
        );
        controller.quit_answered(false);

        world.world().ready();
        assert_eq!(controller.close_requested(), Close::Ask, "while running");
        controller.quit_answered(false);

        // Kept warm (not shown) and suspended worlds still exist.
        controller.shown(false);
        world.latest().block_in_present();
        assert_eq!(controller.close_requested(), Close::Ask, "while suspended");
        world.latest().present_returns();
    }

    #[test]
    fn a_page_that_loads_again_while_asking_is_asked_on_the_next_close() {
        let (controller, world, _) = controller();
        let _started = controller.start();
        world.world().ready();
        assert_eq!(controller.close_requested(), Close::Ask);

        // The page reloads before it answers: its prompt, and the answer, are gone.
        controller.page_loading();

        assert_eq!(
            controller.close_requested(),
            Close::Ask,
            "a close after the page reloaded must ask the new page, not hold forever"
        );
        assert_eq!(controller.close_requested(), Close::Hold);
        assert_eq!(controller.status(), Status::Running);
    }

    #[test]
    fn cancelling_the_quit_prompt_changes_nothing() {
        let (controller, world, heard) = controller();
        let _started = controller.start();
        world.world().ready();
        assert_eq!(controller.close_requested(), Close::Ask);

        assert!(!controller.quit_answered(false), "a cancel must not quit");

        assert_eq!(controller.status(), Status::Running);
        assert!(!world.latest().stopped());
        assert_eq!(world.launches(), 1);
        assert_eq!(*heard.lock().unwrap(), [Status::Starting, Status::Running]);
        assert_eq!(
            controller.close_requested(),
            Close::Ask,
            "the next close asks again"
        );
    }

    #[test]
    fn confirming_the_quit_prompt_stops_the_world_and_quits() {
        let (controller, world, heard) = controller();
        let _started = controller.start();
        world.world().ready();
        assert_eq!(controller.close_requested(), Close::Ask);

        assert!(controller.quit_answered(true), "a confirm must quit");

        assert!(
            world.latest().stopped(),
            "the world was not stopped before the quit"
        );
        assert_eq!(controller.status(), Status::None);
        assert_eq!(
            *heard.lock().unwrap(),
            [Status::Starting, Status::Running, Status::None]
        );
    }

    #[test]
    fn quit_completes_while_the_world_is_blocked_in_present() {
        let (controller, world, _) = controller();
        let _started = controller.start();
        world.world().ready();
        let suspended = world.latest();
        suspended.block_in_present();
        assert_eq!(controller.close_requested(), Close::Ask);

        let begun = Instant::now();
        assert!(controller.quit_answered(true));

        assert!(
            begun.elapsed() < Duration::from_secs(2),
            "quit hung on the blocked world for {:?}",
            begun.elapsed()
        );
        assert_eq!(controller.status(), Status::None);
        assert_eq!(controller.close_requested(), Close::Now);
        suspended.present_returns();
    }
}
