//! Clutter and the haze (sweatshop-rooms ticket 24; spec: "The look", "World settings").
//!
//! Each row segment of a room is dressed with clutter pieces from the generated
//! `clutter_*.glb` files: crumpled paper, rusty cans, cardboard crates, loose sheets and
//! floor cables, and in every room one bucket under a leak and one broken chair.
//! [`row_clutter`] places them, pure and free of Bevy: by a seed from the room's project
//! id and the row, so a room looks the same every time and a row added at the back
//! leaves the others alone. Nothing lands in a desk set's box (every seat's, taken or
//! not, so desks coming and going never meet clutter), in front of the doorway, in the
//! way in from it ([`layout::entry`]) or in a wall. Standing pieces keep to the side walls beside the desks, out of the walking
//! lines between the rows; only pieces lying flat go anywhere else. Clutter never
//! blocks walking (`collision`).
//!
//! The Clutter setting hides every piece tagged as clutter (root node extras
//! `{"clutter": true}`, `desktop/world-assets`) and takes the haze, the main camera's
//! `DistanceFog`, away; the portal cameras follow the main camera's fog (`portals`). The
//! dead tube and the pipes are part of the room and stay.

use std::collections::VecDeque;

use bevy::gltf::GltfExtras;
use bevy::prelude::*;

use super::floor_plan::layout;
use super::pieces::Kit;
use super::portals::{FollowMain, PortalCamera};
use super::rooms::Detail;
use super::walk::PlayerEye;

/// The dusty haze of the look.
pub fn haze() -> DistanceFog {
    DistanceFog {
        color: Color::srgb(0.10, 0.11, 0.09),
        falloff: FogFalloff::Exponential { density: 0.09 },
        ..default()
    }
}

/// A clutter piece: one generated file, holding one scene of the same name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Piece {
    CrumpledPaper,
    RustyCans,
    CardboardCrates,
    LooseSheets,
    FloorCables,
    Bucket,
    BrokenChair,
}

/// Where a piece may go in its row segment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Region {
    /// Against a side wall, beside the desks: it stands up, so it keeps out of the
    /// walking lines between the rows.
    BesideDesks,
    /// Against a side wall, the segment's whole length (flat).
    AlongWall,
    /// Anywhere on the free floor (flat).
    Floor,
}

impl Piece {
    #[cfg(test)]
    pub const ALL: [Piece; 7] = [
        Piece::CrumpledPaper,
        Piece::RustyCans,
        Piece::CardboardCrates,
        Piece::LooseSheets,
        Piece::FloorCables,
        Piece::Bucket,
        Piece::BrokenChair,
    ];

    /// Its scene, and its file's name without `.glb`.
    pub fn scene(self) -> &'static str {
        match self {
            Piece::CrumpledPaper => "clutter_crumpled_paper",
            Piece::RustyCans => "clutter_rusty_cans",
            Piece::CardboardCrates => "clutter_cardboard_crates",
            Piece::LooseSheets => "clutter_loose_sheets",
            Piece::FloorCables => "clutter_floor_cables",
            Piece::Bucket => "clutter_bucket",
            Piece::BrokenChair => "clutter_broken_chair",
        }
    }

    /// Its footprint on the floor from its origin, `[x, z]` from `min` to `max`, from the
    /// generated file.
    fn footprint(self) -> Rect2 {
        let (x, z) = match self {
            Piece::CrumpledPaper => ([-0.19, 0.26], [-0.24, 0.06]),
            Piece::RustyCans => ([-0.16, 0.26], [-0.25, 0.04]),
            Piece::CardboardCrates => ([-0.27, 0.76], [-0.28, 0.24]),
            Piece::LooseSheets => ([-0.26, 0.34], [-0.37, 0.18]),
            Piece::FloorCables => ([-1.11, 1.14], [-0.17, 0.11]),
            Piece::Bucket => ([-0.17, 0.17], [-0.17, 0.17]),
            Piece::BrokenChair => ([-0.652, 0.102], [-0.514, 0.36]),
        };
        Rect2::new(x, z)
    }

    fn region(self) -> Region {
        match self {
            Piece::RustyCans | Piece::CardboardCrates | Piece::Bucket | Piece::BrokenChair => {
                Region::BesideDesks
            }
            Piece::FloorCables => Region::AlongWall,
            Piece::CrumpledPaper | Piece::LooseSheets => Region::Floor,
        }
    }

    /// How far its turn strays from square to the walls, in radians.
    fn wobble(self) -> f32 {
        match self {
            Piece::FloorCables => 0.03,
            Piece::CardboardCrates => 0.12,
            // It only just fits between a desk and the wall.
            Piece::BrokenChair => 0.02,
            _ => 0.3,
        }
    }
}

