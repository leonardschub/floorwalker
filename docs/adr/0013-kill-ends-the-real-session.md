# KILL in the CCTV room ends the real Claude Code session, by a verified signal at the blast

Amends [ADR-0010](0010-rust-agent-registry-from-read-only-files.md): the registry still reads Claude Code's files and never writes them, but it may now stop one session's process, for this one effect only. Decided by the effort owner on 2026-10-19, overturning the management rooms' "show only" rule (`.scratch/management-rooms/spec.md`, Out of Scope).

A kill now ends the agent's session. The session is ended at the blast (the kill's Boom phase; with Punishment effects off, at the same moment on the same timeline). Once KILL fires, the kill always goes through: a change of the agent's state or flag no longer cancels it. It is cleared only if the session ends or its desk goes before the blast; from the blast on, the session ending is the kill's own doing and TARGET NEUTRALISED plays out. Only root agents can be killed, and killing ends the whole session. Work harder stays a show. (Since amended: work harder sends a prompt at the bolt, [ADR-0014](0014-work-harder-sends-a-prompt.md).)

Ending a session is SIGTERM to its `claude` process, then SIGKILL if the same process is still running about 5 s later:

- The process is found by session id through the registry's liveness data: the `pid` of its `claude agents --json` row, or of the `sessions/<pid>.json` file that names the session.
- It is signalled only once verified: `sessions/<pid>.json` must name the session, and the start time Claude Code recorded there as `procStart` must equal the running process's (`/proc/<pid>/stat` field 22), so a pid reused by another process is never signalled. SIGKILL repeats the start-time check. Pids 0 and 1, anything kill(2) would read as a group, and the app itself are never signalled. Nothing unverified is ever signalled, and every step is logged (`[agents] end session …`).
- The world never reaches into the OS. At the blast the CCTV state machine asks once for the session to be ended; the world hands that to the shell through its reports to the controller, and the shell asks the `agents` crate, which signals behind a `Processes` trait so tests use a stand-in. A scripted debug timeline's sessions are never real, so with one playing nothing is signalled.

## Considered Options

- **Ending the session when KILL fires:** the squad would storm a room whose agent is already gone. The blast is the moment the show says the agent dies.
- **Asking Claude Code to exit through its messaging socket:** not a documented contract; a signal to a verified process is.
- **Signalling the pid from `claude agents --json` alone:** the row carries no start time, so a pid reused since could be someone else's process. The session file's `procStart` proves it.

## Consequences

- A kill costs the user the session's work in progress, by design; the flip cover over KILL stays the guard against a slip.
- The wreck is kept against the agent as it is when the kill ends, and the session ending clears it, so in practice the robot (or, with only live sessions given desks, the desk) goes soon after the blast.
- A world stopped after the blast still passes the request on: a kill that has blown always goes through.
