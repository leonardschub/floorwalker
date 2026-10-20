//! The registry staying current (ticket 14), driven through its public seam: files
//! growing under a `~/.claude`-shaped temp directory built from the checked-in, scrubbed
//! fixtures, liveness from a stub or from `sessions/<pid>.json` files describing
//! processes this test spawned, and the snapshots the panel receives.
//!
//! Every line appended here is made up and carries `[scrubbed]` where content would be.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use agents::{
    Agent, AgentState, AgentTreeSnapshot, Clock, LiveSession, Liveness, LivenessError, Registry,
    RootStatus, Session, Sources, Timing, Watching,
};
use chrono::{DateTime, Utc};

const BUSY_HERE: &str = "810985b3-cf04-46f8-a290-0d73594090ad";
const PROJECT_HERE: &str = "-home-dev-Projects-AgentGamified";

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// A temporary home holding `.claude/projects` from both captured versions.
fn claude_home() -> tempfile::TempDir {
    let home = tempfile::tempdir().unwrap();
    for version in ["claude-code-2.1.299", "claude-code-2.1.301"] {
        copy_tree(
            &fixtures().join(version).join("projects"),
            &home.path().join(".claude/projects"),
        );
    }
    home
}

struct FixedClock(DateTime<Utc>);

impl Clock for FixedClock {
    fn now(&self) -> DateTime<Utc> {
        self.0
    }
}

fn at(rfc3339: &str) -> Box<FixedClock> {
    Box::new(FixedClock(rfc3339.parse().unwrap()))
}

/// A stand-in for `claude agents --json` whose answer the test changes.
#[derive(Clone)]
struct StubLiveness(Arc<Mutex<Result<Vec<LiveSession>, LivenessError>>>);

impl StubLiveness {
    fn answering(answer: Result<Vec<LiveSession>, LivenessError>) -> Self {
        StubLiveness(Arc::new(Mutex::new(answer)))
    }

    fn live(sessions: &[(&str, RootStatus)]) -> Self {
        Self::answering(Ok(live(sessions)))
    }

    fn set(&self, answer: Result<Vec<LiveSession>, LivenessError>) {
        *self.0.lock().unwrap() = answer;
    }
}

fn live(sessions: &[(&str, RootStatus)]) -> Vec<LiveSession> {
    sessions
        .iter()
        .map(|(id, status)| LiveSession {
            session_id: id.to_string(),
            status: status.clone(),
            pid: None,
            short_id: None,
            cwd: None,
            started: None,
        })
        .collect()
}

impl Liveness for StubLiveness {
    fn live_sessions(&self) -> Result<Vec<LiveSession>, LivenessError> {
        self.0.lock().unwrap().clone()
    }
}

fn session<'a>(tree: &'a AgentTreeSnapshot, id: &str) -> Option<&'a Session> {
    tree.projects
        .iter()
        .flat_map(|project| &project.sessions)
        .find(|session| session.id == id)
}

fn millis(rfc3339: &str) -> i64 {
    rfc3339.parse::<DateTime<Utc>>().unwrap().timestamp_millis()
}

fn append(path: &Path, text: &str) {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .unwrap();
    file.write_all(text.as_bytes()).unwrap();
}

fn transcript(home: &Path, project: &str, session: &str) -> PathBuf {
    home.join(".claude/projects")
        .join(project)
        .join(format!("{session}.jsonl"))
}

fn user_line(session: &str, at: &str) -> String {
    format!(
        r#"{{"type":"user","sessionId":"{session}","timestamp":"{at}","cwd":"/home/dev/Projects/AgentGamified","message":{{"role":"user","content":"[scrubbed]"}}}}"#
    ) + "\n"
}

