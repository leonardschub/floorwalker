//! The agent registry (ADR-0010): the one in-process source of the user's agents.
//!
//! It reads Claude Code's files under the default `~/.claude`, read-only, takes
//! liveness from `claude agents --json`, and serves the side panel a metadata-only
//! [`AgentTreeSnapshot`]. Nothing is written anywhere; the index lives in memory and is
//! rebuilt on each app start. The crate has no Tauri or Bevy types, so the desktop
//! shell's commands and, later, the world read the same registry.
//!
//! Besides reading, it does two things to a real session, each only once that session's
//! `claude` process is verified (`session_file`), through [`Processes`], which tests
//! replace with a stand-in: [`Registry::end_session`] ends it, for a kill in the CCTV
//! room (ADR-0013), and [`Registry::prompt_session`] sends it a prompt through Claude
//! Code's own messaging socket, for WORK HARDER (ADR-0014); both amend ADR-0010.
//!
//! Everything it touches from outside is injectable for tests: the directories
//! ([`Sources`]), the time ([`Clock`]) and the liveness source ([`Liveness`]).
//!
//! After the first build, [`Registry::watch`] keeps it current: changed files are read
//! from where the last read stopped, liveness is rechecked every few seconds (falling
//! back to `sessions/<pid>.json` when `claude agents --json` fails or is missing), and
//! the tree is pushed to a sink at most about once per second while it changes.
//!
//! The `opencodex` module reads opencodex's ledger and quota caches for the statistics
//! views: opencodex (15) and Of the month (16). The `tokens` module attributes Claude
//! Code's ledger rows to the agents whose transcripts carry their `requestId` (17), for
//! the Tokens view and for each agent's tokens in the tree.

mod claude_dir;
mod ending;
mod liveness;
mod model;
mod opencodex;
mod prompting;
mod session_file;
mod snapshot;
mod starting;
mod tokens;
mod transcript;
mod watch;

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};

use chrono::{DateTime, FixedOffset, Local, Utc};

pub use ending::{Ending, Processes, Signal, SystemProcesses};
pub use liveness::{
    resolve_claude, ClaudeAgentsCommand, LiveSession, Liveness, LivenessError, RootStatus,
    WaitingFor,
};
pub use opencodex::{
    local_midnight, AccountLabel, Harness, Ledger, LedgerTiming, LedgerWatch, LimitWindow,
    ModelFigures, MonthCard, MonthEntry, OfTheMonth, OfTheMonthAnswer, OpencodexAnswer,
    OpencodexStatistics, OpencodexUnavailable, Range, Summary, Surface, SurfaceFilter, Tokens,
    UsageLimit, UsageRow, WindowKind,
};
pub use prompting::Prompting;
pub use snapshot::{
    Agent, AgentKind, AgentState, AgentTreeSnapshot, Flag, FlagKind, Project, Session, TreeStatus,
};
pub use starting::{Deadline, NewSessionError, NEW_SESSION_WITHIN};
pub use tokens::{
    AgentTokens, Bucket, BucketSize, ProjectTokens, SessionTokens, TokenFigures, TokenStatistics,
    TokensAnswer,
};
pub use watch::{Timing, Watching};

/// Where the registry reads from. Only the default locations are used in the app (a
/// relocated `CLAUDE_CONFIG_DIR` is not followed).
#[derive(Clone, Debug)]
pub struct Sources {
    /// `~/.claude`
    pub claude_dir: PathBuf,
    /// `~/.opencodex` (statistics, tickets 15-17)
    pub opencodex_dir: PathBuf,
}

impl Sources {
    /// The default locations under `home`.
    pub fn in_home(home: &Path) -> Self {
        Sources {
            claude_dir: home.join(".claude"),
            opencodex_dir: home.join(".opencodex"),
        }
    }
}

/// The time, and the local offset that calendar days and months are counted in.
pub trait Clock: Send + Sync {
    fn now(&self) -> DateTime<Utc>;

    /// The local UTC offset at `at` (statistics ranges, tickets 15-17).
    fn local_offset(&self, at: DateTime<Utc>) -> FixedOffset {
        at.with_timezone(&Local).fixed_offset().timezone()
    }
}

/// The system clock and time zone.
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

/// Finished sessions stay in the tree this long after their last activity.
const RECENT_MS: i64 = 24 * 60 * 60 * 1000;

#[derive(Default)]
enum Phase {
    #[default]
    NotStarted,
    Building,
    Ready(Index),
}

