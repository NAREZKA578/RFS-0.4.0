// Water surface shader for the CPU Gerstner ocean.
//
// The geometry pass still supplies the displaced grid and its world-space
// normal. This shader adds what makes water read as water and what the plain
// geometry shader cannot:
//
//   * Fresnel sky reflection, so glancing angles turn bright and looking
//     straight down stays dark;
//   * moving procedural ripple normals, domain-warped so the drift meanders
//     like wind-tipped chop instead of running in parallel streaks, at two
//     scales — detail both between the mesh vertices and inside a single
//     swell;
//   * a broad sun path plus tight glitter facets;
//   * foam on crests and in the hull's churn, its edges torn into ridged
//     filaments rather than round milky patches;
//   * light transmitted through the thin, backlit part of a crest, weighted
//     by how steep the face is;
//   * distance fog that meets the clear colour at the horizon.
//
// The first four vec4s of the uniform block are identical to `geometry.wgsl`,
// which is what lets the geometry pass own both pipelines and write one buffer
// layout. Everything after `ambient` is water-only.
//
// Attribute locations match `vertex_input_state()`:
//   0 position  vec3   offset 0
//   1 normal    vec3   offset 12
//   2 tangent   vec4   offset 24
//   3 tex_coord vec2   offset 40
//   4 color     vec4   offset 48

struct Uniforms {
    mvp: mat4x4<f32>,
    light_dir: vec4<f32>,
    light_color: vec4<f32>,
    ambient: vec4<f32>,
    // The item's model matrix. The ocean is already in world space, but the
    // matrix keeps the shader correct for a transformed water entity too.
    model: mat4x4<f32>,
    // xyz = camera position in world space.
    camera_position: vec4<f32>,
    // xyz = normalised direction *towards* the sun, in world space.
    sun_direction: vec4<f32>,
    // x = seconds, y = clarity, z = foam intensity, w = reserved.
    water_params: vec4<f32>,
    // rgb = fog/horizon colour, a = fog density.
    fog_color: vec4<f32>,
    // rgb = zenith colour, a = reflection strength.
    sky_color: vec4<f32>,
    deep_color: vec4<f32>,
    shallow_color: vec4<f32>,
    // x = fresnel bias, y = specular exponent, z = ripple strength,
    // w = reserved.
    water_extra: vec4<f32>,
    // The Gerstner field the CPU grid sampled, packed as
    // (direction.x, direction.z, frequency, effective amplitude). The
    // fragment shader differentiates it analytically instead of interpolating
    // three coarse vertex normals across a cell.
    wave0: vec4<f32>,
    wave1: vec4<f32>,
    wave2: vec4<f32>,
    wave3: vec4<f32>,
    // Phase speed of each wave, in radians per second.
    wave_speeds: vec4<f32>,
    // Camera basis for the skybox; the water only reads the sun/time fields
    // above, but the struct keeps the shared buffer offsets identical.
    sky_right: vec4<f32>,
    sky_up: vec4<f32>,
    sky_forward: vec4<f32>,
    // xyz = the hull's waterline point, w = wake radius.
    focus_position: vec4<f32>,
    // x = wake strength, y = spray strength, z = wave envelope (the largest
    // absolute height the field can reach), w = reserved.
    effect_params: vec4<f32>,
}

@group(0) @binding(0)
var<uniform> uniforms: Uniforms;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) tangent: vec4<f32>,
    @location(3) tex_coord: vec2<f32>,
    @location(4) color: vec4<f32>,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_position: vec3<f32>,
    @location(1) world_normal: vec3<f32>,
    @location(2) color: vec4<f32>,
    // Undisplaced grid XZ. The vertex was moved sideways by the Gerstner
    // displacement, so its world XZ no longer identifies the wave phase.
    @location(3) wave_position: vec2<f32>,
}

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;
    output.clip_position = uniforms.mvp * vec4<f32>(input.position, 1.0);
    output.world_position = (uniforms.model * vec4<f32>(input.position, 1.0)).xyz;

    // A zero-length normal would poison `normalize` with NaN and the whole
    // triangle would disappear.
    var n = input.normal;
    if (dot(n, n) < 1e-12) {
        n = vec3<f32>(0.0, 1.0, 0.0);
    }
    output.world_normal = normalize(n);
    output.color = input.color;
    output.wave_position = input.tex_coord;
    return output;
}

