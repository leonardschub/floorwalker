//! The agent registry's commands (spec "The data contract"; ADR-0010). The registry
//! itself, with everything it reads and decides, is the Tauri- and Bevy-free `agents`
//! crate; this module only holds it as managed state, starts its first build off the
//! main thread when the page or the world first asks, then keeps it watched, and carries
//! each snapshot to the page and to the world (spec: "Data: from the registry to the
//! world"). The statistics commands are in `statistics.rs`.
//!
//! In a debug build, `WORLD_DEBUG_TIMELINE=<file>` replaces the registry's tree with a
//! scripted timeline (`timeline.rs`) along the same path; the registry still builds,
//! for the statistics views.
//!
//! What the CCTV room asks of a real session reaches [`Agents::session_request`]: a
//! kill's blast ends the session (ADR-0013), work harder's bolt sends "WORK HARDER" into
//! it as a prompt (ADR-0014).
//!
//! New session (agent-screens ticket 15) reaches [`session_new`]: `claude --bg` in the
//! project's folder, through the registry, answering the short id the page then waits
//! to see in the tree.
//!
//! The commands are declared in `build.rs`'s app manifest and granted only to `main`.
//! The page's side of them is `web/src/agents/tauri-client.ts`.

use std::path::PathBuf;
#[cfg(debug_assertions)]
use std::sync::Mutex;
use std::sync::{Arc, OnceLock};

use agents::{
    resolve_claude, AgentTreeSnapshot, ClaudeAgentsCommand, Deadline, LedgerTiming, LedgerWatch,
    NewSessionError, Registry, Sources, SystemClock, SystemProcesses, Timing, Watching,
    NEW_SESSION_WITHIN,
};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::world::{Controller, SessionAction, SessionRequest};

/// The event that carries the whole agent tree to the main window: once when the first
/// build has finished, then whenever the tree changes, at most about once per second.
const SNAPSHOT_EVENT: &str = "agents-snapshot";

/// What work harder sends into the session at its bolt (ADR-0014).
const WORK_HARDER: &str = "WORK HARDER";

/// How long a session's process has after SIGTERM before SIGKILL (ADR-0013).
const END_GRACE: std::time::Duration = std::time::Duration::from_secs(5);

/// How often the registry looks for a new session (`session_new`).
const AWAIT_EVERY: std::time::Duration = std::time::Duration::from_millis(250);

/// The event that tells the main window opencodex's ledger has grown, sent only while
/// a statistics view keeps asking (see [`LedgerWatch`]).
const LEDGER_GREW_EVENT: &str = "opencodex-ledger-grew";

/// The registry as managed state, its watch once the first build has finished, and
/// the watch on opencodex's ledger for the open statistics views.
pub struct Agents {
    registry: Arc<Registry>,
    watching: OnceLock<Watching>,
    ledger_watch: LedgerWatch,
    /// The scripted timeline, when one replaces the registry's tree, and the tree it
    /// played last.
    #[cfg(debug_assertions)]
    timeline: Option<(PathBuf, Mutex<AgentTreeSnapshot>)>,
}

/// Hands a snapshot to both views: the page as an `agents-snapshot` event, and the world
/// controller, which keeps it for a world launched later.
fn publish(app: &AppHandle, tree: AgentTreeSnapshot) {
    let tree = Arc::new(tree);
    app.state::<Controller>().tree(tree.clone());
    let _ = app.emit_to("main", SNAPSHOT_EVENT, &*tree);
}

/// The user's home directory: where `~/.claude` and `~/.opencodex` are read, and
/// `~/.local/bin/claude` looked for.
pub(crate) fn home(app: &AppHandle) -> PathBuf {
    app.path()
        .home_dir()
        .unwrap_or_else(|_| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()))
}

