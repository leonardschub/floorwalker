# Desktop (Tauri)

Floorwalker's desktop app. Tauri v2 hosts the React app in `web/`. `desktop/`
contains the native shell and Tauri tooling; routes, components, theme,
translations and frontend build tooling live in `web/`.

The app runs entirely on the user's machine. It talks to no server: the page
reaches only the shell, over Tauri's IPC (the CSP's `connect-src` allows
nothing else), and the shell reads Claude Code's and opencodex's files and runs
`claude`. There is no account and no sign-in.

## Platform

The desktop app is **Linux on native Wayland only**. At startup the shell
(`src-tauri/src/platform.rs`) checks that `WAYLAND_DISPLAY` (or
`WAYLAND_SOCKET`) is set and that GTK opened a Wayland display. Otherwise it
exits with status 1 before any window opens, prints the reason on stderr, and
shows a native error dialog when GTK could open some other display. There is
no X11 or XWayland path, and nothing sets `GDK_BACKEND=x11`.

Windows and macOS are unverified: the crate is not required to compile for
them, and nothing is tested there or under X11.

## Develop

Use Node.js 22.12+ (or Node.js 24), pnpm 10, Rust and the
[Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) on Linux,
plus Blender 4.3.x with numpy (Debian: `blender python3-numpy`): `tauri dev` and
`tauri build` regenerate the world's models and textures before cargo runs,
which takes about 10 minutes the first time and seconds after
([`world-assets/README.md`](world-assets/README.md)).
From the repository root:

```sh
pnpm install
pnpm --filter desktop tauri dev
```

Tauri's hook starts `web/` with `vite --mode desktop` on
http://127.0.0.1:5173. There is one Vite server, and you can also open that URL
in a browser. Stop an existing `pnpm dev:web` server before starting Tauri;
Vite's strict port prevents accidentally attaching to a different build mode.
`pnpm --filter desktop dev` is an alias for the full Tauri development app.

For frontend-only work, use `pnpm --filter web dev:desktop` or the usual
`pnpm dev:web` for browser mode. Lint the shared UI with
`pnpm --filter web lint`.

## Build

Both modes build `web/dist`. Tauri embeds that directory using
`../../web/dist`, relative to `desktop/src-tauri/tauri.conf.json`.
The build hook runs `pnpm --filter web build:desktop`; the ordinary browser
build stays `pnpm build:web`. Build modes share an output directory, so run
them sequentially.

```sh
# Native app and platform installers (also rebuilds the frontend):
pnpm --filter desktop tauri build
# Native executable with embedded assets, without creating installers:
pnpm --filter desktop tauri build --no-bundle
```

`pnpm --filter desktop build` is an alias for `tauri build`. Always build the
executable through `tauri build`: a bare `cargo build` embeds whatever
`web/dist` already holds, which may be a stale or browser-mode page build.

### The world

The world screen (`/world`) shows a 3D world drawn natively by Bevy 0.20 on a
`bevy-world` thread in this process (ADR-0009). The app opens on the world
screen, which calls `world_start` as it mounts; the shell itself starts nothing
at launch. The shell then attaches a Wayland
subsurface below the main window's toplevel and Bevy presents into it; the
main window is transparent, and the world view is the part of the page left
unpainted while the world runs. Native Wayland only. The code lives in
`src-tauri/src/world/`: `controller.rs` owns the lifecycle and is tested with
a stand-in world, `engine.rs` is the Bevy launcher. The page's side is
`web/src/world/`, loaded only in the desktop build. The world's commands
(`world_*`), the session terminals' (`terminal_stream`, `terminal_input`) and
the agent registry's (`agents_*`, `session_new`) are declared in
`src-tauri/build.rs`'s app manifest and granted only to `main`; they are the
shell's only commands.

The main window opens hidden and is shown once its page has loaded, so a
transparent window never shows the desktop through an unpainted page.

### Session terminals

Background sessions' terminals are `claude attach` processes the shell runs
under its own PTYs (the `terminals` crate, ADR-0015); the seated view lays an
xterm.js terminal over the PC's glass. Two webview settings follow from it,
both recorded in ADR-0015's amendments:

- The CSP's `style-src` allows `'unsafe-inline'`: xterm.js creates `<style>`
  elements at run time, which can't carry a hash or a nonce. `script-src`
  stays `'self'`.
- WebKitGTK's `javascript-can-access-clipboard` is on for the main webview
  (`terminals::allow_clipboard`), so the seated terminal's Ctrl+Shift+C and
  Ctrl+Shift+V can run `document.execCommand('copy' | 'paste')`. It holds for
  the whole page, which runs only its own script.

Quitting or restarting the world hangs up every attach; the sessions keep
running.

The world logs `[world]` lines to stderr: its lifecycle (start, first frame,
stop, failures) and, every 5 seconds, the world thread's tick count, frames
drawn and yaw/pitch.

## Routes and smoke check

All `web/` routes are included. `BrowserRouter` and root-relative asset URLs
are retained: Tauri's asset handler falls back to `index.html` for route paths.
To check an embedded build without a Vite server:

1. Run `pnpm --filter desktop tauri build --no-bundle` and launch the resulting
   executable, `desktop/src-tauri/target/release/floorwalker`.
2. Press **Settings** on the world screen's top bar, then open the open-source
   licences from **About**; this reaches the `/oss-licenses` route and loads its
   document chunk.
3. Reload the webview (on Linux, right-click a blank area and choose **Reload**).
   Confirm that the document and styling remain visible at the same route.
   Reload shortcuts depend on the platform webview; this shell adds none.

## Icons

`src-tauri/icons/icon.svg` is the single source. Regenerate the platform set
with `pnpm --filter desktop tauri icon src-tauri/icons/icon.svg` when the
brand changes.
