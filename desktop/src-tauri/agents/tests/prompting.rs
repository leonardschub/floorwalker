//! Sending a prompt into a live session (ADR-0014), driven through the registry's seam:
//! a `~/.claude`-shaped temp directory whose `sessions/<pid>.json` describes a process
//! this test spawned (a `sleep`), its `messagingSocketPath` pointing at a throwaway
//! Unix-socket listener in the same temp directory, and the real `/proc` behind
//! [`SystemProcesses`] (only read: prompting never signals). Nothing here ever reaches a
//! real Claude Code session.

use std::fs;
use std::io::Read;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use agents::{
    Clock, LiveSession, Liveness, LivenessError, Prompting, Registry, RootStatus, Sources,
    SystemProcesses,
};
use chrono::{DateTime, Utc};

const SESSION: &str = "810985b3-cf04-46f8-a290-0d73594090ad";
const OTHER: &str = "4f1d2c3b-0000-4000-8000-000000000001";

struct Fixed;

impl Clock for Fixed {
    fn now(&self) -> DateTime<Utc> {
        "2026-10-19T10:00:00Z".parse().unwrap()
    }
}

/// A stand-in for `claude agents --json`: missing (liveness then comes from the session
/// files, as on a machine without `claude` on the PATH), or listing one live session.
struct StubLiveness(Option<LiveSession>);

impl Liveness for StubLiveness {
    fn live_sessions(&self) -> Result<Vec<LiveSession>, LivenessError> {
        match &self.0 {
            Some(session) => Ok(vec![session.clone()]),
            None => Err(LivenessError::NotFound),
        }
    }
}

/// A registry whose liveness comes from the session files under `home`.
fn registry(home: &Path) -> Registry {
    registry_with(home, StubLiveness(None))
}

/// A registry whose liveness says `session` is live with the process `pid`.
fn registry_saying_live(home: &Path, session: &str, pid: u32) -> Registry {
    registry_with(
        home,
        StubLiveness(Some(LiveSession {
            session_id: session.to_string(),
            status: RootStatus::Idle,
            pid: Some(pid),
            short_id: None,
            cwd: None,
            started: None,
        })),
    )
}

fn registry_with(home: &Path, liveness: StubLiveness) -> Registry {
    Registry::new(Sources::in_home(home), Box::new(Fixed), Box::new(liveness))
}

fn prompt(registry: &Registry, session: &str) -> Prompting {
    registry.prompt_session(session, "WORK HARDER", &SystemProcesses)
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

/// A `sessions/<pid>.json` as Claude Code 2.1.303 writes it, scrubbed, with `socket` as
/// its `messagingSocketPath` (none: the key is left out).
fn session_file(home: &Path, pid: u32, session: &str, proc_start: &str, socket: Option<&Path>) {
    let folder = home.join(".claude/sessions");
    fs::create_dir_all(&folder).unwrap();
    let socket = socket
        .map(|path| format!(r#","messagingSocketPath":"{}""#, path.display()))
        .unwrap_or_default();
    fs::write(
        folder.join(format!("{pid}.json")),
        format!(
            r#"{{"pid":{pid},"sessionId":"{session}","cwd":"/home/dev/Projects/AgentGamified","startedAt":1792150000000,"procStart":"{proc_start}","version":"2.1.303","peerProtocol":1,"kind":"interactive","entrypoint":"cli","name":"[scrubbed]","status":"idle"{socket},"updatedAt":1792150000000}}"#
        ),
    )
    .unwrap();
}

/// A throwaway listener standing in for a session's inbox: every connection's bytes, up
/// to the sender's half-close, arrive on the receiver.
struct Inbox {
    path: PathBuf,
    heard: mpsc::Receiver<Vec<u8>>,
}

impl Inbox {
    fn bind(at: &Path) -> Self {
        let path = at.join("inbox.sock");
        let listener = UnixListener::bind(&path).unwrap();
        let (tell, heard) = mpsc::channel();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let mut stream = stream.unwrap();
                let mut bytes = Vec::new();
                stream.read_to_end(&mut bytes).unwrap();
                if tell.send(bytes).is_err() {
                    return;
                }
            }
        });
        Inbox { path, heard }
    }

    /// What the next connection sent, if one comes within a second.
    fn next(&self) -> Option<String> {
        self.heard
            .recv_timeout(Duration::from_secs(1))
            .ok()
            .map(|bytes| String::from_utf8(bytes).unwrap())
    }
}

