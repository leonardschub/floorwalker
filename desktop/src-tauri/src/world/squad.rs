//! The kill squad's moves (management-rooms ticket 20; spec: "Punishments in motion"),
//! pure and free of Bevy beside the CCTV state machine, which says each kill's phase and
//! how far through it: here is where each of the squad's three robots is at that moment,
//! which way it faces and how it holds itself, for `punishments` to pose them.
//!
//! **The way in** ([`route`]) is worked out from the room's layout and the desk's seat
//! alone, in the room's frame (`[x, z]`, the doorway wall's inner face at z = 0, the room
//! towards -Z): from just inside the doorway, straight in through the empty entry seat
//! ([`layout::ENTRY_SEAT`]) to the aisle behind the first row, along it to the aisle
//! between the first two columns, down that to the aisle behind the target's row and
//! along it to each robot's post. Rows and columns of desk sets never stand in an
//! aisle, so the squad never walks through one, wherever its seats are taken.
//! **The posts:** two stand in the aisle behind the robot and point at it; the third
//! crouches in the aisle at its left (+X, away from where go to desk puts the player)
//! and plants the charge on its chest.
//!
//! **Over time** ([`squad`]): during breach and advance each runs its way in, one after
//! another; during surround the two raise their arms to point and the third crouches,
//! reaches in and plants the charge ([`charge_planted`]); during fall back each runs its
//! way back out and vanishes at the doorway; the blast finds nobody. The way is worked
//! out afresh for every moment, so if desks repack mid-kill the squad heads for the
//! desk where it is now.

use super::cctv_state::{Phase, KILL};
use super::floor_plan::layout;

/// The squad's robots: the first two point, the last plants the charge.
pub const SQUAD: usize = 3;
const PLANTER: usize = 2;

/// Where the squad appears and vanishes: just inside the doorway, on its interior side.
pub const DOORWAY: [f32; 2] = [layout::ROOM_DOORWAY_X, -0.3];
/// Each one's post from the desk set's origin (the seat): the pointers behind the robot,
/// the planter at its left (+X), level with its chest.
const POSTS: [[f32; 2]; SQUAD] = [[-0.5, f32::NAN], [0.25, f32::NAN], [0.8, 0.1]];
/// One stride of the running gait (both legs), in metres.
const STRIDE: f32 = 1.6;
/// How much later each one starts to run, in and back out, in milliseconds.
const IN_STAGGER: f32 = 200.0;
const OUT_STAGGER: f32 = 150.0;

/// How a robot of the squad holds itself.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Pose {
    /// Running, `stride` strides along its way (its legs and arms swing with it).
    Run { stride: f32 },
    /// Standing at its post with its arm raised to point at the robot, `raise` from 0
    /// (hanging) to 1 (level).
    Point { raise: f32 },
    /// Crouched at its post, its arm `reach` from 0 (in) to 1 (on the robot's chest).
    Crouch { reach: f32 },
}

/// One robot of the squad: where it stands in the room (`[x, z]`), its turn about the
/// vertical (zero faces +Z, towards the doorway, like the seated robots; positive turns
/// towards +X), and its pose.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Trooper {
    /// Which of the squad it is (0 to [`SQUAD`] - 1); the last plants the charge.
    pub robot: usize,
    pub at: [f32; 2],
    pub yaw: f32,
    pub pose: Pose,
}

/// The aisle behind row `row` of a room `rows` deep: midway between its chairs' backs and
/// the next row's desk sets, or the back wall.
fn lane(rows: usize, row: usize) -> f32 {
    let behind = layout::desk_position(row, 0)[1] + layout::DESK_SET_Z[0];
    let beyond = if row + 1 < rows {
        layout::desk_position(row + 1, 0)[1] + layout::DESK_SET_Z[1]
    } else {
        -layout::interior_depth(rows)
    };
    (behind + beyond) / 2.0
}

/// The aisle between the first two columns, which runs the room's whole depth.
fn cross_aisle() -> f32 {
    (layout::desk_position(0, 0)[0] + layout::desk_position(0, 1)[0]) / 2.0
}

