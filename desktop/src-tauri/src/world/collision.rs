//! What blocks the player, and how they slide along it (spec: "Walking, collision and
//! the world changing underfoot"; sweatshop-rooms tickets 20 and 21). Our own code, pure
//! and free of Bevy but for [`Space`] being a resource: the player is an upright cylinder
//! of [`layout::PLAYER_RADIUS`], so in the floor plane a circle, and everything that
//! blocks is an axis-aligned box, one per wall. Desk sets (desk, chair and robot) and
//! clutter do not block (ticket 26): the player walks through them.
//!
//! Collision works in the space the player is in: the corridor, one room's interior, or
//! the CCTV room, each standing elsewhere in world space ([`layout::interior_origin`],
//! [`layout::CCTV_ORIGIN`]). Positions here are world `[x, z]`. The CCTV room's walls
//! and its console block.
//!
//! Doorways are open: near one, the other side's blocks are carried through it
//! ([`blocks_near`]), and once the eye passes halfway through the doorway wall the player
//! [`cross`]es into the other space. When the world changes under the player, [`settle`]
//! moves them to where they can stand.

use bevy::prelude::Resource;

use super::cctv_state;
use super::floor_plan::{layout, FloorPlan};

/// A wall's or slab's thickness, as the kits build them.
const WALL: f32 = layout::WALL;
/// The inner side walls of a room, either side of its middle.
const ROOM_HALF_WIDTH: f32 = 4.75;
/// A room's doorway opening across the room, in its frame (the left as seen from the
/// corridor).
const ROOM_DOORWAY: [f32; 2] = [
    layout::ROOM_DOORWAY_X - layout::DOORWAY_WIDTH / 2.0,
    layout::ROOM_DOORWAY_X + layout::DOORWAY_WIDTH / 2.0,
];
/// The longest single move before collision is checked, well under the player's radius
/// and a wall's thickness, so no move skips through a wall.
const STEP: f32 = 0.05;
/// How often one step's push-outs are repeated, for a corner between two boxes.
const PASSES: usize = 4;
/// The crossing plane: halfway through the doorway wall, the corridor's z -0.2..0 and
/// the room's z 0..0.2 in its frame. The opening the eye looks through stands at the
/// wall's far face on either side, so it is never nearer than 0.1 m on either side of
/// the crossing, and the camera's near plane ([`NEAR`]) never cuts it.
const CROSSING: f32 = -WALL / 2.0;
/// The player's camera's near plane. Half a doorway wall would just clear the default
/// 0.1 m looking straight at the opening, but not at its corners or looking up or down:
/// the near plane's corners reach about 1.22 times as far as its middle. At 0.05 m they
/// stay within 0.061 m, short of the 0.1 m to the opening.
pub const NEAR: f32 = 0.05;
/// How far past its edge a spot still counts as on the floor, for rounding.
const SLACK: f32 = 1e-3;

/// Where the player is: in the corridor, in the room whose interior is in this slot, or
/// in the CCTV room.
/// The one notion of it: walking, collision, the main camera's render layer, the live
/// doorway and the lights all follow it. Crossing a doorway, go to desk and the world
/// changing underfoot set it, together with the eye.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Space {
    #[default]
    Corridor,
    /// The interior in this slot.
    Room(usize),
    /// The CCTV room, behind the corridor's far wall.
    Cctv,
}

impl Space {
    /// For the log.
    pub fn describe(self) -> String {
        match self {
            Space::Corridor => "the corridor".into(),
            Space::Room(slot) => format!("room slot {slot}"),
            Space::Cctv => "the CCTV room".into(),
        }
    }

    /// The layers a camera other than the main one sees this space by: its render layer,
    /// and a room's seat layer too, where sitting in hides the seated robot from the
    /// player alone (`seat`).
    pub fn seen_layers(self) -> bevy::camera::visibility::RenderLayers {
        match self {
            Space::Room(slot) => {
                bevy::camera::visibility::RenderLayers::from_layers(&layout::seen_layers(slot))
            }
            space => bevy::camera::visibility::RenderLayers::from_layers(&[space.render_layer()]),
        }
    }

    /// The render layer of what is in this space, its meshes and its lights: the
    /// corridor's 0, a room's by its slot, or the CCTV room's.
    pub fn render_layer(self) -> usize {
        match self {
            Space::Corridor => 0,
            Space::Room(slot) => layout::render_layer(slot),
            Space::Cctv => layout::CCTV_LAYER,
        }
    }
}

/// An axis-aligned box in the floor plane, `[x, z]` from `min` to `max`. Boxes block at
/// every height the player has.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Block {
    pub min: [f32; 2],
    pub max: [f32; 2],
}

impl Block {
    fn new(x: [f32; 2], z: [f32; 2]) -> Self {
        Self {
            min: [x[0], z[0]],
            max: [x[1], z[1]],
        }
    }

    fn shifted(self, by: [f32; 2]) -> Self {
        Self {
            min: [self.min[0] + by[0], self.min[1] + by[1]],
            max: [self.max[0] + by[0], self.max[1] + by[1]],
        }
    }

    /// Whether `at` is in this box, its edges too.
    fn contains(&self, at: [f32; 2]) -> bool {
        (0..2).all(|i| at[i] >= self.min[i] - SLACK && at[i] <= self.max[i] + SLACK)
    }

    /// The nearest point to `at` in this box.
    fn clamp(&self, at: [f32; 2]) -> [f32; 2] {
        [
            at[0].clamp(self.min[0], self.max[0]),
            at[1].clamp(self.min[1], self.max[1]),
        ]
    }

    /// How far a circle of `radius` at `at` must move to stop overlapping this box, if it
    /// does: out along the nearest point's direction, or, from inside, across the
    /// nearest side.
    fn push_out(&self, at: [f32; 2], radius: f32) -> Option<[f32; 2]> {
        let near = [
            at[0].clamp(self.min[0], self.max[0]),
            at[1].clamp(self.min[1], self.max[1]),
        ];
        let off = [at[0] - near[0], at[1] - near[1]];
        let distance = off[0].hypot(off[1]);
        if distance >= radius {
            return None;
        }
        if distance > 1e-6 {
            let by = (radius - distance) / distance;
            return Some([off[0] * by, off[1] * by]);
        }
        // The centre is inside: leave across the nearest side.
        let sides = [
            (at[0] - self.min[0], [-1.0, 0.0]),
            (self.max[0] - at[0], [1.0, 0.0]),
            (at[1] - self.min[1], [0.0, -1.0]),
            (self.max[1] - at[1], [0.0, 1.0]),
        ];
        let (depth, [dx, dz]) = sides
            .into_iter()
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .expect("four sides");
        Some([dx * (depth + radius), dz * (depth + radius)])
    }
}

/// Which doorway: a project room's, by its place along the corridor, or the CCTV room's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Door {
    Project(usize),
    Cctv,
}

