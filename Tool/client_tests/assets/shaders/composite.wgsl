// Copies one texture into another, fullscreen.
//
// Used to be the last link of the deferred chain; that chain is gone
// (№741). The copy itself is not: the particle composite puts the particle
// image over the scene's image with it, and anything else that moves a
// finished texture onto another target can do the same.
//
// Deliberately vertex-less: the three vertices come from
// `@builtin(vertex_index)`, so there is no vertex buffer to own.
//
// The source is read with `textureLoad` and not with a sampler, so the
// descriptor layout needs no sampler binding - and the RHI has no sampler type
// for a depth texture anyway, which is one fewer thing to get wrong.

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vid: u32) -> VsOut {
    // One oversized triangle covering the whole viewport.
    // (-1,-1) (3,-1) (-1,3) in clip space.
    var pos = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0),
    );
    let p = pos[vid];

    var out: VsOut;
    out.clip = vec4<f32>(p, 0.0, 1.0);
    // Clip space is y-up; a texture's row 0 is the top, so flip.
    out.uv = vec2<f32>(p.x * 0.5 + 0.5, 0.5 - p.y * 0.5);
    return out;
}

@group(0) @binding(0) var source_texture: texture_2d<f32>;

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let dims = vec2<f32>(textureDimensions(source_texture));
    return textureLoad(source_texture, vec2<i32>(in.uv * dims), 0);
}
