//! Sitting in (agent-screens spec: "Right-click and picking", "The seated view"), pure
//! and free of Bevy's world: what a right-click aims at, whether its agent can be sat
//! in or which hint it gets, where the seat's camera goes so the PC's glass fills the
//! view squarely, where the glass then is on screen for the overlay, and where the
//! player goes on leaving. `seat` does it in the world.

use bevy::camera::{CameraProjection, PerspectiveProjection};
use bevy::math::{EulerRot, Mat4, Quat, Vec2, Vec3};

use super::collision::{self, Space};
use super::floor_plan::{layout, DeskId, FloorPlan};
use super::picking::{self, Boxed, Ray};

/// How far a right-click reaches: a robot or its desk set further away is not sat in,
/// and gets no hint (spec user stories 12 and 13).
pub const REACH: f32 = 4.0;

/// How much of the world view's width the glass fills from the seat (ticket 03's
/// framing), and the most of its height it may fill in a very wide view.
pub const FILL: f32 = 0.72;
pub const MOST_HEIGHT: f32 = 0.9;

/// Why an agent can't be sat in, as the hint under the centre dot says it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Hint {
    /// A root agent whose session runs in an ordinary terminal: "Runs in a terminal.
    /// Type /bg there to sit in."
    Terminal,
    /// A subagent: "Nothing to sit in."
    Nothing,
}

/// What sitting in an agent comes to.
#[derive(Clone, Debug, PartialEq)]
pub enum SeatOrHint {
    /// An attachable root agent: its seat.
    Seat(Seat),
    Hint(Hint),
}

/// The seat at an attachable root agent's desk.
#[derive(Clone, Debug, PartialEq)]
pub struct Seat {
    pub desk: DeskId,
    /// Its session's short id, for `claude attach`.
    pub short_id: String,
    pub place: Place,
}

/// Where a desk is: the room it is in, by slot, and its seat `(row, column)` there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Place {
    pub slot: usize,
    pub row: usize,
    pub column: usize,
}

/// Sitting in agent `id` (a right-click on its robot, its row in the side panel, SIT IN
/// on the console): its seat when it is an attachable root agent, the *terminal* hint
/// for a terminal-started root agent, the *nothing* hint for a subagent; none when it
/// has no desk.
pub fn sit_in(plan: &FloorPlan, id: &DeskId) -> Option<SeatOrHint> {
    plan.rooms.iter().find_map(|room| {
        let desk = room.desks.iter().find(|desk| &desk.id == id)?;
        Some(match (desk.depth, &desk.short_id) {
            (0, Some(short_id)) => SeatOrHint::Seat(Seat {
                desk: desk.id.clone(),
                short_id: short_id.clone(),
                place: Place {
                    slot: room.slot,
                    row: desk.row,
                    column: desk.column,
                },
            }),
            (0, None) => SeatOrHint::Hint(Hint::Terminal),
            _ => SeatOrHint::Hint(Hint::Nothing),
        })
    })
}

/// The desk whose robot or desk set (desk, chair, PC, keyboard: one box over its
/// footprint, [`layout::DESK_SET_HEIGHT`] high) `ray` enters first within [`REACH`] in
/// the room in `slot`.
pub fn aimed_at(plan: &FloorPlan, slot: usize, ray: &Ray) -> Option<DeskId> {
    let room = plan.rooms.iter().find(|room| room.slot == slot)?;
    let [ox, oy, oz] = layout::interior_origin(slot);
    let boxes = room.desks.iter().map(|desk| {
        let (x, z) = layout::desk_box(desk.row, desk.column);
        let [y0, y1] = layout::DESK_SET_HEIGHT;
        let centre = Vec3::new(
            ox + (x[0] + x[1]) / 2.0,
            oy + (y0 + y1) / 2.0,
            oz + (z[0] + z[1]) / 2.0,
        );
        let half = [(x[1] - x[0]) / 2.0, (y1 - y0) / 2.0, (z[1] - z[0]) / 2.0];
        let boxed = Boxed {
            centre,
            axes: [Vec3::X, Vec3::Y, Vec3::Z],
            half,
        };
        (desk.id.clone(), boxed)
    });
    picking::nearest_within(ray, boxes, REACH)
}

