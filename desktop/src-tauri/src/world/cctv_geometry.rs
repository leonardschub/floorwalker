//! Where the CCTV room's screens hang and where a focus monitor's camera stands, pure
//! and free of Bevy: the monitor wall's curve ([`screen`]), the focus monitors and the
//! ticker above and below it ([`focus_screen`], [`ticker_screen`]), the room's width for
//! a wider wall ([`room_half_width`]), the close-up camera in front of a robot's face
//! ([`focus_camera`]) and the wider one while a punishment plays on it
//! ([`punishment_camera`]). `cctv_state` decides what goes where; `feeds`, `focus` and `cctv`
//! place it.

use super::floor_plan::layout;

/// One screen's place on the monitor wall, in the CCTV room's frame
/// (`layout::cctv_to_world`): its monitor's back centre, and its turn about the vertical
/// (zero faces the console, +Z).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Screen {
    pub centre: [f32; 3],
    pub yaw: f32,
}

/// One column of the wall's width (`cctv_room_kit.py`'s, which its extensions add).
pub const COLUMN_WIDTH: f32 = 4.0 / 3.0;

/// Half the CCTV room's inside width with `extra_columns` past the narrow wall's four:
/// each widens the room by one column, half on each side (a `cctv_extension` strip
/// each), so it stays centred. Depth and height never change.
pub fn room_half_width(extra_columns: usize) -> f32 {
    layout::CCTV_HALF_WIDTH + extra_columns as f32 * COLUMN_WIDTH / 2.0
}
/// The top row's height, and the rows' spacing down from it (`review_renders.py`).
pub const TOP_ROW: f32 = 2.72;
const ROW_SPACING: f32 = 0.8;
/// How much more each column beyond the flat middle turns toward the console, as long as
/// the wall's ends stay behind the console's back (up to eight columns, 24 rooms): a
/// wider wall curves less, so it never comes out past the console into the floor.
const CURVE: f32 = 0.3;

/// The turn per column beyond the flat middle for a wall `columns` wide: [`CURVE`], or
/// less once its ends would come forward past the console's back.
fn curve(columns: usize) -> f32 {
    // Columns beyond the flat middle (two wide when even, one when odd) on each side.
    let outer = columns.saturating_sub(1) / 2;
    let reach = |turn: f32| {
        (1..=outer)
            .map(|k| (k as f32 * turn).sin() * COLUMN_WIDTH)
            .sum::<f32>()
    };
    let room = layout::CCTV_DEPTH + layout::CONSOLE_Z - layout::CONSOLE_HALF[1];
    if reach(CURVE) <= room {
        return CURVE;
    }
    let (mut low, mut high) = (0.0, CURVE);
    for _ in 0..32 {
        let mid = (low + high) / 2.0;
        if reach(mid) <= room {
            low = mid;
        } else {
            high = mid;
        }
    }
    low
}

/// Where the screen at `column` and `row` of a wall `columns` wide hangs: the middle
/// columns flat on the back wall, each further one out coming forward and turning in by
/// [`CURVE`] more, edge to edge, so the wall curves round the console.
pub fn screen(columns: usize, column: usize, row: usize) -> Screen {
    let middle = (columns as f32 - 1.0) / 2.0;
    let offset = column as f32 - middle;
    let y = TOP_ROW - ROW_SPACING * row as f32;
    let z = -layout::CCTV_DEPTH;
    if offset.abs() < 1.0 {
        return Screen {
            centre: [offset * COLUMN_WIDTH, y, z],
            yaw: 0.0,
        };
    }
    // Out from the flat middle's edge, along the right half; the left mirrors it.
    let flat = if columns.is_multiple_of(2) { 1.0 } else { 0.5 };
    let (mut x, mut z, mut turn) = (flat * COLUMN_WIDTH, z, 0.0_f32);
    let mut centre = [x, z];
    let curve = curve(columns);
    for _ in 0..(offset.abs() - flat + 0.5).round() as usize {
        turn += curve;
        let (along, forward) = (turn.cos(), turn.sin());
        centre = [
            x + along * COLUMN_WIDTH / 2.0,
            z + forward * COLUMN_WIDTH / 2.0,
        ];
        x += along * COLUMN_WIDTH;
        z += forward * COLUMN_WIDTH;
    }
    let side = offset.signum();
    Screen {
        centre: [side * centre[0], y, centre[1]],
        yaw: -side * turn,
    }
}

