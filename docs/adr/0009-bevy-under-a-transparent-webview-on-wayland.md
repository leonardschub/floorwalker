# The world is drawn by Bevy in a Wayland subsurface under a transparent webview

Supersedes [ADR-0008](0008-bevy-world-on-a-worker-thread.md).

The world is embedded in the desktop app's main window as the **world view**, framed
by the React world screen. Bevy 0.20, pinned to that minor, runs on a worker thread in
the Tauri process with a custom runner and no `bevy_winit`: it never owns a window or
an event loop. The main window is transparent; Bevy presents into its own Wayland
`wl_subsurface`, placed below the GTK toplevel with an empty input region, and the
webview's transparent region shows it. The webview reports the world view's rectangle
to Rust, and Bevy renders with its camera viewport set to it. Pointer input stays with
the webview and is forwarded to Rust; cursor capture uses the webview's own Pointer
Lock. The app supports native Wayland only. Accepted by the effort owner on 2026-10-16
after running the spike from ticket 11 on their own KDE Plasma desktop.

## Considered Options

- **A separate native world window** (ADR-0008): superseded. The owner wants the
  world inside the 2D frontend, in one window.
- **Presenting on the Tauri window's own surface**: fails on Wayland. Bevy's swapchain
  and GTK both commit to the same `wl_surface`, so the HTML chrome disappears. Z-order
  is not the issue; two producers on one surface is.
- **Child webviews beside a Bevy area** (Tauri's `unstable` `add_child`): kept as the
  fallback only, and not needed. Nothing could overlap the world.
- **Bevy compiled to WASM, or three.js in the webview**: rejected by the owner. The
  world stays native Rust.

## Consequences

- Wayland only, by the owner's choice. X11 and XWayland are not supported or tested.
  The subsurface path is Wayland-specific, so Windows and macOS need their own
  attachment when they are verified.
- The integration reaches below Tauri: it wraps GTK's `wl_display` and the toplevel
  `wl_surface` through `with_webview` and binds `wl_subcompositor` itself. Tauri,
  wry, GTK or Bevy upgrades can break it and must be re-verified; the Bevy pin stays
  deliberate.
- The subsurface is desynchronised from GTK's commits, so a frame of mismatch at the
  edges during live resizing is possible.
- Resize and scale changes are forwarded to Bevy by hand.
- With FIFO present, a hidden window blocks the world thread because no frame
  callbacks arrive, which stops the simulation too. Keeping the simulation running
  while drawing is **suspended** needs explicit handling in the build.
- A Pointer Lock request on a seat with no pointer kills the process inside
  WebKitGTK; capture must only start from a real pointer click.
