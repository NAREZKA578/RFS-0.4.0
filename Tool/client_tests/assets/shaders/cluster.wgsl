// The light clustering compute pass.
//
// Forward+ does not send every light to every pixel. It divides the screen
// into tiles, each tile into depth slices, and fills each of those cells — a
// cluster — with the indices of the lights that reach into it. A pixel then
// asks only its own cluster which lights it has to look at.
//
// The cluster's shape in the world is the frustum slab under one tile between
// two depths, and its bounding box is what a light's sphere is tested
// against. A light that does not touch the box is not in the cluster, and a
// pixel in the cluster will not see it.
//
// The work is organised **per cluster**, not per light: one invocation owns
// one cluster and writes that cluster's list. That is what makes the shader
// free of atomics — the list of a cluster is written by exactly one
// invocation, so nothing can collide with it and no counter is needed.
//
// It is also the cheaper arrangement, and by a lot. The box of a cluster is
// the same for every light that might reach it, so it is computed once per
// cluster instead of once per (light, cluster) pair. With sixteen by nine
// tiles, twenty-four slices and a hundred lights, that is twenty-seven
// thousand box constructions instead of two and a half million.

struct ClusterUniforms {
    view_projection: mat4x4<f32>,
    inverse_view_projection: mat4x4<f32>,
    // x = the screen width, y = the height, z = the tiles in x, w = the tiles in y.
    screen_and_tiles: vec4<f32>,
    // x = the depth slices, y = the near plane, z = the far plane, w = the light count.
    slices_and_planes: vec4<f32>,
};

// The light, in the layout the scene's light buffer uses.
//
// `vec3` is 16-aligned inside a struct, so the fields sit at 0, 16, 28 and 32
// and the stride is 48. The two floats after the vectors are at **28 and 32**,
// not at 32 and 36: a member is placed at the next offset that satisfies its
// own alignment, and `f32`'s is 4, not the struct's 16.
//
// The layout is written down once, in `src/render/lighting/light_buffer.rs`,
// and every shader that reads a light has to declare exactly that one — the
// test `light_layout.rs` compares the declarations in the repository with the
// canonical one, so a shader that drifts is caught by a test rather than by a
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

@group(0) @binding(0) var<uniform> uniforms: ClusterUniforms;
@group(0) @binding(1) var<storage, read> lights: Lights;
@group(0) @binding(2) var<storage, read_write> cluster_indices: array<u32>;
@group(0) @binding(3) var<storage, read_write> cluster_counts: array<u32>;

// The maximum number of lights one cluster may hold.
//
// A cluster that overflows it drops the light: the alternative is a cluster
// that holds an unbounded number, and then the *fragment* shader's loop has
// no bound and the cost of a pixel depends on how many lights happen to be
// near it. A fixed bound costs a little accuracy in the crowded case and
// keeps the cost of a pixel known.
const MAX_LIGHTS_PER_CLUSTER: u32 = 32u;

// The value that marks a slot as empty.
//
// The count of a cluster is written next to its list, and the fragment shader
// stops at the count. The marker is still written, because a list that says
// how long it is *and* says where it ends is a list that cannot be read past
// its end even if the count is stale.
const EMPTY_SLOT: u32 = 0xFFFFFFFFu;