/// A doorway between the corridor and the space behind it, and how it carries one into
/// the other: a project room's in the corridor's doorway wall, or the CCTV room's in its
/// far wall. Collision, crossing and the portals all go by this.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Doorway {
    pub door: Door,
    /// The space behind it.
    pub behind: Space,
    /// The middle of its opening along the corridor.
    pub x: f32,
    /// Whether it is in the far wall (z 2.4..2.6) rather than the doorway wall (z
    /// -0.2..0).
    pub far: bool,
    /// What carries corridor space through it into the space behind: add it to go in,
    /// subtract it to come out. A pure translation, so a turn is the same on both sides.
    pub through: [f32; 3],
    /// The middle of its opening at its wall's far face from the corridor, where the
    /// corridor shows the space behind, in world space.
    pub corridor_side: [f32; 3],
    /// The middle of its opening at the far face from inside, where the space behind
    /// shows the corridor: the corridor face carried through.
    pub inner_side: [f32; 3],
}

impl Doorway {
    /// Which way along Z is in, from the corridor.
    fn inward(&self) -> f32 {
        if self.far {
            1.0
        } else {
            -1.0
        }
    }

    /// Its wall's corridor face, in the corridor.
    fn face(&self) -> f32 {
        if self.far {
            layout::WIDTH
        } else {
            0.0
        }
    }

    /// The crossing plane in the corridor: halfway through its wall.
    fn crossing(&self) -> f32 {
        self.face() + self.inward() * WALL / 2.0
    }

    /// The way the corridor-side opening is seen from the corridor (Z): +1 or -1.
    pub fn facing_corridor(&self) -> f32 {
        -self.inward()
    }
}

/// Every doorway of the corridor: each room's, in corridor order, then the CCTV room's,
/// which is always there.
pub fn doorways(plan: &FloorPlan) -> Vec<Doorway> {
    let mut all: Vec<Doorway> = plan
        .rooms
        .iter()
        .enumerate()
        .map(|(n, room)| Doorway {
            door: Door::Project(n),
            behind: Space::Room(room.slot),
            x: layout::doorway_x(n),
            far: false,
            through: layout::through_doorway(room.slot, n),
            corridor_side: layout::corridor_doorway(n),
            inner_side: layout::room_doorway(room.slot),
        })
        .collect();
    all.push(Doorway {
        door: Door::Cctv,
        behind: Space::Cctv,
        x: layout::cctv_doorway_x(plan),
        far: true,
        through: layout::through_cctv_doorway(plan),
        corridor_side: layout::cctv_corridor_doorway(plan),
        inner_side: layout::cctv_room_doorway(),
    });
    all
}

/// Everything that blocks in `space`: its walls, and the CCTV room's console.
pub fn blocks(plan: &FloorPlan, space: Space) -> Vec<Block> {
    match space {
        Space::Corridor => corridor_walls(plan),
        Space::Room(slot) => plan
            .rooms
            .iter()
            .find(|room| room.slot == slot)
            .map(|room| room_walls(room.slot, room.rows))
            .unwrap_or_default(),
        Space::Cctv => cctv_walls(plan),
    }
}

/// The corridor's walls: the far wall, open at the CCTV doorway, the two end walls and
/// the doorway wall, open at each room's doorway. The corridor runs along +X, its doorway wall's corridor face at
/// z = 0 with the rooms behind it (-Z) and the far wall's face at z = [`layout::WIDTH`].
fn corridor_walls(plan: &FloorPlan) -> Vec<Block> {
    let (start, far) = layout::end_walls(plan);
    let across = [-WALL, layout::WIDTH + WALL];
    let [left, right] = opening(layout::cctv_doorway_x(plan));
    let far_wall = [layout::WIDTH, layout::WIDTH + WALL];
    let mut walls = vec![
        Block::new([start - WALL, left], far_wall),
        Block::new([right, far + WALL], far_wall),
        Block::new([start - WALL, start], across),
        Block::new([far, far + WALL], across),
    ];
    let mut from = start - WALL;
    for n in 0..plan.rooms.len() {
        let [left, right] = opening(layout::doorway_x(n));
        walls.push(Block::new([from, left], [-WALL, 0.0]));
        from = right;
    }
    walls.push(Block::new([from, far + WALL], [-WALL, 0.0]));
    walls
}

/// A doorway's opening across its wall, from its middle.
fn opening(middle: f32) -> [f32; 2] {
    let half = layout::DOORWAY_WIDTH / 2.0;
    [middle - half, middle + half]
}

/// The walls of the room in `slot`, `rows` deep, in world space. The room frame has the
/// doorway wall's inner face at z = 0, the room towards -Z and the back wall's inner face
/// at `-interior_depth(rows)`.
fn room_walls(slot: usize, rows: usize) -> Vec<Block> {
    let depth = layout::interior_depth(rows);
    let side = ROOM_HALF_WIDTH;
    let along = [-depth - WALL, WALL];
    let [left, right] = ROOM_DOORWAY;
    let walls = [
        Block::new([-side - WALL, -side], along),
        Block::new([side, side + WALL], along),
        Block::new([-side - WALL, side + WALL], [-depth - WALL, -depth]),
        Block::new([-side - WALL, left], [0.0, WALL]),
        Block::new([right, side + WALL], [0.0, WALL]),
    ];
    let origin = layout::interior_origin(slot);
    walls
        .into_iter()
        .map(|block| block.shifted([origin[0], origin[2]]))
        .collect()
}

/// A box of the CCTV room's frame (X across, the room towards -Z), in world space: the
/// room stands turned half round ([`layout::cctv_to_world`]).
fn cctv_block(x: [f32; 2], z: [f32; 2]) -> Block {
    let [ox, _, oz] = layout::CCTV_ORIGIN;
    Block::new([ox - x[1], ox - x[0]], [oz - z[1], oz - z[0]])
}

/// The CCTV room's walls, open at its doorway, and its console, in world space. The
/// room frame has the doorway wall's inner face at z = 0, the room towards -Z, the
/// monitor wall's inner face at `-CCTV_DEPTH`. Its side walls stand as wide as the
/// plan's monitor wall needs (`cctv_state::room_half_width`): past twelve rooms it widens.
fn cctv_walls(plan: &FloorPlan) -> Vec<Block> {
    let side = cctv_state::room_half_width(plan.rooms.len());
    let depth = layout::CCTV_DEPTH;
    let along = [-depth - WALL, WALL];
    let [left, right] = opening(0.0);
    let [cx, cz] = layout::CONSOLE_HALF;
    vec![
        cctv_block([-side - WALL, -side], along),
        cctv_block([side, side + WALL], along),
        cctv_block([-side - WALL, side + WALL], [-depth - WALL, -depth]),
        cctv_block([-side - WALL, left], [0.0, WALL]),
        cctv_block([right, side + WALL], [0.0, WALL]),
        cctv_block([-cx, cx], [layout::CONSOLE_Z - cz, layout::CONSOLE_Z + cz]),
    ]
}