/// What the build learned: the sessions found on disk and who is live.
struct Index {
    sessions: claude_dir::Sessions,
    live: Vec<LiveSession>,
    /// For each waiting session, its reason and when the registry first saw it waiting
    /// for that reason. Kept in memory only, since nothing records when a session began
    /// waiting: it restarts with the app.
    waiting_since: HashMap<String, (Option<WaitingFor>, i64)>,
    /// Sessions New session started, found in their session files before the liveness
    /// source lists them: kept live until it does, while their process runs, for at most
    /// [`STARTED_FOR_MS`] (agent-screens ticket 15).
    started: Vec<Started>,
}

/// A session New session started, found in its session file, and when.
struct Started {
    session: LiveSession,
    found_at: i64,
}

/// How long a started session is kept live without the liveness source listing it.
const STARTED_FOR_MS: i64 = 15_000;

impl Index {
    /// Takes a new answer from the liveness source at `now`: a session waiting for the
    /// same reason as before keeps its time; one newly waiting, or waiting for another
    /// reason, starts from `now`; one no longer waiting is forgotten.
    fn set_live(&mut self, mut live: Vec<LiveSession>, now: i64) {
        // Gone once listed, or when its process has ended (it was never listed live).
        self.started.retain(|Started { session, found_at }| {
            now - found_at <= STARTED_FOR_MS
                && !live.iter().any(|l| l.session_id == session.session_id)
                && session
                    .pid
                    .is_some_and(|pid| liveness::process_start(pid).is_some())
        });
        live.extend(self.started.iter().map(|started| started.session.clone()));
        let before = std::mem::take(&mut self.waiting_since);
        self.waiting_since = live
            .iter()
            .filter_map(|session| {
                let RootStatus::Waiting(reason) = &session.status else {
                    return None;
                };
                let since = before
                    .get(&session.session_id)
                    .filter(|(seen, _)| seen == reason)
                    .map_or(now, |(_, since)| *since);
                Some((session.session_id.clone(), (reason.clone(), since)))
            })
            .collect();
        self.live = live;
    }

    /// Keeps `session`, which New session started and its session file shows, live from
    /// `now` until the liveness source lists it (see [`Index::started`]). A session
    /// already live is left alone.
    fn add_started(&mut self, session: LiveSession, now: i64) {
        if self.live.iter().any(|l| l.session_id == session.session_id) {
            return;
        }
        self.live.push(session.clone());
        self.started.push(Started {
            session,
            found_at: now,
        });
    }

    /// Starts reading the subagents of every session the tree now shows. Subagent
    /// transcripts can be large, so sessions that are not shown are skipped; once read,
    /// a session's subagents are kept current like its own transcript.
    fn read_shown_subagents(&mut self, now: i64) {
        let live = &self.live;
        self.sessions.read_subagents(|found| {
            shown(found, live.iter().any(|l| l.session_id == found.id), now)
        });
    }

    /// The sessions found, and which agent made each request over them: the same
    /// attribution for the Tokens view and the tree. Owned, so the ledger can be read
    /// after the registry's lock is let go.
    fn attribution(&self) -> (Found, HashMap<String, claude_dir::Owner>) {
        let found = self.sessions.found();
        let owners = self.sessions.owners(|project, session| {
            found
                .get(project)
                .is_some_and(|sessions| sessions.iter().any(|s| s.id == session))
        });
        (found, owners)
    }
}

/// A session New session started, as the tree first lists it.
#[derive(Debug)]
pub struct Listed {
    pub session_id: String,
    pub tree: AgentTreeSnapshot,
}

/// Removes the app's own `CLAUDECODE` and every `CLAUDE_CODE_*` variable from
/// `command`'s environment (agent-screens spec, "Decided in this spec" 10): an app
/// started from inside a Claude Code session would otherwise pass them on to the
/// `claude` it runs, `claude --bg` here and `claude attach` in the terminals crate.
pub fn without_claude_code_variables(command: &mut std::process::Command) {
    use std::os::unix::ffi::OsStrExt;
    for (key, _) in std::env::vars_os() {
        let name = key.as_bytes();
        if name == b"CLAUDECODE" || name.starts_with(b"CLAUDE_CODE_") {
            command.env_remove(key);
        }
    }
}

/// The sessions found, by project.
type Found = BTreeMap<String, Vec<claude_dir::FoundSession>>;

