//! One `claude attach <id>` and the grid its output feeds.

use std::io;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use alacritty_terminal::event::VoidListener;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::term::{Config, Term, TermDamage, TermMode};
use alacritty_terminal::vte::ansi::Processor;

use crate::paint::{Painter, PC_IMAGE_HEIGHT, PC_IMAGE_WIDTH};
use crate::pty::Pty;
use crate::{COLUMNS, ROWS};

/// The PC image is repainted at most twice a second (spec "The PC screen").
const PAINT_INTERVAL: Duration = Duration::from_millis(500);

/// How often an attach is checked for having left its session (spec "Agent view
/// detection").
const CHECK_INTERVAL: Duration = Duration::from_millis(500);

/// Agent view's dispatch prompt, the backup sign that an attach shows agent view.
const AGENT_VIEW_TEXT: &str = "describe a task for a new session";

/// Why an attach no longer shows its session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Leaving {
    /// It turned into agent view: retitled `claude agents`, or agent view's text is on
    /// its grid.
    AgentView,
    /// Its process exited.
    Exited,
}

/// What a tick did to an attach.
pub(crate) enum Ticked<'a> {
    Unchanged,
    /// The PC image was repainted: here it is.
    Painted(&'a [u8]),
    /// The attach left its session; nothing was painted.
    Left(Leaving),
}

/// The grid an attach's reader thread feeds. `VoidListener` drops the terminal's query
/// replies: nothing is ever written back to the attach (spec "One grid per attach").
struct Grid {
    term: Term<VoidListener>,
    processor: Processor,
    /// Whether output arrived since the last paint.
    changed: bool,
}

/// Entering and leaving the alternate screen. The attach draws its session there, and
/// leaves it first as it turns into agent view or exits (ticket 10's fixtures), so
/// nothing after the leave is the session's.
const ENTER_ALT_SCREEN: &[u8] = b"\x1b[?1049h";
const LEAVE_ALT_SCREEN: &[u8] = b"\x1b[?1049l";

/// What an attach's reader thread has seen of the alternate screen, and the raw output
/// collected for the seated view while it is on (spec "Seated stream").
///
/// Once the attach has left the alternate screen, it has left its session for good:
/// collecting ends there, since agent view, drawn next, must never reach the overlay
/// (spec "The seated view"), and nothing typed reaches the attach any more, since agent
/// view can dispatch to or delete sessions (stories 21, 48). The tick notices the attach
/// left its session only up to [`CHECK_INTERVAL`] later, and hangs it up then.
#[derive(Default)]
struct Tapped {
    /// The output since the last take, while tapped.
    collected: Option<Vec<u8>>,
    /// Whether the attach has drawn on the alternate screen.
    entered: bool,
    /// Whether it has left the alternate screen since.
    ended: bool,
    /// The last few bytes seen, for an enter or a leave split across two reads.
    tail: Vec<u8>,
}

fn find(bytes: &[u8], part: &[u8], from: usize) -> Option<usize> {
    bytes
        .get(from..)?
        .windows(part.len())
        .position(|window| window == part)
        .map(|at| at + from)
}

impl Tapped {
    /// Starts or stops collecting, dropping what was collected; what was seen of the
    /// alternate screen stays.
    fn tap(&mut self, on: bool) {
        self.collected = on.then(Vec::new);
    }

    /// Watches `bytes` for the attach entering and leaving the alternate screen, and
    /// collects them while tapped, up to the leave and never after.
    fn feed(&mut self, bytes: &[u8]) {
        if self.ended {
            return;
        }
        let mut seen = std::mem::take(&mut self.tail);
        let before = seen.len();
        seen.extend_from_slice(bytes);
        let mut from = 0;
        if !self.entered {
            if let Some(at) = find(&seen, ENTER_ALT_SCREEN, 0) {
                self.entered = true;
                from = at + ENTER_ALT_SCREEN.len();
            }
        }
        let leave = if self.entered {
            find(&seen, LEAVE_ALT_SCREEN, from)
        } else {
            None
        };
        if leave.is_some() {
            self.ended = true;
        }
        let Some(collected) = self.collected.as_mut() else {
            if leave.is_none() {
                self.keep_tail(seen);
            }
            return;
        };
        match leave {
            Some(at) if at < before => {
                // The leave began in the last read, whose bytes are collected already.
                let keep = collected.len().saturating_sub(before - at);
                collected.truncate(keep);
            }
            Some(at) => collected.extend_from_slice(&bytes[..at - before]),
            None => {
                collected.extend_from_slice(bytes);
                self.keep_tail(seen);
            }
        }
    }

    /// Keeps the end of `seen` that could begin an enter or a leave.
    fn keep_tail(&mut self, mut seen: Vec<u8>) {
        let from = seen.len().saturating_sub(LEAVE_ALT_SCREEN.len() - 1);
        self.tail = seen.split_off(from);
    }
}

type Tap = Arc<Mutex<Tapped>>;

pub(crate) struct Attach {
    pty: Pty,
    grid: Arc<Mutex<Grid>>,
    tap: Tap,
    /// The PC image as last painted, kept to repaint only damaged lines.
    canvas: Vec<u8>,
    painted_at: Option<Instant>,
    /// When it was last checked for having left its session.
    checked_at: Instant,
}

