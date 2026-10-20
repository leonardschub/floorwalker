//! The world's lettering (sweatshop-rooms ticket 22): the sign over each doorway with its
//! project's name, and each robot's face with its company's pattern and its model's
//! name, all ours (no company logo anywhere). `names` decides what they say; this draws
//! it.
//!
//! Every text is drawn once with Bevy's 2D text into an image (render-to-texture): a
//! [`Camera2d`] of its own, far off on [`TEXT_LAYER`], renders the text (and a face's
//! rings) into the image until it is settled, then stops. Images are made the first time
//! a sign's name or a face is needed and are shared by everything that shows it, for the
//! world's life.
//!
//! - **Signs:** a `sign_plate` piece over each doorway, with a quad on its front whose
//!   material shows the name in warm glowing letters.
//! - **Faces:** a robot's `robot_face` part ([`FacePlate`], set by `rooms`' restyle)
//!   shows the plate's own smudged glass with the face image as its glow.
//!
//! - **Wordings**: the world's fixed text, such as the CCTV
//!   sign and NO SIGNAL, on any quad marked [`Lettered`]. Its words come from the page's
//!   table (`strings`), and each image is redrawn when the table changes.
//!
//! Neither material ever goes through a desk's `Bright` swap or the Textures setting:
//! faces and names always render.

use std::collections::HashMap;

use agents::FlagKind;
use bevy::camera::visibility::RenderLayers;
use bevy::camera::RenderTarget;
use bevy::gltf::GltfMaterialName;
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use bevy::sprite::Anchor;
use bevy::text::TextLayoutInfo;

use super::cctv_state::Words;
use super::corridor::Plan;
use super::floor_plan::layout;
use super::names::{self, Company, Face, Glyphs};
use super::pieces::Kit;
use super::rooms;
use super::strings::{self, WorldStrings};

/// The render layer every text and its camera are on, seen by no other camera. Rooms take
/// layers from 1 up by slot, so this stays clear of them.
const TEXT_LAYER: usize = 1000;
/// How far apart the texts stand on their layer, so no camera sees another's.
const TEXT_SPACING: f32 = 4096.0;
/// Updates a text's camera keeps drawing after its text is laid out, while the world
/// draws, before it stops. Generous: a face's rings need the 2D mesh pipeline, which
/// compiles on its own time the first time (on software Vulkan, 20 updates were too few
/// and some faces kept only their names).
const SETTLE_FRAMES: u32 = 300;

/// Oswald Bold.
const WEIGHT: f32 = 700.0;

/// The sign's image: the plate's usable 1.6 × 0.24 m (`sign_plate.py`'s `sign_text`) at
/// 640 px per metre.
const SIGN_IMAGE: UVec2 = UVec2::new(1024, 154);
const SIGN_TEXT: Vec2 = Vec2::new(1.6, 0.24);
/// The name's width at most, in image pixels, and its size: as large as fits up to the
/// first, down to the second, then cut.
const SIGN_WIDTH: f32 = 984.0;
const SIGN_SIZES: (f32, f32) = (64.0, 104.0);
/// The sign's centre in the corridor frame, above its doorway's centre, and the text's
/// distance in front of the plate's back (its front face is at 0.04, the `sign_text`
/// marker at 0.041).
const SIGN_HEIGHT: f32 = 2.45;
const SIGN_FRONT: f32 = 0.043;
/// The letters' warm light: their colour, and their glow.
const SIGN_COLOUR: Color = Color::srgb(1.0, 0.62, 0.25);
const SIGN_GLOW: LinearRgba = LinearRgba::rgb(2.4, 1.1, 0.35);

/// The face's image: the plate's 0.16 × 0.13 m front (its UVs fill 0..1) at 1600 px per
/// metre.
const FACE_IMAGE: UVec2 = UVec2::new(256, 208);
/// The pattern, concentric rings and a dot above the middle; then the name below it,
/// at most this wide and this large, cut with "…".
const RINGS: [(f32, f32); 3] = [(54.0, 64.0), (34.0, 44.0), (16.0, 24.0)];
const DOT: f32 = 7.0;
const RINGS_Y: f32 = 26.0;
const NAME_Y: f32 = -72.0;
const NAME_WIDTH: f32 = 232.0;
const NAME_SIZE: f32 = 40.0;
/// How strongly a face glows (the PC screen glows at 1.4).
const FACE_GLOW: f32 = 1.6;