/// Where a player at `at` ends up after trying to move `by`: blocked by `blocks`, and
/// sliding along them, so walking into a wall at an angle keeps the part of the move
/// along the wall.
pub fn slide(at: [f32; 2], by: [f32; 2], blocks: &[Block]) -> [f32; 2] {
    let length = by[0].hypot(by[1]);
    let steps = (length / STEP).ceil().max(1.0);
    let step = [by[0] / steps, by[1] / steps];
    let mut at = at;
    for _ in 0..steps as usize {
        at = [at[0] + step[0], at[1] + step[1]];
        for _ in 0..PASSES {
            let mut pushed = false;
            for block in blocks {
                if let Some(push) = block.push_out(at, layout::PLAYER_RADIUS) {
                    at = [at[0] + push[0], at[1] + push[1]];
                    pushed = true;
                }
            }
            if !pushed {
                break;
            }
        }
    }
    at
}

/// The doorway nearest a player at `at` in `space`: in the corridor the nearest of its
/// openings, in either wall, in a room or the CCTV room its own.
fn doorway(plan: &FloorPlan, space: Space, at: [f32; 2]) -> Option<Doorway> {
    let all = doorways(plan);
    match space {
        Space::Corridor => {
            let off = |d: &Doorway| (d.x - at[0]).hypot(d.face() - at[1]);
            all.into_iter().min_by(|a, b| off(a).total_cmp(&off(b)))
        }
        Space::Room(_) | Space::Cctv => all.into_iter().find(|d| d.behind == space),
    }
}

/// The space on the other side of `doorway` from `space`, and what carries world space
/// from this side into it: [`Doorway::through`] going in, the reverse coming out.
fn other_side(space: Space, doorway: &Doorway) -> (Space, [f32; 3]) {
    match space {
        Space::Corridor => (doorway.behind, doorway.through),
        Space::Room(_) | Space::Cctv => (Space::Corridor, doorway.through.map(|v| -v)),
    }
}

/// What blocks a walk from `at` in `space`: the space's own blocks, and those beyond the
/// doorway the player is nearest, carried through it. So walking through a doorway meets
/// the other side's walls where they stand, before and after the crossing alike. Every
/// carried block lies behind the doorway wall's corridor face (in a room, its room face),
/// where only the doorway's opening reaches.
pub fn blocks_near(plan: &FloorPlan, space: Space, at: [f32; 2]) -> Vec<Block> {
    let mut all = blocks(plan, space);
    if let Some(doorway) = doorway(plan, space, at) {
        let (other, [x, _, z]) = other_side(space, &doorway);
        all.extend(
            blocks(plan, other)
                .into_iter()
                .map(|block| block.shifted([-x, -z])),
        );
    }
    all
}

/// A doorway crossing: the space the player is now in, how far their eye moves into it,
/// and which doorway.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Crossing {
    pub space: Space,
    pub by: [f32; 3],
    pub doorway: Door,
}

/// The crossing, if the eye at `at` in `space` has passed halfway through a doorway's
/// wall ([`CROSSING`]) within its opening. Moving the eye by [`Crossing::by`] puts it on
/// the same spot of the other side, which then looks exactly as the doorway showed it.
/// The mapping is a pure translation, so the eye keeps its turn (`Look`); a turning
/// mapping would have to turn `Look::yaw` with it.
pub fn cross(plan: &FloorPlan, space: Space, at: [f32; 2]) -> Option<Crossing> {
    let doorway = doorway(plan, space, at)?;
    let inward = doorway.inward();
    let (past, middle) = match space {
        Space::Corridor => (inward * (at[1] - doorway.crossing()) > 0.0, doorway.x),
        Space::Room(_) | Space::Cctv => (
            inward * (at[1] - doorway.crossing() - doorway.through[2]) < 0.0,
            doorway.x + doorway.through[0],
        ),
    };
    let within = (at[0] - middle).abs() < layout::DOORWAY_WIDTH / 2.0;
    (past && within).then(|| {
        let (space, by) = other_side(space, &doorway);
        Crossing {
            space,
            by,
            doorway: doorway.door,
        }
    })
}

/// Where the player's centre can be in `space`: its floor clear of the walls by the
/// player's radius, and each doorway's opening up to the crossing.
fn area(plan: &FloorPlan, space: Space) -> Vec<Block> {
    const R: f32 = layout::PLAYER_RADIUS;
    let opening = |middle: f32| {
        let half = layout::DOORWAY_WIDTH / 2.0 - R;
        [middle - half, middle + half]
    };
    match space {
        Space::Corridor => {
            let (start, far) = layout::end_walls(plan);
            let mut area = vec![Block::new([start + R, far - R], [R, layout::WIDTH - R])];
            area.extend(
                (0..plan.rooms.len())
                    .map(|n| Block::new(opening(layout::doorway_x(n)), [CROSSING, R])),
            );
            area.push(Block::new(
                opening(layout::cctv_doorway_x(plan)),
                [layout::WIDTH - R, layout::WIDTH - CROSSING],
            ));
            area
        }
        Space::Room(slot) => plan
            .rooms
            .iter()
            .find(|room| room.slot == slot)
            .map(|room| {
                let [x, _, z] = layout::interior_origin(slot);
                let depth = layout::interior_depth(room.rows);
                let side = ROOM_HALF_WIDTH - R;
                vec![
                    Block::new([x - side, x + side], [z - depth + R, z - R]),
                    Block::new(opening(x + layout::ROOM_DOORWAY_X), [z - R, z - CROSSING]),
                ]
            })
            .unwrap_or_default(),
        Space::Cctv => {
            // The floor around the console, in four strips, and the doorway's opening.
            let side = cctv_state::room_half_width(plan.rooms.len()) - R;
            let [cx, cz] = layout::CONSOLE_HALF;
            let (front, back) = (layout::CONSOLE_Z + cz + R, layout::CONSOLE_Z - cz - R);
            let (inner, outer) = (-R, -layout::CCTV_DEPTH + R);
            vec![
                cctv_block([-side, side], [front, inner]),
                cctv_block([-side, side], [outer, back]),
                cctv_block([-side, -cx - R], [outer, inner]),
                cctv_block([cx + R, side], [outer, inner]),
                cctv_block(opening(0.0), [inner, -CROSSING]),
            ]
        }
    }
}

/// The nearest spot to `at` where the player can stand in `space`: `at` itself if it is
/// on the floor, else the nearest point of the floor. Only walls block, and the floor
/// keeps the player's radius from every one of them, so any spot on it is free.
pub fn free_spot(plan: &FloorPlan, space: Space, at: [f32; 2]) -> [f32; 2] {
    let area = area(plan, space);
    if area.iter().any(|a| a.contains(at)) {
        return at;
    }
    let distance = |p: &[f32; 2]| (p[0] - at[0]).hypot(p[1] - at[1]);
    area.iter()
        .map(|a| a.clamp(at))
        .min_by(|a, b| distance(a).total_cmp(&distance(b)))
        .unwrap_or(at)
}

