//! Whether a background session's process is running, from Claude Code's session files
//! (`~/.claude/sessions/<pid>.json`: `kind: "bg"`, `jobId` = the short id, `pid`,
//! `procStart`), read-only.
//!
//! `claude attach` on a stopped or dormant session starts it again (ticket 10's real
//! run), so the crate attaches only while a process backs the session: a live set a few
//! seconds stale, or an attach leaving a session that a kill just ended, never wakes it
//! (spec "Out of Scope": waking dormant sessions).

use std::path::Path;

/// Whether `sessions` holds a background session file for `short_id` whose process is
/// alive and, when the file records its start time, is the same process.
pub(crate) fn running(sessions: &Path, short_id: &str) -> bool {
    let Ok(entries) = std::fs::read_dir(sessions) else {
        return false;
    };
    entries.flatten().any(|entry| {
        let path = entry.path();
        if path.extension().is_none_or(|extension| extension != "json") {
            return false;
        }
        let Some(file) = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        else {
            return false;
        };
        if file["kind"] != "bg" || file["jobId"] != short_id {
            return false;
        }
        let Some(pid) = file["pid"].as_u64() else {
            return false;
        };
        match start_time(pid) {
            None => false,
            Some(started) => file["procStart"]
                .as_str()
                .is_none_or(|recorded| recorded == started),
        }
    })
}

/// The start time of process `pid` (field 22 of `/proc/<pid>/stat`), as Claude Code
/// records it in `procStart`; `None` if there is no such process.
fn start_time(pid: u64) -> Option<String> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // The command name, field 2, is in parentheses and may hold spaces.
    let after_name = stat.rsplit_once(')')?.1;
    after_name.split_whitespace().nth(19).map(str::to_owned)
}