impl Attach {
    /// Starts `claude attach <short_id>`; with `tapped`, its raw output is collected
    /// from its first byte ([`Attach::take_tapped`]).
    pub(crate) fn open(
        claude: &Path,
        short_id: &str,
        now: Instant,
        tapped: bool,
    ) -> io::Result<Attach> {
        // Attached sessions draw fullscreen and scroll themselves: the grid keeps no
        // history of its own.
        let config = Config {
            scrolling_history: 0,
            ..Config::default()
        };
        let size = TermSize::new(COLUMNS.into(), ROWS.into());
        let grid = Arc::new(Mutex::new(Grid {
            term: Term::new(config, &size, VoidListener),
            processor: Processor::new(),
            changed: false,
        }));
        let fed = grid.clone();
        let mut tapped_from_start = Tapped::default();
        tapped_from_start.tap(tapped);
        let tap: Tap = Arc::new(Mutex::new(tapped_from_start));
        let tapping = tap.clone();
        let pty = Pty::spawn(claude, &["attach", short_id], COLUMNS, ROWS, move |bytes| {
            lock(&tapping).feed(bytes);
            let mut grid = lock(&fed);
            let Grid {
                term,
                processor,
                changed,
            } = &mut *grid;
            processor.advance(term, bytes);
            *changed = true;
        })?;
        let canvas = vec![0; (PC_IMAGE_WIDTH * PC_IMAGE_HEIGHT * 4) as usize];
        Ok(Attach {
            pty,
            grid,
            tap,
            canvas,
            painted_at: None,
            checked_at: now,
        })
    }

    /// The grid's text, a line per row with trailing blanks trimmed.
    pub(crate) fn screen_text(&self) -> String {
        let grid = lock(&self.grid);
        let grid = grid.term.grid();
        let mut text = String::new();
        for line in 0..grid.screen_lines() {
            let row = &grid[Line(line as i32)];
            let mut chars = String::new();
            for column in 0..grid.columns() {
                let c = row[Column(column)].c;
                chars.push(if c == '\0' { ' ' } else { c });
            }
            text.push_str(chars.trim_end());
            text.push('\n');
        }
        text
    }

    /// Checks whether the attach left its session, every [`CHECK_INTERVAL`] and before
    /// each paint; otherwise repaints the canvas's damaged lines if output arrived since
    /// the last paint and that paint is [`PAINT_INTERVAL`] old at `now`.
    pub(crate) fn tick(&mut self, painter: &mut Painter, now: Instant) -> Ticked<'_> {
        let paint_due = self
            .painted_at
            .is_none_or(|painted| now >= painted + PAINT_INTERVAL)
            && lock(&self.grid).changed;
        if !paint_due && now < self.checked_at + CHECK_INTERVAL {
            return Ticked::Unchanged;
        }
        self.checked_at = now;
        if let Some(why) = self.leaving() {
            return Ticked::Left(why);
        }
        if !paint_due {
            return Ticked::Unchanged;
        }
        let mut grid = lock(&self.grid);
        grid.changed = false;
        // The attach draws its session on the alternate screen, and leaves it first when
        // it exits or turns into agent view: the main screen is never the session's.
        if !grid.term.mode().contains(TermMode::ALT_SCREEN) {
            return Ticked::Unchanged;
        }
        self.painted_at = Some(now);
        let lines: Vec<usize> = match grid.term.damage() {
            TermDamage::Full => (0..ROWS as usize).collect(),
            TermDamage::Partial(damaged) => damaged
                .filter(|line| line.is_damaged())
                .map(|line| line.line)
                .collect(),
        };
        grid.term.reset_damage();
        painter.paint(&grid.term, lines.into_iter(), &mut self.canvas);
        Ticked::Painted(&self.canvas)
    }

    /// Whether the attach has left its session, and how.
    fn leaving(&mut self) -> Option<Leaving> {
        if self.pty.exited() {
            return Some(Leaving::Exited);
        }
        if retitled_agents(self.pty.pid()) || self.screen_text().contains(AGENT_VIEW_TEXT) {
            return Some(Leaving::AgentView);
        }
        None
    }

    /// Starts or stops collecting the raw output (dropping what was collected).
    pub(crate) fn tap(&self, on: bool) {
        lock(&self.tap).tap(on);
    }

    /// The raw output collected since the last call, while tapped, up to the attach
    /// leaving the alternate screen.
    pub(crate) fn take_tapped(&self) -> Vec<u8> {
        lock(&self.tap)
            .collected
            .as_mut()
            .map(std::mem::take)
            .unwrap_or_default()
    }

    /// Writes what the user typed to the attach (spec "Seated stream"), unless the
    /// attach has left the alternate screen: then it has left its session, and the keys
    /// are dropped (see [`Tapped`]). Returns whether they were written.
    pub(crate) fn write(&self, bytes: &[u8]) -> bool {
        if lock(&self.tap).ended {
            return false;
        }
        if let Err(error) = self.pty.write(bytes) {
            eprintln!(
                "[terminals] attach pid {}: could not write the typing: {error}",
                self.pid()
            );
        }
        true
    }

    /// Sets the attach's terminal size; the grid keeps its own.
    pub(crate) fn resize(&self, columns: u16, rows: u16) {
        if let Err(error) = self.pty.resize(columns, rows) {
            eprintln!(
                "[terminals] attach pid {}: could not resize: {error}",
                self.pid()
            );
        }
    }

    pub(crate) fn pid(&self) -> u32 {
        self.pty.pid()
    }

    pub(crate) fn hang_up(self) {
        self.pty.hang_up();
    }
}

/// Whether process `pid` has retitled itself `claude agents`: agent view rewrites the
/// attach's argv to `<claude> agents` (prove-first 2, ticket 10).
fn retitled_agents(pid: u32) -> bool {
    let Ok(cmdline) = std::fs::read(format!("/proc/{pid}/cmdline")) else {
        return false;
    };
    cmdline.split(|byte| *byte == 0).nth(1) == Some(b"agents".as_slice())
}

fn lock<T>(held: &Mutex<T>) -> MutexGuard<'_, T> {
    held.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}