pub struct Registry {
    sources: Sources,
    clock: Box<dyn Clock>,
    liveness: Box<dyn Liveness>,
    phase: Mutex<Phase>,
    /// Signalled when the first build has finished.
    ready: Condvar,
    /// opencodex's ledger as tailed so far, read on the first statistics request.
    ledger: Mutex<Ledger>,
    /// Each agent's tokens for the tree, as [`Registry::attribute_tokens`] last found them.
    agent_tokens: Mutex<AgentTotals>,
}

impl Registry {
    /// A registry that has read nothing yet: app launch stays light.
    pub fn new(sources: Sources, clock: Box<dyn Clock>, liveness: Box<dyn Liveness>) -> Self {
        Registry {
            sources,
            clock,
            liveness,
            phase: Mutex::new(Phase::NotStarted),
            ready: Condvar::new(),
            ledger: Mutex::new(Ledger::default()),
            agent_tokens: Mutex::new(AgentTotals::new()),
        }
    }

    /// Claims the first build: true exactly once, for the caller that must then run
    /// [`Registry::build`] (off the main thread). Snapshots read *loading* from here
    /// until that build finishes.
    pub fn claim_build(&self) -> bool {
        let mut phase = self.phase.lock().unwrap();
        if matches!(*phase, Phase::NotStarted) {
            *phase = Phase::Building;
            true
        } else {
            false
        }
    }

    /// Scans the sources and makes the registry ready. Slow: run it off the main thread.
    pub fn build(&self) {
        let mut sessions = claude_dir::Sessions::new(&self.sources.claude_dir.join("projects"));
        sessions.scan();
        let live = self.live_sessions();
        let mut index = Index {
            sessions,
            live: Vec::new(),
            waiting_since: HashMap::new(),
            started: Vec::new(),
        };
        index.set_live(live, self.now());
        index.read_shown_subagents(self.now());
        *self.phase.lock().unwrap() = Phase::Ready(index);
        self.ready.notify_all();
    }

    fn now(&self) -> i64 {
        self.clock.now().timestamp_millis()
    }

    /// Files under `~/.claude/projects` changed (written, created, removed, renamed):
    /// reads only what was appended to them. A folder rereads what is in it. Does
    /// nothing before the first build.
    pub fn files_changed(&self, paths: &[PathBuf]) {
        if let Phase::Ready(index) = &mut *self.phase.lock().unwrap() {
            for path in paths {
                index.sessions.changed(path);
            }
            index.read_shown_subagents(self.now());
        }
    }

    /// Everything under `~/.claude/projects` may have changed (the watcher lost track):
    /// reads what was appended to every transcript, and finds new and removed ones.
    pub fn rescan(&self) {
        if let Phase::Ready(index) = &mut *self.phase.lock().unwrap() {
            index.sessions.scan();
            index.read_shown_subagents(self.now());
        }
    }

    /// Keeps the registry current from here on and hands `sink` the agent tree whenever
    /// it changes, at most once per `timing.min_interval` (see [`Timing`]). Call it after
    /// [`Registry::build`]; the snapshot as the watch starts is not pushed again. Runs
    /// until the returned [`Watching`] is dropped.
    pub fn watch(
        self: &Arc<Self>,
        timing: Timing,
        sink: impl Fn(AgentTreeSnapshot) + Send + 'static,
    ) -> Watching {
        watch::watch(self.clone(), timing, sink)
    }

    /// Asks the liveness source again who is live. Slow (it may run `claude`): call it
    /// off the main thread. Does nothing before the first build.
    pub fn refresh_liveness(&self) {
        if !matches!(*self.phase.lock().unwrap(), Phase::Ready(_)) {
            return;
        }
        let live = self.live_sessions();
        if let Phase::Ready(index) = &mut *self.phase.lock().unwrap() {
            index.set_live(live, self.now());
            index.read_shown_subagents(self.now());
        }
    }

    /// Ends the live session `session_id` (ADR-0013): SIGTERM to its `claude` process,
    /// found through the liveness data and verified to still be the session's, then
    /// SIGKILL after the same check if it is still running after `grace`. Nothing
    /// unverified is ever signalled, and every step is logged. Slow (it asks the liveness
    /// source, then waits up to `grace`): call it off the main thread.
    pub fn end_session(
        &self,
        session_id: &str,
        processes: &dyn Processes,
        grace: std::time::Duration,
    ) -> Ending {
        let live = self.live_sessions();
        let Some(session) = live.iter().find(|l| l.session_id == session_id) else {
            eprintln!("[agents] end session {session_id}: not live, nothing signalled");
            return Ending::NotLive;
        };
        ending::end(
            &self.sources.claude_dir,
            session_id,
            session.pid,
            processes,
            grace,
        )
    }