#[test]
fn a_live_session_hears_one_line_naming_it_with_the_prompt_as_a_user_message() {
    let home = tempfile::tempdir().unwrap();
    let inbox = Inbox::bind(home.path());
    let claude = Process::spawn();
    session_file(
        home.path(),
        claude.pid(),
        SESSION,
        &claude.start(),
        Some(&inbox.path),
    );

    let sent = prompt(&registry(home.path()), SESSION);

    assert_eq!(
        sent,
        Prompting::Sent {
            pid: claude.pid(),
            socket: inbox.path.clone()
        }
    );
    let heard = inbox.next().expect("the inbox heard nothing");
    assert_eq!(heard.matches('\n').count(), 1, "not one line: {heard:?}");
    assert!(heard.ends_with('\n'), "not newline-terminated: {heard:?}");
    let line: serde_json::Value = serde_json::from_str(&heard).unwrap();
    assert_eq!(
        line,
        serde_json::json!({
            "type": "user",
            "session_id": SESSION,
            "message": { "role": "user", "content": "WORK HARDER" },
        })
    );
    assert_eq!(inbox.next(), None, "more than one connection");
}

#[test]
fn only_the_named_session_is_prompted_and_another_sessions_socket_hears_nothing() {
    let home = tempfile::tempdir().unwrap();
    let inbox = Inbox::bind(home.path());
    let other = Process::spawn();
    session_file(
        home.path(),
        other.pid(),
        OTHER,
        &other.start(),
        Some(&inbox.path),
    );

    let sent = prompt(&registry(home.path()), SESSION);

    assert_eq!(sent, Prompting::NotLive);
    assert_eq!(inbox.next(), None);
}

#[test]
fn a_session_file_whose_process_is_not_the_one_running_is_never_written_to() {
    let home = tempfile::tempdir().unwrap();
    let inbox = Inbox::bind(home.path());
    // A pid reused since: another start time than the one recorded.
    let reused = Process::spawn();
    session_file(home.path(), reused.pid(), SESSION, "1", Some(&inbox.path));
    let sent = prompt(
        &registry_saying_live(home.path(), SESSION, reused.pid()),
        SESSION,
    );
    assert!(
        matches!(sent, Prompting::Unverified { pid, .. } if pid == reused.pid()),
        "{sent:?}"
    );
    assert_eq!(inbox.next(), None);

    // A file a crash left behind: its process is gone.
    let home = tempfile::tempdir().unwrap();
    let inbox = Inbox::bind(home.path());
    let gone = Process::spawn();
    let (pid, start) = (gone.pid(), gone.start());
    drop(gone);
    session_file(home.path(), pid, SESSION, &start, Some(&inbox.path));
    let sent = prompt(&registry_saying_live(home.path(), SESSION, pid), SESSION);
    assert!(
        matches!(sent, Prompting::Unverified { pid: p, .. } if p == pid),
        "{sent:?}"
    );
    assert_eq!(inbox.next(), None);
}

#[test]
fn a_session_without_a_messaging_socket_or_with_none_listening_is_reported_and_nothing_else_happens(
) {
    let home = tempfile::tempdir().unwrap();
    let claude = Process::spawn();
    session_file(home.path(), claude.pid(), SESSION, &claude.start(), None);
    assert_eq!(
        prompt(&registry(home.path()), SESSION),
        Prompting::NoSocket { pid: claude.pid() }
    );

    let nobody = home.path().join("nobody.sock");
    session_file(
        home.path(),
        claude.pid(),
        SESSION,
        &claude.start(),
        Some(&nobody),
    );
    let sent = prompt(&registry(home.path()), SESSION);
    assert!(
        matches!(sent, Prompting::Failed { pid, .. } if pid == claude.pid()),
        "{sent:?}"
    );
}
