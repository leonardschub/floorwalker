use tauri_build::{AppManifest, Attributes};

fn main() {
    // The world's generated assets are bundled from the regeneration output (ADR-0012).
    // `tauri dev` and `tauri build` fill it first; a bare `cargo build` or `cargo test`
    // may run before any regeneration, so an empty folder stands in and the world's
    // start then fails on its missing assets instead of the build failing here.
    std::fs::create_dir_all("../world-assets/out").expect("create the world-assets output folder");

    // Declaring the app's commands makes each one need an explicit capability grant.
    tauri_build::try_build(
        Attributes::new().app_manifest(AppManifest::new().commands(&[
            "world_start",
            "world_stop",
            "world_status",
            "world_rect",
            "world_look",
            "world_click",
            "world_shown",
            "world_go_to_desk",
            "world_captured",
            "world_quit",
            "world_settings",
            "world_strings",
            "world_sit_in",
            "world_leave",
            "terminal_stream",
            "terminal_input",
            "agents_snapshot",
            "agents_opencodex_statistics",
            "agents_token_statistics",
            "agents_of_the_month",
            "session_new",
        ])),
    )
    .expect("failed to run tauri-build");
}