/// A robot's face part, and what it is to show; `rooms` sets it on each restyle.
#[derive(Component, Clone, PartialEq)]
pub struct FacePlate {
    pub face: Face,
}

/// A sign's lettered quad.
#[derive(Component)]
pub struct SignLetters;

/// A fixed text of the world, drawn in its own style from the page's table.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Wording {
    /// The sign over the CCTV doorway, in the CCTV room's cyan.
    CctvSign,
    /// The monitor wall's screen with no feed.
    NoSignal,
    /// A flagged screen's tag bar: the flag's words in dark
    /// ink on the glowing bar.
    Flag(FlagKind),
    /// The labels in front of the console's controls.
    Kill,
    WorkHarder,
    SitIn,
}

/// How a wording is drawn: its key in the page's table, its image, the widest its words
/// may be in that image and the sizes they are fitted between, their colour and glow.
struct Style {
    key: &'static str,
    image: UVec2,
    width: f32,
    sizes: (f32, f32),
    colour: Color,
    glow: LinearRgba,
}

impl Wording {
    fn style(self) -> Style {
        match self {
            // The plate's usable 0.9 × 0.18 m (`cctv_sign_plate.py`) at 640 px per metre.
            Wording::CctvSign => Style {
                key: strings::CCTV_SIGN,
                image: UVec2::new(576, 115),
                width: 540.0,
                sizes: (48.0, 84.0),
                colour: Color::srgb(0.0, 0.85, 1.0),
                glow: LinearRgba::rgb(0.0, 1.9, 2.4),
            },
            // A feed screen's 1.2 × 0.675 m at about 533 px per metre.
            Wording::NoSignal => Style {
                key: strings::NO_SIGNAL,
                image: UVec2::new(640, 360),
                width: 520.0,
                sizes: (36.0, 64.0),
                colour: Color::srgb(0.55, 0.9, 1.0),
                glow: LinearRgba::rgb(0.45, 0.8, 0.95),
            },
            // A feed's tag bar's usable 0.86 × 0.045 m (`cctv_monitors.py`) at about 890
            // px per metre; a focus monitor's tag shows the same image larger.
            Wording::Flag(kind) => Style {
                key: strings::key(Words::Flag(kind)),
                image: UVec2::new(768, 40),
                width: 740.0,
                sizes: (22.0, 34.0),
                colour: Color::srgb(0.05, 0.03, 0.0),
                glow: LinearRgba::BLACK,
            },
            // A label's usable 0.36 × 0.06 m (`cctv_console.py`) at about 1070 px per
            // metre, printed pale on the dark slab.
            Wording::Kill | Wording::WorkHarder => Style {
                key: if self == Wording::Kill {
                    strings::KILL
                } else {
                    strings::WORK_HARDER
                },
                image: UVec2::new(384, 64),
                width: 370.0,
                sizes: (26.0, 46.0),
                colour: Color::srgb(0.7, 0.8, 0.85),
                glow: LinearRgba::rgb(0.25, 0.32, 0.36),
            },
            // SIT IN's narrower 0.20 × 0.06 m, at the same scale and in the same print.
            Wording::SitIn => Style {
                key: strings::SIT_IN,
                image: UVec2::new(213, 64),
                width: 200.0,
                sizes: (26.0, 46.0),
                colour: Color::srgb(0.7, 0.8, 0.85),
                glow: LinearRgba::rgb(0.25, 0.32, 0.36),
            },
        }
    }

    /// The quad it is drawn on, in metres.
    pub fn size(self) -> Vec2 {
        match self {
            Wording::CctvSign => Vec2::new(0.9, 0.18),
            Wording::NoSignal => Vec2::new(1.2, 0.675),
            Wording::Flag(_) => Vec2::new(0.86, 0.86 * 40.0 / 768.0),
            Wording::Kill | Wording::WorkHarder => Vec2::new(0.36, 0.06),
            Wording::SitIn => Vec2::new(0.20, 0.06),
        }
    }
}

/// A quad showing a wording: `lettering` gives it its material.
#[derive(Component, Clone, Copy, Debug)]
pub struct Lettered(pub Wording);

/// A camera feed's overlay text, white on clear and laid
/// out from the left, for the monitor's shader to tint and place: the room's name
/// bottom-left and its CAM number top-left; on a focus monitor also the
/// model and agent ("OPUS-5.5 / root").
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Overlay {
    FeedName,
    FeedCam,
    FocusWho,
}

