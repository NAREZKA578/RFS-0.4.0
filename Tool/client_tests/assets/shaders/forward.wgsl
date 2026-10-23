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
// coarse. The map is read with a comparison sampler, which is the only sampler
// a depth image may be filtered by.
//
// The light is positional: a point with a radius. The attenuation is the
// inverse square with a radius that fades the light out at its edge, so a
// light does not light the whole world.
//
// The lights are read from the **clustering pass's own buffer** — the same
// buffer the clustering binned from. Reading a different buffer would give
// indices that point at other lights, and the picture would be lit by
// whatever happens to be at those places.

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

// The per-item block: the camera and the item.
struct ItemUniforms {
    // The camera's view-projection, without the item's model matrix.
    //
    // The model matrix is separate because the cluster of a fragment is found
    // from its **world** position: the cluster belongs to the screen, and the
    // screen is the camera's. A matrix with the model in it would put the
    // fragment in a tile it does not occupy.
    view_projection: mat4x4<f32>,
    model: mat4x4<f32>,
};

// The per-frame block: the cascades, the grid and the ambient term.
struct FrameUniforms {
    // The light's view-projection, one per cascade. The cascade the fragment
    // falls into decides which one is used.
    light_space: array<mat4x4<f32>, 4>,
    // x = the tiles in x, y = the tiles in y, z = the depth slices, w = the near plane.
    grid: vec4<f32>,
    // x = the far plane, y and z and w = unused.
    planes: vec4<f32>,
    // The distance at which each cascade ends.
    splits: vec4<f32>,
    // The ambient term.
    ambient: vec4<f32>,
    // x = the light that casts the shadows (the index in the lights buffer),
    // y = the number of cascades, z and w = unused.
    control: vec4<u32>,
};

// The light, in the layout the scene's light buffer uses.
//
// `vec3` is 16-aligned inside a struct, so the fields sit at 0, 16, 28 and 32
// and the stride is 48. The two floats after the vectors are at **28 and 32**,
// not at 32 and 36: a member is placed at the next offset that satisfies its
// own alignment, and `f32`'s is 4, not the struct's 16.
//
// The layout is written down once, in `src/render/lighting/light_buffer.rs`,
// and every shader that reads a light has to declare exactly this one — the
// test `light_layout.rs` compares the declarations in the repository with the
// canonical one, so a shader that drifts is caught by a test and not by a
// picture that comes out black.
struct Light {
    position: vec3<f32>,
    color: vec3<f32>,
    intensity: f32,
    radius: f32,
};

struct Lights {
    lights: array<Light>,
};

@group(0) @binding(0) var<uniform> item: ItemUniforms;
@group(0) @binding(1) var<uniform> frame: FrameUniforms;
@group(0) @binding(2) var shadow_map_0: texture_depth_2d;
@group(0) @binding(3) var shadow_map_1: texture_depth_2d;
@group(0) @binding(4) var shadow_map_2: texture_depth_2d;
@group(0) @binding(5) var shadow_map_3: texture_depth_2d;
@group(0) @binding(6) var shadow_sampler: sampler_comparison;
@group(0) @binding(7) var<storage, read> lights: Lights;
@group(0) @binding(8) var<storage, read> cluster_indices: array<u32>;

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

// The linear depth of the fragment: the distance along the camera's forward
// axis, in world units.
//
// The cluster's slices are cut in this depth and not in the distance to the
// camera, and the cascades are cut in it too, so both need it.
//
// The depth comes from the **world position** and not from the fragment's
// `@builtin(position)`: that one is the framebuffer coordinate, and its `z` is
// the depth buffer's value while its `w` is the *reciprocal* of the clip
// space's w. Dividing one by the other would multiply the depth by the clip
// w, and a fragment nearer than the far plane would come out deeper than the
// far plane — in the wrong slice, and in the wrong cascade.
//
// The form of the inversion depends on the depth convention: **Vulkan's
// projection puts the near plane at z = 0 and the far plane at z = 1**, not at
// -1 and 1 as OpenGL does. The OpenGL form gives a distance about twice the
// right one for a fragment near the camera.
fn linear_depth(world_position: vec3<f32>) -> f32 {
    let clip = item.view_projection * vec4<f32>(world_position, 1.0);
    let ndc_z = clip.z / clip.w;
    let near = frame.grid.w;
    let far = frame.planes.x;
    return (near * far) / (far - ndc_z * (far - near));
}

