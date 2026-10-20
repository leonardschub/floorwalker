//! Ending a live session (ADR-0013), driven through the registry's public seam: a
//! `~/.claude`-shaped temporary home with `sessions/<pid>.json` files, a stubbed
//! liveness source, and a stand-in for the OS's processes, so no test ever signals a
//! real process but the one test of [`SystemProcesses`], which signals only a `sleep` it
//! spawned itself.

use std::collections::HashMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use agents::{
    ClaudeAgentsCommand, Clock, Ending, LiveSession, Liveness, LivenessError, Processes, Registry,
    RootStatus, Signal, Sources, SystemProcesses,
};
use chrono::{DateTime, Utc};

const KILLED: &str = "810985b3-cf04-46f8-a290-0d73594090ad";
const OTHER: &str = "d8e86752-4b38-45e2-9d92-0965078920ae";
const PID: u32 = 381969;

/// The grace the tests give a process between SIGTERM and SIGKILL.
const GRACE: Duration = Duration::from_millis(200);

struct FixedClock;

impl Clock for FixedClock {
    fn now(&self) -> DateTime<Utc> {
        "2026-10-17T10:00:00Z".parse().unwrap()
    }
}

/// A stand-in for `claude agents --json`.
struct StubLiveness(Result<Vec<LiveSession>, LivenessError>);

impl Liveness for StubLiveness {
    fn live_sessions(&self) -> Result<Vec<LiveSession>, LivenessError> {
        self.0.clone()
    }
}

fn live(session: &str, pid: Option<u32>) -> StubLiveness {
    StubLiveness(Ok(vec![LiveSession {
        session_id: session.to_string(),
        status: RootStatus::Busy,
        pid,
        short_id: None,
        cwd: None,
        started: None,
    }]))
}

fn registry(home: &Path, liveness: impl Liveness + 'static) -> Registry {
    Registry::new(
        Sources::in_home(home),
        Box::new(FixedClock),
        Box::new(liveness),
    )
}

/// A `sessions/<pid>.json` as Claude Code 2.1.302 writes it, scrubbed.
fn session_file(home: &Path, pid: u32, session: &str, proc_start: &str) {
    let folder = home.join(".claude/sessions");
    fs::create_dir_all(&folder).unwrap();
    fs::write(
        folder.join(format!("{pid}.json")),
        format!(
            r#"{{"pid":{pid},"sessionId":"{session}","cwd":"/home/dev/Projects/AgentGamified","startedAt":1792150000000,"procStart":"{proc_start}","version":"2.1.302","kind":"interactive","status":"busy"}}"#
        ),
    )
    .unwrap();
}

/// How a stand-in process takes SIGTERM.
#[derive(Clone, Copy, PartialEq)]
enum OnTerm {
    Exits,
    Ignores,
    /// Exits, and another process takes its pid at once.
    ExitsAndPidIsReused,
}

/// The OS's processes, as a test sets them up: each running pid with its start time
/// (`/proc/<pid>/stat` field 22) and how it takes SIGTERM. Records every signal.
#[derive(Clone, Default)]
struct StandIn(Arc<Mutex<Inner>>);

#[derive(Default)]
struct Inner {
    running: HashMap<u32, (String, Option<OnTerm>)>,
    signals: Vec<(u32, Signal)>,
}

impl StandIn {
    fn running(pid: u32, start: &str, on_term: OnTerm) -> Self {
        let stand_in = StandIn::default();
        stand_in
            .0
            .lock()
            .unwrap()
            .running
            .insert(pid, (start.to_string(), Some(on_term)));
        stand_in
    }

    fn signals(&self) -> Vec<(u32, Signal)> {
        self.0.lock().unwrap().signals.clone()
    }
}

impl Processes for StandIn {
    fn start_time(&self, pid: u32) -> Option<String> {
        let inner = self.0.lock().unwrap();
        inner.running.get(&pid).map(|(start, _)| start.clone())
    }

    fn signal(&self, pid: u32, signal: Signal) -> Result<(), String> {
        let mut inner = self.0.lock().unwrap();
        inner.signals.push((pid, signal));
        let on_term = inner.running.get(&pid).and_then(|(_, on_term)| *on_term);
        match (signal, on_term) {
            (Signal::Kill, _) | (Signal::Term, Some(OnTerm::Exits)) => {
                inner.running.remove(&pid);
            }
            (Signal::Term, Some(OnTerm::ExitsAndPidIsReused)) => {
                inner.running.insert(pid, ("999999".to_string(), None));
            }
            _ => {}
        }
        Ok(())
    }
}

#[test]
fn a_verified_session_gets_sigterm_and_ends() {
    let home = tempfile::tempdir().unwrap();
    session_file(home.path(), PID, KILLED, "4867");
    let processes = StandIn::running(PID, "4867", OnTerm::Exits);

    let ending =
        registry(home.path(), live(KILLED, Some(PID))).end_session(KILLED, &processes, GRACE);

    assert_eq!(ending, Ending::Terminated { pid: PID });
    assert_eq!(processes.signals(), vec![(PID, Signal::Term)]);
}

#[test]
fn a_process_still_running_after_the_grace_gets_sigkill() {
    let home = tempfile::tempdir().unwrap();
    session_file(home.path(), PID, KILLED, "4867");
    let processes = StandIn::running(PID, "4867", OnTerm::Ignores);

    let ending =
        registry(home.path(), live(KILLED, Some(PID))).end_session(KILLED, &processes, GRACE);

    assert_eq!(ending, Ending::Killed { pid: PID });
    assert_eq!(
        processes.signals(),
        vec![(PID, Signal::Term), (PID, Signal::Kill)]
    );
}

