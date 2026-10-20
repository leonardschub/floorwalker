//! The keys the world walks with, and which of them it holds (spec: "Keyboard input";
//! sweatshop-rooms ticket 20).
//!
//! GTK hands the main window's key presses and releases to the controller by physical
//! key: GDK's `hardware_keycode`, on Wayland the XKB keycode (the evdev code + 8). The
//! same physical keys walk on every layout, so on AZERTY the key labelled Z walks
//! forward. This Bevy build has no string-to-`KeyCode` parser, so [`key_code`] names the
//! few keys the world uses itself.
//!
//! [`KeyGate`] sits in front of Bevy's `ButtonInput<KeyCode>`: it lets keys through only
//! while the cursor is captured, and lets go of every held key when capture is released
//! or the window loses focus, since on Wayland a focus loss sends no key releases.

use bevy::input::keyboard::KeyCode;
use bevy::input::ButtonState;
use bevy::prelude::Resource;

/// The world's key for a physical key, if it uses that key: WASD, the arrows and both
/// Shifts.
pub fn key_code(hardware: u16) -> Option<KeyCode> {
    Some(match hardware {
        25 => KeyCode::KeyW,
        38 => KeyCode::KeyA,
        39 => KeyCode::KeyS,
        40 => KeyCode::KeyD,
        111 => KeyCode::ArrowUp,
        113 => KeyCode::ArrowLeft,
        114 => KeyCode::ArrowRight,
        116 => KeyCode::ArrowDown,
        50 => KeyCode::ShiftLeft,
        62 => KeyCode::ShiftRight,
        _ => return None,
    })
}

/// Whether the GTK handler keeps a key from the page while the cursor is captured: the
/// keys the world walks with, so a focused panel does not scroll. Every other key, Esc
/// first of all, reaches the page.
pub fn walks(hardware: u16) -> bool {
    key_code(hardware).is_some()
}

/// One change for Bevy's keyboard state.
pub type Change = (KeyCode, ButtonState);

/// The keys the world holds, and whether it takes keys at all.
#[derive(Resource, Debug, Default)]
pub struct KeyGate {
    captured: bool,
    held: Vec<KeyCode>,
}

impl KeyGate {
    /// A key press or release from GTK. While released, keys are ignored. A press of a
    /// key already held (GDK's own repeats, a press seen twice) and a release of a key
    /// not held (one pressed before capture, or held across a focus round trip) change
    /// nothing.
    pub fn key(&mut self, hardware: u16, pressed: bool) -> Option<Change> {
        let code = key_code(hardware)?;
        let held = self.held.iter().position(|&h| h == code);
        match (pressed, held) {
            (true, None) if self.captured => {
                self.held.push(code);
                Some((code, ButtonState::Pressed))
            }
            (false, Some(at)) => {
                self.held.remove(at);
                Some((code, ButtonState::Released))
            }
            _ => None,
        }
    }

    /// Whether the cursor is captured. A release lets go of every held key.
    pub fn captured(&mut self, captured: bool) -> Vec<Change> {
        self.captured = captured;
        if captured {
            Vec::new()
        } else {
            self.let_go()
        }
    }

    /// The window lost focus: no releases will come for the keys held now, and the
    /// cursor is released with it (Pointer Lock ends on focus loss).
    pub fn focus_lost(&mut self) -> Vec<Change> {
        self.captured(false)
    }

    fn let_go(&mut self) -> Vec<Change> {
        self.held
            .drain(..)
            .map(|code| (code, ButtonState::Released))
            .collect()
    }

    /// The keys held, in the order they were pressed.
    #[cfg(test)]
    pub fn held(&self) -> &[KeyCode] {
        &self.held
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: u16 = 25;
    const SHIFT: u16 = 50;
    const UP: u16 = 111;
    const Q: u16 = 24;
    const ESCAPE: u16 = 9;

    fn captured() -> KeyGate {
        let mut gate = KeyGate::default();
        assert!(gate.captured(true).is_empty());
        gate
    }

    #[test]
    fn the_physical_walking_keys_are_named_and_others_are_not() {
        assert_eq!(key_code(W), Some(KeyCode::KeyW));
        assert_eq!(key_code(UP), Some(KeyCode::ArrowUp));
        assert_eq!(key_code(62), Some(KeyCode::ShiftRight));
        assert!(walks(SHIFT));
        assert!(!walks(Q), "Q (A on AZERTY) passes to the page");
        assert!(!walks(ESCAPE), "Esc always reaches the page");
    }

    #[test]
    fn keys_are_ignored_while_released() {
        let mut gate = KeyGate::default();
        assert_eq!(gate.key(W, true), None);
        assert!(gate.held().is_empty());
        assert_eq!(
            gate.key(W, false),
            None,
            "no release for a press never taken"
        );
    }

    #[test]
    fn a_press_and_its_release_pass_once_and_repeats_change_nothing() {
        let mut gate = captured();
        assert_eq!(
            gate.key(W, true),
            Some((KeyCode::KeyW, ButtonState::Pressed))
        );
        assert_eq!(gate.key(W, true), None, "a repeat or a second sight");
        assert_eq!(gate.key(Q, true), None, "a key the world does not use");
        assert_eq!(gate.held(), [KeyCode::KeyW]);
        assert_eq!(
            gate.key(W, false),
            Some((KeyCode::KeyW, ButtonState::Released))
        );
        assert_eq!(gate.key(W, false), None, "a release with no press");
        assert!(gate.held().is_empty());
    }

    #[test]
    fn a_capture_release_lets_go_of_every_held_key() {
        let mut gate = captured();
        gate.key(W, true);
        gate.key(SHIFT, true);
        assert_eq!(
            gate.captured(false),
            [
                (KeyCode::KeyW, ButtonState::Released),
                (KeyCode::ShiftLeft, ButtonState::Released)
            ]
        );
        assert!(gate.held().is_empty());
        assert_eq!(gate.key(UP, true), None, "released: keys are ignored");
        assert_eq!(gate.key(W, false), None, "the late release changes nothing");

        // When capture returns, nothing is held until a key is pressed again.
        assert!(gate.captured(true).is_empty());
        assert!(gate.held().is_empty());
        assert_eq!(
            gate.key(UP, true),
            Some((KeyCode::ArrowUp, ButtonState::Pressed))
        );
    }

    #[test]
    fn a_focus_loss_lets_go_of_every_held_key_and_releases_capture() {
        let mut gate = captured();
        gate.key(W, true);
        assert_eq!(gate.focus_lost(), [(KeyCode::KeyW, ButtonState::Released)]);
        assert!(gate.held().is_empty());
        assert_eq!(gate.key(W, true), None, "no keys until capture returns");
        // W held across the focus round trip: GDK sends its release with no press.
        assert!(gate.captured(true).is_empty());
        assert_eq!(gate.key(W, false), None);
        assert!(gate.held().is_empty());
    }

    #[test]
    fn a_second_capture_report_keeps_the_held_keys() {
        let mut gate = captured();
        gate.key(W, true);
        assert!(gate.captured(true).is_empty());
        assert_eq!(gate.held(), [KeyCode::KeyW]);
    }
}
