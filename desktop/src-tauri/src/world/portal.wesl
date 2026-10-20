// A doorway's view (sweatshop-rooms ticket 19): its portal camera's image, read at the
// fragment's pixel in the world view (Bevy's `mirror` example, without the reflection),
// or a dim flat colour while the doorway is not live. No lighting, fog or tonemapping
// here: the portal camera applied the main view's own, so nothing is applied twice.
import bevy_pbr::render::forward_io::VertexOutput;
import bevy_pbr::render::mesh_view_bindings::view;

// rgb: the dim colour (linear).
@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> dim: vec4<f32>;
// x: image pixels per world view pixel; y: 1 while the doorway is live.
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var<uniform> frame: vec4<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var view_image: texture_2d<f32>;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    if frame.y < 0.5 {
        return vec4(dim.rgb, 1.0);
    }
    // The image covers the world view's viewport, not the whole surface.
    let size = vec2<f32>(textureDimensions(view_image));
    let at = floor((in.position.xy - view.viewport.xy) * frame.x);
    let p = clamp(at, vec2(0.0), size - vec2(1.0));
    return vec4(textureLoad(view_image, vec2<i32>(p), 0).rgb, 1.0);
}
