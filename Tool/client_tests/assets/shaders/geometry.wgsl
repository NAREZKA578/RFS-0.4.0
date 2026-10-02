// Geometry pass: transforms a mesh vertex by an MVP matrix and shades it
// with one directional light.
//
// This is the first shader that consumes real geometry rather than generating
// its own positions. It exists because the repository had no vertex-input
// shader at all, so no scene could be drawn no matter how complete the RHI was.
//
// Attribute locations must match the pipeline's `VertexInputState`, which is
// built from the render layer's `Vertex`:
//   0 position  vec3   offset 0
//   1 normal    vec3   offset 12
//   2 tangent   vec4   offset 24
//   3 tex_coord vec2   offset 40
//   4 color     vec4   offset 48
// The offsets are implied by `from_formats`, which sums the format sizes in
// order, so the order below is load-bearing.

struct Uniforms {
    // Column-major, matching glam's Mat4 memory layout.
    mvp: mat4x4<f32>,
    // The light direction in view space, w padded to vec4 for alignment.
    light_dir: vec4<f32>,
    light_color: vec4<f32>,
    ambient: vec4<f32>,
}

@group(0) @binding(0)
var<uniform> uniforms: Uniforms;

struct VsIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) tangent: vec4<f32>,
    @location(3) tex_coord: vec2<f32>,
    @location(4) color: vec4<f32>,
}

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) tex_coord: vec2<f32>,
    @location(2) color: vec4<f32>,
}

@vertex
fn vs_main(in: VsIn) -> VsOut {
    var out: VsOut;
    out.clip = uniforms.mvp * vec4<f32>(in.position, 1.0);
    out.normal = in.normal;
    out.tex_coord = in.tex_coord;
    out.color = in.color;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    // A zero-length normal would make `normalize` produce NaN, which then
    // propagates into the colour and the whole triangle disappears. The mesh
    // primitives do set normals, but a mesh loaded from a file may not.
    var n = normalize(in.normal);
    if (dot(n, n) < 1e-12) {
        n = vec3<f32>(0.0, 1.0, 0.0);
    }

    let l = normalize(uniforms.light_dir.xyz);
    // Half-Lambert: keeps the unlit side from going fully black, which reads
    // better for a debug pass than a hard terminator.
    let ndotl = dot(n, l) * 0.5 + 0.5;
    let lit = uniforms.light_color.rgb * ndotl + uniforms.ambient.rgb;

    return vec4<f32>(in.color.rgb * lit, in.color.a);
}
