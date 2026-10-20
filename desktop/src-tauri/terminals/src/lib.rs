//! Session terminals (ADR-0015): every `claude attach` the app runs, and what they draw.
//!
//! [`Terminals`] is told which background sessions (by short id) should be live and
//! opens and hangs up attaches to match, each under a PTY of its own at a fixed
//! [`COLUMNS`] × [`ROWS`] ([`pty`]). Each attach's output feeds an `alacritty_terminal`
//! grid that never writes anything back, and [`Terminals::tick`] paints each grid that
//! changed into the session's [`PcImage`], which outlives the attach.
//!
//! An attach can leave its session without exiting: `←`, `/exit` or a double Ctrl+C on
//! an empty prompt turn it into agent view, which can dispatch to, reply to and delete
//! any background session (research §4); the session stopping makes it exit (Claude
//! Code 2.1.303) or show agent view. Agent view never reaches a PC image: the tick hangs
//! such an attach up, reports it, and attaches again while the session is still live.
//! Since `claude attach` starts a stopped or dormant session again, the crate attaches
//! only to sessions whose process runs (`running`).
//!
//! While the user sits in a session (the seated view), the crate also collects that
//! attach's raw output for the overlay ([`Terminals::seat`], [`Terminals::seated_output`])
//! and retakes its size so it redraws its whole screen; what the user types there is
//! written to that attach alone ([`Terminals::input`]).
//!
//! The crate has no Tauri or Bevy types; the `claude` binary is injectable, so tests run
//! a fake attach.

mod attach;
mod paint;
pub mod pty;
mod running;

use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use attach::{Attach, Ticked};
use paint::Painter;

pub use attach::Leaving;

pub use paint::{PC_IMAGE_HEIGHT, PC_IMAGE_WIDTH};

/// The fixed size of every attach's terminal (spec "The terminals crate").
pub const COLUMNS: u16 = 100;
pub const ROWS: u16 = 30;

/// A session's PC image: [`PC_IMAGE_WIDTH`] × [`PC_IMAGE_HEIGHT`] RGBA, rows top first.
#[derive(Clone, Debug)]
pub struct PcImage {
    pub rgba: Arc<[u8]>,
    /// Counts this session's paints: a new number means a new picture.
    pub serial: u64,
}

/// An attach that left its session, as a tick reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LeftSession {
    pub short_id: String,
    pub why: Leaving,
}

/// A session's attaches start at most this often, so one that keeps leaving its
/// session (or failing to start) is not restarted in a loop.
const REATTACH_AFTER: Duration = Duration::from_secs(2);

/// How long a size retake holds the taller size before going back: long enough that
/// Claude Code sees the change (it redraws only when its size changes; ticket 10's
/// prove-first 3).
const RETAKE_HOLD: Duration = Duration::from_millis(400);

/// The seated session, and when its size retake goes back to [`ROWS`].
struct Seated {
    short_id: String,
    back_at: Option<Instant>,
}

/// Every attach the app runs.
pub struct Terminals {
    claude: PathBuf,
    /// Claude Code's `sessions` directory, to tell which sessions' processes run.
    sessions: PathBuf,
    /// The sessions that should be live, by short id.
    live: BTreeSet<String>,
    /// The attach of each live session that has one.
    attaches: HashMap<String, Attach>,
    /// The last image painted for every session ever attached, live or not.
    images: HashMap<String, PcImage>,
    /// When each live session's latest attach was started.
    opened_at: HashMap<String, Instant>,
    /// Live sessions found without a running process, logged once until that changes.
    not_running: BTreeSet<String>,
    painter: Painter,
    seated: Option<Seated>,
}

impl Terminals {
    /// No attach yet; `claude` is the binary run as `claude attach <id>`, and
    /// `sessions` Claude Code's `~/.claude/sessions`.
    pub fn new(claude: impl Into<PathBuf>, sessions: impl Into<PathBuf>) -> Terminals {
        Terminals {
            claude: claude.into(),
            sessions: sessions.into(),
            live: BTreeSet::new(),
            attaches: HashMap::new(),
            images: HashMap::new(),
            opened_at: HashMap::new(),
            not_running: BTreeSet::new(),
            painter: Painter::new(),
            seated: None,
        }
    }

    /// Makes `short_ids` the live set: hangs up the attaches of sessions that left it at
    /// once; the next tick attaches to sessions that joined it. A session that stays
    /// keeps its attach.
    pub fn set_live<I>(&mut self, short_ids: I)
    where
        I: IntoIterator,
        I::Item: Into<String>,
    {
        self.live = short_ids.into_iter().map(Into::into).collect();
        let leaving: Vec<String> = self
            .attaches
            .keys()
            .filter(|id| !self.live.contains(*id))
            .cloned()
            .collect();
        for id in leaving {
            if let Some(attach) = self.attaches.remove(&id) {
                attach.hang_up();
            }
        }
        self.opened_at.retain(|id, _| self.live.contains(id));
        self.not_running.retain(|id| self.live.contains(id));
    }

