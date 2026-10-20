//! The registry driven through its public seam: a `~/.claude`-shaped directory built
//! from the checked-in fixtures, a fixed clock and a stubbed liveness source, with
//! assertions on the snapshot the panel receives (spec "Testing Decisions").
//!
//! Fixtures (tests/fixtures/claude-code-<version>/) were captured from real sessions by
//! `capture.py` and scrubbed: every text, tool input and tool output is a placeholder,
//! titles are "Custom title N" / "AI title N", ids are fresh random UUIDs. Two trees are
//! merged here because one machine runs several Claude Code versions at once. The last
//! activity of each fixture session:
//!
//! | session    | project        | version | title           | last activity     |
//! |------------|----------------|---------|-----------------|-------------------|
//! | 810985b3…  | AgentGamified  | 2.1.301 | AI title 1      | 10-17 05:18 UTC   |
//! | d4a3e1fd…  | AgentGamified  | 2.1.301 | AI title 2      | 10-16 15:17 UTC   |
//! | 27950eda…  | Harbor         | 2.1.301 | AI title 3      | 10-16 16:14 UTC   |
//! | 2fc6a1f9…  | tmp            | 2.1.301 | none            | 10-16 09:29 UTC   |
//! | 034d1210…  | AgentGamified  | 2.1.299 | Custom title 1  | 10-16 06:09 UTC   |
//! | d8e86752…  | Lighthouse     | 2.1.299 | Custom title 2  | 10-16 07:45 UTC   |
//!
//! The 2.1.301 AgentGamified folder also holds an orphaned and a superseded copy of
//! d4a3e1fd… under other session ids, which must never appear.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use agents::{
    resolve_claude, AgentKind, AgentState, AgentTreeSnapshot, ClaudeAgentsCommand, Clock,
    LiveSession, Liveness, LivenessError, Registry, RootStatus, Session, Sources, TreeStatus,
};
use chrono::{DateTime, Utc};

const BUSY_HERE: &str = "810985b3-cf04-46f8-a290-0d73594090ad";
const FINISHED_HERE: &str = "d4a3e1fd-c3d7-45f1-9491-5595f51ff4af";
const FINISHED_HARBOR: &str = "27950eda-9b6e-4b12-b43d-7958cb6bf2b4";
const OLD_TMP: &str = "2fc6a1f9-dea6-4caf-a9ce-ec147cf91f65";
const OLD_HERE: &str = "034d1210-7cf4-49ad-ade1-cb4fd68efa24";
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

fn at(rfc3339: &str) -> FixedClock {
    FixedClock(rfc3339.parse().unwrap())
}

/// A stand-in for `claude agents --json`.
#[derive(Clone)]
struct StubLiveness(Arc<Mutex<Result<Vec<LiveSession>, LivenessError>>>);

impl StubLiveness {
    fn live(sessions: &[(&str, RootStatus)]) -> Self {
        let sessions = sessions
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
        StubLiveness(Arc::new(Mutex::new(Ok(sessions))))
    }
}

impl Liveness for StubLiveness {
    fn live_sessions(&self) -> Result<Vec<LiveSession>, LivenessError> {
        self.0.lock().unwrap().clone()
    }
}

fn registry(home: &Path, clock: FixedClock, liveness: StubLiveness) -> Registry {
    Registry::new(Sources::in_home(home), Box::new(clock), Box::new(liveness))
}

fn two_live() -> StubLiveness {
    StubLiveness::live(&[
        (BUSY_HERE, RootStatus::Busy),
        (IDLE_LIGHTHOUSE, RootStatus::Idle),
    ])
}

fn built(home: &Path, clock: FixedClock, liveness: StubLiveness) -> AgentTreeSnapshot {
    let registry = registry(home, clock, liveness);
    registry.build();
    registry.snapshot()
}

fn session<'a>(tree: &'a AgentTreeSnapshot, id: &str) -> Option<&'a Session> {
    tree.projects
        .iter()
        .flat_map(|project| &project.sessions)
        .find(|session| session.id == id)
}

fn session_ids(tree: &AgentTreeSnapshot) -> Vec<&str> {
    tree.projects
        .iter()
        .flat_map(|project| &project.sessions)
        .map(|session| session.id.as_str())
        .collect()
}

