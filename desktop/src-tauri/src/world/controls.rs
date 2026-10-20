//! How fast the player walks and how far the mouse turns them (sweatshop-rooms ticket
//! 28): two world options on Settings, sent with the others in `world_settings`. Pure and
//! free of Bevy apart from the resource derive.
//!
//! The owner asked on 2026-10-18 for both to be faster and settable. The defaults are the
//! spec's first values ×1.5: walking 1.4 → 2.1 m/s (running 3.0 → 4.5 m/s) and looking
//! 0.0025 → 0.00375 radians per pixel.

use std::ops::RangeInclusive;

use bevy::prelude::Resource;

/// The Walking speed slider's range, in metres per second, and its default.
pub const WALK_SPEED: RangeInclusive<f32> = 1.0..=4.0;
pub const DEFAULT_WALK_SPEED: f32 = 2.1;
/// Running is this many times walking: the spec's first 3 m/s over its 1.4 m/s.
pub const RUN_PER_WALK: f32 = 3.0 / 1.4;

/// The Mouse sensitivity slider's range and default, as a multiple of [`BASE_LOOK`].
pub const LOOK_SENSITIVITY: RangeInclusive<f32> = 0.5..=3.0;
pub const DEFAULT_LOOK_SENSITIVITY: f32 = 1.5;
/// Radians per pixel of mouse movement at sensitivity 1: the spec's first value.
pub const BASE_LOOK: f32 = 0.0025;

/// The player's controls, as the world uses them.
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct Controls {
    /// Walking speed in metres per second; Shift runs at [`Controls::run_speed`].
    pub walk_speed: f32,
    /// A multiple of [`BASE_LOOK`].
    pub look_sensitivity: f32,
}

impl Default for Controls {
    fn default() -> Self {
        Controls {
            walk_speed: DEFAULT_WALK_SPEED,
            look_sensitivity: DEFAULT_LOOK_SENSITIVITY,
        }
    }
}

impl Controls {
    /// The controls the page asked for, each clamped to its slider's range. A value that
    /// is not a number keeps that control's default.
    pub fn new(walk_speed: f32, look_sensitivity: f32) -> Self {
        let within = |value: f32, range: &RangeInclusive<f32>, default: f32| {
            if value.is_finite() {
                value.clamp(*range.start(), *range.end())
            } else {
                default
            }
        };
        Controls {
            walk_speed: within(walk_speed, &WALK_SPEED, DEFAULT_WALK_SPEED),
            look_sensitivity: within(
                look_sensitivity,
                &LOOK_SENSITIVITY,
                DEFAULT_LOOK_SENSITIVITY,
            ),
        }
    }

    /// Shift's speed, in metres per second.
    pub fn run_speed(&self) -> f32 {
        self.walk_speed * RUN_PER_WALK
    }

    /// Radians the camera turns per pixel of mouse movement.
    pub fn radians_per_pixel(&self) -> f32 {
        BASE_LOOK * self.look_sensitivity
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_are_half_again_the_first_values() {
        let controls = Controls::default();
        assert!((controls.walk_speed - 1.4 * 1.5).abs() < 1e-6);
        assert!((controls.run_speed() - 3.0 * 1.5).abs() < 1e-5);
        assert!((controls.radians_per_pixel() - 0.0025 * 1.5).abs() < 1e-7);
        assert!(WALK_SPEED.contains(&controls.walk_speed));
        assert!(LOOK_SENSITIVITY.contains(&controls.look_sensitivity));
    }

    #[test]
    fn values_are_clamped_to_the_sliders_and_non_numbers_keep_the_default() {
        assert_eq!(Controls::new(0.2, 9.0), Controls::new(1.0, 3.0));
        assert_eq!(Controls::new(12.0, 0.0), Controls::new(4.0, 0.5));
        assert_eq!(Controls::new(f32::NAN, f32::INFINITY), Controls::default());
        let chosen = Controls::new(3.2, 0.8);
        assert_eq!((chosen.walk_speed, chosen.look_sensitivity), (3.2, 0.8));
        assert!((chosen.run_speed() - 3.2 * 3.0 / 1.4).abs() < 1e-5);
        assert!((chosen.radians_per_pixel() - 0.002).abs() < 1e-7);
    }
}
