// Procedural skybox for the geometry pass.
//
// The pass clears to a flat colour; this fullscreen triangle replaces it with
// a view-dependent sky. It is not a screen-space gradient: the fragment shader
// rebuilds the world-space view ray from the camera basis packed into the
// shared uniform block, so the horizon, sun and clouds stay fixed in the world
// when the camera turns. Depth testing is off, so the items drawn afterwards
// simply cover it.
//
// `HORIZON` must match `CLEAR_COLOR` in `render/passes/geometry.rs`: the water
// shader fades into that same colour, and a mismatch would leave a visible seam
// along the waterline. On the overcast-bright sky that colour is a light haze,
// so the sea brightens into the distance instead of darkening.
//
// This uses the geometry pass's uniform block. Only the prefix through
// `sky_forward` is read here; the water-only fields keep the offsets identical
// to `water_surface.wgsl`.

struct Uniforms {
    mvp: mat4x4<f32>,
    light_dir: vec4<f32>,
    light_color: vec4<f32>,
    ambient: vec4<f32>,
    model: mat4x4<f32>,
    camera_position: vec4<f32>,
    sun_direction: vec4<f32>,
    // x = seconds. The clouds drift with the same clock as the ripples.
    water_params: vec4<f32>,
    fog_color: vec4<f32>,
    sky_color: vec4<f32>,
    deep_color: vec4<f32>,
    shallow_color: vec4<f32>,
    water_extra: vec4<f32>,
    wave0: vec4<f32>,
    wave1: vec4<f32>,
    wave2: vec4<f32>,
    wave3: vec4<f32>,
    wave_speeds: vec4<f32>,
    // Camera basis and projection constants: xyz = vector, w = aspect or
    // tan(half vertical fov).
    sky_right: vec4<f32>,
    sky_up: vec4<f32>,
    sky_forward: vec4<f32>,
    focus_position: vec4<f32>,
    effect_params: vec4<f32>,
}

@group(0) @binding(0)
var<uniform> uniforms: Uniforms;

// Both colours describe an overcast-bright sky: the horizon is the light haze
// the clouds glow into, the zenith is the blue that shows through cloud gaps.
// The old near-black pair made the sky's own horizon band a hard dark strip
// between the lit clouds and the sea.
const HORIZON: vec3<f32> = vec3<f32>(0.34, 0.40, 0.46);
const ZENITH: vec3<f32> = vec3<f32>(0.10, 0.17, 0.28);
const SUN_COLOR: vec3<f32> = vec3<f32>(1.0, 0.78, 0.52);

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VsOut {
    // One oversized triangle in clip space. uv spans 0..1 across the screen,
    // with y growing upward in this pass.
    var positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0),
    );
    let p = positions[vertex_index];

    var out: VsOut;
    out.clip = vec4<f32>(p, 0.0, 1.0);
    out.uv = vec2<f32>(p.x * 0.5 + 0.5, p.y * 0.5 + 0.5);
    return out;
}

// 3D value noise for the cloud layer. The layer itself is a world-space
// plane (see fs_main), but the noise is 3D so the clouds keep volume instead
// of looking like a flat tiled texture.
fn hash31(p: vec3<f32>) -> f32 {
    var q = fract(p * 0.1031);
    q += dot(q, q.yzx + 33.33);
    return fract((q.x + q.y) * q.z);
}

fn value_noise3(p: vec3<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);

    let n000 = hash31(i);
    let n100 = hash31(i + vec3<f32>(1.0, 0.0, 0.0));
    let n010 = hash31(i + vec3<f32>(0.0, 1.0, 0.0));
    let n110 = hash31(i + vec3<f32>(1.0, 1.0, 0.0));
    let n001 = hash31(i + vec3<f32>(0.0, 0.0, 1.0));
    let n101 = hash31(i + vec3<f32>(1.0, 0.0, 1.0));
    let n011 = hash31(i + vec3<f32>(0.0, 1.0, 1.0));
    let n111 = hash31(i + vec3<f32>(1.0, 1.0, 1.0));

    let x00 = mix(n000, n100, u.x);
    let x10 = mix(n010, n110, u.x);
    let x01 = mix(n001, n101, u.x);
    let x11 = mix(n011, n111, u.x);
    return mix(mix(x00, x10, u.y), mix(x01, x11, u.y), u.z);
}

fn fbm3(p: vec3<f32>) -> f32 {
    var total = 0.0;
    var amplitude = 0.5;
    var point = p;
    for (var octave = 0; octave < 4; octave = octave + 1) {
        total = total + value_noise3(point) * amplitude;
        point = point * 2.07 + vec3<f32>(11.3, 7.1, 5.7);
        amplitude = amplitude * 0.5;
    }
    return total;
}

// Clouds are a finite slab, not a plane. The density is sampled in world space
// on all three axes, so the layer has a base, a top and volume between them:
// an aircraft can climb through it and look down on the tops.
const CLOUD_BOTTOM: f32 = 900.0;
const CLOUD_TOP: f32 = 1800.0;
const CLOUD_SCALE: f32 = 0.00115;
const CLOUD_STEPS: i32 = 12;

fn cloud_density(p: vec3<f32>, time: f32) -> f32 {
    let drift = vec2<f32>(time * 1.1, time * 0.45);
    // The vertical coordinate is stretched so the slab contains several noise
    // cells from base to top instead of one thin sheet.
    let q = vec3<f32>(
        (p.x + drift.x) * CLOUD_SCALE,
        p.y * CLOUD_SCALE * 2.2,
        (p.z + drift.y) * CLOUD_SCALE,
    );
    let shape = fbm3(q);
    let detail = value_noise3(q * 3.7 + vec3<f32>(11.3, 7.1, 5.7));
    let height = clamp((p.y - CLOUD_BOTTOM) / (CLOUD_TOP - CLOUD_BOTTOM), 0.0, 1.0);
    // Cumulus profile: a flat, dense base and a thinner, billowing top.
    let profile = smoothstep(0.0, 0.10, height) * (1.0 - smoothstep(0.55, 1.0, height));
    return smoothstep(0.46, 0.70, shape * 0.82 + detail * 0.18) * profile;
}

