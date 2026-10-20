//! Every surface's material (sweatshop-rooms ticket 24): textured, or flat in the same
//! palette when the Textures setting is off, and dimmed on the CCTV console's dark
//! controls.
//! The corridor, the rooms, the clutter, the desk sets and the robots all go through
//! here.
//!
//! Each glTF mesh keeps its own (textured) material in [`Bright`]. What it shows is
//! worked out from that: the flat variant of it while Textures is off ([`FLAT`], one
//! small table by the glTF material's name), then the dimmed variant of that while it
//! is a dark console control ([`Dim`], set by `console`). The variants are made
//! on first use, shared, and swapped in place, so a switch applies on the next frame.
//!
//! What never swaps: the robots' faces with their names ([`FacePlate`], dressed by
//! `lettering`) and the signs' letters ([`SignLetters`]); materials the table keeps
//! ([`Flat::Keep`]: the parts already flat, glowing or not); any mesh without a glTF
//! material name; and anything not drawn with a `StandardMaterial` (the doorways'
//! portal quads). The sign plate behind the letters swaps like any other surface.

use std::collections::HashMap;

use bevy::gltf::GltfMaterialName;
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;

use super::lettering::{FacePlate, SignLetters};
use super::rooms::Detail;

/// What Textures off does to a material, by its glTF name.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Flat {
    /// Drawn in this flat colour (sRGB) with this roughness and metalness: each the
    /// mean of its baked texture (`desktop/world-assets`), so the palette stays.
    Colour {
        srgb: [f32; 3],
        roughness: f32,
        metallic: f32,
    },
    /// Never swaps.
    Keep,
}

const fn colour(srgb: [f32; 3], roughness: f32, metallic: f32) -> Flat {
    Flat::Colour {
        srgb,
        roughness,
        metallic,
    }
}

/// The flat palette: one entry per material the generated pieces use
/// (`wa_materials.py`'s recipes and its `plain` materials). For an emissive surface (the
/// PC screen) the colour is its glow's.
pub const FLAT: &[(&str, Flat)] = &[
    ("brick", colour([0.343, 0.247, 0.202], 0.92, 0.0)),
    ("concrete", colour([0.311, 0.296, 0.276], 0.78, 0.0)),
    ("ceiling", colour([0.303, 0.289, 0.266], 0.95, 0.0)),
    ("peeling_paint", colour([0.414, 0.439, 0.403], 0.85, 0.0)),
    ("laminate", colour([0.489, 0.375, 0.258], 0.45, 0.0)),
    ("rusty_steel", colour([0.458, 0.375, 0.340], 0.65, 0.49)),
    (
        "rusty_steel_dark",
        colour([0.419, 0.324, 0.282], 0.65, 0.49),
    ),
    (
        "rusty_galvanised",
        colour([0.525, 0.458, 0.423], 0.65, 0.49),
    ),
    ("rusty_can", colour([0.621, 0.259, 0.220], 0.49, 0.74)),
    ("yellowed_plastic", colour([0.655, 0.626, 0.541], 0.55, 0.0)),
    ("bezel", colour([0.422, 0.406, 0.369], 0.70, 0.0)),
    ("keyboard", colour([0.516, 0.490, 0.424], 0.50, 0.0)),
    ("pc_screen", colour([0.604, 0.714, 0.865], 0.15, 0.0)),
    ("robot_paint", colour([0.705, 0.582, 0.250], 0.55, 0.01)),
    ("robot_joint", colour([0.308, 0.307, 0.310], 0.40, 0.60)),
    ("cardboard", colour([0.573, 0.475, 0.339], 0.95, 0.0)),
    ("paper", colour([0.773, 0.755, 0.704], 0.90, 0.0)),
    ("rubber", colour([0.224, 0.219, 0.213], 0.60, 0.0)),
    // Faces and signs always render as they are (spec: "Robots and faces").
    ("rusty_sign", colour([0.479, 0.339, 0.230], 0.79, 0.26)),
    // Faces always render as they are (spec: "Robots and faces"); `lettering` gives
    // them their own material anyway.
    ("robot_face", Flat::Keep),
    // Already flat.
    ("tube_glow", Flat::Keep),
    ("tube_dead", Flat::Keep),
    ("led_green", Flat::Keep),
    ("robot_antenna_tip", Flat::Keep),
    ("dirty_water", Flat::Keep),
    ("pc_plastic", Flat::Keep),
    ("gaffer_tape", Flat::Keep),
    // The CCTV room and the punishments.
    ("cctv_wall", colour([0.113, 0.125, 0.142], 0.14, 0.03)),
    ("cctv_floor", colour([0.114, 0.120, 0.132], 0.11, 0.0)),
    ("charred_paint", colour([0.210, 0.191, 0.169], 0.90, 0.0)),
    ("fx_fire", colour([0.988, 0.640, 0.253], 1.0, 0.0)),
    ("cctv_ceiling", Flat::Keep),
    ("cctv_trim", Flat::Keep),
    ("cctv_screen", Flat::Keep),
    ("cctv_flag", Flat::Keep),
    ("cctv_sign_glass", Flat::Keep),
    ("monitor_bezel", Flat::Keep),
    ("console_metal", Flat::Keep),
    ("kill_button", Flat::Keep),
    ("kill_cover", Flat::Keep),
    ("hazard_yellow", Flat::Keep),
    ("work_harder", Flat::Keep),
    ("sit_in", Flat::Keep),
    ("camera_lens", Flat::Keep),
    ("camera_led", Flat::Keep),
    ("squad_armour", Flat::Keep),
    ("squad_joint", Flat::Keep),
    ("squad_visor", Flat::Keep),
    ("charge_led", Flat::Keep),
    ("wreck_glass", Flat::Keep),
    ("scorch", Flat::Keep),
    ("scorch_core", Flat::Keep),
    ("fx_smoke", Flat::Keep),
    ("fx_bolt", Flat::Keep),
    ("fx_spark", Flat::Keep),
];

