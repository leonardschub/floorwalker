//! Flags (management-rooms spec, "The registry: waiting and flags"): a live root agent
//! that has finished its turn or is blocked on the user is flagged *waiting on you* or
//! *asking you something*. Driven through the registry's public seam with the scrubbed
//! fixture home, a clock the test moves, and liveness from a stand-in, a stand-in
//! `claude` printing `claude agents --json` rows, or `sessions/<pid>.json` files.
//!
//! The waiting rows follow `tests/fixtures/claude-code-2.1.302/synthetic-claude-agents-waiting.json`.
//! It is **synthetic, not a capture**: no `waiting` row could be observed live, so it was
//! written by hand from the research note (`.scratch/management-rooms/research/
//! detecting-a-pending-question.md`): the keys `claude agents --json` prints on 2.1.302,
//! plus the documented `waitingFor` values. Swap in a real capture once one is made.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use agents::{
    AgentState, AgentTreeSnapshot, ClaudeAgentsCommand, Clock, Flag, FlagKind, LiveSession,
    Liveness, LivenessError, Registry, RootStatus, Session, Sources, WaitingFor,
};
use chrono::{DateTime, Utc};

const BUSY_HERE: &str = "810985b3-cf04-46f8-a290-0d73594090ad";
/// BUSY_HERE's last activity in the fixture.
const BUSY_HERE_LAST: &str = "2026-10-17T05:18:09.354Z";

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

/// A temporary home holding `.claude/projects` from the captured versions.
fn claude_home() -> tempfile::TempDir {
    let home = tempfile::tempdir().unwrap();
    for version in [
        "claude-code-2.1.299",
        "claude-code-2.1.301",
        "claude-code-2.1.302",
    ] {
        let projects = fixtures().join(version).join("projects");
        if projects.is_dir() {
            copy_tree(&projects, &home.path().join(".claude/projects"));
        }
    }
    home
}

fn millis(rfc3339: &str) -> i64 {
    rfc3339.parse::<DateTime<Utc>>().unwrap().timestamp_millis()
}

/// A clock the test moves.
#[derive(Clone)]
struct TestClock(Arc<Mutex<DateTime<Utc>>>);

impl TestClock {
    fn at(rfc3339: &str) -> Self {
        TestClock(Arc::new(Mutex::new(rfc3339.parse().unwrap())))
    }

    fn set(&self, rfc3339: &str) {
        *self.0.lock().unwrap() = rfc3339.parse().unwrap();
    }
}

impl Clock for TestClock {
    fn now(&self) -> DateTime<Utc> {
        *self.0.lock().unwrap()
    }
}

/// A stand-in liveness source whose answer the test changes.
#[derive(Clone)]
struct StubLiveness(Arc<Mutex<Vec<LiveSession>>>);

impl StubLiveness {
    fn new(sessions: &[(&str, RootStatus)]) -> Self {
        let stub = StubLiveness(Arc::new(Mutex::new(Vec::new())));
        stub.set(sessions);
        stub
    }

    fn set(&self, sessions: &[(&str, RootStatus)]) {
        *self.0.lock().unwrap() = sessions
            .iter()
            .map(|(id, status)| LiveSession {
                session_id: id.to_string(),
                status: status.clone(),
                pid: None,
                short_id: None,
                cwd: None,
                started: None,
            })
            .collect();
    }
}

impl Liveness for StubLiveness {
    fn live_sessions(&self) -> Result<Vec<LiveSession>, LivenessError> {
        Ok(self.0.lock().unwrap().clone())
    }
}

fn waiting(reason: &str) -> RootStatus {
    RootStatus::Waiting(Some(WaitingFor::from(reason)))
}

fn session<'a>(tree: &'a AgentTreeSnapshot, id: &str) -> &'a Session {
    tree.projects
        .iter()
        .flat_map(|project| &project.sessions)
        .find(|session| session.id == id)
        .unwrap_or_else(|| panic!("session {id} in the tree"))
}