/// Where a player at `at` in the room in `slot` stands clear of every desk set there:
/// `at` itself, or pushed out of the desk set it stands in by the walking rule (out
/// across the nearest side, the player's radius clear), then onto the floor
/// ([`free_spot`]). Desk sets never block walking; this is for putting the player back
/// where they stood after sitting in (agent-screens spec: "The seated view").
pub fn clear_of_desks(plan: &FloorPlan, slot: usize, at: [f32; 2]) -> [f32; 2] {
    let Some(room) = plan.rooms.iter().find(|room| room.slot == slot) else {
        return at;
    };
    let [ox, _, oz] = layout::interior_origin(slot);
    let mut at = at;
    for desk in &room.desks {
        let (x, z) = layout::desk_box(desk.row, desk.column);
        let set = Block::new(x, z).shifted([ox, oz]);
        if let Some(by) = set.push_out(at, layout::PLAYER_RADIUS) {
            at = [at[0] + by[0], at[1] + by[1]];
        }
    }
    free_spot(plan, Space::Room(slot), at)
}

/// Why the world changing moved the player.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Landing {
    /// Their room closed: they stand in the corridor in front of where its doorway was.
    InFrontOfTheDoorway,
    /// The corridor ends before where they were: it shrank past them, or their room
    /// closed and its doorway was beyond the new end. They stand at the new end.
    AtTheNewEnd,
    /// The floor under them went (a row removed, its back wall now where they stood):
    /// they stand at the nearest free spot. Desk sets coming and going never move them:
    /// they do not block.
    FreeSpot,
}

/// Where the world changing put the player: the space, the spot, and why.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Landed {
    pub space: Space,
    pub at: [f32; 2],
    pub landing: Landing,
}

/// Where a player at `at` in `space` stands once the floor plan changed from `previous`
/// to `plan`, if they have to move. Their room is gone when no room of the plan has its
/// slot, or another project's room took the slot.
pub fn settle(
    previous: &FloorPlan,
    plan: &FloorPlan,
    space: Space,
    at: [f32; 2],
) -> Option<Landed> {
    const R: f32 = layout::PLAYER_RADIUS;
    let (_, far) = layout::end_walls(plan);
    if let Space::Room(slot) = space {
        let project = |p: &FloorPlan| {
            p.rooms
                .iter()
                .find(|room| room.slot == slot)
                .map(|room| room.project.clone())
        };
        let gone = match (project(plan), project(previous)) {
            (None, _) => true,
            (Some(now), Some(was)) => now != was,
            (Some(_), None) => false,
        };
        if gone {
            let n = previous
                .rooms
                .iter()
                .position(|room| room.slot == slot)
                .unwrap_or(0);
            let x = layout::doorway_x(n);
            let (x, landing) = if x > far - R {
                (far - R, Landing::AtTheNewEnd)
            } else {
                (x, Landing::InFrontOfTheDoorway)
            };
            return Some(Landed {
                space: Space::Corridor,
                at: free_spot(plan, Space::Corridor, [x, layout::WIDTH / 2.0]),
                landing,
            });
        }
    }
    let free = free_spot(plan, space, at);
    (free != at).then(|| Landed {
        space,
        at: free,
        landing: if space == Space::Corridor && at[0] > far - R {
            Landing::AtTheNewEnd
        } else {
            Landing::FreeSpot
        },
    })
}

#[cfg(test)]
mod tests {
    use super::super::clutter;
    use super::super::floor_plan::{Desk, DeskId, Room};
    use super::*;

    const R: f32 = layout::PLAYER_RADIUS;

    fn close(a: [f32; 2], b: [f32; 2]) -> bool {
        (a[0] - b[0]).abs() < 1e-3 && (a[1] - b[1]).abs() < 1e-3
    }

    /// An empty room in `slot`.
    fn room(slot: usize) -> Room {
        Room {
            project: format!("-p{slot}"),
            name: format!("p{slot}"),
            slot,
            rows: layout::MIN_ROWS,
            desks: Vec::new(),
        }
    }

    /// A room in `slot`, `rows` deep, with desk sets at `seats` (`(row, column)`).
    fn room_with(slot: usize, rows: usize, seats: &[(usize, usize)]) -> Room {
        let desks = seats
            .iter()
            .map(|&(row, column)| Desk {
                id: DeskId {
                    session: format!("s{row}{column}"),
                    agent: format!("a{row}{column}"),
                },
                row,
                column,
                depth: 0,
                state: agents::AgentState::Busy,
                model: None,
                model_id: None,
                flag: None,
                short_id: None,
            })
            .collect();
        Room {
            rows,
            desks,
            ..room(slot)
        }
    }

    fn plan(rooms: Vec<Room>) -> FloorPlan {
        FloorPlan { rooms }
    }

    /// Each crossing of a walk, with the spot it was made from.
    type Crossings = Vec<([f32; 2], Crossing)>;

    /// Walks from `at` in `space` by `by` in steps of `step`, as frames of `walk` do:
    /// sliding against the blocks near the player, and crossing where a step passes a
    /// doorway's crossing. Gives where the walk ends, and its crossings.
    fn walk(
        plan: &FloorPlan,
        mut space: Space,
        mut at: [f32; 2],
        by: [f32; 2],
        step: f32,
    ) -> (Space, [f32; 2], Crossings) {
        let steps = (by[0].hypot(by[1]) / step).ceil() as usize;
        let one = [by[0] / steps as f32, by[1] / steps as f32];
        let mut crossings = Vec::new();
        for _ in 0..steps {
            at = slide(at, one, &blocks_near(plan, space, at));
            if let Some(crossing) = cross(plan, space, at) {
                crossings.push((at, crossing));
                at = [at[0] + crossing.by[0], at[1] + crossing.by[2]];
                space = crossing.space;
            }
        }
        (space, at, crossings)
    }

    /// Whether the player can stand at `at` in `space`.
    fn stands_free(plan: &FloorPlan, space: Space, at: [f32; 2]) -> bool {
        free_spot(plan, space, at) == at
    }

    #[test]
    fn walking_into_a_wall_stops_the_player_its_radius_from_it() {
        let plan = plan(vec![room(0)]);
        let walls = blocks(&plan, Space::Corridor);
        // Straight at the far wall (+Z) from the middle of the corridor.
        let at = slide([2.0, 1.2], [0.0, 5.0], &walls);
        assert!(close(at, [2.0, layout::WIDTH - R]), "{at:?}");
        // Back to the doorway wall, beside the doorway.
        let at = slide([0.0, 1.2], [0.0, -5.0], &walls);
        assert!(close(at, [0.0, R]), "{at:?}");
        // Along the corridor into the far end wall.
        let (_, far) = layout::end_walls(&plan);
        let at = slide([2.0, 1.2], [20.0, 0.0], &walls);
        assert!(close(at, [far - R, 1.2]), "{at:?}");
    }

    #[test]
    fn a_long_move_does_not_skip_through_a_wall() {
        let plan = plan(Vec::new());
        let walls = blocks(&plan, Space::Corridor);
        // Beside the CCTV doorway (x 0.65..1.85 with no room open).
        let at = slide([2.5, 1.2], [0.0, 100.0], &walls);
        assert!(close(at, [2.5, layout::WIDTH - R]), "{at:?}");
    }