/// A piece placed in its row segment: `at` is `[x, z]` from the row's origin
/// ([`layout::row_z`]), and `yaw` its turn about the vertical, as Bevy turns
/// (`Quat::from_rotation_y`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    pub piece: Piece,
    pub at: [f32; 2],
    pub yaw: f32,
}

impl Placement {
    /// The box its footprint takes up, turned and placed, in the row's frame.
    pub fn bounds(&self) -> Rect2 {
        let f = self.piece.footprint();
        let corners = [
            [f.min[0], f.min[1]],
            [f.min[0], f.max[1]],
            [f.max[0], f.min[1]],
            [f.max[0], f.max[1]],
        ]
        .map(|p| {
            let [x, z] = turn(p, self.yaw);
            [x + self.at[0], z + self.at[1]]
        });
        let mut bounds = Rect2 {
            min: corners[0],
            max: corners[0],
        };
        for [x, z] in corners {
            bounds.min = [bounds.min[0].min(x), bounds.min[1].min(z)];
            bounds.max = [bounds.max[0].max(x), bounds.max[1].max(z)];
        }
        bounds
    }
}

/// `[x, z]` turned by `yaw` about the vertical, as `Quat::from_rotation_y(yaw)` turns.
fn turn([x, z]: [f32; 2], yaw: f32) -> [f32; 2] {
    let (sin, cos) = yaw.sin_cos();
    [x * cos + z * sin, -x * sin + z * cos]
}

/// An axis-aligned box in the floor plane, `[x, z]` from `min` to `max`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect2 {
    pub min: [f32; 2],
    pub max: [f32; 2],
}

impl Rect2 {
    fn new(x: [f32; 2], z: [f32; 2]) -> Self {
        Self {
            min: [x[0], z[0]],
            max: [x[1], z[1]],
        }
    }

    pub fn overlaps(&self, other: &Rect2) -> bool {
        self.min[0] < other.max[0]
            && other.min[0] < self.max[0]
            && self.min[1] < other.max[1]
            && other.min[1] < self.max[1]
    }

    fn inside(&self, outer: &Rect2) -> bool {
        self.min[0] >= outer.min[0]
            && self.max[0] <= outer.max[0]
            && self.min[1] >= outer.min[1]
            && self.max[1] <= outer.max[1]
    }

    fn grown(&self, by: f32) -> Rect2 {
        Rect2 {
            min: [self.min[0] - by, self.min[1] - by],
            max: [self.max[0] + by, self.max[1] + by],
        }
    }
}

/// The room's inner side walls, either side of its middle.
const HALF_WIDTH: f32 = 4.75;
/// Kept clear around every piece: desk boxes and the other pieces, and the walls.
const MARGIN: f32 = 0.05;
const WALL_GAP: f32 = 0.02;
/// The side walls' strip that standing pieces keep to: from the outer desks' boxes to
/// the wall.
const STRIP: f32 = 0.85;
/// How often a piece is tried somewhere else before it is left out.
const TRIES: usize = 64;
/// How far a standing piece may stand off the end of its strip.
const END_SLACK: f32 = 0.08;
/// The row with the dead tube (`rooms`): the leak drips there, so the bucket stands
/// there too. Every room has it.
const BUCKET_ROW: usize = 1;

