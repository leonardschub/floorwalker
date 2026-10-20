# WORK HARDER sends "WORK HARDER" into the real Claude Code session, at the bolt, through Claude Code's own messaging socket

Amends [ADR-0010](0010-rust-agent-registry-from-read-only-files.md): the registry still reads Claude Code's files and never writes them, but it may now write one line to a session's messaging socket, for this one effect only. Decided by the effort owner on 2026-10-19, overturning the management rooms' "show only" rule for work harder (`.scratch/management-rooms/spec.md`, Out of Scope). The research behind it: [sending a prompt to a session](../../.scratch/management-rooms/research/sending-a-prompt-to-a-session.md).

Work harder now writes to the agent's session: at the bolt (its Strike phase; with Punishment effects off, at the same moment on the same timeline), the words `WORK HARDER` are sent into the session as a user prompt, once. A work harder cleared before the bolt (the agent's state or flag changed, it finished, or its desk went) sends nothing; one cleared after it has already sent. Only root agents are flagged, so only root sessions are ever prompted.

How it is sent is Claude Code's own cross-session messaging, the transport its sessions use to message each other:

- The session's `claude` process is found and verified exactly as a kill's is ([ADR-0013](0013-kill-ends-the-real-session.md), the shared `session_file` module): the pid of its `claude agents --json` row, or of the `sessions/<pid>.json` that names it; then that file must name the session and the start time Claude Code recorded as `procStart` must equal the running process's (`/proc/<pid>/stat` field 22), so a file a crash left behind or a pid reused since is never written to.
- The app connects to the Unix socket the file names as `messagingSocketPath`, writes one newline-terminated JSON line and half-closes: `{"type":"user","session_id":"<id>","message":{"role":"user","content":"WORK HARDER"}}`. Claude Code drops a line whose `session_id` is not its own. No priority is given (Claude Code's default, `next`: queued after the current turn, never interrupting it), no auth token is sent (optional on Linux; the socket's directory is the user's own), and nothing is read back.
- In a session running with bypass permissions (`--dangerously-skip-permissions`), Claude Code holds such a message for the user's approval before it reaches the conversation (its "permission-mode parity" rule). That is accepted: the app never changes a Claude Code setting (such as `crossSessionInbound`) to get around it. In a prompting mode the line is queued and submitted as a user turn.
- The world never reaches into the OS. Both punishments share one path: at its step each asks once for a `SessionRequest` (`SessionAction::WorkHarder` at the bolt, `SessionAction::End` at a kill's blast); the world hands it to the shell through its reports to the controller, and the shell asks the `agents` crate on a thread of its own (`Registry::prompt_session` or `Registry::end_session`, behind the same `Processes` trait). Every step is logged (`[agents] prompt session …`). A scripted debug timeline's sessions are never real, so with one playing nothing is sent.

## Considered Options

- **Typing into the terminal** (QTerminal's D-Bus `sendText`): it cannot be addressed by pid, and would type into whatever is in the foreground of a pane, a shell included.
- **`TIOCSTI` keystroke injection:** disabled by the kernel on this machine (`legacy_tiocsti = 0`).
- **tmux or screen `send-keys`:** the owner's sessions do not run under a multiplexer.
- **Sending when WORK HARDER is clicked:** the bolt is the moment the show says the agent is told; clicking and clearing during the crackle stays free.
- **Setting `crossSessionInbound` to accept, or asserting a permission mode:** it would change the user's Claude Code settings or forge a sender's trust; the hold is Claude Code's guard and stays.

## Consequences

- The socket protocol is Claude Code's internal one (read from 2.1.303), not a documented contract; a Claude Code update can change or remove it, and the send then fails, logged, while the show still plays.
- Against the owner's bypass-permissions sessions each WORK HARDER asks the owner to approve it in that session before the agent sees it.
- The agent receives a real prompt, which it answers as a user turn; work harder is no longer harmless to the session's flow.