#[test]
fn sessions_are_grouped_by_project_named_after_their_working_directory() {
    let home = claude_home();
    let tree = built(home.path(), at("2026-10-17T10:00:00Z"), two_live());

    assert_eq!(tree.status, TreeStatus::Ready);
    let projects: Vec<(&str, &str, Vec<&str>)> = tree
        .projects
        .iter()
        .map(|project| {
            (
                project.id.as_str(),
                project.name.as_str(),
                project.sessions.iter().map(|s| s.id.as_str()).collect(),
            )
        })
        .collect();
    assert_eq!(
        projects,
        vec![
            (
                "-home-dev-Projects-AgentGamified",
                "AgentGamified",
                vec![BUSY_HERE, FINISHED_HERE],
            ),
            (
                "-home-dev-Projects-Lighthouse",
                "Lighthouse",
                vec![IDLE_LIGHTHOUSE]
            ),
            ("-home-dev-Projects-Harbor", "Harbor", vec![FINISHED_HARBOR]),
        ],
        "live projects first, then by latest activity; live sessions first in a project"
    );
}

#[test]
fn finished_sessions_stay_for_24_hours_after_their_last_activity() {
    let home = claude_home();

    let tree = built(home.path(), at("2026-10-17T10:00:00Z"), two_live());
    let finished = session(&tree, FINISHED_HERE).expect("finished 18 h ago: shown");
    assert!(!finished.live);
    assert_eq!(finished.root.state, AgentState::Finished);
    assert_eq!(finished.last_activity, millis("2026-10-16T15:17:35.154Z"));
    assert_eq!(
        finished.started,
        millis("2026-10-16T15:14:43.291Z"),
        "the start is the transcript's first record"
    );
    assert!(
        session(&tree, OLD_TMP).is_none(),
        "finished 24.5 h ago: absent"
    );
    assert!(
        session(&tree, OLD_HERE).is_none(),
        "finished 28 h ago: absent"
    );

    let later = built(home.path(), at("2026-10-17T16:00:00Z"), two_live());
    assert!(
        session(&later, FINISHED_HERE).is_none(),
        "24 h 43 min after its last activity"
    );
    assert!(
        session(&later, FINISHED_HARBOR).is_some(),
        "23 h 46 min after its last activity"
    );
    assert!(
        session(&later, IDLE_LIGHTHOUSE).is_some(),
        "a live session stays however old its last activity"
    );
}

#[test]
fn live_root_agents_are_busy_or_idle_as_the_liveness_source_says() {
    let home = claude_home();
    let tree = built(home.path(), at("2026-10-17T10:00:00Z"), two_live());

    let busy = session(&tree, BUSY_HERE).unwrap();
    assert!(busy.live);
    assert_eq!(busy.root.id, BUSY_HERE);
    assert_eq!(busy.root.kind, AgentKind::Root);
    assert_eq!(busy.root.state, AgentState::Busy);
    let idle = session(&tree, IDLE_LIGHTHOUSE).unwrap();
    assert!(idle.live);
    assert_eq!(idle.root.state, AgentState::Idle);
}

#[test]
fn titles_come_from_the_custom_title_file_or_the_ai_title() {
    let home = claude_home();
    let tree = built(home.path(), at("2026-10-16T12:00:00Z"), two_live());

    let title = |id| session(&tree, id).unwrap().title.as_deref();
    assert_eq!(title(IDLE_LIGHTHOUSE), Some("Custom title 2"));
    assert_eq!(title(OLD_HERE), Some("Custom title 1"));
    assert_eq!(title(BUSY_HERE), Some("AI title 1"));
    assert_eq!(title(OLD_TMP), None, "a session without a title has none");

    // The title file is what a rename writes; it wins over records in the transcript.
    let renamed = home
        .path()
        .join(".claude/projects/-home-dev-Projects-AgentGamified")
        .join(BUSY_HERE);
    fs::create_dir_all(&renamed).unwrap();
    fs::write(
        renamed.join("custom-title.json"),
        r#"{"customTitle":"Renamed"}"#,
    )
    .unwrap();
    let tree = built(home.path(), at("2026-10-16T12:00:00Z"), two_live());
    assert_eq!(
        session(&tree, BUSY_HERE).unwrap().title.as_deref(),
        Some("Renamed")
    );
}