impl Overlay {
    /// Its image, and the sizes its words are fitted between.
    fn image(self) -> UVec2 {
        match self {
            Overlay::FeedName | Overlay::FocusWho => UVec2::new(512, 44),
            Overlay::FeedCam => UVec2::new(192, 44),
        }
    }

    const SIZES: (f32, f32) = (24.0, 34.0);
    const PAD: f32 = 4.0;
}

/// A text that keeps changing, such as a flag's timer: one
/// image each, white on clear, redrawn whenever its words change, for a shader to tint
/// and place. A focus monitor's flag and timer, laid out from the right; the ticker's
/// line of every flagged agent, laid out from the left, which its shader scrolls.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Live {
    FocusStatus(usize),
    Ticker,
    /// The console's heading, each of its rows, and how many more do not fit
    ///, laid out from the left but the count.
    ConsoleHeading,
    ConsoleRow(usize),
    ConsoleMore,
}

impl Live {
    fn image(self) -> UVec2 {
        match self {
            Live::FocusStatus(_) => UVec2::new(512, 44),
            // As wide as the GPU surely allows: a dozen or more entries at full size.
            Live::Ticker => UVec2::new(8192, 40),
            Live::ConsoleHeading | Live::ConsoleRow(_) => UVec2::new(1024, 48),
            Live::ConsoleMore => UVec2::new(192, 48),
        }
    }

    fn sizes(self) -> (f32, f32) {
        match self {
            Live::FocusStatus(_) => (20.0, 28.0),
            Live::Ticker => (16.0, 28.0),
            Live::ConsoleHeading | Live::ConsoleMore => (24.0, 38.0),
            Live::ConsoleRow(_) => (20.0, 34.0),
        }
    }

    fn right_aligned(self) -> bool {
        matches!(self, Live::FocusStatus(_) | Live::ConsoleMore)
    }
}

/// A live text's image, and how wide its words are drawn in it, in pixels from its
/// padded edge.
#[derive(Clone, Debug)]
pub struct LiveImage {
    pub image: Handle<Image>,
    pub width: f32,
}

/// A live text: its image, its text and camera, the words it should show (fitted, with
/// their size) and the words it shows.
struct LiveSlot {
    image: Handle<Image>,
    text: Entity,
    camera: Entity,
    want: (String, f32),
    shown: (String, f32),
}

/// Updates a live text's camera keeps drawing after a change of words, once its first
/// words have settled.
const LIVE_FRAMES: u32 = 4;
/// How far apart the live texts' rows are on [`TEXT_LAYER`], below the other texts.
const LIVE_ROWS: f32 = 4096.0;

/// Where callers ask for live texts' words, before they are drawn (`apply_live`).
#[derive(SystemSet, Clone, Debug, PartialEq, Eq, Hash)]
pub struct LiveTexts;

/// What a text image shows.
#[derive(Clone, PartialEq, Eq, Hash)]
enum Label {
    Sign(String),
    Face(Face),
    Overlay(Overlay, String),
}

/// A text image's camera, until its text is settled.
#[derive(Component)]
struct LabelCamera {
    /// The text it draws; none for a face with no name.
    text: Option<Entity>,
    /// Updates drawn since the text was laid out.
    settled: u32,
}

/// Everything lettered so far, shared by every sign and face that shows it.
#[derive(Resource, Default)]
pub struct Lettering {
    glyphs: Option<Glyphs>,
    images: HashMap<Label, Handle<Image>>,
    /// The face plate's own glass material from the robot's glTF, once one is seen.
    glass: Option<Handle<StandardMaterial>>,
    /// Each face's material.
    faces: HashMap<Face, Handle<StandardMaterial>>,
    signs: HashMap<String, Handle<StandardMaterial>>,
    sign_quad: Option<Handle<Mesh>>,
    /// Each wording's material, and the text and camera that draw its image.
    wordings: HashMap<Wording, (Handle<StandardMaterial>, Entity, Entity)>,
    live: HashMap<Live, LiveSlot>,
}

/// The signs over the doorways, in doorway order: each one's name and entity.
#[derive(Resource, Default)]
struct Signs(Vec<(String, Entity)>);

pub struct LetteringPlugin;

