# A Rust agent registry, fed by reading Claude Code's and opencodex's files, is the one source of agents and usage

> **Amended (2026-10-19)** by [ADR-0015](0015-session-terminals-through-claude-attach.md): the app also runs `claude attach` to show background sessions' terminals, writes to them what the user types in the seated view, and starts background sessions on the user's click.

The desktop app learns about the user's agents by reading, never writing, Claude Code's files under the default `~/.claude` (transcripts, subagent `.meta.json`) and opencodex's `usage.jsonl` and quota caches, watched for changes. Liveness comes from the supported `claude agents --json`, with `~/.claude/sessions/<pid>.json` as fallback. One in-memory **agent registry** in the Tauri process owns which agents exist, their state and the aggregated usage; the side panel receives view-ready, metadata-only data from it (a snapshot command plus pushed events for the agent tree, request/response for statistics), and the Bevy world, in the same process, will read the same registry, so the two never disagree. Nothing is written to disk: the index is rebuilt on each app start. Decided with the effort owner on 2026-10-16 ([How data reaches the panel through Rust](../../.scratch/side-panel/issues/07-grilling-how-data-reaches-the-panel.md)).

## Considered Options

- **The Claude Agent SDK's session functions** (via a Node/Bun sidecar): rejected. They read the same files and return less (no live state, no watching, untyped bodies, a parser pinned to the SDK's bundled CLI version); the npm SDK is not openly licensed, and a sidecar is ~92 MB. Instead the reader's parsing is ported from the MIT-licensed Python SDK's session code.
- **Hooks or OpenTelemetry:** more stable contracts, but they require changing the user's Claude Code settings and miss past sessions. Ruled out for now by the owner.
- **Cross-harness protocols** (ACP, Codex app-server, OpenCode server): serve agents the client launches, not sessions already running in another terminal.
- **`ocx usage --json`:** aggregates only; the token view needs each row's `requestId` to attribute usage to agents.
- **Aggregating in the webview:** keeps product logic in `web/`, but a month of ledger is hundreds of MB to ship across IPC. Aggregation lives in Rust, an explicit exception to the shell staying thin.

## Consequences

- Claude Code's transcript format is documented as internal and changing between versions, and opencodex's files carry no published schema. The parsers must tolerate unknown fields and record types, and are tested against captured, scrubbed fixtures; a harness update can still break them.
- Each app start rescans the history (about 30 days of transcripts and the opencodex ledger), so the first panel open shows a loading state.
- The app never reads opencodex credentials and never shows message or tool content.