#[test]
fn orphaned_and_superseded_copies_are_not_sessions() {
    let home = claude_home();
    let tree = built(home.path(), at("2026-10-16T12:00:00Z"), two_live());

    let mut ids = session_ids(&tree);
    ids.sort();
    let mut expected = vec![
        BUSY_HERE,
        FINISHED_HERE,
        FINISHED_HARBOR,
        OLD_TMP,
        OLD_HERE,
        IDLE_LIGHTHOUSE,
    ];
    expected.sort();
    assert_eq!(ids, expected);
}

#[test]
fn an_unknown_record_type_and_a_corrupt_line_are_skipped() {
    let home = claude_home();
    let transcript = home
        .path()
        .join(".claude/projects/-home-dev-Projects-Harbor")
        .join(format!("{FINISHED_HARBOR}.jsonl"));
    let mut lines = fs::read_to_string(&transcript).unwrap();
    lines.push_str(
        r#"{"type":"record-from-the-future","timestamp":12,"cwd":["not","a","path"],"sessionId":"x"}"#,
    );
    lines.push_str("\n{\"type\":\"assistant\",\"timestamp\":\"2026-10-16T20:\n");
    lines.push_str(&format!(
        r#"{{"type":"ai-title","aiTitle":"After the damage","sessionId":"{FINISHED_HARBOR}"}}"#
    ));
    lines.push_str(&format!(
        "\n{{\"type\":\"system\",\"timestamp\":\"2026-10-16T21:00:00.000Z\",\"sessionId\":\"{FINISHED_HARBOR}\"}}\n"
    ));
    fs::write(&transcript, lines).unwrap();

    let tree = built(home.path(), at("2026-10-17T10:00:00Z"), two_live());
    let harbor = session(&tree, FINISHED_HARBOR).expect("the session survives bad lines");
    assert_eq!(harbor.title.as_deref(), Some("After the damage"));
    assert_eq!(
        harbor.last_activity,
        millis("2026-10-16T21:00:00Z"),
        "records after the bad lines still count"
    );
    let project = tree
        .projects
        .iter()
        .find(|p| p.id == "-home-dev-Projects-Harbor");
    assert_eq!(
        project.unwrap().name,
        "Harbor",
        "a non-string cwd is ignored"
    );
}

#[test]
fn the_snapshot_carries_no_message_text_tool_input_or_tool_output() {
    // Every content string in the fixtures was replaced by "[scrubbed]", so any of it
    // reaching the payload would show up as that placeholder.
    let home = claude_home();
    let tree = built(home.path(), at("2026-10-16T12:00:00Z"), two_live());
    let payload = serde_json::to_string(&tree).unwrap();

    assert!(!payload.contains("[scrubbed]"), "{payload}");
    for field in ["content", "input", "output", "message", "text", "prompt"] {
        assert!(
            !payload.contains(&format!("\"{field}")),
            "{field} in {payload}"
        );
    }
}

#[test]
fn the_registry_reads_nothing_until_the_first_build_and_reports_loading_until_it_ends() {
    let home = claude_home();
    let liveness = two_live();
    let asked = Arc::new(Mutex::new(0));
    let counted = CountingLiveness(liveness, asked.clone());
    let registry = Registry::new(
        Sources::in_home(home.path()),
        Box::new(at("2026-10-17T10:00:00Z")),
        Box::new(counted),
    );

    assert_eq!(registry.snapshot(), AgentTreeSnapshot::loading());
    assert_eq!(
        *asked.lock().unwrap(),
        0,
        "no liveness before the first request"
    );
    assert!(registry.claim_build(), "the first request starts the build");
    assert!(
        !registry.claim_build(),
        "later requests do not start another"
    );
    assert_eq!(registry.snapshot().status, TreeStatus::Loading);

    registry.build();
    let tree = registry.snapshot();
    assert_eq!(tree.status, TreeStatus::Ready);
    assert_eq!(session_ids(&tree).len(), 4);
    assert_eq!(*asked.lock().unwrap(), 1);
    assert!(!registry.claim_build(), "a ready registry is not rebuilt");
}

struct CountingLiveness(StubLiveness, Arc<Mutex<usize>>);

impl Liveness for CountingLiveness {
    fn live_sessions(&self) -> Result<Vec<LiveSession>, LivenessError> {
        *self.1.lock().unwrap() += 1;
        self.0.live_sessions()
    }
}

