//! The baked textures, made light for the owner's laptop (sweatshop-rooms ticket 27). The
//! pieces' textures are 1024² PNGs without mip levels: every surface further than a step
//! away sampled its full-size textures, which shimmers on brick and ceiling tiles, and
//! the 60 of them, kept both in the world's memory and on the GPU, cost far more memory
//! than a laptop with 7 GB to share can spare (the measurements swapped).
//!
//! As each one loads, before it is first drawn, it is cut to [`MAX_SIZE`] and given its
//! whole mip chain (the glTF samplers already ask for trilinear filtering). Only the
//! colour textures keep their pixels in the world's memory after they reach the GPU:
//! `surfaces` makes a dimmed part's desaturated copies from them. Normal and ORM maps
//! live on the GPU only.
//!
//! The chain is a plain 2×2 box filter. The desktop crate is built optimised in dev
//! builds too (`Cargo.toml`), so the 60 textures take a fraction of a second.

use std::time::{Duration, Instant};

use bevy::asset::{AssetEvent, RenderAssetUsages};
use bevy::image::Image;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureFormat};

/// The largest a baked texture is drawn: half the baked size each way. The world view
/// is under a thousand pixels across, so even a wall a step away shows little more.
pub const MAX_SIZE: u32 = 512;

/// Cuts and mips every baked texture as it loads.
pub struct MipmapsPlugin;

impl Plugin for MipmapsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Baked>()
            .add_systems(PreUpdate, bake_textures);
    }
}

/// How many textures are done, and how long they took.
#[derive(Resource, Default)]
struct Baked {
    count: usize,
    took: Duration,
}

/// The baked textures' folder in the asset root (`desktop/world-assets`).
const TEXTURES: &str = "textures/";

fn bake_textures(
    mut events: MessageReader<AssetEvent<Image>>,
    mut images: ResMut<Assets<Image>>,
    assets: Res<AssetServer>,
    mut baked: ResMut<Baked>,
) {
    let began = Instant::now();
    let mut made = 0;
    for event in events.read() {
        let AssetEvent::LoadedWithDependencies { id } = *event else {
            continue;
        };
        let Some(path) = assets.get_path(id) else {
            continue;
        };
        let path = path.path().to_string_lossy().into_owned();
        if !path.starts_with(TEXTURES) {
            continue;
        }
        let colour = path.ends_with("_color.png");
        if let Some(mut image) = images.get_mut(id) {
            if bake(&mut image, MAX_SIZE, colour) {
                made += 1;
            }
        }
    }
    if made > 0 {
        baked.count += made;
        baked.took += began.elapsed();
        eprintln!(
            "[world] textures: {} cut to {MAX_SIZE}² and mipped so far, {} ms",
            baked.count,
            baked.took.as_millis()
        );
    }
}

/// Cuts a single-level, single-layer RGBA8 `image` to at most `max` pixels each way and
/// gives it its whole mip chain; without `keep_pixels` its pixels then live on the GPU
/// only. False when it has mip levels already, or is not such an image.
pub fn bake(image: &mut Image, max: u32, keep_pixels: bool) -> bool {
    let descriptor = &image.texture_descriptor;
    let size = descriptor.size;
    let rgba8 = matches!(
        descriptor.format,
        TextureFormat::Rgba8Unorm | TextureFormat::Rgba8UnormSrgb
    );
    if !rgba8 || descriptor.mip_level_count != 1 || size.depth_or_array_layers != 1 {
        return false;
    }
    let Some(data) = image.data.as_ref() else {
        return false;
    };
    let Some(chain) = mip_chain(data, size.width, size.height, max) else {
        return false;
    };
    image.texture_descriptor.size = Extent3d {
        width: chain.base.0,
        height: chain.base.1,
        depth_or_array_layers: 1,
    };
    image.texture_descriptor.mip_level_count = chain.levels;
    image.data = Some(chain.data);
    if !keep_pixels {
        image.asset_usage = RenderAssetUsages::RENDER_WORLD;
    }
    true
}

/// A mip chain: every level's RGBA8 pixels, largest first.
#[derive(Debug, PartialEq)]
pub struct Chain {
    pub data: Vec<u8>,
    pub levels: u32,
    /// The largest level's size.
    pub base: (u32, u32),
}