/// The desk sets' boxes in a row segment, every seat's, in the row's frame: from the
/// generated `desk_set.glb` ([`layout::desk_box`]). The empty entry seat's too
/// ([`layout::ENTRY_SEAT`]), so clutter keeps out of the way in.
pub fn desk_boxes() -> Vec<Rect2> {
    (0..layout::DESKS_PER_ROW)
        .map(|column| {
            let (x, z) = layout::desk_box(0, column);
            let row = layout::row_z(0);
            Rect2::new(x, [z[0] - row, z[1] - row])
        })
        .collect()
}

/// The floor in front of the doorway that stays clear, in row 0's frame: from the
/// doorway's wall 1.2 m into the room, and from the left wall to 0.6 m past the
/// opening's right edge (x -4.10..-2.90).
pub fn doorway_clearing() -> Rect2 {
    let z = -layout::row_z(0);
    Rect2::new([-HALF_WIDTH, -2.3], [z - 1.2, z])
}

/// The way in from the doorway, in row 0's frame ([`layout::entry`]): clutter does not
/// block, but keeps out of it so the way in looks as clear as it is.
pub fn entry_clearing() -> Rect2 {
    let (x, z) = layout::entry();
    let row = layout::row_z(0);
    Rect2::new(x, [z[0] - row, z[1] - row])
}

/// A row segment's floor, in its frame, inside its walls.
fn segment() -> Rect2 {
    Rect2::new([-HALF_WIDTH, HALF_WIDTH], [-layout::ROW_DEPTH, 0.0]).grown(-WALL_GAP)
}

/// Where a piece of `region` may lie, in the row's frame, on `side` (-1 left, 1 right).
fn region_box(region: Region, side: f32) -> Rect2 {
    let wall = |z: [f32; 2]| {
        let x = if side < 0.0 {
            [-HALF_WIDTH + WALL_GAP, -HALF_WIDTH + STRIP - MARGIN]
        } else {
            [HALF_WIDTH - STRIP + MARGIN, HALF_WIDTH - WALL_GAP]
        };
        Rect2::new(x, [z[0] + WALL_GAP, z[1] - WALL_GAP])
    };
    match region {
        Region::BesideDesks => {
            let desk = desk_boxes()[0];
            wall([desk.min[1] - 0.1, desk.max[1] + 0.1])
        }
        Region::AlongWall => wall([-layout::ROW_DEPTH, 0.0]),
        Region::Floor => segment(),
    }
}

/// The clutter of row `row` in the room of `project`: the same every time.
pub fn row_clutter(project: &str, row: usize) -> Vec<Placement> {
    let room = fnv(project);
    let mut random = Random(room ^ (row as u64 + 1).wrapping_mul(0x9e37_79b9_7f4a_7c15));

    // The biggest first, so they find room.
    let mut wanted = VecDeque::new();
    if row == (room % 2) as usize {
        wanted.push_back(Piece::BrokenChair);
    }
    if row == BUCKET_ROW {
        wanted.push_back(Piece::Bucket);
    }
    wanted.push_back(if random.below(3) == 0 {
        Piece::RustyCans
    } else {
        Piece::CardboardCrates
    });
    if random.below(3) == 0 {
        wanted.push_back(Piece::RustyCans);
    }
    if random.below(2) == 0 {
        wanted.push_back(Piece::FloorCables);
    }
    for _ in 0..random.below(2) {
        wanted.push_back(Piece::LooseSheets);
    }
    for _ in 0..1 + random.below(3) {
        wanted.push_back(Piece::CrumpledPaper);
    }

    let mut keep_out = desk_boxes();
    if row == 0 {
        keep_out.extend([doorway_clearing(), entry_clearing()]);
    }
    let floor = segment();
    let mut placed: Vec<Placement> = Vec::new();
    while let Some(piece) = wanted.pop_front() {
        let before = placed.len();
        for _ in 0..TRIES {
            let side = if random.below(2) == 0 { -1.0 } else { 1.0 };
            let region = region_box(piece.region(), side);
            let quarter = std::f32::consts::FRAC_PI_2 * random.below(4) as f32;
            let yaw = quarter + random.between(-piece.wobble(), piece.wobble());
            // Only where the turned piece fits in its region.
            let size = Placement {
                piece,
                at: [0.0, 0.0],
                yaw,
            }
            .bounds();
            let (low, high) = (
                [region.min[0] - size.min[0], region.min[1] - size.min[1]],
                [region.max[0] - size.max[0], region.max[1] - size.max[1]],
            );
            if low[0] > high[0] || low[1] > high[1] {
                continue;
            }
            // A standing piece is pushed up to one end of its strip, next to the walk
            // between the rows, so the next one still finds room behind it.
            let z = match piece.region() {
                Region::BesideDesks if random.below(2) == 0 => {
                    low[1] + random.between(0.0, END_SLACK).min(high[1] - low[1])
                }
                Region::BesideDesks => {
                    high[1] - random.between(0.0, END_SLACK).min(high[1] - low[1])
                }
                _ => random.between(low[1], high[1]),
            };
            let candidate = Placement {
                piece,
                at: [random.between(low[0], high[0]), z],
                yaw,
            };
            let bounds = candidate.bounds();
            let fits = bounds.inside(&region)
                && bounds.inside(&floor)
                && !keep_out
                    .iter()
                    .any(|other| bounds.overlaps(&other.grown(MARGIN)))
                && !placed
                    .iter()
                    .any(|other| bounds.overlaps(&other.bounds().grown(MARGIN)));
            if fits {
                placed.push(candidate);
                break;
            }
        }
        // No room for the crates: a few cans fit where they do not.
        if placed.len() == before && piece == Piece::CardboardCrates {
            wanted.push_front(Piece::RustyCans);
        }
    }
    placed
}

