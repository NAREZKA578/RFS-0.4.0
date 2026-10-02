// Fullscreen procedural pattern.
//
// Deliberately vertex-less: the three vertices come from
// `@builtin(vertex_index)`, so the test needs no vertex buffer at all. That
// isolates the pipeline to shader + rasterisation + readback, which is what
// this milestone is meant to prove.
//
// The pattern is analytically predictable so the test can assert on exact
// pixel values rather than "looks about right":
//   - outside the disc: a vertical gradient, blue-ish, no red
//   - inside the disc:  solid red-ish, no green
// The ring between them is an antialiasing artefact and is not asserted on.

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

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
    // Clip space is y-up; texture/vulkan NDC here is y-down, so flip.
    out.uv = vec2<f32>(p.x * 0.5 + 0.5, 0.5 - p.y * 0.5);
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    // Centre of the disc, in uv space.
    let centre = vec2<f32>(0.5, 0.5);
    let radius = 0.3;
    let d = distance(in.uv, centre);

    if (d < radius) {
        // Solid red. No green, no blue: the test asserts the channels are 0
        // so a clear colour or a stale frame cannot pass as a match.
        return vec4<f32>(1.0, 0.0, 0.0, 1.0);
    }

    // Vertical gradient: more blue at the top, a little green at the bottom,
    // red stays 0 everywhere outside the disc.
    let g = in.uv.y;
    return vec4<f32>(0.0, g, 1.0 - g, 1.0);
}