/// The table's entry for glTF material `name`, if it has one.
pub fn flat(name: &str) -> Option<Flat> {
    FLAT.iter().find(|(n, _)| *n == name).map(|&(_, flat)| flat)
}

/// A dimmed part: colours moved this far towards grey, then darkened, and their glow
/// kept this strong.
const DESATURATE: f32 = 0.85;
const DARKEN: f32 = 0.4;
const GLOW: f32 = 0.25;

/// A mesh's own material, from its glTF scene. Whatever it shows instead is a variant
/// of this one, so a handle swap that should last goes here, not on the mesh.
#[derive(Component)]
pub struct Bright(pub Handle<StandardMaterial>);

/// Whether a mesh is dimmed: a dark control on the CCTV console (set by `console`).
#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub struct Dim(pub bool);

/// Whether a robot's part is charred black for a moment by work harder's bolt
/// (set by `punishments`). It wins over Textures and [`Dim`]:
/// the part shows [`Char`] until it is false again.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub struct Charred(pub bool);

/// What a PC's glass shows instead of its own material: its session terminal's image,
/// live or frozen (set by `pc_screens`), or nothing. It wins over Textures and [`Dim`].
#[derive(Component, Clone, Debug, Default, PartialEq)]
pub struct OnGlass(pub Option<Handle<StandardMaterial>>);

/// The charred robot's one material: near black, rough, no glow.
#[derive(Resource, Clone)]
pub struct Char(pub Handle<StandardMaterial>);

impl FromWorld for Char {
    fn from_world(world: &mut World) -> Self {
        let mut materials = world.resource_mut::<Assets<StandardMaterial>>();
        Self(materials.add(StandardMaterial {
            base_color: Color::srgb(0.03, 0.028, 0.026),
            perceptual_roughness: 0.95,
            emissive: LinearRgba::BLACK,
            ..default()
        }))
    }
}

/// The flat and dimmed variants, made on first use: of each material, and of each
/// colour texture a dimmed variant needs.
#[derive(Resource, Default)]
struct Variants {
    flat: HashMap<AssetId<StandardMaterial>, Handle<StandardMaterial>>,
    dimmed: HashMap<AssetId<StandardMaterial>, Handle<StandardMaterial>>,
    images: HashMap<AssetId<Image>, Handle<Image>>,
}

/// Swaps the materials, after whatever sets [`Dim`] or [`Bright`] this frame.
#[derive(SystemSet, Clone, Debug, PartialEq, Eq, Hash)]
pub struct SwapMaterials;

pub struct SurfacesPlugin;

