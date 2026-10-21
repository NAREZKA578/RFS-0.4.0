struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) tangent: vec4<f32>,
    @location(3) tex_coord: vec2<f32>,
    @location(4) color: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
};

struct Uniforms {
    light_space_matrix: mat4x4<f32>,
    model: mat4x4<f32>,
};

@group(0) @binding(0) var<uniform> uniforms: Uniforms;

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;
    output.clip_position = uniforms.light_space_matrix * uniforms.model * vec4<f32>(input.position, 1.0);
    return output;
}

@fragment
fn fs_main(input: VertexOutput) -> @builtin(frag_depth) f32 {
    // The framebuffer depth, as it is.
    //
    // In the fragment stage `@builtin(position)` is the framebuffer
    // coordinate: `z` is the depth in [0, 1] and `w` is the *reciprocal* of
    // the clip-space w. Dividing `z` by `w` therefore multiplies the depth by
    // the clip w, and for anything nearer than the far plane that product is
    // larger than 1 - a depth the depth test rejects, so the shadow map stays
    // empty and the pass reports that it drew.
    return input.clip_position.z;
}
