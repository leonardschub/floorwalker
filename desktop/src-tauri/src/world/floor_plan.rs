//! The floor plan: the world's rules, pure and free of Bevy (spec: "The floor plan",
//! "Layout"). [`next`] turns an agent tree snapshot and the previous plan into the next
//! plan; Bevy only builds what the plan says (`corridor`). The tests below are the seam
//! for the world's rules.
//!
//! A plan holds the corridor's rooms, one per open project in order of opening, and
//! each room's desks, one per running agent of the project's live sessions, packed in
//! rows from the doorway inward, with the seat in line with the doorway left empty so
//! the way in is clear ([`layout::ENTRY_SEAT`]).

use std::collections::HashSet;

use agents::{Agent, AgentState, AgentTreeSnapshot, Flag, Project, TreeStatus};

/// What the world shows, decided from the agent tree.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FloorPlan {
    /// One per open project, in order of opening: room `n` stands behind the corridor's
    /// doorway `n`, counted from the start end.
    pub rooms: Vec<Room>,
}

/// One open project's room.
#[derive(Clone, Debug, PartialEq)]
pub struct Room {
    /// The project's `id` (its `projects/<dir>`): a room is the same room across plans
    /// while its project's id is.
    pub project: String,
    /// The project's name, for the sign over its doorway.
    pub name: String,
    /// Where the room's interior stands in world space, and its render layer
    /// ([`layout::interior_origin`], [`layout::render_layer`]): the lowest slot no other
    /// room holds when the room opens, kept while it stays open, so an interior never
    /// moves while it exists, whatever the rooms before it do.
    pub slot: usize,
    /// How many rows deep the interior is: at least [`layout::MIN_ROWS`], and one more
    /// for each further row of desks.
    pub rows: usize,
    /// One per running agent in the project's live sessions, nearest the doorway first.
    pub desks: Vec<Desk>,
}

/// One agent's desk.
#[derive(Clone, Debug, PartialEq)]
pub struct Desk {
    /// A desk is the same desk across plans while its agent is, wherever it sits.
    pub id: DeskId,
    /// Its row, counted from the doorway, and its column, counted from the left as seen
    /// from the doorway (the doorway's own side).
    pub row: usize,
    pub column: usize,
    /// The agent's depth in its session's tree: the root agent is 0, its subagents 1.
    pub depth: usize,
    /// The agent's model as the tree names it, and its id, each none before it has
    /// answered: the robot's face (`names::Face::of`).
    pub model: Option<String>,
    pub model_id: Option<String>,
    /// A root agent's flag, as the registry gives it, or none: whether it waits on the
    /// user, and since when. Only root agents are flagged, so a subagent's desk never
    /// carries one (its robot's marker, `markers`).
    pub flag: Option<Flag>,
    /// The agent's state as the tree gives it: busy, waiting or idle for a root agent,
    /// running for a subagent (a finished agent has no desk).
    /// With the flag, it is what a punishment keeps or clears against (`cctv_state`):
    /// idle and waiting on a dialog carry the same flag.
    pub state: AgentState,
    /// The session's short id when it is attachable (`claude attach <short id>`), on
    /// its root agent's desk only: the desk whose PC shows the session terminal. None
    /// for a terminal-started session and for every subagent (spec: "The PC screen").
    pub short_id: Option<String>,
}

/// An agent, by its session's id and its own (a root agent's is its session's).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct DeskId {
    pub session: String,
    pub agent: String,
}

/// The next floor plan for `snapshot`, keeping `previous`'s room order.
///
/// A room stands for each **open project**, one with at least one live session; it goes
/// when its project's last live session ends, finished sessions or not. Rooms keep their
/// order from `previous`, the rooms after a closed one close the gap, and newly open
/// projects are appended, ordered by their earliest live session's start, then by
/// project id (also the order of a world's first plan). A *loading* snapshot has no
/// open project.
///
/// Rooms are given [`Room::slot`]s, and each room's desks are packed anew from the tree
/// every time ([`desks`]). The tree holds every session the panel shows, those finished
/// within the last 24 hours too, but only live sessions' running agents get desks: a
/// session's desks go the moment the registry reports it ended.
pub fn next(snapshot: &AgentTreeSnapshot, previous: &FloorPlan) -> FloorPlan {
    if snapshot.status == TreeStatus::Loading {
        return FloorPlan::default();
    }
    let open: Vec<(&Project, i64)> = snapshot
        .projects
        .iter()
        .filter_map(|project| {
            let earliest = project
                .sessions
                .iter()
                .filter(|session| session.live)
                .map(|session| session.started)
                .min()?;
            Some((project, earliest))
        })
        .collect();

    let mut placed = HashSet::new();
    let kept: Vec<(&Project, usize)> = previous
        .rooms
        .iter()
        .filter(|room| placed.insert(room.project.clone()))
        .filter_map(|room| {
            let (project, _) = open
                .iter()
                .find(|(project, _)| project.id == room.project)?;
            Some((*project, room.slot))
        })
        .collect();
    let mut opened: Vec<_> = open
        .iter()
        .filter(|(project, _)| !placed.contains(&project.id))
        .collect();
    opened.sort_by(|(a, a_start), (b, b_start)| a_start.cmp(b_start).then(a.id.cmp(&b.id)));

    let mut slots: HashSet<usize> = kept.iter().map(|&(_, slot)| slot).collect();
    let mut rooms: Vec<Room> = kept
        .into_iter()
        .map(|(project, slot)| room(project, slot))
        .collect();
    for (project, _) in opened {
        if placed.insert(project.id.clone()) {
            let slot = (0..)
                .find(|slot| !slots.contains(slot))
                .expect("a free slot");
            slots.insert(slot);
            rooms.push(room(project, slot));
        }
    }
    FloorPlan { rooms }
}

fn room(project: &Project, slot: usize) -> Room {
    let desks = desks(project);
    Room {
        project: project.id.clone(),
        name: project.name.clone(),
        slot,
        rows: layout::rows(desks.len()),
        desks,
    }
}

/// A room's desks: one per running agent of every live session of `project`, packed
/// [`layout::DESKS_PER_ROW`] to a row from the doorway inward, each row filled from the
/// left, skipping the seat in line with the doorway ([`layout::seat`]). A finished
/// session has no desks, and a subagent's desk goes the moment it finishes. Sessions
/// come oldest first (by their start, then id); within a session the root agent comes
/// first, then its subagents in tree order, each followed by its own. A session does
/// not start a new row: the next one takes the next seat. A robot may change seats when
/// an agent before it appears or goes.
pub fn desks(project: &Project) -> Vec<Desk> {
    let mut sessions: Vec<_> = project
        .sessions
        .iter()
        .filter(|session| session.live)
        .collect();
    sessions.sort_by(|a, b| a.started.cmp(&b.started).then(a.id.cmp(&b.id)));
    let mut desks = Vec::new();
    for session in sessions {
        let short_id = session.short_id.as_deref().filter(|_| session.attachable);
        seat(&session.id, short_id, &session.root, 0, &mut desks);
    }
    desks
}

/// Seats `agent` unless it is finished, then its subagents, which keep their depth
/// either way. An attachable session's `short_id` goes on its root agent's desk.
fn seat(session: &str, short_id: Option<&str>, agent: &Agent, depth: usize, desks: &mut Vec<Desk>) {
    if agent.state != AgentState::Finished {
        let (row, column) = layout::seat(desks.len());
        desks.push(Desk {
            id: DeskId {
                session: session.to_string(),
                agent: agent.id.clone(),
            },
            row,
            column,
            depth,
            model: agent.model.clone(),
            model_id: agent.model_id.clone(),
            flag: agent.flag.filter(|_| depth == 0),
            state: agent.state,
            short_id: short_id.filter(|_| depth == 0).map(str::to_owned),
        });
    }
    for child in &agent.children {
        seat(session, None, child, depth + 1, desks);
    }
}

/// How big the robot of an agent at `depth` is beside a root agent's, for a per-level
/// `factor` (the Subagent size setting, ×0.75 by default): `factor` smaller per level,
/// never below [`layout::MIN_ROBOT_SCALE`]. Only the robot shrinks.
pub fn robot_scale(depth: usize, factor: f32) -> f32 {
    let depth = i32::try_from(depth).unwrap_or(i32::MAX);
    factor.powi(depth).max(layout::MIN_ROBOT_SCALE)
}

/// Where go to desk puts the player (spec: "Go to desk"): the room they land in, by its
/// slot (for its render layer), their eye in world space, and their turn as
/// `look::Look` holds it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DeskView {
    pub slot: usize,
    pub eye: [f32; 3],
    pub yaw: f32,
    pub pitch: f32,
}

/// Go to desk: the player beside agent `id`'s desk, turned to its robot's face and
/// screen ([`layout::beside_desk`]), or none when the agent has no desk (its project is
/// not open, or the agent is gone).
pub fn beside_desk(plan: &FloorPlan, id: &DeskId) -> Option<DeskView> {
    plan.rooms.iter().find_map(|room| {
        let desk = room.desks.iter().find(|desk| &desk.id == id)?;
        let at = layout::desk_position(desk.row, desk.column);
        Some(layout::beside_desk(room.slot, at))
    })
}

