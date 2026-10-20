//! The seated view in the world (agent-screens spec: "The seated view", "Right-click
//! and picking"): sitting in an attachable root agent snaps the camera into its robot's
//! eyes, square to its PC's glass ([`seating::seat_view`]), hides the robot, its flag
//! marker and any wreck from the player's own camera, and reports the seat with the
//! glass's rectangle for the page's terminal overlay. Leaving snaps back to where the
//! player stood ([`seating::leave_to`]).
//!
//! - A **right-click** while captured aims through the centre dot: the desk set within
//!   4 m it hits sits the player in its agent, or reports its hint
//!   ([`seating::sit_in`]). While seated the cursor is free, and the page turns any
//!   right-click on the world view into a leave.
//! - **Sit in by session** (`ToWorld::SitIn`, for the side panel and New session) goes
//!   to the session's root desk from anywhere and sits in it. Seat to seat keeps the
//!   original "where they stood".
//! - **The seat follows its desk** when desks repack, and closes by itself when the desk
//!   goes or stops being attachable (its session ended, went dormant). The world screen
//!   going away (Back) leaves too.
//! - **Hidden from the player's camera only:** the robot, marker and wreck move to the
//!   room's seat layer ([`layout::seat_layer`]), which every camera that sees the room
//!   sees except the main one. Punishment rigs stay on the room's layer, so a show
//!   plays around the overlay; nothing is cancelled.
//!
//! What the page and the shell hear goes out through [`SeatOutbox`], drained by the
//! frame loop into the controller's reports.

use bevy::app::{Propagate, PropagateSet};
use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;

use super::collision::{self, Space};
use super::controller::{FromWorld, SeatRect, SeatReport};
use super::corridor::Plan;
use super::engine::WorldRect;
use super::floor_plan::{layout, DeskId};
use super::look::Look;
use super::pc_screens::SeatedSession;
use super::picking::Ray;
use super::portals::PortalCamera;
use super::rooms::{InRoom, Seated};
use super::seating::{self, SeatOrHint, SeatView, Stood};
use super::walk::PlayerEye;

/// What sitting in hides from the player's own camera: on a desk's robot, its flag
/// marker and a wreck laid at it, each a direct child of the desk entity.
#[derive(Component)]
pub struct HiddenWhenSeated;

/// The seat the player holds, if any.
#[derive(Resource, Default)]
pub struct PlayerSeat(Option<Sitting>);

impl PlayerSeat {
    pub fn seated(&self) -> bool {
        self.0.is_some()
    }
}

struct Sitting {
    seat: seating::Seat,
    /// Where the player stood before the first seat of this sitting.
    stood: Stood,
    /// The pose last put on the camera, and the world view size it was worked out for.
    view: Option<(SeatView, [u32; 4])>,
    /// What was last reported.
    reported: Option<SeatReport>,
    /// What is hidden now.
    hidden: Vec<Entity>,
}

/// Seat changes and hints for the shell, drained each frame.
#[derive(Resource, Default)]
pub struct SeatOutbox(pub Vec<FromWorld>);

pub struct SeatPlugin;

impl Plugin for SeatPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PlayerSeat>()
            .init_resource::<SeatOutbox>()
            // After the markers and wrecks of this frame are placed, before the room's
            // layer reaches them.
            .add_systems(
                PostUpdate,
                hide_seated
                    .after(super::flag_markers::PlaceMarkers)
                    .after(super::wrecks::LayWrecks)
                    .before(PropagateSet::<RenderLayers>::default()),
            );
    }
}

/// A right-click while captured: sits in the robot or desk set the centre dot is on
/// within reach, or reports its hint. In the corridor and the CCTV room, and further
/// than 4 m, nothing happens. While seated it leaves (the page sends a leave itself).
pub fn right_click(world: &mut World) {
    if world.resource::<PlayerSeat>().seated() {
        leave(world, "right-click");
        return;
    }
    let Space::Room(slot) = *world.resource::<Space>() else {
        eprintln!("[world] right-click outside a project room: nothing");
        return;
    };
    let look = *world.resource::<Look>();
    let Some(eye) = eye(world) else { return };
    let ray = Ray {
        origin: eye,
        direction: Quat::from_euler(EulerRot::YXZ, look.yaw, look.pitch, 0.0) * Vec3::NEG_Z,
    };
    let Some(id) = seating::aimed_at(&world.resource::<Plan>().0, slot, &ray) else {
        eprintln!("[world] right-click on nothing within reach");
        return;
    };
    sit_in(world, &id);
}

