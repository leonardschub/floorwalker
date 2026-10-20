//! Starting a new background session (agent-screens spec "The side panel": New
//! session): `claude --bg` with no prompt, no name and no flags, run in a project's
//! folder with the app's own Claude Code variables removed, and the short id it prints.
//!
//! What it prints (2.1.303, to stdout; "Starting background service…" goes to stderr):
//!
//! ```text
//! backgrounded · 2ca84613 (idle — send a prompt to start)
//!   claude agents             list sessions
//!   claude attach 2ca84613    open in this terminal
//! ```
//!
//! An untrusted folder fails at once with exit 1 and `Workspace not trusted. Run
//! `claude` in <folder> once and accept the trust prompt, then retry.` on stderr (there
//! is no dialog to accept outside a terminal).
//!
//! One [`Deadline`], [`NEW_SESSION_WITHIN`] from the press, covers both `claude --bg`
//! and the wait for the tree to list what it started ([`crate::Registry::await_session`]):
//! the page waits for the command's answer and keeps no clock of its own. A `claude
//! --bg` still running at the deadline is left to finish, never stopped: Claude Code's
//! background service may already have started the session, and the app never stops a
//! session on its own (agent-screens story 48).

use std::io;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;

/// How long New session has, from the press, for `claude --bg` to answer and the tree
/// to list the session it started (agent-screens spec "The side panel": "no desk
/// appears within 15 s"). `claude --bg` returned in about a second on the owner's
/// machine, longer when it first starts Claude Code's background service.
pub const NEW_SESSION_WITHIN: Duration = Duration::from_secs(15);

/// When New session gives up: one moment for `claude --bg` and the wait after it.
#[derive(Clone, Copy, Debug)]
pub struct Deadline {
    at: Instant,
    within: Duration,
}

impl Deadline {
    /// `within` from now.
    pub fn after(within: Duration) -> Deadline {
        Deadline {
            at: Instant::now() + within,
            within,
        }
    }

    /// Whether it has passed, or will have `after` from now.
    pub fn passed_in(&self, after: Duration) -> bool {
        Instant::now() + after > self.at
    }

    /// What giving up at it tells the page.
    pub fn timed_out(&self) -> NewSessionError {
        NewSessionError::TimedOut {
            seconds: self.within.as_secs(),
        }
    }
}

/// How long its output is still read after it exits: the background service it may
/// start keeps nothing of its pipes, but nothing waits on that.
const DRAIN: Duration = Duration::from_millis(500);

/// Why New session started nothing. Serialized for the page as
/// `{ error: 'claude-not-found' | 'no-folder' | 'not-trusted' }`,
/// `{ error: 'timed-out', seconds }` or `{ error: 'failed', message }`.
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "error", rename_all = "kebab-case")]
pub enum NewSessionError {
    /// No `claude` binary on `PATH` or at `~/.local/bin/claude`.
    ClaudeNotFound,
    /// The project has no session with a working directory to start in.
    NoFolder,
    /// Claude Code refused the folder: it was never trusted.
    NotTrusted,
    /// `claude --bg` did not answer, or no desk appeared, within `seconds` of the press.
    TimedOut { seconds: u64 },
    /// Anything else, with Claude Code's own message.
    Failed { message: String },
}

/// Runs `claude --bg` in `folder` and answers the new session's short id, or gives up
/// at `deadline` and leaves it running.
pub(crate) fn start(
    claude: &Path,
    folder: &Path,
    deadline: Deadline,
) -> Result<String, NewSessionError> {
    let mut command = Command::new(claude);
    command
        .arg("--bg")
        .current_dir(folder)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // Out of the app's process group, like the session it starts.
        .process_group(0);
    crate::without_claude_code_variables(&mut command);
    let mut child = command.spawn().map_err(|error| NewSessionError::Failed {
        message: error.to_string(),
    })?;
    let stdout = collect(child.stdout.take().expect("stdout is piped"));
    let stderr = collect(child.stderr.take().expect("stderr is piped"));
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if !deadline.passed_in(Duration::ZERO) => {
                std::thread::sleep(Duration::from_millis(20))
            }
            Ok(None) => {
                eprintln!("[agents] claude --bg is still running at the deadline: left to finish");
                std::thread::spawn(move || child.wait());
                return Err(deadline.timed_out());
            }
            Err(error) => {
                return Err(NewSessionError::Failed {
                    message: error.to_string(),
                })
            }
        }
    };
    let (stdout, stderr) = (stdout.take(), stderr.take());
    match short_id(&stdout) {
        Some(id) if status.success() => Ok(id),
        _ => Err(failure(&stderr, &stdout, &status.to_string())),
    }
}

/// The new session's short id from what `claude --bg` printed: the id after
/// `backgrounded ·`, else the one `claude attach` is shown with.
pub(crate) fn short_id(stdout: &str) -> Option<String> {
    let id = |word: Option<&str>| {
        word.filter(|w| !w.is_empty() && w.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'))
            .map(str::to_string)
    };
    let lines = || stdout.lines().map(str::trim);
    lines()
        .find_map(|line| {
            id(line
                .strip_prefix("backgrounded ·")?
                .split_whitespace()
                .next())
        })
        .or_else(|| {
            lines().find_map(|line| {
                id(line
                    .strip_prefix("claude attach ")?
                    .split_whitespace()
                    .next())
            })
        })
}

/// What a failed start says: an untrusted folder as such, else Claude Code's message
/// (stderr's lines, else stdout's, without the background service's progress line).
fn failure(stderr: &str, stdout: &str, status: &str) -> NewSessionError {
    if stderr.contains("Workspace not trusted") {
        return NewSessionError::NotTrusted;
    }
    let said = |text: &str| {
        text.lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with("Starting background service"))
            .collect::<Vec<_>>()
            .join(" ")
    };
    let message = [said(stderr), said(stdout)]
        .into_iter()
        .find(|message| !message.is_empty())
        .unwrap_or_else(|| format!("claude --bg exited with {status}"));
    NewSessionError::Failed { message }
}

/// What a pipe's reader thread has read so far, and whether the pipe has ended.
#[derive(Default)]
struct PipeRead {
    bytes: Vec<u8>,
    ended: bool,
}

/// A pipe read to its end on a thread of its own.
struct Collected(Arc<Mutex<PipeRead>>);

fn collect(mut pipe: impl io::Read + Send + 'static) -> Collected {
    let shared = Arc::new(Mutex::new(PipeRead::default()));
    let reading = shared.clone();
    std::thread::spawn(move || {
        let mut chunk = [0u8; 4096];
        loop {
            match pipe.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(n) => reading.lock().unwrap().bytes.extend_from_slice(&chunk[..n]),
            }
        }
        reading.lock().unwrap().ended = true;
    });
    Collected(shared)
}

impl Collected {
    /// What was read, once the pipe has ended or after [`DRAIN`].
    fn take(&self) -> String {
        let until = Instant::now() + DRAIN;
        while !self.0.lock().unwrap().ended && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(10));
        }
        String::from_utf8_lossy(&self.0.lock().unwrap().bytes).into_owned()
    }
}
