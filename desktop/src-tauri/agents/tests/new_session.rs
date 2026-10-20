//! New session from the side panel (agent-screens ticket 15), driven through the
//! registry's public seam: a stand-in `claude` replaying what `claude --bg` printed on
//! the owner's machine, and the snapshot the world and the panel receive.
//!
//! `claude-code-2.1.303/claude-bg.{stdout,stderr}` were captured from `claude --bg` with
//! no prompt in a trusted throwaway folder; `claude-bg-untrusted.stderr` in an untrusted
//! one (exit 1), its path replaced. `claude-agents-new-session.json` is `claude agents
//! --json` just after such a start, scrubbed: the new background session has a process
//! but no transcript yet (none is written before the first prompt), beside an
//! interactive session that also has none, and a background session started in a
//! folder with no project yet.

use std::fs;
use std::path::{Path, PathBuf};

use std::time::{Duration, Instant};

use agents::{
    AgentState, AgentTreeSnapshot, ClaudeAgentsCommand, Clock, Deadline, NewSessionError, Registry,
    Sources, NEW_SESSION_WITHIN,
};
use chrono::{DateTime, Utc};

const NEWEST: &str = "810985b3-cf04-46f8-a290-0d73594090ad";
const OLDER: &str = "d4a3e1fd-c3d7-45f1-9491-5595f51ff4af";
const PROJECT: &str = "-home-dev-Projects-AgentGamified";

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

struct FixedClock(DateTime<Utc>);

impl Clock for FixedClock {
    fn now(&self) -> DateTime<Utc> {
        self.0
    }
}

/// Within a day of both fixture sessions' last activity.
fn morning() -> Box<FixedClock> {
    Box::new(FixedClock("2026-10-17T06:00:00Z".parse().unwrap()))
}

/// A temporary home whose AgentGamified project has two sessions, their working
/// directories moved to `newest/` and `older/` under `folders`, which exist.
fn home_with_folders(folders: &Path) -> tempfile::TempDir {
    let home = tempfile::tempdir().unwrap();
    let from = fixtures()
        .join("claude-code-2.1.301/projects")
        .join(PROJECT);
    let to = home.path().join(".claude/projects").join(PROJECT);
    fs::create_dir_all(&to).unwrap();
    for (session, folder) in [(NEWEST, "newest"), (OLDER, "older")] {
        let folder = folders.join(folder);
        fs::create_dir_all(&folder).unwrap();
        let transcript = fs::read_to_string(from.join(format!("{session}.jsonl"))).unwrap();
        fs::write(
            to.join(format!("{session}.jsonl")),
            transcript.replace(
                "\"cwd\":\"/home/dev/Projects/AgentGamified\"",
                &format!("\"cwd\":\"{}\"", folder.display()),
            ),
        )
        .unwrap();
    }
    home
}

/// A stand-in `claude` that records its working directory, arguments and environment
/// under `record`, then prints `stdout` and `stderr` and exits with `code`.
fn fake_claude(bin: &Path, record: &Path, stdout: &Path, stderr: &Path, code: i32) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let script = bin.join("claude");
    fs::write(
        &script,
        format!(
            "#!/bin/sh\npwd > '{record}/cwd'\necho \"$@\" > '{record}/args'\nenv > '{record}/env'\ncat '{}' >&2\ncat '{}'\nexit {code}\n",
            stderr.display(),
            stdout.display(),
            record = record.display(),
        ),
    )
    .unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    script
}

/// New session's own deadline, which the stand-ins answer well within.
fn soon() -> Deadline {
    Deadline::after(NEW_SESSION_WITHIN)
}

/// A built registry over `home`; liveness is never asked for here.
fn registry(home: &Path) -> Registry {
    let registry = Registry::new(
        Sources::in_home(home),
        morning(),
        Box::new(ClaudeAgentsCommand::new(None)),
    );
    registry.build();
    registry
}