/// FNV-1a: stable across runs and builds, unlike std's hasher.
fn fnv(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// SplitMix64: small, and the same everywhere.
struct Random(u64);

impl Random {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    fn between(&mut self, low: f32, high: f32) -> f32 {
        let unit = (self.next() >> 40) as f32 / (1u64 << 24) as f32;
        low + (high - low) * unit
    }
}

/// A piece tagged as clutter in its file: the root node of a `clutter_*` scene.
#[derive(Component)]
pub struct Clutter;

pub struct ClutterPlugin;

impl Plugin for ClutterPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, show_clutter.before(FollowMain))
            // After the scenes spawn, before Bevy works out what is visible: a new
            // piece is hidden from its first frame while Clutter is off.
            .add_systems(
                PostUpdate,
                tag_clutter
                    .before(bevy::camera::visibility::VisibilitySystems::VisibilityPropagate),
            );
    }
}

/// Spawns row `n`'s clutter in the room of `project`, below `row`.
pub fn spawn_row_clutter(row: &mut ChildSpawnerCommands, kit: &Kit, project: &str, n: usize) {
    for placement in row_clutter(project, n) {
        let [x, z] = placement.at;
        row.spawn((
            kit.piece(placement.piece.scene()),
            Transform::from_xyz(x, 0.0, z).with_rotation(Quat::from_rotation_y(placement.yaw)),
        ));
    }
}

/// Whether Clutter shows.
fn shown(detail: &Detail) -> Visibility {
    if detail.clutter {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    }
}

/// Marks each newly spawned node tagged as clutter, shown or hidden as Clutter says.
fn tag_clutter(
    mut commands: Commands,
    detail: Res<Detail>,
    mut nodes: Query<(Entity, &GltfExtras, &mut Visibility), Added<GltfExtras>>,
) {
    for (entity, extras, mut visibility) in &mut nodes {
        let tagged = serde_json::from_str::<serde_json::Value>(&extras.value)
            .is_ok_and(|extras| extras.get("clutter") == Some(&serde_json::Value::Bool(true)));
        if tagged {
            commands.entity(entity).insert(Clutter);
            visibility.set_if_neq(shown(&detail));
        }
    }
}

/// The player's camera, not a doorway's.
type MainCamera = (With<PlayerEye>, Without<PortalCamera>);

