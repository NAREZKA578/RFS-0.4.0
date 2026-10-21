// Textured UI pipeline: samples a coverage atlas.
//
// Used for glyphs and icons. The vertex layout is identical to the untextured
// pipeline so both can share one vertex buffer and one build path; only the
// fragment stage differs.
//
// The atlas is single-channel (R8_UNORM) coverage rather than RGBA: a glyph is
// white-on-transparent, so only coverage carries information. That is a quarter
// of the memory of an RGBA atlas, which matters because a large atlas is the
// largest thing a UI subsystem allocates.

struct UiUniforms {
    // Column-major 4x4, matching the std140 mat4x4<f32> layout below.
    projection: mat4x4<f32>,
};

@group(0) @binding(0) var<uniform> uniforms: UiUniforms;
@group(0) @binding(1) var atlas: texture_2d<f32>;
@group(0) @binding(2) var atlas_sampler: sampler;

struct VsOut {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) tex_coord: vec2<f32>,
    @location(1) color: vec4<f32>,
};

@vertex
fn vs_main(
    @location(0) position: vec2<f32>,
    @location(1) tex_coord: vec2<f32>,
    @location(2) color: vec4<f32>,
) -> VsOut {
    var out: VsOut;
    out.clip_position = uniforms.projection * vec4<f32>(position, 0.0, 1.0);
    out.tex_coord = tex_coord;
    out.color = color;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let coverage = textureSample(atlas, atlas_sampler, in.tex_coord).r;
    // Multiply alpha by the coverage, not RGB. Folding coverage into the colour
    // as well would darken the glyph twice over under alpha blending and make
    // antialiased edges look muddy.
    return vec4<f32>(in.color.rgb, in.color.a * coverage);
}