#[test]
fn a_new_session_starts_in_the_newest_sessions_folder_without_the_apps_claude_variables() {
    let scratch = tempfile::tempdir().unwrap();
    let home = home_with_folders(scratch.path());
    let record = scratch.path().join("record");
    fs::create_dir_all(&record).unwrap();
    let claude = fake_claude(
        scratch.path(),
        &record,
        &fixtures().join("claude-code-2.1.303/claude-bg.stdout"),
        &fixtures().join("claude-code-2.1.303/claude-bg.stderr"),
        0,
    );
    // As when the app itself is started from inside a Claude Code session.
    std::env::set_var("CLAUDECODE", "1");
    std::env::set_var("CLAUDE_CODE_SESSION_ID", "the-apps-own");

    let started = registry(home.path()).new_session(PROJECT, Some(&claude), soon());

    assert_eq!(started, Ok("2ca84613".to_string()));
    let cwd = fs::read_to_string(record.join("cwd")).unwrap();
    assert_eq!(Path::new(cwd.trim()), scratch.path().join("newest"));
    assert_eq!(
        fs::read_to_string(record.join("args")).unwrap().trim(),
        "--bg"
    );
    let env = fs::read_to_string(record.join("env")).unwrap();
    assert!(
        !env.lines()
            .any(|line| line.starts_with("CLAUDECODE=") || line.starts_with("CLAUDE_CODE_")),
        "the app's own Claude Code variables are removed"
    );
}

/// Starts a new session in the AgentGamified project through a stand-in `claude` that
/// prints `stdout` and `stderr` (fixture files, or none) and exits with `code`.
fn start_with(
    stdout: Option<&str>,
    stderr: Option<&str>,
    code: i32,
) -> Result<String, NewSessionError> {
    let scratch = tempfile::tempdir().unwrap();
    let home = home_with_folders(scratch.path());
    let record = scratch.path().join("record");
    fs::create_dir_all(&record).unwrap();
    let file =
        |name: Option<&str>| name.map_or(PathBuf::from("/dev/null"), |name| fixtures().join(name));
    let claude = fake_claude(scratch.path(), &record, &file(stdout), &file(stderr), code);
    registry(home.path()).new_session(PROJECT, Some(&claude), soon())
}

#[test]
fn an_untrusted_folder_is_reported_as_such() {
    assert_eq!(
        start_with(
            None,
            Some("claude-code-2.1.303/claude-bg-untrusted.stderr"),
            1
        ),
        Err(NewSessionError::NotTrusted)
    );
}

#[test]
fn any_other_failure_carries_claude_codes_message() {
    // What it says is made up; that it is passed on, without the progress line, is not.
    let scratch = tempfile::tempdir().unwrap();
    let said = scratch.path().join("said");
    fs::write(
        &said,
        "Starting background service…\nError: the background service did not start\n",
    )
    .unwrap();
    assert_eq!(
        start_with(None, Some(said.to_str().unwrap()), 1),
        Err(NewSessionError::Failed {
            message: "Error: the background service did not start".into()
        })
    );
}

#[test]
fn a_project_with_no_folder_or_no_claude_starts_nothing() {
    let scratch = tempfile::tempdir().unwrap();
    let home = home_with_folders(scratch.path());
    let registry = registry(home.path());
    assert_eq!(
        registry.new_session(
            "-home-dev-Projects-Nowhere",
            Some(Path::new("/bin/false")),
            soon()
        ),
        Err(NewSessionError::NoFolder)
    );
    assert_eq!(
        registry.new_session(PROJECT, None, soon()),
        Err(NewSessionError::ClaudeNotFound)
    );
}

#[test]
fn the_failure_is_serialized_as_the_page_expects() {
    let json = |error: NewSessionError| serde_json::to_value(error).unwrap();
    assert_eq!(
        json(NewSessionError::NotTrusted),
        serde_json::json!({ "error": "not-trusted" })
    );
    assert_eq!(
        json(NewSessionError::Failed {
            message: "no".into()
        }),
        serde_json::json!({ "error": "failed", "message": "no" })
    );
}

/// A stand-in `claude` that prints `claude-agents-new-session.json` for `claude agents
/// --json`.
fn agents_json(bin: &Path) -> ClaudeAgentsCommand {
    use std::os::unix::fs::PermissionsExt;
    let script = bin.join("claude");
    fs::write(
        &script,
        format!(
            "#!/bin/sh\n[ \"$1 $2\" = \"agents --json\" ] || exit 64\ncat '{}'\n",
            fixtures()
                .join("claude-code-2.1.303/claude-agents-new-session.json")
                .display()
        ),
    )
    .unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    ClaudeAgentsCommand::new(Some(script))
}