/// When Clutter changes: shows or hides every clutter piece, and gives the main camera
/// the haze or takes it away (the portal cameras follow it).
fn show_clutter(
    mut commands: Commands,
    detail: Res<Detail>,
    mut pieces: Query<&mut Visibility, With<Clutter>>,
    cameras: Query<(Entity, Has<DistanceFog>), MainCamera>,
    mut clutter: Local<Option<bool>>,
) {
    // Only Clutter changing, not Textures.
    if clutter.replace(detail.clutter) == Some(detail.clutter) {
        return;
    }
    let want = shown(&detail);
    let mut count = 0;
    for mut visibility in &mut pieces {
        visibility.set_if_neq(want);
        count += 1;
    }
    for (camera, hazy) in &cameras {
        match (detail.clutter, hazy) {
            (true, false) => {
                commands.entity(camera).insert(haze());
            }
            (false, true) => {
                commands.entity(camera).remove::<DistanceFog>();
            }
            _ => {}
        }
    }
    eprintln!(
        "[world] clutter {}: {count} pieces {}, haze {}",
        if detail.clutter { "on" } else { "off" },
        if detail.clutter { "shown" } else { "hidden" },
        if detail.clutter { "on" } else { "off" },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clutter_off_hides_the_tagged_pieces_and_the_haze_and_on_brings_them_back() {
        let mut app = App::new();
        app.init_resource::<Detail>().add_plugins(ClutterPlugin);
        let tagged = |app: &mut App, extras: &str| {
            app.world_mut()
                .spawn((
                    GltfExtras {
                        value: extras.into(),
                    },
                    Visibility::default(),
                ))
                .id()
        };
        let piece = tagged(&mut app, r#"{"clutter":true}"#);
        let other = tagged(&mut app, r#"{"clutter":false}"#);
        let camera = app.world_mut().spawn((PlayerEye, haze())).id();
        app.update();
        let visibility = |app: &App, entity| *app.world().get::<Visibility>(entity).unwrap();
        assert_eq!(visibility(&app, piece), Visibility::Inherited);

        app.world_mut().resource_mut::<Detail>().clutter = false;
        app.update();
        assert_eq!(visibility(&app, piece), Visibility::Hidden);
        assert_eq!(visibility(&app, other), Visibility::Inherited);
        assert!(app.world().get::<DistanceFog>(camera).is_none());
        // A piece spawned while Clutter is off is hidden from its first frame.
        let late = tagged(&mut app, r#"{"clutter": true}"#);
        app.update();
        assert_eq!(visibility(&app, late), Visibility::Hidden);

        app.world_mut().resource_mut::<Detail>().clutter = true;
        app.update();
        assert_eq!(visibility(&app, piece), Visibility::Inherited);
        assert_eq!(visibility(&app, late), Visibility::Inherited);
        assert!(app.world().get::<DistanceFog>(camera).is_some());
    }

    fn projects() -> Vec<String> {
        (0..300)
            .map(|n| format!("-home-dev-Projects-room-{n}"))
            .chain(["-home-dev-Projects-Alpha".into(), String::new()])
            .collect()
    }

    #[test]
    fn the_same_room_gets_the_same_clutter_and_rooms_differ() {
        let a = row_clutter("-home-dev-Projects-Alpha", 2);
        assert_eq!(a, row_clutter("-home-dev-Projects-Alpha", 2));
        assert_ne!(a, row_clutter("-home-dev-Projects-Beta", 2));
        assert_ne!(a, row_clutter("-home-dev-Projects-Alpha", 3));
    }

    #[test]
    fn clutter_stays_clear_of_every_desk_box_the_doorway_and_the_walls() {
        let floor = Rect2::new([-HALF_WIDTH, HALF_WIDTH], [-layout::ROW_DEPTH, 0.0]);
        for project in projects() {
            for row in 0..8 {
                let placed = row_clutter(&project, row);
                for (i, item) in placed.iter().enumerate() {
                    let bounds = item.bounds();
                    assert!(bounds.inside(&floor), "{project} row {row}: {item:?}");
                    for desk in desk_boxes() {
                        assert!(!bounds.overlaps(&desk), "{project} row {row}: {item:?}");
                    }
                    if row == 0 {
                        assert!(!bounds.overlaps(&doorway_clearing()), "{item:?}");
                        assert!(!bounds.overlaps(&entry_clearing()), "{item:?}");
                    }
                    for other in &placed[i + 1..] {
                        assert!(!bounds.overlaps(&other.bounds()), "{item:?} {other:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn standing_pieces_keep_beside_the_desks_out_of_the_walk_between_rows() {
        let desk = desk_boxes()[0];
        for project in projects() {
            for row in 0..6 {
                for item in row_clutter(&project, row) {
                    let bounds = item.bounds();
                    let flat = matches!(
                        item.piece,
                        Piece::CrumpledPaper | Piece::LooseSheets | Piece::FloorCables
                    );
                    if flat {
                        continue;
                    }
                    assert!(bounds.min[1] >= desk.min[1] - 0.1, "{item:?}");
                    assert!(bounds.max[1] <= desk.max[1] + 0.1, "{item:?}");
                    assert!(
                        bounds.min[0] >= HALF_WIDTH - STRIP || bounds.max[0] <= STRIP - HALF_WIDTH,
                        "{item:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn every_room_has_one_bucket_and_one_broken_chair_and_more() {
        for project in projects() {
            let rows: Vec<_> = (0..layout::MIN_ROWS)
                .flat_map(|row| row_clutter(&project, row))
                .collect();
            let count = |piece| rows.iter().filter(|p| p.piece == piece).count();
            assert_eq!(count(Piece::Bucket), 1, "{project}: {rows:?}");
            assert_eq!(count(Piece::BrokenChair), 1, "{project}");
            assert!(count(Piece::CrumpledPaper) >= 2, "{project}: {rows:?}");
            assert!(
                count(Piece::CardboardCrates) + count(Piece::RustyCans) >= 2,
                "{project}: {rows:?}"
            );
        }
        let all: Vec<Piece> = projects()
            .iter()
            .flat_map(|p| (0..4).flat_map(|row| row_clutter(p, row)))
            .map(|p| p.piece)
            .collect();
        for piece in Piece::ALL {
            assert!(all.contains(&piece), "{piece:?} is never placed");
        }
    }

    #[test]
    fn a_footprint_turns_as_bevy_turns() {
        for yaw in [0.3_f32, 1.2, -2.0, 3.1] {
            let bevy = Quat::from_rotation_y(yaw) * Vec3::new(0.7, 0.0, -0.2);
            let [x, z] = turn([0.7, -0.2], yaw);
            assert!(
                (bevy.x - x).abs() < 1e-5 && (bevy.z - z).abs() < 1e-5,
                "{yaw}"
            );
        }
    }

    #[test]
    fn the_desk_boxes_and_the_clearing_are_where_the_room_has_them() {
        // Row 0's desks stand at z -1.6 in the room, so 1.4 m from the row's start.
        let boxes = desk_boxes();
        assert_eq!(boxes.len(), 4);
        assert!((boxes[0].min[0] - -3.9).abs() < 1e-5);
        assert!((boxes[3].max[0] - 3.9).abs() < 1e-5);
        assert!((boxes[0].min[1] - -1.65).abs() < 1e-5);
        assert!((boxes[0].max[1] - -0.35).abs() < 1e-5);
        // The clearing spans the doorway's opening (x -4.10..-2.90) from its wall.
        let clearing = doorway_clearing();
        assert!(clearing.min[0] <= -4.10 && clearing.max[0] >= -2.90);
        assert!((clearing.max[1] - 0.2).abs() < 1e-5);
        // The way in: the opening and a player's radius either side, 2.45 m deep.
        let entry = entry_clearing();
        assert!((entry.min[0] - -4.4).abs() < 1e-5 && (entry.max[0] - -2.6).abs() < 1e-5);
        assert!((entry.max[1] - 0.2).abs() < 1e-5);
        assert!((entry.min[1] - (0.2 - 2.45)).abs() < 1e-5, "{entry:?}");
    }
}