    fn live_sessions(&self) -> Vec<LiveSession> {
        self.liveness
            .live_sessions()
            .unwrap_or_else(|_| liveness::from_session_files(&self.sources.claude_dir))
    }

    /// Sends `text` into the live session `session_id` as a prompt (ADR-0014), through
    /// the messaging socket its `sessions/<pid>.json` names: its process found and
    /// verified as [`Registry::end_session`] finds and verifies it. Nothing unverified is
    /// ever written to, and every step is logged. Slow (it asks the liveness source, and
    /// may block briefly on the socket): call it off the main thread.
    pub fn prompt_session(
        &self,
        session_id: &str,
        text: &str,
        processes: &dyn Processes,
    ) -> Prompting {
        let live = self.live_sessions();
        let Some(session) = live.iter().find(|l| l.session_id == session_id) else {
            eprintln!("[agents] prompt session {session_id}: not live, nothing sent");
            return Prompting::NotLive;
        };
        prompting::send(
            &self.sources.claude_dir,
            session_id,
            session.pid,
            processes,
            text,
        )
    }

    /// Starts a new background session in the project `project_id` (agent-screens
    /// spec "The side panel": New session): `claude --bg` through `claude`, with no
    /// prompt, name or flags, in the working directory of the project's newest session
    /// and without the app's own Claude Code variables. Answers the short id it printed,
    /// which the tree's session shows as `short_id` once liveness lists it, or gives up at
    /// `deadline` (leaving `claude` running). Slow (it waits for a first build under
    /// way, then runs `claude`): call it off the main thread.
    pub fn new_session(
        &self,
        project_id: &str,
        claude: Option<&Path>,
        deadline: Deadline,
    ) -> Result<String, NewSessionError> {
        let folder = self.project_folder(project_id);
        let started = match (&folder, claude) {
            (None, _) => Err(NewSessionError::NoFolder),
            (Some(_), None) => Err(NewSessionError::ClaudeNotFound),
            (Some(folder), Some(claude)) => starting::start(claude, folder, deadline),
        };
        let shown = folder
            .as_deref()
            .unwrap_or(Path::new("no folder"))
            .display();
        eprintln!("[agents] new session in {project_id} ({shown}): {started:?}");
        started
    }

    /// After New session: until the tree lists a session with `short_id`, looks every
    /// `every` for its `sessions/<pid>.json` (written within a second of `claude --bg`
    /// returning, while `claude agents --json` can take seconds longer on a loaded
    /// machine) and, once found, keeps it live until the liveness source lists it. So
    /// its desk need not wait for the watch's next liveness check. Answers the session
    /// and the tree that lists it, or none when it was not listed by `deadline`.
    pub fn await_session(
        &self,
        short_id: &str,
        every: std::time::Duration,
        deadline: Deadline,
    ) -> Option<Listed> {
        loop {
            let tree = self.snapshot();
            let listed = tree
                .projects
                .iter()
                .flat_map(|project| &project.sessions)
                .find(|session| session.short_id.as_deref() == Some(short_id))
                .map(|session| session.id.clone());
            if let Some(session_id) = listed {
                return Some(Listed { session_id, tree });
            }
            let found = liveness::from_session_files(&self.sources.claude_dir)
                .into_iter()
                .find(|live| live.short_id.as_deref() == Some(short_id));
            if let Some(found) = found {
                let now = self.now();
                if let Phase::Ready(index) = &mut *self.phase.lock().unwrap() {
                    index.add_started(found, now);
                }
            }
            if deadline.passed_in(every) {
                return None;
            }
            std::thread::sleep(every);
        }
    }

    /// The working directory of the newest session of `project_id` that has one,
    /// waiting for a first build under way. None before any build was claimed.
    fn project_folder(&self, project_id: &str) -> Option<PathBuf> {
        let phase = self
            .ready
            .wait_while(self.phase.lock().unwrap(), |phase| {
                matches!(phase, Phase::Building)
            })
            .unwrap();
        let Phase::Ready(index) = &*phase else {
            return None;
        };
        index
            .sessions
            .found()
            .remove(project_id)?
            .into_iter()
            .filter(|session| session.cwd.is_some())
            .max_by_key(|session| session.last_activity)
            .and_then(|session| session.cwd)
            .map(PathBuf::from)
    }

