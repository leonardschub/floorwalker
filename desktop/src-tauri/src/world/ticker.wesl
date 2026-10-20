// The CCTV room's ticker (management-rooms ticket 17): one line of every flagged agent,
// drawn white on clear into an image (`lettering::Live::Ticker`), scrolled right to left
// across the strip on the world's clock, in the room's pale cyan on dark glass.
import bevy_pbr::render::forward_io::VertexOutput;
import bevy_pbr::render::mesh_view_bindings::globals;

// x: the line's width in its image, in pixels (0: nothing to show); y, z: the image's
// size in pixels.
@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> params: vec4<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var line: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var line_sampler: sampler;

const TEXT: vec3<f32> = vec3<f32>(0.55, 0.92, 1.00);
const GLASS: vec3<f32> = vec3<f32>(0.004, 0.012, 0.016);
// The strip's shape (5.1 × 0.09 m), and how fast the line moves, in image pixels a second.
const STRIP_ASPECT: f32 = 56.666667;
const SPEED: f32 = 140.0;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let uv = in.uv;
    let width = params.x;
    let image = params.yz;
    // The strip shows `window` pixels of the line at a time; the line comes in at the
    // right, crosses and leaves at the left, then comes round again.
    let window = image.y * STRIP_ASPECT;
    let cycle = width + window;
    let travelled = (globals.time * SPEED) % max(cycle, 1.0);
    let x = uv.x * window + travelled - window;
    let inside = width > 0.0 && x >= 0.0 && x <= width;
    let alpha = textureSampleLevel(line, line_sampler, vec2<f32>(clamp(x, 0.0, image.x) / image.x, uv.y), 0.0).a;
    let lines = 0.9 + 0.1 * sin(uv.y * 3.14159265 * 2.0 * 6.0);
    let colour = mix(GLASS, TEXT * lines, select(0.0, alpha, inside));
    return vec4<f32>(colour, 1.0);
}