impl Plugin for SurfacesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Variants>()
            .init_resource::<Char>()
            .add_systems(
                PostUpdate,
                swap_materials
                    .in_set(SwapMaterials)
                    .before(bevy::transform::TransformSystems::Propagate),
            );
    }
}

type Part = (
    &'static mut MeshMaterial3d<StandardMaterial>,
    Option<&'static Bright>,
    Option<&'static Dim>,
    Option<&'static Charred>,
    Option<&'static OnGlass>,
    &'static GltfMaterialName,
);

/// The meshes that swap: glTF ones, but never a face or a sign's letters.
type Swaps = (
    With<GltfMaterialName>,
    Without<FacePlate>,
    Without<SignLetters>,
);

/// A mesh that is new, or whose [`Bright`], [`Dim`], [`Charred`] or [`OnGlass`]
/// changed.
type Fresh = Or<(
    Added<MeshMaterial3d<StandardMaterial>>,
    Changed<Bright>,
    Changed<Dim>,
    Changed<Charred>,
    Changed<OnGlass>,
)>;

/// Gives each new mesh, each mesh whose [`Bright`], [`Dim`], [`Charred`] or [`OnGlass`] changed, and
/// every mesh when Textures changes, the material it should show.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn swap_materials(
    mut commands: Commands,
    detail: Res<Detail>,
    // The meshes that swap, and those of them that are fresh: the change filters read
    // the materials the swap writes, so the two take turns.
    mut parts: ParamSet<(Query<Part, Swaps>, Query<Entity, (Fresh, Swaps)>)>,
    everything: Query<Entity, (With<MeshMaterial3d<StandardMaterial>>, Swaps)>,
    mut variants: ResMut<Variants>,
    char: Res<Char>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut textures: Local<Option<bool>>,
) {
    // Only Textures changing redoes every mesh, not Clutter.
    let all = textures.replace(detail.textures) != Some(detail.textures);
    let which: Vec<Entity> = if all {
        everything.iter().collect()
    } else {
        parts.p1().iter().collect()
    };
    let mut parts = parts.p0();
    let mut swapped = 0;
    for entity in which {
        let Ok((mut material, bright, dim, charred, on_glass, name)) = parts.get_mut(entity) else {
            continue;
        };
        let own = match bright {
            Some(Bright(own)) => own.clone(),
            None => {
                commands.entity(entity).insert(Bright(material.0.clone()));
                material.0.clone()
            }
        };
        let base = match flat(name) {
            Some(Flat::Colour {
                srgb,
                roughness,
                metallic,
            }) if !detail.textures => {
                variants.flat(&own, srgb, roughness, metallic, &mut materials)
            }
            _ => own,
        };
        let shown = on_glass.and_then(|on| on.0.clone());
        let want = if charred.is_some_and(|c| c.0) {
            char.0.clone()
        } else if let Some(shown) = shown {
            shown
        } else if dim.is_some_and(|d| d.0) {
            variants.dimmed(&base, &mut materials, &mut images)
        } else {
            base
        };
        if material.0 != want {
            material.0 = want;
            swapped += 1;
        }
    }
    if all {
        eprintln!(
            "[world] textures {}: {swapped} surfaces swapped",
            if detail.textures { "on" } else { "off" }
        );
    }
}

impl Variants {
    /// The flat variant of `own`: no textures, in the table's colour.
    fn flat(
        &mut self,
        own: &Handle<StandardMaterial>,
        srgb: [f32; 3],
        roughness: f32,
        metallic: f32,
        materials: &mut Assets<StandardMaterial>,
    ) -> Handle<StandardMaterial> {
        if let Some(flat) = self.flat.get(&own.id()) {
            return flat.clone();
        }
        let Some(mut flat) = materials.get(own).cloned() else {
            return own.clone();
        };
        let [r, g, b] = srgb;
        let tint = Color::srgb(r, g, b).to_linear();
        if flat.emissive_texture.is_some() {
            // The glow carries the colour (the PC screen).
            flat.emissive = LinearRgba::rgb(
                flat.emissive.red * tint.red,
                flat.emissive.green * tint.green,
                flat.emissive.blue * tint.blue,
            );
        } else {
            flat.base_color = Color::LinearRgba(tint);
        }
        flat.base_color_texture = None;
        flat.emissive_texture = None;
        flat.metallic_roughness_texture = None;
        flat.normal_map_texture = None;
        flat.occlusion_texture = None;
        flat.perceptual_roughness = roughness;
        flat.metallic = metallic;
        let flat = materials.add(flat);
        self.flat.insert(own.id(), flat.clone());
        flat
    }

