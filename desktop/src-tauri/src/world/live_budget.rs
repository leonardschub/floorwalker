//! Which PCs are live (spec: "Which PCs are live"), pure and free of Bevy: the sessions
//! whose terminals the app watches now. Every other attachable PC shows its last frame,
//! frozen (`pc_screens`).

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use super::floor_plan::{layout, FloorPlan};

/// How many PCs near the player are live, besides the seated session's: with it, at
/// most four sessions are watched at once (spec user story 11).
pub const NEAREST: usize = 3;

/// How much nearer, in metres, a PC must be than a live one to take its place, so
/// walking doesn't churn attaches (each takes seconds to start).
pub const HYSTERESIS: f32 = 1.0;

/// What the live set is chosen from.
#[derive(Clone, Copy, Debug)]
pub struct Wants<'a> {
    pub plan: &'a FloorPlan,
    /// Whether the world is on screen: shown on the world screen and drawing (not kept
    /// warm, not suspended).
    pub shown: bool,
    /// The room the player stands in, by its slot; none in the corridor or the CCTV
    /// room.
    pub room: Option<usize>,
    /// The player's eye, in world space.
    pub eye: [f32; 3],
    /// The session the player is seated at, by its id: always live.
    pub seated: Option<&'a str>,
}

/// The live set for `wants`, by short id, given the `current` one. The seated
/// session's, plus the short ids of up to [`NEAREST`] other attachable root desks in the
/// player's room, nearest their PC's glass first. A live desk stays live until another
/// is [`HYSTERESIS`] nearer. Nothing while the world is not on screen.
pub fn live_set(wants: &Wants, current: &BTreeSet<String>) -> BTreeSet<String> {
    if !wants.shown {
        return BTreeSet::new();
    }
    let seated = wants.seated.and_then(|session| {
        let mut desks = wants.plan.rooms.iter().flat_map(|room| &room.desks);
        desks
            .find(|desk| desk.id.session == session && desk.short_id.is_some())?
            .short_id
            .as_deref()
    });
    let mut live: BTreeSet<String> = seated.into_iter().map(str::to_owned).collect();
    let Some(room) = wants
        .room
        .and_then(|slot| wants.plan.rooms.iter().find(|room| room.slot == slot))
    else {
        return live;
    };
    // Each candidate by how near it counts as: a live one a metre nearer than it is.
    let mut near: Vec<(f32, &str)> = room
        .desks
        .iter()
        .filter_map(|desk| {
            let short_id = desk.short_id.as_deref().filter(|id| Some(*id) != seated)?;
            let glass = layout::pc_glass(room.slot, desk.row, desk.column);
            let held = if current.contains(short_id) {
                HYSTERESIS
            } else {
                0.0
            };
            Some((distance(wants.eye, glass) - held, short_id))
        })
        .collect();
    near.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(b.1)));
    live.extend(
        near.into_iter()
            .take(NEAREST)
            .map(|(_, short_id)| short_id.to_owned()),
    );
    live
}

/// When the live set is chosen afresh, while walking, at the most.
pub const RECHOOSE_EVERY: Duration = Duration::from_secs(1);

/// The live set as it stands, chosen anew at most every [`RECHOOSE_EVERY`] as the player
/// walks, and at once when the world leaves or comes back on screen or the seat
/// changes.
#[derive(Debug, Default)]
pub struct LiveBudget {
    live: BTreeSet<String>,
    /// When it was last chosen, and from what screen and seat.
    chosen: Option<(Instant, bool, Option<String>)>,
}

impl LiveBudget {
    pub fn live(&self) -> &BTreeSet<String> {
        &self.live
    }

    /// Chooses the live set for `wants` at `now` if it is time, and returns it if it
    /// changed.
    pub fn update(&mut self, now: Instant, wants: &Wants) -> Option<&BTreeSet<String>> {
        let due = self.chosen.as_ref().is_none_or(|(at, shown, seated)| {
            now >= *at + RECHOOSE_EVERY
                || *shown != wants.shown
                || seated.as_deref() != wants.seated
        });
        if !due {
            return None;
        }
        self.chosen = Some((now, wants.shown, wants.seated.map(str::to_owned)));
        let live = live_set(wants, &self.live);
        if live == self.live {
            return None;
        }
        self.live = live;
        Some(&self.live)
    }
}

fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    let [x, y, z] = [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    (x * x + y * y + z * z).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::floor_plan::{Desk, DeskId, Room};
    use agents::AgentState;

    /// A root desk at `(row, column)` whose session is `id`, attachable by `id` too
    /// unless it runs in a terminal.
    fn desk(id: &str, row: usize, column: usize, attachable: bool) -> Desk {
        Desk {
            id: DeskId {
                session: id.into(),
                agent: id.into(),
            },
            row,
            column,
            depth: 0,
            model: None,
            model_id: None,
            flag: None,
            state: AgentState::Busy,
            short_id: attachable.then(|| id.to_string()),
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

    /// Room 0's first row: `a` to `d` from the left, each attachable.
    fn first_row() -> FloorPlan {
        FloorPlan {
            rooms: vec![room(
                0,
                vec![
                    desk("a", 0, 0, true),
                    desk("b", 0, 1, true),
                    desk("c", 0, 2, true),
                    desk("d", 0, 3, true),
                ],
            )],
        }
    }

    /// An eye level with room 0's first row of PCs, `x` along it: the PCs stand at x
    /// -3.3, -1.1, 1.1 and 3.3, so each is as far as its x is from `x`.
    fn along_first_row(x: f32) -> [f32; 3] {
        let [_, y, z] = layout::pc_glass(0, 0, 0);
        [x, y, z]
    }

    fn standing<'a>(plan: &'a FloorPlan, room: Option<usize>, eye: [f32; 3]) -> Wants<'a> {
        Wants {
            plan,
            shown: true,
            room,
            eye,
            seated: None,
        }
    }

    fn set(ids: &[&str]) -> BTreeSet<String> {
        ids.iter().map(|id| id.to_string()).collect()
    }

    #[test]
    fn the_three_pcs_nearest_the_player_in_their_room_are_live() {
        let plan = first_row();
        // From b's PC: a and c 2.2 m away, d 4.4 m.
        let wants = standing(&plan, Some(0), along_first_row(-1.1));
        assert_eq!(live_set(&wants, &BTreeSet::new()), set(&["a", "b", "c"]));
        // From c's PC: b and d 2.2 m away, a 4.4 m.
        let wants = standing(&plan, Some(0), along_first_row(1.1));
        assert_eq!(live_set(&wants, &BTreeSet::new()), set(&["b", "c", "d"]));
    }

    #[test]
    fn only_attachable_root_pcs_in_the_players_own_room_count() {
        let mut plan = first_row();
        // b runs in a terminal; room 1's desks stand elsewhere, however near.
        plan.rooms[0].desks[1] = desk("b", 0, 1, false);
        plan.rooms.push(room(1, vec![desk("e", 0, 1, true)]));
        let wants = standing(&plan, Some(0), along_first_row(-1.1));
        assert_eq!(live_set(&wants, &BTreeSet::new()), set(&["a", "c", "d"]));
        let [_, y, z] = layout::pc_glass(1, 0, 1);
        let in_room_1 = standing(&plan, Some(1), [layout::pc_glass(1, 0, 1)[0], y, z]);
        assert_eq!(live_set(&in_room_1, &BTreeSet::new()), set(&["e"]));
    }

    #[test]
    fn nothing_is_live_in_the_corridor_or_the_cctv_room() {
        let plan = first_row();
        // Both are no room; the eye is where the corridor would stand.
        let wants = standing(&plan, None, along_first_row(-1.1));
        assert_eq!(live_set(&wants, &set(&["a", "b", "c"])), BTreeSet::new());
    }

    #[test]
    fn a_live_pc_stays_live_until_another_is_a_metre_nearer() {
        let plan = first_row();
        let live = set(&["a", "b", "c"]);
        // At x 0.4, d (2.9 m) is 0.8 m nearer than a (3.7 m): a stays.
        let wants = standing(&plan, Some(0), along_first_row(0.4));
        assert_eq!(live_set(&wants, &live), live);
        // Choosing afresh there, d is in.
        assert_eq!(live_set(&wants, &BTreeSet::new()), set(&["b", "c", "d"]));
        // At x 0.6, d (2.7 m) is 1.2 m nearer than a (3.9 m): d takes a's place.
        let wants = standing(&plan, Some(0), along_first_row(0.6));
        assert_eq!(live_set(&wants, &live), set(&["b", "c", "d"]));
    }

    #[test]
    fn nothing_is_live_while_the_world_is_not_on_screen() {
        let plan = first_row();
        let mut wants = standing(&plan, Some(0), along_first_row(-1.1));
        wants.shown = false;
        wants.seated = Some("b");
        assert_eq!(live_set(&wants, &set(&["a", "b", "c"])), BTreeSet::new());
    }

    #[test]
    fn the_seated_session_is_live_besides_the_three_nearest_anywhere() {
        let plan = first_row();
        // Seated at d, 4.4 m from b's PC: b's three nearest and d.
        let mut wants = standing(&plan, Some(0), along_first_row(-1.1));
        wants.seated = Some("d");
        assert_eq!(
            live_set(&wants, &BTreeSet::new()),
            set(&["a", "b", "c", "d"])
        );
        // Seated at b: b and the three others.
        wants.seated = Some("b");
        assert_eq!(
            live_set(&wants, &BTreeSet::new()),
            set(&["a", "b", "c", "d"])
        );
        // Out in the corridor (as ticket 12's seat never is), the seat alone.
        wants.room = None;
        assert_eq!(live_set(&wants, &BTreeSet::new()), set(&["b"]));
    }

    #[test]
    fn walking_changes_the_live_set_at_most_once_a_second() {
        let plan = first_row();
        let mut budget = LiveBudget::default();
        let start = Instant::now();
        let at_b = standing(&plan, Some(0), along_first_row(-1.1));
        let at_d = standing(&plan, Some(0), along_first_row(3.3));
        assert_eq!(budget.update(start, &at_b), Some(&set(&["a", "b", "c"])));
        // At d's PC half a second later: not yet.
        let soon = start + Duration::from_millis(500);
        assert_eq!(budget.update(soon, &at_d), None);
        assert_eq!(budget.live(), &set(&["a", "b", "c"]));
        let later = start + Duration::from_secs(1);
        assert_eq!(budget.update(later, &at_d), Some(&set(&["b", "c", "d"])));
        // Standing still, nothing changes.
        assert_eq!(budget.update(later + Duration::from_secs(1), &at_d), None);
    }

    #[test]
    fn leaving_the_screen_coming_back_and_sitting_in_change_it_at_once() {
        let plan = first_row();
        let mut budget = LiveBudget::default();
        let start = Instant::now();
        let ms = |n| start + Duration::from_millis(n);
        let mut wants = standing(&plan, Some(0), along_first_row(-1.1));
        budget.update(start, &wants);
        wants.shown = false;
        assert_eq!(budget.update(ms(100), &wants), Some(&BTreeSet::new()));
        wants.shown = true;
        assert_eq!(budget.update(ms(200), &wants), Some(&set(&["a", "b", "c"])));
        wants.seated = Some("d");
        assert_eq!(
            budget.update(ms(300), &wants),
            Some(&set(&["a", "b", "c", "d"]))
        );
    }
}
