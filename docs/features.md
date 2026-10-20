# Feature inventory

What the app does, and what it deliberately does not. It is a desktop app for Linux on
native Wayland, and it runs entirely on the user's machine.

| Capability | Implementation |
| --- | --- |
| The world | A Bevy 0.20 world drawn under a transparent webview (ADR-0009): a corridor of project rooms, management rooms, desks and robots, from models and textures Blender generates at build time (ADR-0012). The app opens straight into it |
| Agent registry | The `agents` crate reads Claude Code's and opencodex's files read-only and builds the agent tree (ADR-0010) |
| Side panel | Every session's agent tree, with models, states, flags and tokens, and the Of the month, Tokens and opencodex statistics views; also on its own route, `/agents` |
| Sitting in | A background session's terminal, through `claude attach` under PTYs the app owns, painted onto its PC and typed into from the seated view (ADR-0015) |
| New session | `claude --bg` in a project's folder from the side panel, then sitting in it |
| Punishments | Kill ends the real session by a verified signal (ADR-0013); Work harder sends it a prompt (ADR-0014) |
| Settings | Appearance (a device-local light/dark/system theme), World (subagent size, walking speed, mouse sensitivity, textures, clutter, punishment effects, all stored on the device) and About |
| About and legal | The credit, the AGPL-3.0 notices, the licence text and a generated open-source licences page, validated before every build |
| i18n | A shared i18next package and English dictionary |

What it does not have:

- **No account and no service.** There is no sign-in, no server and nothing sent off
  the machine. The page reaches only the shell, over Tauri's IPC.
- **No browser product.** The browser build of `web/` is for development: it has no
  world, opens on Settings, and shows the side panel on fixtures only in `vite dev`.
- **No other platforms.** Windows, macOS and X11 are unverified, and the shell refuses
  to start without a native Wayland display.

Validation:

- `pnpm test` runs the web tests (vitest in jsdom, the pages as the desktop build
  against a shell faked at its IPC boundary), the legal validator and the
  open-source licences check.
- `cargo test --workspace` in `desktop/src-tauri` runs the shell's, the `agents`
  crate's and the `terminals` crate's tests; the world's controller is tested with a
  stand-in world.
- Each effort under `.scratch/` records its own demonstration in a nested KWin.
