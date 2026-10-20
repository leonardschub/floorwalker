//! The world's clock: the wall time in milliseconds since the epoch, like a flag's
//! `since`, read once at the start of every update whether or not anything is drawn.
//! The CCTV state machine, clicks, the console's and focus monitors' timers all read this
//! one value, so everything in one frame agrees on the time.

use std::time::{SystemTime, UNIX_EPOCH};

use bevy::prelude::*;

/// This update's time, in milliseconds since the epoch.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WorldClock(pub i64);

pub struct ClockPlugin;

impl Plugin for ClockPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<WorldClock>()
            .add_systems(First, read_clock);
    }
}

fn read_clock(mut clock: ResMut<WorldClock>) {
    clock.0 = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as i64);
}
