//! WebKitGTK's Pointer Lock permission, answered for the main webview (spec: "Capture";
//! the spike's `allow_pointer_lock`).
//!
//! The page captures the cursor through the webview's own Pointer Lock, and WebKitGTK
//! asks the embedder before granting it. The main webview answers allow; every other
//! permission request is left unhandled, so it keeps WebKit's default handling.

use gtk::glib::prelude::ObjectExt;
use tauri::{AppHandle, Manager};
use webkit2gtk::{PermissionRequestExt, PointerLockPermissionRequest, WebViewExt};

/// Connects the answer on the main webview. Called once, at setup.
pub fn allow_on_main(app: &AppHandle) {
    let Some(main) = app.get_webview_window("main") else {
        eprintln!("[world] no main window to allow pointer lock on");
        return;
    };
    let connected = main.with_webview(|platform| {
        platform.inner().connect_permission_request(|_, request| {
            if !request.is::<PointerLockPermissionRequest>() {
                return false;
            }
            eprintln!("[world] pointer lock permission requested -> allow");
            request.allow();
            true
        });
    });
    if let Err(error) = connected {
        eprintln!("[world] could not reach the main webview for pointer lock: {error}");
    }
}