    #[test]
    fn walking_into_a_wall_at_an_angle_slides_along_it() {
        let plan = plan(vec![room(0), room(1), room(2)]);
        let walls = blocks(&plan, Space::Corridor);
        // 45 degrees into the far wall: the move along the corridor is kept.
        let at = slide([1.0, 1.9], [2.0, 2.0], &walls);
        assert!(close(at, [3.0, layout::WIDTH - R]), "{at:?}");
        // And into the doorway wall between two doorways.
        let start = [layout::doorway_x(1) + 0.9, 0.6];
        let at = slide(start, [0.3, -0.6], &walls);
        assert!(close(at, [start[0] + 0.3, R]), "{at:?}");
    }

    #[test]
    fn walking_through_a_doorway_crosses_into_its_room_and_back_on_the_same_spot() {
        let plan = plan(vec![room(0), room(3)]);
        let middle = layout::doorway_x(1);
        let through = layout::through_doorway(3, 1);
        // Walking frames at 60 fps, straight in for 3 m: through the doorway and on.
        let step = 1.4 / 60.0;
        let (space, end, crossings) =
            walk(&plan, Space::Corridor, [middle, 1.2], [0.0, -3.0], step);
        assert_eq!(space, Space::Room(3));
        let [(from, inwards)] = crossings[..] else {
            panic!("one crossing: {crossings:?}");
        };
        assert_eq!(
            inwards,
            Crossing {
                space: Space::Room(3),
                by: through,
                doorway: Door::Project(1),
            }
        );
        // Made on the first step past halfway through the wall, within the opening.
        assert!(from[1] < CROSSING && from[1] > CROSSING - step, "{from:?}");
        assert!((from[0] - middle).abs() < 1e-4, "{from:?}");
        // 3 m from where the walk began, carried into the room's frame.
        let [ox, _, oz] = layout::interior_origin(3);
        assert!(close(end, [ox - 3.5, oz + 1.2 + WALL - 3.0]), "{end:?}");

        // Back out the same way: the crossing undoes the one in.
        let (space, back, crossings) = walk(&plan, space, end, [0.0, 3.0], step);
        assert_eq!(space, Space::Corridor);
        let [(from, outwards)] = crossings[..] else {
            panic!("one crossing: {crossings:?}");
        };
        assert_eq!(outwards.by, through.map(|v| -v));
        assert_eq!(outwards.doorway, Door::Project(1));
        assert!(
            from[1] > oz - CROSSING && from[1] < oz - CROSSING + step,
            "{from:?}"
        );
        assert!(close(back, [middle, 1.2]), "{back:?}");
    }

    /// The CCTV room's frame (`layout::cctv_to_world`) in world `[x, z]`.
    fn cctv(x: f32, z: f32) -> [f32; 2] {
        let [x, _, z] = layout::cctv_to_world([x, 0.0, z]);
        [x, z]
    }

    #[test]
    fn walking_through_the_cctv_doorway_crosses_into_the_cctv_room_and_back_on_the_same_spot() {
        let plan = plan(vec![room(0), room(1), room(2)]);
        let middle = layout::cctv_doorway_x(&plan);
        let through = layout::through_cctv_doorway(&plan);
        let step = 1.4 / 60.0;
        // From the middle of the corridor, straight across it for 3 m: through the far
        // wall's doorway and on.
        let (space, end, crossings) = walk(&plan, Space::Corridor, [middle, 1.2], [0.0, 3.0], step);
        assert_eq!(space, Space::Cctv);
        let [(from, inwards)] = crossings[..] else {
            panic!("one crossing: {crossings:?}");
        };
        assert_eq!(
            inwards,
            Crossing {
                space: Space::Cctv,
                by: through,
                doorway: Door::Cctv,
            }
        );
        // Made on the first step past halfway through the far wall (z 2.4..2.6).
        assert!(from[1] > 2.5 && from[1] < 2.5 + step, "{from:?}");
        // 3 m on, which is 1.6 m into the room past its doorway wall's inner face.
        assert!(close(end, cctv(0.0, -1.6)), "{end:?}");

        // Back out the same way.
        let (space, back, crossings) = walk(&plan, space, end, [0.0, -3.0], step);
        assert_eq!(space, Space::Corridor);
        let [(_, outwards)] = crossings[..] else {
            panic!("one crossing: {crossings:?}");
        };
        assert_eq!(outwards.by, through.map(|v| -v));
        assert!(close(back, [middle, 1.2]), "{back:?}");
    }

    #[test]
    fn the_cctv_rooms_walls_and_console_block() {
        let plan = plan(vec![room(0), room(1)]);
        let near = |at: [f32; 2]| blocks_near(&plan, Space::Cctv, at);
        // Straight in from the doorway: the console stands 2.6 m out from the monitor
        // wall (its slab 0.8 m deep, room z -4.8..-4.0), and stops the player.
        let at = cctv(0.0, -0.5);
        let stopped = slide(at, [0.0, 10.0], &near(at));
        assert!(close(stopped, cctv(0.0, -4.0 + R)), "{stopped:?}");
        // Beside the console (its slab x -1.3..1.3), on to the monitor wall.
        let at = cctv(2.0, -2.0);
        let stopped = slide(at, [0.0, 10.0], &near(at));
        assert!(close(stopped, cctv(2.0, -7.0 + R)), "{stopped:?}");
        // Across the room to the right as seen from the doorway (world -X), and to the
        // left, into the side walls (x ±4).
        let at = cctv(0.0, -2.0);
        let stopped = slide(at, [-10.0, 0.0], &near(at));
        assert!(close(stopped, cctv(4.0 - R, -2.0)), "{stopped:?}");
        let stopped = slide(at, [10.0, 0.0], &near(at));
        assert!(close(stopped, cctv(-4.0 + R, -2.0)), "{stopped:?}");
        // Back to the doorway wall beside the doorway (x -0.6..0.6).
        let at = cctv(2.0, -2.0);
        let stopped = slide(at, [0.0, -10.0], &near(at));
        assert!(close(stopped, cctv(2.0, -R)), "{stopped:?}");
        // Behind the console, side to side along it: its back blocks nothing there.
        let at = cctv(-3.0, -5.5);
        let along = slide(at, [-6.0, 0.0], &near(at));
        assert!(close(along, cctv(3.0, -5.5)), "{along:?}");
    }

