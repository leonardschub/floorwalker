# The world is a Bevy application on a worker thread

> **Superseded (2026-10-16)** by [ADR-0009](0009-bevy-under-a-transparent-webview-on-wayland.md): the world is embedded in the main window, drawn by Bevy in a Wayland subsurface under a transparent webview. The engine choice and pin carry over; the separate window does not.

The world has to be drawn by the Rust side inside the Tauri process, while Tauri
keeps the main thread and its event loop. It is a Bevy 0.20 application, pinned
to that minor, that creates its own window on a dedicated thread using
`WinitPlugin { run_on_any_thread: true }` — the arrangement Bevy documents for
Linux (X11/Wayland) and Windows. The shell starts the thread on **Enter world**
and stops it when the world window closes; the dashboard hides once the world
reports its first frame, and a failed start reports inline on the dashboard.
Recorded from the 2026-10-16 design interview for ticket 02.

## Considered Options

- **Raw wgpu against a window we manage**: rejected. It saves code on the first
  cube, but the later walkable game would need scenes, assets, input and physics
  written by hand, and a bare Tauri window offers no clean route for the
  keyboard and pointer events a game needs.
- **Bevy rendering into a Tauri-owned window with manual frame pumping** (the
  `sunxfancy/BevyTauriExample` shape): rejected. It alternates two frameworks'
  event loops on the main thread through Tauri's deprecated `run_iteration`, and
  input would still have to be manufactured.
- **Rendering in the webview** (three.js): rejected by the effort owner during
  charting; the research note records it as the baseline this decision was
  taken against.

## Consequences

- macOS does not support this arrangement: Bevy's `run_on_any_thread` is
  documented as Linux and Windows only. When macOS is verified, the attachment
  has to be revisited — a sidecar world process or a different surface path.
  The effort owner accepted a Linux demonstration for this milestone, with
  other platforms reported as unverified.
- Bevy minors land every few months and break APIs; the pin is deliberate and
  upgrades are their own work.
- Two event loops share one process on different threads. The Tauri loop stays
  vanilla; the world thread signals the shell over channels: ready (its first
  frame has been presented), closed, and failed.
- winit builds one event loop per process and never allows a second, so the world
  cannot be stopped and started again. Amended 2026-10-16 by ticket 08, which proved
  this with the real launcher: the world thread starts on the first **Enter world**
  and lives as long as the app. Closing the world window releases the window and its
  scene, and the loop sleeps until the next entry opens a fresh window on it. The
  event loop, the Bevy app and the GPU device stay until the app quits, so "stops it
  when the world window closes" above means the window and scene, not the thread. A
  failure that kills the world thread after its event loop exists cannot be retried
  in the same process. Releasing everything on close would need a world process.