// Cheap 2D hash in [0, 1). No sine: sin-based hashes band on large world
// coordinates, and the foam needs to look random at every scale.
fn hash21(p: vec2<f32>) -> f32 {
    var p3 = fract(vec3<f32>(p.x, p.y, p.x) * 0.1031);
    p3 += dot(p3, p3.yzx + 33.33);
    return fract((p3.x + p3.y) * p3.z);
}

// Smooth value noise. The lattice is regular but its values are not, so it
// reads as blobs rather than as the diamond grid a sine product produces.
fn value_noise(p: vec2<f32>) -> f32 {
    let cell = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash21(cell);
    let b = hash21(cell + vec2<f32>(1.0, 0.0));
    let c = hash21(cell + vec2<f32>(0.0, 1.0));
    let d = hash21(cell + vec2<f32>(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

// Two octaves: one broad patch plus one finer breakup. Good enough for foam
// and spray, and much cheaper than a full spectral noise.
fn fbm2(p: vec2<f32>) -> f32 {
    return value_noise(p) * 0.65
        + value_noise(p * 2.17 + vec2<f32>(13.7, 7.1)) * 0.35;
}

// Broad ripple: four octaves of drifting value noise at roughly 6 m, 2 m,
// 0.7 m and 0.25 m. The long octave bridges the gap between the 30 m grid
// wave and the capillary field. Noise is used instead of a sine because a
// single 15 m sine drew regular parallel ridges across the whole sea, which
// is exactly the "corduroy" look this pass is trying to avoid — and even the
// multi-octave version kept faint streaks in glancing light, so the domain
// is warped below.
fn capillary_height(position: vec2<f32>, time: f32, footprint: f32) -> f32 {
    let fade0 = 1.0 - smoothstep(1.20, 4.00, footprint);
    let fade1 = 1.0 - smoothstep(0.50, 1.60, footprint);
    let fade2 = 1.0 - smoothstep(0.18, 0.60, footprint);
    let fade3 = 1.0 - smoothstep(0.07, 0.24, footprint);

    // Domain warp: a slow, broad noise bends the sampling position before
    // any octave is evaluated. Drifting noise alone still showed its cells
    // as soft parallel streaks; a warped domain turns them into meandering
    // filaments — wind-tipped chop — for the price of two lookups.
    let warp_drift = vec2<f32>(time * 0.03, -time * 0.021);
    let warp = vec2<f32>(
        value_noise(position * 0.045 + warp_drift),
        value_noise(position * 0.052 - warp_drift + vec2<f32>(7.3, 2.1)),
    ) - 0.5;
    let warped = position + warp * 5.0;

    // Slightly anisotropic scales: wind-driven chop is longer along the wind
    // than across it.
    let p0 = vec2<f32>(warped.x * 0.16, warped.y * 0.23)
        + vec2<f32>(time * 0.045, -time * 0.028);
    let p1 = vec2<f32>(warped.x * 0.52, warped.y * 0.44)
        + vec2<f32>(-time * 0.09, time * 0.07);
    let p2 = vec2<f32>(warped.x * 1.60, warped.y * 1.30)
        + vec2<f32>(time * 0.20, -time * 0.16);
    let p3 = vec2<f32>(warped.x * 4.30, warped.y * 3.70)
        + vec2<f32>(-time * 0.38, time * 0.30);

    return (value_noise(p0) - 0.5) * 0.95 * fade0
        + (value_noise(p1) - 0.5) * 0.50 * fade1
        + (value_noise(p2) - 0.5) * 0.22 * fade2
        + (value_noise(p3) - 0.5) * 0.10 * fade3;
}

// Slope of the capillary field from central differences. Three height
// samples per fragment are cheaper than ten sine/cosine pairs and produce a
// much less regular surface.
fn capillary_slope(position: vec2<f32>, time: f32, footprint: f32) -> vec2<f32> {
    let step = 0.06;
    let h = capillary_height(position, time, footprint);
    let hx = capillary_height(position + vec2<f32>(step, 0.0), time, footprint);
    let hz = capillary_height(position + vec2<f32>(0.0, step), time, footprint);
    return vec2<f32>((hx - h) / step, (hz - h) / step) * 0.60;
}

// Height of the same four-wave field the CPU grid sampled. The grid stores a
// normalised crest in `color.a`, but that value is interpolated across a cell
// several metres wide, so foam cut from it showed the triangles. Evaluating the
// field per fragment makes the crest and foam continuous.
fn gerstner_height(wave: vec4<f32>, speed: f32, position: vec2<f32>, time: f32) -> f32 {
    let theta = dot(wave.xy, position) * wave.z + time * speed;
    return sin(theta) * wave.w;
}

// Parameter-space tangents of one Gerstner component. The surface is
//   P = (x0 - qa*dx*cos(theta), A*sin(theta), z0 - qa*dz*cos(theta)),
// so dP/dx0 and dP/dz0 are no longer the coordinate axes: a crest is
// compressed sideways and a trough is stretched. The cross product of the
// summed tangents is the true surface normal.
struct Tangents {
    tx: vec3<f32>,
    tz: vec3<f32>,
}

fn gerstner_tangents(
    wave: vec4<f32>,
    speed: f32,
    choppiness: f32,
    position: vec2<f32>,
    time: f32,
) -> Tangents {
    let theta = dot(wave.xy, position) * wave.z + time * speed;
    let s = sin(theta);
    let c = cos(theta);
    let k = wave.z;
    let amplitude = wave.w;
    // Same cap as `get_horizontal_displacement` on the CPU: four waves at
    // 0.22 keep the sum below the fold-over limit of one.
    let qa = min(amplitude * choppiness, 0.22 / max(k, 1e-4));
    let dx = wave.x;
    let dz = wave.y;

    var result: Tangents;
    result.tx = vec3<f32>(
        1.0 - qa * dx * dx * k * s,
        amplitude * k * dx * c,
        -qa * dx * dz * k * s,
    );
    result.tz = vec3<f32>(
        -qa * dx * dz * k * s,
        amplitude * k * dz * c,
        1.0 - qa * dz * dz * k * s,
    );
    return result;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let time = uniforms.water_params.x;
    let foam_intensity = uniforms.water_params.z;
    let sun = normalize(uniforms.sun_direction.xyz);

    let to_camera = uniforms.camera_position.xyz - input.world_position;
    let distance = length(to_camera);
    let view_dir = to_camera / max(distance, 1e-4);

    // World-space size of one pixel on the surface. Every detail layer fades
    // when its own wavelength approaches this, which is both an anti-aliasing
    // measure and a performance one: distant water stops computing detail it
    // could not show.
    let footprint = max(length(fwidth(input.world_position.xz)), 5e-4);

    // The wave phase is defined in the undisplaced grid, not in world space:
    // the vertex shader moved the point sideways to sharpen the crest, so
    // evaluating the field at the displaced XZ would slide normals and foam
    // off the crest they belong to.
    let wave_position = input.wave_position;

    // Crest height, evaluated per fragment from the same four waves that shaped
    // the grid. `color.a` is only a per-vertex value: using it directly made the
    // foam break along triangle edges when the sea was rough. If no wave field
    // was uploaded (flat sea), the mesh crest is the only information there is.
    let wave_envelope = uniforms.effect_params.z;
    let wave_height = gerstner_height(
            uniforms.wave0, uniforms.wave_speeds.x, wave_position, time)
        + gerstner_height(
            uniforms.wave1, uniforms.wave_speeds.y, wave_position, time)
        + gerstner_height(
            uniforms.wave2, uniforms.wave_speeds.z, wave_position, time)
        + gerstner_height(
            uniforms.wave3, uniforms.wave_speeds.w, wave_position, time);
    var crest = clamp(input.color.a, 0.0, 1.0);
    if (wave_envelope > 1e-4) {
        crest = clamp(wave_height / wave_envelope * 0.5 + 0.5, 0.0, 1.0);
    }

    // Four-octave noise chop from 6 m down to 25 cm. It is irregular by
    // construction, so no domain warp is needed.
    let ripple_gain = uniforms.water_extra.z;
    var ripple = capillary_slope(wave_position, time, footprint) * ripple_gain;

    // Distance at which crest foam stops being resolvable. Used by the wake
    // branch as well, so it is computed once.
    let foam_fade = 1.0 - smoothstep(120.0, 320.0, distance);

    // The hull churns the water it sits in: a ring of radial waves and foam
    // that fades within a few hull widths. This is deliberately local — an
    // infinite wake would read as a texture, not as the ship's effect.
    // Everything expensive below is gated on this mask: open sea, which is
    // most of the screen, skips the wake noise entirely.
    let wake_strength = uniforms.effect_params.x;
    let wake_radius = max(uniforms.focus_position.w, 1e-3);
    let to_hull = input.world_position.xz - uniforms.focus_position.xz;
    let hull_distance = length(to_hull);
    let hull_dir = to_hull / max(hull_distance, 1e-4);
    let wake_fade = (1.0 - smoothstep(wake_radius, wake_radius * 4.5, hull_distance))
        * wake_strength;

    let wake_active = wake_fade > 0.003;
    var wake_ripple = vec2<f32>(0.0);
    if (wake_active) {
        // Noise in the phase breaks the concentric rings into irregular
        // wavefronts.
        let wake_phase = hull_distance * 2.2 - time * 4.2
            + (value_noise(input.world_position.xz * 0.55 + vec2<f32>(time * 0.12, 0.0))
                - 0.5) * 2.6;
        let wake_wave = sin(wake_phase) * wake_fade;
        wake_ripple = hull_dir * (wake_wave * 0.20);
    }
    ripple += wake_ripple;

    // If the wave field is present (a non-zero effective amplitude), rebuild
    // the true Gerstner normal per fragment. A water mesh supplied without
    // wave metadata falls back to the normals its vertices carried.
    let wave_strength = uniforms.wave0.w + uniforms.wave1.w
        + uniforms.wave2.w + uniforms.wave3.w;
    var base_normal = normalize(input.world_normal);
    if (wave_strength > 1e-6) {
        let choppiness = uniforms.water_extra.w;
        let t0 = gerstner_tangents(
            uniforms.wave0, uniforms.wave_speeds.x, choppiness, wave_position, time);
        let t1 = gerstner_tangents(
            uniforms.wave1, uniforms.wave_speeds.y, choppiness, wave_position, time);
        let t2 = gerstner_tangents(
            uniforms.wave2, uniforms.wave_speeds.z, choppiness, wave_position, time);
        let t3 = gerstner_tangents(
            uniforms.wave3, uniforms.wave_speeds.w, choppiness, wave_position, time);
        let tangent_x = t0.tx + t1.tx + t2.tx + t3.tx;
        let tangent_z = t0.tz + t1.tz + t2.tz + t3.tz;
        base_normal = normalize(cross(tangent_z, tangent_x));
    }

    // Tangent of the surface tilt from the true normal. The height gradient
    // has a different phase once crests are compressed, so foam and the
    // shallow-water tint use this instead.
    let surface_slope = length(base_normal.xz) / max(base_normal.y, 1e-4);

    let normal = normalize(vec3<f32>(
        base_normal.x - ripple.x,
        base_normal.y,
        base_normal.z - ripple.y,
    ));

    // Same tilt including the capillary field. Foam must stay on the large
    // breaking crests, but the body colour benefits from the fine ripples:
    // without this the sea away from the sun reads as smooth blue jelly.
    let detail_slope = length(normal.xz) / max(normal.y, 1e-4);

    let ndv = clamp(dot(normal, view_dir), 0.0, 1.0);
    let fresnel = uniforms.water_extra.x
        + (1.0 - uniforms.water_extra.x) * pow(1.0 - ndv, 5.0);

    // Sky reflection. The gradient mirrors `sky.wgsl`: the horizon is the haze
    // colour the sea itself fades into, the zenith is the sky's own zenith —
    // the same two colours, mixed by the same altitude curve the sky renders
    // with. The old curve additionally multiplied the zenith by 0.16–0.20,
    // which was a correction for the *old*, near-black sky pair; with the
    // bright haze it made the reflected sky much darker than the sky it
    // claims to reflect, and the sea read as an opaque painted surface.
    let reflected = reflect(-view_dir, normal);
    let up = clamp(reflected.y, 0.0, 1.0);
    var reflection = mix(uniforms.fog_color.rgb, uniforms.sky_color.rgb, pow(up, 0.55));

    // Warm haze around the sun's azimuth, fading with altitude exactly as in
    // the sky shader. The flat direction is guarded: a reflection pointing
    // straight up has no azimuth and would normalise a zero vector.
    let flat_length = length(reflected.xz);
    let reflected_flat = reflected.xz / max(flat_length, 1e-4);
    let sun_flat = normalize(uniforms.sun_direction.xz);
    let sun_azimuth = max(dot(reflected_flat, sun_flat), 0.0);
    reflection += vec3<f32>(0.075, 0.042, 0.018)
        * pow(sun_azimuth, 5.0)
        * exp(-up * 5.0)
        * smoothstep(0.0, 0.10, up);

    // The reflected sun: a broad atmospheric halo plus the tight core. The
    // core uses a high power because the solar disk is only half a degree
    // across; a wider lobe smeared the path into large white blobs instead
    // of letting the ripple facets break it into glints.
    let sun_dot = max(dot(reflected, sun), 0.0);
    reflection += vec3<f32>(1.0, 0.78, 0.52)
        * (pow(sun_dot, 24.0) * 0.020 + pow(sun_dot, 900.0) * 0.30);

    // Cloud mottling. The sky is not a clean gradient, and a perfectly smooth
    // reflected sky is what made the sea look like painted metal. The water
    // cannot sample the sky pass, so it projects the reflected direction onto
    // a drifting cloud plane with the same faint grey highlight. This is one
    // value-noise lookup, and only where the reflected ray can see sky.
    let cloud_fade = smoothstep(0.02, 0.10, up) * (1.0 - smoothstep(0.70, 0.95, up));
    if (cloud_fade > 0.001) {
        let cloud_plane = reflected.xz / max(up + 0.16, 0.12);
        let cloud = value_noise(cloud_plane * 2.2 + vec2<f32>(time * 0.006, time * 0.002));
        reflection += vec3<f32>(0.035, 0.038, 0.044)
            * smoothstep(0.46, 0.70, cloud) * cloud_fade;
    }

    let diffuse = max(dot(normal, sun), 0.0);

    var body = mix(
        uniforms.deep_color.rgb,
        uniforms.shallow_color.rgb,
        clamp(crest * 0.82 + detail_slope * 0.22 + 0.03, 0.0, 1.0),
    );
    body *= 0.26 + 0.74 * diffuse;
    body += uniforms.ambient.rgb * 0.05;
    // Skylight fills the water even when the surface faces away from the sun;
    // without it the body colour collapses to nearly black between crests.
    // The fill is also what keeps the near field, where the Fresnel reflection
    // is weak, from reading as black paint under the bright sky. The old
    // 0.10 was tuned against the sky's old, near-black zenith; the brighter
    // one left a dark band across the mid distance that no light explained.
    body += uniforms.sky_color.rgb * 0.16;

    // Light bleeding through the thin, backlit part of a crest. The camera is
    // on the sunlit side when `view_dir` points towards the sun; the previous
    // expression used `-sun`, so it lit the crests the sun had already left.
    // The glow is now weighted by the face's slope as well: a flat, thick
    // swell top transmits nothing, a steep thin face glows — the same tilt
    // range the foam's break-mask uses, so backlight and whitecap read as one
    // phenomenon. The coefficient comes up because the gate spends most of
    // the old energy budget on faces that never transmitted at all.
    let backlight = pow(clamp(dot(view_dir, sun), 0.0, 1.0), 2.0)
        * smoothstep(0.04, 0.14, surface_slope);
    body += uniforms.shallow_color.rgb * crest * backlight * 0.55;

    // Sun glitter. Three lobes: a broad glow that lays the path, a mid lobe
    // that sharpens it, and a tight lobe that the fine ripples break into
    // individual flashes. The old single 220-power lobe was multiplied by
    // N·L, which at this sun elevation cut it to a quarter and left the water
    // with no path at all.
    let half_vector = normalize(sun + view_dir);
    let ndoth = max(dot(normal, half_vector), 0.0);
    let broad = pow(ndoth, 12.0);
    let mid = pow(ndoth, 64.0);
    let tight = pow(ndoth, 700.0);
    // The normals alone still leave the tight lobe as long thin curves where
    // a whole run of facets happens to catch the sun. A high-frequency noise
    // gate breaks those curves into separate flashes. It fades out once its
    // cells approach a pixel, so distant glitter does not shimmer.
    let sparkle = value_noise(
        input.world_position.xz * 5.0 + vec2<f32>(time * 0.9, -time * 0.8),
    );
    let sparkle_fade = 1.0 - smoothstep(0.08, 0.35, footprint);
    let sparkle_mask = mix(
        1.0,
        0.15 + 0.85 * smoothstep(0.42, 0.68, sparkle),
        sparkle_fade,
    );
    // The lobes alone lit every facet that happened to face the half vector,
    // wherever it stood on the sea: the frame read as uniform sequins instead
    // of a path under the sun. Real glitter lives near the mirror image of the
    // sun, which at this elevation is a patch near the horizon on the sun's
    // azimuth. The same azimuth alignment the warm haze uses concentrates the
    // lobes there; the floor keeps stray flashes over the rest of the sea,
    // only dimmer. The broad and mid lobes were also too weak to lay a visible
    // path at this sun height, so they come up; the tight flash lobe does not.
    let path_gate = mix(0.30, 1.0, pow(sun_azimuth, 3.0));
    let specular = (broad * 0.022 + mid * 0.075 + tight * 0.75 * sparkle_mask)
        * path_gate;
    var color = mix(
        body,
        reflection,
        clamp(fresnel * uniforms.sky_color.a, 0.0, 1.0),
    );
    color += uniforms.light_color.rgb * specular;

    // Spray glints and the hull's churn. Both are confined to the wake, so
    // they share one branch; the open sea never evaluates the noise.
    var wake_foam = 0.0;
    var spray = 0.0;
    if (wake_active) {
        // A noisy ring just outside the waterline, widest where the bow
        // pushes water aside. It stays narrow: a stationary hull does not
        // trail a broad wake, and a wide ring reads as foam glued to the
        // object rather than as water.
        let ring = smoothstep(wake_radius * 0.92, wake_radius * 1.00, hull_distance)
            * (1.0 - smoothstep(wake_radius * 1.02, wake_radius * 1.45, hull_distance));

        // Two scales: broad patches plus small bubbles. A single low frequency
        // made milky islands instead of foam.
        let churn_point = input.world_position.xz * 2.4
            + vec2<f32>(time * 0.55, -time * 0.42);
        let churn = fbm2(churn_point) * 0.72
            + value_noise(churn_point * 3.7 + vec2<f32>(time * 1.1, -time * 0.9)) * 0.28;
        // Low-frequency mask breaks the perfect torus: thicker in some places,
        // open water in others.
        let ring_mask = smoothstep(
            0.30,
            0.72,
            value_noise(input.world_position.xz * 0.38
                + vec2<f32>(time * 0.06, time * 0.04)),
        );
        wake_foam = ring * smoothstep(0.55, 0.80, churn)
            * mix(0.45, 1.0, ring_mask) * wake_strength * foam_fade;

        // Spray glints: the churn breaks the surface into tiny facets that
        // flash as they catch the sun. The 420-power lobe keeps them confined
        // to the sun path; the branch around it means the rest of the wake
        // pays nothing for it.
        if (ndoth > 0.88) {
            let spray_mask = wake_fade
                * (1.0 - smoothstep(wake_radius * 0.8, wake_radius * 3.0, hull_distance));
            let spray_point = input.world_position.xz;
            let facets = value_noise(spray_point * 6.5 + vec2<f32>(time * 0.9, -time * 0.7))
                * value_noise(spray_point * 15.0 - vec2<f32>(time * 1.2, time * 0.8));
            spray = pow(ndoth, 420.0) * spray_mask * 3.0
                * smoothstep(0.14, 0.52, facets);
        }
    }
    color += uniforms.light_color.rgb * spray;

    // Foam gathers on the tallest, steepest crests. The threshold is
    // deliberately high: a broad band of white over every crest reads as snow
    // rather than as whitecaps. Two noise octaves give the edge a lacy break
    // instead of the smooth blobs a single octave produced, and the slope
    // term puts foam on the faces that are actually breaking.
    var foam = 0.0;
    if (foam_fade > 0.002) {
        let foam_noise = value_noise(
            wave_position * 1.15 + vec2<f32>(time * 0.11, -time * 0.08),
        );
        // The two finer octaves alias into shimmer once their cells drop below
        // a pixel. Fade them to their mean instead of letting them sparkle.
        let detail_fade = 1.0 - smoothstep(0.25, 0.90, footprint);
        let bubble_fade = 1.0 - smoothstep(0.08, 0.30, footprint);
        let foam_detail = mix(
            0.5,
            value_noise(wave_position * 3.10 - vec2<f32>(time * 0.24, time * 0.18)),
            detail_fade,
        );
        // Bubble-scale breakup. Without it the foam reads as a milky blob;
        // with it the patch keeps holes and torn edges at the scale of the
        // whitecaps.
        let foam_bubbles = mix(
            0.5,
            value_noise(wave_position * 9.0 + vec2<f32>(time * 0.30, -time * 0.22)),
            bubble_fade,
        );
        // Height alone saturates: the normalised crest is clamped, so a whole
        // plateau reads as "1". Foam needs all three conditions — near the
        // crest, on a face steep enough to break, and inside a ragged noise
        // patch. That turns the former snow fields into scattered whitecaps.
        // The slope range follows the current spectrum: long swell tops out
        // near 0.21, where the old 8-13 m chop reached 0.7.
        let crest_mask = smoothstep(0.58, 0.80, crest);
        let break_mask = smoothstep(0.06, 0.16, surface_slope);
        // A ridged octave tears the lace into filaments. Plain noise lumps
        // foam into round milky patches; a ridge — one minus the absolute
        // deviation from the field's mean — runs along the noise contours,
        // which is the stringy, torn edge a real whitecap breaks with. It
        // fades with its own cells like the detail octave above.
        let ridge_cell = value_noise(wave_position * 1.9 + vec2<f32>(time * 0.17, -time * 0.13));
        let ridge = pow(1.0 - abs(ridge_cell * 2.0 - 1.0), 3.0) * detail_fade;
        let lace = smoothstep(
            0.47,
            0.58,
            foam_noise * 0.46 + foam_detail * 0.24 + foam_bubbles * 0.20 + ridge * 0.10,
        );
        // Three masks multiply, so a physically plausible "maybe foam" value
        // is only a few percent white. Sharpen the product: where all three
        // agree the patch is opaque, elsewhere there is no foam at all.
        let foam_band = smoothstep(0.14, 0.32, crest_mask * break_mask * lace);
        foam = clamp(
            foam_band * foam_intensity * foam_fade + wake_foam * 0.35,
            0.0,
            1.0,
        );
    }

    // Foam is white where the sun reaches it and blue-grey in shadow; the old
    // fixed 0.42–0.57 grey read as dirty patches on a bright sea.
    let foam_color = mix(
        vec3<f32>(0.42, 0.50, 0.58),
        vec3<f32>(0.95, 0.97, 1.0),
        clamp(0.30 + 0.70 * diffuse, 0.0, 1.0),
    );
    color = mix(color, foam_color, foam);

    // Distance fog: the sea edge and the horizon vanish into the same colour
    // the pass clears to instead of ending in a hard line.
    let fog = 1.0 - exp(-pow(distance * uniforms.fog_color.a, 1.35));
    color = mix(color, uniforms.fog_color.rgb, clamp(fog, 0.0, 1.0));

    return vec4<f32>(color, 1.0);
}
