//! Go to desk (sweatshop-rooms ticket 18; spec: "Go to desk"): pressing an agent in the
//! side panel puts the player beside its desk. The player's eye jumps, with no walking,
//! to the pose [`floor_plan::beside_desk`] works out: in the aisle at the robot's right,
//! in that room's interior, turned to the robot's face and screen. The player's
//! [`Space`] becomes that room, so the main camera sees its render layer and its doorway
//! is the live one (`portals`). An agent with no desk changes nothing. The cursor stays
//! as it is: capture starts only from a click in the world view.

use bevy::prelude::*;

use super::collision::Space;
use super::corridor::Plan;
use super::floor_plan::{self, DeskId};
use super::look::Look;
use super::rooms::InRoom;
use super::walk::PlayerEye;

/// Puts the player's eye beside agent `id`'s desk in the current plan, if it has one.
/// Walking goes on from there, in that room's space.
pub fn go_to_desk(world: &mut World, id: &DeskId) {
    let Some(view) = floor_plan::beside_desk(&world.resource::<Plan>().0, id) else {
        eprintln!(
            "[world] go to desk: {} / {} has no desk, nothing changes",
            id.session, id.agent
        );
        return;
    };
    // A debug build's room placement no longer holds the camera.
    world.remove_resource::<InRoom>();
    let mut eyes = world.query_filtered::<&mut Transform, With<PlayerEye>>();
    for mut eye in eyes.iter_mut(world) {
        eye.translation = Vec3::from_array(view.eye);
    }
    *world.resource_mut::<Space>() = Space::Room(view.slot);
    // `apply_look` turns the camera to it.
    *world.resource_mut::<Look>() = Look {
        yaw: view.yaw,
        pitch: view.pitch,
    };
    eprintln!(
        "[world] go to desk: {} / {} in slot {}, eye ({:.2}, {:.2}, {:.2}), yaw {:.2} pitch {:.2}",
        id.session,
        id.agent,
        view.slot,
        view.eye[0],
        view.eye[1],
        view.eye[2],
        view.yaw,
        view.pitch
    );
}