/// The world's layout, in metres. The corridor is laid out in the corridor kit's frame
/// (`corridor_kit.py`, as exported), which is world space: X along the corridor from
/// its start end, the doorway wall's corridor face at z = 0 with the rooms behind it
/// towards -Z, the far wall's face at z = 2.4, the floor at y = 0. Each interior is laid
/// out in the room kit's frame (`room_kit.py`): X across the room (inner walls at
/// ±4.75), the doorway wall's inner face at z = 0, the room running towards -Z. It stands
/// elsewhere in world space, at its [`interior_origin`].
pub mod layout {
    use super::FloorPlan;

    /// One doorway's length of corridor: a 1.2 m doorway and its wall.
    pub const SEGMENT: f32 = 2.5;
    /// The corridor's width, from the doorway wall to the far wall.
    pub const WIDTH: f32 = 2.4;
    /// The doorway opening's width and height.
    pub const DOORWAY_WIDTH: f32 = 1.2;
    pub const DOORWAY_HEIGHT: f32 = 2.1;
    /// The doorway opening's left edge in its segment.
    const DOORWAY_LEFT: f32 = 0.65;
    /// The end walls stand this far beyond the outer doorways' edges.
    pub const END_MARGIN: f32 = 1.5;
    /// The player's eye height, and their radius (spec: "The player").
    pub const EYE_HEIGHT: f32 = 1.6;
    pub const PLAYER_RADIUS: f32 = 0.3;

    /// How many segments the corridor has: one per room, and a plain one with no
    /// doorway when no project is open.
    pub fn segments(plan: &FloorPlan) -> usize {
        plan.rooms.len().max(1)
    }

    /// Where segment `n` starts.
    pub fn segment_x(n: usize) -> f32 {
        SEGMENT * n as f32
    }

    /// The middle of doorway `n` (room `n`'s), along the corridor.
    pub fn doorway_x(n: usize) -> f32 {
        segment_x(n) + DOORWAY_LEFT + DOORWAY_WIDTH / 2.0
    }

    /// The corridor's end walls, along the corridor: the start end's, then the far
    /// end's.
    pub fn end_walls(plan: &FloorPlan) -> (f32, f32) {
        let after_last_doorway = SEGMENT - DOORWAY_LEFT - DOORWAY_WIDTH;
        (
            DOORWAY_LEFT - END_MARGIN,
            segment_x(segments(plan)) - after_last_doorway + END_MARGIN,
        )
    }

    /// The middle of the CCTV doorway along the corridor, in its far wall: opposite the
    /// middle of the project doorways (the middle door with an odd count, the wall
    /// between the two middle doors with an even one), and in the middle of the
    /// shortest corridor when no project is open. That is the middle between the end
    /// walls (they stand as far before the first segment as after the last), so it snaps
    /// as the corridor changes.
    pub fn cctv_doorway_x(plan: &FloorPlan) -> f32 {
        segment_x(segments(plan)) / 2.0
    }

    /// One segment's far wall: plain, or open for the CCTV doorway in its middle, at its
    /// end or at its start (each a part of the corridor kit).
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum FarWall {
        Plain,
        Doorway,
        DoorwayEnd,
        DoorwayStart,
    }

    /// Each segment's far wall, in order: the CCTV doorway falls in the middle segment's
    /// middle with an odd count, and across the two middle segments' join with an even one
    /// ([`cctv_doorway_x`]).
    pub fn far_walls(plan: &FloorPlan) -> Vec<FarWall> {
        let n = segments(plan);
        (0..n)
            .map(|k| match (n % 2, 2 * k + 1) {
                (1, at) if at == n => FarWall::Doorway,
                (0, at) if at + 1 == n => FarWall::DoorwayEnd,
                (0, at) if at == n + 1 => FarWall::DoorwayStart,
                _ => FarWall::Plain,
            })
            .collect()
    }

    /// Where a fresh world's player stands: against the far wall, opposite the first
    /// doorway, eye at [`EYE_HEIGHT`]. They face the doorway wall (-Z), which is the
    /// camera's turn at zero yaw.
    pub fn start_eye() -> [f32; 3] {
        [doorway_x(0), EYE_HEIGHT, WIDTH - PLAYER_RADIUS]
    }

    /// Desks to a row, and the fewest rows an interior has.
    pub const DESKS_PER_ROW: usize = 4;
    pub const MIN_ROWS: usize = 2;
    /// Desks stand this far apart across a row, and rows this far apart: one row
    /// segment of the room kit.
    pub const DESK_SPACING: f32 = 2.2;
    pub const ROW_DEPTH: f32 = 2.4;
    /// The strip of room the doorway-wall piece carries before the first row segment.
    pub const DOORWAY_STRIP: f32 = 0.2;
    /// From the doorway wall to the first row of desks (a desk set's origin, under its
    /// seat). The last row then stands 1.0 m before the back wall.
    pub const FIRST_ROW: f32 = 1.6;
    /// No robot shrinks below this, however deep its agent.
    pub const MIN_ROBOT_SCALE: f32 = 0.4;
    /// Where `robot.glb`'s origin goes on a desk set: its `robot_seat` empty.
    pub const ROBOT_SEAT: [f32; 3] = [0.0, 0.46, 0.0];
    /// Interiors stand side by side this far apart, and this far behind the corridor:
    /// far enough that no room's tube lights reach another room or the corridor.
    pub const INTERIOR_SPACING: f32 = 50.0;
    pub const INTERIORS_Z: f32 = -100.0;

    /// The seat left empty: the first row's leftmost, the one in line with the doorway
    /// (x -4.10..-2.90 against the desk set's -3.90..-2.70), so no desk set stands in the
    /// way in ([`entry`]; sweatshop-rooms ticket 26).
    pub const ENTRY_SEAT: (usize, usize) = (0, 0);

    /// The seat of a room's `n`th desk (from 0), as `(row, column)`: rows of
    /// [`DESKS_PER_ROW`] from the doorway inward, each from the left, past
    /// [`ENTRY_SEAT`].
    pub fn seat(n: usize) -> (usize, usize) {
        let (row, column) = ENTRY_SEAT;
        let index = if n >= row * DESKS_PER_ROW + column {
            n + 1
        } else {
            n
        };
        (index / DESKS_PER_ROW, index % DESKS_PER_ROW)
    }

    /// The rows an interior needs for `desks` desks.
    pub fn rows(desks: usize) -> usize {
        (desks + 1).div_ceil(DESKS_PER_ROW).max(MIN_ROWS)
    }

    /// A desk set's footprint from its origin, from the generated `desk_set.glb`: across,
    /// and from the chair's back (-Z) to where the CRT's back was (+Z, towards the
    /// doorway): the PC now ends at 0.73 and the desk at 0.87, so the aisles keep their
    /// width.
    pub const DESK_SET_X: [f32; 2] = [-0.6, 0.6];
    pub const DESK_SET_Z: [f32; 2] = [-0.25, 1.05];

    /// The desk set at seat `(row, column)`'s footprint in its room, as `([x0, x1], [z0,
    /// z1])`.
    pub fn desk_box(row: usize, column: usize) -> ([f32; 2], [f32; 2]) {
        let [x, z] = desk_position(row, column);
        (
            [x + DESK_SET_X[0], x + DESK_SET_X[1]],
            [z + DESK_SET_Z[0], z + DESK_SET_Z[1]],
        )
    }

    /// The way in, which no desk set stands in, as `([x0, x1], [z0, z1])` in the room's
    /// frame: the doorway's width and a player's radius either side, from the doorway
    /// wall to past the first row's chairs by a player's width, into the aisle between
    /// the first two rows, from where every aisle runs.
    pub fn entry() -> ([f32; 2], [f32; 2]) {
        let half = DOORWAY_WIDTH / 2.0 + PLAYER_RADIUS;
        let depth = FIRST_ROW - DESK_SET_Z[0] + 2.0 * PLAYER_RADIUS;
        (
            [ROOM_DOORWAY_X - half, ROOM_DOORWAY_X + half],
            [-depth, 0.0],
        )
    }

    /// How deep an interior of `rows` rows is, from the doorway wall's inner face to the
    /// back wall's.
    pub fn interior_depth(rows: usize) -> f32 {
        DOORWAY_STRIP + ROW_DEPTH * rows as f32
    }

    /// Where row segment `row` starts (its doorway side), along the room.
    pub fn row_z(row: usize) -> f32 {
        -(DOORWAY_STRIP + ROW_DEPTH * row as f32)
    }

    /// A desk set's origin in its room, as `[x, z]`: columns from the left as seen from
    /// the doorway, rows from the doorway inward. Desk sets and robots face the doorway
    /// (+Z) as built, so the PCs' backs face it too.
    pub fn desk_position(row: usize, column: usize) -> [f32; 2] {
        let middle = (DESKS_PER_ROW as f32 - 1.0) / 2.0;
        [
            (column as f32 - middle) * DESK_SPACING,
            -(FIRST_ROW + ROW_DEPTH * row as f32),
        ]
    }

    /// Where the interior in `slot` stands in world space: its room frame's origin.
    pub fn interior_origin(slot: usize) -> [f32; 3] {
        [INTERIOR_SPACING * slot as f32, 0.0, INTERIORS_Z]
    }

