mod agents;
mod platform;
mod statistics;
mod terminals;
#[cfg(debug_assertions)]
mod timeline;
mod world;

use std::time::Duration;

use tauri::webview::PageLoadEvent;
use tauri::{Manager, RunEvent, WindowEvent};

/// The main window is transparent so the world can show through the world view, and a
/// transparent window that has not painted yet shows the desktop behind it. So it opens
/// hidden and is shown once its page has loaded, with its stylesheets, and paints its
/// own background (spec: "First paint"). If the page never reports in, it is shown
/// anyway after this long.
const SHOW_AT_THE_LATEST: Duration = Duration::from_secs(5);

fn show_main(app: &tauri::AppHandle) {
    if let Some(main) = app.get_webview_window("main") {
        let _ = main.show();
    }
}

// The desktop shell stays thin on purpose: product logic lives in the shared
// frontend (web/src). Add Tauri commands here only when a capability genuinely
// needs the Rust side, as the world (drawn natively under the webview) and the
// session terminals do. The one exception holding product logic is the agent
// registry (ADR-0010, the `agents` crate): it reads Claude Code's and
// opencodex's files and aggregates them here, because shipping them across IPC
// to aggregate in the webview would be hundreds of MB. The app runs entirely on
// the user's machine: no command reaches a server (local-only spec).
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    platform::refuse_unless_native_wayland();
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            // Only the controller is made here; no world starts at launch.
            app.manage(world::controller(app.handle()));
            // Only the registry is made here; it reads nothing until the page asks.
            app.manage(agents::Agents::new(app.handle()));
            // The session terminals attach to nothing until the world asks for a live set.
            app.manage(terminals::SessionTerminals::new(app.handle()));
            world::allow_pointer_lock(app.handle());
            terminals::allow_clipboard(app.handle());
            world::catch_keys(app.handle());
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                std::thread::sleep(SHOW_AT_THE_LATEST);
                show_main(&handle);
            });
            Ok(())
        })
        .on_page_load(|webview, payload| {
            if webview.label() != "main" {
                return;
            }
            match payload.event() {
                PageLoadEvent::Started => world::on_page_loading(webview.app_handle()),
                PageLoadEvent::Finished => show_main(webview.app_handle()),
            }
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                world::on_close_requested(window, api);
            }
        })
        .invoke_handler(tauri::generate_handler![
            world::world_start,
            world::world_stop,
            world::world_status,
            world::world_rect,
            world::world_look,
            world::world_click,
            world::world_shown,
            world::world_go_to_desk,
            world::world_captured,
            world::world_quit,
            world::world_settings,
            world::world_strings,
            world::world_sit_in,
            world::world_leave,
            terminals::terminal_stream,
            terminals::terminal_input,
            agents::agents_snapshot,
            agents::session_new,
            statistics::agents_opencodex_statistics,
            statistics::agents_token_statistics,
            statistics::agents_of_the_month,
        ])
        .build(tauri::generate_context!())
        .expect("error while building the desktop app")
        .run(|app, event| {
            // However the app exits, every attach is hung up first (spec: "The
            // terminals crate", shutdown); exiting runs no destructors.
            if let RunEvent::Exit = event {
                if let Some(terminals) = app.try_state::<terminals::SessionTerminals>() {
                    terminals.shutdown();
                }
            }
        });
}

#[cfg(test)]
mod tests {
    /// The commands build.rs declares in the app manifest, in its order.
    fn declared_commands() -> Vec<String> {
        let build = include_str!("../build.rs");
        let list = build
            .split_once(".commands(&[")
            .and_then(|(_, rest)| rest.split_once("])"))
            .expect("build.rs declares its commands in one .commands(&[...]) list")
            .0;
        list.split(',')
            .map(|item| item.trim().trim_matches('"'))
            .filter(|item| !item.is_empty())
            .map(String::from)
            .collect()
    }

