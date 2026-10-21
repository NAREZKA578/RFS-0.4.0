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
// The culling here is the straightforward one: every light is tested against
// every cluster. That is O(lights x clusters) rather than the O(lights x
// touched clusters) of the usual implementation, and it gives the same answer.
// The screen-space bound on the tiles a light can reach is the optimisation
// that comes later, not the difference between right and wrong.

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
// `vec3` is 16-aligned inside a struct, so the fields sit at 0, 16, 32 and 36
// and the stride is 48. The same buffer is read by the lighting shader, so
// the two have to agree exactly - a stride that is off by eight makes every
// light after the first read the wrong colour.
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
@group(0) @binding(3) var<storage, read_write> cluster_counts: array<atomic<u32>>;

// The maximum number of lights one cluster may hold.
//
// A cluster that overflows it drops the light: the alternative is a cluster
// that holds an unbounded number, and then the *fragment* shader's loop has
// no bound and the cost of a pixel depends on how many lights happen to be
// near it. A fixed bound costs a little accuracy in the crowded case and
// keeps the cost of a pixel known.
const MAX_LIGHTS_PER_CLUSTER: u32 = 32u;

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
    // buffer is non-linear, but a cluster's depth range has to be linear in
    // the view space or the boxes would not nest.
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

// The value that marks a slot as empty.
//
// A fragment shader may not read an atomic counter unless the device has the
// `fragmentStoresAndAtomics` feature, which is not something to depend on.
// So the slots are marked instead: an unused slot holds this, and the
// fragment shader walks the slots and skips the marked ones.
const EMPTY_SLOT: u32 = 0xFFFFFFFFu;

// Zero the counts, and mark every slot as empty.
//
// The binning pass *adds* to the counts, so they have to start at zero. Doing
// it here, on the GPU, is not a detail: the host cannot do it, because the
// host's write would race with the previous frame's binning, which may still
// be running.
@compute @workgroup_size(64)
fn cs_clear(@builtin(global_invocation_id) gid: vec3<u32>) {
    let index = gid.x;
    if (index >= arrayLength(&cluster_counts)) {
        return;
    }
    atomicStore(&cluster_counts[index], 0u);
    // The slots of this cluster: the cluster's count is the number of slots
    // it has, and each cluster has MAX_LIGHTS_PER_CLUSTER of them.
    let per_cluster = arrayLength(&cluster_indices) / arrayLength(&cluster_counts);
    let cluster = index;
    for (var slot = 0u; slot < per_cluster; slot = slot + 1u) {
        cluster_indices[cluster * per_cluster + slot] = EMPTY_SLOT;
    }
}

@compute @workgroup_size(64)
fn cs_main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let light_index = gid.x;
    if (light_index >= u32(uniforms.slices_and_planes.w)) {
        return;
    }
    let light = lights.lights[light_index];
    let centre = light.position;
    let radius = light.radius;

    let tiles_x = u32(uniforms.screen_and_tiles.z);
    let tiles_y = u32(uniforms.screen_and_tiles.w);
    let slices = u32(uniforms.slices_and_planes.x);

    for (var tile_y = 0u; tile_y < tiles_y; tile_y = tile_y + 1u) {
        for (var tile_x = 0u; tile_x < tiles_x; tile_x = tile_x + 1u) {
            for (var slice = 0u; slice < slices; slice = slice + 1u) {
                let bounds = cluster_bounds(tile_x, tile_y, slice);
                if (!sphere_touches_box(centre, radius, bounds[0], bounds[1])) {
                    continue;
                }
                let cluster = (tile_y * tiles_x + tile_x) * slices + slice;
                let count = atomicAdd(&cluster_counts[cluster], 1u);
                if (count < MAX_LIGHTS_PER_CLUSTER) {
                    cluster_indices[cluster * MAX_LIGHTS_PER_CLUSTER + count] = light_index;
                } else {
                    // The cluster is full. The count is put back, or the
                    // fragment shader would read a count larger than the
                    // buffer's capacity and run off the end of it.
                    atomicSub(&cluster_counts[cluster], 1u);
                }
            }
        }
    }
}