#[test]
fn a_failing_liveness_source_leaves_every_session_finished() {
    let home = claude_home();
    let failing = StubLiveness(Arc::new(Mutex::new(Err(LivenessError::NotFound))));
    let tree = built(home.path(), at("2026-10-17T10:00:00Z"), failing);

    let busy_before = session(&tree, BUSY_HERE).expect("last activity 5 h ago: shown");
    assert!(!busy_before.live);
    assert_eq!(busy_before.root.state, AgentState::Finished);
    assert!(
        session(&tree, IDLE_LIGHTHOUSE).is_none(),
        "not live, 26 h old"
    );
}

/// A stand-in `claude` executable that prints `stdout` and exits with `code`.
fn fake_claude(dir: &Path, stdout: &Path, code: i32) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let script = dir.join("claude");
    fs::write(
        &script,
        format!(
            "#!/bin/sh\n[ \"$1 $2\" = \"agents --json\" ] || exit 64\ncat '{}'\nexit {code}\n",
            stdout.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    script
}

#[test]
fn liveness_comes_from_claude_agents_json() {
    let home = claude_home();
    let bin = tempfile::tempdir().unwrap();
    let output = fixtures().join("claude-code-2.1.302/claude-agents.json");
    let claude = fake_claude(bin.path(), &output, 0);
    let registry = Registry::new(
        Sources::in_home(home.path()),
        Box::new(at("2026-10-17T10:00:00Z")),
        Box::new(ClaudeAgentsCommand::new(Some(claude))),
    );
    registry.build();
    let tree = registry.snapshot();

    assert_eq!(
        session(&tree, BUSY_HERE).unwrap().root.state,
        AgentState::Busy
    );
    assert_eq!(
        session(&tree, IDLE_LIGHTHOUSE).unwrap().root.state,
        AgentState::Idle
    );
    assert_eq!(
        session_ids(&tree).len(),
        4,
        "a live session without a transcript yet is not shown"
    );
}

#[test]
fn claude_agents_json_failing_or_missing_is_an_error() {
    let bin = tempfile::tempdir().unwrap();
    let output = fixtures().join("claude-code-2.1.302/claude-agents.json");
    let failing = ClaudeAgentsCommand::new(Some(fake_claude(bin.path(), &output, 1)));
    assert!(matches!(
        failing.live_sessions(),
        Err(LivenessError::Failed(_))
    ));

    let garbage = bin.path().join("garbage.txt");
    fs::write(&garbage, "not json").unwrap();
    let other = tempfile::tempdir().unwrap();
    let unreadable = ClaudeAgentsCommand::new(Some(fake_claude(other.path(), &garbage, 0)));
    assert!(matches!(
        unreadable.live_sessions(),
        Err(LivenessError::Failed(_))
    ));

    assert_eq!(
        ClaudeAgentsCommand::new(None).live_sessions(),
        Err(LivenessError::NotFound)
    );
}

#[test]
fn a_hanging_claude_agents_json_times_out() {
    use std::os::unix::fs::PermissionsExt;
    let bin = tempfile::tempdir().unwrap();
    let script = bin.path().join("claude");
    fs::write(&script, "#!/bin/sh\nexec sleep 30\n").unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    let command =
        ClaudeAgentsCommand::new(Some(script)).with_timeout(std::time::Duration::from_millis(200));

    let started = std::time::Instant::now();
    assert!(matches!(
        command.live_sessions(),
        Err(LivenessError::Failed(_))
    ));
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
}

#[test]
fn claude_is_found_on_path_first_then_in_local_bin() {
    use std::os::unix::fs::PermissionsExt;
    let home = tempfile::tempdir().unwrap();
    let on_path = tempfile::tempdir().unwrap();
    let empty = tempfile::tempdir().unwrap();
    let executable = |path: &Path| {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "#!/bin/sh\n").unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    };
    let local = home.path().join(".local/bin/claude");
    let path = std::env::join_paths([empty.path(), on_path.path()]).unwrap();

    assert_eq!(resolve_claude(Some(&path), home.path()), None);
    executable(&local);
    assert_eq!(
        resolve_claude(Some(&path), home.path()),
        Some(local.clone())
    );
    assert_eq!(resolve_claude(None, home.path()), Some(local.clone()));
    executable(&on_path.path().join("claude"));
    assert_eq!(
        resolve_claude(Some(&path), home.path()),
        Some(on_path.path().join("claude"))
    );

    // Present but not executable does not count.
    fs::set_permissions(&local, fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(resolve_claude(None, home.path()), None);
}

fn millis(rfc3339: &str) -> i64 {
    rfc3339.parse::<DateTime<Utc>>().unwrap().timestamp_millis()
}