/// A stand-in `claude` executable that prints `stdout`.
fn fake_claude(dir: &Path, stdout: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let output = dir.join("agents.json");
    fs::write(&output, stdout).unwrap();
    let script = dir.join("claude");
    fs::write(
        &script,
        format!(
            "#!/bin/sh\n[ \"$1 $2\" = \"agents --json\" ] || exit 64\ncat '{}'\n",
            output.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    script
}

/// One `claude agents --json` row for BUSY_HERE, shaped like the waiting fixture's.
fn agents_row(status: &str, waiting_for: Option<&str>) -> String {
    let waiting_for = waiting_for
        .map(|reason| format!(r#","waitingFor":"{reason}""#))
        .unwrap_or_default();
    format!(
        r#"[{{"pid":381969,"cwd":"/home/dev/Projects/AgentGamified","kind":"interactive","startedAt":1792143398358,"sessionId":"{BUSY_HERE}","name":"[scrubbed]","status":"{status}"{waiting_for}}}]"#
    )
}

/// BUSY_HERE's root agent's state and flag kind when `claude agents --json` prints
/// `row`.
fn through_claude_agents_json(row: &str) -> (AgentState, Option<FlagKind>) {
    let home = claude_home();
    let bin = tempfile::tempdir().unwrap();
    let registry = Registry::new(
        Sources::in_home(home.path()),
        Box::new(TestClock::at("2026-10-17T10:00:00Z")),
        Box::new(ClaudeAgentsCommand::new(Some(fake_claude(bin.path(), row)))),
    );
    registry.build();
    let root = session(&registry.snapshot(), BUSY_HERE).root.clone();
    (root.state, root.flag.map(|flag| flag.kind))
}

/// Every case of status and reason, with the root state and flag it must give.
fn cases() -> Vec<(
    &'static str,
    Option<&'static str>,
    AgentState,
    Option<FlagKind>,
)> {
    use AgentState::{Busy, Idle, Waiting};
    use FlagKind::{AskingYouSomething as Asking, WaitingOnYou as OnYou};
    vec![
        ("busy", None, Busy, None),
        ("idle", None, Idle, Some(OnYou)),
        ("waiting", Some("input needed"), Waiting, Some(Asking)),
        ("waiting", Some("permission prompt"), Waiting, Some(Asking)),
        ("waiting", Some("sandbox request"), Waiting, Some(Asking)),
        ("waiting", Some("worker request"), Waiting, Some(Asking)),
        ("waiting", Some("goal proposal"), Waiting, Some(Asking)),
        (
            "waiting",
            Some("a reason from the future"),
            Waiting,
            Some(Asking),
        ),
        ("waiting", Some("dialog open"), Waiting, Some(OnYou)),
        // Without `waitingFor` (Claude Code before 2.1.162): never asking.
        ("waiting", None, Waiting, Some(OnYou)),
        // An unknown status reads as busy.
        ("thinking hard", None, Busy, None),
        ("thinking hard", Some("permission prompt"), Busy, None),
    ]
}

#[test]
fn claude_agents_json_status_and_waiting_reason_give_the_root_state_and_flag() {
    for (status, reason, state, flag) in cases() {
        assert_eq!(
            through_claude_agents_json(&agents_row(status, reason)),
            (state, flag),
            "status {status:?}, waitingFor {reason:?}"
        );
    }
}

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

    /// The kernel's start time of the process, which Claude Code records as `procStart`.
    fn start(&self) -> String {
        let stat = fs::read_to_string(format!("/proc/{}/stat", self.0.id())).unwrap();
        let after_name = &stat[stat.rfind(')').unwrap() + 2..];
        after_name.split(' ').nth(19).unwrap().to_string()
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// BUSY_HERE's root agent's state and flag kind when `claude agents --json` is missing
/// and its `sessions/<pid>.json` (shaped as 2.1.302 writes it) says `status` and
/// `waitingFor`.
fn through_session_file(status: &str, waiting_for: Option<&str>) -> (AgentState, Option<FlagKind>) {
    let home = claude_home();
    let claude = Process::spawn();
    let folder = home.path().join(".claude/sessions");
    fs::create_dir_all(&folder).unwrap();
    let waiting_for = waiting_for
        .map(|reason| format!(r#","waitingFor":"{reason}""#))
        .unwrap_or_default();
    fs::write(
        folder.join(format!("{}.json", claude.0.id())),
        format!(
            r#"{{"pid":{},"sessionId":"{BUSY_HERE}","cwd":"/home/dev/Projects/AgentGamified","startedAt":1792150000000,"procStart":"{}","version":"2.1.302","kind":"interactive","entrypoint":"cli","name":"[scrubbed]","status":"{status}"{waiting_for},"updatedAt":1792150000000,"statusUpdatedAt":1792150000000}}"#,
            claude.0.id(),
            claude.start()
        ),
    )
    .unwrap();
    let registry = Registry::new(
        Sources::in_home(home.path()),
        Box::new(TestClock::at("2026-10-17T10:00:00Z")),
        Box::new(ClaudeAgentsCommand::new(None)),
    );
    registry.build();
    let root = session(&registry.snapshot(), BUSY_HERE).root.clone();
    (root.state, root.flag.map(|flag| flag.kind))
}

#[test]
fn the_session_file_fallback_gives_the_same_answers() {
    for (status, reason, state, flag) in cases() {
        assert_eq!(
            through_session_file(status, reason),
            (state, flag),
            "status {status:?}, waitingFor {reason:?}"
        );
    }
}

fn root_flag(registry: &Registry) -> Option<Flag> {
    session(&registry.snapshot(), BUSY_HERE).root.flag
}

#[test]
fn waiting_on_you_counts_from_the_last_activity() {
    let home = claude_home();
    let clock = TestClock::at("2026-10-17T10:00:00Z");
    let liveness = StubLiveness::new(&[(BUSY_HERE, RootStatus::Idle)]);
    let registry = Registry::new(
        Sources::in_home(home.path()),
        Box::new(clock.clone()),
        Box::new(liveness.clone()),
    );
    registry.build();
    let on_you = Some(Flag {
        kind: FlagKind::WaitingOnYou,
        since: millis(BUSY_HERE_LAST),
    });
    assert_eq!(root_flag(&registry), on_you);

    clock.set("2026-10-17T10:05:00Z");
    liveness.set(&[(BUSY_HERE, waiting("dialog open"))]);
    registry.refresh_liveness();
    assert_eq!(root_flag(&registry), on_you, "a dialog also counts from it");
}

#[test]
fn asking_you_something_counts_from_when_the_registry_first_saw_it_until_the_reason_changes() {
    let home = claude_home();
    let clock = TestClock::at("2026-10-17T10:00:00Z");
    let liveness = StubLiveness::new(&[(BUSY_HERE, waiting("permission prompt"))]);
    let registry = Registry::new(
        Sources::in_home(home.path()),
        Box::new(clock.clone()),
        Box::new(liveness.clone()),
    );
    let asking = |since: &str| {
        Some(Flag {
            kind: FlagKind::AskingYouSomething,
            since: millis(since),
        })
    };
    registry.build();
    assert_eq!(root_flag(&registry), asking("2026-10-17T10:00:00Z"));

    clock.set("2026-10-17T10:05:00Z");
    registry.refresh_liveness();
    assert_eq!(
        root_flag(&registry),
        asking("2026-10-17T10:00:00Z"),
        "still waiting for the same reason: the time holds"
    );

    clock.set("2026-10-17T10:07:00Z");
    liveness.set(&[(BUSY_HERE, waiting("input needed"))]);
    registry.refresh_liveness();
    assert_eq!(
        root_flag(&registry),
        asking("2026-10-17T10:07:00Z"),
        "another reason restarts it"
    );

    clock.set("2026-10-17T10:08:00Z");
    liveness.set(&[(BUSY_HERE, RootStatus::Busy)]);
    registry.refresh_liveness();
    assert_eq!(root_flag(&registry), None, "answered: no flag");

    clock.set("2026-10-17T10:09:00Z");
    liveness.set(&[(BUSY_HERE, waiting("input needed"))]);
    registry.refresh_liveness();
    assert_eq!(
        root_flag(&registry),
        asking("2026-10-17T10:09:00Z"),
        "waiting again, even for the same reason, starts again"
    );

    clock.set("2026-10-17T10:10:00Z");
    liveness.set(&[(BUSY_HERE, waiting("dialog open"))]);
    registry.refresh_liveness();
    clock.set("2026-10-17T10:11:00Z");
    liveness.set(&[(BUSY_HERE, waiting("input needed"))]);
    registry.refresh_liveness();
    assert_eq!(
        root_flag(&registry),
        asking("2026-10-17T10:11:00Z"),
        "back from a dialog to a question is a reason change"
    );
}

const ORCHARD: &str = "65537283-8305-4b67-abb5-97e88ac877df";
const LUNA: &str = "a1317fd98b265dbc2";
const FINISHED_HERE: &str = "d4a3e1fd-c3d7-45f1-9491-5595f51ff4af";

/// Removes the `finished` mark from an Orchard subagent's meta, as while it still runs.
fn unfinish(home: &Path, id: &str) {
    let path = home
        .join(".claude/projects/-home-dev-Projects-Orchard")
        .join(ORCHARD)
        .join(format!("subagents/agent-{id}.meta.json"));
    let mut meta: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    meta.as_object_mut().unwrap().remove("finished");
    fs::write(&path, serde_json::to_vec(&meta).unwrap()).unwrap();
}

/// The tree with Orchard waiting on a permission prompt while its subagent Luna runs
/// (a subagent's prompt shows as its root session waiting), BUSY_HERE idle, and
/// FINISHED_HERE not live.
fn orchard_waiting() -> AgentTreeSnapshot {
    let home = claude_home();
    unfinish(home.path(), LUNA);
    let registry = Registry::new(
        Sources::in_home(home.path()),
        Box::new(TestClock::at("2026-10-17T10:00:00Z")),
        Box::new(StubLiveness::new(&[
            (ORCHARD, waiting("permission prompt")),
            (BUSY_HERE, RootStatus::Idle),
        ])),
    );
    registry.build();
    registry.snapshot()
}

#[test]
fn a_waiting_session_flags_its_root_agent_never_a_subagent_or_a_finished_agent() {
    let tree = orchard_waiting();

    let orchard = &session(&tree, ORCHARD).root;
    assert_eq!(orchard.state, AgentState::Waiting);
    assert_eq!(
        orchard.flag.map(|flag| flag.kind),
        Some(FlagKind::AskingYouSomething)
    );
    let luna = orchard
        .children
        .iter()
        .find(|child| child.id == LUNA)
        .unwrap();
    assert_eq!(luna.state, AgentState::Running);
    assert_eq!(luna.flag, None, "the subagent that asked is not named");
    fn no_subagent_flagged(agents: &[agents::Agent]) -> bool {
        agents
            .iter()
            .all(|agent| agent.flag.is_none() && no_subagent_flagged(&agent.children))
    }
    for project in &tree.projects {
        for session in &project.sessions {
            assert!(no_subagent_flagged(&session.root.children));
        }
    }

    let finished = &session(&tree, FINISHED_HERE).root;
    assert_eq!(finished.state, AgentState::Finished);
    assert_eq!(finished.flag, None);
}

#[test]
fn the_snapshot_serializes_the_waiting_state_and_the_flag_but_not_the_reason() {
    let tree = orchard_waiting();
    let payload = serde_json::to_value(&tree).unwrap();
    let root = |id: &str| {
        payload["projects"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|project| project["sessions"].as_array().unwrap())
            .find(|session| session["id"] == id)
            .unwrap()["root"]
            .clone()
    };

    let orchard = root(ORCHARD);
    assert_eq!(orchard["state"], "waiting");
    assert_eq!(
        orchard["flag"],
        serde_json::json!({ "kind": "asking-you-something", "since": millis("2026-10-17T10:00:00Z") })
    );
    assert_eq!(orchard["children"][0]["flag"], serde_json::Value::Null);
    let here = root(BUSY_HERE);
    assert_eq!(here["state"], "idle");
    assert_eq!(
        here["flag"],
        serde_json::json!({ "kind": "waiting-on-you", "since": millis(BUSY_HERE_LAST) })
    );
    assert_eq!(root(FINISHED_HERE)["flag"], serde_json::Value::Null);

    let text = payload.to_string();
    assert!(!text.contains("permission prompt"), "{text}");
    assert!(!text.contains("waitingFor"), "{text}");
}

const IDLE_LIGHTHOUSE: &str = "d8e86752-4b38-45e2-9d92-0965078920ae";

#[test]
fn a_permission_prompt_and_a_question_from_claude_agents_json_ask_you_something() {
    // A permission prompt (BUSY_HERE), an `AskUserQuestion` (IDLE_LIGHTHOUSE) and a
    // finished turn (ORCHARD), as `claude agents --json` prints them.
    let output = fs::read_to_string(
        fixtures().join("claude-code-2.1.302/synthetic-claude-agents-waiting.json"),
    )
    .unwrap();
    let home = claude_home();
    let bin = tempfile::tempdir().unwrap();
    let registry = Registry::new(
        Sources::in_home(home.path()),
        Box::new(TestClock::at("2026-10-17T10:00:00Z")),
        Box::new(ClaudeAgentsCommand::new(Some(fake_claude(
            bin.path(),
            &output,
        )))),
    );
    registry.build();
    let tree = registry.snapshot();
    let answer = |id| {
        let root = &session(&tree, id).root;
        (root.state, root.flag.map(|flag| flag.kind))
    };

    let asking = (AgentState::Waiting, Some(FlagKind::AskingYouSomething));
    assert_eq!(answer(BUSY_HERE), asking);
    assert_eq!(answer(IDLE_LIGHTHOUSE), asking);
    assert_eq!(
        answer(ORCHARD),
        (AgentState::Idle, Some(FlagKind::WaitingOnYou))
    );
}