/// Sits in agent `id` from wherever the player is: its seat if it is an attachable root
/// agent, else its hint; nothing without a desk.
pub fn sit_in(world: &mut World, id: &DeskId) {
    match seating::sit_in(&world.resource::<Plan>().0, id) {
        None => eprintln!(
            "[world] sit in: {} / {} has no desk, nothing changes",
            id.session, id.agent
        ),
        Some(SeatOrHint::Hint(hint)) => {
            eprintln!(
                "[world] sit in: {} / {} can't be sat in, hint {hint:?}",
                id.session, id.agent
            );
            world
                .resource_mut::<SeatOutbox>()
                .0
                .push(FromWorld::Hint(hint));
        }
        Some(SeatOrHint::Seat(seat)) => take(world, seat),
    }
}

/// Takes `seat`, keeping where the player stood before a seat they already hold.
fn take(world: &mut World, seat: seating::Seat) {
    let held = world.resource_mut::<PlayerSeat>().0.take();
    let stood = match held {
        Some(mut held) => {
            unhide(world, &mut held);
            held.stood
        }
        None => {
            let space = *world.resource::<Space>();
            let look = *world.resource::<Look>();
            let project = match space {
                Space::Room(slot) => world
                    .resource::<Plan>()
                    .0
                    .rooms
                    .iter()
                    .find(|room| room.slot == slot)
                    .map(|room| room.project.clone()),
                _ => None,
            };
            Stood {
                space,
                project,
                eye: eye(world).unwrap_or(Vec3::ZERO).to_array(),
                yaw: look.yaw,
                pitch: look.pitch,
            }
        }
    };
    eprintln!(
        "[world] sat in {} ({}) in slot {} at seat ({}, {}); stood in {}",
        seat.desk.session,
        seat.short_id,
        seat.place.slot,
        seat.place.row,
        seat.place.column,
        stood.space.describe()
    );
    world.resource_mut::<SeatedSession>().0 = Some(seat.desk.session.clone());
    // A debug build's room placement no longer holds the camera.
    world.remove_resource::<InRoom>();
    world.resource_mut::<PlayerSeat>().0 = Some(Sitting {
        seat,
        stood,
        view: None,
        reported: None,
        hidden: Vec::new(),
    });
    settle(world);
}

/// Leaves the seat, if there is one, for `why` (for the log): back where the player
/// stood, or the nearest free spot, or the corridor's start, and reports no seat.
pub fn leave(world: &mut World, why: &str) {
    let Some(mut held) = world.resource_mut::<PlayerSeat>().0.take() else {
        return;
    };
    unhide(world, &mut held);
    let back = seating::leave_to(&world.resource::<Plan>().0, &held.stood);
    eprintln!(
        "[world] left the seat at {} ({why}): back in {} at ({:.2}, {:.2}, {:.2})",
        held.seat.desk.session,
        back.space.describe(),
        back.eye[0],
        back.eye[1],
        back.eye[2]
    );
    put_eye(world, Vec3::from_array(back.eye));
    *world.resource_mut::<Look>() = Look {
        yaw: back.yaw,
        pitch: back.pitch,
    };
    world.resource_mut::<Space>().set_if_neq(back.space);
    world.resource_mut::<SeatedSession>().0 = None;
    world
        .resource_mut::<SeatOutbox>()
        .0
        .push(FromWorld::Seat(None));
}

/// Every frame while seated, after the messages: the seat follows its desk through a
/// new plan, or closes when its desk is gone or no longer attachable; a new world view
/// rectangle moves the camera to keep the glass's share of the view; and the seat is
/// reported whenever its rectangle changes.
pub fn keep_seat(world: &mut World) {
    let Some(desk) = world
        .resource::<PlayerSeat>()
        .0
        .as_ref()
        .map(|held| held.seat.desk.clone())
    else {
        return;
    };
    if world.is_resource_changed::<Plan>() {
        match seating::sit_in(&world.resource::<Plan>().0, &desk) {
            Some(SeatOrHint::Seat(seat)) => {
                if let Some(held) = world.resource_mut::<PlayerSeat>().0.as_mut() {
                    if held.seat.place != seat.place {
                        eprintln!(
                            "[world] the seat at {} moved with its desk to ({}, {})",
                            seat.desk.session, seat.place.row, seat.place.column
                        );
                    }
                    held.seat = seat;
                }
            }
            Some(SeatOrHint::Hint(_)) => {
                return leave(world, "the session is no longer attachable")
            }
            None => return leave(world, "the session ended"),
        }
    }
    settle(world);
}

