//! The main window's keys, caught from GTK for the world (spec: "Keyboard input";
//! sweatshop-rooms ticket 20).
//!
//! `key-press-event` and `key-release-event` handlers on the main window's toplevel see
//! each key before WebKit does, in Wayland's order, on the main thread. They hand every
//! key to the controller by physical key and, while the cursor is captured, keep the
//! keys the world walks with from the page, so a focused panel does not scroll; every
//! other key, Esc above all, reaches the page as before. While the player sits in a
//! session it keeps nothing and the world hears nothing: every key is the seated
//! terminal's (agent-screens spec: "Keys, paste and links"). A `focus-out-event` handler
//! tells the world to let go of every held key, since on Wayland a focus loss sends no
//! key releases.

use gtk::glib::Propagation;
use gtk::prelude::WidgetExt;
use tauri::{AppHandle, Manager};

use super::controller::Controller;
use super::engine::debug;

/// Connects the handlers on the main window. Called once, at setup.
pub fn catch_on_main(app: &AppHandle) {
    let Some(main) = app.get_webview_window("main") else {
        eprintln!("[world] no main window to catch keys on");
        return;
    };
    let window = match main.gtk_window() {
        Ok(window) => window,
        Err(error) => {
            eprintln!("[world] could not reach the main window for keys: {error}");
            return;
        }
    };
    let logged = debug::log_keys();
    for pressed in [true, false] {
        let app = app.clone();
        let handler = move |_: &gtk::ApplicationWindow, event: &gtk::gdk::EventKey| {
            let code = event.hardware_keycode();
            let keep = app.state::<Controller>().key(code, pressed);
            if logged {
                eprintln!(
                    "[keys] gtk {} code {code} keyval {:?} time {}{}",
                    if pressed { "press" } else { "release" },
                    event.keyval().name().unwrap_or_default(),
                    event.time(),
                    if keep { " kept from the page" } else { "" }
                );
            }
            if keep {
                Propagation::Stop
            } else {
                Propagation::Proceed
            }
        };
        if pressed {
            window.connect_key_press_event(handler);
        } else {
            window.connect_key_release_event(handler);
        }
    }
    let app = app.clone();
    window.connect_focus_out_event(move |_, _| {
        if logged {
            eprintln!("[keys] gtk focus out");
        }
        app.state::<Controller>().focus_lost();
        Propagation::Proceed
    });
}
