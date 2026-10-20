//! Attachable and dormant sessions (agent-screens ticket 08), driven through the
//! registry's public seam: a stand-in `claude` printing `claude agents --json` rows, or
//! `sessions/<pid>.json` files describing processes this test spawned, and the snapshot
//! the world and the panel receive.
//!
//! `claude-code-2.1.303/claude-agents.json` was captured on the owner's machine and
//! scrubbed (names and working directories replaced, session ids mapped onto the
//! fixture sessions, each short id the first eight characters of its session id, as
//! Claude Code makes them). It holds a background session with a process (attachable),
//! an interactive one, and a background session whose process the supervisor stopped
//! (dormant) that was since reopened in a terminal, so its session id is listed twice.
//! `synthetic-claude-agents-dormant.json` is made from those rows: a dormant session on
//! its own and a `kind` not known yet.

use std::fs;
use std::path::{Path, PathBuf};

use agents::{
    AgentState, AgentTreeSnapshot, ClaudeAgentsCommand, Clock, LiveSession, Liveness,
    LivenessError, Registry, RootStatus, Session, Sources,
};
use chrono::{DateTime, Utc};

const BUSY_HERE: &str = "810985b3-cf04-46f8-a290-0d73594090ad";
const FINISHED_HERE: &str = "d4a3e1fd-c3d7-45f1-9491-5595f51ff4af";
const IDLE_LIGHTHOUSE: &str = "d8e86752-4b38-45e2-9d92-0965078920ae";

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

/// A time at which every fixture session is in the tree: 10-17 06:00 UTC, within a day
/// of each one's last activity.
fn morning() -> Box<FixedClock> {
    Box::new(FixedClock("2026-10-17T06:00:00Z".parse().unwrap()))
}

fn session<'a>(tree: &'a AgentTreeSnapshot, id: &str) -> Option<&'a Session> {
    tree.projects
        .iter()
        .flat_map(|project| &project.sessions)
        .find(|session| session.id == id)
}