// The world-space box of one cluster: the frustum slab under one tile between
// two depths.
fn cluster_bounds(tile_x: u32, tile_y: u32, slice: u32) -> array<vec3<f32>, 2> {
    let tiles_x = u32(uniforms.screen_and_tiles.z);
    let tiles_y = u32(uniforms.screen_and_tiles.w);
    let slices = u32(uniforms.slices_and_planes.x);
    let near = uniforms.slices_and_planes.y;
    let far = uniforms.slices_and_planes.z;

    // The tile's edges in the screen's [0, 1] range.
    let x0 = f32(tile_x) / f32(tiles_x);
    let x1 = f32(tile_x + 1u) / f32(tiles_x);
    let y0 = f32(tile_y) / f32(tiles_y);
    let y1 = f32(tile_y + 1u) / f32(tiles_y);

    // The slice's edges in the view space's depth range. The slice is a piece
    // of the *linear* depth range, not of the non-linear one: the depth
    // buffer is non-linear, but a cluster's depth range has to be linear in the
    // view space or the boxes would not nest.
    let z0 = mix(near, far, f32(slice) / f32(slices));
    let z1 = mix(near, far, f32(slice + 1u) / f32(slices));

    // The eight corners of the slab, in the world, and the box they make.
    var minimum = vec3<f32>(1e30, 1e30, 1e30);
    var maximum = vec3<f32>(-1e30, -1e30, -1e30);
    for (var corner = 0u; corner < 8u; corner = corner + 1u) {
        let cx = select(x0, x1, (corner & 1u) != 0u);
        let cy = select(y0, y1, (corner & 2u) != 0u);
        let cz = select(z0, z1, (corner & 4u) != 0u);
        // The inverse view-projection takes the screen's [0, 1] range and the
        // view space's depth to a world position. The depth has to be turned
        // into the clip space's z first: the projection is not linear in the
        // depth, and a cluster's box is in the world.
        //
        // The screen's y grows downwards and the clip space's y grows upwards,
        // so the y is flipped.
        //
        // The z: **Vulkan's projection puts the near plane at z = 0 and the
        // far plane at z = 1**, not at -1 and 1 as OpenGL does. The OpenGL
        // form puts a slice's far end in the wrong place, the boxes of the
        // slices stop nesting, and the light ends up in a slice that does not
        // cover it - while the forward shader, which recovers the distance the
        // other way round, looks in a different slice again.
        let ndc_y = 1.0 - cy * 2.0;
        let ndc_z = (far * (cz - near)) / ((far - near) * cz);
        let ndc = vec4<f32>(cx * 2.0 - 1.0, ndc_y, ndc_z, 1.0);
        let world = uniforms.inverse_view_projection * ndc;
        let point = world.xyz / world.w;
        minimum = min(minimum, point);
        maximum = max(maximum, point);
    }
    return array<vec3<f32>, 2>(minimum, maximum);
}

// Whether a sphere touches a box.
fn sphere_touches_box(centre: vec3<f32>, radius: f32, minimum: vec3<f32>, maximum: vec3<f32>) -> bool {
    // The closest point of the box to the sphere's centre.
    let closest = clamp(centre, minimum, maximum);
    let delta = closest - centre;
    return dot(delta, delta) <= radius * radius;
}

@compute @workgroup_size(64)
fn cs_main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let cluster = gid.x;
    if (cluster >= arrayLength(&cluster_counts)) {
        return;
    }
    let slices = u32(uniforms.slices_and_planes.x);
    let tiles_x = u32(uniforms.screen_and_tiles.z);
    let tiles_y = u32(uniforms.screen_and_tiles.w);

    // The cluster's own place in the grid. The numbering is the one the
    // forward shader reproduces: `(tile_y * tiles_x + tile_x) * slices +
    // slice`.
    let slice = cluster % slices;
    let tile = cluster / slices;
    let tile_x = tile % tiles_x;
    let tile_y = tile / tiles_x;

    let bounds = cluster_bounds(tile_x, tile_y, slice);
    let light_count = u32(uniforms.slices_and_planes.w);

    // The lights that reach into this cluster.
    var count = 0u;
    for (var light_index = 0u; light_index < light_count; light_index = light_index + 1u) {
        if (count >= MAX_LIGHTS_PER_CLUSTER) {
            break;
        }
        let light = lights.lights[light_index];
        if (!sphere_touches_box(light.position, light.radius, bounds[0], bounds[1])) {
            continue;
        }
        cluster_indices[cluster * MAX_LIGHTS_PER_CLUSTER + count] = light_index;
        count = count + 1u;
    }
    cluster_counts[cluster] = count;

    // The unused slots are marked, so a reader that walks the slots cannot run
    // past the end of the list even if it ignores the count.
    for (var slot = count; slot < MAX_LIGHTS_PER_CLUSTER; slot = slot + 1u) {
        cluster_indices[cluster * MAX_LIGHTS_PER_CLUSTER + slot] = EMPTY_SLOT;
    }
}