impl Plugin for LetteringPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Lettering>()
            .init_resource::<Signs>()
            .init_resource::<WorldStrings>()
            .add_systems(
                Update,
                (
                    (read_font, hang_signs, dress_lettered, reword).chain(),
                    apply_live.after(LiveTexts),
                ),
            )
            .add_systems(Last, settle_labels)
            // Right after a desk's restyle sets its face, before anything is drawn.
            .add_systems(
                PostUpdate,
                dress_faces
                    .after(rooms::Restyled)
                    .before(bevy::transform::TransformSystems::Propagate),
            );
    }
}

/// Reads the font's glyphs once the kit (with the font) is in.
fn read_font(kit: Option<Res<Kit>>, fonts: Res<Assets<Font>>, mut lettering: ResMut<Lettering>) {
    let Some(kit) = kit else { return };
    if lettering.glyphs.is_some() {
        return;
    }
    let Some(font) = fonts.get(&kit.font) else {
        return;
    };
    lettering.glyphs = Glyphs::new(font.data.as_ref().to_vec(), WEIGHT);
    if lettering.glyphs.is_none() {
        eprintln!("[world] the sign font cannot be read; signs and names stay blank");
    }
}

/// Hangs a sign over each doorway with its room's project name, when the kit is in and
/// whenever a doorway's name changes.
#[allow(clippy::too_many_arguments)]
fn hang_signs(
    mut commands: Commands,
    kit: Option<Res<Kit>>,
    plan: Res<Plan>,
    mut signs: ResMut<Signs>,
    mut lettering: ResMut<Lettering>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut colours: ResMut<Assets<ColorMaterial>>,
) {
    let Some(kit) = kit else { return };
    if lettering.glyphs.is_none() || (!plan.is_changed() && !signs.0.is_empty()) {
        return;
    }
    let names: Vec<&str> = plan.0.rooms.iter().map(|r| r.name.as_str()).collect();
    let kept = names.len().min(signs.0.len());
    for (_, gone) in signs.0.drain(kept..) {
        commands.entity(gone).despawn();
    }
    for (n, name) in names.into_iter().enumerate() {
        if signs.0.get(n).is_some_and(|(had, _)| had == name) {
            continue;
        }
        let material = lettering.sign_material(
            name,
            &kit,
            &mut commands,
            (&mut images, &mut meshes, &mut materials, &mut colours),
        );
        let quad = lettering
            .sign_quad
            .get_or_insert_with(|| meshes.add(Rectangle::new(SIGN_TEXT.x, SIGN_TEXT.y)))
            .clone();
        let sign = commands
            .spawn((
                kit.piece("sign_plate"),
                Transform::from_xyz(layout::doorway_x(n), SIGN_HEIGHT, 0.0),
            ))
            .with_children(|sign| {
                sign.spawn((
                    SignLetters,
                    Mesh3d(quad),
                    MeshMaterial3d(material),
                    Transform::from_xyz(0.0, 0.0, SIGN_FRONT),
                ));
            })
            .id();
        match signs.0.get_mut(n) {
            Some(slot) => {
                commands.entity(slot.1).despawn();
                *slot = (name.to_string(), sign);
            }
            None => signs.0.push((name.to_string(), sign)),
        }
        eprintln!("[world] sign {n}: {name:?}");
    }
}

/// Gives each quad marked [`Lettered`] its wording's material, once the font is read.
fn dress_lettered(
    mut commands: Commands,
    kit: Option<Res<Kit>>,
    quads: Query<(Entity, &Lettered), Without<MeshMaterial3d<StandardMaterial>>>,
    strings: Res<WorldStrings>,
    mut lettering: ResMut<Lettering>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let Some(kit) = kit else { return };
    if lettering.glyphs.is_none() {
        return;
    }
    for (entity, lettered) in &quads {
        let material = lettering.wording_material(
            lettered.0,
            &strings,
            &kit,
            &mut commands,
            (&mut images, &mut materials),
        );
        commands.entity(entity).insert(MeshMaterial3d(material));
    }
}

