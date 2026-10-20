//! Which sessions are live, and whether their root agent is busy or idle.
//!
//! The source is the supported `claude agents --json` (ADR-0010). The binary is
//! resolved from `PATH`, then `~/.local/bin/claude`, since an app launched from the
//! desktop may not inherit the shell's `PATH`. When the command fails or is missing,
//! the registry falls back to the `sessions/<pid>.json` files ([`from_session_files`]).

use std::ffi::{OsStr, OsString};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use serde::Deserialize;

/// A live session as the liveness source reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LiveSession {
    pub session_id: String,
    pub status: RootStatus,
    /// The pid of the session's `claude` process, when the source gives it: what ending
    /// the session signals, once verified (`ending`).
    pub pid: Option<u32>,
    /// A background session's short id (`claude attach <id>`), set only while it can be
    /// attached: see [`classify`].
    pub short_id: Option<String>,
    /// Its working directory, when the source gives it: where a background session
    /// without a transcript yet belongs (a new one waits for its first prompt).
    pub cwd: Option<String>,
    /// When it started, in milliseconds since the epoch, when the source gives it.
    pub started: Option<i64>,
}

/// What a session the liveness source lists is, by its `kind` and whether it has a
/// process.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Classification {
    /// Live, and attachable by the short id when it has one.
    Live { short_id: Option<String> },
    /// A background session whose process the supervisor stopped: it counts as ended,
    /// like a session that is not listed at all.
    Dormant,
}

/// A background session (`kind: "background"` in `claude agents --json`, `"bg"` in its
/// session file) with a process is attachable by its short id; one without a process is
/// dormant. Any other kind (`interactive` today) is live as listed, never attachable.
pub(crate) fn classify(
    kind: Option<&str>,
    pid: Option<u32>,
    short_id: Option<String>,
) -> Classification {
    match (kind, pid) {
        (Some("background" | "bg"), Some(_)) => Classification::Live { short_id },
        (Some("background" | "bg"), None) => Classification::Dormant,
        _ => Classification::Live { short_id: None },
    }
}

/// What a live root agent is doing, as Claude Code's `status` says: working, finished
/// its turn, or blocked on the user.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RootStatus {
    /// `busy`, and any status not known yet.
    Busy,
    /// `idle`: the agent finished its turn.
    Idle,
    /// `waiting`, with what for as `waitingFor` says, or none (Claude Code before
    /// 2.1.162).
    Waiting(Option<WaitingFor>),
}

/// What a waiting session waits for, from `waitingFor`: an open dialog, which the user
/// opened themselves, or any other reason as Claude Code wrote it (`permission prompt`,
/// `input needed`, and ones not known yet).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WaitingFor {
    DialogOpen,
    Other(String),
}

impl From<&str> for WaitingFor {
    fn from(reason: &str) -> Self {
        match reason {
            "dialog open" => WaitingFor::DialogOpen,
            other => WaitingFor::Other(other.to_string()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LivenessError {
    /// No `claude` binary on `PATH` or at `~/.local/bin/claude`.
    NotFound,
    /// The command could not run, timed out, failed, or printed something unreadable.
    Failed(String),
}

/// A source of live sessions. The registry asks it when it builds; a stand-in replaces
/// it in tests.
pub trait Liveness: Send + Sync {
    fn live_sessions(&self) -> Result<Vec<LiveSession>, LivenessError>;
}

/// `claude agents --json`.
pub struct ClaudeAgentsCommand {
    binary: OnceLock<Option<PathBuf>>,
    search: Option<(Option<OsString>, PathBuf)>,
    timeout: Duration,
}

const TIMEOUT: Duration = Duration::from_secs(10);

impl ClaudeAgentsCommand {
    /// The command run through the given binary (or none, when it was not found).
    pub fn new(binary: Option<PathBuf>) -> Self {
        ClaudeAgentsCommand {
            binary: OnceLock::from(binary),
            search: None,
            timeout: TIMEOUT,
        }
    }

    /// The command through the `claude` found on `path` (the `PATH` value) or in
    /// `home`'s `.local/bin`, looked up when first run rather than now.
    pub fn resolve(path: Option<&OsStr>, home: &Path) -> Self {
        ClaudeAgentsCommand {
            binary: OnceLock::new(),
            search: Some((path.map(OsStr::to_os_string), home.to_path_buf())),
            timeout: TIMEOUT,
        }
    }

    fn binary(&self) -> Option<&PathBuf> {
        self.binary
            .get_or_init(|| {
                let (path, home) = self.search.as_ref()?;
                resolve_claude(path.as_deref(), home)
            })
            .as_ref()
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

/// The `claude` binary: the first executable `claude` on `path`, else
/// `home/.local/bin/claude` when it is executable.
pub fn resolve_claude(path: Option<&OsStr>, home: &Path) -> Option<PathBuf> {
    path.into_iter()
        .flat_map(std::env::split_paths)
        .filter(|dir| dir.is_absolute())
        .map(|dir| dir.join("claude"))
        .chain(std::iter::once(home.join(".local/bin/claude")))
        .find(|candidate| is_executable(candidate))
}

fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

impl Liveness for ClaudeAgentsCommand {
    fn live_sessions(&self) -> Result<Vec<LiveSession>, LivenessError> {
        let binary = self.binary().ok_or(LivenessError::NotFound)?;
        let mut child = Command::new(binary)
            .args(["agents", "--json"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| LivenessError::Failed(error.to_string()))?;
        let mut stdout = child.stdout.take().expect("stdout is piped");
        let reader = std::thread::spawn(move || {
            let mut output = Vec::new();
            stdout.read_to_end(&mut output).map(|_| output)
        });
        let deadline = Instant::now() + self.timeout;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(20))
                }
                Ok(None) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(LivenessError::Failed("timed out".into()));
                }
                Err(error) => return Err(LivenessError::Failed(error.to_string())),
            }
        };
        let output = reader
            .join()
            .map_err(|_| LivenessError::Failed("reader panicked".into()))?
            .map_err(|error| LivenessError::Failed(error.to_string()))?;
        if !status.success() {
            return Err(LivenessError::Failed(format!("exited with {status}")));
        }
        parse_agents_json(&output)
    }
}

/// The fallback when `claude agents --json` fails or is missing: the session files
/// `sessions/<pid>.json` under `claude_dir`. Each running Claude Code writes one, and one
/// left behind by a crash stays, so a file counts only while its PID is alive with the
/// start time it recorded as `procStart` (a reused PID has another). Only `pid`,
/// `sessionId`, `procStart`, `status`, `waitingFor`, `kind`, `jobId` (a background
/// session's short id, with `kind: "bg"`), `cwd` and `startedAt` are read; the `<pid>.<hash>.key` files are not.
pub(crate) fn from_session_files(claude_dir: &Path) -> Vec<LiveSession> {
    let Ok(entries) = std::fs::read_dir(claude_dir.join("sessions")) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            name.strip_suffix(".json")
                .is_some_and(|pid| !pid.is_empty() && pid.bytes().all(|b| b.is_ascii_digit()))
        })
        .filter_map(|entry| {
            let bytes = std::fs::read(entry.path()).ok()?;
            let file: SessionFile = serde_json::from_slice(&bytes).ok()?;
            if process_start(file.pid?)? != file.proc_start? {
                return None;
            }
            let Classification::Live { short_id } =
                classify(file.kind.as_deref(), file.pid, file.job_id)
            else {
                return None;
            };
            Some(LiveSession {
                session_id: file.session_id?,
                status: root_status(file.status.as_deref(), file.waiting_for),
                pid: file.pid,
                short_id,
                cwd: file.cwd,
                started: file.started_at,
            })
        })
        .collect()
}