/// Where the seat's camera is: the room, the eye, and its turn as `look::Look` holds
/// it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SeatView {
    pub slot: usize,
    pub eye: [f32; 3],
    pub yaw: f32,
    pub pitch: f32,
}

/// The glass of the PC at `place`: its middle, the way it faces (towards the robot's
/// eyes, unit length), and its right and up edges' halves as the robot sees it.
fn glass(place: Place) -> (Vec3, Vec3, Vec3, Vec3) {
    let middle = Vec3::from_array(layout::pc_glass(place.slot, place.row, place.column));
    let (sin, cos) = layout::PC_GLASS_TILT.sin_cos();
    // The desk set faces the doorway (+Z): its robot looks along +Z at the glass, which
    // faces back towards -Z and up at the eyes. The robot's right is -X.
    let facing = Vec3::new(0.0, sin, -cos);
    let [width, height] = layout::PC_GLASS_SIZE;
    let right = Vec3::new(-width / 2.0, 0.0, 0.0);
    let up = Vec3::new(0.0, cos, sin) * (height / 2.0);
    (middle, facing, right, up)
}

/// The corners of that glass in world space.
fn glass_corners(place: Place) -> [Vec3; 4] {
    let (middle, _, right, up) = glass(place);
    [
        middle - right + up,
        middle + right + up,
        middle + right - up,
        middle - right - up,
    ]
}

/// The seat at `place`, for a world view `aspect` wide
/// (width over height) seen with vertical field of view `fov`: the eye on the line
/// through the glass's middle square to it, near the robot's eyes, just far enough that
/// the glass fills [`FILL`] of the view's width (and no more than [`MOST_HEIGHT`] of
/// its height), looking straight at it.
pub fn seat_view(place: Place, aspect: f32, fov: f32) -> SeatView {
    let (middle, facing, right, up) = glass(place);
    let tan = (fov / 2.0).tan();
    let across = right.length() / (FILL * tan * aspect);
    let high = up.length() / (MOST_HEIGHT * tan);
    let eye = middle + facing * across.max(high);
    let to = middle - eye;
    SeatView {
        slot: place.slot,
        eye: eye.to_array(),
        yaw: (-to.x).atan2(-to.z),
        pitch: to.y.atan2(to.x.hypot(to.z)),
    }
}

/// Where the glass of the seat at `place` lies in a world view of `size` pixels
/// seen from `view` with vertical field of view `fov` and near plane `near`: its
/// rectangle as `[x, y, width, height]` from the view's top-left, if it is on screen.
pub fn glass_rect(
    view: &SeatView,
    place: Place,
    fov: f32,
    near: f32,
    size: [f32; 2],
) -> Option<[f32; 4]> {
    let size = Vec2::from_array(size);
    let clip_from_view = PerspectiveProjection {
        fov,
        near,
        aspect_ratio: size.x / size.y,
        ..Default::default()
    }
    .get_clip_from_view();
    let rotation = Quat::from_euler(EulerRot::YXZ, view.yaw, view.pitch, 0.0);
    let world_from_view = Mat4::from_rotation_translation(rotation, Vec3::from_array(view.eye));
    let clip_from_world = clip_from_view * world_from_view.inverse();
    let corners = glass_corners(place);
    let rect = super::portals::screen_rect(clip_from_world, corners, size)?;
    Some([rect.min.x, rect.min.y, rect.width(), rect.height()])
}

/// Where the player stood before sitting in: their space, their eye and their turn, and
/// for a room, its project (another project's room may take the slot later).
#[derive(Clone, Debug, PartialEq)]
pub struct Stood {
    pub space: Space,
    pub project: Option<String>,
    pub eye: [f32; 3],
    pub yaw: f32,
    pub pitch: f32,
}