/// Redraws every wording's image when the page's table changes: new words, fitted again,
/// and its camera drawing until they settle.
fn reword(
    strings: Res<WorldStrings>,
    lettering: Res<Lettering>,
    mut texts: Query<(&mut Text2d, &mut TextFont)>,
    mut cameras: Query<(&mut Camera, &mut LabelCamera)>,
) {
    if !strings.is_changed() || strings.is_added() {
        return;
    }
    let Some(glyphs) = &lettering.glyphs else {
        return;
    };
    for (&wording, &(_, text, camera)) in &lettering.wordings {
        let (words, px) = fitted(wording, &strings, glyphs);
        if let Ok((mut shown, mut font)) = texts.get_mut(text) {
            if shown.0 != words {
                shown.0 = words.clone();
            }
            font.font_size = FontSize::Px(px);
        }
        if let Ok((mut camera, mut label)) = cameras.get_mut(camera) {
            camera.is_active = true;
            label.settled = 0;
            label.text = (!words.is_empty()).then_some(text);
        }
        eprintln!("[world] reworded {wording:?}: {words:?}");
    }
}

/// Gives each live text the words it was last asked for, and its camera a few frames to
/// draw them.
fn apply_live(
    mut lettering: ResMut<Lettering>,
    mut texts: Query<(&mut Text2d, &mut TextFont)>,
    mut cameras: Query<(&mut Camera, &mut LabelCamera)>,
) {
    for slot in lettering.live.values_mut() {
        if slot.want == slot.shown {
            continue;
        }
        let (words, px) = slot.want.clone();
        if let Ok((mut shown, mut font)) = texts.get_mut(slot.text) {
            shown.0.clone_from(&words);
            font.font_size = FontSize::Px(px);
        }
        if let Ok((mut camera, mut label)) = cameras.get_mut(slot.camera) {
            camera.is_active = true;
            // At least a few more frames, and the first words' whole settling.
            label.settled = label.settled.min(SETTLE_FRAMES - LIVE_FRAMES);
            label.text = (!words.is_empty()).then_some(slot.text);
        }
        slot.shown = (words, px);
    }
}

/// A wording's words from the table, as the font can draw them, fitted to its image.
fn fitted(wording: Wording, strings: &WorldStrings, glyphs: &Glyphs) -> (String, f32) {
    let style = wording.style();
    let words = names::known(strings.get(style.key), |c| glyphs.has(c));
    names::fit(&words, style.width, style.sizes, |s| glyphs.width(s))
}

/// Gives each robot face the material for its face.
#[allow(clippy::too_many_arguments)]
fn dress_faces(
    mut commands: Commands,
    kit: Option<Res<Kit>>,
    mut plates: Query<(&FacePlate, &mut MeshMaterial3d<StandardMaterial>), Changed<FacePlate>>,
    mut lettering: ResMut<Lettering>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut colours: ResMut<Assets<ColorMaterial>>,
) {
    let Some(kit) = kit else { return };
    for (plate, mut material) in &mut plates {
        // Every robot's face starts with the same glass from the glTF.
        let glass = lettering
            .glass
            .get_or_insert_with(|| material.0.clone())
            .clone();
        let want = lettering.face_material(
            plate,
            &glass,
            &kit,
            &mut commands,
            (&mut images, &mut meshes, &mut materials, &mut colours),
        );
        if material.0 != want {
            material.0 = want;
        }
    }
}

/// Stops each text's camera once its text has been drawn for a few frames, so a settled
/// image costs nothing more. Updates count only while the world draws.
fn settle_labels(
    mut cameras: Query<(&mut Camera, &mut LabelCamera)>,
    world: Query<&Camera, (With<Camera3d>, Without<LabelCamera>)>,
    texts: Query<&TextLayoutInfo>,
) {
    if !world.iter().any(|camera| camera.is_active) {
        return;
    }
    for (mut camera, mut label) in &mut cameras {
        if !camera.is_active {
            continue;
        }
        let laid_out = label
            .text
            .is_none_or(|text| texts.get(text).is_ok_and(|t| !t.glyphs.is_empty()));
        if laid_out {
            label.settled += 1;
        }
        if label.settled >= SETTLE_FRAMES {
            camera.is_active = false;
        }
    }
}

type Stores<'a> = (
    &'a mut Assets<Image>,
    &'a mut Assets<Mesh>,
    &'a mut Assets<StandardMaterial>,
    &'a mut Assets<ColorMaterial>,
);