/// A stand-in `claude` executable that prints `stdout` for `claude agents --json`.
fn fake_claude(dir: &Path, stdout: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let script = dir.join("claude");
    fs::write(
        &script,
        format!(
            "#!/bin/sh\n[ \"$1 $2\" = \"agents --json\" ] || exit 64\ncat '{}'\n",
            stdout.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    script
}

fn claude_agents(fixture: &str) -> (tempfile::TempDir, ClaudeAgentsCommand) {
    let bin = tempfile::tempdir().unwrap();
    let claude = fake_claude(bin.path(), &fixtures().join(fixture));
    (bin, ClaudeAgentsCommand::new(Some(claude)))
}

#[test]
fn a_background_session_with_a_process_is_attachable_by_its_short_id() {
    let (_bin, command) = claude_agents("claude-code-2.1.303/claude-agents.json");
    let live = command.live_sessions().unwrap();
    let find = |id: &str| live.iter().find(|session| session.session_id == id);

    let background = find(IDLE_LIGHTHOUSE).unwrap();
    assert_eq!(background.short_id.as_deref(), Some("d8e86752"));
    assert_eq!(background.status, RootStatus::Idle);
    assert_eq!(background.pid, Some(133845));

    let interactive = find(BUSY_HERE).unwrap();
    assert_eq!(
        interactive.short_id, None,
        "an interactive session never is"
    );
    assert_eq!(interactive.status, RootStatus::Busy);
}

#[test]
fn a_dormant_row_leaves_only_the_terminal_copy_of_its_session_live() {
    let (_bin, command) = claude_agents("claude-code-2.1.303/claude-agents.json");
    let live = command.live_sessions().unwrap();

    let copies: Vec<_> = live
        .iter()
        .filter(|session| session.session_id == FINISHED_HERE)
        .collect();
    assert_eq!(copies.len(), 1, "the dormant background row is not live");
    assert_eq!(copies[0].pid, Some(122415));
    assert_eq!(copies[0].status, RootStatus::Idle);
    assert_eq!(copies[0].short_id, None);
}

/// The agent tree as a registry reading `liveness` sees it.
fn tree(home: &Path, liveness: impl Liveness + 'static) -> AgentTreeSnapshot {
    let registry = Registry::new(Sources::in_home(home), morning(), Box::new(liveness));
    registry.build();
    registry.snapshot()
}

#[test]
fn a_dormant_session_is_ended_not_busy() {
    let home = claude_home();
    let (_bin, command) = claude_agents("claude-code-2.1.303/synthetic-claude-agents-dormant.json");
    let tree = tree(home.path(), command);

    let dormant = session(&tree, BUSY_HERE).unwrap();
    assert!(!dormant.live);
    assert_eq!(dormant.root.state, AgentState::Finished);
    assert!(!dormant.attachable);
    assert_eq!(dormant.short_id, None);

    let attachable = session(&tree, IDLE_LIGHTHOUSE).unwrap();
    assert!(attachable.live && attachable.attachable);
    assert_eq!(attachable.short_id.as_deref(), Some("d8e86752"));

    let unknown = session(&tree, FINISHED_HERE).unwrap();
    assert!(unknown.live, "a kind not known yet is live as listed");
    assert_eq!(unknown.root.state, AgentState::Busy);
    assert!(
        !unknown.attachable,
        "but never attachable, though it has an id"
    );
    assert_eq!(unknown.short_id, None);
}

#[test]
fn the_snapshot_serializes_attachable_and_short_id_as_the_page_expects() {
    let home = claude_home();
    let (_bin, command) = claude_agents("claude-code-2.1.303/claude-agents.json");
    let tree = tree(home.path(), command);

    let json = |id: &str| serde_json::to_value(session(&tree, id).unwrap()).unwrap();
    let attachable = json(IDLE_LIGHTHOUSE);
    assert_eq!(attachable["attachable"], true);
    assert_eq!(attachable["shortId"], "d8e86752");
    let interactive = json(BUSY_HERE);
    assert_eq!(interactive["attachable"], false);
    assert_eq!(interactive["shortId"], serde_json::Value::Null);
}

/// A process standing in for a session's `claude`, killed when dropped.
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
}

impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// A `sessions/<pid>.json` shaped as Claude Code 2.1.303 wrote one for a `claude --bg`
/// session on the owner's machine, scrubbed: `kind` and `jobId` as `kind_and_job` gives
/// them.
fn session_file(home: &Path, process: &Process, session: &str, kind_and_job: &str) {
    let folder = home.join(".claude/sessions");
    fs::create_dir_all(&folder).unwrap();
    let pid = process.pid();
    fs::write(
        folder.join(format!("{pid}.json")),
        format!(
            r#"{{"pid":{pid},"sessionId":"{session}","cwd":"/home/dev/Projects/Lighthouse","startedAt":1792416438939,"procStart":"{}","version":"2.1.303","peerProtocol":1,"peerFeatures":["notify_idle"],{kind_and_job},"entrypoint":"cli","messagingSocketPath":"/run/user/1000/cc-socks/{pid}.sock","name":"[scrubbed]","status":"idle","updatedAt":1792416438975}}"#,
            process.start()
        ),
    )
    .unwrap();
}

/// A liveness source that always fails, as when `claude` is not found.
struct NoClaude;

impl Liveness for NoClaude {
    fn live_sessions(&self) -> Result<Vec<LiveSession>, LivenessError> {
        Err(LivenessError::NotFound)
    }
}

#[test]
fn without_claude_a_background_session_file_with_a_live_pid_is_attachable_by_its_job_id() {
    let home = claude_home();
    let background = Process::spawn();
    let interactive = Process::spawn();
    session_file(
        home.path(),
        &background,
        IDLE_LIGHTHOUSE,
        r#""kind":"bg","jobId":"d8e86752""#,
    );
    session_file(
        home.path(),
        &interactive,
        BUSY_HERE,
        r#""kind":"interactive""#,
    );
    let tree = tree(home.path(), NoClaude);

    let attachable = session(&tree, IDLE_LIGHTHOUSE).unwrap();
    assert!(attachable.live && attachable.attachable);
    assert_eq!(attachable.short_id.as_deref(), Some("d8e86752"));
    let terminal = session(&tree, BUSY_HERE).unwrap();
    assert!(terminal.live);
    assert!(!terminal.attachable);
    assert_eq!(terminal.short_id, None);
}