/// A feed monitor's bezel height, and a focus monitor's (`cctv_monitors.py`).
const FEED_BEZEL_HEIGHT: f32 = 0.711;
const FOCUS_BEZEL_HEIGHT: f32 = 0.915;
/// The focus monitors' centres either side of the middle, and the gap from the top row's
/// bezel up to theirs; the gap from the lowest row's bezel down to the ticker's centre
/// (`review_renders.py`'s accepted layout).
const FOCUS_X: f32 = 0.94;
const FOCUS_GAP: f32 = 0.08;
const TICKER_GAP: f32 = 0.17;

/// Where focus monitor `n` (0 the left, 1 the right) hangs: flat on the back wall, above
/// the top row and centred on the room, however wide the wall grows.
pub fn focus_screen(n: usize) -> Screen {
    let side = if n == 0 { -1.0 } else { 1.0 };
    let y = TOP_ROW + FEED_BEZEL_HEIGHT / 2.0 + FOCUS_GAP + FOCUS_BEZEL_HEIGHT / 2.0;
    Screen {
        centre: [side * FOCUS_X, y, -layout::CCTV_DEPTH],
        yaw: 0.0,
    }
}

/// Where the ticker hangs under a wall `rows` high (one for NO SIGNAL): flat on the
/// back wall, centred, just under the lowest row.
pub fn ticker_screen(rows: usize) -> Screen {
    let lowest = TOP_ROW - ROW_SPACING * (rows.max(1) - 1) as f32;
    Screen {
        centre: [
            0.0,
            lowest - FEED_BEZEL_HEIGHT / 2.0 - TICKER_GAP,
            -layout::CCTV_DEPTH,
        ],
        yaw: 0.0,
    }
}

/// A focus monitor's close-up camera, in its room's frame: where it stands, its turn
/// (yaw about the vertical, zero looking towards -Z, then pitch, up positive) and its
/// vertical field of view, for a 16:9 picture.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FocusCamera {
    pub eye: [f32; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub fov: f32,
}

/// A full-size robot's face from its desk set's origin (`layout::DESK_AIM`'s note), and
/// where a focus camera stands from the face: in front of it and up over its PC, whose
/// back is towards the doorway; the face's frame is this high.
const FACE: [f32; 3] = [0.0, 1.10, 0.07];
const FOCUS_FROM_FACE: [f32; 3] = [0.0, 0.36, 0.84];
const FOCUS_FRAME: f32 = 0.5;

/// The close-up camera on the robot at the desk in `row` and `column`: up over the PC,
/// looking down at the face, which fills the middle of the picture.
pub fn focus_camera(row: usize, column: usize) -> FocusCamera {
    let [x, z] = layout::desk_position(row, column);
    let face = [x + FACE[0], FACE[1], z + FACE[2]];
    let eye = [
        face[0] + FOCUS_FROM_FACE[0],
        face[1] + FOCUS_FROM_FACE[1],
        face[2] + FOCUS_FROM_FACE[2],
    ];
    let [dx, dy, dz] = [face[0] - eye[0], face[1] - eye[1], face[2] - eye[2]];
    let distance = (dx * dx + dy * dy + dz * dz).sqrt();
    FocusCamera {
        eye,
        yaw: (-dx).atan2(-dz),
        pitch: dy.atan2(dx.hypot(dz)),
        fov: 2.0 * (FOCUS_FRAME / 2.0 / distance).atan(),
    }
}

/// While a punishment plays on its agent, a focus camera stands back from the face over
/// the row in front and up towards the ceiling (`[up, back]`), so the whole show is in
/// its picture.
const WIDE_FROM_FACE: [f32; 2] = [1.4, 2.0];
/// Never nearer the doorway than this, for a desk in the first row (the doorway wall's
/// inner face is at z = 0).
const WIDE_NEAREST_WALL: f32 = -0.2;
/// What the wide shot keeps in its picture, from the desk set's origin (`[x, y, z]`): the
/// robot's face; the ceiling over the robot, where the arcs crackle and the bolt starts
/// (`punishments`' ceiling at 2.96 m); the aisle behind it, where two of the kill squad
/// point (`squad`'s posts, from their knees to their heads); and beside it, where the
/// third crouches and plants the charge.
const WIDE_SUBJECT: [[f32; 3]; 8] = [
    [0.0, 1.10, 0.07],
    [-0.5, 3.0, 0.0],
    [0.5, 3.0, 0.0],
    [-0.7, 0.5, -0.9],
    [0.45, 0.5, -0.9],
    [-0.7, 1.6, -0.9],
    [0.45, 1.6, -0.9],
    [1.0, 0.3, 0.1],
];
/// A focus monitor's picture is 16:9.
const FOCUS_ASPECT: f32 = 16.0 / 9.0;
/// Room left round the subject, as a share of the field of view.
const WIDE_MARGIN: f32 = 1.1;

