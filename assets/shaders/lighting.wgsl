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
    output.tex_coord = input.position.xy * 0.5 + 0.5;
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
@group(0) @binding(5) var depth_texture: texture_depth_2d;
@group(0) @binding(6) var<storage, read> lights: array<Light>;

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let position = textureLoad(position_texture, vec2<i32>(input.tex_coord * vec2<f32>(textureDimensions(position_texture))), 0).xyz;
    let normal = normalize(textureLoad(normal_texture, vec2<i32>(input.tex_coord * vec2<f32>(textureDimensions(normal_texture))), 0).xyz);
    let albedo = textureLoad(albedo_texture, vec2<i32>(input.tex_coord * vec2<f32>(textureDimensions(albedo_texture))), 0).rgb;
    let material = textureLoad(material_texture, vec2<i32>(input.tex_coord * vec2<f32>(textureDimensions(material_texture))), 0);

    var color = albedo * uniforms.ambient;

    for (var i = 0u; i < uniforms.light_count; i++) {
        let light = lights[i];
        let light_dir = normalize(light.position - position);
        let distance = length(light.position - position);
        let attenuation = max(0.0, 1.0 - distance / light.radius);
        let diffuse = max(dot(normal, light_dir), 0.0);
        color += albedo * light.color * light.intensity * diffuse * attenuation;
    }

    return vec4<f32>(color, 1.0);
}