/// Where leaving the seat puts a player who stood at `stood`: back there, facing the
/// same way; pushed to the nearest free spot if desk sets moved onto it or the floor
/// went; at the start of the corridor, facing the doorway wall, if its room has closed.
pub fn leave_to(plan: &FloorPlan, stood: &Stood) -> Stood {
    let [x, y, z] = stood.eye;
    let at = match stood.space {
        Space::Room(slot) => {
            let open = plan
                .rooms
                .iter()
                .any(|room| room.slot == slot && Some(&room.project) == stood.project.as_ref());
            if !open {
                return Stood {
                    space: Space::Corridor,
                    project: None,
                    eye: layout::start_eye(),
                    yaw: 0.0,
                    pitch: 0.0,
                };
            }
            collision::clear_of_desks(plan, slot, [x, z])
        }
        space => collision::free_spot(plan, space, [x, z]),
    };
    Stood {
        eye: [at[0], y, at[1]],
        ..stood.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::super::floor_plan::{Desk, Room};
    use super::*;

    /// A desk at `(row, column)` for agent `agent` of session `session`.
    fn desk(session: &str, agent: &str, row: usize, column: usize) -> Desk {
        Desk {
            id: DeskId {
                session: session.into(),
                agent: agent.into(),
            },
            row,
            column,
            depth: usize::from(session != agent),
            model: None,
            model_id: None,
            flag: None,
            state: agents::AgentState::Busy,
            short_id: None,
        }
    }

    fn attachable(mut desk: Desk, short_id: &str) -> Desk {
        desk.short_id = Some(short_id.into());
        desk
    }

    fn room(project: &str, slot: usize, desks: Vec<Desk>) -> Room {
        Room {
            project: project.into(),
            name: project.into(),
            slot,
            rows: layout::rows(desks.len()),
            desks,
        }
    }

    fn id(session: &str, agent: &str) -> DeskId {
        DeskId {
            session: session.into(),
            agent: agent.into(),
        }
    }

    /// Room `-alpha` in slot 0: an attachable root agent at (0, 1) with a subagent at
    /// (0, 2), and a terminal-started root agent at (1, 0).
    fn alpha() -> FloorPlan {
        FloorPlan {
            rooms: vec![room(
                "-alpha",
                0,
                vec![
                    attachable(desk("s1", "s1", 0, 1), "aaaa1111"),
                    desk("s1", "sub", 0, 2),
                    desk("s2", "s2", 1, 0),
                ],
            )],
        }
    }

    #[test]
    fn an_attachable_root_agent_is_sat_in_and_others_get_their_hint() {
        let plan = alpha();
        assert_eq!(
            sit_in(&plan, &id("s1", "s1")),
            Some(SeatOrHint::Seat(Seat {
                desk: id("s1", "s1"),
                short_id: "aaaa1111".into(),
                place: SEAT_0_1,
            }))
        );
        assert_eq!(
            sit_in(&plan, &id("s2", "s2")),
            Some(SeatOrHint::Hint(Hint::Terminal))
        );
        assert_eq!(
            sit_in(&plan, &id("s1", "sub")),
            Some(SeatOrHint::Hint(Hint::Nothing))
        );
        // No desk, nothing at all.
        assert_eq!(sit_in(&plan, &id("gone", "gone")), None);
    }

    /// From the eye at `eye` (room frame of slot 0), the ray straight at the desk set at
    /// `(row, column)`'s robot seat at chest height.
    fn ray_at(eye: [f32; 2], row: usize, column: usize) -> Ray {
        let [ox, _, oz] = layout::interior_origin(0);
        let [x, z] = layout::desk_position(row, column);
        let origin = Vec3::new(ox + eye[0], layout::EYE_HEIGHT, oz + eye[1]);
        let target = Vec3::new(ox + x, 0.9, oz + z + 0.3);
        Ray {
            origin,
            direction: target - origin,
        }
    }

    /// `plan`'s room with only the desks at `seats`.
    fn room_of(plan: &FloorPlan, seats: &[(usize, usize)]) -> Room {
        let mut room = plan.rooms[0].clone();
        room.desks
            .retain(|desk| seats.contains(&(desk.row, desk.column)));
        room
    }

    #[test]
    fn a_right_click_picks_the_nearest_desk_set_within_4_m_only() {
        let plan = alpha();
        let [x, z] = layout::desk_position(0, 1);
        // From the aisle in front of desk (0, 1), towards the doorway side, 2 m away.
        let close = ray_at([x, z + 2.5], 0, 1);
        assert_eq!(aimed_at(&plan, 0, &close), Some(id("s1", "s1")));
        // The same line from 6 m: beyond reach, nothing.
        let far = ray_at([x, z + 6.0], 0, 1);
        assert_eq!(aimed_at(&plan, 0, &far), None);
        // Aiming at the subagent's desk (0, 2) picks it, for its hint.
        let [x2, z2] = layout::desk_position(0, 2);
        let sub = ray_at([x2, z2 + 2.0], 0, 2);
        assert_eq!(aimed_at(&plan, 0, &sub), Some(id("s1", "sub")));
        // Through desk (0, 1) to the terminal desk (1, 0) behind it: the nearer wins.
        let [bx, bz] = layout::desk_position(1, 0);
        let room = Vec3::from(layout::interior_origin(0));
        let origin = Vec3::new(x, layout::EYE_HEIGHT, z + 0.9) + room;
        let through = Ray {
            origin,
            direction: Vec3::new(bx, 0.0, bz) + room - origin,
        };
        assert_eq!(
            aimed_at(
                &FloorPlan {
                    rooms: vec![room_of(&plan, &[(1, 0)])]
                },
                0,
                &through
            ),
            Some(id("s2", "s2")),
            "the ray reaches the terminal desk behind"
        );
        assert_eq!(aimed_at(&plan, 0, &through), Some(id("s1", "s1")));
        // Another room's slot has nothing to pick.
        assert_eq!(aimed_at(&plan, 3, &close), None);
        // Over every desk set's top: nothing.
        let above = Ray {
            origin: close.origin,
            direction: Vec3::new(0.0, 0.3, -1.0),
        };
        assert_eq!(aimed_at(&plan, 0, &above), None);
    }

    /// The seat at (0, 1) in slot 0.
    const SEAT_0_1: Place = Place {
        slot: 0,
        row: 0,
        column: 1,
    };

    /// The glass's rectangle in a view of `w` × `h` from the seat at (0, 1) in slot 0.
    fn seated_rect(w: f32, h: f32) -> (SeatView, [f32; 4]) {
        let fov = std::f32::consts::FRAC_PI_4;
        let view = seat_view(SEAT_0_1, w / h, fov);
        let rect = glass_rect(&view, SEAT_0_1, fov, collision::NEAR, [w, h]).expect("on screen");
        (view, rect)
    }

    #[test]
    fn the_seat_looks_squarely_at_the_glass_and_fills_72_percent_of_the_width() {
        for (w, h) in [
            (1280.0, 836.0),
            (1600.0, 900.0),
            (900.0, 900.0),
            (1024.0, 600.0),
        ] {
            let (view, [x, y, rw, rh]) = seated_rect(w, h);
            // 72 % of the width, in the middle of the view.
            assert!((rw / w - FILL).abs() < 0.005, "{w}x{h}: {}", rw / w);
            assert!((x + rw / 2.0 - w / 2.0).abs() < 1.0, "{w}x{h}: x {x}");
            assert!((y + rh / 2.0 - h / 2.0).abs() < 1.0, "{w}x{h}: y {y}");
            // Square on: the glass keeps its 5:3, with no trapezoid's stretch.
            let [gw, gh] = layout::PC_GLASS_SIZE;
            assert!(((rw / rh) / (gw / gh) - 1.0).abs() < 0.002, "{w}x{h}");
            // The eye is about where the robot's are, half a metre from the glass (the
            // face plate is 0.54 m away); a square view, narrower, sits further back.
            let glass = Vec3::from(layout::pc_glass(0, 0, 1));
            let distance = Vec3::from(view.eye).distance(glass);
            let most = if w / h < 1.2 { 0.85 } else { 0.6 };
            assert!((0.4..most).contains(&distance), "{w}x{h}: {distance}");
        }
        // A very wide view: the glass would overflow its height, so it fills less width.
        let (_, [_, _, rw, rh]) = seated_rect(2400.0, 700.0);
        assert!(rh / 700.0 <= MOST_HEIGHT + 0.005, "{}", rh / 700.0);
        assert!(rw / 2400.0 < FILL);
    }

    #[test]
    fn the_seat_faces_the_glass_as_the_robot_does() {
        let (view, _) = seated_rect(1280.0, 836.0);
        let forward = Quat::from_euler(EulerRot::YXZ, view.yaw, view.pitch, 0.0) * Vec3::NEG_Z;
        // Towards the doorway (+Z), looking down the glass's tilt.
        assert!(forward.z > 0.98, "{forward}");
        assert!(
            (forward.y + layout::PC_GLASS_TILT.sin()).abs() < 1e-4,
            "{forward}"
        );
        assert!(
            (view.eye[1] - 1.10).abs() < 0.03,
            "near the robot's face height"
        );
    }

    fn stood(space: Space, project: Option<&str>, eye: [f32; 3]) -> Stood {
        Stood {
            space,
            project: project.map(str::to_owned),
            eye,
            yaw: 0.7,
            pitch: -0.2,
        }
    }

    #[test]
    fn leaving_puts_the_player_back_where_they_stood_facing_the_same_way() {
        let plan = alpha();
        let [ox, _, oz] = layout::interior_origin(0);
        // In the aisle beside desk (0, 1).
        let [x, z] = layout::desk_position(0, 1);
        let there = stood(
            Space::Room(0),
            Some("-alpha"),
            [ox + x - 1.0, 1.6, oz + z + 0.4],
        );
        assert_eq!(leave_to(&plan, &there), there);
        // In the corridor, and in the CCTV room's doorway side.
        let corridor = stood(Space::Corridor, None, [2.0, 1.6, 1.2]);
        assert_eq!(leave_to(&plan, &corridor), corridor);
    }

    #[test]
    fn leaving_onto_a_desk_set_that_moved_there_pushes_the_player_to_the_nearest_free_spot() {
        let plan = alpha();
        let [ox, _, oz] = layout::interior_origin(0);
        // Where a desk set now stands, near its left side (-X): the terminal desk (1, 0)
        // came here while they sat.
        let [x, z] = layout::desk_position(1, 0);
        let on = stood(
            Space::Room(0),
            Some("-alpha"),
            [ox + x - 0.5, 1.6, oz + z + 0.4],
        );
        let left = leave_to(&plan, &on);
        let set_left = ox + x + layout::DESK_SET_X[0];
        assert!(
            (left.eye[0] - (set_left - layout::PLAYER_RADIUS)).abs() < 1e-3,
            "out across the nearest side, a radius clear: {:?}",
            left.eye
        );
        assert_eq!(left.eye[2], on.eye[2]);
        assert_eq!(
            (left.yaw, left.pitch, left.space),
            (on.yaw, on.pitch, on.space)
        );
    }

    #[test]
    fn leaving_after_the_room_closed_puts_the_player_at_the_start_of_the_corridor() {
        let there = stood(Space::Room(0), Some("-alpha"), [1.0, 1.6, -101.0]);
        let start = Stood {
            space: Space::Corridor,
            project: None,
            eye: layout::start_eye(),
            yaw: 0.0,
            pitch: 0.0,
        };
        // No room at all, and another project's room in the slot.
        assert_eq!(leave_to(&FloorPlan::default(), &there), start);
        let other = FloorPlan {
            rooms: vec![room("-beta", 0, Vec::new())],
        };
        assert_eq!(leave_to(&other, &there), start);
    }
}