/// The focus camera on the robot at the desk in `row` and `column` while a punishment
/// plays on it: back and up from the close-up, looking down the desk's own column, its
/// view centred on and just wide enough for the crackle on the ceiling, the bolt, the
/// kill squad round the desk and the blast.
pub fn punishment_camera(row: usize, column: usize) -> FocusCamera {
    let [x, z] = layout::desk_position(row, column);
    let eye = [
        x + FACE[0],
        FACE[1] + WIDE_FROM_FACE[0],
        (z + FACE[2] + WIDE_FROM_FACE[1]).min(WIDE_NEAREST_WALL),
    ];
    let subject = WIDE_SUBJECT.map(|p| [x + p[0], p[1], z + p[2]]);
    // Looking straight down the column (-Z), tilted to the middle of the subject's
    // heights as the eye sees them.
    let level = FocusCamera {
        eye,
        yaw: 0.0,
        pitch: 0.0,
        fov: 0.0,
    };
    let (low, high) = subject
        .iter()
        .map(|&p| {
            let [_, up, ahead] = level.view_of(p);
            up.atan2(ahead)
        })
        .fold((f32::MAX, f32::MIN), |(low, high), a| {
            (low.min(a), high.max(a))
        });
    let mut camera = FocusCamera {
        pitch: (low + high) / 2.0,
        ..level
    };
    // Just wide enough for every point of it, upright or across, with a margin.
    let half = subject
        .iter()
        .map(|&p| {
            let [right, up, ahead] = camera.view_of(p);
            up.abs()
                .atan2(ahead)
                .max((right.abs() / FOCUS_ASPECT).atan2(ahead))
        })
        .fold(0.0_f32, f32::max);
    camera.fov = 2.0 * half * WIDE_MARGIN;
    camera
}

impl FocusCamera {
    /// A point in the room's frame as this camera sees it: how far right, up and ahead
    /// of the eye along its view.
    fn view_of(&self, point: [f32; 3]) -> [f32; 3] {
        let d = [
            point[0] - self.eye[0],
            point[1] - self.eye[1],
            point[2] - self.eye[2],
        ];
        // Undo the yaw (about +Y), then the pitch (about +X); ahead is -Z.
        let (sy, cy) = self.yaw.sin_cos();
        let x = d[0] * cy - d[2] * sy;
        let z = d[0] * sy + d[2] * cy;
        let (sp, cp) = self.pitch.sin_cos();
        let y = d[1] * cp + z * sp;
        let z = -d[1] * sp + z * cp;
        [x, y, -z]
    }

