//! The PCs' screens (spec: "Which PCs are live", "The PC screen"). The live budget
//! (`live_budget`) picks the sessions whose terminals the app watches, and the session
//! terminals (the shell's, through [`SessionScreens`]) attach to match; each attachable
//! root agent's PC then shows its session's PC image on its glass:
//!
//! - **live**, attached now: the image in full colour;
//! - **frozen**, not live (or not attached yet) but seen before: its last image, greyed
//!   and dimmed ([`frozen`]);
//! - **plain**, a terminal-started session's, a subagent's, or one never seen: the
//!   glass's own lit material, as before.
//!
//! The image goes on the glass's material, so doorways and CCTV feeds show whatever
//! look a PC has (feeds see only the PCs' backs).

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;
use std::time::{Duration, Instant};

use bevy::asset::RenderAssetUsages;
use bevy::gltf::GltfMaterialName;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use terminals::{PcImage, PC_IMAGE_HEIGHT, PC_IMAGE_WIDTH};

use super::collision::Space;
use super::corridor::Plan;
use super::floor_plan::DeskId;
use super::live_budget::{LiveBudget, Wants};
use super::rooms::Seated;
use super::surfaces::{OnGlass, SwapMaterials};
use super::walk::PlayerEye;

/// The glTF material of a PC's glass (`desk_set.py`).
const GLASS: &str = "pc_screen";

/// A frozen screen keeps this much of its colours' saturation and value (HSV).
const FROZEN_SATURATION: f32 = 0.15;
const FROZEN_VALUE: f32 = 0.45;

/// The PCs' looks are brought up to date this often at most: images change at most
/// twice a second.
const LOOK_EVERY: Duration = Duration::from_millis(100);

/// What the session terminals have for one session's PC.
#[derive(Clone, Debug)]
pub struct Screen {
    /// The last image painted, live or not.
    pub image: PcImage,
    /// Whether an attach is open now, so the image is live.
    pub attached: bool,
}

/// The session terminals, as the world reaches them (the shell's `terminals` crate).
pub trait SessionScreens: Send + Sync + 'static {
    /// Makes `short_ids` the live set: attaches to them, hangs up every other.
    fn set_live(&self, short_ids: &BTreeSet<String>);

    /// What each of `short_ids` has, in order; none while the terminals are busy
    /// painting (asked again next time, so the world never waits).
    fn screens(&self, short_ids: &[&str]) -> Option<Vec<Option<Screen>>>;

    /// Hangs up every attach now and forgets the seat: the world has ended (a stop, a
    /// crash or a restart). Nothing attaches again until a live set comes.
    fn shutdown(&self);
}

/// The session terminals, when the shell has them.
#[derive(Resource, Clone)]
pub struct Screens(pub Arc<dyn SessionScreens>);

/// Whether the world is on screen: shown and drawing, not kept warm or suspended. Set
/// by the frame loop.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OnScreen(pub bool);

/// The session the player is seated at, by its id (ticket 12): always live.
#[derive(Resource, Clone, Debug, Default, PartialEq, Eq)]
pub struct SeatedSession(pub Option<String>);

#[derive(Resource, Default)]
struct Budget(LiveBudget);

/// A PC's glass, on the desk entity it belongs to.
#[derive(Component)]
struct PcGlass {
    desk: Entity,
}

/// Each attachable session's PC image in the world, by short id.
#[derive(Resource, Default)]
struct PcImages {
    /// The glass's own material, which each session's is made from.
    glass: Option<Handle<StandardMaterial>>,
    screens: HashMap<String, ScreenMaterial>,
}

struct ScreenMaterial {
    image: Handle<Image>,
    material: Handle<StandardMaterial>,
    /// The serial and look of the picture the image holds.
    serial: u64,
    live: bool,
}

pub struct PcScreensPlugin;

impl Plugin for PcScreensPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<OnScreen>()
            .init_resource::<SeatedSession>()
            .init_resource::<Budget>()
            .init_resource::<PcImages>()
            .add_systems(Update, choose_live)
            .add_systems(
                PostUpdate,
                (find_glass, show_screens).chain().before(SwapMaterials),
            );
    }
}

/// Chooses the live set from where the player stands and hands it to the terminals.
fn choose_live(
    terminals: Option<Res<Screens>>,
    on_screen: Res<OnScreen>,
    seated: Res<SeatedSession>,
    plan: Res<Plan>,
    space: Res<Space>,
    eyes: Query<&Transform, With<PlayerEye>>,
    mut budget: ResMut<Budget>,
) {
    let Some(terminals) = terminals else { return };
    let eye = eyes.single().map_or(Vec3::ZERO, |eye| eye.translation);
    let wants = Wants {
        plan: &plan.0,
        shown: on_screen.0,
        room: match *space {
            Space::Room(slot) => Some(slot),
            Space::Corridor | Space::Cctv => None,
        },
        eye: eye.to_array(),
        seated: seated.0.as_deref(),
    };
    if let Some(live) = budget.0.update(Instant::now(), &wants) {
        let ids: Vec<&str> = live.iter().map(String::as_str).collect();
        eprintln!(
            "[world] live screens: {} in {}",
            if ids.is_empty() {
                "none".to_string()
            } else {
                ids.join(", ")
            },
            space.describe()
        );
        terminals.0.set_live(live);
    }
}