fn cloud_slab(origin: vec3<f32>, direction: vec3<f32>) -> vec2<f32> {
    // A horizontal ray never leaves the slab. The horizon fade hides that case;
    // returning an empty span keeps the marcher from looping over it.
    if (abs(direction.y) < 1e-4) {
        return vec2<f32>(1.0, -1.0);
    }
    let inv_y = 1.0 / direction.y;
    let ta = (CLOUD_BOTTOM - origin.y) * inv_y;
    let tb = (CLOUD_TOP - origin.y) * inv_y;
    return vec2<f32>(min(ta, tb), max(ta, tb));
}

@fragment
fn fs_main(input: VsOut) -> @location(0) vec4<f32> {
    let time = uniforms.water_params.x;
    let sun = normalize(uniforms.sun_direction.xyz);

    // Rebuild the view ray this pixel looks along. `uv.y = 0` is the bottom of
    // the screen, so the vertical term must not be flipped here.
    let ndc = input.uv * 2.0 - 1.0;
    let tan_half = uniforms.sky_up.w;
    let aspect = uniforms.sky_right.w;
    let ray = normalize(
        uniforms.sky_forward.xyz
        + uniforms.sky_right.xyz * (ndc.x * aspect * tan_half)
        + uniforms.sky_up.xyz * (ndc.y * tan_half)
    );

    // 0 at the horizon, 1 at the zenith.
    let up = clamp(ray.y, 0.0, 1.0);
    let t = pow(up, 0.55);
    var color = mix(HORIZON, ZENITH, t);

    // Warm haze gathers around the sun's azimuth and fades with altitude. It
    // dies out exactly at the horizon so the sky meets the water's fog colour
    // instead of leaving a bright strip along the seam.
    let sun_dot = max(dot(ray, sun), 0.0);
    let sun_azimuth = max(dot(normalize(vec3<f32>(ray.x, 0.0, ray.z)),
                              normalize(vec3<f32>(sun.x, 0.0, sun.z))), 0.0);
    color += vec3<f32>(0.075, 0.042, 0.018)
        * pow(sun_azimuth, 5.0)
        * exp(-up * 5.0)
        * smoothstep(0.0, 0.10, up);

    // Sun disc and its glow. The disc is small enough to stay crisp without a
    // bloom pass; the wide term is the atmospheric halo. It goes down before
    // the clouds so a dense cloud can occlude the sun and a thin one only dims
    // it.
    let disc = smoothstep(0.99965, 0.99988, sun_dot);
    let halo = pow(sun_dot, 220.0) * 0.55 + pow(sun_dot, 18.0) * 0.10;
    color += SUN_COLOR * ((disc + halo) * smoothstep(-0.02, 0.06, ray.y));

    // Volumetric cloud slab. Each ray is intersected with the layer, then
    // marched through it: density comes from 3D noise, and the accumulated
    // opacity is composited over the sky and sun. This is what gives the
    // clouds a base/top and lets the camera fly through them.
    let horizon_fade = smoothstep(0.012, 0.09, abs(ray.y));
    if (horizon_fade > 0.001) {
        let cam = uniforms.camera_position.xyz;
        let slab = cloud_slab(cam, ray);
        let t_enter = max(slab.x, 0.0);
        let span = slab.y - t_enter;
        if (span > 0.0) {
            let step_size = span / f32(CLOUD_STEPS);
            let jitter = hash31(vec3<f32>(input.clip.xy, time * 60.0));
            var transmittance = 1.0;
            var scattered = vec3<f32>(0.0);

            for (var i = 0; i < CLOUD_STEPS; i = i + 1) {
                let t = t_enter + (f32(i) + jitter) * step_size;
                let pos = cam + ray * t;
                let density = cloud_density(pos, time) * horizon_fade;
                if (density > 0.002) {
                    // One shadow sample per dense step: thin wisps stay
                    // uniformly lit, thick cores get a dark underside and a
                    // bright rim facing the sun.
                    var light = 0.30;
                    if (density > 0.10) {
                        let shadow = cloud_density(pos + sun * 140.0, time);
                        light = 0.22 + 0.78 * exp(-shadow * 3.2);
                    }
                    let silver = pow(max(dot(ray, sun), 0.0), 8.0)
                        * (1.0 - density) * 0.35;
                    var sample_color = mix(
                        vec3<f32>(0.17, 0.21, 0.30),
                        vec3<f32>(0.82, 0.79, 0.74),
                        light,
                    );
                    sample_color += SUN_COLOR * (light * 0.18 + silver);
                    let alpha = 1.0 - exp(-density * step_size * 0.0035);
                    scattered += sample_color * (alpha * transmittance);
                    transmittance *= 1.0 - alpha;
                    if (transmittance < 0.015) {
                        break;
                    }
                }
            }

            color = color * transmittance + scattered;
        }
    }

    // The gradient spans hundreds of 8-bit steps across the screen, which
    // shows as horizontal bands on a flat sky. One LSB of noise turns the
    // bands into imperceptible grain.
    let dither = fract(sin(dot(input.clip.xy, vec2<f32>(12.9898, 78.233))) * 43758.5453)
        - 0.5;
    color += vec3<f32>(dither * 1.5 / 255.0);

    return vec4<f32>(color, 1.0);
}