    /// Go to desk: where the player stands from a desk set's origin. In the aisle at
    /// the robot's right (-X, as it faces the doorway), 0.8 m beyond the chair's side
    /// (the chair is 0.4 m wide), a little in front of the seat, at eye height. The
    /// player's radius then clears the desk set (x -0.6..0.6) and the next desk's.
    pub const BESIDE_DESK: [f32; 3] = [-1.0, EYE_HEIGHT, 0.4];
    /// Where the player then looks, from the desk set's origin: between a full-size
    /// robot's face (y 1.10, z 0.07) and its PC's glass (centre y 1.035, z 0.605).
    pub const DESK_AIM: [f32; 3] = [0.0, 1.07, 0.34];

    /// Go to desk's pose for the desk set at `desk` (`[x, z]` in its room, as
    /// [`desk_position`] gives it) in the interior in `slot`: the eye at
    /// [`BESIDE_DESK`], turned towards [`DESK_AIM`]. At zero yaw and pitch the player
    /// looks towards -Z; a positive yaw turns left, a positive pitch looks up.
    pub fn beside_desk(slot: usize, desk: [f32; 2]) -> super::DeskView {
        let [ox, oy, oz] = interior_origin(slot);
        let [x, z] = [ox + desk[0], oz + desk[1]];
        let eye = [x + BESIDE_DESK[0], oy + BESIDE_DESK[1], z + BESIDE_DESK[2]];
        let aim = [x + DESK_AIM[0], oy + DESK_AIM[1], z + DESK_AIM[2]];
        let [dx, dy, dz] = [aim[0] - eye[0], aim[1] - eye[1], aim[2] - eye[2]];
        super::DeskView {
            slot,
            eye,
            yaw: (-dx).atan2(-dz),
            pitch: dy.atan2(dx.hypot(dz)),
        }
    }

    /// The middle of a desk set's PC glass from the desk set's origin (`desk_set.py`).
    pub const PC_GLASS: [f32; 3] = [0.0, 1.035, 0.605];
    /// The glass's width and height, and how far it is tilted back from upright, in
    /// radians, to face a full-size robot's eyes (`desk_set.py`'s `TILT`: from the
    /// screen's centre (y 1.03, z 0.60) up to the face (y 1.10, z 0.07)).
    pub const PC_GLASS_SIZE: [f32; 2] = [0.49, 0.294];
    pub const PC_GLASS_TILT: f32 = 0.131_315;
    /// How high a desk set and its seated robot reach, from the floor: the box a
    /// right-click picks with the footprint ([`desk_box`]). The PC's top is at 1.20 and
    /// a full-size robot's head a little above.
    pub const DESK_SET_HEIGHT: [f32; 2] = [0.0, 1.35];

    /// The middle of the PC glass of the desk set at seat `(row, column)` in the
    /// interior in `slot`, in world space.
    pub fn pc_glass(slot: usize, row: usize, column: usize) -> [f32; 3] {
        let [ox, oy, oz] = interior_origin(slot);
        let [x, z] = desk_position(row, column);
        [ox + x + PC_GLASS[0], oy + PC_GLASS[1], oz + z + PC_GLASS[2]]
    }

    /// The render layer of the interior in `slot`, for its meshes and its lights; the
    /// corridor is on layer 0.
    pub fn render_layer(slot: usize) -> usize {
        slot + 1
    }

    /// The seat layer of the interior in `slot`: what sitting in hides from the
    /// player's own camera goes there (the seated robot, its marker, its wreck), and
    /// every other camera that sees the room sees it too (`seat`).
    pub fn seat_layer(slot: usize) -> usize {
        SEAT_LAYERS + slot
    }

    /// The first seat layer: clear of the rooms' (1 up by slot), the CCTV room's (999),
    /// the lettering's (1000) and the presenter's (1001).
    const SEAT_LAYERS: usize = 1100;

    /// The layers a camera that sees the interior in `slot` sees, besides the main
    /// camera, which sees only the room's own: the room's, and its seat layer.
    pub fn seen_layers(slot: usize) -> [usize; 2] {
        [render_layer(slot), seat_layer(slot)]
    }

    /// The doorway walls' thickness: the corridor's spans z -0.2..0, a room's z 0..0.2 in
    /// the room's frame.
    pub const WALL: f32 = 0.2;
    /// The middle of a room's doorway opening (x -4.10..-2.90) across its doorway wall, in
    /// the room's frame.
    pub const ROOM_DOORWAY_X: f32 = -3.5;

    /// The middle of doorway `n`'s opening at the corridor wall's room side, where the
    /// corridor shows its room (facing +Z).
    pub fn corridor_doorway(n: usize) -> [f32; 3] {
        [doorway_x(n), DOORWAY_HEIGHT / 2.0, -WALL]
    }

    /// The middle of the room in `slot`'s doorway opening at its wall's corridor side,
    /// where the room shows the corridor (facing -Z), in world space.
    pub fn room_doorway(slot: usize) -> [f32; 3] {
        let [x, y, z] = interior_origin(slot);
        [x + ROOM_DOORWAY_X, y + DOORWAY_HEIGHT / 2.0, z + WALL]
    }

    /// What carries corridor space through doorway `n` into the room in `slot`: a pure
    /// translation, so the corridor's doorway wall lands on the room's. Add it to go in,
    /// subtract it to come out; a turn is the same on both sides.
    pub fn through_doorway(slot: usize, n: usize) -> [f32; 3] {
        let [x, y, z] = interior_origin(slot);
        [x + ROOM_DOORWAY_X - doorway_x(n), y, z + WALL]
    }

    /// A project room's wall camera: where its head is
    /// turned and its feed camera looks from, in the room's frame. At zero yaw and pitch it
    /// looks towards -Z, into the room; a positive yaw turns left, a positive pitch up,
    /// as the player's look.
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct WallCamera {
        /// The head's ball joint, which it turns about.
        pub joint: [f32; 3],
        /// The front of its lens, where the feed camera stands.
        pub lens: [f32; 3],
        pub yaw: f32,
        pub pitch: f32,
        /// The feed's vertical field of view, in radians, for a 16:9 image.
        pub fov: f32,
    }

    /// The wall camera's mount on the doorway wall's inner face, over the doorway and
    /// under the ceiling (`wall_camera.py`), its head's joint from the mount, and its
    /// lens's front from the joint, in the head's frame.
    pub const WALL_CAMERA_MOUNT: [f32; 3] = [ROOM_DOORWAY_X, 2.55, 0.0];
    const WALL_CAMERA_JOINT: [f32; 3] = [0.0, -0.05, -0.17];
    const WALL_CAMERA_LENS: [f32; 3] = [0.0, -0.06, -0.235];
    /// The feed's frame round what it must show, as a share of the field of view.
    const WALL_CAMERA_MARGIN: f32 = 1.08;

    /// The wall camera's framing of a room `rows` deep: turned to the middle of what it
    /// must show (every seat's robot face and its desk's front), with a lens just wide
    /// enough for all of it in a 16:9 feed. A room that grows or shrinks gets a new one.
    pub fn wall_camera(rows: usize) -> WallCamera {
        let [mx, my, mz] = WALL_CAMERA_MOUNT;
        let [jx, jy, jz] = WALL_CAMERA_JOINT;
        let joint = [mx + jx, my + jy, mz + jz];
        let points: Vec<[f32; 3]> = (0..rows * DESKS_PER_ROW - 1)
            .flat_map(|n| {
                let (row, column) = seat(n);
                let [x, z] = desk_position(row, column);
                [[x, 1.10, z + 0.07], [x, 0.5, z + DESK_SET_Z[1]]]
            })
            .collect();
        let from = |at: [f32; 3], p: [f32; 3]| [p[0] - at[0], p[1] - at[1], p[2] - at[2]];
        let middle = |angles: Vec<f32>| {
            let (lo, hi) = angles
                .iter()
                .fold((f32::MAX, f32::MIN), |(lo, hi), &a| (lo.min(a), hi.max(a)));
            (lo + hi) / 2.0
        };
        let yaw = middle(
            points
                .iter()
                .map(|&p| {
                    let [x, _, z] = from(joint, p);
                    (-x).atan2(-z)
                })
                .collect(),
        );
        let pitch = middle(
            points
                .iter()
                .map(|&p| {
                    let [_, y, z] = turned(-yaw, 0.0, from(joint, p));
                    y.atan2(-z)
                })
                .collect(),
        );
        let [lx, ly, lz] = turn(yaw, pitch, WALL_CAMERA_LENS);
        let lens = [joint[0] + lx, joint[1] + ly, joint[2] + lz];
        let aspect = 16.0 / 9.0;
        let half = points.iter().fold(0.0_f32, |half, &p| {
            let [x, y, z] = turned(-yaw, -pitch, from(lens, p));
            half.max(y.abs() / -z).max(x.abs() / -z / aspect)
        });
        WallCamera {
            joint,
            lens,
            yaw,
            pitch,
            fov: 2.0 * (half * WALL_CAMERA_MARGIN).atan(),
        }
    }

    /// `v` turned by `pitch` about X, then by `yaw` about Y.
    fn turn(yaw: f32, pitch: f32, [x, y, z]: [f32; 3]) -> [f32; 3] {
        let (y, z) = (
            y * pitch.cos() - z * pitch.sin(),
            y * pitch.sin() + z * pitch.cos(),
        );
        [
            x * yaw.cos() + z * yaw.sin(),
            y,
            -x * yaw.sin() + z * yaw.cos(),
        ]
    }