impl Lettering {
    /// The image of a feed's overlay text, drawn the first time it is asked for; none
    /// until the font is read.
    pub fn overlay(
        &mut self,
        overlay: Overlay,
        text: &str,
        kit: &Kit,
        commands: &mut Commands,
        stores: (
            &mut Assets<Image>,
            &mut Assets<Mesh>,
            &mut Assets<ColorMaterial>,
        ),
    ) -> Option<Handle<Image>> {
        self.glyphs.as_ref()?;
        Some(self.image(
            Label::Overlay(overlay, text.to_string()),
            kit,
            commands,
            stores,
        ))
    }

    /// The image of live text `which`, to show `words` (fitted to it) from now on; made
    /// the first time it is asked for. None until the font is read.
    pub fn live(
        &mut self,
        which: Live,
        words: &str,
        kit: &Kit,
        commands: &mut Commands,
        images: &mut Assets<Image>,
    ) -> Option<LiveImage> {
        let glyphs = self.glyphs.as_ref()?;
        let size = which.image();
        let words = names::known(words, |c| glyphs.has(c));
        let (words, px) = names::fit(
            &words,
            size.x as f32 - 2.0 * Overlay::PAD,
            which.sizes(),
            |s| glyphs.width(s),
        );
        let width = glyphs.width(&words) * px;
        if let Some(slot) = self.live.get_mut(&which) {
            slot.want = (words, px);
            return Some(LiveImage {
                image: slot.image.clone(),
                width,
            });
        }
        let image = images.add(Image::new_target_texture(
            size.x,
            size.y,
            TextureFormat::Rgba8UnormSrgb,
            None,
        ));
        // A row of its own below everything else, as wide as its image: the ticker's is
        // wider than the other texts' spacing.
        let at = Vec3::NEG_Y * LIVE_ROWS * (self.live.len() + 1) as f32;
        let layer = RenderLayers::from_layers(&[TEXT_LAYER]);
        let edge = size.x as f32 / 2.0 - Overlay::PAD;
        let (x, anchor) = if which.right_aligned() {
            (edge, Anchor::CENTER_RIGHT)
        } else {
            (-edge, Anchor::CENTER_LEFT)
        };
        let text = commands
            .spawn((
                Text2d::new(words.clone()),
                TextFont {
                    font: FontSource::Handle(kit.font.clone()),
                    font_size: FontSize::Px(px),
                    weight: FontWeight(WEIGHT as u16),
                    ..default()
                },
                TextColor(Color::WHITE),
                anchor,
                Transform::from_translation(at + Vec3::X * x),
                layer.clone(),
            ))
            .id();
        let camera = commands
            .spawn((
                Camera2d,
                Camera {
                    order: -1,
                    clear_color: ClearColorConfig::Custom(Color::NONE),
                    ..default()
                },
                RenderTarget::Image(image.clone().into()),
                layer,
                Transform::from_translation(at),
                LabelCamera {
                    text: (!words.is_empty()).then_some(text),
                    settled: 0,
                },
            ))
            .id();
        self.live.insert(
            which,
            LiveSlot {
                image: image.clone(),
                text,
                camera,
                want: (words.clone(), px),
                shown: (words, px),
            },
        );
        Some(LiveImage { image, width })
    }

    /// The material of a sign showing `name`.
    fn sign_material(
        &mut self,
        name: &str,
        kit: &Kit,
        commands: &mut Commands,
        stores: Stores,
    ) -> Handle<StandardMaterial> {
        if let Some(material) = self.signs.get(name) {
            return material.clone();
        }
        let (images, meshes, materials, colours) = stores;
        let image = self.image(
            Label::Sign(name.to_string()),
            kit,
            commands,
            (images, meshes, colours),
        );
        let material = materials.add(StandardMaterial {
            base_color: SIGN_COLOUR,
            base_color_texture: Some(image.clone()),
            emissive: SIGN_GLOW,
            emissive_texture: Some(image),
            perceptual_roughness: 0.6,
            // The image is drawn over a clear background: letters only.
            alpha_mode: AlphaMode::Premultiplied,
            ..default()
        });
        self.signs.insert(name.to_string(), material.clone());
        material
    }

    /// How many images are drawn, each in its own place on [`TEXT_LAYER`].
    fn drawn(&self) -> usize {
        self.images.len() + self.wordings.len()
    }