    fn granted_permissions() -> Vec<String> {
        let capability: serde_json::Value =
            serde_json::from_str(include_str!("../capabilities/default.json")).unwrap();
        capability["permissions"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|permission| permission.as_str().map(String::from))
            .collect()
    }

    /// The app runs entirely on the user's machine (local-only spec): the page asks the
    /// shell for the world, the session terminals and the agent registry, and for
    /// nothing that reaches a server. There is no Worker bridge.
    #[test]
    fn the_shell_declares_only_the_world_terminal_and_agent_commands() {
        assert_eq!(
            declared_commands(),
            [
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
            ]
        );
    }

    /// Every declared command is granted to the main window and has the permission file
    /// tauri-build generates for it, and nothing is granted or generated for a command
    /// the shell no longer declares.
    #[test]
    fn grants_and_permission_files_match_the_declared_commands() {
        let declared = declared_commands();
        let mut expected_grants: Vec<String> = declared
            .iter()
            .map(|command| format!("allow-{}", command.replace('_', "-")))
            .collect();
        expected_grants.sort();
        let mut app_grants: Vec<String> = granted_permissions()
            .into_iter()
            .filter(|permission| !permission.contains(':'))
            .collect();
        app_grants.sort();
        assert_eq!(app_grants, expected_grants);

        let mut files: Vec<String> = std::fs::read_dir(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/permissions/autogenerated"
        ))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
        files.sort();
        let mut expected_files: Vec<String> = declared
            .iter()
            .map(|command| format!("{command}.toml"))
            .collect();
        expected_files.sort();
        assert_eq!(files, expected_files);
    }

    /// The seated terminal's Ctrl+click opens links through the opener plugin, which may
    /// open http and https URLs and nothing else: no other scheme, no file, no app of the
    /// page's choosing (agent-screens spec: "Keys, paste and links").
    #[test]
    fn the_opener_is_allowed_only_http_and_https_urls() {
        let capability: serde_json::Value =
            serde_json::from_str(include_str!("../capabilities/default.json")).unwrap();
        let opener: Vec<&serde_json::Value> = capability["permissions"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|permission| {
                let name = permission
                    .as_str()
                    .or_else(|| permission["identifier"].as_str())
                    .unwrap_or_default();
                name.starts_with("opener:")
            })
            .collect();
        assert_eq!(opener.len(), 1, "one opener grant: {opener:?}");
        assert_eq!(opener[0]["identifier"], "opener:allow-open-url");
        assert_eq!(
            opener[0]["allow"],
            serde_json::json!([{ "url": "http://*" }, { "url": "https://*" }])
        );
        assert!(opener[0].get("deny").is_none());
    }

    /// Sitting in from the side panel (and New session) asks the world through
    /// `world_sit_in`: a declared command, so it needs a grant, and only the main
    /// window has one (agent-screens spec: "Controller and commands").
    #[test]
    fn world_sit_in_is_declared_and_granted_to_the_main_window_only() {
        assert!(include_str!("../build.rs").contains("\"world_sit_in\","));
        let capability: serde_json::Value =
            serde_json::from_str(include_str!("../capabilities/default.json")).unwrap();
        assert_eq!(capability["windows"], serde_json::json!(["main"]));
        assert!(capability.get("webviews").is_none());
        assert!(capability["permissions"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("allow-world-sit-in")));
        let capabilities = std::fs::read_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/capabilities"))
            .unwrap()
            .count();
        assert_eq!(capabilities, 1, "default.json is the only capability");
    }

    /// New session starts `claude --bg` through `session_new`: a declared command, so
    /// it needs a grant, and only the main window has one (agent-screens spec:
    /// "Controller and commands").
    #[test]
    fn session_new_is_declared_and_granted_to_the_main_window_only() {
        assert!(include_str!("../build.rs").contains("\"session_new\","));
        let capability: serde_json::Value =
            serde_json::from_str(include_str!("../capabilities/default.json")).unwrap();
        assert_eq!(capability["windows"], serde_json::json!(["main"]));
        assert!(capability["permissions"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("allow-session-new")));
    }
}