    /// The agent tree as it is now: *loading* until the first build has finished. It
    /// never reads opencodex's ledger: each agent's tokens are the ones the last
    /// [`Registry::attribute_tokens`] found, 0 before it has run.
    pub fn snapshot(&self) -> AgentTreeSnapshot {
        let phase = self.phase.lock().unwrap();
        let Phase::Ready(index) = &*phase else {
            return AgentTreeSnapshot::loading();
        };
        let now = self.clock.now().timestamp_millis();
        let found = with_unwritten(index.sessions.found(), &index.live, now);
        let tokens = self.agent_tokens.lock().unwrap();
        AgentTreeSnapshot {
            status: TreeStatus::Ready,
            projects: tree(index, &found, now, &tokens),
        }
    }

    /// Works out each agent's tokens for the tree: the rows of opencodex's ledger its
    /// transcript made, over all the ledger keeps (none while opencodex is
    /// unavailable). It reads the ledger, which takes a moment the first time, so the
    /// tree is served without waiting for it and the watch calls this off the tree's
    /// way, then pushes the tree again. The registry's lock is held only to read who
    /// made which request, never during the ledger read. Does nothing before the
    /// first build.
    pub fn attribute_tokens(&self) {
        let owners = {
            let Phase::Ready(index) = &*self.phase.lock().unwrap() else {
                return;
            };
            index.attribution().1
        };
        let totals = self
            .with_ledger(|rows| tokens::per_agent(rows, &owners))
            .unwrap_or_default();
        *self.agent_tokens.lock().unwrap() = totals;
    }
}

impl Registry {
    /// Reads what was appended to opencodex's ledger, then hands `use_rows` every row
    /// read so far: the one way the statistics views reach the ledger. An error means
    /// opencodex is unavailable (no `~/.opencodex`, no ledger, or unreadable).
    pub fn with_ledger<R>(
        &self,
        use_rows: impl FnOnce(&[UsageRow]) -> R,
    ) -> Result<R, OpencodexUnavailable> {
        let mut ledger = self.ledger.lock().unwrap();
        ledger
            .read_new(&self.sources.opencodex_dir.join(opencodex::LEDGER))
            .map_err(|_| OpencodexUnavailable::OpencodexUnavailable)?;
        Ok(use_rows(ledger.rows()))
    }

    /// The opencodex view's figures for `range` (local time) and `surface`.
    pub fn opencodex_statistics(&self, range: Range, surface: SurfaceFilter) -> OpencodexAnswer {
        let start = range.start(self.clock.as_ref());
        let now = self.clock.now().timestamp_millis();
        match self.with_ledger(|rows| {
            let limits = opencodex::read_limits(&self.sources.opencodex_dir, now);
            opencodex::statistics(rows, start, surface, limits)
        }) {
            Ok(statistics) => OpencodexAnswer::Ready(statistics),
            Err(_) => OpencodexAnswer::unavailable(),
        }
    }

    /// The model and the harness of the current local calendar month (ticket 16).
    pub fn of_the_month(&self) -> OfTheMonthAnswer {
        let months = opencodex::Months::around(self.clock.as_ref());
        match self.with_ledger(|rows| opencodex::of_the_month(rows, months)) {
            Ok(figures) => OfTheMonthAnswer::Ready(figures),
            Err(_) => OfTheMonthAnswer::unavailable(),
        }
    }
}

impl Registry {
    /// The Tokens view's figures for `range` (local time): Claude Code's rows by
    /// project, session and agent, a chart per hour (Today) or per day, and the rows
    /// that match no transcript. Waits for the first build, which someone must have
    /// claimed (the agent tree's first request does). Reads the subagents of every
    /// session that may have rows in the range the first time, so the first request
    /// over 30 days takes a moment: call it off the main thread.
    pub fn token_statistics(&self, range: Range) -> TokensAnswer {
        let clock = self.clock.as_ref();
        let start = range.start(clock);
        let (size, buckets) = buckets(range, start, clock);
        // Who made which request is read under the registry's lock; the ledger after
        // it is let go, so the tree is never kept waiting on the ledger.
        let (found, owners) = {
            let mut phase = self
                .ready
                .wait_while(self.phase.lock().unwrap(), |phase| {
                    !matches!(phase, Phase::Ready(_))
                })
                .unwrap();
            let Phase::Ready(index) = &mut *phase else {
                unreachable!("waited for the build");
            };
            // A background subagent may write after its root's last line: a day's margin.
            index
                .sessions
                .read_subagents(|found| found.last_activity >= start - RECENT_MS);
            index.attribution()
        };
        let answer = self.with_ledger(|rows| {
            tokens::statistics(rows, start, size, buckets, &owners, &found, project_name)
        });
        match answer {
            Ok(statistics) => TokensAnswer::Ready(statistics),
            Err(_) => TokensAnswer::unavailable(),
        }
    }
}