impl Agents {
    /// The registry over the user's home. Making it reads nothing, and the `claude`
    /// binary is looked up only when liveness is first asked for: the first build waits
    /// for the page's first request.
    pub fn new(app: &AppHandle) -> Self {
        let home = home(app);
        let sources = Sources::in_home(&home);
        let path = std::env::var_os("PATH");
        let events = app.clone();
        Agents {
            ledger_watch: LedgerWatch::new(
                &sources.opencodex_dir,
                LedgerTiming::default(),
                move || {
                    let _ = events.emit_to("main", LEDGER_GREW_EVENT, ());
                },
            ),
            registry: Arc::new(Registry::new(
                sources,
                Box::new(SystemClock),
                Box::new(ClaudeAgentsCommand::resolve(path.as_deref(), &home)),
            )),
            watching: OnceLock::new(),
            #[cfg(debug_assertions)]
            timeline: crate::timeline::from_env().map(|file| {
                eprintln!(
                    "[timeline] {} replaces the agent registry's tree",
                    file.display()
                );
                (file, Mutex::new(AgentTreeSnapshot::loading()))
            }),
        }
    }

    /// The agent tree as it is now.
    fn tree(&self) -> AgentTreeSnapshot {
        #[cfg(debug_assertions)]
        if let Some((_, played)) = &self.timeline {
            return played.lock().unwrap().clone();
        }
        self.registry.snapshot()
    }

    /// Starts the registry's first build on its own thread, unless it has been
    /// started: the ready tree then goes to the page as an `agents-snapshot` event and
    /// to the world, and the watch that keeps it current begins (it sends the tree
    /// again once each agent's tokens are attributed).
    pub(crate) fn start_first_build(&self, app: &AppHandle) {
        if !self.registry.claim_build() {
            return;
        }
        #[cfg(debug_assertions)]
        if let Some((file, _)) = &self.timeline {
            self.play_timeline(app, file.clone());
            let building = self.registry.clone();
            let spawned = std::thread::Builder::new()
                .name("agent-registry".into())
                .spawn(move || building.build());
            if let Err(error) = spawned {
                eprintln!("agent registry: could not start its build thread: {error}");
            }
            return;
        }
        let building = self.registry.clone();
        let app = app.clone();
        let build = move || {
            building.build();
            publish(&app, building.snapshot());
            let events = app.clone();
            let watching = building.watch(Timing::default(), move |tree| publish(&events, tree));
            let _ = app.state::<Agents>().watching.set(watching);
        };
        if let Err(error) = std::thread::Builder::new()
            .name("agent-registry".into())
            .spawn(build)
        {
            eprintln!("agent registry: could not start its build thread: {error}");
        }
    }

    /// Plays the timeline in `file` on its own thread, each step to both views.
    #[cfg(debug_assertions)]
    fn play_timeline(&self, app: &AppHandle, file: PathBuf) {
        let app = app.clone();
        let play = move || {
            let timeline = match crate::timeline::Timeline::read(&file) {
                Ok(timeline) => timeline,
                Err(error) => {
                    eprintln!("[timeline] cannot play {}: {error}", file.display());
                    return;
                }
            };
            timeline.play(|tree| {
                if let Some((_, played)) = &app.state::<Agents>().timeline {
                    *played.lock().unwrap() = tree.clone();
                }
                publish(&app, tree);
            });
        };
        if let Err(error) = std::thread::Builder::new()
            .name("agent-timeline".into())
            .spawn(play)
        {
            eprintln!("[timeline] could not start its thread: {error}");
        }
    }

