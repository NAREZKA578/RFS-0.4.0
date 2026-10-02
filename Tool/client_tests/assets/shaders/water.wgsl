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
    @location(2) tex_coord: vec2<f32>,
};

struct WaterUniforms {
    view_proj: mat4x4<f32>,
    model: mat4x4<f32>,
    time: f32,
    wave_scale: f32,
    wave_speed: f32,
    wave_height: f32,
};

@group(0) @binding(0) var<uniform> uniforms: WaterUniforms;
@group(0) @binding(1) var reflection_texture: texture_2d<f32>;
@group(0) @binding(2) var refraction_texture: texture_2d<f32>;
@group(0) @binding(3) var normal_map: texture_2d<f32>;

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;
    output.clip_position = uniforms.view_proj * uniforms.model * vec4<f32>(input.position, 1.0);
    output.world_position = (uniforms.model * vec4<f32>(input.position, 1.0)).xyz;
    output.world_normal = normalize((uniforms.model * vec4<f32>(input.normal, 0.0)).xyz);
    output.tex_coord = input.tex_coord;
    return output;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let normal_sample = textureLoad(normal_map, vec2<i32>(input.tex_coord * vec2<f32>(textureDimensions(normal_map))), 0).xyz;
    let perturbed_normal = normalize(input.world_normal + normal_sample * 0.3);

    let reflection_color = textureLoad(reflection_texture, vec2<i32>(input.tex_coord * vec2<f32>(textureDimensions(reflection_texture))), 0).rgb;
    let refraction_color = textureLoad(refraction_texture, vec2<i32>(input.tex_coord * vec2<f32>(textureDimensions(refraction_texture))), 0).rgb;

    let fresnel = pow(1.0 - max(dot(perturbed_normal, vec3<f32>(0.0, 1.0, 0.0)), 0.0), 3.0);
    let color = mix(refraction_color, reflection_color, fresnel);

    return vec4<f32>(color, 0.85);
}