/// The chart's bars for `range`: each local hour of today, or each local calendar day
/// from the range's `start` to today.
fn buckets(range: Range, start: i64, clock: &dyn Clock) -> (BucketSize, Vec<i64>) {
    let first = DateTime::from_timestamp_millis(start).unwrap_or_default();
    let first_day = first.with_timezone(&clock.local_offset(first)).date_naive();
    let now = clock.now();
    let today = now.with_timezone(&clock.local_offset(now)).date_naive();
    if range == Range::Today {
        const HOUR: i64 = 60 * 60 * 1000;
        let end = local_midnight(today + chrono::Duration::days(1), clock).timestamp_millis();
        let hours = (start..end).step_by(HOUR as usize).collect();
        return (BucketSize::Hour, hours);
    }
    let days = first_day
        .iter_days()
        .take_while(|day| *day <= today)
        .map(|day| local_midnight(day, clock).timestamp_millis())
        .collect();
    (BucketSize::Day, days)
}

/// A project's display name: the last path segment of its newest session's working
/// directory, or the directory name.
fn project_name(project_id: &str, sessions: &[claude_dir::FoundSession]) -> String {
    sessions
        .iter()
        .max_by_key(|s| s.last_activity)
        .and_then(|s| s.cwd.as_deref())
        .and_then(last_segment)
        .unwrap_or(project_id)
        .to_string()
}

/// `found`, with each attachable live session that has no transcript yet (a new
/// background session before its first prompt: agent-screens ticket 15) added to the
/// project of a session with its working directory, else to the project Claude Code
/// names after that directory. Terminal-started sessions still wait for their
/// transcript.
fn with_unwritten(mut found: Found, live: &[LiveSession], now: i64) -> Found {
    for session in live {
        let (Some(_), Some(cwd)) = (&session.short_id, &session.cwd) else {
            continue;
        };
        if found
            .values()
            .flatten()
            .any(|found| found.id == session.session_id)
        {
            continue;
        }
        let project = found
            .iter()
            .find(|(_, sessions)| sessions.iter().any(|s| s.cwd.as_ref() == Some(cwd)))
            .map_or_else(|| project_dir(cwd), |(project, _)| project.clone());
        let started = session.started.unwrap_or(now);
        found
            .entry(project)
            .or_default()
            .push(claude_dir::FoundSession {
                id: session.session_id.clone(),
                title: None,
                cwd: Some(cwd.clone()),
                started,
                last_activity: started,
                activity: Default::default(),
                subagents: Vec::new(),
            });
    }
    found
}