/// Puts the camera at the seat for the current world view, if the seat or the view
/// changed, and reports the seat when its rectangle does.
fn settle(world: &mut World) {
    let Some(rect) = world.resource::<WorldRect>().0 else {
        return;
    };
    if rect.w == 0 || rect.h == 0 {
        return;
    }
    let fov = main_fov(world);
    let Some(held) = world.resource::<PlayerSeat>().0.as_ref() else {
        return;
    };
    let seat = &held.seat;
    let size = [rect.x, rect.y, rect.w, rect.h];
    let view = seating::seat_view(seat.place, rect.w as f32 / rect.h as f32, fov);
    if held.view == Some((view, size)) {
        return;
    }
    let report = seating::glass_rect(
        &view,
        seat.place,
        fov,
        collision::NEAR,
        [rect.w as f32, rect.h as f32],
    )
    .map(|[x, y, w, h]| SeatReport {
        session_id: seat.desk.session.clone(),
        short_id: seat.short_id.clone(),
        rect: SeatRect {
            x: rect.x as f32 + x,
            y: rect.y as f32 + y,
            w,
            h,
        },
    });
    put_eye(world, Vec3::from_array(view.eye));
    *world.resource_mut::<Look>() = Look {
        yaw: view.yaw,
        pitch: view.pitch,
    };
    world
        .resource_mut::<Space>()
        .set_if_neq(Space::Room(view.slot));
    let fresh = {
        let mut seat = world.resource_mut::<PlayerSeat>();
        let Some(held) = seat.0.as_mut() else { return };
        held.view = Some((view, size));
        if report.is_none() || held.reported == report {
            return;
        }
        held.reported = report.clone();
        report
    };
    if let Some(report) = &fresh {
        eprintln!(
            "[world] seat at {}: glass at ({:.1}, {:.1}) {:.1}x{:.1}",
            report.session_id, report.rect.x, report.rect.y, report.rect.w, report.rect.h
        );
    }
    world
        .resource_mut::<SeatOutbox>()
        .0
        .push(FromWorld::Seat(fresh));
}

/// Keeps the seated desk's robot, marker and wreck on the room's seat layer, which the
/// player's camera does not see; a marker or wreck that appears while seated is hidden
/// from its first frame.
fn hide_seated(
    mut commands: Commands,
    mut seat: ResMut<PlayerSeat>,
    desks: Query<(&Seated, &Children)>,
    hideable: Query<(), With<HiddenWhenSeated>>,
) {
    let Some(held) = seat.0.as_mut() else { return };
    let Some((_, children)) = desks.iter().find(|(id, _)| id.0 == held.seat.desk) else {
        return;
    };
    let layer = RenderLayers::from_layers(&[layout::seat_layer(held.seat.place.slot)]);
    for child in children.iter() {
        if hideable.contains(child) && !held.hidden.contains(&child) {
            commands.entity(child).insert(Propagate(layer.clone()));
            held.hidden.push(child);
        }
    }
}

/// Gives what the seat hid back to its room's layer.
fn unhide(world: &mut World, held: &mut Sitting) {
    for entity in held.hidden.drain(..) {
        if let Ok(mut entity) = world.get_entity_mut(entity) {
            entity.remove::<Propagate<RenderLayers>>();
        }
    }
}

fn eye(world: &mut World) -> Option<Vec3> {
    let mut eyes = world.query_filtered::<&Transform, (With<PlayerEye>, Without<PortalCamera>)>();
    eyes.iter(world).next().map(|eye| eye.translation)
}

fn put_eye(world: &mut World, at: Vec3) {
    let mut eyes =
        world.query_filtered::<&mut Transform, (With<PlayerEye>, Without<PortalCamera>)>();
    for mut eye in eyes.iter_mut(world) {
        eye.translation = at;
    }
}

/// The main camera's vertical field of view.
fn main_fov(world: &mut World) -> f32 {
    let mut cameras =
        world.query_filtered::<&Projection, (With<PlayerEye>, Without<PortalCamera>)>();
    match cameras.iter(world).next() {
        Some(Projection::Perspective(perspective)) => perspective.fov,
        _ => PerspectiveProjection::default().fov,
    }
}

#[cfg(test)]
mod tests {
    use super::super::controller::Rect;
    use super::super::floor_plan::{Desk, FloorPlan, Room};
    use super::*;

