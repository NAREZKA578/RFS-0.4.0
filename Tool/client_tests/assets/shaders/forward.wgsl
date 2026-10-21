// The forward lighting shader: the scene lit per pixel, from the clusters.
//
// This is the consumer of the shadow map, of the cascades and of the light
// clustering. The clustering pass has already put the lights that reach into
// each cluster of the screen into that cluster; a pixel finds its own cluster
// and looks at the lights in it, instead of looking at every light in the
// scene.
//
// The shadow is a comparison against the depth the shadow pass wrote: the
// fragment's depth in the light's space against the map's. Which map depends
// on how far the fragment is: a directional light's shadow is cut into
// cascades by distance, and the near cascade is detailed while the far one is
// coarse. The map is read with a comparison sampler, which is the only
// sampler a depth image may be filtered by.
//
// The light is positional: a point with a radius. The attenuation is the
// inverse square with a radius that fades the light out at its edge, so a
// light does not light the whole world.

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) tangent: vec4<f32>,
    @location(3) tex_coord: vec2<f32>,
    @location(4) color: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_position: vec3<f32>,
    @location(1) world_normal: vec3<f32>,
    @location(2) color: vec4<f32>,
};

struct Uniforms {
    mvp: mat4x4<f32>,
    model: mat4x4<f32>,
    // The light's view-projection, one per cascade. The cascade the fragment
    // falls into decides which one is used.
    light_space: array<mat4x4<f32>, 4>,
    // x = the tiles in x, y = the tiles in y, z = the depth slices, w = the near plane.
    grid: vec4<f32>,
    // x = the far plane, y = the light the shadow map belongs to (-1 for none).
    planes: vec4<f32>,
    // The distances at which the frustum is cut: the cascade a fragment belongs
    // to is the one whose range holds its distance.
    splits: vec4<f32>,
    ambient: vec4<f32>,
};

// The light, in the layout the scene's light buffer uses.
//
// `vec3` is 16-aligned inside a struct, so `position` is at 0 and `color` at
// 16 - and the two floats after them are at **28 and 32**, not at 32 and 36: a
// member is aligned by its own alignment, and `f32`'s is 4, whereas a `vec3`
// takes 12 bytes at an alignment of 16. The array's stride is 48.
struct Light {
    position: vec3<f32>,
    color: vec3<f32>,
    intensity: f32,
    radius: f32,
};

struct Lights {
    lights: array<Light>,
};

@group(0) @binding(0) var<uniform> uniforms: Uniforms;
@group(0) @binding(1) var shadow_map_0: texture_depth_2d;
@group(0) @binding(2) var shadow_map_1: texture_depth_2d;
@group(0) @binding(3) var shadow_map_2: texture_depth_2d;
@group(0) @binding(4) var shadow_map_3: texture_depth_2d;
@group(0) @binding(5) var shadow_sampler: sampler_comparison;
@group(0) @binding(6) var<storage, read> lights: Lights;
@group(0) @binding(7) var<storage, read> cluster_indices: array<u32>;

// The value that marks a slot as empty.
//
// A fragment shader may not read an atomic counter unless the device has the
// `fragmentStoresAndAtomics` feature, and the counts are atomic because the
// clustering pass adds to them. So the slots are marked instead: the
// clustering pass marks the unused ones, and the fragment shader walks the
// slots and skips the marked ones.
const EMPTY_SLOT: u32 = 0xFFFFFFFFu;

// The number of slots in a cluster, and of cascades.
const SLOTS: u32 = 32u;
const CASCADES: u32 = 4u;

// The distance to the camera, in world units.
//
// The clip space's z is not linear in the distance, and the clusters' slices
// are linear in the distance, so the distance has to be recovered from it.
//
// The form matters: **Vulkan's projection puts the near plane at z = 0 and the
// far plane at z = 1**, not at -1 and 1 as OpenGL does. The OpenGL form of
// this formula gives a distance about twice the right one for a fragment near
// the camera, which puts it in the wrong depth slice - and a fragment in the
// wrong slice looks at the wrong lights, and the scene comes out black.
fn distance_to_camera(world_position: vec3<f32>) -> f32 {
    let clip = uniforms.mvp * vec4<f32>(world_position, 1.0);
    let ndc = clip.xyz / clip.w;
    let near = uniforms.grid.w;
    let far = uniforms.planes.x;
    return max((near * far) / (far - ndc.z * (far - near)), near);
}