/// The `projects/<project>` name Claude Code gives a working directory: every
/// character but ASCII letters and digits becomes `-`.
fn project_dir(cwd: &str) -> String {
    cwd.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

/// Tokens per agent, by (session id, agent id); the root agent's id is its session's.
type AgentTotals = HashMap<(String, String), u64>;

fn tree(index: &Index, found: &Found, now: i64, tokens: &AgentTotals) -> Vec<Project> {
    let mut projects: Vec<Project> = found
        .iter()
        .filter_map(|(project_id, found)| {
            let mut sessions: Vec<Session> = found
                .iter()
                .filter_map(|found| {
                    let live = index.live.iter().find(|l| l.session_id == found.id);
                    if !shown(found, live.is_some(), now) {
                        return None;
                    }
                    let short_id = live.and_then(|live| live.short_id.clone());
                    Some(Session {
                        id: found.id.clone(),
                        title: found.title.clone(),
                        live: live.is_some(),
                        attachable: short_id.is_some(),
                        short_id,
                        // A background session's transcript begins at its first prompt,
                        // after it started: the earlier time keeps its desk in place.
                        started: live
                            .and_then(|live| live.started)
                            .map_or(found.started, |started| started.min(found.started)),
                        last_activity: found.last_activity,
                        root: Agent {
                            id: found.id.clone(),
                            kind: AgentKind::Root,
                            name: String::new(),
                            model: found.activity.model.clone(),
                            model_id: found.activity.model_id.clone(),
                            state: match live.map(|l| &l.status) {
                                Some(RootStatus::Busy) => AgentState::Busy,
                                Some(RootStatus::Waiting(_)) => AgentState::Waiting,
                                Some(RootStatus::Idle) => AgentState::Idle,
                                None => AgentState::Finished,
                            },
                            flag: live.and_then(|live| flag(index, live, found.last_activity)),
                            current_tool: live.and(found.activity.current_tool.clone()),
                            tokens: agent_tokens(tokens, &found.id, &found.id),
                            children: children(
                                &found.subagents,
                                None,
                                live.is_some(),
                                (&found.id, tokens),
                            ),
                        },
                    })
                })
                .collect();
            if sessions.is_empty() {
                return None;
            }
            sessions.sort_by_key(|s| std::cmp::Reverse((s.live, s.last_activity)));
            Some(Project {
                id: project_id.clone(),
                name: project_name(project_id, found),
                sessions,
            })
        })
        .collect();
    projects.sort_by_key(|p| std::cmp::Reverse((p.sessions[0].live, p.sessions[0].last_activity)));
    projects
}

/// A live root agent's flag: *waiting on you* while idle or blocked on a dialog,
/// counted from its last activity; *asking you something* for any other reason,
/// including ones not known yet, counted from when the registry first saw it.
fn flag(index: &Index, live: &LiveSession, last_activity: i64) -> Option<Flag> {
    let waiting_on_you = Flag {
        kind: FlagKind::WaitingOnYou,
        since: last_activity,
    };
    match &live.status {
        RootStatus::Busy => None,
        RootStatus::Idle => Some(waiting_on_you),
        // No reason: Claude Code before 2.1.162, which never says what it waits for.
        RootStatus::Waiting(None) => Some(waiting_on_you),
        RootStatus::Waiting(Some(WaitingFor::DialogOpen)) => Some(waiting_on_you),
        RootStatus::Waiting(Some(_)) => Some(Flag {
            kind: FlagKind::AskingYouSomething,
            since: index
                .waiting_since
                .get(&live.session_id)
                .map_or(last_activity, |(_, since)| *since),
        }),
    }
}

/// Whether a session belongs in the tree: live, or with its last activity within the last 24 hours.
fn shown(found: &claude_dir::FoundSession, live: bool, now: i64) -> bool {
    live || now - found.last_activity <= RECENT_MS
}

/// The agents spawned by `parent` (a subagent id; none for the root agent). A subagent
/// whose `parentAgentId` names no known subagent hangs under the root. A subagent runs
/// while its meta lacks `finished` and its session is live.
fn children(
    subagents: &[claude_dir::FoundSubagent],
    parent: Option<&str>,
    live: bool,
    (session, tokens): (&str, &AgentTotals),
) -> Vec<Agent> {
    let known = |id: &str| subagents.iter().any(|agent| agent.id == id);
    let running = |agent: &claude_dir::FoundSubagent| live && !agent.finished;
    subagents
        .iter()
        .filter(|agent| {
            let spawner = agent
                .parent
                .as_deref()
                .filter(|id| known(id) && *id != agent.id);
            spawner == parent
        })
        .map(|agent| Agent {
            id: agent.id.clone(),
            kind: AgentKind::Subagent,
            name: agent.agent_type.clone(),
            model: agent.activity.model.clone(),
            model_id: agent.activity.model_id.clone(),
            state: if running(agent) {
                AgentState::Running
            } else {
                AgentState::Finished
            },
            current_tool: agent
                .activity
                .current_tool
                .clone()
                .filter(|_| running(agent)),
            tokens: agent_tokens(tokens, session, &agent.id),
            // A subagent's prompt shows as its root session waiting: never its own flag.
            flag: None,
            children: children(subagents, Some(&agent.id), live, (session, tokens)),
        })
        .collect()
}

fn agent_tokens(tokens: &AgentTotals, session: &str, agent: &str) -> u64 {
    tokens
        .get(&(session.to_string(), agent.to_string()))
        .copied()
        .unwrap_or(0)
}

fn last_segment(path: &str) -> Option<&str> {
    path.trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
}
