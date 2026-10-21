struct VertexInput {
    @location(0) position: vec3<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) tex_coord: vec2<f32>,
};

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;
    output.clip_position = vec4<f32>(input.position.xy, 0.0, 1.0);
    // The GBuffer's row 0 is the *top* of the image: Vulkan's framebuffer
    // origin is top-left, so clip-space +Y is the top and the texel row
    // decreases as Y grows. Sampling with the Y unflipped reads every row from
    // the wrong end of the image, which is a picture that is not upside down
    // but is not the right picture either.
    output.tex_coord = vec2<f32>(input.position.x * 0.5 + 0.5, 0.5 - input.position.y * 0.5);
    return output;
}

struct Light {
    position: vec3<f32>,
    color: vec3<f32>,
    intensity: f32,
    radius: f32,
};

struct LightingUniforms {
    view_proj: mat4x4<f32>,
    camera_position: vec3<f32>,
    light_count: u32,
    ambient: vec3<f32>,
};

@group(0) @binding(0) var<uniform> uniforms: LightingUniforms;
@group(0) @binding(1) var position_texture: texture_2d<f32>;
@group(0) @binding(2) var normal_texture: texture_2d<f32>;
@group(0) @binding(3) var albedo_texture: texture_2d<f32>;
@group(0) @binding(4) var material_texture: texture_2d<f32>;
// The lights are a read-only storage buffer, bound at 5.
//
// The depth buffer used to be bound here and was never read by the fragment
// shader. It is gone because a depth texture cannot be described by the RHI's
// descriptor types: WGSL's `texture_depth_2d` is a `SAMPLED_IMAGE` in
// SPIR-V, and for a depth format the descriptor layout has to be
// `NON_FILTERING_FLOAT`, which the RHI cannot express. A binding nothing
// reads is not worth a new RHI feature.
@group(0) @binding(5) var<storage, read> lights: array<Light>;

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let dims = vec2<f32>(textureDimensions(position_texture));
    let texel = vec2<i32>(input.tex_coord * dims);
    let position = textureLoad(position_texture, texel, 0).xyz;
    let normal = normalize(textureLoad(normal_texture, texel, 0).xyz);
    let albedo = textureLoad(albedo_texture, texel, 0).rgb;
    let material = textureLoad(material_texture, texel, 0);

    var color = albedo * uniforms.ambient;

    for (var i = 0u; i < uniforms.light_count; i = i + 1u) {
        let light = lights[i];
        let light_dir = normalize(light.position - position);
        let distance = length(light.position - position);
        let attenuation = max(0.0, 1.0 - distance / light.radius);
        let diffuse = max(dot(normal, light_dir), 0.0);
        color += albedo * light.color * light.intensity * diffuse * attenuation;
    }

    return vec4<f32>(color, 1.0);
}
