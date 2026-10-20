// A focus monitor of the CCTV room (management-rooms ticket 17): its close-up camera's
// picture of one flagged agent's robot, monochrome in the flag's colour with a wash of
// it (a killed agent's in the wall feeds' plain tint, with no wash), scanlines and grain like the wall feeds (`monitor.wesl`), targeting brackets round
// the face in the middle, the CAM number top-left, the model and agent and the room's
// name bottom-left, the flag and its timer bottom-right, and a blinking recording dot.
import bevy_pbr::render::forward_io::VertexOutput;
import bevy_pbr::render::mesh_view_bindings::globals;

// x: 1 once the camera has drawn its agent; y: the grain seed.
@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> params: vec4<f32>;
// rgb: the flag's colour; a: 1 while its agent is flagged (0 for a killed agent).
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var<uniform> flag: vec4<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var feed: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(3) var feed_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(4) var cam_image: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(5) var cam_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(6) var name_image: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(7) var name_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(8) var who_image: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(9) var who_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(10) var status_image: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(11) var status_sampler: sampler;

const TEXT: vec3<f32> = vec3<f32>(0.70, 0.95, 1.00);
// An unflagged picture's tint, as on the wall feeds (`monitor.wesl`).
const TINT: vec3<f32> = vec3<f32>(0.30, 0.80, 1.00);
const REC: vec3<f32> = vec3<f32>(1.00, 0.03, 0.02);
const WASH: f32 = 0.18;
// The overlays' boxes as (left, top, right, bottom) in the screen's UVs, keeping their
// images' shapes (512 × 44 and 192 × 44, `lettering`) on the 1.52 × 0.855 m screen.
const CAM_BOX: vec4<f32> = vec4<f32>(0.03, 0.05, 0.30, 0.16);
const WHO_BOX: vec4<f32> = vec4<f32>(0.03, 0.72, 0.75, 0.83);
const NAME_BOX: vec4<f32> = vec4<f32>(0.03, 0.835, 0.75, 0.945);
const STATUS_BOX: vec4<f32> = vec4<f32>(0.25, 0.72, 0.97, 0.83);
const REC_AT: vec2<f32> = vec2<f32>(0.95, 0.10);
const REC_RADIUS: f32 = 0.025;
const ASPECT: f32 = 1.7777778;
// The targeting brackets round the middle: half the square's size, the arms' length and
// the bars' thickness, in metres on the screen.
const SCREEN: vec2<f32> = vec2<f32>(1.52, 0.855);
const BRACKET_HALF: f32 = 0.2;
const BRACKET_ARM: f32 = 0.07;
const BRACKET_BAR: f32 = 0.011;

fn hash(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(12.9898, 78.233))) * 43758.5453);
}

fn overlay(image: texture_2d<f32>, image_sampler: sampler, box: vec4<f32>, uv: vec2<f32>) -> f32 {
    let at = (uv - box.xy) / (box.zw - box.xy);
    let inside = all(at >= vec2<f32>(0.0)) && all(at <= vec2<f32>(1.0));
    let alpha = textureSampleLevel(image, image_sampler, clamp(at, vec2<f32>(0.0), vec2<f32>(1.0)), 0.0).a;
    return select(0.0, alpha, inside);
}

// Whether `p` (metres from the screen's middle) is on one of the four corner brackets.
fn bracket(p: vec2<f32>) -> f32 {
    let a = abs(p);
    let along_x = a.x <= BRACKET_HALF && a.x >= BRACKET_HALF - BRACKET_ARM
        && abs(a.y - BRACKET_HALF) <= BRACKET_BAR / 2.0;
    let along_y = a.y <= BRACKET_HALF && a.y >= BRACKET_HALF - BRACKET_ARM
        && abs(a.x - BRACKET_HALF) <= BRACKET_BAR / 2.0;
    return select(0.0, 1.0, along_x || along_y);
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let uv = in.uv;
    let time = globals.time;
    let drawn = params.x > 0.5;
    let rgb = textureSampleLevel(feed, feed_sampler, uv, 0.0).rgb;
    let luma = dot(rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
    var level = select(0.0, 0.03 + 1.5 * luma, drawn);
    level *= 0.86 + 0.14 * sin(uv.y * 3.14159265 * 2.0 * 140.0);
    let band = fract(uv.y * 0.6 - time * 0.08);
    level *= 0.94 + 0.08 * smoothstep(0.0, 0.08, band) * (1.0 - smoothstep(0.08, 0.2, band));
    let grain = hash(floor(uv * vec2<f32>(320.0, 180.0)) + vec2<f32>(fract(time * 7.13), params.y));
    level += (grain - 0.5) * select(0.3, 0.06, drawn);
    let off = (uv - vec2<f32>(0.5)) * vec2<f32>(1.0, 0.8);
    level *= 1.0 - 0.9 * dot(off, off);
    let tint = mix(TINT, flag.rgb, flag.a);
    var colour = tint * max(level, 0.0);
    colour = mix(colour, 1.0 - (1.0 - colour) * (1.0 - flag.rgb), WASH * flag.a);
    // The brackets, then the texts and the recording dot.
    let at = (uv - vec2<f32>(0.5)) * SCREEN;
    colour = mix(colour, tint * 1.6, bracket(at));
    let text = max(
        max(overlay(cam_image, cam_sampler, CAM_BOX, uv), overlay(name_image, name_sampler, NAME_BOX, uv)),
        overlay(who_image, who_sampler, WHO_BOX, uv),
    );
    colour = mix(colour, TEXT, text);
    colour = mix(colour, tint * 1.6, overlay(status_image, status_sampler, STATUS_BOX, uv));
    let rec = length((uv - REC_AT) * vec2<f32>(ASPECT, 1.0));
    let lit = step(fract(time), 0.6);
    colour = mix(colour, REC, (1.0 - smoothstep(REC_RADIUS - 0.006, REC_RADIUS, rec)) * lit);
    return vec4<f32>(colour, 1.0);
}