#[derive(Deserialize)]
struct SessionFile {
    #[serde(default, deserialize_with = "lenient")]
    pid: Option<u32>,
    #[serde(rename = "sessionId", default, deserialize_with = "lenient")]
    session_id: Option<String>,
    #[serde(rename = "procStart", default, deserialize_with = "lenient")]
    proc_start: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    status: Option<String>,
    #[serde(rename = "waitingFor", default, deserialize_with = "lenient")]
    waiting_for: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    kind: Option<String>,
    #[serde(rename = "jobId", default, deserialize_with = "lenient")]
    job_id: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    cwd: Option<String>,
    #[serde(rename = "startedAt", default, deserialize_with = "lenient")]
    started_at: Option<i64>,
}

pub(crate) fn lenient<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::de::DeserializeOwned,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(serde_json::from_value(value).ok())
}

/// The start time of a running process, in clock ticks after boot: field 22 of
/// `/proc/<pid>/stat`, what Claude Code records as `procStart`. None when no such
/// process runs (a zombie, already dead, counts as none).
pub(crate) fn process_start(pid: u32) -> Option<String> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // The command name (field 2) is in parentheses and may hold spaces or parentheses.
    let mut fields = stat.get(stat.rfind(')')? + 1..)?.split_whitespace();
    let state = fields.next()?;
    if state == "Z" || state == "X" {
        return None;
    }
    fields.nth(18).map(str::to_string)
}

/// `status` and `waitingFor` as Claude Code writes them. A status not known yet (or
/// none) counts as busy, so it never raises a flag; the reason counts only while waiting.
fn root_status(status: Option<&str>, waiting_for: Option<String>) -> RootStatus {
    match status {
        Some("idle") => RootStatus::Idle,
        Some("waiting") => RootStatus::Waiting(waiting_for.as_deref().map(WaitingFor::from)),
        _ => RootStatus::Busy,
    }
}

#[derive(Deserialize)]
struct Row {
    #[serde(default, deserialize_with = "lenient")]
    pid: Option<u32>,
    #[serde(default, deserialize_with = "lenient")]
    kind: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    id: Option<String>,
    #[serde(rename = "sessionId", default, deserialize_with = "lenient")]
    session_id: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    status: Option<String>,
    #[serde(rename = "waitingFor", default, deserialize_with = "lenient")]
    waiting_for: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    cwd: Option<String>,
    #[serde(rename = "startedAt", default, deserialize_with = "lenient")]
    started_at: Option<i64>,
}

/// The rows of `claude agents --json`. Only `pid`, `sessionId`, `kind`, `id` (a
/// background session's short id), `cwd`, `startedAt`, `status` (`busy`, `waiting`,
/// `idle`) and, while waiting, `waitingFor` are read (documented; `waitingFor` since 2.1.162). Sessions
/// carry `pid` and `status` only while their process runs; background ones also carry
/// `state`, which is not read. A dormant row is left out: a session reopened in a
/// terminal is listed again, live, under the same session id.
fn parse_agents_json(output: &[u8]) -> Result<Vec<LiveSession>, LivenessError> {
    let rows: Vec<serde_json::Value> = serde_json::from_slice(output)
        .map_err(|error| LivenessError::Failed(format!("unreadable output: {error}")))?;
    Ok(rows
        .into_iter()
        .filter_map(|row| serde_json::from_value::<Row>(row).ok())
        .filter_map(|row| {
            let Classification::Live { short_id } = classify(row.kind.as_deref(), row.pid, row.id)
            else {
                return None;
            };
            Some(LiveSession {
                session_id: row.session_id?,
                status: root_status(row.status.as_deref(), row.waiting_for),
                pid: row.pid,
                short_id,
                cwd: row.cwd,
                started: row.started_at,
            })
        })
        .collect())
}