/// The fixture home of the 2.1.301 capture, as the attachable tests read it.
fn fixture_tree() -> AgentTreeSnapshot {
    let home = tempfile::tempdir().unwrap();
    let to = home.path().join(".claude/projects").join(PROJECT);
    fs::create_dir_all(&to).unwrap();
    let from = fixtures()
        .join("claude-code-2.1.301/projects")
        .join(PROJECT);
    for session in [NEWEST, OLDER] {
        fs::copy(
            from.join(format!("{session}.jsonl")),
            to.join(format!("{session}.jsonl")),
        )
        .unwrap();
    }
    let bin = tempfile::tempdir().unwrap();
    let registry = Registry::new(
        Sources::in_home(home.path()),
        morning(),
        Box::new(agents_json(bin.path())),
    );
    registry.build();
    registry.snapshot()
}

#[test]
fn a_new_background_session_has_a_desk_before_its_first_prompt() {
    let tree = fixture_tree();
    let project = tree.projects.iter().find(|p| p.id == PROJECT).unwrap();

    let new = project
        .sessions
        .iter()
        .find(|s| s.id == "2ca84613-8f4a-48a5-a00e-b434768df212")
        .expect("listed in its project, though it has no transcript yet");
    assert!(new.live && new.attachable);
    assert_eq!(new.short_id.as_deref(), Some("2ca84613"));
    assert_eq!(new.title, None);
    assert_eq!(new.started, 1792440001301);
    assert_eq!(new.root.state, AgentState::Idle);
    assert_eq!(new.root.model, None);
    assert_eq!(project.name, "AgentGamified");
}

/// Its first prompt (90 s after it started) writes its transcript, as Claude Code
/// 2.1.304 did in agent-screens ticket 17's demonstration: a name record, then the
/// prompt.
const FIRST_PROMPT: &str = r#"{"type":"custom-title","customTitle":"as17-1","sessionId":"2ca84613-8f4a-48a5-a00e-b434768df212"}
{"parentUuid":null,"isSidechain":false,"type":"user","message":{"role":"user","content":"[scrubbed]"},"uuid":"0b1c2d3e-0000-4000-8000-000000000001","timestamp":"2026-10-19T20:01:31.301Z","userType":"external","entrypoint":"cli","cwd":"/home/dev/Projects/AgentGamified","sessionId":"2ca84613-8f4a-48a5-a00e-b434768df212","version":"2.1.304"}
"#;

#[test]
fn a_background_session_keeps_its_start_once_its_transcript_is_written() {
    // Desks are packed oldest session first, so a start that jumped to the first
    // prompt's time moved the desk the user sat at (ticket 17's demonstration).
    let home = tempfile::tempdir().unwrap();
    let to = home.path().join(".claude/projects").join(PROJECT);
    fs::create_dir_all(&to).unwrap();
    let from = fixtures()
        .join("claude-code-2.1.301/projects")
        .join(PROJECT);
    for session in [NEWEST, OLDER] {
        fs::copy(
            from.join(format!("{session}.jsonl")),
            to.join(format!("{session}.jsonl")),
        )
        .unwrap();
    }
    fs::write(
        to.join("2ca84613-8f4a-48a5-a00e-b434768df212.jsonl"),
        FIRST_PROMPT,
    )
    .unwrap();
    let bin = tempfile::tempdir().unwrap();
    let registry = Registry::new(
        Sources::in_home(home.path()),
        morning(),
        Box::new(agents_json(bin.path())),
    );
    registry.build();

    let tree = registry.snapshot();
    let new = tree
        .projects
        .iter()
        .flat_map(|p| &p.sessions)
        .find(|s| s.id == "2ca84613-8f4a-48a5-a00e-b434768df212")
        .unwrap();
    assert_eq!(new.title.as_deref(), Some("as17-1"));
    assert_eq!(
        new.started, 1792440001301,
        "when `claude agents` says it started"
    );
}

