//! Keeping the registry current and pushing its changes (ticket 14; spec "The data
//! contract": the agent tree changes event).
//!
//! A watch runs two threads. One owns a file watcher on `~/.claude/projects` (recursive,
//! so subagent folders are covered), feeds each changed path to
//! [`Registry::files_changed`], and pushes the snapshot to the sink when it differs from
//! the last one pushed, at most once per [`Timing::min_interval`]: the first change after
//! a quiet spell goes out at once, later ones wait for the interval to pass. The other
//! rechecks liveness every [`Timing::liveness_every`] (it may run `claude`, which can be
//! slow), and sooner when a file in `~/.claude/sessions` changes (each running Claude
//! Code rewrites its own there when it turns busy or idle); after each recheck the
//! snapshot is compared again, which also lets sessions age out of the 24-hour window.
//!
//! The watcher reports only that a path changed; what changed is read by the registry,
//! from where its last read stopped. When the watcher loses track (its queue
//! overflowed, or it failed), every transcript is read again the same way. A folder
//! that does not exist yet is watched once it does.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};

use crate::{AgentTreeSnapshot, Registry};

/// How often a watch looks and pushes.
#[derive(Clone, Copy, Debug)]
pub struct Timing {
    /// The least time between two pushed snapshots.
    pub min_interval: Duration,
    /// How often liveness is rechecked when nothing in `~/.claude/sessions` changes.
    pub liveness_every: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Timing {
            min_interval: Duration::from_secs(1),
            liveness_every: Duration::from_secs(3),
        }
    }
}

/// A running watch. Dropping it stops the watch and drops its sink; a liveness check
/// still under way finishes in the background first.
pub struct Watching {
    stop: Arc<AtomicBool>,
    wake: mpsc::Sender<Message>,
    pusher: Option<JoinHandle<()>>,
}

impl Drop for Watching {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = self.wake.send(Message::Stop);
        if let Some(pusher) = self.pusher.take() {
            let _ = pusher.join();
        }
    }
}

enum Message {
    Changed(Vec<PathBuf>),
    LostTrack,
    /// Liveness was rechecked, or time passed: compare the snapshot again.
    Recheck,
    Stop,
}

/// The folders watched, and where their changes go.
struct Folders {
    /// `~/.claude/projects`, recursively: changes go to the registry's reader.
    projects: PathBuf,
    /// `~/.claude/sessions`: a change asks for liveness to be rechecked now.
    sessions: PathBuf,
}

pub(crate) fn watch(
    registry: Arc<Registry>,
    timing: Timing,
    sink: impl Fn(AgentTreeSnapshot) + Send + 'static,
) -> Watching {
    let (wake, messages) = mpsc::channel();
    let (recheck_now, rechecks) = mpsc::channel::<()>();
    let stop = Arc::new(AtomicBool::new(false));
    let claude_dir = &registry.sources.claude_dir;
    let folders = Arc::new(Folders {
        projects: claude_dir.join("projects"),
        sessions: claude_dir.join("sessions"),
    });

    // The tree as the watch starts, taken before anything below can change it: the
    // baseline later snapshots are compared with.
    let baseline = registry.snapshot();
    let pusher = {
        let registry = registry.clone();
        let events = wake.clone();
        std::thread::Builder::new()
            .name("agent-registry-watch".into())
            .spawn(move || {
                push_changes(
                    &registry,
                    folders,
                    timing,
                    events,
                    recheck_now,
                    messages,
                    (baseline, sink),
                )
            })
            .ok()
    };

    {
        let stop = stop.clone();
        let wake = wake.clone();
        let spawned = std::thread::Builder::new()
            .name("agent-registry-liveness".into())
            .spawn(move || {
                // Each agent's tokens are read from opencodex's ledger here, off the
                // tree's way: first at once, then after each liveness check.
                registry.attribute_tokens();
                if wake.send(Message::Recheck).is_err() {
                    return;
                }
                loop {
                    match rechecks.recv_timeout(timing.liveness_every) {
                        Ok(()) | Err(mpsc::RecvTimeoutError::Timeout) => {}
                        Err(mpsc::RecvTimeoutError::Disconnected) => return,
                    }
                    // Several changes at once ask for one recheck.
                    while rechecks.try_recv().is_ok() {}
                    if stop.load(Ordering::Relaxed) {
                        return;
                    }
                    registry.refresh_liveness();
                    registry.attribute_tokens();
                    if wake.send(Message::Recheck).is_err() {
                        return;
                    }
                }
            });
        if let Err(error) = spawned {
            eprintln!("agent registry: could not start rechecking liveness: {error}");
        }
    }

    Watching { stop, wake, pusher }
}

