//! Session terminals (ADR-0015): the `terminals` crate, which runs every `claude attach`
//! the app owns and paints the PCs' images, held as managed state. The world's live
//! budget tells it which sessions to watch and reads their images back
//! ([`SessionScreens`]); a thread of its own ticks it, so attaching and painting never
//! hold up a frame.
//!
//! While the player sits in a session (the seated view), the same thread streams that
//! session's raw output to the page's overlay over a Tauri channel, about once a frame
//! ([`terminal_stream`]); the overlay's terminal starts with the full redraw the size
//! retake brings. What the player types there comes back through [`terminal_input`] and
//! is written to the seated attach alone. An attach that leaves the seated session (`/exit`, ← or a double
//! Ctrl+C on an empty prompt, the session stopped elsewhere) closes the seated view.
//!
//! In a debug build, `WORLD_DEBUG_CLAUDE=<path>` runs that binary as `claude attach`
//! instead of the real one (a fake attach, `terminals/tests/fake_claude.py`, behind a
//! wrapper script), and `WORLD_DEBUG_CLAUDE_SESSIONS=<dir>` reads the session files that
//! say which sessions run from there instead of `~/.claude/sessions`.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tauri::ipc::{Channel, InvokeBody, InvokeResponseBody, Request};
use tauri::{AppHandle, Manager, State};

use crate::world::{Controller, Screen, SeatReport, SessionScreens};

/// How often the terminals are ticked: attaches started, left sessions noticed (twice a
/// second each), changed grids painted (at most twice a second each).
const TICK_EVERY: Duration = Duration::from_millis(100);

/// How often while the overlay streams: about once a frame, so typing echoes at once
/// and no more than 60 sends a second cross IPC (spec "Keys, paste and links").
const STREAM_EVERY: Duration = Duration::from_millis(16);

/// The terminals, or nothing when no `claude` binary was found.
pub struct SessionTerminals(Option<Arc<Shared>>);

struct Shared {
    terminals: Mutex<terminals::Terminals>,
    /// The live set the world asked for last, until the ticker hands it on: the world
    /// never waits on a paint.
    asked: Mutex<Option<BTreeSet<String>>>,
    seat: Mutex<SeatStream>,
}

/// The seated session and the overlay's stream of it.
#[derive(Default)]
struct SeatStream {
    /// The seated session, as the world last reported it.
    seated: Option<SeatedAt>,
    /// The overlay's channel, once the page has asked for the seated session's stream.
    channel: Option<Channel<InvokeResponseBody>>,
    /// What the ticker tells the terminals about the seat next.
    change: SeatChange,
}

/// A seated session, by its id and its short id.
#[derive(Clone, PartialEq, Eq)]
struct SeatedAt {
    session_id: String,
    short_id: String,
}

/// A change of seat for the terminals, until the ticker hands it on.
#[derive(Default)]
enum SeatChange {
    #[default]
    Unchanged,
    /// Nobody is seated, or the overlay has not asked for the new seat's stream yet.
    Unseat,
    /// Stream this session, by short id.
    Stream(String),
}