#[test]
fn a_new_background_session_in_a_folder_with_no_project_yet_gets_one() {
    let tree = fixture_tree();
    let project = tree
        .projects
        .iter()
        .find(|p| p.id == "-home-dev-Projects-Kestrel")
        .expect("its folder's project");
    assert_eq!(project.name, "Kestrel");
    assert_eq!(project.sessions.len(), 1);
    assert_eq!(project.sessions[0].short_id.as_deref(), Some("5e0d6a1c"));
}

#[test]
fn a_terminal_session_still_waits_for_its_transcript() {
    let tree = fixture_tree();
    assert!(!tree
        .projects
        .iter()
        .flat_map(|p| &p.sessions)
        .any(|s| s.id == "9f1b2c3d-4e5f-4a6b-8c7d-0e1f2a3b4c5d"));
}

#[test]
fn a_named_sessions_short_id_is_read_too() {
    // `claude-bg-named.stdout`: `claude --bg --name probe-throwaway "<prompt>"`, as the
    // agent-screens research captured it (§7).
    assert_eq!(
        start_with(Some("claude-code-2.1.303/claude-bg-named.stdout"), None, 0),
        Ok("cac5c364".to_string())
    );
}

#[test]
fn a_new_session_asked_for_during_the_first_build_waits_for_it() {
    // The app's first build of a large `~/.claude` takes a while; a + pressed meanwhile
    // still starts in the project's folder.
    let scratch = tempfile::tempdir().unwrap();
    let home = home_with_folders(scratch.path());
    let record = scratch.path().join("record");
    fs::create_dir_all(&record).unwrap();
    let claude = fake_claude(
        scratch.path(),
        &record,
        &fixtures().join("claude-code-2.1.303/claude-bg.stdout"),
        &fixtures().join("claude-code-2.1.303/claude-bg.stderr"),
        0,
    );
    let registry = std::sync::Arc::new(Registry::new(
        Sources::in_home(home.path()),
        morning(),
        Box::new(ClaudeAgentsCommand::new(None)),
    ));
    assert!(registry.claim_build());
    let building = registry.clone();
    let build = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(300));
        building.build();
    });

    assert_eq!(
        registry.new_session(PROJECT, Some(&claude), soon()),
        Ok("2ca84613".to_string())
    );
    build.join().unwrap();
}

/// A liveness source that has not caught up yet: it lists nothing.
struct NotYet;

impl agents::Liveness for NotYet {
    fn live_sessions(&self) -> Result<Vec<agents::LiveSession>, agents::LivenessError> {
        Ok(Vec::new())
    }
}

