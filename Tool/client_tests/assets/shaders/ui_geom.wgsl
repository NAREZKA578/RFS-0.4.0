// UI geometry pipeline: untextured, vertex-coloured quads.
//
// The vertex layout carries UVs even though this shader ignores them, so that
// adding a textured variant later reuses the same buffer layout instead of
// forcing a vertex-format change on every caller.

struct UiUniforms {
    // Column-major 4x4, matching the std140 mat4x4<f32> layout below.
    projection: mat4x4<f32>,
};

@group(0) @binding(0) var<uniform> uniforms: UiUniforms;

struct VsOut {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_main(
    @location(0) position: vec2<f32>,
    @location(1) tex_coord: vec2<f32>,
    @location(2) color: vec4<f32>,
) -> VsOut {
    var out: VsOut;
    out.clip_position = uniforms.projection * vec4<f32>(position, 0.0, 1.0);
    out.color = color;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    return in.color;
}