    /// `v` turned back: by `yaw` about Y, then by `pitch` about X (pass both negated to
    /// undo [`turn`]).
    fn turned(yaw: f32, pitch: f32, [x, y, z]: [f32; 3]) -> [f32; 3] {
        let (x, z) = (
            x * yaw.cos() + z * yaw.sin(),
            -x * yaw.sin() + z * yaw.cos(),
        );
        [
            x,
            y * pitch.cos() - z * pitch.sin(),
            y * pitch.sin() + z * pitch.cos(),
        ]
    }

    /// The CCTV room, laid out in its kit's frame
    /// (`cctv_room_kit.py`): X across, centred on its doorway, the doorway wall's inner
    /// face at z = 0 and the room towards -Z. Its doorway is in the corridor's far wall,
    /// so the room stands beyond it turned half round ([`cctv_to_world`]): going in, the
    /// player keeps walking towards +Z, and the doorway stays a pure translation like
    /// the project doorways. It stands on the corridor's +Z side, as far out as the
    /// interiors stand on the other; later management rooms would stand beside it.
    pub const CCTV_ORIGIN: [f32; 3] = [0.0, 0.0, -INTERIORS_Z];
    /// The CCTV room's render layer, for its meshes and its lights: clear of the rooms'
    /// (1 up by slot) and of the lettering's (1000).
    pub const CCTV_LAYER: usize = 999;
    /// The CCTV room's inside up to twelve open projects: 8 m wide (inner side walls at
    /// ±4) and 7 m deep (and 4.2 m high). Past twelve it widens
    /// (`cctv_geometry::room_half_width`); depth and height never change.
    pub const CCTV_HALF_WIDTH: f32 = 4.0;
    pub const CCTV_DEPTH: f32 = 7.0;
    /// The console (`cctv_console.py`): its origin on the floor under its centre, 2.6 m
    /// out from the monitor wall, and its slab's half size across and along the room.
    pub const CONSOLE_Z: f32 = -(CCTV_DEPTH - 2.6);
    pub const CONSOLE_HALF: [f32; 2] = [1.3, 0.4];

    /// A point of the CCTV room's frame in world space.
    pub fn cctv_to_world([x, y, z]: [f32; 3]) -> [f32; 3] {
        let [ox, oy, oz] = CCTV_ORIGIN;
        [ox - x, oy + y, oz - z]
    }

    /// The middle of the CCTV doorway's opening at the far wall's outer side (z 2.6),
    /// where the corridor shows the CCTV room (facing -Z).
    pub fn cctv_corridor_doorway(plan: &FloorPlan) -> [f32; 3] {
        [cctv_doorway_x(plan), DOORWAY_HEIGHT / 2.0, WIDTH + WALL]
    }

    /// The middle of the CCTV room's doorway opening at its wall's corridor side, where
    /// the room shows the corridor (facing +Z), in world space. It never moves.
    pub fn cctv_room_doorway() -> [f32; 3] {
        cctv_to_world([0.0, DOORWAY_HEIGHT / 2.0, WALL])
    }