    /// The dimmed variant of `bright` (textured or flat), shared by every dimmed desk.
    fn dimmed(
        &mut self,
        bright: &Handle<StandardMaterial>,
        materials: &mut Assets<StandardMaterial>,
        images: &mut Assets<Image>,
    ) -> Handle<StandardMaterial> {
        if let Some(dim) = self.dimmed.get(&bright.id()) {
            return dim.clone();
        }
        let Some(mut dim) = materials.get(bright).cloned() else {
            return bright.clone();
        };
        dim.base_color = dim_colour(dim.base_color);
        dim.base_color_texture = dim.base_color_texture.map(|t| self.image(t, images));
        dim.emissive = LinearRgba::rgb(
            dim.emissive.red * GLOW,
            dim.emissive.green * GLOW,
            dim.emissive.blue * GLOW,
        );
        dim.emissive_texture = dim.emissive_texture.map(|t| self.image(t, images));
        let dim = materials.add(dim);
        self.dimmed.insert(bright.id(), dim.clone());
        dim
    }

    /// A desaturated copy of colour texture `bright` (the darkening is the material's).
    /// A texture whose pixels are not at hand stays as it is.
    fn image(&mut self, bright: Handle<Image>, images: &mut Assets<Image>) -> Handle<Image> {
        if let Some(dim) = self.images.get(&bright.id()) {
            return dim.clone();
        }
        let Some(mut copy) = images.get(&bright).cloned() else {
            return bright;
        };
        let rgba8 = matches!(
            copy.texture_descriptor.format,
            TextureFormat::Rgba8UnormSrgb | TextureFormat::Rgba8Unorm
        );
        let Some(data) = copy.data.as_mut().filter(|_| rgba8) else {
            eprintln!("[world] a desk texture cannot be dimmed: no RGBA8 pixels");
            return bright;
        };
        for pixel in data.as_chunks_mut::<4>().0 {
            let [r, g, b] = [pixel[0], pixel[1], pixel[2]].map(f32::from);
            let grey = 0.299 * r + 0.587 * g + 0.114 * b;
            for channel in &mut pixel[..3] {
                let c = f32::from(*channel);
                *channel = (c + (grey - c) * DESATURATE).round().clamp(0.0, 255.0) as u8;
            }
        }
        let dim = images.add(copy);
        self.images.insert(bright.id(), dim.clone());
        dim
    }
}

