//! A live session's `sessions/<pid>.json`, found by session id and verified, for every
//! action on a real session (ADR-0013's ending, ADR-0014's prompt): nothing is done to a
//! session whose file is not verified.
//!
//! A pid is the session's only while its `sessions/<pid>.json` names the session and the
//! start time Claude Code recorded there as `procStart` is the running process's
//! (`/proc/<pid>/stat` field 22, through [`Processes::start_time`]), so a file a crash
//! left behind, or a pid reused by another process since, is never acted on.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::ending::{signallable, Processes};

/// What a verified session file says.
pub(crate) struct Verified {
    /// The `procStart` that proved it.
    pub proc_start: String,
    /// The Unix socket its inbox listens on (`messagingSocketPath`), if it names one.
    pub messaging_socket: Option<PathBuf>,
}

/// The pid of a `sessions/<pid>.json` that names `session_id`, for liveness data
/// without pids.
pub(crate) fn pid_from_session_files(claude_dir: &Path, session_id: &str) -> Option<u32> {
    let entries = std::fs::read_dir(claude_dir.join("sessions")).ok()?;
    entries.flatten().find_map(|entry| {
        let name = entry.file_name();
        let pid: u32 = name.to_str()?.strip_suffix(".json")?.parse().ok()?;
        let file = read_session_file(claude_dir, pid)?;
        (file.session_id.as_deref() == Some(session_id)).then_some(pid)
    })
}

/// Verifies that the process `pid` is still the session `session_id`'s: returns what its
/// session file says, or why it is not.
pub(crate) fn verify(
    claude_dir: &Path,
    session_id: &str,
    pid: u32,
    processes: &dyn Processes,
) -> Result<Verified, String> {
    if !signallable(pid) {
        return Err(format!("pid {pid} is never signalled"));
    }
    let file = read_session_file(claude_dir, pid)
        .ok_or_else(|| format!("no readable sessions/{pid}.json"))?;
    if file.session_id.as_deref() != Some(session_id) {
        return Err(format!("sessions/{pid}.json names another session"));
    }
    let proc_start = file
        .proc_start
        .ok_or_else(|| format!("sessions/{pid}.json has no procStart"))?;
    match processes.start_time(pid) {
        Some(start) if start == proc_start => Ok(Verified {
            proc_start,
            messaging_socket: file.messaging_socket_path.map(PathBuf::from),
        }),
        Some(start) => Err(format!(
            "the process started at {start}, the session's at {proc_start}: the pid was reused"
        )),
        None => Err("no such process runs".into()),
    }
}

#[derive(Deserialize)]
struct SessionFile {
    #[serde(
        rename = "sessionId",
        default,
        deserialize_with = "crate::liveness::lenient"
    )]
    session_id: Option<String>,
    #[serde(
        rename = "procStart",
        default,
        deserialize_with = "crate::liveness::lenient"
    )]
    proc_start: Option<String>,
    #[serde(
        rename = "messagingSocketPath",
        default,
        deserialize_with = "crate::liveness::lenient"
    )]
    messaging_socket_path: Option<String>,
}

fn read_session_file(claude_dir: &Path, pid: u32) -> Option<SessionFile> {
    let bytes = std::fs::read(claude_dir.join(format!("sessions/{pid}.json"))).ok()?;
    serde_json::from_slice(&bytes).ok()
}