    /// The material of a quad showing `wording`, its image drawn by a camera of its own
    /// the first time it is asked for.
    fn wording_material(
        &mut self,
        wording: Wording,
        strings: &WorldStrings,
        kit: &Kit,
        commands: &mut Commands,
        (images, materials): (&mut Assets<Image>, &mut Assets<StandardMaterial>),
    ) -> Handle<StandardMaterial> {
        if let Some((material, _, _)) = self.wordings.get(&wording) {
            return material.clone();
        }
        let glyphs = self
            .glyphs
            .as_ref()
            .expect("read before anything is lettered");
        let style = wording.style();
        let (words, px) = fitted(wording, strings, glyphs);
        let image = images.add(Image::new_target_texture(
            style.image.x,
            style.image.y,
            TextureFormat::Rgba8UnormSrgb,
            None,
        ));
        let at = Vec3::X * TEXT_SPACING * self.drawn() as f32;
        let layer = RenderLayers::from_layers(&[TEXT_LAYER]);
        let text = commands
            .spawn((
                Text2d::new(words.clone()),
                TextFont {
                    font: FontSource::Handle(kit.font.clone()),
                    font_size: FontSize::Px(px),
                    weight: FontWeight(WEIGHT as u16),
                    ..default()
                },
                TextColor(Color::WHITE),
                Transform::from_translation(at),
                layer.clone(),
            ))
            .id();
        let camera = commands
            .spawn((
                Camera2d,
                Camera {
                    order: -1,
                    clear_color: ClearColorConfig::Custom(Color::NONE),
                    ..default()
                },
                RenderTarget::Image(image.clone().into()),
                layer,
                Transform::from_translation(at),
                LabelCamera {
                    text: (!words.is_empty()).then_some(text),
                    settled: 0,
                },
            ))
            .id();
        let material = materials.add(StandardMaterial {
            base_color: style.colour,
            base_color_texture: Some(image.clone()),
            emissive: style.glow,
            emissive_texture: Some(image.clone()),
            perceptual_roughness: 0.6,
            alpha_mode: AlphaMode::Premultiplied,
            ..default()
        });
        self.wordings
            .insert(wording, (material.clone(), text, camera));
        eprintln!("[world] lettered {wording:?}: {words:?}");
        material
    }

    /// The material of a face plate showing `plate`: its glass, glowing with the face's
    /// image.
    fn face_material(
        &mut self,
        plate: &FacePlate,
        glass: &Handle<StandardMaterial>,
        kit: &Kit,
        commands: &mut Commands,
        stores: Stores,
    ) -> Handle<StandardMaterial> {
        if let Some(material) = self.faces.get(&plate.face) {
            return material.clone();
        }
        let (images, meshes, materials, colours) = stores;
        let image = self.image(
            Label::Face(plate.face.clone()),
            kit,
            commands,
            (images, meshes, colours),
        );
        let mut material = materials.get(glass).cloned().unwrap_or_default();
        material.emissive = LinearRgba::rgb(FACE_GLOW, FACE_GLOW, FACE_GLOW);
        material.emissive_texture = Some(image);
        let material = materials.add(material);
        self.faces.insert(plate.face.clone(), material.clone());
        material
    }