/// The mip chain of the RGBA8 pixels `data` (`width` × `height`) from the first level no
/// bigger than `max` each way down to 1×1: each level is the 2×2 box average of the one
/// before (an odd edge repeats its last row or column). None if `data` is not that size.
pub fn mip_chain(data: &[u8], width: u32, height: u32, max: u32) -> Option<Chain> {
    let (mut w, mut h) = (width as usize, height as usize);
    if data.len() != w * h * 4 {
        return None;
    }
    let max = max.max(1) as usize;
    let mut level = data.to_vec();
    let mut chain = Chain {
        data: Vec::new(),
        levels: 0,
        base: (0, 0),
    };
    loop {
        if w <= max && h <= max {
            if chain.levels == 0 {
                chain.base = (w as u32, h as u32);
                chain.data.reserve(level.len() * 4 / 3 + 64);
            }
            chain.data.extend_from_slice(&level);
            chain.levels += 1;
        }
        if w == 1 && h == 1 {
            return Some(chain);
        }
        let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
        let mut next = vec![0u8; nw * nh * 4];
        for (y, out_row) in next.chunks_exact_mut(nw * 4).enumerate() {
            let row = |r: usize| &level[r.min(h - 1) * w * 4..][..w * 4];
            let (top, bottom) = (row(2 * y), row(2 * y + 1));
            for (x, px) in out_row.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                let (a, b) = ((2 * x).min(w - 1) * 4, (2 * x + 1).min(w - 1) * 4);
                for c in 0..4 {
                    let sum = u16::from(top[a + c])
                        + u16::from(top[b + c])
                        + u16::from(bottom[a + c])
                        + u16::from(bottom[b + c]);
                    px[c] = ((sum + 2) / 4) as u8;
                }
            }
        }
        level = next;
        (w, h) = (nw, nh);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 4×2: left half black, right half white.
    fn halves() -> Vec<u8> {
        let mut data = Vec::new();
        for _ in 0..2 {
            for x in 0..4 {
                let v = if x < 2 { 0 } else { 255 };
                data.extend_from_slice(&[v, v, v, 255]);
            }
        }
        data
    }

    #[test]
    fn a_chain_halves_down_to_one_pixel_and_averages() {
        let chain = mip_chain(&halves(), 4, 2, 1024).unwrap();
        // 4×2, 2×1, 1×1.
        assert_eq!((chain.levels, chain.base), (3, (4, 2)));
        assert_eq!(chain.data.len(), (8 + 2 + 1) * 4);
        // The 2×1 level keeps the halves apart; the last pixel is their mean.
        assert_eq!(&chain.data[32..40], &[0, 0, 0, 255, 255, 255, 255, 255]);
        let last = &chain.data[chain.data.len() - 4..];
        assert!((127..=128).contains(&last[0]), "{last:?}");
        assert_eq!(last[3], 255);
        assert_eq!(mip_chain(&halves(), 4, 3, 1024), None);
    }

    #[test]
    fn a_chain_starts_at_the_first_level_no_bigger_than_the_cap() {
        let chain = mip_chain(&halves(), 4, 2, 2).unwrap();
        assert_eq!((chain.levels, chain.base), (2, (2, 1)));
        assert_eq!(
            chain.data,
            [0, 0, 0, 255, 255, 255, 255, 255, 128, 128, 128, 255]
        );
    }

    #[test]
    fn a_baked_texture_is_cut_and_mipped_and_only_colour_keeps_its_pixels() {
        let image = |format| {
            Image::new(
                Extent3d {
                    width: 4,
                    height: 2,
                    depth_or_array_layers: 1,
                },
                bevy::render::render_resource::TextureDimension::D2,
                halves(),
                format,
                RenderAssetUsages::default(),
            )
        };
        let mut colour = image(TextureFormat::Rgba8UnormSrgb);
        assert!(bake(&mut colour, 2, true));
        assert_eq!(colour.texture_descriptor.mip_level_count, 2);
        assert_eq!(colour.size(), UVec2::new(2, 1));
        assert_eq!(colour.asset_usage, RenderAssetUsages::default());
        // Done once only.
        assert!(!bake(&mut colour, 2, true));
        let mut normal = image(TextureFormat::Rgba8Unorm);
        assert!(bake(&mut normal, 2, false));
        assert_eq!(normal.asset_usage, RenderAssetUsages::RENDER_WORLD);
    }

    /// How long a 1024² texture takes in this build:
    /// `cargo test mip_chain_time -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn mip_chain_time() {
        let data: Vec<u8> = (0..1024 * 1024 * 4).map(|i| (i % 251) as u8).collect();
        let began = Instant::now();
        let chain = mip_chain(&data, 1024, 1024, MAX_SIZE).unwrap();
        eprintln!(
            "1024² to {:?} with {} levels in {:?}",
            chain.base,
            chain.levels,
            began.elapsed()
        );
    }
}