/// Each robot's way in, as points in the room's frame from [`DOORWAY`] to its post
/// beside the desk at `row` and `column` of a room `rows` deep (see the module's notes).
pub fn route(rows: usize, row: usize, column: usize) -> [Vec<[f32; 2]>; SQUAD] {
    let [x, z] = layout::desk_position(row, column);
    let behind = lane(rows, row);
    let mut common = vec![DOORWAY, [DOORWAY[0], lane(rows, 0)]];
    if row > 0 {
        common.push([cross_aisle(), lane(rows, 0)]);
        common.push([cross_aisle(), behind]);
    }
    std::array::from_fn(|n| {
        let mut way = common.clone();
        let [dx, dz] = POSTS[n];
        way.push([x + dx, behind]);
        if !dz.is_nan() {
            way.push([x + dx, z + dz]);
        }
        way
    })
}

/// A way's length.
fn length(way: &[[f32; 2]]) -> f32 {
    way.windows(2).map(|leg| distance(leg[0], leg[1])).sum()
}

fn distance(a: [f32; 2], b: [f32; 2]) -> f32 {
    (b[0] - a[0]).hypot(b[1] - a[1])
}

/// The turn that faces from `from` towards `to`.
fn facing(from: [f32; 2], to: [f32; 2]) -> f32 {
    (to[0] - from[0]).atan2(to[1] - from[1])
}

/// The point `along` metres along `way` (clamped to its ends), and the turn of the leg
/// it is on, facing onwards.
fn at(way: &[[f32; 2]], along: f32) -> ([f32; 2], f32) {
    let mut left = along.max(0.0);
    for leg in way.windows(2) {
        let span = distance(leg[0], leg[1]);
        if left <= span && span > 0.0 {
            let t = left / span;
            let point = [
                leg[0][0] + (leg[1][0] - leg[0][0]) * t,
                leg[0][1] + (leg[1][1] - leg[0][1]) * t,
            ];
            return (point, facing(leg[0], leg[1]));
        }
        left -= span;
    }
    let n = way.len();
    (way[n - 1], facing(way[n - 2], way[n - 1]))
}

/// How long `phase` of a kill lasts, and how long the kill had run when it began, in
/// milliseconds.
fn span(phase: Phase) -> (f32, f32) {
    let mut start = 0;
    for &(p, lasts) in &KILL {
        if p == phase {
            return (lasts as f32, start as f32);
        }
        start += lasts;
    }
    (0.0, start as f32)
}

/// How far the `n`th robot is along its way (0 to 1) `t` milliseconds into a run of
/// `lasts`, starting `stagger` later than the one before it.
fn run(t: f32, lasts: f32, n: usize, stagger: f32) -> Option<f32> {
    let start = stagger * n as f32;
    let reach = lasts - stagger * (SQUAD - 1) as f32;
    (t >= start).then(|| ((t - start) / reach).min(1.0))
}