fn push_changes(
    registry: &Registry,
    folders: Arc<Folders>,
    timing: Timing,
    events: mpsc::Sender<Message>,
    recheck_now: mpsc::Sender<()>,
    messages: mpsc::Receiver<Message>,
    (mut pushed, sink): (AgentTreeSnapshot, impl Fn(AgentTreeSnapshot)),
) {
    let mut watcher = start_watcher(folders.clone(), events, recheck_now);
    let mut watching_projects = false;
    let mut watching_sessions = false;
    // Starts watching what exists by now; true when the projects folder just started,
    // so that anything written to it before is read.
    let mut ensure_watching = |watcher: &mut Option<RecommendedWatcher>| {
        let Some(watcher) = watcher else {
            return false;
        };
        if !watching_sessions && folders.sessions.is_dir() {
            watching_sessions =
                watch_folder(watcher, &folders.sessions, RecursiveMode::NonRecursive);
        }
        if !watching_projects && folders.projects.is_dir() {
            watching_projects = watch_folder(watcher, &folders.projects, RecursiveMode::Recursive);
            return watching_projects;
        }
        false
    };
    if ensure_watching(&mut watcher) {
        registry.rescan();
    }

    let mut last_push: Option<Instant> = None;
    let mut dirty = false;
    loop {
        let wait = match (dirty, last_push) {
            (true, Some(at)) => {
                (at + timing.min_interval).saturating_duration_since(Instant::now())
            }
            (true, None) => Duration::ZERO,
            (false, _) => Duration::from_secs(3600),
        };
        match messages.recv_timeout(wait) {
            Ok(Message::Changed(paths)) => {
                registry.files_changed(&paths);
                dirty = true;
            }
            Ok(Message::LostTrack) => {
                registry.rescan();
                dirty = true;
            }
            Ok(Message::Recheck) => {
                if ensure_watching(&mut watcher) {
                    registry.rescan();
                }
                dirty = true;
            }
            Ok(Message::Stop) | Err(mpsc::RecvTimeoutError::Disconnected) => return,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        let due = last_push.is_none_or(|at| at.elapsed() >= timing.min_interval);
        if dirty && due {
            dirty = false;
            let snapshot = registry.snapshot();
            if snapshot != pushed {
                pushed = snapshot.clone();
                last_push = Some(Instant::now());
                sink(snapshot);
            }
        }
    }
}

fn watch_folder(watcher: &mut RecommendedWatcher, folder: &Path, mode: RecursiveMode) -> bool {
    match watcher.watch(folder, mode) {
        Ok(()) => true,
        Err(error) => {
            eprintln!(
                "agent registry: could not watch {}: {error}",
                folder.display()
            );
            false
        }
    }
}

fn start_watcher(
    folders: Arc<Folders>,
    events: mpsc::Sender<Message>,
    recheck_now: mpsc::Sender<()>,
) -> Option<RecommendedWatcher> {
    let started = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        let event = match event {
            // Opening and reading files, the registry's own reads included, change nothing.
            Ok(event) if matches!(event.kind, EventKind::Access(_)) => return,
            Ok(event) if !event.need_rescan() => event,
            _ => {
                let _ = recheck_now.send(());
                let _ = events.send(Message::LostTrack);
                return;
            }
        };
        let (sessions, projects): (Vec<PathBuf>, Vec<PathBuf>) = event
            .paths
            .into_iter()
            .partition(|path| path.starts_with(&folders.sessions));
        if !sessions.is_empty() {
            let _ = recheck_now.send(());
        }
        if !projects.is_empty() {
            let _ = events.send(Message::Changed(projects));
        }
    });
    match started {
        Ok(watcher) => Some(watcher),
        Err(error) => {
            eprintln!("agent registry: could not start watching files: {error}");
            None
        }
    }
}