/// Marks each new PC glass with the desk it stands on.
fn find_glass(
    mut commands: Commands,
    glasses: Query<
        (Entity, &GltfMaterialName, &MeshMaterial3d<StandardMaterial>),
        Added<GltfMaterialName>,
    >,
    parents: Query<&ChildOf>,
    desks: Query<(), With<Seated>>,
    mut held: ResMut<PcImages>,
) {
    for (entity, name, material) in &glasses {
        if name.0 != GLASS {
            continue;
        }
        let Some(desk) = parents
            .iter_ancestors(entity)
            .find(|ancestor| desks.contains(*ancestor))
        else {
            continue;
        };
        held.glass.get_or_insert_with(|| material.0.clone());
        commands
            .entity(entity)
            .insert((PcGlass { desk }, OnGlass(None)));
    }
}

/// Gives each PC's glass its look: its session's image, live or frozen, or its own
/// material.
#[allow(clippy::too_many_arguments)]
fn show_screens(
    terminals: Option<Res<Screens>>,
    plan: Res<Plan>,
    budget: Res<Budget>,
    desks: Query<&Seated>,
    mut glasses: Query<(&PcGlass, &mut OnGlass)>,
    mut held: ResMut<PcImages>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut last: Local<Option<Instant>>,
) {
    let Some(terminals) = terminals else { return };
    let now = Instant::now();
    if last.is_some_and(|at| now < at + LOOK_EVERY) {
        return;
    }
    let short_ids: HashMap<&DeskId, &str> = plan
        .0
        .rooms
        .iter()
        .flat_map(|room| &room.desks)
        .filter_map(|desk| Some((&desk.id, desk.short_id.as_deref()?)))
        .collect();
    let mut wanted: Vec<&str> = short_ids.values().copied().collect();
    wanted.sort_unstable();
    wanted.dedup();
    let Some(screens) = terminals.0.screens(&wanted) else {
        return;
    };
    *last = Some(now);
    let screens: HashMap<&str, Screen> = wanted
        .iter()
        .zip(screens)
        .filter_map(|(id, screen)| Some((*id, screen?)))
        .collect();
    let held = &mut *held;
    held.screens
        .retain(|id, _| screens.contains_key(id.as_str()));
    let Some(glass) = held.glass.clone() else {
        return;
    };
    for (id, screen) in &screens {
        let live = screen.attached && budget.0.live().contains(*id);
        let picture = held.screens.entry(id.to_string()).or_insert_with(|| {
            let image = images.add(blank());
            let material = screen_material(&glass, image.clone(), &mut materials);
            ScreenMaterial {
                image,
                material,
                serial: 0,
                live: false,
            }
        });
        if picture.serial == screen.image.serial && picture.live == live {
            continue;
        }
        if let Some(mut image) = images.get_mut(&picture.image) {
            image.data = Some(if live {
                screen.image.rgba.to_vec()
            } else {
                frozen(&screen.image.rgba)
            });
        }
        picture.serial = screen.image.serial;
        picture.live = live;
    }
    for (glass, mut on) in &mut glasses {
        let shows = desks
            .get(glass.desk)
            .ok()
            .and_then(|Seated(desk)| short_ids.get(desk))
            .and_then(|id| held.screens.get(*id))
            .map(|picture| picture.material.clone());
        if on.0 != shows {
            on.0 = shows;
        }
    }
}

/// A PC image's texture before its first picture.
fn blank() -> Image {
    Image::new_fill(
        Extent3d {
            width: PC_IMAGE_WIDTH,
            height: PC_IMAGE_HEIGHT,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[0, 0, 0, 255],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    )
}

/// The glass showing `image`: the glass's own material with the image as its glow and
/// nothing lit on top, so the text stays readable under the tubes.
fn screen_material(
    glass: &Handle<StandardMaterial>,
    image: Handle<Image>,
    materials: &mut Assets<StandardMaterial>,
) -> Handle<StandardMaterial> {
    let mut material = materials.get(glass).cloned().unwrap_or_default();
    material.base_color = Color::BLACK;
    material.base_color_texture = None;
    material.emissive_texture = Some(image);
    if material.emissive == LinearRgba::BLACK {
        material.emissive = LinearRgba::WHITE;
    }
    materials.add(material)
}

/// A frozen screen: its last image greyed and dimmed (spec: "The PC screen"), each
/// pixel keeping its hue, [`FROZEN_SATURATION`] of its saturation and
/// [`FROZEN_VALUE`] of its value.
pub fn frozen(rgba: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(rgba.len());
    for &[r, g, b, a] in rgba.as_chunks::<4>().0 {
        let value = r.max(g).max(b) as f32;
        for channel in [r, g, b] {
            // A channel sits below the value by saturation × something of the hue's.
            let greyed = value - FROZEN_SATURATION * (value - channel as f32);
            out.push((greyed * FROZEN_VALUE).round() as u8);
        }
        out.push(a);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frozen_image_keeps_a_seventh_of_its_saturation_and_under_half_its_value() {
        let image = [
            255, 0, 0, 255, // red
            255, 255, 255, 255, // white
            0, 0, 0, 255, // black
            40, 200, 120, 128, // a green, half transparent
        ];
        // Saturation 0.15 and value 0.45 of each: red (0°, 1, 1) becomes (0°, 0.15,
        // 0.45), (114.75, 97.5, 97.5); the green (150°, 0.8, 200) becomes (150°,
        // 0.12, 90), (79.2, 90, 84.6). Alpha stays.
        assert_eq!(
            frozen(&image),
            [115, 98, 98, 255, 115, 115, 115, 255, 0, 0, 0, 255, 79, 90, 85, 128]
        );
    }
}