    #[test]
    fn the_cctv_doorway_snapping_onto_the_player_pushes_them_to_the_nearest_free_spot() {
        let before = plan(vec![room(0), room(1)]);
        let after = plan(vec![room(0), room(1), room(2)]);
        // In the CCTV doorway's opening on the corridor's side (x 1.9..3.1 with two
        // rooms); with three it moves to x 3.15..4.35 and the far wall stands there.
        let doorway = [layout::cctv_doorway_x(&before), 2.45];
        assert!(stands_free(&before, Space::Corridor, doorway));
        assert_eq!(
            settle(&before, &after, Space::Corridor, doorway),
            Some(Landed {
                space: Space::Corridor,
                at: [doorway[0], layout::WIDTH - R],
                landing: Landing::FreeSpot,
            })
        );
        // Where it lands, in the corridor in front of it, the player stays.
        let front = [layout::cctv_doorway_x(&after), 2.0];
        assert_eq!(settle(&before, &after, Space::Corridor, front), None);
        // The room behind it does not move: a player in it stays where they are.
        assert_eq!(settle(&before, &after, Space::Cctv, cctv(0.2, -0.1)), None);
        assert_eq!(settle(&before, &after, Space::Cctv, cctv(3.0, -6.0)), None);
    }

    /// A plan of `count` rooms, slots 0 up.
    fn rooms(count: usize) -> FloorPlan {
        plan((0..count).map(room).collect())
    }

    #[test]
    fn past_twelve_rooms_the_cctv_rooms_side_walls_stand_a_column_out_per_extra_column() {
        // As seen from the doorway, room +X is world -X (the room stands turned round).
        for (count, side) in [
            (12, 4.0),
            (13, 4.0 + 2.0 / 3.0),
            (16, 4.0 + 4.0 / 3.0),
            (24, 4.0 + 8.0 / 3.0),
        ] {
            let plan = rooms(count);
            let near = |at: [f32; 2]| blocks_near(&plan, Space::Cctv, at);
            let at = cctv(0.0, -2.0);
            let right = slide(at, [-20.0, 0.0], &near(at));
            assert!(close(right, cctv(side - R, -2.0)), "{count}: {right:?}");
            let left = slide(at, [20.0, 0.0], &near(at));
            assert!(close(left, cctv(-side + R, -2.0)), "{count}: {left:?}");
            // The doorway wall and the monitor wall still close it along its new width.
            let corner = cctv(side - R - 0.1, -2.0);
            let back = slide(corner, [0.0, 10.0], &near(corner));
            assert!(
                close(back, cctv(side - R - 0.1, -7.0 + R)),
                "{count}: {back:?}"
            );
            let front = slide(corner, [0.0, -10.0], &near(corner));
            assert!(close(front, cctv(side - R - 0.1, -R)), "{count}: {front:?}");
            // The floor reaches the new side walls, and no further.
            assert!(stands_free(&plan, Space::Cctv, cctv(side - R, -2.0)));
            assert!(!stands_free(
                &plan,
                Space::Cctv,
                cctv(side - R + 0.05, -2.0)
            ));
        }
    }

    #[test]
    fn a_side_wall_landing_on_the_player_as_the_cctv_room_narrows_pushes_them_aside() {
        let (wide, narrow) = (rooms(16), rooms(12));
        // Standing where the right side wall lands (x 4..4.2 once it narrows to 8 m).
        let at = cctv(4.1, -2.0);
        assert!(stands_free(&wide, Space::Cctv, at));
        assert_eq!(
            settle(&wide, &narrow, Space::Cctv, at),
            Some(Landed {
                space: Space::Cctv,
                at: cctv(4.0 - R, -2.0),
                landing: Landing::FreeSpot,
            })
        );
        // Out beyond where it lands, in the left part that goes: on to the floor left.
        let landed = settle(&wide, &narrow, Space::Cctv, cctv(-5.0, -6.0)).expect("moved");
        assert!(close(landed.at, cctv(-4.0 + R, -6.0)), "{landed:?}");
        // Clear of it, and as the room widens again, nobody moves.
        assert_eq!(settle(&wide, &narrow, Space::Cctv, cctv(2.0, -2.0)), None);
        assert_eq!(
            settle(&narrow, &wide, Space::Cctv, cctv(4.0 - R, -2.0)),
            None
        );
        // A column at a time: from 16 rooms to 13, the wall lands at 4 + 2/3.
        let landed = settle(&wide, &rooms(13), Space::Cctv, cctv(5.0, -3.0)).expect("moved");
        assert!(
            close(landed.at, cctv(4.0 + 2.0 / 3.0 - R, -3.0)),
            "{landed:?}"
        );
    }

    #[test]
    fn the_start_pose_is_unchanged_on_the_corridors_side_of_the_cctv_doorway() {
        let [x, y, z] = layout::start_eye();
        assert!(close([x, z], [1.25, 2.1]) && y == 1.6, "{x} {y} {z}");
        let start = [x, z];
        for rooms in 0..4 {
            let plan = plan((0..rooms).map(room).collect());
            assert!(stands_free(&plan, Space::Corridor, start), "{rooms} rooms");
            assert_eq!(cross(&plan, Space::Corridor, start), None, "{rooms} rooms");
        }
        // With one room (or none) it is right in front of the CCTV doorway.
        assert_eq!(layout::cctv_doorway_x(&plan(vec![room(0)])), start[0]);
    }

    #[test]
    fn a_crossing_happens_only_past_halfway_through_the_wall_within_the_opening() {
        let plan = plan(vec![room(0), room(1)]);
        let middle = layout::doorway_x(0);
        assert_eq!(cross(&plan, Space::Corridor, [middle, -0.09]), None);
        let inwards = cross(&plan, Space::Corridor, [middle, -0.11]).expect("a crossing");
        assert_eq!(inwards.space, Space::Room(0));
        // Beside the opening, behind the wall, where no walk reaches: no crossing.
        assert_eq!(cross(&plan, Space::Corridor, [middle + 0.7, -0.11]), None);
        // From the room, the same plane in its frame, onto the same spot.
        let [ox, _, oz] = layout::interior_origin(1);
        let door = ox + layout::ROOM_DOORWAY_X;
        assert_eq!(cross(&plan, Space::Room(1), [door, oz + 0.09]), None);
        let out = cross(&plan, Space::Room(1), [door, oz + 0.11]).expect("a crossing");
        assert_eq!(out.space, Space::Corridor);
        let landed = [door + out.by[0], oz + 0.11 + out.by[2]];
        assert!(close(landed, [layout::doorway_x(1), -0.09]), "{landed:?}");
        // With no room open there is no doorway to cross.
        let none = FloorPlan::default();
        assert_eq!(cross(&none, Space::Corridor, [middle, -0.11]), None);
        // The CCTV doorway's crossing is halfway through the far wall (z 2.4..2.6), there
        // with or without rooms, and lands on the same spot of the CCTV room.
        for plan in [&plan, &none] {
            let middle = layout::cctv_doorway_x(plan);
            assert_eq!(cross(plan, Space::Corridor, [middle, 2.49]), None);
            assert_eq!(cross(plan, Space::Corridor, [middle + 0.7, 2.51]), None);
            let inwards = cross(plan, Space::Corridor, [middle, 2.51]).expect("a crossing");
            assert_eq!(inwards.space, Space::Cctv);
            let out = cross(plan, Space::Cctv, cctv(0.0, 0.11)).expect("a crossing");
            assert_eq!(out.space, Space::Corridor);
            assert_eq!(cross(plan, Space::Cctv, cctv(0.0, 0.09)), None);
            let [x, z] = cctv(0.0, 0.11);
            let landed = [x + out.by[0], z + out.by[2]];
            assert!(close(landed, [middle, 2.49]), "{landed:?}");
        }
    }