    /// The image of `label`, drawn by a camera of its own the first time it is asked for.
    fn image(
        &mut self,
        label: Label,
        kit: &Kit,
        commands: &mut Commands,
        (images, meshes, colours): (
            &mut Assets<Image>,
            &mut Assets<Mesh>,
            &mut Assets<ColorMaterial>,
        ),
    ) -> Handle<Image> {
        if let Some(image) = self.images.get(&label) {
            return image.clone();
        }
        let glyphs = self
            .glyphs
            .as_ref()
            .expect("read before anything is lettered");
        let size = match &label {
            Label::Sign(_) => SIGN_IMAGE,
            Label::Face(_) => FACE_IMAGE,
            Label::Overlay(overlay, _) => overlay.image(),
        };
        let image = images.add(Image::new_target_texture(
            size.x,
            size.y,
            TextureFormat::Rgba8UnormSrgb,
            None,
        ));
        let at = Vec3::X * TEXT_SPACING * self.drawn() as f32;
        // Beyond what `RenderLayers::layer` builds as a constant (layers below 64).
        let layer = RenderLayers::from_layers(&[TEXT_LAYER]);
        let text = |words: String, px: f32, colour: Color, y: f32| {
            (
                Text2d::new(words),
                TextFont {
                    font: FontSource::Handle(kit.font.clone()),
                    font_size: FontSize::Px(px),
                    weight: FontWeight(WEIGHT as u16),
                    ..default()
                },
                TextColor(colour),
                Transform::from_translation(at + Vec3::Y * y),
                layer.clone(),
            )
        };
        let mut drawn = None;
        match &label {
            Label::Sign(name) => {
                let name = names::known(name, |c| glyphs.has(c));
                let (name, px) = names::fit(&name, SIGN_WIDTH, SIGN_SIZES, |s| glyphs.width(s));
                let y = 0.0;
                drawn = Some(commands.spawn(text(name, px, Color::WHITE, y)).id());
            }
            Label::Overlay(overlay, words) => {
                let width = overlay.image().x as f32 - 2.0 * Overlay::PAD;
                let words = names::known(words, |c| glyphs.has(c));
                let (words, px) = names::fit(&words, width, Overlay::SIZES, |s| glyphs.width(s));
                let left = Vec3::X * (Overlay::PAD - overlay.image().x as f32 / 2.0);
                let mut words = text(words, px, Color::WHITE, 0.0);
                words.3.translation += left;
                drawn = Some(commands.spawn((words, Anchor::CENTER_LEFT)).id());
            }
            Label::Face(face) => {
                let [r, g, b] = face.company.colour();
                let colour = Color::srgb(r, g, b);
                let paint = colours.add(ColorMaterial::from_color(colour));
                for (inner, outer) in RINGS {
                    commands.spawn((
                        Mesh2d(meshes.add(Annulus::new(inner, outer))),
                        MeshMaterial2d(paint.clone()),
                        Transform::from_translation(at + Vec3::Y * RINGS_Y),
                        layer.clone(),
                    ));
                }
                commands.spawn((
                    Mesh2d(meshes.add(Circle::new(DOT))),
                    MeshMaterial2d(paint),
                    Transform::from_translation(at + Vec3::Y * RINGS_Y),
                    layer.clone(),
                ));
                if let Some(name) = &face.name {
                    let name = names::known(name, |c| glyphs.has(c));
                    let name = names::cut(&name, NAME_WIDTH, |s| glyphs.width(s) * NAME_SIZE);
                    drawn = Some(commands.spawn(text(name, NAME_SIZE, colour, NAME_Y)).id());
                }
                let company = match face.company {
                    Company::Anthropic => "Anthropic",
                    Company::OpenAi => "OpenAI",
                    Company::XAi => "xAI",
                    Company::Google => "Google",
                    Company::Unknown => "unknown",
                };
                eprintln!("[world] face lettered: {company} {:?}", face.name);
            }
        }
        commands.spawn((
            Camera2d,
            Camera {
                // Before the world's camera, which shows the image the same frame.
                order: -1,
                clear_color: ClearColorConfig::Custom(Color::NONE),
                ..default()
            },
            RenderTarget::Image(image.clone().into()),
            layer,
            Transform::from_translation(at),
            LabelCamera {
                text: drawn,
                settled: 0,
            },
        ));
        self.images.insert(label, image.clone());
        image
    }
}

/// Whether `entity` is a robot's face part: the mesh with the glTF's `robot_face`
/// material.
pub fn is_face(names: &Query<&GltfMaterialName>, entity: Entity) -> bool {
    names.get(entity).is_ok_and(|name| name.0 == "robot_face")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_wording_is_a_key_of_the_pages_english_table_and_fits_its_quad() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packages/i18n/src/en.ts");
        let english = std::fs::read_to_string(path).unwrap();
        let wordings = [
            Wording::CctvSign,
            Wording::NoSignal,
            Wording::Flag(FlagKind::WaitingOnYou),
            Wording::Flag(FlagKind::AskingYouSomething),
            Wording::Kill,
            Wording::WorkHarder,
            Wording::SitIn,
        ];
        for wording in wordings {
            let style = wording.style();
            assert!(
                english.contains(&format!("'{}':", style.key)),
                "{wording:?}: {}",
                style.key
            );
            // The image keeps its quad's shape, within a pixel.
            let size = wording.size();
            let tall = style.image.x as f32 * size.y / size.x;
            assert!((tall - style.image.y as f32).abs() <= 1.0, "{wording:?}");
            assert!(style.width < style.image.x as f32, "{wording:?}");
        }
        // SIT IN's label fits the 0.20 m between the glass and KILL's (`cctv_console.py`).
        assert!(Wording::SitIn.size().x <= 0.20);
    }
}
