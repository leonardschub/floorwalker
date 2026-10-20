//! Flag markers (management-rooms ticket 13; spec: "Flag markers in project rooms"),
//! pure and free of Bevy: which robots carry a marker, in which colour, and how it moves.
//! `flag_markers` only draws what this says.
//!
//! A marker is wordless HUD corner brackets with a small ring, locked on a flagged
//! robot's head, in its flag's colour: red, pulsing slowly, for *asking you something*;
//! amber and steady for *waiting on you*. Only root agents are flagged, and they are full
//! size, so a marker never scales with depth. No setting touches it.

use std::collections::HashMap;

use agents::FlagKind;

use super::floor_plan::{DeskId, FloorPlan};

/// One robot's marker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Marker {
    pub kind: FlagKind,
}

impl Marker {
    /// How long one slow pulse of *asking you something* takes, in seconds.
    const PULSE: f32 = 2.0;
    /// How dim the pulse gets, of full brightness: dim, never gone.
    const PULSE_LOW: f32 = 0.15;

    /// Its colour, linear RGB at full brightness: red for *asking you something*, amber
    /// for *waiting on you*. Deeper than the accepted CCTV look's (`prototype/cctv-look`)
    /// so that they still read red and amber, not pink and cream, through the world's
    /// tone mapping (AgX).
    pub fn colour(self) -> [f32; 3] {
        match self.kind {
            FlagKind::AskingYouSomething => [1.0, 0.02, 0.03],
            FlagKind::WaitingOnYou => [1.0, 0.4, 0.0],
        }
    }

    /// Its brightness, of full, `secs` seconds into the world: a slow, smooth pulse
    /// for *asking you something*, steady for *waiting on you*. Every marker of a kind
    /// pulses together.
    pub fn brightness(self, secs: f32) -> f32 {
        match self.kind {
            FlagKind::WaitingOnYou => 1.0,
            FlagKind::AskingYouSomething => {
                let phase = (secs / Self::PULSE).fract();
                let swing = 0.5 + 0.5 * (std::f32::consts::TAU * phase).cos();
                Self::PULSE_LOW + (1.0 - Self::PULSE_LOW) * swing
            }
        }
    }
}

/// The markers a plan asks for: one on the robot of each flagged desk, keyed by its
/// desk. The CCTV state machine takes some away again
/// ([`CctvState::markers`](super::cctv_state::CctvState::markers)): a killed
/// robot carries none until its wreck clears.
pub fn of_plan(plan: &FloorPlan) -> HashMap<DeskId, Marker> {
    plan.rooms
        .iter()
        .flat_map(|room| &room.desks)
        .filter_map(|desk| {
            let flag = desk.flag?;
            Some((desk.id.clone(), Marker { kind: flag.kind }))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::floor_plan::{layout, Desk, Room};
    use agents::Flag;

    fn desk(agent: &str, depth: usize, flag: Option<FlagKind>) -> Desk {
        Desk {
            id: DeskId {
                session: format!("s-{agent}"),
                agent: agent.into(),
            },
            row: 0,
            column: 1,
            depth,
            state: agents::AgentState::Busy,
            model: None,
            model_id: None,
            flag: flag.map(|kind| Flag { kind, since: 1_000 }),
            short_id: None,
        }
    }

    fn room(slot: usize, desks: Vec<Desk>) -> Room {
        Room {
            project: format!("-p{slot}"),
            name: format!("p{slot}"),
            slot,
            rows: layout::MIN_ROWS,
            desks,
        }
    }

    fn id(agent: &str) -> DeskId {
        DeskId {
            session: format!("s-{agent}"),
            agent: agent.into(),
        }
    }

    #[test]
    fn every_flagged_desk_in_every_room_carries_a_marker_of_its_flags_kind_and_no_other_does() {
        let plan = FloorPlan {
            rooms: vec![
                room(
                    0,
                    vec![
                        desk("asking", 0, Some(FlagKind::AskingYouSomething)),
                        desk("busy", 0, None),
                    ],
                ),
                room(1, vec![desk("waiting", 0, Some(FlagKind::WaitingOnYou))]),
                room(2, vec![desk("idle-sub", 1, None)]),
            ],
        };
        let markers = of_plan(&plan);
        assert_eq!(
            markers,
            HashMap::from([
                (
                    id("asking"),
                    Marker {
                        kind: FlagKind::AskingYouSomething
                    }
                ),
                (
                    id("waiting"),
                    Marker {
                        kind: FlagKind::WaitingOnYou
                    }
                ),
            ])
        );
        assert!(of_plan(&FloorPlan::default()).is_empty());
    }

    const ASKING: Marker = Marker {
        kind: FlagKind::AskingYouSomething,
    };
    const WAITING: Marker = Marker {
        kind: FlagKind::WaitingOnYou,
    };

    /// The marker's brightness every 50 ms over `secs` seconds.
    fn samples(marker: Marker, secs: f32) -> Vec<f32> {
        (0..(secs * 20.0) as usize)
            .map(|n| marker.brightness(n as f32 * 0.05))
            .collect()
    }

    #[test]
    fn waiting_on_you_is_steady_at_full_brightness() {
        assert!(samples(WAITING, 10.0).iter().all(|&b| b == 1.0));
        assert_eq!(WAITING.brightness(1e6), 1.0);
    }

    #[test]
    fn asking_you_something_pulses_slowly_and_never_fades_out() {
        let seen = samples(ASKING, 10.0);
        let (low, high) = seen
            .iter()
            .fold((f32::MAX, f32::MIN), |(lo, hi), &b| (lo.min(b), hi.max(b)));
        // It swings from full brightness down to a dim glow, deep enough to see through
        // the tone mapping, and still there.
        assert!(high > 0.99, "peaks at {high}");
        assert!((0.1..=0.25).contains(&low), "dips to {low}");
        // Slowly: one pulse every 1.5 to 3 seconds, so 3 to 7 peaks in 10 s, and it
        // changes smoothly (no step of more than a tenth of its swing per 50 ms frame).
        let peaks = seen
            .windows(3)
            .filter(|w| w[1] > w[0] && w[1] >= w[2])
            .count();
        assert!((3..=7).contains(&peaks), "{peaks} peaks in 10 s");
        assert!(
            seen.windows(2)
                .all(|w| (w[1] - w[0]).abs() <= (high - low) * 0.1),
            "it jumps"
        );
        // A long-running world keeps pulsing.
        let later: Vec<_> = (0..60)
            .map(|n| ASKING.brightness(86_400.0 + n as f32 * 0.05))
            .collect();
        assert!(later.iter().cloned().fold(f32::MIN, f32::max) > 0.95);
        assert!(later.iter().cloned().fold(f32::MAX, f32::min) < 0.55);
    }

    #[test]
    fn asking_is_red_and_waiting_is_amber() {
        let [r, g, b] = ASKING.colour();
        assert!(r > 0.9 && g < 0.15 && b < 0.15, "asking {:?}", [r, g, b]);
        let [r, g, b] = WAITING.colour();
        assert!(
            r > 0.9 && (0.3..0.8).contains(&(g / r)) && b < 0.15,
            "waiting {:?}",
            [r, g, b]
        );
    }
}