impl SessionTerminals {
    /// Finds `claude` as the registry does (`PATH`, then `~/.local/bin`) and reads
    /// Claude Code's sessions from `~/.claude/sessions`.
    pub fn new(app: &AppHandle) -> Self {
        let home = crate::agents::home(app);
        let path = std::env::var_os("PATH");
        let found = agents::resolve_claude(path.as_deref(), &home);
        #[cfg_attr(not(debug_assertions), allow(unused_mut))]
        let mut sessions = home.join(".claude/sessions");
        #[cfg(debug_assertions)]
        let found = match std::env::var_os("WORLD_DEBUG_CLAUDE") {
            Some(fake) => {
                if let Some(dir) = std::env::var_os("WORLD_DEBUG_CLAUDE_SESSIONS") {
                    sessions = std::path::PathBuf::from(dir);
                }
                eprintln!(
                    "[terminals] debug: attaching with {}",
                    fake.to_string_lossy()
                );
                Some(std::path::PathBuf::from(fake))
            }
            None => found,
        };
        let Some(claude) = found else {
            eprintln!("[terminals] no claude binary found: no session terminals");
            return SessionTerminals(None);
        };
        eprintln!(
            "[terminals] ready: {} attach, sessions in {}, {}x{} at most",
            claude.display(),
            sessions.display(),
            terminals::COLUMNS,
            terminals::ROWS
        );
        let shared = Arc::new(Shared {
            terminals: Mutex::new(terminals::Terminals::new(claude, sessions)),
            asked: Mutex::new(None),
            seat: Mutex::new(SeatStream::default()),
        });
        let ticked = shared.clone();
        let app = app.clone();
        let spawned = std::thread::Builder::new()
            .name("session-terminals".into())
            .spawn(move || loop {
                let streaming = ticked.seat.lock().unwrap().channel.is_some();
                std::thread::sleep(if streaming { STREAM_EVERY } else { TICK_EVERY });
                if ticked.tick() {
                    // The seated attach left its session: so does the player.
                    app.state::<Controller>().leave();
                }
            });
        if let Err(error) = spawned {
            eprintln!("[terminals] could not start ticking: {error}");
        }
        SessionTerminals(Some(shared))
    }

    /// The seat as the world reports it, each time it does: a new seated session, or
    /// none, ends the overlay's stream until the page asks for the new one.
    pub fn seated(&self, seat: Option<&SeatReport>) {
        let Some(shared) = &self.0 else { return };
        let mut stream = shared.seat.lock().unwrap();
        let seated = seat.map(|seat| SeatedAt {
            session_id: seat.session_id.clone(),
            short_id: seat.short_id.clone(),
        });
        if stream.seated == seated {
            return;
        }
        stream.seated = seated;
        stream.channel = None;
        stream.change = SeatChange::Unseat;
    }

    /// Hangs up every attach now and forgets the seat, as the app exits (the world
    /// shuts the terminals down as it ends, too: [`SessionScreens::shutdown`]). Every
    /// session keeps running.
    pub fn shutdown(&self) {
        if let Some(shared) = &self.0 {
            shared.shutdown();
        }
    }

    /// The terminals as the world reaches them, if there are any.
    pub fn screens(&self) -> Option<Arc<dyn SessionScreens>> {
        self.0
            .clone()
            .map(|shared| shared as Arc<dyn SessionScreens>)
    }
}

impl Shared {
    /// Hands on the live set the world asked for and the seated session to stream, then
    /// ticks, and sends the overlay what the seated session drew. An attach that left
    /// its session is logged by the crate and attached again while it stays live.
    /// Returns whether the seated session's attach left it.
    fn tick(&self) -> bool {
        let asked = self.asked.lock().unwrap().take();
        let (change, seated, channel) = {
            let mut seat = self.seat.lock().unwrap();
            (
                std::mem::take(&mut seat.change),
                seat.seated.as_ref().map(|seated| seated.short_id.clone()),
                seat.channel.clone(),
            )
        };
        let now = Instant::now();
        let mut terminals = self.terminals.lock().unwrap();
        if let Some(live) = asked {
            terminals.set_live(live);
        }
        match change {
            SeatChange::Unchanged => {}
            SeatChange::Unseat => terminals.seat(None, now),
            SeatChange::Stream(short_id) => terminals.seat(Some(&short_id), now),
        }
        let left = terminals.tick(now);
        let output = terminals.seated_output();
        drop(terminals);
        if let (Some(channel), false) = (channel, output.is_empty()) {
            if let Err(error) = channel.send(InvokeResponseBody::Raw(output)) {
                eprintln!("[terminals] could not stream to the overlay: {error}");
            }
        }
        seated.is_some_and(|short| left.iter().any(|gone| gone.short_id == short))
    }
}