    /// Whether `point` is in this camera's 16:9 picture.
    #[cfg(test)]
    pub fn sees(&self, point: [f32; 3]) -> bool {
        let [right, up, ahead] = self.view_of(point);
        let half = (self.fov / 2.0).tan();
        ahead > 0.0 && up.abs() <= half * ahead && right.abs() <= half * FOCUS_ASPECT * ahead
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn while_a_punishment_plays_the_focus_camera_shows_the_desk_the_ceiling_over_it_and_the_squads_posts(
    ) {
        for (row, column) in [(0, 1), (0, 3), (3, 3), (7, 1), (10, 1)] {
            let [x, z] = layout::desk_position(row, column);
            let at = |p: [f32; 3]| [x + p[0], p[1], z + p[2]];
            let wide = punishment_camera(row, column);
            let close = focus_camera(row, column);
            // The robot's face; the ceiling over it, where the arcs crackle and the bolt
            // starts; the kill squad's two in the aisle behind it, knees and heads, and
            // the third crouched beside it.
            for p in [
                [0.0, 1.10, 0.07],
                [0.0, 2.96, 0.0],
                [-0.5, 0.5, -0.8],
                [0.25, 1.5, -0.8],
                [0.8, 0.3, 0.1],
            ] {
                assert!(
                    wide.sees(at(p)),
                    "row {row} column {column}: {p:?} {wide:?}"
                );
            }
            // The close-up shows the face, but not the ceiling or the one planting the
            // charge.
            assert!(close.sees(at([0.0, 1.10, 0.07])));
            assert!(!close.sees(at([0.0, 2.96, 0.0])));
            assert!(!close.sees(at([0.8, 0.3, 0.1])));
            // It stands inside the room, over the heads of the row in front, under the
            // ceiling, and is not so wide the robot is lost.
            let [ex, ey, ez] = wide.eye;
            assert!(
                ez < -0.1 && ex.abs() < 4.5 && (1.9..2.9).contains(&ey),
                "{wide:?}"
            );
            assert!(wide.fov < 1.6, "{wide:?}");
        }
    }

    #[test]
    fn the_wall_curves_its_outer_columns_toward_the_console_edge_to_edge_from_the_top_row_down() {
        let at = |column, row| screen(4, column, row);
        // The middle two hang flat on the back wall, either side of the middle.
        for (column, x) in [(1, -2.0 / 3.0), (2, 2.0 / 3.0)] {
            let s = at(column, 0);
            assert!((s.centre[0] - x).abs() < 1e-4, "{s:?}");
            assert!((s.centre[2] + layout::CCTV_DEPTH).abs() < 1e-4, "{s:?}");
            assert_eq!(s.yaw, 0.0);
        }
        // The outer two come forward and turn in, mirrored.
        let (left, right) = (at(0, 0), at(3, 0));
        assert!(right.centre[2] > -layout::CCTV_DEPTH + 0.05, "{right:?}");
        assert!(
            (0.15..0.5).contains(&left.yaw) && right.yaw == -left.yaw,
            "{left:?} {right:?}"
        );
        assert!((left.centre[0] + right.centre[0]).abs() < 1e-4);
        assert_eq!(left.centre[2], right.centre[2]);
        // Edge to edge: the outer screen's inner edge is the inner screen's outer edge,
        // and the wall stays clear of the side walls.
        let edge = |s: Screen, side: f32| {
            let half = COLUMN_WIDTH / 2.0 * side;
            [
                s.centre[0] + half * s.yaw.cos(),
                s.centre[2] - half * s.yaw.sin(),
            ]
        };
        let (a, b) = (edge(at(2, 0), 1.0), edge(right, -1.0));
        assert!(
            (a[0] - b[0]).abs() < 1e-3 && (a[1] - b[1]).abs() < 1e-3,
            "{a:?} {b:?}"
        );
        assert!(edge(right, 1.0)[0] < layout::CCTV_HALF_WIDTH - 0.5);
        // Rows from the top one down, 0.8 m apart.
        for row in 0..3 {
            for column in 0..4 {
                let s = at(column, row);
                assert!((s.centre[1] - (2.72 - 0.8 * row as f32)).abs() < 1e-4);
                assert_eq!((s.centre[0], s.centre[2], s.yaw), {
                    let top = at(column, 0);
                    (top.centre[0], top.centre[2], top.yaw)
                });
            }
        }
    }

    #[test]
    fn focus_monitors_hang_centred_above_the_wall_and_the_ticker_under_its_lowest_row() {
        // `review_renders.py`'s accepted layout: focus centres at (±0.94, 3.61), the
        // ticker 0.17 m under the lowest row's bezel.
        let (left, right) = (focus_screen(0), focus_screen(1));
        assert!((left.centre[0] + 0.94).abs() < 1e-3 && (right.centre[0] - 0.94).abs() < 1e-3);
        for s in [left, right] {
            assert!((s.centre[1] - 3.613).abs() < 1e-3, "{s:?}");
            assert_eq!((s.centre[2], s.yaw), (-layout::CCTV_DEPTH, 0.0));
        }
        for (rows, y) in [(3, 0.5945), (2, 1.3945), (1, 2.1945), (0, 2.1945)] {
            let t = ticker_screen(rows);
            assert!((t.centre[1] - y).abs() < 1e-3, "{rows}: {t:?}");
            assert_eq!(
                (t.centre[0], t.centre[2], t.yaw),
                (0.0, -layout::CCTV_DEPTH, 0.0)
            );
        }
    }

    #[test]
    fn a_focus_camera_looks_over_the_pc_straight_at_the_robots_face() {
        for (row, column) in [(0, 1), (0, 3), (2, 0), (5, 2)] {
            let [dx, dz] = layout::desk_position(row, column);
            let cam = focus_camera(row, column);
            // A full-size robot's face (`layout::DESK_AIM`'s note) in the middle of the
            // picture, a close-up: under a metre away, the frame about half a metre high.
            let face = [dx, 1.10, dz + 0.07];
            let to = |p: [f32; 3]| [p[0] - cam.eye[0], p[1] - cam.eye[1], p[2] - cam.eye[2]];
            let [x, y, z] = to(face);
            let distance = (x * x + y * y + z * z).sqrt();
            assert!((0.6..1.0).contains(&distance), "{distance}");
            assert!((cam.yaw - (-x).atan2(-z)).abs() < 1e-4, "{cam:?}");
            assert!((cam.pitch - y.atan2(x.hypot(z))).abs() < 1e-4, "{cam:?}");
            let tall = 2.0 * distance * (cam.fov / 2.0).tan();
            assert!((0.4..0.7).contains(&tall), "{tall}");
            // The PC (`desk_set.py`'s screen B: 0.54 wide with its tape, its top at
            // 1.20, from 0.55 to 0.73 in front of the seat) stays below the line to the
            // face.
            for k in 0..=20 {
                let t = k as f32 / 20.0;
                let p = [cam.eye[0] + x * t, cam.eye[1] + y * t, cam.eye[2] + z * t];
                let in_pc =
                    (p[0] - dx).abs() < 0.28 && p[1] < 1.21 && (0.55..0.74).contains(&(p[2] - dz));
                assert!(!in_pc, "{row} {column}: {p:?}");
            }
        }
    }

    #[test]
    fn a_wider_wall_curves_its_outer_columns_edge_to_edge_inside_the_room_and_behind_the_console() {
        // A screen's edge on the floor plan (x, z), on `side` (+1 right, -1 left).
        let edge = |s: Screen, side: f32| {
            let half = COLUMN_WIDTH / 2.0 * side;
            [
                s.centre[0] + half * s.yaw.cos(),
                s.centre[2] - half * s.yaw.sin(),
            ]
        };
        let console_back = layout::CONSOLE_Z - layout::CONSOLE_HALF[1];
        for columns in 4..=24 {
            let half_width = room_half_width(columns - 4);
            for column in 0..columns {
                let s = screen(columns, column, 0);
                let mirror = screen(columns, columns - 1 - column, 0);
                assert!(
                    (s.centre[0] + mirror.centre[0]).abs() < 1e-4
                        && (s.centre[2] - mirror.centre[2]).abs() < 1e-4
                        && (s.yaw + mirror.yaw).abs() < 1e-5,
                    "{columns} columns: {s:?} mirrors {mirror:?}"
                );
                // Each column further out turns further in, never past a quarter turn.
                if column + 1 < columns && 2 * column + 1 >= columns {
                    let next = screen(columns, column + 1, 0);
                    assert!(next.yaw <= s.yaw && next.yaw > -1.2, "{columns}: {next:?}");
                    // Edge to edge with its outer neighbour.
                    let (a, b) = (edge(s, 1.0), edge(next, -1.0));
                    assert!(
                        (a[0] - b[0]).abs() < 1e-3 && (a[1] - b[1]).abs() < 1e-3,
                        "{columns} columns, column {column}: {a:?} {b:?}"
                    );
                }
                // Clear of the side walls and behind the console's back.
                for side in [-1.0, 1.0] {
                    let [x, z] = edge(s, side);
                    assert!(x.abs() < half_width - 0.5, "{columns}: {x} in {half_width}");
                    assert!(z <= console_back + 1e-3, "{columns} columns: {z}");
                }
            }
        }
        // Up to 24 rooms (eight columns) each column past the flat middle turns 0.3 rad
        // more, the curve of the wall of four.
        for (columns, column, yaw) in [(5, 3, -0.3), (5, 4, -0.6), (6, 5, -0.6), (8, 7, -0.9)] {
            assert!(
                (screen(columns, column, 0).yaw - yaw).abs() < 1e-5,
                "{columns}"
            );
        }
        // Wider still, the curve flattens to stay behind the console.
        assert!(screen(16, 15, 0).yaw > -0.9);
    }
}
