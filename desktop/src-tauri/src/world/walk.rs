//! Walking (spec: "Walking, collision and the world changing underfoot"; sweatshop-rooms
//! tickets 20, 21 and 26): WASD or the arrows move the player's eye, Shift runs, the
//! mouse turns it (`look`). How fast is the Walking speed setting (`controls`). Keys reach `ButtonInput<KeyCode>` only while the cursor is
//! captured (`keys`). The player walks in their [`Space`], blocked by its walls and
//! sliding along them, through desk sets and clutter, and through a doorway into the
//! other space (`collision`). When the world changes under them, [`settle`] moves them to
//! where they can stand. No jumping or crouching: the eye stays at
//! [`layout::EYE_HEIGHT`].

use std::time::Instant;

use bevy::prelude::*;

use super::collision::{self, Landing, Space};
use super::controls::Controls;
use super::corridor::Plan;
use super::floor_plan::{layout, FloorPlan};
use super::look::Look;
use super::portals::FollowMain;

/// The longest frame a move covers: a stalled update does not leap the player.
const LONGEST_FRAME: f32 = 0.1;

/// The player's eye: the main camera, which ticket 19's portal cameras are not.
#[derive(Component)]
pub struct PlayerEye;

/// Where the player's eye is, for the world log. Its space is the [`Space`] resource.
#[derive(Resource, Default)]
pub struct Whereabouts {
    pub eye: Vec3,
}

/// One walk, from the first moving frame to the last, for the log.
struct Walked {
    began: Instant,
    from: Vec3,
    distance: f32,
}

pub struct WalkPlugin;

impl Plugin for WalkPlugin {
    fn build(&self, app: &mut App) {
        // Before the portal cameras follow the eye, and so before transform propagation
        // and the frusta in `PostUpdate`: the frame that moves the player into another
        // space draws that space, and the doorway's view from it, from the new pose.
        app.init_resource::<Space>()
            .init_resource::<Controls>()
            .init_resource::<Whereabouts>()
            .add_systems(Update, (settle, walk).chain().before(FollowMain));
    }
}

/// The move the held keys ask for over `seconds`, as world `[x, z]`: forward is where
/// `yaw` faces (-Z at zero yaw), diagonals no faster than straight, at `controls`'
/// walking speed, and Shift runs.
pub fn wanted(
    held: impl Fn(KeyCode) -> bool,
    yaw: f32,
    seconds: f32,
    controls: Controls,
) -> [f32; 2] {
    let either = |a, b| held(a) || held(b);
    let axis = |plus: bool, minus: bool| f32::from(u8::from(plus)) - f32::from(u8::from(minus));
    let forward = axis(
        either(KeyCode::KeyW, KeyCode::ArrowUp),
        either(KeyCode::KeyS, KeyCode::ArrowDown),
    );
    let right = axis(
        either(KeyCode::KeyD, KeyCode::ArrowRight),
        either(KeyCode::KeyA, KeyCode::ArrowLeft),
    );
    let length = forward.hypot(right);
    if length == 0.0 {
        return [0.0, 0.0];
    }
    let speed = if either(KeyCode::ShiftLeft, KeyCode::ShiftRight) {
        controls.run_speed()
    } else {
        controls.walk_speed
    };
    let scale = speed * seconds / length;
    let (sin, cos) = yaw.sin_cos();
    // Forward at yaw is -Z turned by yaw about +Y; right is +X turned the same.
    [
        (-sin * forward + cos * right) * scale,
        (-cos * forward - sin * right) * scale,
    ]
}

/// When the floor plan changes, moves the player to where they can stand in it, if
/// where they stand is gone (`collision::settle`): back onto the floor when a row goes,
/// into the corridor when their room closes, and back to the corridor's end when it
/// shrinks past them. A desk set placed on them leaves them where they are: desk sets do
/// not block. Their turn stays.
pub fn settle(
    plan: Res<Plan>,
    mut previous: Local<FloorPlan>,
    mut space: ResMut<Space>,
    mut eyes: Query<&mut Transform, With<PlayerEye>>,
) {
    if !plan.is_changed() {
        return;
    }
    let before = std::mem::replace(&mut *previous, plan.0.clone());
    let Ok(mut eye) = eyes.single_mut() else {
        return;
    };
    let at = [eye.translation.x, eye.translation.z];
    let Some(landed) = collision::settle(&before, &plan.0, *space, at) else {
        return;
    };
    let [x, z] = landed.at;
    eye.translation = Vec3::new(x, layout::EYE_HEIGHT, z);
    let why = match landed.landing {
        Landing::InFrontOfTheDoorway => "their room closed: in front of where its doorway was",
        Landing::AtTheNewEnd => "the corridor ends before them: at its new end",
        Landing::FreeSpot => "the floor where they stood is gone: at the nearest free spot",
    };
    eprintln!(
        "[world] the world changed under the player, {why}: from ({:.2}, {:.2}) in {} to ({x:.2}, {z:.2}) in {}",
        at[0],
        at[1],
        space.describe(),
        landed.space.describe(),
    );
    space.set_if_neq(landed.space);
}