/// A process standing in for a new background session's, and its
/// `sessions/<pid>.json` under `home`, as Claude Code 2.1.303 wrote it for the captured
/// new session (scrubbed), with `cwd` left out when `cwd` is None. Kill it when done.
fn running_session(home: &Path, cwd: Option<&str>) -> std::process::Child {
    let process = std::process::Command::new("sleep")
        .arg("60")
        .spawn()
        .unwrap();
    let pid = process.id();
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).unwrap();
    let start = stat[stat.rfind(')').unwrap() + 2..]
        .split(' ')
        .nth(19)
        .unwrap()
        .to_string();
    fs::create_dir_all(home.join(".claude/sessions")).unwrap();
    let cwd = cwd.map_or(String::new(), |cwd| format!(r#""cwd":"{cwd}","#));
    fs::write(
        home.join(format!(".claude/sessions/{pid}.json")),
        format!(
            r#"{{"pid":{pid},"sessionId":"2ca84613-8f4a-48a5-a00e-b434768df212",{cwd}"startedAt":1792440001301,"procStart":"{start}","version":"2.1.303","peerProtocol":1,"kind":"bg","entrypoint":"cli","name":"2ca84613","jobId":"2ca84613","status":"idle"}}"#
        ),
    )
    .unwrap();
    process
}

/// A home whose AgentGamified project has the newest fixture session.
fn home_with_newest() -> tempfile::TempDir {
    let home = tempfile::tempdir().unwrap();
    let to = home.path().join(".claude/projects").join(PROJECT);
    fs::create_dir_all(&to).unwrap();
    let from = fixtures()
        .join("claude-code-2.1.301/projects")
        .join(PROJECT);
    fs::copy(
        from.join(format!("{NEWEST}.jsonl")),
        to.join(format!("{NEWEST}.jsonl")),
    )
    .unwrap();
    home
}

#[test]
fn a_started_session_is_listed_from_its_session_file_before_liveness_catches_up() {
    // `claude agents --json` can take seconds to list a new session on a loaded machine;
    // its `sessions/<pid>.json` is there within a second of `claude --bg` returning.
    let home = home_with_newest();
    let mut process = running_session(home.path(), Some("/home/dev/Projects/AgentGamified"));
    let registry = Registry::new(Sources::in_home(home.path()), morning(), Box::new(NotYet));
    registry.build();
    let listed = |tree: &AgentTreeSnapshot| {
        tree.projects
            .iter()
            .flat_map(|p| &p.sessions)
            .any(|s| s.short_id.as_deref() == Some("2ca84613") && s.live && s.attachable)
    };

    let found = registry.await_session(
        "2ca84613",
        Duration::from_millis(10),
        Deadline::after(Duration::from_secs(5)),
    );
    let found = found.expect("listed");
    assert_eq!(found.session_id, "2ca84613-8f4a-48a5-a00e-b434768df212");
    assert!(listed(&found.tree));
    registry.refresh_liveness();
    assert!(
        listed(&registry.snapshot()),
        "kept while liveness has not caught up, so a seat at it is not left"
    );
    assert!(
        registry
            .await_session(
                "ffffffff",
                Duration::from_millis(10),
                Deadline::after(Duration::from_millis(50))
            )
            .is_none(),
        "a session never found is given up on"
    );
    let _ = process.kill();
    let _ = process.wait();
}

#[test]
fn a_started_session_that_never_reaches_the_tree_is_given_up_on_at_the_deadline() {
    // Its session file names no folder, so no project can list it: the wait still ends.
    let home = home_with_newest();
    let mut process = running_session(home.path(), None);
    let registry = std::sync::Arc::new(Registry::new(
        Sources::in_home(home.path()),
        morning(),
        Box::new(NotYet),
    ));
    registry.build();

    let (answer, answered) = std::sync::mpsc::channel();
    let waiting = registry.clone();
    std::thread::spawn(move || {
        let found = waiting.await_session(
            "2ca84613",
            Duration::from_millis(10),
            Deadline::after(Duration::from_millis(200)),
        );
        let _ = answer.send(found.is_some());
    });
    let given_up = answered.recv_timeout(Duration::from_secs(3));
    let _ = process.kill();
    let _ = process.wait();
    assert_eq!(given_up, Ok(false), "given up on at the deadline");
}

#[test]
fn a_slow_claude_bg_is_given_up_on_at_the_deadline_and_left_running() {
    // The background service may already have started the session: the app never stops
    // a session on its own (story 48), so the command is left to finish.
    let scratch = tempfile::tempdir().unwrap();
    let home = home_with_folders(scratch.path());
    let finished = scratch.path().join("finished");
    let claude = scratch.path().join("claude");
    fs::write(
        &claude,
        format!(
            "#!/bin/sh
sleep 1
touch '{}'
cat '{}'
",
            finished.display(),
            fixtures()
                .join("claude-code-2.1.303/claude-bg.stdout")
                .display()
        ),
    )
    .unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&claude, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let registry = registry(home.path());

    let asked = Instant::now();
    let started = registry.new_session(
        PROJECT,
        Some(&claude),
        Deadline::after(Duration::from_millis(200)),
    );
    assert!(matches!(started, Err(NewSessionError::TimedOut { .. })));
    assert!(
        asked.elapsed() < Duration::from_millis(900),
        "not waited for"
    );

    let until = Instant::now() + Duration::from_secs(5);
    while !finished.exists() {
        assert!(Instant::now() < until, "claude --bg was stopped");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn a_timeout_tells_the_page_how_long_was_waited() {
    assert_eq!(
        serde_json::to_value(Deadline::after(NEW_SESSION_WITHIN).timed_out()).unwrap(),
        serde_json::json!({ "error": "timed-out", "seconds": 15 })
    );
}
