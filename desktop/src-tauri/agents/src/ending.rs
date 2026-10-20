//! Ending a live session: what a KILL in the CCTV room does to the real Claude Code
//! session at the blast (ADR-0013, which amends ADR-0010's read-only rule for this one
//! effect).
//!
//! The session's `claude` process is found by session id through the liveness data (the
//! pid of its `claude agents --json` row, or of its `sessions/<pid>.json` file), and is
//! signalled only once that pid is verified to still be the session's (`session_file`:
//! its `sessions/<pid>.json` names the session, and the start time Claude Code recorded
//! there as `procStart` is the running process's, `/proc/<pid>/stat` field 22), so a pid
//! reused by another process is never signalled. SIGTERM first; if the same process is
//! still running after the grace, SIGKILL after the same check. Every step is logged.
//!
//! The OS is behind [`Processes`], so tests use a stand-in.

use std::path::Path;
use std::time::{Duration, Instant};

use crate::session_file::{pid_from_session_files, verify};

/// A signal [`Processes::signal`] sends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Signal {
    Term,
    Kill,
}

/// The OS's processes, as ending a session needs them. [`SystemProcesses`] is the real
/// one; tests use a stand-in.
pub trait Processes: Send + Sync {
    /// The start time of the running process `pid`, in clock ticks after boot
    /// (`/proc/<pid>/stat` field 22, what Claude Code records as `procStart`), or none
    /// when no such process runs (a zombie counts as none).
    fn start_time(&self, pid: u32) -> Option<String>;

    /// Sends `signal` to the process `pid`.
    fn signal(&self, pid: u32, signal: Signal) -> Result<(), String>;
}

/// The real processes: `/proc` and `kill(2)`.
pub struct SystemProcesses;

impl Processes for SystemProcesses {
    fn start_time(&self, pid: u32) -> Option<String> {
        crate::liveness::process_start(pid)
    }

    fn signal(&self, pid: u32, signal: Signal) -> Result<(), String> {
        // Never a pid kill(2) reads as a process group or as every process.
        if !signallable(pid) {
            return Err(format!("pid {pid} is never signalled"));
        }
        let number = match signal {
            Signal::Term => libc::SIGTERM,
            Signal::Kill => libc::SIGKILL,
        };
        // SAFETY: kill(2) with a positive pid and a valid signal number touches no memory.
        if unsafe { libc::kill(pid as libc::pid_t, number) } == 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error().to_string())
        }
    }
}

/// Whether a pid may be signalled at all: a single process (kill(2) reads 0 and
/// negative pids as process groups, and -1 as every process), never init, and never
/// this app.
pub(crate) fn signallable(pid: u32) -> bool {
    pid > 1 && i32::try_from(pid).is_ok() && pid != std::process::id()
}

/// How ending a session went.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ending {
    /// The session is not live (or its process could not be found): nothing was
    /// signalled.
    NotLive,
    /// The pid found could not be verified as the session's: nothing was signalled.
    Unverified { pid: u32, why: String },
    /// SIGTERM was sent and the process was gone within the grace.
    Terminated { pid: u32 },
    /// The same process was still running after the grace: SIGKILL was sent.
    Killed { pid: u32 },
    /// A signal could not be sent.
    Failed { pid: u32, why: String },
}

/// How often the grace checks whether the process is gone.
const POLL: Duration = Duration::from_millis(100);

/// Ends the session `session_id` whose process is `pid` (from the liveness data), after
/// verifying it against `claude_dir`'s `sessions/<pid>.json`; see the module docs.
/// Blocks for up to `grace`.
pub(crate) fn end(
    claude_dir: &Path,
    session_id: &str,
    pid: Option<u32>,
    processes: &dyn Processes,
    grace: Duration,
) -> Ending {
    let Some(pid) = pid.or_else(|| pid_from_session_files(claude_dir, session_id)) else {
        eprintln!("[agents] end session {session_id}: not live, nothing signalled");
        return Ending::NotLive;
    };
    let proc_start = match verify(claude_dir, session_id, pid, processes) {
        Ok(verified) => verified.proc_start,
        Err(why) => {
            eprintln!(
                "[agents] end session {session_id}: pid {pid} not verified ({why}), nothing signalled"
            );
            return Ending::Unverified { pid, why };
        }
    };
    eprintln!(
        "[agents] end session {session_id}: pid {pid} verified (procStart {proc_start}), sending SIGTERM"
    );
    if let Err(why) = processes.signal(pid, Signal::Term) {
        eprintln!("[agents] end session {session_id}: SIGTERM to pid {pid} failed: {why}");
        return Ending::Failed { pid, why };
    }
    let same = || processes.start_time(pid).as_deref() == Some(proc_start.as_str());
    let deadline = Instant::now() + grace;
    while same() {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            eprintln!(
                "[agents] end session {session_id}: pid {pid} still running after {grace:?}, sending SIGKILL"
            );
            return match processes.signal(pid, Signal::Kill) {
                Ok(()) => Ending::Killed { pid },
                Err(why) => {
                    eprintln!(
                        "[agents] end session {session_id}: SIGKILL to pid {pid} failed: {why}"
                    );
                    Ending::Failed { pid, why }
                }
            };
        }
        std::thread::sleep(left.min(POLL));
    }
    eprintln!("[agents] end session {session_id}: pid {pid} has exited");
    Ending::Terminated { pid }
}
