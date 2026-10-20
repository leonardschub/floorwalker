//! The desktop app runs on native Wayland only (spec: Platform; ADR-0009).
//! There is no X11 or XWayland path, and nothing here sets `GDK_BACKEND`.

use gtk::prelude::*;

/// Exits the process before any window opens unless GTK's default display is
/// a native Wayland display. The reason goes to stderr and, when GTK could
/// open some other display, to a native error dialog as well.
pub fn refuse_unless_native_wayland() {
    // GTK picks the backend exactly as Tauri's event loop will a moment later.
    let display = gtk::init().ok().and_then(|()| gtk::gdk::Display::default());
    let backend = display.as_ref().map(|d| d.type_().name());
    let named = ["WAYLAND_DISPLAY", "WAYLAND_SOCKET"]
        .iter()
        .any(|name| std::env::var_os(name).is_some_and(|value| !value.is_empty()));
    let Err(message) = check_session(named, backend) else {
        return;
    };
    eprintln!("{message}");
    if display.is_some() {
        let dialog = gtk::MessageDialog::new(
            None::<&gtk::Window>,
            gtk::DialogFlags::MODAL,
            gtk::MessageType::Error,
            gtk::ButtonsType::Close,
            &message,
        );
        dialog.run();
        dialog.close();
    }
    std::process::exit(1);
}

const NEEDS_WAYLAND: &str = "This app supports only a native Wayland session and will not start.";

/// Decides whether this is a native Wayland session from two facts:
/// whether the environment names a Wayland display (`WAYLAND_DISPLAY` or
/// `WAYLAND_SOCKET`), and the type name of the `GdkDisplay` GTK opened (for
/// example `GdkWaylandDisplay`), or `None` when GTK could open no display.
/// Both are needed: GTK falls back to `wayland-0` when nothing names one.
pub fn check_session(wayland_named: bool, display: Option<&str>) -> Result<(), String> {
    if !wayland_named {
        return Err(format!("{NEEDS_WAYLAND} WAYLAND_DISPLAY is not set."));
    }
    match display {
        Some("GdkWaylandDisplay") => Ok(()),
        Some(other) => Err(format!(
            "{NEEDS_WAYLAND} GTK opened a {other} instead of a Wayland display."
        )),
        None => Err(format!(
            "{NEEDS_WAYLAND} No display could be opened at WAYLAND_DISPLAY."
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::check_session;

    const NAMED: bool = true;

    #[test]
    fn a_native_wayland_display_is_accepted() {
        assert_eq!(check_session(NAMED, Some("GdkWaylandDisplay")), Ok(()));
    }

    #[test]
    fn no_display_at_all_is_refused_with_a_message_naming_wayland() {
        let message = check_session(NAMED, None).unwrap_err();
        assert!(message.contains("native Wayland session"), "{message}");
        assert!(message.contains("WAYLAND_DISPLAY"), "{message}");
    }

    #[test]
    fn an_x11_display_is_refused_with_a_message_naming_what_was_found() {
        let message = check_session(NAMED, Some("GdkX11Display")).unwrap_err();
        assert!(message.contains("native Wayland session"), "{message}");
        assert!(message.contains("GdkX11Display"), "{message}");
    }

    #[test]
    fn a_wayland_display_reached_without_wayland_display_set_is_refused() {
        // GTK falls back to `wayland-0` when WAYLAND_DISPLAY is unset.
        let message = check_session(!NAMED, Some("GdkWaylandDisplay")).unwrap_err();
        assert!(message.contains("native Wayland session"), "{message}");
        assert!(message.contains("WAYLAND_DISPLAY is not set"), "{message}");
    }
}
