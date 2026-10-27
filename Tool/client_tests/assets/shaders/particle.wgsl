// The particle shader: camera-facing quads (billboards), one per live
// particle, instanced.
//
// The particles are not drawn as points and not as a mesh: the pass draws the
// same unit quad `instance_count` times — one instance per live particle —
// and the vertex stage puts each quad where the particle is, facing the
// camera. The particle's own data comes from the **particle system's buffer**
// at `@builtin(instance_index)`: the system uploads its live particles in
// pool order, the instance index counts in draw order, and those are the same
// order — the particle instance 3 shows is `particles[3]`.
//
// The billboard: the quad's corners come in as `(-1..1, -1..1, 0)` (the
// mesh's clip-space quad, reused), and the vertex stage walks them out along
// the camera's **right** and **up** axes, so the sprite always faces the
// camera no matter where it stands in the world. `size` is the sprite's
// **half-extent** in world units: the quad spans `2 * size` across.
//
// The fragment is a soft disc: solid inside a small core, falling to nothing
// well before the quad's edge, so a particle never shows as a hard square.
// There is no texture in v1: the colour is the particle's own. The
// `rotation` fields the particle carries are not applied yet — the sprite is
// always axis-aligned on the camera's plane.
//
// The layout of `Particle` is written down once, in
// `src/render/particles/particle.rs`, whose pads put every vector where WGSL
// puts it: `vec3<f32>` is 16-aligned in the storage address space, so the
// fields sit at 0, 16, 32, 48... and the array's element stride is 96 bytes.
// The CPU-side asserts in that file pin the same numbers — the upload casts
// the struct to bytes, so a shader that drifts from the struct does not fail
// loudly, it draws garbage from the wrong offsets.

struct CameraUniforms {
    // The view-projection the frame is drawn with.
    view_proj: mat4x4<f32>,
    // The camera's right axis, in the world.
    camera_right: vec4<f32>,
    // The camera's up axis, in the world.
    camera_up: vec4<f32>,
};

struct Particle {
    position: vec3<f32>,
    velocity: vec3<f32>,
    size: vec2<f32>,
    color: vec4<f32>,
    lifetime: f32,
    max_lifetime: f32,
    rotation: f32,
    rotation_speed: f32,
    texture_index: u32,
    emitter_id: u32,
};

struct Particles {
    particles: array<Particle>,
};

@group(0) @binding(0) var<uniform> camera: CameraUniforms;
@group(0) @binding(1) var<storage, read> particles: Particles;

struct VsOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
};

@vertex
fn vs_main(
    @location(0) corner: vec3<f32>,
    @builtin(instance_index) instance: u32,
) -> VsOutput {
    let particle = particles.particles[instance];
    // The quad's corner walked out on the camera's plane: across on the
    // camera's right, up on the camera's up, `size` the half-extent of each.
    let world = particle.position
        + camera.camera_right.xyz * (corner.x * particle.size.x)
        + camera.camera_up.xyz * (corner.y * particle.size.y);

    var output: VsOutput;
    output.clip_position = camera.view_proj * vec4<f32>(world, 1.0);
    // The quad's uv, 0 at one corner and 1 at the other: the fragment reads
    // its distance from the sprite's centre out of this.
    output.uv = corner.xy * 0.5 + vec2<f32>(0.5, 0.5);
    output.color = particle.color;
    return output;
}

@fragment
fn fs_main(input: VsOutput) -> @location(0) vec4<f32> {
    // The distance from the sprite's centre: 0 at the centre, 1 at the quad's
    // edge.
    let from_centre = (input.uv - vec2<f32>(0.5, 0.5)) * 2.0;
    let d = length(from_centre);
    // A soft disc: solid inside d < 0.3, gone past d = 0.5, smooth between —
    // the square the quad is drawn as stays invisible.
    let falloff = 1.0 - smoothstep(0.30, 0.50, d);
    var color = input.color;
    color.a = color.a * falloff;
    return color;
}