    /// What carries corridor space through the CCTV doorway into the CCTV room: a pure
    /// translation, so the far wall lands on the room's doorway wall. Add it to go in,
    /// subtract it to come out.
    pub fn through_cctv_doorway(plan: &FloorPlan) -> [f32; 3] {
        let [x, y, z] = cctv_room_doorway();
        [
            x - cctv_doorway_x(plan),
            y - DOORWAY_HEIGHT / 2.0,
            z - WIDTH,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agents::{Agent, AgentKind, AgentState, Flag, FlagKind, Session};

    fn session(id: &str, live: bool, started: i64) -> Session {
        Session {
            id: id.into(),
            title: None,
            live,
            attachable: false,
            short_id: None,
            started,
            last_activity: started + 60_000,
            root: Agent {
                id: id.into(),
                kind: AgentKind::Root,
                name: String::new(),
                model: Some("Opus 5.5".into()),
                model_id: Some("claude-opus-5-5".into()),
                state: if live {
                    AgentState::Busy
                } else {
                    AgentState::Finished
                },
                current_tool: None,
                tokens: 0,
                flag: None,
                children: Vec::new(),
            },
        }
    }

    fn project(id: &str, sessions: Vec<Session>) -> Project {
        Project {
            id: id.into(),
            name: id.trim_start_matches('-').into(),
            sessions,
        }
    }

    fn ready(projects: Vec<Project>) -> AgentTreeSnapshot {
        AgentTreeSnapshot {
            status: TreeStatus::Ready,
            projects,
        }
    }

    /// A project with one live session that started at `started`.
    fn open(id: &str, started: i64) -> Project {
        project(id, vec![session(&format!("{id}-live"), true, started)])
    }

    fn rooms(plan: &FloorPlan) -> Vec<&str> {
        plan.rooms
            .iter()
            .map(|room| room.project.as_str())
            .collect()
    }

    fn plan_of(ids: &[&str]) -> FloorPlan {
        FloorPlan {
            rooms: ids
                .iter()
                .enumerate()
                .map(|(slot, id)| Room {
                    project: id.to_string(),
                    name: id.trim_start_matches('-').to_string(),
                    slot,
                    rows: layout::MIN_ROWS,
                    desks: Vec::new(),
                })
                .collect(),
        }
    }

    /// A subagent with these children.
    fn sub(id: &str, state: AgentState, children: Vec<Agent>) -> Agent {
        Agent {
            id: id.into(),
            kind: AgentKind::Subagent,
            name: "Explore".into(),
            model: Some("Haiku 5.5".into()),
            model_id: Some("claude-haiku-5-5".into()),
            state,
            current_tool: None,
            tokens: 0,
            flag: None,
            children,
        }
    }

    fn running(id: &str) -> Agent {
        sub(id, AgentState::Running, Vec::new())
    }

    /// A live session whose root agent has these subagents.
    fn with_subagents(id: &str, started: i64, children: Vec<Agent>) -> Session {
        let mut session = session(id, true, started);
        session.root.children = children;
        session
    }

    /// Each desk as `(session, agent, row, column)`.
    fn seats(desks: &[Desk]) -> Vec<(&str, &str, usize, usize)> {
        desks
            .iter()
            .map(|desk| {
                (
                    desk.id.session.as_str(),
                    desk.id.agent.as_str(),
                    desk.row,
                    desk.column,
                )
            })
            .collect()
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-5
    }

    #[test]
    fn only_open_projects_get_rooms() {
        let tree = ready(vec![
            open("-a", 10),
            project("-b", vec![session("b-done", false, 5)]),
            project(
                "-c",
                vec![session("c-done", false, 1), session("c-live", true, 30)],
            ),
        ]);
        assert_eq!(rooms(&next(&tree, &FloorPlan::default())), ["-a", "-c"]);
    }

    #[test]
    fn a_project_whose_last_live_session_ended_loses_its_room() {
        let before = next(
            &ready(vec![project(
                "-a",
                vec![session("a-1", true, 10), session("a-2", true, 20)],
            )]),
            &FloorPlan::default(),
        );
        assert_eq!(rooms(&before), ["-a"]);

        let one_ended = ready(vec![project(
            "-a",
            vec![session("a-1", false, 10), session("a-2", true, 20)],
        )]);
        assert_eq!(
            rooms(&next(&one_ended, &before)),
            ["-a"],
            "one session still live"
        );

        // Both finished within the last 24 hours: still in the tree, but no room.
        let all_ended = ready(vec![project(
            "-a",
            vec![session("a-1", false, 10), session("a-2", false, 20)],
        )]);
        assert!(next(&all_ended, &before).rooms.is_empty());
    }

    #[test]
    fn a_finished_session_gets_no_desks() {
        // Finished within the last 24 hours: still in the tree (the panel lists it),
        // but only live sessions sit in the room.
        let tree = ready(vec![project(
            "-a",
            vec![session("done", false, 1), session("live", true, 2)],
        )]);
        let plan = next(&tree, &FloorPlan::default());
        assert_eq!(seats(&plan.rooms[0].desks), [("live", "live", 0, 1)]);
    }

    #[test]
    fn a_session_ending_takes_its_desks_and_the_rest_repack() {
        let tree = |middle_live: bool| {
            let mut middle = with_subagents("middle", 2, vec![running("helper")]);
            if !middle_live {
                middle.live = false;
                middle.root.state = AgentState::Finished;
                middle.root.children[0].state = AgentState::Finished;
            }
            ready(vec![project(
                "-a",
                vec![
                    session("first", true, 1),
                    middle,
                    with_subagents("last", 3, vec![running("worker")]),
                ],
            )])
        };
        let before = next(&tree(true), &FloorPlan::default());
        assert_eq!(
            seats(&before.rooms[0].desks),
            [
                ("first", "first", 0, 1),
                ("middle", "middle", 0, 2),
                ("middle", "helper", 0, 3),
                ("last", "last", 1, 0),
                ("last", "worker", 1, 1),
            ]
        );

        let after = next(&tree(false), &before);
        assert_eq!(
            seats(&after.rooms[0].desks),
            [
                ("first", "first", 0, 1),
                ("last", "last", 0, 2),
                ("last", "worker", 0, 3),
            ]
        );
        assert_eq!(after.rooms[0].slot, before.rooms[0].slot);
        assert_eq!(after.rooms[0].rows, layout::MIN_ROWS);
    }

    #[test]
    fn a_subagent_loses_its_desk_the_moment_it_finishes() {
        let tree = |done: AgentState| {
            ready(vec![project(
                "-a",
                vec![with_subagents(
                    "s",
                    1,
                    vec![
                        sub("a", done, Vec::new()),
                        sub("b", AgentState::Running, vec![running("b1")]),
                    ],
                )],
            )])
        };
        let before = next(&tree(AgentState::Running), &FloorPlan::default());
        assert_eq!(
            seats(&before.rooms[0].desks),
            [
                ("s", "s", 0, 1),
                ("s", "a", 0, 2),
                ("s", "b", 0, 3),
                ("s", "b1", 1, 0)
            ]
        );

        let after = next(&tree(AgentState::Finished), &before);
        assert_eq!(
            seats(&after.rooms[0].desks),
            [("s", "s", 0, 1), ("s", "b", 0, 2), ("s", "b1", 0, 3)]
        );
    }

    #[test]
    fn a_running_subagent_under_a_finished_one_keeps_its_desk_and_depth() {
        // Not expected of Claude Code, but a running agent always sits at a desk.
        let tree = ready(vec![project(
            "-a",
            vec![with_subagents(
                "s",
                1,
                vec![sub("done", AgentState::Finished, vec![running("still")])],
            )],
        )]);
        let plan = next(&tree, &FloorPlan::default());
        let desks: Vec<_> = plan.rooms[0]
            .desks
            .iter()
            .map(|d| (d.id.agent.as_str(), d.depth))
            .collect();
        assert_eq!(desks, [("s", 0), ("still", 2)]);
    }

    #[test]
    fn a_first_plan_is_ordered_by_earliest_live_session_start_then_project_id() {
        let tree = ready(vec![
            // The tree lists projects by recent activity, which the plan ignores.
            open("-late", 300),
            project(
                "-early",
                vec![
                    // A finished session's earlier start does not count.
                    session("early-done", false, 1),
                    session("early-live", true, 100),
                    session("early-later", true, 400),
                ],
            ),
            open("-tied-b", 200),
            open("-tied-a", 200),
        ]);
        assert_eq!(
            rooms(&next(&tree, &FloorPlan::default())),
            ["-early", "-tied-a", "-tied-b", "-late"]
        );
    }

    #[test]
    fn room_order_is_stable_across_snapshots() {
        // The previous plan's order wins over start times and the tree's own order.
        let previous = plan_of(&["-b", "-a", "-c"]);
        let tree = ready(vec![open("-c", 1), open("-a", 2), open("-b", 3)]);
        assert_eq!(rooms(&next(&tree, &previous)), ["-b", "-a", "-c"]);
    }

    #[test]
    fn newly_open_projects_are_appended_in_start_order() {
        let previous = plan_of(&["-m"]);
        let tree = ready(vec![open("-z", 50), open("-m", 900), open("-a", 60)]);
        assert_eq!(rooms(&next(&tree, &previous)), ["-m", "-z", "-a"]);
    }

    #[test]
    fn a_closed_rooms_successors_close_the_gap() {
        let previous = plan_of(&["-a", "-b", "-c"]);
        let tree = ready(vec![
            project("-a", vec![session("a", false, 1)]),
            open("-b", 2),
            open("-c", 3),
        ]);
        let plan = next(&tree, &previous);
        assert_eq!(rooms(&plan), ["-b", "-c"]);

        // A project that opens again is a new room, at the end.
        let reopened = ready(vec![open("-a", 10), open("-b", 2), open("-c", 3)]);
        assert_eq!(rooms(&next(&reopened, &plan)), ["-b", "-c", "-a"]);
    }

    #[test]
    fn a_room_is_identified_by_its_project_id() {
        let previous = plan_of(&["-a", "-b"]);
        // The same project under a new name, with entirely new sessions, keeps its
        // place; a new project with an old project's name is a new room.
        let mut renamed = open("-a", 500);
        renamed.name = "elsewhere".into();
        let mut namesake = open("-c", 1);
        namesake.name = "a".into();
        let tree = ready(vec![namesake, open("-b", 400), renamed]);
        assert_eq!(rooms(&next(&tree, &previous)), ["-a", "-b", "-c"]);
    }

    #[test]
    fn an_empty_or_loading_snapshot_gives_an_empty_corridor() {
        let previous = plan_of(&["-a", "-b"]);
        assert!(next(&ready(Vec::new()), &previous).rooms.is_empty());
        assert!(next(&AgentTreeSnapshot::loading(), &previous)
            .rooms
            .is_empty());
        assert!(next(&AgentTreeSnapshot::loading(), &FloorPlan::default())
            .rooms
            .is_empty());
    }

    #[test]
    fn doorways_stand_two_and_a_half_metres_apart_with_end_walls_beyond_them() {
        let three = plan_of(&["-a", "-b", "-c"]);
        assert_eq!(layout::segments(&three), 3);
        assert_eq!(layout::doorway_x(1) - layout::doorway_x(0), 2.5);
        let (start, far) = layout::end_walls(&three);
        let half = layout::DOORWAY_WIDTH / 2.0;
        assert!((layout::doorway_x(0) - half - start - 1.5).abs() < 1e-5);
        assert!((far - (layout::doorway_x(2) + half) - 1.5).abs() < 1e-5);
    }

    #[test]
    fn with_no_project_open_the_corridor_is_one_doorways_length_between_its_end_margins() {
        let empty = FloorPlan::default();
        assert_eq!(layout::segments(&empty), 1);
        // The end pieces carry 0.85 m of corridor each (1.5 m from where a doorway's
        // edges would be); between them stands the one plain 2.5 m segment.
        let (start, far) = layout::end_walls(&empty);
        assert!((start - -0.85).abs() < 1e-5, "start wall at {start}");
        assert!((far - (2.5 + 0.85)).abs() < 1e-5, "far wall at {far}");
    }

    #[test]
    fn a_fresh_world_starts_against_the_far_wall_opposite_the_first_doorway() {
        let [x, y, z] = layout::start_eye();
        assert_eq!(x, layout::doorway_x(0));
        assert_eq!(y, 1.6);
        assert!((z - (layout::WIDTH - layout::PLAYER_RADIUS)).abs() < 1e-6);
    }

    #[test]
    fn desks_are_packed_four_to_a_row_from_the_doorway_each_row_from_the_left_past_the_entry_seat()
    {
        let tree = ready(vec![project(
            "-a",
            vec![with_subagents(
                "s",
                10,
                ["1", "2", "3", "4", "5"].map(running).to_vec(),
            )],
        )]);
        let plan = next(&tree, &FloorPlan::default());
        assert_eq!(
            seats(&plan.rooms[0].desks),
            [
                // (0, 0), in line with the doorway, stays empty.
                ("s", "s", 0, 1),
                ("s", "1", 0, 2),
                ("s", "2", 0, 3),
                ("s", "3", 1, 0),
                ("s", "4", 1, 1),
                ("s", "5", 1, 2),
            ]
        );
    }

    #[test]
    fn sessions_come_oldest_first_root_then_subagents_in_tree_order_with_no_new_row() {
        let older = with_subagents(
            "old",
            100,
            vec![
                sub("x", AgentState::Running, vec![running("x1")]),
                running("y"),
            ],
        );
        let newer = with_subagents("new", 200, vec![running("z")]);
        let first = with_subagents("first", 50, Vec::new());
        // The tree lists sessions by recent activity: not the order.
        let tree = ready(vec![project("-a", vec![newer, older, first])]);
        let plan = next(&tree, &FloorPlan::default());
        assert_eq!(
            seats(&plan.rooms[0].desks),
            [
                ("first", "first", 0, 1),
                ("old", "old", 0, 2),
                ("old", "x", 0, 3),
                ("old", "x1", 1, 0),
                ("old", "y", 1, 1),
                // The next session takes the next seat, mid-row.
                ("new", "new", 1, 2),
                ("new", "z", 1, 3),
            ]
        );
    }

    #[test]
    fn a_desk_is_identified_by_its_session_and_agent_ids() {
        // Two sessions whose subagents share an id: two desks.
        let tree = ready(vec![project(
            "-a",
            vec![
                with_subagents("s1", 1, vec![running("agent")]),
                with_subagents("s2", 2, vec![running("agent")]),
            ],
        )]);
        let before = next(&tree, &FloorPlan::default());
        let ids: Vec<_> = before.rooms[0].desks.iter().map(|d| &d.id).collect();
        let s2 = DeskId {
            session: "s2".into(),
            agent: "agent".into(),
        };
        assert_eq!(ids.iter().filter(|id| id.agent == "agent").count(), 2);
        assert_eq!(*ids[3], s2);

        // When the first session goes, its desks go and the rest move up a seat each,
        // keeping their identity.
        let later = ready(vec![project(
            "-a",
            vec![with_subagents("s2", 2, vec![running("agent")])],
        )]);
        let after = next(&later, &before);
        let moved = after.rooms[0].desks.iter().find(|d| d.id == s2).unwrap();
        assert_eq!((moved.row, moved.column), (0, 2));
        assert_eq!(after.rooms[0].desks.len(), 2);
    }

    #[test]
    fn a_desks_depth_comes_from_its_agents_nesting() {
        let tree = ready(vec![project(
            "-a",
            vec![with_subagents(
                "s",
                1,
                vec![
                    sub(
                        "a",
                        AgentState::Running,
                        vec![sub("a1", AgentState::Running, vec![running("a1x")])],
                    ),
                    running("b"),
                ],
            )],
        )]);
        let plan = next(&tree, &FloorPlan::default());
        let depths: Vec<_> = plan.rooms[0]
            .desks
            .iter()
            .map(|d| (d.id.agent.as_str(), d.depth))
            .collect();
        assert_eq!(
            depths,
            [("s", 0), ("a", 1), ("a1", 2), ("a1x", 3), ("b", 1)]
        );
    }

    #[test]
    fn a_robot_is_the_factor_smaller_per_level_never_below_the_floor() {
        // The default ×0.75: the spec's ×0.75 at depth 1 and ×0.5625 at depth 2.
        let scales: Vec<f32> = (0..6).map(|depth| robot_scale(depth, 0.75)).collect();
        for (have, want) in scales.iter().zip([1.0, 0.75, 0.5625, 0.421875, 0.4, 0.4]) {
            assert!(close(*have, want), "{scales:?}");
        }
        // The slider's ends, and one between.
        assert!(close(robot_scale(1, 0.5), 0.5));
        assert!(close(robot_scale(2, 0.5), 0.4), "0.25 is floored");
        assert!(close(robot_scale(2, 0.6), 0.4), "0.36 is floored");
        assert!(close(robot_scale(1, 0.6), 0.6));
        assert!(close(robot_scale(9, 1.0), 1.0));
        assert!(close(robot_scale(0, 0.5), 1.0), "a root is full size");
        assert!(close(robot_scale(usize::MAX, 0.75), 0.4));
    }

    #[test]
    fn a_desk_carries_its_agents_model_and_its_id() {
        let mut fresh = running("fresh");
        fresh.model = None;
        fresh.model_id = None;
        let tree = ready(vec![project(
            "-a",
            vec![with_subagents("s", 1, vec![fresh])],
        )]);
        let plan = next(&tree, &FloorPlan::default());
        let models: Vec<_> = plan.rooms[0]
            .desks
            .iter()
            .map(|d| (d.model.as_deref(), d.model_id.as_deref()))
            .collect();
        assert_eq!(
            models,
            [(Some("Opus 5.5"), Some("claude-opus-5-5")), (None, None)]
        );
    }

    #[test]
    fn each_root_desk_carries_its_agents_flag_or_none_and_no_subagent_desk_does() {
        let asking = Flag {
            kind: FlagKind::AskingYouSomething,
            since: 41_000,
        };
        let waiting = Flag {
            kind: FlagKind::WaitingOnYou,
            since: 7_000,
        };
        let mut held = running("held");
        // The registry never flags a subagent; were one flagged, its desk still is not.
        held.flag = Some(asking);
        let mut first = with_subagents("first", 1, vec![held]);
        first.root.state = AgentState::Waiting;
        first.root.flag = Some(asking);
        let mut second = session("second", true, 2);
        second.root.state = AgentState::Idle;
        second.root.flag = Some(waiting);
        let busy = session("busy", true, 3);
        let tree = ready(vec![project("-a", vec![first, second, busy])]);
        let plan = next(&tree, &FloorPlan::default());
        let flags: Vec<_> = plan.rooms[0]
            .desks
            .iter()
            .map(|d| (d.id.agent.as_str(), d.flag))
            .collect();
        assert_eq!(
            flags,
            [
                ("first", Some(asking)),
                ("held", None),
                ("second", Some(waiting)),
                ("busy", None)
            ]
        );
    }

    #[test]
    fn only_an_attachable_sessions_root_desk_carries_its_short_id() {
        let mut background = with_subagents("background", 1, vec![running("helper")]);
        background.attachable = true;
        background.short_id = Some("bg01".into());
        let terminal = session("terminal", true, 2);
        let tree = ready(vec![project("-a", vec![background, terminal])]);
        let plan = next(&tree, &FloorPlan::default());
        let short_ids: Vec<_> = plan.rooms[0]
            .desks
            .iter()
            .map(|d| (d.id.agent.as_str(), d.short_id.as_deref()))
            .collect();
        assert_eq!(
            short_ids,
            [
                ("background", Some("bg01")),
                ("helper", None),
                ("terminal", None)
            ]
        );
    }

    #[test]
    fn each_desk_carries_its_agents_state_so_a_punishment_can_tell_idle_from_a_dialog() {
        // Idle and waiting on a dialog give the same flag (*waiting on you*, since the
        // last activity); only the state tells them apart (spec: "Keep or clear").
        let flag = Flag {
            kind: FlagKind::WaitingOnYou,
            since: 7_000,
        };
        let mut dialog = with_subagents("dialog", 1, vec![running("sub")]);
        dialog.root.state = AgentState::Waiting;
        dialog.root.flag = Some(flag);
        let mut idle = session("idle", true, 2);
        idle.root.state = AgentState::Idle;
        idle.root.flag = Some(flag);
        let tree = ready(vec![project(
            "-a",
            vec![dialog, idle, session("busy", true, 3)],
        )]);
        let plan = next(&tree, &FloorPlan::default());
        let states: Vec<_> = plan.rooms[0]
            .desks
            .iter()
            .map(|d| (d.id.agent.as_str(), d.state))
            .collect();
        assert_eq!(
            states,
            [
                ("dialog", AgentState::Waiting),
                ("sub", AgentState::Running),
                ("idle", AgentState::Idle),
                ("busy", AgentState::Busy)
            ]
        );
    }

    #[test]
    fn a_room_carries_its_projects_name_for_its_sign() {
        let mut alpha = open("-home-dev-Projects-Alpha", 1);
        alpha.name = "Alpha".into();
        let plan = next(&ready(vec![alpha]), &FloorPlan::default());
        assert_eq!(plan.rooms[0].name, "Alpha");
    }

    #[test]
    fn an_interior_is_at_least_two_rows_deep_and_one_row_deeper_per_further_row() {
        // The first row holds three: its seat in line with the doorway stays empty.
        let rows: Vec<_> = [1, 3, 4, 7, 8, 11, 12, 15, 16, 40]
            .map(layout::rows)
            .to_vec();
        assert_eq!(rows, [2, 2, 2, 2, 3, 3, 4, 4, 5, 11]);

        // 16 desks, the 16th alone in a fifth row.
        let tree = ready(vec![project(
            "-a",
            vec![with_subagents(
                "s",
                1,
                (1..16).map(|n| running(&n.to_string())).collect(),
            )],
        )]);
        let room = &next(&tree, &FloorPlan::default()).rooms[0];
        assert_eq!((room.desks.len(), room.rows), (16, 5));
        assert_eq!((room.desks[15].row, room.desks[15].column), (4, 0));

        // The doorway wall's 0.2 m strip and 2.4 m per row: 1.6 m from the doorway wall
        // to the first row, 1.0 m from the last row to the back wall.
        for rows in [2, 5] {
            let depth = layout::interior_depth(rows);
            assert!(close(depth, 0.2 + 2.4 * rows as f32));
            let [_, last] = layout::desk_position(rows - 1, 0);
            assert!(close(depth + last, 1.0), "{rows} rows");
            assert!(close(layout::row_z(rows), -depth), "the back wall's place");
        }
    }

    /// A room of project `-a` with `n` desks: one session's root and its subagents.
    fn room_of(n: usize) -> Room {
        let children = (1..n).map(|k| running(&k.to_string())).collect();
        let tree = ready(vec![project("-a", vec![with_subagents("s", 1, children)])]);
        next(&tree, &FloorPlan::default()).rooms.remove(0)
    }

    /// Whether two boxes, `([x0, x1], [z0, z1])`, overlap by more than a touch.
    fn overlap(a: ([f32; 2], [f32; 2]), b: ([f32; 2], [f32; 2])) -> bool {
        let apart = |a: [f32; 2], b: [f32; 2]| a[1] <= b[0] + 1e-5 || b[1] <= a[0] + 1e-5;
        !(apart(a.0, b.0) || apart(a.1, b.1))
    }

    #[test]
    fn no_desk_set_stands_in_the_way_in_from_the_doorway_however_many_desks() {
        // The way in: the doorway's opening (x -4.10..-2.90) and a player's radius either
        // side, from the doorway wall's inner face (z 0) to a player's width past the
        // first row's chair backs, into the aisle between the first two rows.
        let way_in = layout::entry();
        let half = layout::DOORWAY_WIDTH / 2.0 + layout::PLAYER_RADIUS;
        assert!(close(way_in.0[0], -3.5 - half) && close(way_in.0[1], -3.5 + half));
        let chairs = layout::desk_position(0, 1)[1] + layout::DESK_SET_Z[0];
        assert!(close(way_in.1[1], 0.0));
        assert!(
            close(way_in.1[0], chairs - 2.0 * layout::PLAYER_RADIUS),
            "{way_in:?}"
        );
        // And it ends short of the second row's desk sets: the aisle is there to turn into.
        let second = layout::desk_position(1, 0)[1] + layout::DESK_SET_Z[1];
        assert!(way_in.1[0] > second, "{way_in:?} reaches row 1 at {second}");

        for n in 1..=48 {
            let room = room_of(n);
            assert_eq!(room.desks.len(), n);
            let mut seats = HashSet::new();
            for desk in &room.desks {
                assert!(
                    seats.insert((desk.row, desk.column)),
                    "{n} desks share a seat"
                );
                assert!(desk.row < room.rows && desk.column < layout::DESKS_PER_ROW);
                let footprint = layout::desk_box(desk.row, desk.column);
                assert!(
                    !overlap(footprint, way_in),
                    "{n} desks: ({}, {}) at {footprint:?} stands in the way in",
                    desk.row,
                    desk.column
                );
            }
        }
        // The seat in line with the doorway would.
        let (row, column) = layout::ENTRY_SEAT;
        assert!(overlap(layout::desk_box(row, column), way_in));
    }

    #[test]
    fn go_to_desk_stands_clear_of_every_desk_set_in_a_full_room() {
        let room = room_of(40);
        let footprints: Vec<_> = room
            .desks
            .iter()
            .map(|desk| layout::desk_box(desk.row, desk.column))
            .collect();
        let r = layout::PLAYER_RADIUS;
        let [ox, _, oz] = layout::interior_origin(0);
        for desk in &room.desks {
            let view = layout::beside_desk(0, layout::desk_position(desk.row, desk.column));
            let [x, z] = [view.eye[0] - ox, view.eye[2] - oz];
            let player = ([x - r, x + r], [z - r, z + r]);
            for footprint in &footprints {
                assert!(
                    !overlap(player, *footprint),
                    "beside ({}, {}) the player stands in {footprint:?}",
                    desk.row,
                    desk.column
                );
            }
            // Inside the room's side walls.
            assert!(player.0[0] >= -4.75 && player.0[1] <= 4.75, "{player:?}");
        }
    }

    #[test]
    fn desks_stand_on_a_grid_two_point_two_across_and_two_point_four_between_rows() {
        let xs: Vec<f32> = (0..4).map(|c| layout::desk_position(0, c)[0]).collect();
        for (have, want) in xs.iter().zip([-3.3, -1.1, 1.1, 3.3]) {
            assert!(close(*have, want), "{xs:?}");
        }
        assert!(close(layout::desk_position(0, 0)[1], -1.6));
        assert!(close(layout::desk_position(3, 0)[1], -1.6 - 3.0 * 2.4));
    }

    #[test]
    fn a_rooms_slot_stays_while_it_is_open_and_a_new_room_takes_the_lowest_free_one() {
        let first = next(
            &ready(vec![open("-a", 1), open("-b", 2), open("-c", 3)]),
            &FloorPlan::default(),
        );
        let slots: Vec<_> = first.rooms.iter().map(|r| r.slot).collect();
        assert_eq!(slots, [0, 1, 2]);

        // -a closes: -b and -c close the gap in the corridor but keep their interiors.
        let second = next(&ready(vec![open("-b", 2), open("-c", 3)]), &first);
        let kept: Vec<_> = second
            .rooms
            .iter()
            .map(|r| (r.project.as_str(), r.slot))
            .collect();
        assert_eq!(kept, [("-b", 1), ("-c", 2)]);

        // -d opens at the end of the corridor, in the slot -a left.
        let third = next(
            &ready(vec![open("-b", 2), open("-c", 3), open("-d", 4)]),
            &second,
        );
        assert_eq!(third.rooms[2].project, "-d");
        assert_eq!(third.rooms[2].slot, 0);
        assert_ne!(layout::interior_origin(0), layout::interior_origin(1));
        assert_ne!(layout::render_layer(0), 0, "layer 0 is the corridor's");
    }

    /// The sample timeline the world was checked with, played through the plan step by
    /// step as the world plays it.
    #[test]
    fn the_sample_timeline_fills_the_rooms_desk_by_desk() {
        let file = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/timelines/desks-fill-the-rooms.json"
        );
        let timeline = crate::timeline::Timeline::read(std::path::Path::new(file)).unwrap();
        let mut plan = FloorPlan::default();
        let mut alpha = Vec::new();
        for (at, tree) in timeline.trees() {
            plan = next(&tree, &plan);
            if let Some(room) = plan.rooms.first() {
                alpha.push((at, room.clone()));
            }
        }
        let room_at = |at: u64| &alpha.iter().find(|(t, _)| *t == at).unwrap().1;
        let desk = |room: &Room, agent: &str| {
            room.desks
                .iter()
                .find(|d| d.id.agent == agent)
                .cloned()
                .unwrap_or_else(|| panic!("no desk for {agent}"))
        };

        // Neither finished session has desks, the one finished 26 hours ago nor the one
        // finished 3 hours ago (still in the tree): only the live session's agent sits.
        let first = room_at(3000);
        assert_eq!(first.project, "-home-dev-Projects-Alpha");
        assert_eq!(
            seats(&first.desks),
            [("a2000000-main", "a2000000-main", 0, 1)]
        );

        // A subagent's desk comes with the snapshot that adds it.
        assert!(room_at(3000).desks.iter().all(|d| d.id.agent != "s1"));
        assert_eq!(desk(room_at(10000), "s1").depth, 1);
        assert_eq!(desk(room_at(16000), "s1a").depth, 2);
        // And it goes when the subagent finishes.
        assert!(room_at(22000).desks.iter().all(|d| d.id.agent != "s1a"));

        // The first row holds three, so thirteen desks already open a fourth row.
        let filled = |at: u64| (room_at(at).desks.len(), room_at(at).rows);
        assert_eq!(filled(30000), (13, 4));
        assert_eq!(filled(40000), (14, 4));
        assert_eq!(plan.rooms.len(), 2);
        assert_eq!(
            plan.rooms.iter().map(|r| r.slot).collect::<Vec<_>>(),
            [0, 1]
        );
    }

    #[test]
    fn a_room_grows_without_moving_the_corridor() {
        let small = next(
            &ready(vec![open("-a", 1), open("-b", 2)]),
            &FloorPlan::default(),
        );
        let mut grown = open("-a", 1);
        grown.sessions[0].root.children = (0..20).map(|n| running(&n.to_string())).collect();
        let big = next(&ready(vec![grown, open("-b", 2)]), &small);
        assert_eq!((small.rooms[0].rows, big.rooms[0].rows), (2, 6));
        assert_eq!(layout::end_walls(&small), layout::end_walls(&big));
        let slots = |plan: &FloorPlan| plan.rooms.iter().map(|r| r.slot).collect::<Vec<_>>();
        assert_eq!(slots(&small), slots(&big));
    }

    /// Where the player looks at yaw `yaw` and pitch `pitch` (the camera's turn).
    fn forward(yaw: f32, pitch: f32) -> [f32; 3] {
        [
            -yaw.sin() * pitch.cos(),
            pitch.sin(),
            -yaw.cos() * pitch.cos(),
        ]
    }

    #[test]
    fn go_to_desk_stands_in_the_aisle_at_the_robots_right_turned_to_its_face_and_screen() {
        // The third desk of the second row, in the interior in slot 2.
        let desk = layout::desk_position(1, 2);
        let view = layout::beside_desk(2, desk);
        let [ox, _, oz] = layout::interior_origin(2);
        // The eye in the desk set's frame.
        let eye = [
            view.eye[0] - ox - desk[0],
            view.eye[1],
            view.eye[2] - oz - desk[1],
        ];

        assert_eq!(view.slot, 2);
        assert!(close(eye[1], 1.6), "eye height {}", eye[1]);
        // The robot faces the doorway (+Z), so its right is -X: 0.8 m beyond the
        // 0.4 m chair's side, and a little in front of the seat.
        assert!(close(-eye[0] - 0.2, 0.8), "{eye:?}");
        assert!(eye[2] > 0.1 && eye[2] < 0.5, "{eye:?}");
        // In the aisle, clear of this desk set (x -0.6..0.6) and the next one.
        let clear = (
            -eye[0] - layout::PLAYER_RADIUS,
            -eye[0] + layout::PLAYER_RADIUS,
        );
        assert!(
            clear.0 >= 0.6 && clear.1 <= layout::DESK_SPACING - 0.6,
            "{clear:?}"
        );

        // Turned towards the robot (+X), a little back towards its face, looking down
        // at the face and the screen, both near the middle of the view.
        let look = forward(view.yaw, view.pitch);
        assert!(look[0] > 0.8 && look[1] < 0.0 && look[2] < 0.0, "{look:?}");
        let off = |at: [f32; 3]| {
            let to = [at[0] - eye[0], at[1] - eye[1], at[2] - eye[2]];
            let len = (to[0] * to[0] + to[1] * to[1] + to[2] * to[2]).sqrt();
            let cos = (to[0] * look[0] + to[1] * look[1] + to[2] * look[2]) / len;
            cos.clamp(-1.0, 1.0).acos().to_degrees()
        };
        // The PC's glass centre (`desk_set.py`'s screen B).
        let (face, screen) = ([0.0, 1.10, 0.07], [0.0, 1.035, 0.605]);
        assert!(off(face) < 15.0, "the face is {}° off", off(face));
        assert!(off(screen) < 15.0, "the screen is {}° off", off(screen));
        assert!(off(layout::DESK_AIM) < 0.1, "{}°", off(layout::DESK_AIM));
    }

    #[test]
    fn go_to_desk_finds_the_agents_desk_in_its_room_and_ignores_an_agent_without_one() {
        let alpha = project("-a", vec![session("s1", false, 1), session("s2", true, 2)]);
        let mut beta = open("-b", 3);
        beta.sessions[0].root.children = vec![running("sub")];
        let plan = next(&ready(vec![alpha, beta]), &FloorPlan::default());
        let id = |session: &str, agent: &str| DeskId {
            session: session.into(),
            agent: agent.into(),
        };

        // Beta's subagent sits at its room's second desk, past the empty entry seat;
        // Beta has slot 1.
        assert_eq!(
            beside_desk(&plan, &id("-b-live", "sub")),
            Some(layout::beside_desk(1, layout::desk_position(0, 2)))
        );
        // A live session's agent sits at its room's first desk; a finished one has none.
        assert_eq!(
            beside_desk(&plan, &id("s2", "s2")),
            Some(layout::beside_desk(0, layout::desk_position(0, 1)))
        );
        assert_eq!(beside_desk(&plan, &id("s1", "s1")), None);
        // No such agent, or an agent of a project with no room.
        assert_eq!(beside_desk(&plan, &id("-b-live", "gone")), None);
        assert_eq!(beside_desk(&plan, &id("-c-live", "-c-live")), None);
    }

    #[test]
    fn the_cctv_doorway_stands_opposite_the_middle_of_the_project_doorways() {
        let at = |ids: &[&str]| layout::cctv_doorway_x(&plan_of(ids));
        // Odd: opposite the middle door (doorways at 1.25, 3.75, 6.25, ...).
        assert_eq!(at(&["-a"]), 1.25);
        assert_eq!(at(&["-a", "-b", "-c"]), 3.75);
        assert_eq!(at(&["-a", "-b", "-c", "-d", "-e"]), 6.25);
        // Even: opposite the wall between the two middle doors.
        assert_eq!(at(&["-a", "-b"]), 2.5);
        assert_eq!(at(&["-a", "-b", "-c", "-d"]), 5.0);
        // None: the middle of the shortest corridor, between its end walls.
        assert_eq!(at(&[]), 1.25);
        let (start, far) = layout::end_walls(&plan_of(&[]));
        assert!((at(&[]) - (start + far) / 2.0).abs() < 1e-5);
    }

    #[test]
    fn each_segments_far_wall_opens_where_the_cctv_doorway_falls() {
        use layout::FarWall::{Doorway, DoorwayEnd, DoorwayStart, Plain};
        let walls = |ids: &[&str]| layout::far_walls(&plan_of(ids));
        // The shortest corridor, and one room: its one segment, open in the middle.
        assert_eq!(walls(&[]), [Doorway]);
        assert_eq!(walls(&["-a"]), [Doorway]);
        // Two: the doorway straddles the segments' join (x 1.9..3.1).
        assert_eq!(walls(&["-a", "-b"]), [DoorwayEnd, DoorwayStart]);
        assert_eq!(walls(&["-a", "-b", "-c"]), [Plain, Doorway, Plain]);
        assert_eq!(
            walls(&["-a", "-b", "-c", "-d"]),
            [Plain, DoorwayEnd, DoorwayStart, Plain]
        );
    }

    #[test]
    fn going_through_the_cctv_doorway_lands_on_its_rooms_doorway_and_walls_meet() {
        let add = |a: [f32; 3], b: [f32; 3]| [a[0] + b[0], a[1] + b[1], a[2] + b[2]];
        let close = |a: [f32; 3], b: [f32; 3]| a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-4);
        let [ox, oy, oz] = layout::CCTV_ORIGIN;
        for ids in [
            &[][..],
            &["-a"],
            &["-a", "-b"],
            &["-a", "-b", "-c", "-d", "-e"],
        ] {
            let plan = plan_of(ids);
            let through = layout::through_cctv_doorway(&plan);
            // The corridor's view through its far wall lands on the room's doorway at its
            // inner face (room z = 0), in the opening's middle (x 0, half its height up).
            let landed = add(layout::cctv_corridor_doorway(&plan), through);
            assert!(close(landed, [ox, oy + 1.05, oz]), "{landed:?}");
            // The far wall's corridor face (z 2.4) lands on the room wall's outer face
            // (room z 0.2), where the room shows the corridor.
            let face = [layout::cctv_doorway_x(&plan), 1.05, layout::WIDTH];
            assert!(close(add(face, through), layout::cctv_room_doorway()));
        }
        // The room stands beyond its doorway, turned to face the corridor: into the room
        // (room -Z) is away from the corridor (+Z), and the right as seen from the
        // doorway (room +X) is the right of a player walking in (-X).
        let inner = layout::cctv_to_world([0.0, 0.0, 0.0]);
        let back = layout::cctv_to_world([0.0, 0.0, -7.0]);
        let right = layout::cctv_to_world([1.0, 0.0, 0.0]);
        assert!(close(inner, [ox, oy, oz]));
        assert!(close(back, [ox, oy, oz + 7.0]));
        assert!(close(right, [ox - 1.0, oy, oz]));
        // The doorway snaps along the corridor as it changes; the room does not move.
        let (a, b) = (
            layout::through_cctv_doorway(&plan_of(&["-a"])),
            layout::through_cctv_doorway(&plan_of(&["-a", "-b", "-c"])),
        );
        assert!(close(
            [a[0] - b[0], a[1] - b[1], a[2] - b[2]],
            [2.5, 0.0, 0.0]
        ));
    }

