//! How the world view is drawn, for speed on the owner's laptop (sweatshop-rooms ticket
//! 27): MSAA, a depth prepass, how far a room's tube lights reach, and the render scale
//! with the presenter that stretches a scaled view over the world view. The defaults are
//! what was measured to keep the look at the lowest cost; a debug build can override each
//! one (`WORLD_DEBUG_*`) to measure it.

use bevy::camera::visibility::RenderLayers;
use bevy::camera::{RenderTarget, ScalingMode, Viewport};
use bevy::light::cluster::{ClusterConfig, ClusterFarZMode, ClusterZConfig};
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use bevy::render::view::{Msaa, Tonemapping};
use bevy::window::WindowRef;

/// The presenter's own render layer: only the scaled world view's sprite is on it.
const PRESENT_LAYER: usize = 1001;

/// The 2D camera that shows the main camera's scaled image in the world view, below full
/// render scale. It draws while the main camera does (`engine::set_drawing`).
#[derive(Component)]
pub struct Presenter;

/// The scaled world view: the image the main camera draws into, the sprite and camera
/// that show it, and the size it is drawn at (the world view's own below full scale).
#[derive(Resource, Default)]
pub struct ScaledView {
    image: Option<Handle<Image>>,
    sprite: Option<Entity>,
    /// The main camera's drawn size, in pixels.
    pub drawn: UVec2,
    /// The world view rectangle on the window last framed, and the scale.
    framed: Option<(UVec2, UVec2, u32)>,
}

/// The size the main camera draws a world view of `size` pixels at, at `scale`.
pub fn drawn_size(size: UVec2, scale: f32) -> UVec2 {
    if scale >= 1.0 {
        return size;
    }
    (size.as_vec2() * scale).round().as_uvec2().max(UVec2::ONE)
}

/// Frames the main camera for the world view rectangle `rect` (position and size on the
/// window, physical pixels). At full scale the camera draws straight into the window at
/// the rectangle. Below it, the camera draws into an image of [`drawn_size`] and the
/// presenter, a 2D camera at the rectangle, stretches the image over it with linear
/// filtering. True when anything changed.
pub fn frame_view(
    commands: &mut Commands,
    images: &mut Assets<Image>,
    scaled: &mut ScaledView,
    scale: f32,
    rect: (UVec2, UVec2),
    (camera, target): (&mut Camera, &mut RenderTarget),
    presenter: Option<&mut Camera>,
) -> bool {
    let key = (rect.0, rect.1, scale.to_bits());
    if scaled.framed == Some(key) {
        return false;
    }
    scaled.framed = Some(key);
    let viewport = |position, size| {
        Some(Viewport {
            physical_position: position,
            physical_size: size,
            ..default()
        })
    };
    if scale >= 1.0 {
        scaled.drawn = rect.1;
        camera.viewport = viewport(rect.0, rect.1);
        return true;
    }
    let size = drawn_size(rect.1, scale);
    scaled.drawn = size;
    if scaled
        .image
        .as_ref()
        .and_then(|i| images.get(i))
        .map(|i| i.size())
        != Some(size)
    {
        let image = images.add(Image::new_target_texture(
            size.x,
            size.y,
            TextureFormat::Bgra8UnormSrgb,
            None,
        ));
        *target = RenderTarget::Image(image.clone().into());
        let sprite = (
            Sprite {
                image: image.clone(),
                custom_size: Some(Vec2::ONE),
                ..default()
            },
            RenderLayers::from_layers(&[PRESENT_LAYER]),
        );
        match scaled.sprite {
            Some(entity) => {
                commands.entity(entity).insert(sprite);
            }
            None => scaled.sprite = Some(commands.spawn(sprite).id()),
        }
        scaled.image = Some(image);
    }
    camera.viewport = viewport(UVec2::ZERO, size);
    let shown = viewport(rect.0, rect.1);
    match presenter {
        Some(presenter) => presenter.viewport = shown,
        None => {
            commands.spawn((
                Camera2d,
                Camera {
                    // After the world's camera, whose image it shows.
                    order: 1,
                    is_active: camera.is_active,
                    viewport: shown,
                    ..default()
                },
                RenderTarget::Window(WindowRef::Primary),
                // The image is the main camera's, tonemapped already.
                Tonemapping::None,
                Msaa::Off,
                Projection::Orthographic(OrthographicProjection {
                    scaling_mode: ScalingMode::Fixed {
                        width: 1.0,
                        height: 1.0,
                    },
                    ..OrthographicProjection::default_2d()
                }),
                RenderLayers::from_layers(&[PRESENT_LAYER]),
                Presenter,
            ));
        }
    }
    true
}