    /// The CCTV room asks something of the session `request.session`: done on a thread of
    /// its own, through the registry, which finds the session's `claude` process and acts
    /// only once it is verified. A kill's blast ends the session (ADR-0013; see
    /// [`Registry::end_session`]): the watch then sees it go. Work harder's bolt sends
    /// "WORK HARDER" into it as a prompt (ADR-0014; see [`Registry::prompt_session`]). A
    /// scripted timeline's sessions are never real, so with one playing nothing is done.
    pub(crate) fn session_request(&self, request: SessionRequest) {
        let SessionRequest { session, action } = request;
        #[cfg(debug_assertions)]
        if self.timeline.is_some() {
            eprintln!(
                "[agents] {action:?} on session {session}: the tree is a scripted timeline, nothing done"
            );
            return;
        }
        let registry = self.registry.clone();
        let asked = session.clone();
        let spawned = std::thread::Builder::new()
            .name("session-request".into())
            .spawn(move || match action {
                SessionAction::End => {
                    let ending = registry.end_session(&session, &SystemProcesses, END_GRACE);
                    eprintln!("[agents] end session {session}: {ending:?}");
                }
                SessionAction::WorkHarder => {
                    let prompting =
                        registry.prompt_session(&session, WORK_HARDER, &SystemProcesses);
                    eprintln!("[agents] prompt session {session}: {prompting:?}");
                }
            });
        if let Err(error) = spawned {
            eprintln!(
                "[agents] {action:?} on session {asked}: could not start its thread: {error}"
            );
        }
    }

    /// A statistics view asked: its answer from the registry, with the ledger watched
    /// for another lease so the open view hears when the ledger grows.
    pub(crate) fn statistics<R>(&self, ask: impl FnOnce(&Registry) -> R) -> R {
        self.ledger_watch.renew();
        ask(&self.registry)
    }
}

/// **snapshot**: the agent tree as it is now. The first call starts the registry's
/// build on its own thread and answers *loading*; the ready tree follows as an
/// `agents-snapshot` event, and so does every change after it.
#[tauri::command]
pub fn agents_snapshot(app: AppHandle, agents: State<'_, Agents>) -> AgentTreeSnapshot {
    agents.start_first_build(&app);
    agents.tree()
}

/// A session New session started and the tree now lists: the page sits the user in it.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartedSession {
    short_id: String,
    session_id: String,
}

/// **New session**: starts a background session in the project `project_id`, with no
/// prompt, name or flags (the user's Claude Code settings decide), in the working
/// directory of its newest session, then waits for the tree to list it: the registry
/// looks for its session file ([`Registry::await_session`]), and the tree that lists it
/// is published before the answer, so the world has its desk when the page sits the
/// user in it. Answers the session, or why there is none: an untrusted folder, Claude
/// Code's own message, or no desk within [`NEW_SESSION_WITHIN`] of the press (one
/// deadline for `claude --bg` and the wait; the page keeps no clock of its own). Runs
/// off the main thread. `claude --bg` is real even under a scripted timeline (the
/// folder is the registry's); the timeline's tree is left alone, so the session is
/// never listed there.
#[tauri::command]
pub async fn session_new(
    app: AppHandle,
    project_id: String,
) -> Result<StartedSession, NewSessionError> {
    let deadline = Deadline::after(NEW_SESSION_WITHIN);
    let registry = app.state::<Agents>().registry.clone();
    let claude = resolve_claude(std::env::var_os("PATH").as_deref(), &home(&app));
    #[cfg(debug_assertions)]
    let scripted = app.state::<Agents>().timeline.is_some();
    #[cfg(not(debug_assertions))]
    let scripted = false;
    tauri::async_runtime::spawn_blocking(move || {
        let asked = std::time::Instant::now();
        let short_id = registry.new_session(&project_id, claude.as_deref(), deadline)?;
        let listed = if scripted {
            None
        } else {
            registry.await_session(&short_id, AWAIT_EVERY, deadline)
        };
        let Some(listed) = listed else {
            eprintln!("[agents] new session {short_id}: not listed in time");
            return Err(deadline.timed_out());
        };
        let after = asked.elapsed().as_millis();
        eprintln!("[agents] new session {short_id}: listed after {after} ms");
        publish(&app, listed.tree);
        Ok(StartedSession {
            short_id,
            session_id: listed.session_id,
        })
    })
    .await
    .unwrap_or_else(|error| {
        Err(NewSessionError::Failed {
            message: error.to_string(),
        })
    })
}