// The cluster this fragment belongs to.
//
// The cluster is found from the **world position**, not from the fragment's own
// position: the world position is an honest varying, and the same matrix the
// camera uses turns it into the clip space, and from there into the tile and
// the depth.
fn cluster_of(world_position: vec3<f32>) -> u32 {
    let tiles_x = u32(uniforms.grid.x);
    let tiles_y = u32(uniforms.grid.y);
    let slices = u32(uniforms.grid.z);

    let clip = uniforms.mvp * vec4<f32>(world_position, 1.0);
    let ndc = clip.xyz / clip.w;

    // The screen's [0, 1] range. The x grows to the right and the y grows
    // *downwards*, while the clip space's y grows upwards.
    let screen_x = clamp(ndc.x * 0.5 + 0.5, 0.0, 0.9999);
    let screen_y = clamp(0.5 - ndc.y * 0.5, 0.0, 0.9999);
    let tile_x = min(u32(screen_x * f32(tiles_x)), tiles_x - 1u);
    let tile_y = min(u32(screen_y * f32(tiles_y)), tiles_y - 1u);

    let distance = distance_to_camera(world_position);
    let near = uniforms.grid.w;
    let far = uniforms.planes.x;
    let slice = min(u32((distance - near) / max(far - near, 0.0001) * f32(slices)), slices - 1u);

    return (tile_y * tiles_x + tile_x) * slices + slice;
}

// The cascade the fragment falls into, by its distance.
fn cascade_of(distance: f32) -> u32 {
    var cascade = 0u;
    for (var i = 0u; i < CASCADES; i = i + 1u) {
        if (distance > uniforms.splits[i]) {
            cascade = i + 1u;
        }
    }
    return min(cascade, CASCADES - 1u);
}

// The shadow of one cascade, for a fragment.
fn cascade_shadow(cascade: u32, world_position: vec3<f32>, distance_to_light: f32) -> f32 {
    let matrix = uniforms.light_space[cascade];
    let light_space = matrix * vec4<f32>(world_position, 1.0);
    let ndc = light_space.xyz / light_space.w;
    if (ndc.x <= 0.0 || ndc.x >= 1.0 || ndc.y <= 0.0 || ndc.y >= 1.0 || ndc.z <= 0.0 || ndc.z >= 1.0) {
        // Outside the map there is no shadow: a fragment beyond the light's
        // frustum is lit, not black.
        return 1.0;
    }
    // The map's row 0 is the top of the image, and the light's space has y
    // growing upwards.
    let uv = vec2<f32>(ndc.x, 1.0 - ndc.y);
    // The bias: a surface shadows itself, and without a bias every lit
    // surface is its own shadow. It is a depth in the light's units, so it
    // grows with the distance to the light.
    let bias = 0.0015 * max(distance_to_light, 1.0);
    let reference = ndc.z - bias;
    if (cascade == 0u) {
        return textureSampleCompare(shadow_map_0, shadow_sampler, uv, reference);
    } else if (cascade == 1u) {
        return textureSampleCompare(shadow_map_1, shadow_sampler, uv, reference);
    } else if (cascade == 2u) {
        return textureSampleCompare(shadow_map_2, shadow_sampler, uv, reference);
    }
    return textureSampleCompare(shadow_map_3, shadow_sampler, uv, reference);
}

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;
    output.clip_position = uniforms.mvp * vec4<f32>(input.position, 1.0);
    output.world_position = (uniforms.model * vec4<f32>(input.position, 1.0)).xyz;
    output.world_normal = normalize((uniforms.model * vec4<f32>(input.normal, 0.0)).xyz);
    output.color = input.color;
    return output;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let normal = normalize(input.world_normal);
    let cluster = cluster_of(input.world_position);
    let distance = distance_to_camera(input.world_position);
    let cascade = cascade_of(distance);

    // The cluster's lights: the slots that are not marked as empty.
    var lit = vec3<f32>(0.0);
    for (var slot = 0u; slot < SLOTS; slot = slot + 1u) {
        let light_index = cluster_indices[cluster * SLOTS + slot];
        if (light_index == EMPTY_SLOT) {
            continue;
        }
        let light = lights.lights[light_index];

        let to_light = light.position - input.world_position;
        let distance_to_light = length(to_light);
        let light_dir = to_light / max(distance_to_light, 0.0001);
        let ndotl = max(dot(normal, light_dir), 0.0);

        // The shadow, for the light the map belongs to.
        var shadow = 1.0;
        if (f32(light_index) == uniforms.planes.y) {
            shadow = cascade_shadow(cascade, input.world_position, distance_to_light);
        }

        // The attenuation: inverse square, and the radius fades the light out
        // at its edge instead of letting it reach infinity.
        let radius = max(light.radius, 0.0001);
        let falloff = clamp(1.0 - distance_to_light / radius, 0.0, 1.0);
        let attenuation = falloff * falloff / (1.0 + distance_to_light * distance_to_light * 0.05);

        lit = lit + light.color * light.intensity * ndotl * attenuation * shadow;
    }
    return vec4<f32>(input.color.rgb * (uniforms.ambient.rgb + lit), 1.0);
}