    #[test]
    fn in_a_doorway_both_sides_walls_block_where_they_stand() {
        let plan = plan(vec![room(0), room(1)]);
        let middle = layout::doorway_x(1);
        let jamb = middle + layout::DOORWAY_WIDTH / 2.0 - R;
        // In the opening on the corridor's side of the crossing, strafing meets the jamb.
        let at = [middle, -0.05];
        let at = slide(at, [1.0, 0.0], &blocks_near(&plan, Space::Corridor, at));
        assert!(close(at, [jamb, -0.05]), "{at:?}");
        // Just past the crossing, in the room: its jamb, at the same place.
        let [ox, _, oz] = layout::interior_origin(1);
        let inside = [ox + layout::ROOM_DOORWAY_X, oz + 0.05];
        let at = slide(
            inside,
            [1.0, 0.0],
            &blocks_near(&plan, Space::Room(1), inside),
        );
        let through = layout::through_doorway(1, 1);
        let seen = [at[0] - through[0], at[1] - through[2]];
        assert!(close(seen, [jamb, -0.15]), "{seen:?}");
        // Walking at an angle into the doorway slides along its jamb and still crosses,
        // onto the room's free floor.
        let (space, end, crossings) = walk(
            &plan,
            Space::Corridor,
            [middle - 0.5, 1.0],
            [1.5, -3.0],
            0.03,
        );
        assert_eq!((space, crossings.len()), (Space::Room(1), 1));
        assert!(stands_free(&plan, space, end), "{end:?}");
        // With no room open, the plain corridor's wall is closed where doorway 0 would be.
        let first = [layout::doorway_x(0), 1.2];
        let none = FloorPlan::default();
        let at = slide(
            first,
            [0.0, -5.0],
            &blocks_near(&none, Space::Corridor, first),
        );
        assert!(close(at, [first[0], R]), "{at:?}");
    }

    #[test]
    fn walking_straight_through_a_desk_set_is_not_stopped_while_walls_stop_and_slide() {
        // A full room: desk sets in every seat, the entry seat's too.
        let seats: Vec<_> = (0..12).map(|n| (n / 4, n % 4)).collect();
        let full = plan(vec![room_with(1, 3, &seats)]);
        let walls = blocks(&full, Space::Room(1));
        assert_eq!(
            walls.len(),
            5,
            "the room's five walls and nothing else block"
        );
        let [ox, _, oz] = layout::interior_origin(1);
        let world = |x: f32, z: f32| [ox + x, oz + z];
        let depth = layout::interior_depth(3);

        // Straight down column 1, through the PC, desk and chair of every row, to the
        // back wall: only the wall stops the player, their radius from it.
        let [x, _] = layout::desk_position(0, 1);
        let at = slide(world(x, -0.3), [0.0, -20.0], &walls);
        assert!(close(at, world(x, -depth + R)), "{at:?}");
        // Across the first row, through all four desk sets, wall to wall.
        let [_, z] = layout::desk_position(0, 0);
        let at = slide(world(-4.0, z + 0.4), [20.0, 0.0], &walls);
        assert!(close(at, world(ROOM_HALF_WIDTH - R, z + 0.4)), "{at:?}");
        // At an angle into the side wall, from inside a desk set, the walk slides along
        // the wall.
        let [x, z] = layout::desk_position(1, 3);
        let at = slide(world(x, z), [3.0, -0.5], &walls);
        assert!(close(at, world(ROOM_HALF_WIDTH - R, z - 0.5)), "{at:?}");

        // A walk in from the corridor's start pose, straight through the doorway and on
        // through a second-row desk set, ends at the back wall.
        let full = plan(vec![room_with(0, 3, &seats)]);
        let start = [layout::doorway_x(0), layout::WIDTH - R];
        let (space, end, crossings) = walk(&full, Space::Corridor, start, [0.0, -14.0], 0.03);
        let [ox, _, oz] = layout::interior_origin(0);
        assert_eq!((space, crossings.len()), (Space::Room(0), 1));
        assert!(
            close(end, [ox + layout::ROOM_DOORWAY_X, oz - depth + R]),
            "{end:?}"
        );
    }

    #[test]
    fn a_room_that_is_not_open_has_no_blocks() {
        let plan = plan(vec![room(0), room(2)]);
        assert!(blocks(&plan, Space::Room(1)).is_empty());
        assert!(!blocks(&plan, Space::Room(2)).is_empty());
    }

    #[test]
    fn a_player_inside_a_box_is_pushed_out_across_its_nearest_side() {
        let block = Block::new([0.0, 2.0], [0.0, 1.0]);
        let at = slide([1.0, 0.9], [0.0, 0.0], &[block]);
        assert!(close(at, [1.0, 1.0 + R]), "{at:?}");
    }

    #[test]
    fn a_desk_set_placed_where_the_player_stands_leaves_them_there() {
        let before = plan(vec![room_with(0, 2, &[(0, 1)])]);
        let after = plan(vec![room_with(0, 2, &[(0, 1), (0, 2)])]);
        let [ox, _, oz] = layout::interior_origin(0);
        let [x, z] = layout::desk_position(0, 2);
        // On the new desk set's seat.
        assert_eq!(
            settle(&before, &after, Space::Room(0), [ox + x, oz + z]),
            None
        );
        // And one going from under them.
        assert_eq!(
            settle(&after, &before, Space::Room(0), [ox + x, oz + z]),
            None
        );
    }