#[test]
fn appended_lines_update_the_snapshot() {
    let home = claude_home();
    let registry = Registry::new(
        Sources::in_home(home.path()),
        at("2026-10-17T10:00:00Z"),
        Box::new(StubLiveness::live(&[(BUSY_HERE, RootStatus::Busy)])),
    );
    registry.build();
    let path = transcript(home.path(), PROJECT_HERE, BUSY_HERE);

    append(&path, &user_line(BUSY_HERE, "2026-10-17T09:30:00Z"));
    append(
        &path,
        &format!(r#"{{"type":"ai-title","aiTitle":"AI title 9","sessionId":"{BUSY_HERE}"}}"#),
    );
    registry.files_changed(&[path.clone()]);
    let tree = registry.snapshot();
    let grown = session(&tree, BUSY_HERE).unwrap();
    assert_eq!(grown.last_activity, millis("2026-10-17T09:30:00Z"));
    assert_eq!(
        grown.title.as_deref(),
        Some("AI title 1"),
        "a line still being written (no newline yet) waits"
    );

    append(&path, "\n");
    registry.files_changed(&[path]);
    let tree = registry.snapshot();
    assert_eq!(
        session(&tree, BUSY_HERE).unwrap().title.as_deref(),
        Some("AI title 9")
    );
}

#[test]
fn a_new_session_file_appears_in_the_snapshot() {
    const NEW: &str = "5b1f0c3e-2a4d-4e8f-9c6b-7d2e1f0a9b8c";
    let home = claude_home();
    let registry = Registry::new(
        Sources::in_home(home.path()),
        at("2026-10-17T10:00:00Z"),
        Box::new(StubLiveness::live(&[(NEW, RootStatus::Idle)])),
    );
    registry.build();
    assert!(session(&registry.snapshot(), NEW).is_none());

    let project = home
        .path()
        .join(".claude/projects/-home-dev-Projects-Fresh");
    fs::create_dir_all(&project).unwrap();
    let path = project.join(format!("{NEW}.jsonl"));
    append(&path, &user_line(NEW, "2026-10-17T09:59:00Z"));
    registry.files_changed(&[project]);
    registry.refresh_liveness();

    let tree = registry.snapshot();
    let fresh = session(&tree, NEW).expect("the new session is in the tree");
    assert!(fresh.live);
    assert_eq!(fresh.root.state, AgentState::Idle);
}

const FINISHED_HERE: &str = "d4a3e1fd-c3d7-45f1-9491-5595f51ff4af";
const IDLE_LIGHTHOUSE: &str = "d8e86752-4b38-45e2-9d92-0965078920ae";

/// A process this test owns, standing in for a running `claude`.
struct Process(std::process::Child);

impl Process {
    fn spawn() -> Self {
        Process(
            std::process::Command::new("sleep")
                .arg("60")
                .spawn()
                .unwrap(),
        )
    }

    fn pid(&self) -> u32 {
        self.0.id()
    }

    /// The kernel's start time of the process (`/proc/<pid>/stat` field 22), which
    /// Claude Code records as `procStart`.
    fn start(&self) -> String {
        let stat = fs::read_to_string(format!("/proc/{}/stat", self.pid())).unwrap();
        let after_name = &stat[stat.rfind(')').unwrap() + 2..];
        after_name.split(' ').nth(19).unwrap().to_string()
    }

    fn kill(&mut self) {
        self.0.kill().unwrap();
        self.0.wait().unwrap();
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// A `sessions/<pid>.json` as Claude Code 2.1.302 writes it, scrubbed to the keys the
/// fallback may read plus a few it must ignore.
fn session_file(home: &Path, pid: u32, session: &str, proc_start: &str, status: &str) {
    let folder = home.join(".claude/sessions");
    fs::create_dir_all(&folder).unwrap();
    fs::write(
        folder.join(format!("{pid}.json")),
        format!(
            r#"{{"pid":{pid},"sessionId":"{session}","cwd":"/home/dev/Projects/AgentGamified","startedAt":1792150000000,"procStart":"{proc_start}","version":"2.1.302","kind":"interactive","entrypoint":"cli","name":"[scrubbed]","status":"{status}","updatedAt":1792150000000}}"#
        ),
    )
    .unwrap();
}

fn without_claude(home: &Path, error: LivenessError) -> Registry {
    let registry = Registry::new(
        Sources::in_home(home),
        at("2026-10-17T10:00:00Z"),
        Box::new(StubLiveness::answering(Err(error))),
    );
    registry.build();
    registry
}

#[test]
fn without_claude_liveness_falls_back_to_session_files_of_running_processes() {
    let home = claude_home();
    let busy = Process::spawn();
    let idle = Process::spawn();
    session_file(home.path(), busy.pid(), BUSY_HERE, &busy.start(), "busy");
    session_file(
        home.path(),
        idle.pid(),
        IDLE_LIGHTHOUSE,
        &idle.start(),
        "idle",
    );
    let registry = without_claude(home.path(), LivenessError::NotFound);

    let tree = registry.snapshot();
    assert_eq!(
        session(&tree, BUSY_HERE).unwrap().root.state,
        AgentState::Busy
    );
    assert_eq!(
        session(&tree, IDLE_LIGHTHOUSE).unwrap().root.state,
        AgentState::Idle
    );
}

#[test]
fn a_session_file_of_a_dead_process_is_not_live() {
    let home = claude_home();
    let mut gone = Process::spawn();
    let start = gone.start();
    gone.kill();
    session_file(home.path(), gone.pid(), BUSY_HERE, &start, "busy");
    let registry = without_claude(home.path(), LivenessError::Failed("exited with 1".into()));

    let tree = registry.snapshot();
    let stale = session(&tree, BUSY_HERE).unwrap();
    assert!(!stale.live);
    assert_eq!(stale.root.state, AgentState::Finished);
}

#[test]
fn a_session_file_whose_pid_was_reused_is_not_live() {
    let home = claude_home();
    let other = Process::spawn();
    let earlier_start = (other.start().parse::<u64>().unwrap() - 1000).to_string();
    session_file(home.path(), other.pid(), BUSY_HERE, &earlier_start, "busy");
    let registry = without_claude(home.path(), LivenessError::NotFound);

    let tree = registry.snapshot();
    assert!(
        !session(&tree, BUSY_HERE).unwrap().live,
        "the PID is alive but its process started at another time"
    );
}

#[test]
fn killing_the_process_finishes_its_session_on_the_next_liveness_check() {
    let home = claude_home();
    let mut claude = Process::spawn();
    session_file(
        home.path(),
        claude.pid(),
        FINISHED_HERE,
        &claude.start(),
        "idle",
    );
    let registry = without_claude(home.path(), LivenessError::NotFound);
    assert!(session(&registry.snapshot(), FINISHED_HERE).unwrap().live);

    claude.kill();
    registry.refresh_liveness();
    let tree = registry.snapshot();
    let finished = session(&tree, FINISHED_HERE).unwrap();
    assert!(!finished.live);
    assert_eq!(finished.root.state, AgentState::Finished);
}

#[test]
fn claude_agents_json_wins_over_session_files_while_it_answers() {
    let home = claude_home();
    let process = Process::spawn();
    session_file(
        home.path(),
        process.pid(),
        FINISHED_HERE,
        &process.start(),
        "busy",
    );
    let liveness = StubLiveness::live(&[(BUSY_HERE, RootStatus::Idle)]);
    let registry = Registry::new(
        Sources::in_home(home.path()),
        at("2026-10-17T10:00:00Z"),
        Box::new(liveness.clone()),
    );
    registry.build();
    let tree = registry.snapshot();
    assert!(session(&tree, BUSY_HERE).unwrap().live);
    assert!(!session(&tree, FINISHED_HERE).unwrap().live);

    liveness.set(Err(LivenessError::NotFound));
    registry.refresh_liveness();
    let tree = registry.snapshot();
    assert!(!session(&tree, BUSY_HERE).unwrap().live);
    assert_eq!(
        session(&tree, FINISHED_HERE).unwrap().root.state,
        AgentState::Busy
    );
}

type Pushed = mpsc::Receiver<(Instant, AgentTreeSnapshot)>;

/// A built registry over `home`, watched, with every pushed snapshot sent to the
/// returned receiver along with when it came.
fn watched(home: &Path, liveness: StubLiveness, timing: Timing) -> (Watching, Pushed) {
    let registry = Arc::new(Registry::new(
        Sources::in_home(home),
        at("2026-10-17T10:00:00Z"),
        Box::new(liveness),
    ));
    registry.build();
    let (send, pushed) = mpsc::channel();
    let watching = registry.watch(timing, move |tree| {
        let _ = send.send((Instant::now(), tree));
    });
    (watching, pushed)
}

/// The first snapshot pushed within `within` that satisfies `test`.
fn first_pushed(
    pushed: &Pushed,
    within: Duration,
    test: impl Fn(&AgentTreeSnapshot) -> bool,
) -> Option<(Instant, AgentTreeSnapshot)> {
    let deadline = Instant::now() + within;
    while let Ok((when, tree)) =
        pushed.recv_timeout(deadline.saturating_duration_since(Instant::now()))
    {
        if test(&tree) {
            return Some((when, tree));
        }
    }
    None
}

#[test]
fn a_written_transcript_is_pushed_within_about_a_second() {
    let home = claude_home();
    let (_watching, pushed) = watched(
        home.path(),
        StubLiveness::live(&[(BUSY_HERE, RootStatus::Busy)]),
        Timing::default(),
    );
    std::thread::sleep(Duration::from_millis(100));

    let written = Instant::now();
    append(
        &transcript(home.path(), PROJECT_HERE, BUSY_HERE),
        &user_line(BUSY_HERE, "2026-10-17T09:45:00Z"),
    );
    let (when, _) = first_pushed(&pushed, Duration::from_secs(5), |tree| {
        session(tree, BUSY_HERE).is_some_and(|s| s.last_activity == millis("2026-10-17T09:45:00Z"))
    })
    .expect("the change is pushed");
    assert!(
        when - written < Duration::from_millis(1200),
        "pushed {:?} after the write",
        when - written
    );
}

#[test]
fn a_session_started_after_watching_began_is_pushed() {
    const NEW: &str = "5b1f0c3e-2a4d-4e8f-9c6b-7d2e1f0a9b8c";
    let home = claude_home();
    let (_watching, pushed) = watched(
        home.path(),
        StubLiveness::live(&[(NEW, RootStatus::Busy)]),
        Timing::default(),
    );
    std::thread::sleep(Duration::from_millis(100));

    let project = home
        .path()
        .join(".claude/projects/-home-dev-Projects-Fresh");
    fs::create_dir_all(&project).unwrap();
    append(
        &project.join(format!("{NEW}.jsonl")),
        &user_line(NEW, "2026-10-17T09:59:00Z"),
    );
    first_pushed(&pushed, Duration::from_secs(5), |tree| {
        session(tree, NEW).is_some_and(|s| s.root.state == AgentState::Busy)
    })
    .expect("the new session is pushed, live");
}

#[test]
fn continuous_activity_is_pushed_at_most_about_once_per_second() {
    let home = claude_home();
    let (_watching, pushed) = watched(
        home.path(),
        StubLiveness::live(&[(BUSY_HERE, RootStatus::Busy)]),
        Timing::default(),
    );
    let path = transcript(home.path(), PROJECT_HERE, BUSY_HERE);

    // A line every 50 ms for 3.5 s, each with a later timestamp.
    let started = Instant::now();
    let mut minute = 0;
    while started.elapsed() < Duration::from_millis(3500) {
        minute += 1;
        append(
            &path,
            &user_line(
                BUSY_HERE,
                &format!("2026-10-17T09:{:02}:{:02}Z", minute / 60, minute % 60),
            ),
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    let last = millis(&format!(
        "2026-10-17T09:{:02}:{:02}Z",
        minute / 60,
        minute % 60
    ));
    std::thread::sleep(Duration::from_millis(2500));

    let all: Vec<(Instant, AgentTreeSnapshot)> = pushed.try_iter().collect();
    assert!(
        (3..=6).contains(&all.len()),
        "{} pushes over about 3.5 s of activity",
        all.len()
    );
    for pair in all.windows(2) {
        let gap = pair[1].0 - pair[0].0;
        assert!(
            gap >= Duration::from_millis(950),
            "two pushes {gap:?} apart"
        );
    }
    let (_, latest) = all.last().unwrap();
    assert_eq!(
        session(latest, BUSY_HERE).unwrap().last_activity,
        last,
        "the last line is pushed in the end"
    );
}

#[test]
fn liveness_is_rechecked_and_a_change_pushed() {
    let home = claude_home();
    let liveness = StubLiveness::live(&[(BUSY_HERE, RootStatus::Busy)]);
    let (_watching, pushed) = watched(
        home.path(),
        liveness.clone(),
        Timing {
            liveness_every: Duration::from_millis(300),
            ..Timing::default()
        },
    );

    liveness.set(Ok(Vec::new()));
    first_pushed(&pushed, Duration::from_secs(3), |tree| {
        session(tree, BUSY_HERE).is_some_and(|s| s.root.state == AgentState::Finished)
    })
    .expect("the session is pushed finished once it is no longer live");
}

#[test]
fn each_agents_tokens_follow_the_tree_in_a_later_push() {
    let home = claude_home();
    let ledger = home.path().join(".opencodex/usage.jsonl");
    fs::create_dir_all(ledger.parent().unwrap()).unwrap();
    append(
        &ledger,
        r#"{"requestId":"ocx-2698c0f832aee76d0322adb30e3703fb","timestamp":1792224000000,"provider":"anthropic-native","model":"claude-opus-5-5","surface":"claude","status":200,"usage":{"inputTokens":700,"outputTokens":7},"totalTokens":707}
"#,
    );
    let (_watching, pushed) = watched(
        home.path(),
        StubLiveness::live(&[(BUSY_HERE, RootStatus::Busy)]),
        Timing::default(),
    );

    first_pushed(&pushed, Duration::from_secs(5), |tree| {
        session(tree, BUSY_HERE).is_some_and(|s| s.root.tokens == 707)
    })
    .expect("the tree is pushed again once its tokens are attributed");
}

#[test]
fn nothing_is_pushed_while_nothing_changes() {
    let home = claude_home();
    let (_watching, pushed) = watched(
        home.path(),
        StubLiveness::live(&[(BUSY_HERE, RootStatus::Busy)]),
        Timing {
            liveness_every: Duration::from_millis(100),
            ..Timing::default()
        },
    );
    assert!(pushed.recv_timeout(Duration::from_millis(1500)).is_err());
}

#[test]
fn a_session_file_changing_rechecks_liveness_within_about_a_second() {
    let home = claude_home();
    let claude = Process::spawn();
    session_file(
        home.path(),
        claude.pid(),
        BUSY_HERE,
        &claude.start(),
        "busy",
    );
    let (_watching, pushed) = watched(
        home.path(),
        StubLiveness::answering(Err(LivenessError::NotFound)),
        Timing::default(),
    );
    std::thread::sleep(Duration::from_millis(300));

    let written = Instant::now();
    session_file(
        home.path(),
        claude.pid(),
        BUSY_HERE,
        &claude.start(),
        "idle",
    );
    let (when, _) = first_pushed(&pushed, Duration::from_secs(5), |tree| {
        session(tree, BUSY_HERE).is_some_and(|s| s.root.state == AgentState::Idle)
    })
    .expect("the root agent is pushed idle");
    assert!(
        when - written < Duration::from_millis(1200),
        "pushed {:?} after the write, not on the next 3 s recheck",
        when - written
    );
}

#[test]
fn a_session_file_removed_ends_its_session_within_about_a_second() {
    // Claude Code removes its `sessions/<pid>.json` as it exits: the world takes the
    // session's desks away on that push, not on the next 3 s recheck.
    let home = claude_home();
    let claude = Process::spawn();
    session_file(
        home.path(),
        claude.pid(),
        BUSY_HERE,
        &claude.start(),
        "busy",
    );
    let (_watching, pushed) = watched(
        home.path(),
        StubLiveness::answering(Err(LivenessError::NotFound)),
        Timing::default(),
    );
    first_pushed(&pushed, Duration::from_secs(5), |tree| {
        session(tree, BUSY_HERE).is_some_and(|s| s.live)
    });
    std::thread::sleep(Duration::from_millis(300));

    let removed = Instant::now();
    fs::remove_file(
        home.path()
            .join(format!(".claude/sessions/{}.json", claude.pid())),
    )
    .unwrap();
    let (when, _) = first_pushed(&pushed, Duration::from_secs(5), |tree| {
        session(tree, BUSY_HERE).is_none_or(|s| !s.live)
    })
    .expect("the session is pushed no longer live");
    assert!(
        when - removed < Duration::from_millis(1200),
        "pushed {:?} after the removal, not on the next 3 s recheck",
        when - removed
    );
}

#[test]
fn dropping_the_watch_stops_it() {
    let home = claude_home();
    let (watching, pushed) = watched(
        home.path(),
        StubLiveness::live(&[(BUSY_HERE, RootStatus::Busy)]),
        Timing::default(),
    );
    drop(watching);
    append(
        &transcript(home.path(), PROJECT_HERE, BUSY_HERE),
        &user_line(BUSY_HERE, "2026-10-17T09:45:00Z"),
    );
    assert!(matches!(
        pushed.recv_timeout(Duration::from_millis(1500)),
        Err(mpsc::RecvTimeoutError::Disconnected)
    ));
}

// The agents inside a session, kept current: the 2.1.302 Orchard fixture (see
// `agent_tree.rs`), one live root session with four background subagents.

const ORCHARD: &str = "65537283-8305-4b67-abb5-97e88ac877df";
const ORCHARD_PROJECT: &str = "-home-dev-Projects-Orchard";
const EXPLORE: &str = "a1b52a3f5dbbcf977";
const LUNA: &str = "a1317fd98b265dbc2";

fn orchard_home() -> tempfile::TempDir {
    let home = tempfile::tempdir().unwrap();
    copy_tree(
        &fixtures().join("claude-code-2.1.302/projects"),
        &home.path().join(".claude/projects"),
    );
    home
}

fn subagents_dir(home: &Path) -> PathBuf {
    home.join(".claude/projects")
        .join(ORCHARD_PROJECT)
        .join(ORCHARD)
        .join("subagents")
}

fn orchard_live() -> StubLiveness {
    StubLiveness::live(&[(ORCHARD, RootStatus::Busy)])
}

fn find<'a>(agent: &'a Agent, id: &str) -> Option<&'a Agent> {
    if agent.id == id {
        return Some(agent);
    }
    agent.children.iter().find_map(|child| find(child, id))
}

fn orchard_agent<'a>(tree: &'a AgentTreeSnapshot, id: &str) -> Option<&'a Agent> {
    find(&session(tree, ORCHARD)?.root, id)
}

/// Sets or removes a subagent meta's `finished` flag.
fn set_finished(home: &Path, id: &str, finished: bool) -> PathBuf {
    let path = subagents_dir(home).join(format!("agent-{id}.meta.json"));
    let mut meta: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let meta_object = meta.as_object_mut().unwrap();
    if finished {
        meta_object.insert("finished".into(), true.into());
    } else {
        meta_object.remove("finished");
    }
    fs::write(&path, serde_json::to_vec(&meta).unwrap()).unwrap();
    path
}

/// Cuts a transcript right after its last `tool_use`, as while that tool still runs,
/// and returns the lines cut off (its result and everything after it).
fn cut_after_last_tool_use(path: &Path) -> String {
    let text = fs::read_to_string(path).unwrap();
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let last = lines
        .iter()
        .rposition(|line| line.contains(r#""type":"tool_use""#))
        .unwrap();
    fs::write(path, lines[..=last].concat()).unwrap();
    lines[last + 1..].concat()
}

fn built_orchard(home: &Path) -> Registry {
    let registry = Registry::new(
        Sources::in_home(home),
        at("2026-10-17T10:00:00Z"),
        Box::new(orchard_live()),
    );
    registry.build();
    registry
}

#[test]
fn a_tool_result_appended_clears_the_current_tool() {
    let home = orchard_home();
    set_finished(home.path(), EXPLORE, false);
    let subagent = subagents_dir(home.path()).join(format!("agent-{EXPLORE}.jsonl"));
    let subagent_rest = cut_after_last_tool_use(&subagent);
    let root = transcript(home.path(), ORCHARD_PROJECT, ORCHARD);
    let root_rest = cut_after_last_tool_use(&root);
    let registry = built_orchard(home.path());
    let tree = registry.snapshot();
    assert_eq!(
        orchard_agent(&tree, EXPLORE)
            .unwrap()
            .current_tool
            .as_deref(),
        Some("Bash")
    );
    assert_eq!(
        session(&tree, ORCHARD)
            .unwrap()
            .root
            .current_tool
            .as_deref(),
        Some("Bash")
    );

    append(&subagent, &subagent_rest);
    append(&root, &root_rest);
    registry.files_changed(&[subagent, root]);
    let tree = registry.snapshot();
    assert_eq!(orchard_agent(&tree, EXPLORE).unwrap().current_tool, None);
    assert_eq!(session(&tree, ORCHARD).unwrap().root.current_tool, None);
}

#[test]
fn a_subagent_meta_turning_finished_finishes_it() {
    let home = orchard_home();
    set_finished(home.path(), EXPLORE, false);
    let registry = built_orchard(home.path());
    assert_eq!(
        orchard_agent(&registry.snapshot(), EXPLORE).unwrap().state,
        AgentState::Running
    );

    let meta = set_finished(home.path(), EXPLORE, true);
    registry.files_changed(&[meta]);
    assert_eq!(
        orchard_agent(&registry.snapshot(), EXPLORE).unwrap().state,
        AgentState::Finished
    );
}

#[test]
fn a_subagent_spawned_later_appears_under_its_parent() {
    const NEW: &str = "a0123456789abcdef";
    let home = orchard_home();
    let registry = built_orchard(home.path());
    assert!(orchard_agent(&registry.snapshot(), NEW).is_none());

    let folder = subagents_dir(home.path());
    let meta = folder.join(format!("agent-{NEW}.meta.json"));
    fs::write(
        &meta,
        format!(r#"{{"agentType":"Explore","description":"[scrubbed]","parentAgentId":"{LUNA}"}}"#),
    )
    .unwrap();
    let transcript = folder.join(format!("agent-{NEW}.jsonl"));
    append(&transcript, &user_line(ORCHARD, "2026-10-17T09:59:00Z"));
    registry.files_changed(&[meta, transcript]);

    let tree = registry.snapshot();
    let luna = orchard_agent(&tree, LUNA).unwrap();
    let new = luna
        .children
        .iter()
        .find(|child| child.id == NEW)
        .expect("under the subagent that spawned it");
    assert_eq!(new.state, AgentState::Running);
}

#[test]
fn a_subagent_tool_change_is_pushed_within_about_a_second() {
    let home = orchard_home();
    set_finished(home.path(), EXPLORE, false);
    let subagent = subagents_dir(home.path()).join(format!("agent-{EXPLORE}.jsonl"));
    let rest = cut_after_last_tool_use(&subagent);
    let (_watching, pushed) = watched(home.path(), orchard_live(), Timing::default());
    std::thread::sleep(Duration::from_millis(100));

    let written = Instant::now();
    append(&subagent, &rest);
    let (when, _) = first_pushed(&pushed, Duration::from_secs(5), |tree| {
        orchard_agent(tree, EXPLORE).is_some_and(|agent| agent.current_tool.is_none())
    })
    .expect("the cleared tool is pushed");
    assert!(
        when - written < Duration::from_millis(1200),
        "pushed {:?} after the write",
        when - written
    );
}
