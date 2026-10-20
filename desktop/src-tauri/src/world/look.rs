//! The camera's turn from look deltas (spec: "Capture"; the spike's look handling).
//!
//! The page sends `movementX/Y` while the cursor is captured, summed per animation
//! frame. Moving right turns the camera right, moving down looks down, and the pitch
//! stops short of straight up and straight down. How far a pixel turns is the Mouse
//! sensitivity setting (`controls`).

use bevy::prelude::Resource;

/// How far up or down the camera can look, in radians.
const PITCH_LIMIT: f32 = 1.4;

/// The camera's turn: yaw about the vertical axis, pitch about the camera's own
/// horizontal axis. Both start at zero, the camera's orientation in the room.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq)]
pub struct Look {
    pub yaw: f32,
    pub pitch: f32,
}

impl Look {
    /// Turns by one batch of mouse movement, `radians_per_pixel` for each pixel.
    pub fn turn(&mut self, dx: f32, dy: f32, radians_per_pixel: f32) {
        self.yaw -= dx * radians_per_pixel;
        self.pitch = (self.pitch - dy * radians_per_pixel).clamp(-PITCH_LIMIT, PITCH_LIMIT);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::controls::{Controls, BASE_LOOK};

    #[test]
    fn moving_right_turns_right_and_moving_down_looks_down() {
        let mut look = Look::default();
        look.turn(400.0, 200.0, BASE_LOOK);
        assert!((look.yaw - -1.0).abs() < 1e-6, "yaw {}", look.yaw);
        assert!((look.pitch - -0.5).abs() < 1e-6, "pitch {}", look.pitch);
    }

    #[test]
    fn pitch_is_clamped_and_yaw_is_not() {
        let mut look = Look::default();
        look.turn(-4000.0, -4000.0, BASE_LOOK);
        assert_eq!(look.pitch, 1.4);
        assert!((look.yaw - 10.0).abs() < 1e-4, "yaw {}", look.yaw);
        look.turn(0.0, 8000.0, BASE_LOOK);
        assert_eq!(look.pitch, -1.4);
    }

    #[test]
    fn the_sensitivity_scales_the_turn_and_the_pitch_limit_still_holds() {
        let mut slow = Look::default();
        slow.turn(100.0, 40.0, Controls::new(2.1, 0.5).radians_per_pixel());
        let mut fast = Look::default();
        fast.turn(100.0, 40.0, Controls::new(2.1, 3.0).radians_per_pixel());
        assert!(
            (fast.yaw / slow.yaw - 6.0).abs() < 1e-4,
            "{fast:?} {slow:?}"
        );
        assert!(
            (fast.pitch / slow.pitch - 6.0).abs() < 1e-4,
            "{fast:?} {slow:?}"
        );
        let mut default = Look::default();
        default.turn(400.0, 0.0, Controls::default().radians_per_pixel());
        assert!((default.yaw - -1.5).abs() < 1e-5, "yaw {}", default.yaw);
        fast.turn(0.0, 4000.0, Controls::new(2.1, 3.0).radians_per_pixel());
        assert_eq!(fast.pitch, -1.4);
    }
}