    #[test]
    fn every_spot_on_the_floor_is_clear_of_every_wall() {
        // So free_spot need only find the floor: the floor keeps the player's radius from
        // the walls, in the doorway openings too, with both sides' walls near a doorway.
        // And in a CCTV room widened past twelve rooms.
        let narrow = plan(vec![room(0), room_with(3, 4, &[]), room(1)]);
        let wide = rooms(17);
        let t = |k: i32, a: f32, b: f32| a + (b - a) * k as f32 / 40.0;
        for (plan, space) in [
            (&narrow, Space::Corridor),
            (&narrow, Space::Room(0)),
            (&narrow, Space::Room(3)),
            (&narrow, Space::Room(1)),
            (&narrow, Space::Cctv),
            (&wide, Space::Cctv),
            (&wide, Space::Corridor),
        ] {
            for part in area(plan, space) {
                for i in 0..=40 {
                    for j in 0..=40 {
                        let at = [
                            t(i, part.min[0], part.max[0]),
                            t(j, part.min[1], part.max[1]),
                        ];
                        for wall in blocks_near(plan, space, at) {
                            let push = wall.push_out(at, R - SLACK);
                            assert!(push.is_none(), "{at:?} in {space:?} is in {wall:?}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn a_row_removed_under_the_player_moves_them_onto_the_floor_left() {
        let seats: Vec<_> = (0..9).map(|n| (n / 4, n % 4)).collect();
        let before = plan(vec![room(0), room_with(1, 3, &seats)]);
        let after = plan(vec![room(0), room_with(1, 2, &seats[..8])]);
        let [ox, _, oz] = layout::interior_origin(1);
        // In the third row's aisle, now behind the back wall.
        let aisle = (layout::desk_position(0, 0)[0] + layout::desk_position(0, 1)[0]) / 2.0;
        let at = [ox + aisle, oz + layout::row_z(2) - 1.0];
        let landed = settle(&before, &after, Space::Room(1), at).expect("moved");
        assert_eq!(landed.landing, Landing::FreeSpot);
        assert_eq!(landed.space, Space::Room(1));
        let back = oz - layout::interior_depth(2);
        assert!(close(landed.at, [at[0], back + R]), "{landed:?}");
        assert!(stands_free(&after, Space::Room(1), landed.at));
    }

    #[test]
    fn the_players_room_closing_lands_them_in_front_of_where_its_doorway_was() {
        let before = plan(vec![room(0), room(1), room(2)]);
        let after = plan(vec![room(0), room(2)]);
        let [ox, _, oz] = layout::interior_origin(1);
        let inside = [ox, oz - 2.0];
        let landed = settle(&before, &after, Space::Room(1), inside).expect("landed");
        assert_eq!(
            landed,
            Landed {
                space: Space::Corridor,
                at: [layout::doorway_x(1), layout::WIDTH / 2.0],
                landing: Landing::InFrontOfTheDoorway,
            }
        );
        // Another project's room taking the slot in the same change is not their room.
        let taken = plan(vec![
            room(0),
            Room {
                project: "-new".into(),
                ..room(1)
            },
        ]);
        let landed = settle(&before, &taken, Space::Room(1), inside).expect("landed");
        assert_eq!(landed.space, Space::Corridor);
        assert_eq!(landed.landing, Landing::InFrontOfTheDoorway);
        // While their room stays open, nothing moves them.
        let [ox, _, oz] = layout::interior_origin(2);
        assert_eq!(
            settle(&before, &after, Space::Room(2), [ox, oz - 2.0]),
            None
        );
    }

    #[test]
    fn the_players_room_closing_at_the_far_end_lands_them_at_the_new_end() {
        let before = plan(vec![room(0), room(1), room(2)]);
        let after = plan(vec![room(0), room(1)]);
        let [ox, _, oz] = layout::interior_origin(2);
        let landed = settle(&before, &after, Space::Room(2), [ox, oz - 2.0]).expect("landed");
        let (_, far) = layout::end_walls(&after);
        assert_eq!(landed.space, Space::Corridor);
        assert_eq!(landed.landing, Landing::AtTheNewEnd);
        assert!(
            close(landed.at, [far - R, layout::WIDTH / 2.0]),
            "{landed:?}"
        );
        // The last room closing leaves the plain corridor, with its doorway's place in it.
        let [ox, _, oz] = layout::interior_origin(0);
        let landed = settle(
            &plan(vec![room(0)]),
            &FloorPlan::default(),
            Space::Room(0),
            [ox, oz - 2.0],
        )
        .expect("landed");
        assert_eq!(landed.landing, Landing::InFrontOfTheDoorway);
        assert!(close(
            landed.at,
            [layout::doorway_x(0), layout::WIDTH / 2.0]
        ));
    }

    #[test]
    fn the_corridor_shrinking_past_the_player_pushes_them_back_to_the_new_end() {
        let before = plan(vec![room(0), room(1), room(2), room(3)]);
        let after = plan(vec![room(0), room(1)]);
        let (_, far) = layout::end_walls(&after);
        let at = [layout::doorway_x(3), 0.8];
        assert_eq!(
            settle(&before, &after, Space::Corridor, at),
            Some(Landed {
                space: Space::Corridor,
                at: [far - R, 0.8],
                landing: Landing::AtTheNewEnd,
            })
        );
        // Standing before the new end, nothing moves them.
        assert_eq!(
            settle(&before, &after, Space::Corridor, [far - 1.0, 0.8]),
            None
        );
        // In a doorway that is still open, they stay in it.
        let doorway = [layout::doorway_x(1), -0.05];
        assert_eq!(settle(&before, &after, Space::Corridor, doorway), None);
        // In one whose room closed and whose segment went: at the new end too.
        let doorway = [layout::doorway_x(2), -0.05];
        let landed = settle(&before, &after, Space::Corridor, doorway).expect("pushed");
        assert_eq!(landed.landing, Landing::AtTheNewEnd);
        assert!(
            stands_free(&after, Space::Corridor, landed.at),
            "{landed:?}"
        );
    }

    #[test]
    fn go_to_desk_lands_on_its_rooms_floor() {
        // A full room of three rows: every seat, the wall-side columns too.
        let seats: Vec<_> = (0..12).map(|n| (n / 4, n % 4)).collect();
        let full = room_with(2, layout::rows(seats.len()), &seats);
        let plan = plan(vec![room(0), room(1), full]);
        for &(row, column) in &seats {
            let view = layout::beside_desk(2, layout::desk_position(row, column));
            let at = [view.eye[0], view.eye[2]];
            assert!(
                stands_free(&plan, Space::Room(2), at),
                "seat {row},{column}: the player is off the floor at {at:?}"
            );
        }
    }

    #[test]
    fn clutter_never_blocks() {
        // A room with no desks, so only its walls block; and its clutter, every row's.
        let room = room_with(0, 3, &[]);
        let project = room.project.clone();
        let plan = plan(vec![room]);
        let walls = blocks(&plan, Space::Room(0));
        assert_eq!(walls.len(), 5, "the room's walls and nothing else");
        let [ox, _, oz] = layout::interior_origin(0);
        let pieces: Vec<_> = (0..3)
            .flat_map(|row| {
                clutter::row_clutter(&project, row)
                    .into_iter()
                    .map(move |piece| (row, piece))
            })
            .collect();
        assert!(!pieces.is_empty(), "the room has clutter");
        for (row, piece) in pieces {
            // Walking from the middle of the room onto the piece (short of the wall it
            // may stand against) is not stopped.
            let b = piece.bounds();
            let side = ROOM_HALF_WIDTH - R;
            let target = [
                (ox + (b.min[0] + b.max[0]) / 2.0).clamp(ox - side, ox + side),
                (oz + layout::row_z(row) + (b.min[1] + b.max[1]) / 2.0)
                    .clamp(oz - layout::interior_depth(3) + R, oz - R),
            ];
            let at = [ox, target[1]];
            let along = slide(
                at,
                [target[0] - ox, 0.0],
                &blocks_near(&plan, Space::Room(0), at),
            );
            assert!(close(along, target), "{piece:?} in row {row}: {along:?}");
            assert!(
                stands_free(&plan, Space::Room(0), target),
                "{piece:?} in row {row}"
            );
        }
    }
}