// The cluster this fragment belongs to.
//
// The cluster is found from the **world position**, projected by the camera's
// matrix: the cluster belongs to the screen, and the screen is the camera's.
fn cluster_of(world_position: vec3<f32>) -> u32 {
    let tiles_x = u32(frame.grid.x);
    let tiles_y = u32(frame.grid.y);
    let slices = u32(frame.grid.z);

    let clip = item.view_projection * vec4<f32>(world_position, 1.0);
    let ndc = clip.xyz / clip.w;

    // The screen's [0, 1] range. The x grows to the right and the y grows
    // *downwards*, while the clip space's y grows upwards.
    let screen_x = clamp(ndc.x * 0.5 + 0.5, 0.0, 0.9999);
    let screen_y = clamp(0.5 - ndc.y * 0.5, 0.0, 0.9999);
    let tile_x = min(u32(screen_x * f32(tiles_x)), tiles_x - 1u);
    let tile_y = min(u32(screen_y * f32(tiles_y)), tiles_y - 1u);

    let depth = linear_depth(world_position);
    let near = frame.grid.w;
    let far = frame.planes.x;
    let slice = min(u32((depth - near) / max(far - near, 0.0001) * f32(slices)), slices - 1u);

    return (tile_y * tiles_x + tile_x) * slices + slice;
}

// The cascade the fragment falls into, by its depth.
//
// The cascade boundaries are the distances at which the cascades end, and they
// are in the same linear depth the cluster's slices are cut in. The cascade
// count comes from the frame: a light with fewer cascades than the shader
// knows about must not send the fragment to a map that was never rendered.
fn cascade_of(depth: f32) -> u32 {
    let count = min(frame.control.y, CASCADES);
    var cascade = 0u;
    for (var i = 0u; i < CASCADES; i = i + 1u) {
        if (i >= count) {
            break;
        }
        if (depth > frame.splits[i]) {
            cascade = i + 1u;
        }
    }
    return min(cascade, max(count, 1u) - 1u);
}

// The shadow of one cascade, for a fragment.
fn cascade_shadow(cascade: u32, world_position: vec3<f32>, distance_to_light: f32) -> f32 {
    let matrix = frame.light_space[cascade];
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
    let world = item.model * vec4<f32>(input.position, 1.0);
    output.world_position = world.xyz;
    output.clip_position = item.view_projection * world;
    output.world_normal = normalize((item.model * vec4<f32>(input.normal, 0.0)).xyz);
    output.color = input.color;
    return output;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let normal = normalize(input.world_normal);
    let cluster = cluster_of(input.world_position);
    let depth = linear_depth(input.world_position);
    let cascade = cascade_of(depth);

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

        // The shadow, for the light the maps belong to. The other lights have
        // no map, and a light with no map is not in shadow.
        var shadow = 1.0;
        if (light_index == frame.control.x) {
            shadow = cascade_shadow(cascade, input.world_position, distance_to_light);
        }

        // The attenuation: inverse square, and the radius fades the light out
        // at its edge instead of letting it reach infinity.
        let radius = max(light.radius, 0.0001);
        let falloff = clamp(1.0 - distance_to_light / radius, 0.0, 1.0);
        let attenuation = falloff * falloff / (1.0 + distance_to_light * distance_to_light * 0.05);

        lit = lit + light.color * light.intensity * ndotl * attenuation * shadow;
    }
    return vec4<f32>(input.color.rgb * (frame.ambient.rgb + lit), 1.0);
}