    /// An attachable root agent's desk at `(row, column)`.
    fn root(session: &str, row: usize, column: usize) -> Desk {
        Desk {
            id: DeskId {
                session: session.into(),
                agent: session.into(),
            },
            row,
            column,
            depth: 0,
            model: None,
            model_id: None,
            flag: None,
            state: agents::AgentState::Busy,
            short_id: Some(format!("{session}-short")),
        }
    }

    fn root_id(session: &str) -> DeskId {
        DeskId {
            session: session.into(),
            agent: session.into(),
        }
    }

    /// Two rooms of attachable root agents, the player in the corridor facing somewhere
    /// particular, and a world view on screen.
    fn world() -> World {
        let mut world = World::new();
        let room = |project: &str, slot: usize, desks: Vec<Desk>| Room {
            project: project.into(),
            name: project.into(),
            slot,
            rows: layout::rows(desks.len()),
            desks,
        };
        world.insert_resource(Plan(FloorPlan {
            rooms: vec![
                room("-alpha", 0, vec![root("a1", 0, 0), root("a2", 0, 1)]),
                room("-beta", 1, vec![root("b1", 0, 0)]),
            ],
        }));
        world.init_resource::<PlayerSeat>();
        world.init_resource::<SeatOutbox>();
        world.init_resource::<SeatedSession>();
        world.insert_resource(Space::Corridor);
        world.insert_resource(Look {
            yaw: 0.7,
            pitch: -0.2,
        });
        world.insert_resource(WorldRect(Some(Rect {
            x: 0,
            y: 28,
            w: 1152,
            h: 840,
        })));
        world.spawn((PlayerEye, Transform::from_xyz(2.0, 1.6, 1.2)));
        world
    }

    fn reported(world: &mut World) -> Vec<Option<String>> {
        std::mem::take(&mut world.resource_mut::<SeatOutbox>().0)
            .into_iter()
            .map(|report| match report {
                FromWorld::Seat(seat) => seat.map(|seat| seat.session_id),
                FromWorld::Hint(hint) => panic!("a hint: {hint:?}"),
            })
            .collect()
    }

    #[test]
    fn seat_to_seat_keeps_where_the_player_first_stood_and_leaving_returns_there() {
        let mut world = world();
        sit_in(&mut world, &root_id("a1"));
        let first = eye(&mut world).unwrap();
        // Across the room, then into another room.
        sit_in(&mut world, &root_id("a2"));
        let second = eye(&mut world).unwrap();
        sit_in(&mut world, &root_id("b1"));
        assert_ne!(first, second);
        assert_eq!(*world.resource::<Space>(), Space::Room(1));
        assert_eq!(world.resource::<SeatedSession>().0.as_deref(), Some("b1"));
        assert_eq!(
            reported(&mut world),
            [Some("a1".into()), Some("a2".into()), Some("b1".into())]
        );

        leave(&mut world, "test");
        assert_eq!(eye(&mut world), Some(Vec3::new(2.0, 1.6, 1.2)));
        assert_eq!(
            *world.resource::<Look>(),
            Look {
                yaw: 0.7,
                pitch: -0.2
            }
        );
        assert_eq!(*world.resource::<Space>(), Space::Corridor);
        assert_eq!(world.resource::<SeatedSession>().0, None);
        assert_eq!(reported(&mut world), [None]);
    }

    #[test]
    fn leaving_a_seat_taken_at_the_cctv_console_returns_to_the_console_facing_it() {
        let mut world = world();
        // In front of the console, looking down at its SIT IN bar.
        let at = Vec3::from_array(layout::cctv_to_world([
            0.2,
            1.6,
            layout::CONSOLE_Z + layout::CONSOLE_HALF[1] + 0.5,
        ]));
        let bar = Vec3::from_array(layout::cctv_to_world([0.17, 0.9, layout::CONSOLE_Z]));
        let d = bar - at;
        let look = Look {
            yaw: (-d.x).atan2(-d.z),
            pitch: d.y.atan2(Vec2::new(d.x, d.z).length()),
        };
        world.insert_resource(Space::Cctv);
        world.insert_resource(look);
        put_eye(&mut world, at);

        sit_in(&mut world, &root_id("b1"));
        assert_eq!(*world.resource::<Space>(), Space::Room(1));
        leave(&mut world, "test");

        assert_eq!(*world.resource::<Space>(), Space::Cctv);
        assert_eq!(eye(&mut world), Some(at));
        assert_eq!(*world.resource::<Look>(), look);
        let facing = Quat::from_euler(EulerRot::YXZ, look.yaw, look.pitch, 0.0) * Vec3::NEG_Z;
        assert!(facing.dot(d.normalize()) > 0.9999);
    }
}