/// How far every 3D camera's light clusters reach, in metres: beyond the deepest room (11
/// rows, 27 m) and a tube's reach, seen from its far corner.
const CLUSTER_FAR: f32 = 60.0;

/// The light clusters of every 3D camera of the world: Bevy's default, but reaching
/// [`CLUSTER_FAR`] instead of the farthest light seen on the previous frame. A camera that
/// was off for a tick (the world simulates without drawing while it waits for the
/// compositor, and a doorway's camera is off while it is not live) saw no lights then, so
/// on its next frames everything further than its first 5 m slice went unlit: the far end
/// of a room and the doorway's view flickered dark (ticket 27; before ticket 27 too).
pub fn clusters() -> ClusterConfig {
    ClusterConfig::FixedZ {
        total: 4096,
        z_slices: 24,
        z_config: ClusterZConfig {
            first_slice_depth: 5.0,
            far_z_mode: ClusterFarZMode::Constant(CLUSTER_FAR),
        },
        dynamic_resizing: true,
    }
}

/// The world view's drawing settings. Read when the cameras and the room lights are
/// spawned.
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct ViewQuality {
    /// The main camera's MSAA, and so the warm-up camera's.
    pub msaa: Msaa,
    /// Whether the main camera draws a depth prepass first, so each pixel is lit once.
    pub prepass: bool,
    /// How far a room's tube light reaches, in metres.
    pub light_range: f32,
    /// The main camera draws this many pixels per world view pixel, each way, and the
    /// presenter stretches its image over the world view ([`frame_view`]). 1 draws
    /// straight into the window.
    pub render_scale: f32,
}

/// The defaults, measured in a nested KWin on the owner's laptop (ticket 27's comments):
/// - **MSAA off.** 4× MSAA cost a fifth of a room's frame; the stretched image softens
///   edges a little anyway.
/// - **A depth prepass.** A room's fragments are lit once each instead of once per
///   overlapping surface: a third off a room's frame.
/// - **Tube lights reach 8 m**, not 14, so each fragment adds up fewer of a room's 21
///   lights: 30 % off a room's frame. A tube's light 8 m away was about an eighth of
///   what it is under the tube, so the room looks much the same.
/// - **Render scale 0.75**: a little under 60 % of the pixels. Faces and signs stay
///   readable; at 0.5 the faces' names blur.
impl Default for ViewQuality {
    fn default() -> Self {
        Self {
            msaa: Msaa::Off,
            prepass: true,
            light_range: 8.0,
            render_scale: 0.75,
        }
    }
}

impl ViewQuality {
    /// The defaults, with a debug build's overrides:
    /// `WORLD_DEBUG_MSAA=off|2|4`, `WORLD_DEBUG_PREPASS=0|1`,
    /// `WORLD_DEBUG_LIGHT_RANGE=<metres>`, `WORLD_DEBUG_RENDER_SCALE=<0.25..1>`.
    pub fn with_debug_overrides() -> Self {
        let mut quality = Self::default();
        if !cfg!(debug_assertions) {
            return quality;
        }
        let var = |name: &str| std::env::var(format!("WORLD_DEBUG_{name}")).ok();
        match var("MSAA").as_deref() {
            Some("off" | "1") => quality.msaa = Msaa::Off,
            Some("2") => quality.msaa = Msaa::Sample2,
            Some("4") => quality.msaa = Msaa::Sample4,
            _ => {}
        }
        match var("PREPASS").as_deref() {
            Some("1") => quality.prepass = true,
            Some("0") => quality.prepass = false,
            _ => {}
        }
        if let Some(range) = var("LIGHT_RANGE").and_then(|r| r.parse().ok()) {
            quality.light_range = range;
        }
        if let Some(scale) = var("RENDER_SCALE").and_then(|s| s.parse::<f32>().ok()) {
            quality.render_scale = scale.clamp(0.25, 1.0);
        }
        quality
    }
}