/// **terminal stream**: the overlay's channel for the seated session's raw output (the
/// seated attach's, ADR-0015), asked for once the overlay is up. The session's size is
/// retaken then, so the stream starts with a full redraw. Ignored unless `session_id`
/// is the seated session; while nobody is seated no terminal data crosses IPC.
#[tauri::command]
pub fn terminal_stream(
    terminals: State<'_, SessionTerminals>,
    session_id: String,
    channel: Channel<InvokeResponseBody>,
) {
    let Some(shared) = &terminals.0 else { return };
    let mut seat = shared.seat.lock().unwrap();
    let Some(seated) = seat.seated.clone() else {
        eprintln!("[terminals] stream for {session_id} asked while nobody is seated: ignored");
        return;
    };
    if seated.session_id != session_id {
        let at = seated.session_id;
        eprintln!("[terminals] stream for {session_id} asked while seated at {at}: ignored");
        return;
    }
    seat.channel = Some(channel);
    seat.change = SeatChange::Stream(seated.short_id);
}

/// The header naming the session the overlay typed into: input meant for a seat the
/// player has just left never reaches the next one.
const SESSION_HEADER: &str = "session-id";

/// **terminal input**: what the player typed in the seated view, as the overlay's
/// terminal encodes it (keys, pastes, mouse reports and its query replies), as the raw
/// request body, for the session named by the `Session-Id` header. Written to the
/// seated session's attach only (spec "Keys, paste and links"); ignored while nobody
/// is seated, for any other session, and before the overlay asked for its stream.
#[tauri::command]
pub fn terminal_input(terminals: State<'_, SessionTerminals>, request: Request<'_>) {
    let Some(shared) = &terminals.0 else { return };
    let InvokeBody::Raw(bytes) = request.body() else {
        eprintln!("[terminals] typing that is not raw bytes: ignored");
        return;
    };
    let session_id = request
        .headers()
        .get(SESSION_HEADER)
        .and_then(|value| value.to_str().ok());
    let Some(seated) = shared.seat.lock().unwrap().seated.clone() else {
        return;
    };
    if session_id != Some(seated.session_id.as_str()) {
        return;
    }
    // The terminals take a new seat on their next tick: until then, they may still be
    // seated at the last session.
    let terminals = shared.terminals.lock().unwrap();
    if terminals.seated() == Some(seated.short_id.as_str()) {
        terminals.input(bytes);
    }
}

/// Lets the main webview's own script copy and paste through `document.execCommand`
/// (WebKitGTK's `javascript-can-access-clipboard`, off unless asked for): the seated
/// terminal's Ctrl+Shift+C and Ctrl+Shift+V (spec "Keys, paste and links"). The page
/// runs only the app's own script (`script-src 'self'`). Called once, at setup.
pub fn allow_clipboard(app: &AppHandle) {
    use webkit2gtk::{SettingsExt, WebViewExt};
    let Some(main) = app.get_webview_window("main") else {
        eprintln!("[terminals] no main window to allow the clipboard on");
        return;
    };
    let reached = main.with_webview(|platform| {
        if let Some(settings) = platform.inner().settings() {
            settings.set_javascript_can_access_clipboard(true);
        }
    });
    if let Err(error) = reached {
        eprintln!("[terminals] could not reach the main webview for the clipboard: {error}");
    }
}

impl SessionScreens for Shared {
    fn set_live(&self, short_ids: &BTreeSet<String>) {
        *self.asked.lock().unwrap() = Some(short_ids.clone());
    }

    fn screens(&self, short_ids: &[&str]) -> Option<Vec<Option<Screen>>> {
        let terminals = self.terminals.try_lock().ok()?;
        Some(
            short_ids
                .iter()
                .map(|id| {
                    terminals.image(id).map(|image| Screen {
                        image,
                        attached: terminals.is_attached(id),
                    })
                })
                .collect(),
        )
    }

    fn shutdown(&self) {
        *self.asked.lock().unwrap() = None;
        *self.seat.lock().unwrap() = SeatStream::default();
        self.terminals.lock().unwrap().shutdown();
        eprintln!("[terminals] shut down: every attach hung up");
    }
}