/// The squad at a kill's `phase` and `progress` through it (0 to 1), on the desk at `row`
/// and `column` of a room `rows` deep: each robot that is in the room now. None outside
/// the squad's phases, and none once each has run back out (see the module's notes).
pub fn squad(rows: usize, row: usize, column: usize, phase: Phase, progress: f32) -> Vec<Trooper> {
    let ways = route(rows, row, column);
    let robot = layout::desk_position(row, column);
    let (lasts, began) = span(phase);
    let t = began + progress.clamp(0.0, 1.0) * lasts;
    let running = |n: usize, s: f32, back: bool| {
        let way = &ways[n];
        let along = length(way) * if back { 1.0 - s } else { s };
        let (at, yaw) = at(way, along);
        Trooper {
            robot: n,
            at,
            yaw: if back {
                yaw + std::f32::consts::PI
            } else {
                yaw
            },
            pose: Pose::Run {
                stride: along / STRIDE,
            },
        }
    };
    match phase {
        Phase::Breach | Phase::Advance => {
            let (advance, _) = span(Phase::Advance);
            let (breach, _) = span(Phase::Breach);
            (0..SQUAD)
                .filter_map(|n| {
                    let s = run(t, breach + advance, n, IN_STAGGER)?;
                    Some(running(n, s, false))
                })
                .collect()
        }
        Phase::Surround => (0..SQUAD)
            .map(|n| {
                let post = *ways[n].last().expect("a way has an end");
                let pose = if n == PLANTER {
                    // In over the first half, held, then back.
                    let reach = (progress * 3.0).min(1.0).min((1.0 - progress) * 5.0);
                    Pose::Crouch {
                        reach: reach.max(0.0),
                    }
                } else {
                    Pose::Point {
                        raise: (progress * 4.0).min(1.0),
                    }
                };
                Trooper {
                    robot: n,
                    at: post,
                    yaw: facing(post, robot),
                    pose,
                }
            })
            .collect(),
        Phase::FallBack => (0..SQUAD)
            .filter_map(|n| {
                // In turn, the planter, crouched by the robot, last.
                let s = run(t - began, lasts, n, OUT_STAGGER).unwrap_or(0.0);
                (s < 1.0).then(|| running(n, s, true))
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Whether the charge is on the robot's chest: once planted, half way through surround,
/// until it blows.
pub fn charge_planted(phase: Phase, progress: f32) -> bool {
    match phase {
        Phase::Surround => progress >= 0.5,
        Phase::FallBack => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every seat a desk can take in a room `rows` deep, as (row, column).
    fn seats(rows: usize) -> Vec<(usize, usize)> {
        (0..rows * layout::DESKS_PER_ROW - 1)
            .map(layout::seat)
            .collect()
    }

    /// Points every 2 cm along `way`.
    fn walk(way: &[[f32; 2]]) -> Vec<[f32; 2]> {
        let steps = (length(way) / 0.02).ceil() as usize;
        (0..=steps).map(|k| at(way, k as f32 * 0.02).0).collect()
    }

    /// Whether `p` stands clear of every desk set of a full room `rows` deep, by a
    /// squad robot's half width, and on its floor, inside its walls.
    fn in_the_aisles(rows: usize, p: [f32; 2]) -> bool {
        const BODY: f32 = 0.15;
        let clear = seats(rows).into_iter().all(|(row, column)| {
            let ([x0, x1], [z0, z1]) = layout::desk_box(row, column);
            !(p[0] > x0 - BODY && p[0] < x1 + BODY && p[1] > z0 - BODY && p[1] < z1 + BODY)
        });
        let half = 4.75 - BODY;
        clear && p[0].abs() < half && p[1] < 0.0 && p[1] > -layout::interior_depth(rows) + BODY
    }

    #[test]
    fn the_squad_appears_inside_the_rooms_doorway() {
        // In the doorway's opening (x -4.10..-2.90), just inside its wall (z < 0).
        let [x, z] = std::hint::black_box(DOORWAY);
        assert!((x - layout::ROOM_DOORWAY_X).abs() < layout::DOORWAY_WIDTH / 2.0);
        assert!(z < 0.0 && z > -0.5);
    }

    #[test]
    fn the_squads_way_runs_from_the_doorway_along_the_aisles_to_beside_the_desk() {
        for rows in 2..=4 {
            for (row, column) in seats(rows) {
                let [x, z] = layout::desk_position(row, column);
                for (n, way) in route(rows, row, column).iter().enumerate() {
                    let at = format!("rows {rows}, seat {row},{column}, robot {n}");
                    // From just inside the doorway …
                    assert_eq!(way[0], DOORWAY, "{at}");
                    // … through the aisles only …
                    for p in walk(way) {
                        assert!(in_the_aisles(rows, p), "{at}: {p:?} is not in an aisle");
                    }
                    // … to beside the desk: within a metre of the robot.
                    let end = *way.last().unwrap();
                    let off = distance(end, [x, z]);
                    assert!(off < 1.0, "{at}: ends {off} m from the robot");
                }
                // The planter crouches right beside the robot, at its left (+X).
                let planter = *route(rows, row, column)[PLANTER].last().unwrap();
                assert!(planter[0] > x && (planter[1] - z).abs() < 0.2);
            }
        }
    }

    #[test]
    fn a_way_follows_the_desk_wherever_it_is() {
        // The same room, the desk moved a row back and across: a new way, to it.
        let before = route(3, 0, 2);
        let after = route(3, 2, 0);
        assert_ne!(before, after);
        let [x, z] = layout::desk_position(2, 0);
        for way in &after {
            assert!(distance(*way.last().unwrap(), [x, z]) < 1.0);
        }
        // A deeper room moves the aisle behind its last row back with its wall.
        let shallow = route(2, 1, 1)[0].last().copied().unwrap();
        let deep = route(3, 1, 1)[0].last().copied().unwrap();
        assert!(deep[1] < shallow[1]);
    }

    /// The squad `ms` milliseconds into a kill of the desk at seat (1, 2) of a 3-row room.
    fn at_ms(ms: i64) -> Vec<Trooper> {
        let mut start = 0;
        for &(phase, lasts) in &KILL {
            if ms < start + lasts {
                let progress = (ms - start) as f32 / lasts as f32;
                return squad(3, 1, 2, phase, progress);
            }
            start += lasts;
        }
        Vec::new()
    }

    fn running(trooper: &Trooper) -> bool {
        matches!(trooper.pose, Pose::Run { .. })
    }

    #[test]
    fn the_squad_runs_in_surrounds_the_robot_runs_back_out_and_is_gone_for_the_blast() {
        let robot = layout::desk_position(1, 2);
        let posts: Vec<[f32; 2]> = route(3, 1, 2)
            .iter()
            .map(|way| *way.last().unwrap())
            .collect();
        // It appears in the doorway, one after another, running in.
        let first = at_ms(0);
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].at, DOORWAY);
        assert!(running(&first[0]));
        assert_eq!(at_ms(500).len(), SQUAD);
        // On its way, everyone is in an aisle and running, legs swinging with the way.
        for ms in (0..3_000).step_by(100) {
            for trooper in at_ms(ms) {
                assert!(running(&trooper), "at {ms}");
                assert!(in_the_aisles(3, trooper.at), "at {ms}: {:?}", trooper.at);
            }
        }
        // By the end of the advance each is at its post.
        let arrived = at_ms(2_999);
        for (trooper, post) in arrived.iter().zip(&posts) {
            assert!(
                distance(trooper.at, *post) < 0.05,
                "{:?} {post:?}",
                trooper.at
            );
        }
        // Surround: two point, one crouches; all face the robot.
        let surround = at_ms(3_700);
        for trooper in &surround {
            let to = facing(trooper.at, robot);
            assert!((trooper.yaw - to).abs() < 1e-4);
        }
        assert!(matches!(surround[0].pose, Pose::Point { raise } if raise == 1.0));
        assert!(matches!(surround[1].pose, Pose::Point { raise } if raise == 1.0));
        assert!(matches!(surround[2].pose, Pose::Crouch { reach } if reach == 1.0));
        // The charge is planted half way through, and stays until it blows.
        assert!(!charge_planted(Phase::Surround, 0.4));
        assert!(charge_planted(Phase::Surround, 0.5));
        assert!(charge_planted(Phase::FallBack, 1.0));
        assert!(!charge_planted(Phase::Boom, 0.0));
        // Fall back: running out, the planter last, back through the aisles.
        let leaving = at_ms(4_500);
        assert_eq!(leaving.len(), SQUAD);
        assert!(leaving.iter().all(running));
        for ms in (4_400..6_200).step_by(100) {
            for trooper in at_ms(ms) {
                assert!(in_the_aisles(3, trooper.at), "at {ms}: {:?}", trooper.at);
            }
        }
        // Each vanishes at the doorway; nobody is left for the blast.
        let last = at_ms(6_150);
        assert!(last.len() < SQUAD);
        for trooper in &last {
            assert!(distance(trooper.at, DOORWAY) < 1.5);
        }
        assert!(at_ms(6_200).is_empty());
        assert!(at_ms(7_000).is_empty());
        // Work harder's phases have no squad.
        assert!(squad(3, 1, 2, Phase::Strike, 0.5).is_empty());
    }
}
