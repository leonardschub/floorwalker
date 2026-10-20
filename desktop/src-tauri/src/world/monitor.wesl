// A camera feed on a monitor of the CCTV room's wall (management-rooms ticket 15): its
// feed camera's image, monochrome in the room's cyan, with scanlines, grain and a slow
// rolling band, the room's name bottom-left, its CAM number top-left and a blinking red
// recording dot top-right. A flagged feed is drawn in its flag's colour
// instead, with a wash of it over the picture. Its own time is the world's (`globals.time`), so nothing is
// rendered twice and no pass is added. No lighting, fog or tonemapping here: the feed
// camera applied its own, and the screen glows by itself.
import bevy_pbr::render::forward_io::VertexOutput;
import bevy_pbr::render::mesh_view_bindings::globals;

// x: 1 once the feed has an image; y: the feed's own grain seed.
@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> params: vec4<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var feed: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var feed_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(3) var name_image: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(4) var name_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(5) var cam_image: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(6) var cam_sampler: sampler;
// rgb: the flag's colour; a: 1 while the feed is flagged.
@group(constants::MATERIAL_BIND_GROUP) @binding(7) var<uniform> flag: vec4<f32>;

// The cyan the picture is drawn in, and the overlay text's paler cyan (linear).
const TINT: vec3<f32> = vec3<f32>(0.30, 0.80, 1.00);
const TEXT: vec3<f32> = vec3<f32>(0.70, 0.95, 1.00);
const REC: vec3<f32> = vec3<f32>(1.00, 0.03, 0.02);
// Where the overlay texts sit on the screen, as (left, top, right, bottom) in its UVs;
// their images are 512 × 44 and 192 × 44 (`lettering::Overlay`), so the boxes keep
// that shape on the 16:9 screen.
const NAME_BOX: vec4<f32> = vec4<f32>(0.03, 0.835, 0.75, 0.945);
const CAM_BOX: vec4<f32> = vec4<f32>(0.03, 0.055, 0.30, 0.165);
const REC_AT: vec2<f32> = vec2<f32>(0.94, 0.11);
const REC_RADIUS: f32 = 0.032;
const ASPECT: f32 = 1.7777778;
// How much of a flagged feed's colour washes over it (screen blend), as in the accepted
// renders.
const WASH: f32 = 0.18;

fn hash(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(12.9898, 78.233))) * 43758.5453);
}

// The overlay image `image` laid over `box`, at `uv`: its alpha (premultiplied white).
fn overlay(image: texture_2d<f32>, image_sampler: sampler, box: vec4<f32>, uv: vec2<f32>) -> f32 {
    let at = (uv - box.xy) / (box.zw - box.xy);
    let inside = all(at >= vec2<f32>(0.0)) && all(at <= vec2<f32>(1.0));
    let alpha = textureSampleLevel(image, image_sampler, clamp(at, vec2<f32>(0.0), vec2<f32>(1.0)), 0.0).a;
    return select(0.0, alpha, inside);
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let uv = in.uv;
    let time = globals.time;
    // The picture's brightness, lifted out of the room's dim light.
    let rgb = textureSampleLevel(feed, feed_sampler, uv, 0.0).rgb;
    let luma = dot(rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
    var level = select(0.0, 0.03 + 1.5 * luma, params.x > 0.5);
    // Scanlines, a slow band rolling down, and grain that changes every frame.
    level *= 0.84 + 0.16 * sin(uv.y * 3.14159265 * 2.0 * 96.0);
    let band = fract(uv.y * 0.6 - time * 0.08);
    level *= 0.94 + 0.08 * smoothstep(0.0, 0.08, band) * (1.0 - smoothstep(0.08, 0.2, band));
    let grain = hash(floor(uv * vec2<f32>(320.0, 180.0)) + vec2<f32>(fract(time * 7.13), params.y));
    level += (grain - 0.5) * select(0.3, 0.07, params.x > 0.5);
    // A darker rim, like a camera's lens.
    let off = (uv - vec2<f32>(0.5)) * vec2<f32>(1.0, 0.8);
    level *= 1.0 - 0.9 * dot(off, off);
    let tint = mix(TINT, flag.rgb, flag.a);
    var colour = tint * max(level, 0.0);
    colour = mix(colour, 1.0 - (1.0 - colour) * (1.0 - flag.rgb), WASH * flag.a);
    // The texts, then the recording dot, blinking once a second.
    let text = max(overlay(name_image, name_sampler, NAME_BOX, uv), overlay(cam_image, cam_sampler, CAM_BOX, uv));
    colour = mix(colour, TEXT, text);
    let rec = length((uv - REC_AT) * vec2<f32>(ASPECT, 1.0));
    let lit = step(fract(time), 0.6);
    let dot_alpha = (1.0 - smoothstep(REC_RADIUS - 0.006, REC_RADIUS, rec)) * lit;
    colour = mix(colour, REC, dot_alpha);
    return vec4<f32>(colour, 1.0);
}