#[test]
fn a_pid_taken_by_another_process_during_the_grace_never_gets_sigkill() {
    let home = tempfile::tempdir().unwrap();
    session_file(home.path(), PID, KILLED, "4867");
    let processes = StandIn::running(PID, "4867", OnTerm::ExitsAndPidIsReused);

    let ending =
        registry(home.path(), live(KILLED, Some(PID))).end_session(KILLED, &processes, GRACE);

    assert_eq!(ending, Ending::Terminated { pid: PID });
    assert_eq!(processes.signals(), vec![(PID, Signal::Term)]);
}

#[test]
fn a_reused_pid_is_never_signalled() {
    let home = tempfile::tempdir().unwrap();
    // The session's process started at 4867; another one holds its pid now.
    session_file(home.path(), PID, KILLED, "4867");
    let processes = StandIn::running(PID, "5120", OnTerm::Exits);

    let ending =
        registry(home.path(), live(KILLED, Some(PID))).end_session(KILLED, &processes, GRACE);

    assert!(matches!(ending, Ending::Unverified { pid: PID, .. }));
    assert_eq!(processes.signals(), vec![]);
}

#[test]
fn a_pid_whose_session_file_names_another_session_is_never_signalled() {
    let home = tempfile::tempdir().unwrap();
    session_file(home.path(), PID, OTHER, "4867");
    let processes = StandIn::running(PID, "4867", OnTerm::Exits);

    let ending =
        registry(home.path(), live(KILLED, Some(PID))).end_session(KILLED, &processes, GRACE);

    assert!(matches!(ending, Ending::Unverified { pid: PID, .. }));
    assert_eq!(processes.signals(), vec![]);
}

#[test]
fn a_pid_without_a_session_file_is_never_signalled() {
    let home = tempfile::tempdir().unwrap();
    let processes = StandIn::running(PID, "4867", OnTerm::Exits);

    let ending =
        registry(home.path(), live(KILLED, Some(PID))).end_session(KILLED, &processes, GRACE);

    assert!(matches!(ending, Ending::Unverified { pid: PID, .. }));
    assert_eq!(processes.signals(), vec![]);
}

#[test]
fn a_session_that_is_not_live_is_never_signalled() {
    let home = tempfile::tempdir().unwrap();
    session_file(home.path(), PID, KILLED, "4867");
    let processes = StandIn::running(PID, "4867", OnTerm::Exits);

    let ending =
        registry(home.path(), live(OTHER, Some(PID))).end_session(KILLED, &processes, GRACE);

    assert_eq!(ending, Ending::NotLive);
    assert_eq!(processes.signals(), vec![]);
}

#[test]
fn without_a_pid_from_liveness_the_session_file_naming_the_session_gives_it() {
    let home = tempfile::tempdir().unwrap();
    session_file(home.path(), 4242, OTHER, "100");
    session_file(home.path(), PID, KILLED, "4867");
    let processes = StandIn::running(PID, "4867", OnTerm::Exits);

    let ending = registry(home.path(), live(KILLED, None)).end_session(KILLED, &processes, GRACE);

    assert_eq!(ending, Ending::Terminated { pid: PID });
    assert_eq!(processes.signals(), vec![(PID, Signal::Term)]);
}

#[test]
fn init_and_this_app_are_never_signalled_whatever_the_files_say() {
    for pid in [1, std::process::id()] {
        let home = tempfile::tempdir().unwrap();
        session_file(home.path(), pid, KILLED, "4867");
        let processes = StandIn::running(pid, "4867", OnTerm::Exits);

        let ending =
            registry(home.path(), live(KILLED, Some(pid))).end_session(KILLED, &processes, GRACE);

        assert!(matches!(ending, Ending::Unverified { .. }), "pid {pid}");
        assert_eq!(processes.signals(), vec![], "pid {pid}");
    }
}

/// `claude agents --json` through a fake `claude` that prints the 2.1.302 fixture, whose
/// first row is the session KILLED with pid 381969.
#[test]
fn the_pid_comes_from_claude_agents_json() {
    let home = tempfile::tempdir().unwrap();
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/claude-code-2.1.302/claude-agents.json");
    let claude = home.path().join("claude");
    fs::write(&claude, format!("#!/bin/sh\ncat '{}'\n", fixture.display())).unwrap();
    fs::set_permissions(&claude, fs::Permissions::from_mode(0o755)).unwrap();
    session_file(home.path(), PID, KILLED, "4867");
    let processes = StandIn::running(PID, "4867", OnTerm::Exits);

    let ending = registry(home.path(), ClaudeAgentsCommand::new(Some(claude)))
        .end_session(KILLED, &processes, GRACE);

    assert_eq!(ending, Ending::Terminated { pid: PID });
    assert_eq!(processes.signals(), vec![(PID, Signal::Term)]);
}

/// The real processes, on a `sleep` this test spawned and nothing else: its start time
/// is `/proc`'s, SIGTERM ends it, and once it has exited it has no start time.
#[test]
fn the_system_processes_signal_a_process_this_test_owns() {
    let mut child = std::process::Command::new("sleep")
        .arg("60")
        .spawn()
        .unwrap();
    let pid = child.id();
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).unwrap();
    let start = stat[stat.rfind(')').unwrap() + 2..]
        .split(' ')
        .nth(19)
        .unwrap()
        .to_string();

    assert_eq!(SystemProcesses.start_time(pid), Some(start));
    SystemProcesses.signal(pid, Signal::Term).unwrap();
    let status = child.wait().unwrap();

    use std::os::unix::process::ExitStatusExt;
    assert_eq!(status.signal(), Some(libc_sigterm()));
    assert_eq!(SystemProcesses.start_time(pid), None);
}

/// SIGTERM's number on Linux.
fn libc_sigterm() -> i32 {
    15
}