    #[test]
    fn going_through_a_doorway_lands_on_its_rooms_doorway_and_walls_meet() {
        let add = |a: [f32; 3], b: [f32; 3]| [a[0] + b[0], a[1] + b[1], a[2] + b[2]];
        let close = |a: [f32; 3], b: [f32; 3]| a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-4);
        for (slot, n) in [(0, 0), (3, 1), (1, 5)] {
            let through = layout::through_doorway(slot, n);
            let [ox, oy, oz] = layout::interior_origin(slot);
            // The corridor's view of the room lands on the room's doorway at its inner
            // face (room z = 0), in the opening's middle (x -3.5, half its height up).
            let landed = add(layout::corridor_doorway(n), through);
            assert!(close(landed, [ox - 3.5, oy + 1.05, oz]), "{landed:?}");
            // The corridor wall's corridor face (z = 0) lands on the room wall's (z 0.2),
            // where the room shows the corridor.
            let face = [layout::doorway_x(n), layout::DOORWAY_HEIGHT / 2.0, 0.0];
            assert!(close(add(face, through), layout::room_doorway(slot)));
        }
        // The doorway moves along the corridor when the rooms before it close; the
        // interior does not.
        let (a, b) = (layout::through_doorway(2, 2), layout::through_doorway(2, 1));
        assert!(close(
            [b[0] - a[0], b[1] - a[1], b[2] - a[2]],
            [layout::SEGMENT, 0.0, 0.0]
        ));
    }

    /// `v` in the frame of a camera turned by `yaw` then `pitch` (Y then X, as the
    /// player's look): x right, y up, -z ahead.
    fn into_camera(yaw: f32, pitch: f32, [x, y, z]: [f32; 3]) -> [f32; 3] {
        let (x, z) = (x * yaw.cos() - z * yaw.sin(), x * yaw.sin() + z * yaw.cos());
        let (y, z) = (
            y * pitch.cos() + z * pitch.sin(),
            -y * pitch.sin() + z * pitch.cos(),
        );
        [x, y, z]
    }

    #[test]
    fn the_wall_camera_hangs_over_the_doorway_and_frames_every_robots_face_in_a_room_of_any_depth()
    {
        let mut fovs = Vec::new();
        for rows in 2..=12 {
            let camera = layout::wall_camera(rows);
            // Inside, over the doorway, under the ceiling.
            let [x, y, z] = camera.lens;
            assert!((x - layout::ROOM_DOORWAY_X).abs() < 0.3, "{camera:?}");
            assert!(
                (2.1..3.0).contains(&y) && (-0.5..0.0).contains(&z),
                "{camera:?}"
            );
            // Turned into the room, away from the doorway's side, and down.
            assert!(camera.yaw < 0.0 && camera.pitch < 0.0, "{camera:?}");
            // A wide lens, never a fisheye.
            assert!((0.8..1.35).contains(&camera.fov), "{camera:?}");
            fovs.push(camera.fov);
            let half_y = (camera.fov / 2.0).tan();
            let half_x = half_y * 16.0 / 9.0;
            for n in 0..rows * layout::DESKS_PER_ROW - 1 {
                let (row, column) = layout::seat(n);
                let [dx, dz] = layout::desk_position(row, column);
                // A full-size robot's face (`DESK_AIM`'s note) and its desk's front.
                for p in [[dx, 1.10, dz + 0.07], [dx, 0.5, dz + 1.05]] {
                    let [cx, cy, cz] =
                        into_camera(camera.yaw, camera.pitch, [p[0] - x, p[1] - y, p[2] - z]);
                    assert!(cz < 0.0, "{rows} rows: seat {n} behind the camera");
                    let (sx, sy) = (cx / -cz / half_x, cy / -cz / half_y);
                    assert!(
                        sx.abs() < 0.98 && sy.abs() < 0.98,
                        "{rows} rows: seat {n} at ({sx:.2}, {sy:.2}) out of frame"
                    );
                }
            }
        }
        // It snaps to a new framing as the room grows.
        assert_ne!(layout::wall_camera(2), layout::wall_camera(6));
        assert!(fovs.windows(2).all(|w| w[1] >= w[0] - 1e-3), "{fovs:?}");
    }
}