/// A colour moved towards its grey and darkened.
fn dim_colour(colour: Color) -> Color {
    let c = colour.to_linear();
    let grey = 0.2126 * c.red + 0.7152 * c.green + 0.0722 * c.blue;
    let dim = |v: f32| (v + (grey - v) * DESATURATE) * DARKEN;
    Color::LinearRgba(LinearRgba::new(
        dim(c.red),
        dim(c.green),
        dim(c.blue),
        c.alpha,
    ))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::path::Path;

    use super::*;

    /// The swap in an app of its own, with one brick wall and one dimmed robot part.
    fn world() -> (App, Entity, Entity, Handle<StandardMaterial>) {
        let mut app = App::new();
        app.init_resource::<Assets<StandardMaterial>>()
            .init_resource::<Assets<Image>>()
            .init_resource::<Detail>()
            .add_plugins(SurfacesPlugin);
        let mut materials = app.world_mut().resource_mut::<Assets<StandardMaterial>>();
        let brick = materials.add(StandardMaterial {
            base_color_texture: Some(Handle::default()),
            normal_map_texture: Some(Handle::default()),
            ..default()
        });
        let paint = materials.add(StandardMaterial::default());
        let wall = app
            .world_mut()
            .spawn((
                MeshMaterial3d(brick.clone()),
                GltfMaterialName("brick".into()),
            ))
            .id();
        let robot = app
            .world_mut()
            .spawn((
                MeshMaterial3d(paint),
                GltfMaterialName("robot_paint".into()),
                Dim(true),
            ))
            .id();
        app.update();
        (app, wall, robot, brick)
    }

    fn shown(app: &App, entity: Entity) -> StandardMaterial {
        let handle = &app
            .world()
            .get::<MeshMaterial3d<StandardMaterial>>(entity)
            .unwrap()
            .0;
        app.world()
            .resource::<Assets<StandardMaterial>>()
            .get(handle)
            .unwrap()
            .clone()
    }

    #[test]
    fn textures_off_swaps_in_flat_colours_and_back_dimmed_parts_staying_dimmed() {
        let (mut app, wall, robot, brick) = world();
        assert_eq!(
            app.world()
                .get::<MeshMaterial3d<StandardMaterial>>(wall)
                .unwrap()
                .0,
            brick
        );
        let dimmed_textured = shown(&app, robot).base_color;

        app.world_mut().resource_mut::<Detail>().textures = false;
        app.update();
        let flat_wall = shown(&app, wall);
        assert!(flat_wall.base_color_texture.is_none() && flat_wall.normal_map_texture.is_none());
        assert_eq!(flat_wall.base_color.to_srgba().red, 0.343);
        let robot_flat = shown(&app, robot).base_color.to_linear();
        let paint = Color::srgb(0.705, 0.582, 0.250).to_linear();
        // Still dimmed: darker than the flat paint, and not the textured dimming.
        assert!(robot_flat.green < paint.green * 0.5, "{robot_flat:?}");
        assert_ne!(shown(&app, robot).base_color, dimmed_textured);

        app.world_mut().resource_mut::<Detail>().textures = true;
        app.update();
        assert_eq!(
            app.world()
                .get::<MeshMaterial3d<StandardMaterial>>(wall)
                .unwrap()
                .0,
            brick
        );
        assert_eq!(shown(&app, robot).base_color, dimmed_textured);
    }

    /// Every material name the piece scripts give their models: `wa_materials.py`'s
    /// recipes (`@recipe(...)` then `def <name>(g):`) and each `wm.plain("<name>", …)`
    /// in a piece script.
    fn generated_materials() -> BTreeSet<String> {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../world-assets");
        let mut names = BTreeSet::new();
        let recipes = std::fs::read_to_string(dir.join("wa_materials.py")).unwrap();
        let mut decorated = false;
        for line in recipes.lines() {
            if line.starts_with("@recipe(") {
                decorated = true;
            } else if let Some(def) = line.strip_prefix("def ").filter(|_| decorated) {
                names.insert(def.split('(').next().unwrap().to_string());
                decorated = false;
            }
        }
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            let file = path.file_name().unwrap().to_string_lossy().into_owned();
            // The review renders' backdrop is not part of any model.
            if !file.ends_with(".py") || file.starts_with("wa_") || file == "review_renders.py" {
                continue;
            }
            let source = std::fs::read_to_string(&path).unwrap();
            for call in source.split("wm.plain(\"").skip(1) {
                names.insert(call.split('"').next().unwrap().to_string());
            }
        }
        names
    }

    #[test]
    fn the_flat_table_covers_every_material_the_pieces_use_and_nothing_else() {
        let generated = generated_materials();
        assert!(generated.len() >= 25, "found only {generated:?}");
        let table: BTreeSet<String> = FLAT.iter().map(|(n, _)| n.to_string()).collect();
        assert_eq!(table.len(), FLAT.len(), "a material is listed twice");
        assert_eq!(table, generated);
    }

    #[test]
    fn faces_and_glowing_parts_keep_their_material_and_the_sign_plate_swaps() {
        for name in ["robot_face", "tube_glow", "led_green"] {
            assert_eq!(flat(name), Some(Flat::Keep), "{name}");
        }
        for name in ["brick", "rusty_sign", "pc_screen"] {
            assert!(matches!(flat(name), Some(Flat::Colour { .. })), "{name}");
        }
        assert_eq!(flat("a material from ticket 22"), None);
    }

    #[test]
    fn flat_colours_are_in_range() {
        for (name, flat) in FLAT {
            if let Flat::Colour {
                srgb,
                roughness,
                metallic,
            } = flat
            {
                assert!(
                    srgb.iter()
                        .chain([roughness, metallic])
                        .all(|v| (0.0..=1.0).contains(v)),
                    "{name}"
                );
            }
        }
    }
}
