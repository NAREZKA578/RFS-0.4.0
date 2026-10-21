// Reads the scene's light buffer and writes the first light's numbers.
//
// This exists for one reason: to prove that the shader and the writer agree
// on where a light's fields are.
//
// The layout is not obvious. A `vec3` inside a struct is 16-aligned and takes
// 12 bytes, and the member after it is aligned by **its own** alignment, not
// by the struct's. So `position` is at 0, `color` at 16, `intensity` at 28 and
// `radius` at 32 - not at 32 and 36. A writer that puts the two floats four
// bytes late makes the shader read the intensity where the radius is, and the
// light's reach collapses to a point.
//
// The numbers come out in the colour: the position's x, the intensity scaled
// down, the radius scaled down, and the colour's red.

struct Light {
    position: vec3<f32>,
    color: vec3<f32>,
    intensity: f32,
    radius: f32,
};

struct Lights {
    lights: array<Light>,
};

@group(0) @binding(0) var<storage, read> lights: Lights;

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> @builtin(position) vec4<f32> {
    // A fullscreen triangle: (-1, -1) (3, -1) (-1, 3).
    var positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0),
    );
    return vec4<f32>(positions[vertex_index], 0.0, 1.0);
}

@fragment
fn fs_main() -> @location(0) vec4<f32> {
    let light = lights.lights[0];
    return vec4<f32>(
        light.position.x * 0.5 + 0.5,
        light.intensity / 8.0,
        light.radius / 64.0,
        light.color.r,
    );
}