/// Walks the player in their space, and through a doorway into the other: on the frame
/// the eye passes the doorway's crossing, it moves onto the same spot of the other space
/// and the player is in that space from then on (`collision::cross`).
#[allow(clippy::too_many_arguments)]
fn walk(
    keys: Res<ButtonInput<KeyCode>>,
    look: Res<Look>,
    controls: Res<Controls>,
    time: Res<Time>,
    plan: Res<Plan>,
    mut space: ResMut<Space>,
    mut eyes: Query<&mut Transform, With<PlayerEye>>,
    mut whereabouts: ResMut<Whereabouts>,
    mut walked: Local<Option<Walked>>,
) {
    let Ok(mut eye) = eyes.single_mut() else {
        return;
    };
    let at = [eye.translation.x, eye.translation.z];
    let seconds = time.delta_secs().min(LONGEST_FRAME);
    let by = wanted(|key| keys.pressed(key), look.yaw, seconds, *controls);
    if by != [0.0, 0.0] {
        let blocks = collision::blocks_near(&plan.0, *space, at);
        let [x, z] = collision::slide(at, by, &blocks);
        let before = eye.translation;
        eye.translation = Vec3::new(x, layout::EYE_HEIGHT, z);
        let walk = walked.get_or_insert_with(|| Walked {
            began: Instant::now(),
            from: before,
            distance: 0.0,
        });
        walk.distance += before.distance(eye.translation);
        if let Some(crossing) = collision::cross(&plan.0, *space, [x, z]) {
            let by = Vec3::from_array(crossing.by);
            eprintln!(
                "[world] crossed doorway {:?} from {} at ({x:.3}, {z:.3}) into {} at ({:.3}, {:.3})",
                crossing.doorway,
                space.describe(),
                crossing.space.describe(),
                x + by.x,
                z + by.z,
            );
            eye.translation += by;
            // The walk's start, carried along, so its line reads in the new space.
            walk.from += by;
            *space = crossing.space;
        }
    } else if let Some(done) = walked.take() {
        let ms = done.began.elapsed().as_millis();
        let to = eye.translation;
        eprintln!(
            "[world] walked {:.2} m in {ms} ms ({:.2} m/s) from ({:.2}, {:.2}) to ({:.2}, {:.2}) in {}",
            done.distance,
            done.distance / (ms.max(1) as f32 / 1000.0),
            done.from.x,
            done.from.z,
            to.x,
            to.z,
            space.describe(),
        );
    }
    whereabouts.eye = eye.translation;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::FRAC_PI_2;

    const WALK: f32 = 2.1;
    const RUN: f32 = 4.5;

    fn close(a: [f32; 2], b: [f32; 2]) -> bool {
        (a[0] - b[0]).abs() < 1e-5 && (a[1] - b[1]).abs() < 1e-5
    }

    fn holding(keys: &[KeyCode]) -> impl Fn(KeyCode) -> bool + '_ {
        move |key| keys.contains(&key)
    }

    #[test]
    fn w_walks_forward_where_the_eye_faces_at_walking_speed() {
        let by = wanted(holding(&[KeyCode::KeyW]), 0.0, 1.0, Controls::default());
        assert!(close(by, [0.0, -WALK]), "{by:?}");
        // Turned right (the mouse moved right): forward is +X.
        let by = wanted(
            holding(&[KeyCode::ArrowUp]),
            -FRAC_PI_2,
            0.5,
            Controls::default(),
        );
        assert!(close(by, [WALK / 2.0, 0.0]), "{by:?}");
    }

    #[test]
    fn the_four_directions_and_both_key_sets() {
        let one = |key| wanted(holding(&[key]), 0.0, 1.0, Controls::default());
        assert!(close(one(KeyCode::KeyS), [0.0, WALK]));
        assert!(close(one(KeyCode::ArrowDown), [0.0, WALK]));
        assert!(close(one(KeyCode::KeyD), [WALK, 0.0]));
        assert!(close(one(KeyCode::ArrowRight), [WALK, 0.0]));
        assert!(close(one(KeyCode::KeyA), [-WALK, 0.0]));
        assert!(close(one(KeyCode::ArrowLeft), [-WALK, 0.0]));
        assert_eq!(one(KeyCode::KeyQ), [0.0, 0.0]);
        let both = wanted(
            holding(&[KeyCode::KeyW, KeyCode::KeyS]),
            0.0,
            1.0,
            Controls::default(),
        );
        assert_eq!(both, [0.0, 0.0], "forward and back cancel");
    }

    #[test]
    fn shift_runs_and_diagonals_are_no_faster() {
        let by = wanted(
            holding(&[KeyCode::KeyW, KeyCode::ShiftRight]),
            0.0,
            1.0,
            Controls::default(),
        );
        assert!(close(by, [0.0, -RUN]), "{by:?}");
        let by = wanted(
            holding(&[KeyCode::KeyW, KeyCode::KeyD]),
            0.3,
            1.0,
            Controls::default(),
        );
        assert!((by[0].hypot(by[1]) - WALK).abs() < 1e-5, "{by:?}");
        assert_eq!(
            wanted(
                holding(&[KeyCode::ShiftLeft]),
                0.0,
                1.0,
                Controls::default()
            ),
            [0.0, 0.0]
        );
    }

    #[test]
    fn the_walking_speed_setting_scales_walking_and_running() {
        let fast = Controls::new(4.0, 1.5);
        let by = wanted(holding(&[KeyCode::KeyW]), 0.0, 0.5, fast);
        assert!(close(by, [0.0, -2.0]), "{by:?}");
        let by = wanted(
            holding(&[KeyCode::KeyW, KeyCode::ShiftLeft]),
            0.0,
            1.0,
            fast,
        );
        assert!(close(by, [0.0, -4.0 * 3.0 / 1.4]), "{by:?}");
        let slow = Controls::new(0.1, 1.5);
        let by = wanted(holding(&[KeyCode::KeyD]), 0.0, 1.0, slow);
        assert!(
            close(by, [1.0, 0.0]),
            "clamped to the slider's 1 m/s: {by:?}"
        );
    }
}