    /// Makes `short_id` the seated session (spec "Seated stream", "Size retake"), or
    /// none. From now on its attach's raw output is collected for the overlay
    /// ([`Terminals::seated_output`]) and its size retaken: 100 × 31, then 100 × 30 a
    /// moment later, so Claude Code clears and redraws its whole screen, which fills
    /// the fresh overlay. A seated session not attached yet streams its attach from its
    /// first byte instead (it draws everything as it starts). The seated session must be
    /// in the live set to be attached.
    pub fn seat(&mut self, short_id: Option<&str>, now: Instant) {
        if let Some(old) = self.seated.take() {
            if let Some(attach) = self.attaches.get(&old.short_id) {
                attach.tap(false);
                if old.back_at.is_some() {
                    attach.resize(COLUMNS, ROWS);
                }
            }
        }
        let Some(id) = short_id else { return };
        let mut seated = Seated {
            short_id: id.to_owned(),
            back_at: None,
        };
        if let Some(attach) = self.attaches.get(id) {
            attach.tap(true);
            attach.resize(COLUMNS, ROWS + 1);
            seated.back_at = Some(now + RETAKE_HOLD);
        }
        eprintln!(
            "[terminals] seated at {id}{}",
            if seated.back_at.is_some() {
                ": retaking its size"
            } else {
                ": streaming its attach from the start"
            }
        );
        self.seated = Some(seated);
    }

    /// The seated session's short id, if any.
    pub fn seated(&self) -> Option<&str> {
        self.seated.as_ref().map(|seated| seated.short_id.as_str())
    }

    /// The seated attach's raw output since the last call; nothing while nobody is
    /// seated.
    pub fn seated_output(&mut self) -> Vec<u8> {
        self.seated
            .as_ref()
            .and_then(|seated| self.attaches.get(&seated.short_id))
            .map(Attach::take_tapped)
            .unwrap_or_default()
    }

    /// Writes what the user typed in the seated view (keys, pastes and mouse reports as
    /// the overlay's terminal encodes them, and its query replies) to the seated
    /// session's attach, and to no other. Returns whether it was written: nothing is
    /// while nobody is seated, the seated session has no attach yet, or its attach has
    /// left the alternate screen (it is leaving its session, maybe for agent view, which
    /// must never get a key: stories 21, 48).
    pub fn input(&self, bytes: &[u8]) -> bool {
        self.seated
            .as_ref()
            .and_then(|seated| self.attaches.get(&seated.short_id))
            .is_some_and(|attach| attach.write(bytes))
    }

    /// Whether the session `short_id` has an attach open now.
    pub fn is_attached(&self, short_id: &str) -> bool {
        self.attaches.contains_key(short_id)
    }

    /// Twice a second per attach, checks whether it left its session (and hangs it up
    /// and reports it if so); repaints the PC image of every attach whose grid changed,
    /// at most twice a second each; and attaches to live sessions without an attach
    /// whose process runs, each at most every [`REATTACH_AFTER`]. The caller drives it
    /// with the time, often (every frame is fine).
    pub fn tick(&mut self, now: Instant) -> Vec<LeftSession> {
        let mut left = Vec::new();
        for (id, attach) in &mut self.attaches {
            match attach.tick(&mut self.painter, now) {
                Ticked::Unchanged => {}
                Ticked::Painted(canvas) => {
                    let serial = self.images.get(id).map_or(1, |image| image.serial + 1);
                    self.images.insert(
                        id.clone(),
                        PcImage {
                            rgba: canvas.into(),
                            serial,
                        },
                    );
                }
                Ticked::Left(why) => left.push(LeftSession {
                    short_id: id.clone(),
                    why,
                }),
            }
        }
        for gone in &left {
            eprintln!(
                "[terminals] attach {}: left the session ({:?}), hanging up",
                gone.short_id, gone.why
            );
            if let Some(attach) = self.attaches.remove(&gone.short_id) {
                attach.hang_up();
            }
        }
        if let Some(seated) = self.seated.as_mut() {
            if seated.back_at.is_some_and(|at| now >= at) {
                seated.back_at = None;
                if let Some(attach) = self.attaches.get(&seated.short_id) {
                    attach.resize(COLUMNS, ROWS);
                }
            }
        }
        for id in &self.live {
            let due = self
                .opened_at
                .get(id)
                .is_none_or(|opened| now >= *opened + REATTACH_AFTER);
            if self.attaches.contains_key(id) || !due {
                continue;
            }
            self.opened_at.insert(id.clone(), now);
            if !running::running(&self.sessions, id) {
                if self.not_running.insert(id.clone()) {
                    eprintln!("[terminals] attach {id}: no running process, not attaching");
                }
                continue;
            }
            self.not_running.remove(id);
            let tapped = self
                .seated
                .as_ref()
                .is_some_and(|seated| &seated.short_id == id);
            match Attach::open(&self.claude, id, now, tapped) {
                Ok(attach) => {
                    eprintln!("[terminals] attach {id}: started (pid {})", attach.pid());
                    self.attaches.insert(id.clone(), attach);
                }
                Err(error) => eprintln!("[terminals] attach {id}: could not start: {error}"),
            }
        }
        left
    }

    /// Hangs up every attach and empties the live set, so nothing attaches again until
    /// a new one is set: for quitting, signing out, a world restart or the world screen
    /// going away. Every session keeps running (research §4). Dropping the terminals
    /// does the same.
    pub fn shutdown(&mut self) {
        self.seated = None;
        self.live.clear();
        self.opened_at.clear();
        self.not_running.clear();
        for (_, attach) in self.attaches.drain() {
            attach.hang_up();
        }
    }

    /// The session's last painted PC image, whether its attach is still open or not.
    pub fn image(&self, short_id: &str) -> Option<PcImage> {
        self.images.get(short_id).cloned()
    }

    /// The text on the session's grid now, a line per row; `None` without an attach.
    pub fn screen_text(&self, short_id: &str) -> Option<String> {
        self.attaches.get(short_id).map(Attach::screen_text)
    }
}
