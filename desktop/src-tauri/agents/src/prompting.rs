//! Sending a prompt into a live session: what WORK HARDER in the CCTV room does to the
//! real Claude Code session at the bolt (ADR-0014, which amends ADR-0010's read-only rule
//! for this one effect).
//!
//! It goes through Claude Code's own cross-session messaging: each interactive session
//! listens on a Unix socket it names as `messagingSocketPath` in its
//! `sessions/<pid>.json`. The session's process is found as ending a session finds it
//! (ADR-0013: the pid of the liveness data, or of the session file naming it) and its
//! socket is written to only once that session file is verified (`session_file`: it
//! names the session, and its `procStart` is the running process's), so a file a crash
//! left behind, or a pid reused since, is never written to. The one line written, then
//! half-closed, is a user message carrying the session's id, which the session checks
//! too:
//!
//! `{"type":"user","session_id":"<id>","message":{"role":"user","content":"<text>"}}`
//!
//! Claude Code then queues it as a prompt, or, in a session running with bypass
//! permissions, holds it for the user's approval; nothing comes back on the connection.
//! Every step is logged.

use std::io::Write;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::ending::Processes;
use crate::session_file::{pid_from_session_files, verify};

/// How long a write to a session's socket may block.
const WRITE_TIMEOUT: Duration = Duration::from_secs(2);

/// How sending a prompt went.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Prompting {
    /// The session is not live (or its process could not be found): nothing was sent.
    NotLive,
    /// The session file found could not be verified as a running session's: nothing
    /// was sent.
    Unverified { pid: u32, why: String },
    /// The session names no messaging socket: nothing was sent.
    NoSocket { pid: u32 },
    /// The line was written to the session's socket.
    Sent { pid: u32, socket: PathBuf },
    /// The socket could not be reached or written to.
    Failed { pid: u32, why: String },
}

/// Sends `text` into the session `session_id` whose process is `pid` (from the liveness
/// data), after verifying it against `claude_dir`'s `sessions/<pid>.json`; see the module
/// docs.
pub(crate) fn send(
    claude_dir: &Path,
    session_id: &str,
    pid: Option<u32>,
    processes: &dyn Processes,
    text: &str,
) -> Prompting {
    let Some(pid) = pid.or_else(|| pid_from_session_files(claude_dir, session_id)) else {
        eprintln!("[agents] prompt session {session_id}: not live, nothing sent");
        return Prompting::NotLive;
    };
    let verified = match verify(claude_dir, session_id, pid, processes) {
        Ok(verified) => verified,
        Err(why) => {
            eprintln!(
                "[agents] prompt session {session_id}: pid {pid} not verified ({why}), nothing sent"
            );
            return Prompting::Unverified { pid, why };
        }
    };
    let Some(socket) = verified.messaging_socket else {
        eprintln!(
            "[agents] prompt session {session_id}: pid {pid} names no messaging socket, nothing sent"
        );
        return Prompting::NoSocket { pid };
    };
    eprintln!(
        "[agents] prompt session {session_id}: pid {pid} verified (procStart {}), writing to {}",
        verified.proc_start,
        socket.display()
    );
    match write_line(&socket, &line(session_id, text)) {
        Ok(()) => {
            eprintln!(
                "[agents] prompt session {session_id}: sent; Claude Code queues it, or holds it for the user's approval in a session with bypass permissions"
            );
            Prompting::Sent { pid, socket }
        }
        Err(error) => {
            let why = error.to_string();
            eprintln!(
                "[agents] prompt session {session_id}: writing to pid {pid}'s socket failed: {why}"
            );
            Prompting::Failed { pid, why }
        }
    }
}

/// The one line a session's inbox reads as a user message to it.
fn line(session_id: &str, text: &str) -> String {
    let message = serde_json::json!({
        "type": "user",
        "session_id": session_id,
        "message": { "role": "user", "content": text },
    });
    format!("{message}\n")
}

fn write_line(socket: &Path, line: &str) -> std::io::Result<()> {
    let mut stream = UnixStream::connect(socket)?;
    stream.set_write_timeout(Some(WRITE_TIMEOUT))?;
    stream.write_all(line.as_bytes())?;
    stream.shutdown(std::net::Shutdown::Write)
}
