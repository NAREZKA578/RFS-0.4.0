//! Resource Manager
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//!
//! Central resource management for meshes, textures, materials, and shaders

use crate::render::materials::{Material, MaterialLibrary};
use crate::render::meshes::{Mesh, MeshLibrary};
use crate::render::textures::{
    SamplerDesc, Texture, TextureFilterMode, TextureLibrary, TextureWrapMode,
};
use crate::rhi::{
    AddressMode, BorderColor, CompareOp, Device, FilterMode, Pipeline, Sampler, ShaderModule,
    ShaderModuleDesc,
};
use glam::{Vec2, Vec4};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

/// Resource manager
pub struct ResourceManager {
    /// Device reference
    device: Arc<Device>,

    /// Mesh library
    mesh_library: MeshLibrary,
    /// Texture library
    texture_library: TextureLibrary,
    /// Material library
    material_library: MaterialLibrary,

    /// Shader cache
    shaders: HashMap<String, Arc<ShaderModule>>,
    /// Pipeline cache
    pipelines: HashMap<String, Arc<Pipeline>>,
    /// Sampler cache
    samplers: HashMap<String, Arc<Sampler>>,

    /// Resource loading statistics
    stats: ResourceStats,
}

/// Resource statistics
#[derive(Debug, Clone, Default)]
pub struct ResourceStats {
    pub meshes_loaded: usize,
    pub textures_loaded: usize,
    pub materials_loaded: usize,
    pub shaders_loaded: usize,
    pub pipelines_created: usize,
    pub memory_used: u64,
}

impl ResourceManager {
    /// Creates a new resource manager
    pub fn new(device: Arc<Device>) -> Self {
        Self {
            device: device.clone(),
            mesh_library: MeshLibrary::new(),
            texture_library: TextureLibrary::new(),
            material_library: MaterialLibrary::new(),
            shaders: HashMap::new(),
            pipelines: HashMap::new(),
            samplers: HashMap::new(),
            stats: ResourceStats::default(),
        }
    }

    /// Loads a mesh from file
    pub fn load_mesh(&mut self, path: &str) -> Option<Arc<Mesh>> {
        if let Some(mesh) = self.mesh_library.get(path) {
            return Some(mesh);
        }

        // Load mesh from file
        let mesh = self.mesh_library.load(Path::new(path))?;
        self.stats.meshes_loaded += 1;

        Some(mesh)
    }

    /// Creates a primitive mesh
    pub fn create_primitive_mesh(&mut self, name: &str, mesh_type: PrimitiveMeshType) -> Arc<Mesh> {
        if let Some(mesh) = self.mesh_library.get(name) {
            return mesh;
        }

        let mesh = match mesh_type {
            PrimitiveMeshType::Cube => Mesh::cube(name, 1.0),
            PrimitiveMeshType::Sphere => Mesh::sphere(name, 0.5, 32, 32),
            PrimitiveMeshType::Plane => Mesh::plane(name, 1.0, 1.0),
            PrimitiveMeshType::Cylinder => Mesh::cylinder(name, 0.5, 1.0, 32),
        };

        let arc_mesh = self.mesh_library.add(name, mesh);
        self.stats.meshes_loaded += 1;

        arc_mesh
    }

    /// Loads a texture from file
    pub fn load_texture(&mut self, path: &str) -> Option<Arc<Texture>> {
        if let Some(texture) = self.texture_library.get(path) {
            return Some(texture);
        }

        // Load texture from file
        let texture = self.texture_library.load(Path::new(path))?;
        self.stats.textures_loaded += 1;

        Some(texture)
    }

    /// Creates a solid color texture
    pub fn create_color_texture(&mut self, name: &str, color: Vec4, size: Vec2) -> Arc<Texture> {
        let width = size.x.max(1.0) as u32;
        let height = size.y.max(1.0) as u32;
        let data = vec![color; (width * height) as usize];
        let data_bytes: Vec<u8> = data
            .iter()
            .flat_map(|c| {
                [
                    (c.x * 255.0) as u8,
                    (c.y * 255.0) as u8,
                    (c.z * 255.0) as u8,
                    (c.w * 255.0) as u8,
                ]
            })
            .collect();

        let texture = Texture::new_2d(
            name,
            crate::render::textures::TextureFormat::RGBA8,
            width,
            height,
            crate::render::textures::TextureUsage::Sampled,
            &self.device,
        );
        // The counter must not claim a texture arrived when the upload failed:
        // it is used as evidence that assets are present on the GPU.
        if texture.upload_data(&data_bytes, &self.device) {
            self.stats.textures_loaded += 1;
        }
        Arc::new(texture)
    }

    /// Creates a procedural texture
    pub fn create_procedural_texture(
        &mut self,
        name: &str,
        generator: ProceduralTextureGenerator,
    ) -> Arc<Texture> {
        let width = 256u32;
        let height = 256u32;
        let data: Vec<Vec4> = match generator {
            ProceduralTextureGenerator::White => vec![Vec4::ONE; (width * height) as usize],
            ProceduralTextureGenerator::Black => vec![Vec4::ZERO; (width * height) as usize],
            ProceduralTextureGenerator::Gray(v) => {
                vec![Vec4::new(v, v, v, 1.0); (width * height) as usize]
            }
            ProceduralTextureGenerator::Checkerboard { size, color_a, color_b } => {
                let mut data = Vec::with_capacity((width * height) as usize);
                for y in 0..height {
                    for x in 0..width {
                        let u = x as f32 / width as f32 * size.x;
                        let v = y as f32 / height as f32 * size.y;
                        let checker = (u.floor() as i32 + v.floor() as i32) % 2 == 0;
                        data.push(if checker { color_a } else { color_b });
                    }
                }
                data
            }
            ProceduralTextureGenerator::Gradient { direction, colors } => {
                let mut data = Vec::with_capacity((width * height) as usize);
                for y in 0..height {
                    for x in 0..width {
                        let t = if direction.x.abs() > direction.y.abs() {
                            x as f32 / width as f32
                        } else {
                            y as f32 / height as f32
                        };
                        let idx = (t * (colors.len() - 1) as f32) as usize;
                        data.push(colors[idx.min(colors.len() - 1)]);
                    }
                }
                data
            }
            ProceduralTextureGenerator::Noise { scale, octaves } => {
                let mut data = Vec::with_capacity((width * height) as usize);
                for y in 0..height {
                    for x in 0..width {
                        let mut value = 0.0f32;
                        let mut amplitude = 1.0f32;
                        let mut frequency = scale;
                        for _ in 0..octaves {
                            value += amplitude * ((x as f32 * frequency).sin()
                                * (y as f32 * frequency).cos());
                            amplitude *= 0.5;
                            frequency *= 2.0;
                        }
                        value = (value + 1.0) * 0.5;
                        data.push(Vec4::new(value, value, value, 1.0));
                    }
                }
                data
            }
            ProceduralTextureGenerator::NormalMap { strength } => {
                let mut data = Vec::with_capacity((width * height) as usize);
                for y in 0..height {
                    for x in 0..width {
                        let nx = (x as f32 / width as f32 * strength).sin();
                        let ny = (y as f32 / height as f32 * strength).cos();
                        let nz = (1.0 - nx * nx - ny * ny).sqrt().max(0.0);
                        data.push(Vec4::new(nx, ny, nz, 1.0));
                    }
                }
                data
            }
        };

        let data_bytes: Vec<u8> = data
            .iter()
            .flat_map(|c| {
                [
                    (c.x * 255.0) as u8,
                    (c.y * 255.0) as u8,
                    (c.z * 255.0) as u8,
                    (c.w * 255.0) as u8,
                ]
            })
            .collect();

        let texture = Texture::new_2d(
            name,
            crate::render::textures::TextureFormat::RGBA8,
            width,
            height,
            crate::render::textures::TextureUsage::Sampled,
            &self.device,
        );
        if texture.upload_data(&data_bytes, &self.device) {
            self.stats.textures_loaded += 1;
        }
        Arc::new(texture)
    }

    /// Loads a material
    pub fn load_material(&mut self, path: &str) -> Option<Material> {
        self.material_library.get(path).cloned()
    }

    /// Creates a PBR material
    pub fn create_pbr_material(
        &mut self,
        name: &str,
        config: PbrMaterialConfig,
    ) -> Arc<crate::render::materials::PbrMaterial> {
        let mut material = crate::render::materials::PbrMaterial::new(name);
        material.set_albedo([config.albedo.x, config.albedo.y, config.albedo.z, config.albedo.w]);
        material.set_roughness(config.roughness);
        material.set_metallic(config.metallic);
        material.set_ao(config.ao);
        material.set_emissive([config.emissive.x, config.emissive.y, config.emissive.z, config.emissive.w]);
        material.set_normal_intensity(config.normal_scale);
        material.set_packed_material(config.two_sided);
        self.material_library.add_pbr(material.clone());
        self.stats.materials_loaded += 1;
        Arc::new(material)
    }

    /// Loads a shader
    pub fn load_shader(
        &mut self,
        name: &str,
        path: &str,
        stage: ShaderStage,
    ) -> Option<Arc<ShaderModule>> {
        let key = format!("{}:{}", name, stage as u32);

        if let Some(shader) = self.shaders.get(&key) {
            return Some(shader.clone());
        }

        // Load shader from file
        let code = std::fs::read(path).ok()?;
        let shader = ShaderModule::new(ShaderModuleDesc {
            code,
            name: Some(name.to_string()),
            ..Default::default()
        });
        let arc_shader = Arc::new(shader);
        self.shaders.insert(key, arc_shader.clone());
        self.stats.shaders_loaded += 1;

        Some(arc_shader)
    }

    /// Creates a pipeline
    pub fn create_pipeline(&mut self, name: &str, desc: PipelineDesc) -> Option<Arc<Pipeline>> {
        if let Some(pipeline) = self.pipelines.get(name) {
            return Some(pipeline.clone());
        }

        use crate::rhi::{
            BlendFactor, BlendOp, ColorBlendAttachment, ColorBlendState, CompareOp,
            CullMode as RhiCullMode, DepthStencilState, FrontFace, InputAssemblyState,
            PipelineShaderStage, PolygonMode, RasterizerState,
            ShaderStage as RhiShaderStage, VertexInputState,
        };

        let vertex_shader = self.load_shader(&desc.vertex_shader, &desc.vertex_shader, ShaderStage::Vertex)?;
        let fragment_shader = desc.fragment_shader.as_ref().and_then(|s| self.load_shader(s, s, ShaderStage::Fragment));

        let mut stages = vec![PipelineShaderStage {
            stage: RhiShaderStage::VERTEX,
            module: (*vertex_shader).clone(),
            entry_point: "main".to_string(),
        }];
        if let Some(fs) = fragment_shader {
            stages.push(PipelineShaderStage {
                stage: RhiShaderStage::FRAGMENT,
                module: (*fs).clone(),
                entry_point: "main".to_string(),
            });
        }

        let vertex_input_state = VertexInputState {
            bindings: vec![crate::rhi::VertexBinding {
                binding: 0,
                stride: std::mem::size_of::<crate::render::meshes::Vertex>() as u32,
                input_rate: crate::rhi::VertexInputRate::Vertex,
            }],
            attributes: desc.vertex_layout.attributes.iter().enumerate().map(|(i, a)| {
                crate::rhi::VertexAttribute {
                    location: i as u32,
                    binding: 0,
                    format: crate::rhi::Format::from(a.format),
                    offset: a.offset,
                }
            }).collect(),
        };

        let pipeline_desc = crate::rhi::GraphicsPipelineDesc {
            shader_stages: stages,
            vertex_input_state: Some(vertex_input_state),
            input_assembly_state: InputAssemblyState {
                topology: crate::rhi::pipeline::PrimitiveTopology::TriangleList,
                primitive_restart_enable: false,
            },
            rasterizer_state: Some(RasterizerState {
                polygon_mode: PolygonMode::Fill,
                cull_mode: RhiCullMode::Back,
                front_face: FrontFace::CounterClockwise,
                depth_bias_enable: false,
                depth_bias_constant: 0.0,
                depth_bias_clamp: 0.0,
                depth_bias_slope: 0.0,
                line_width: 1.0,
                ..Default::default()
            }),
            depth_stencil_state: Some(DepthStencilState {
                depth_test_enable: true,
                depth_write_enable: true,
                depth_compare_op: CompareOp::Less,
                stencil_test_enable: false,
                front: crate::rhi::StencilOpState::default(),
                back: crate::rhi::StencilOpState::default(),
                ..Default::default()
            }),
            color_blend_state: Some(ColorBlendState {
                logic_op_enable: false,
                logic_op: crate::rhi::LogicOp::Copy,
                attachments: vec![ColorBlendAttachment {
                    blend_enable: false,
                    src_color_blend_factor: BlendFactor::One,
                    dst_color_blend_factor: BlendFactor::Zero,
                    color_blend_op: BlendOp::Add,
                    src_alpha_blend_factor: BlendFactor::One,
                    dst_alpha_blend_factor: BlendFactor::Zero,
                    alpha_blend_op: BlendOp::Add,
                    color_write_mask: crate::rhi::ColorComponentFlags::all(),
                }],
                blend_constants: [0.0, 0.0, 0.0, 1.0],
            }),
            viewport_state: crate::rhi::ViewportState::default(),
            multisample_state: crate::rhi::MultisampleState::default(),
        };

        let pipeline = crate::rhi::GraphicsPipeline::new(pipeline_desc);
        let arc_pipeline = Arc::new(pipeline);
        self.pipelines.insert(name.to_string(), arc_pipeline.clone());
        self.stats.pipelines_created += 1;
        Some(arc_pipeline)
    }

    /// Creates a sampler
    pub fn create_sampler(&mut self, name: &str, desc: SamplerDesc) -> Arc<Sampler> {
        if let Some(sampler) = self.samplers.get(name) {
            return sampler.clone();
        }

        let rhi_desc = crate::rhi::SamplerDesc {
            mag_filter: map_filter_mode(desc.filter_mode),
            min_filter: map_filter_mode(desc.filter_mode),
            mipmap_mode: FilterMode::Linear,
            address_mode_u: map_wrap_mode(desc.wrap_mode),
            address_mode_v: map_wrap_mode(desc.wrap_mode),
            address_mode_w: map_wrap_mode(desc.wrap_mode),
            mip_lod_bias: desc.lod_bias,
            max_anisotropy: desc.anisotropy,
            compare_enable: false,
            compare_op: CompareOp::Always,
            min_lod: desc.min_lod,
            max_lod: desc.max_lod,
            border_color: BorderColor::FloatOpaqueBlack,
            unnormalized_coordinates: false,
        };

        let sampler = Sampler::new(rhi_desc);
        let arc_sampler = Arc::new(sampler);
        self.samplers.insert(name.to_string(), arc_sampler.clone());

        arc_sampler
    }

    /// Gets the mesh library
    pub fn mesh_library(&self) -> &MeshLibrary {
        &self.mesh_library
    }

    /// Gets the texture library
    pub fn texture_library(&self) -> &TextureLibrary {
        &self.texture_library
    }

    /// Gets the material library
    pub fn material_library(&self) -> &MaterialLibrary {
        &self.material_library
    }

    /// Gets resource statistics
    pub fn stats(&self) -> &ResourceStats {
        &self.stats
    }

    /// Clears all resources
    pub fn clear(&mut self) {
        self.mesh_library.clear();
        self.texture_library.clear();
        self.material_library.cleanup();
        self.shaders.clear();
        self.pipelines.clear();
        self.samplers.clear();
        self.stats = ResourceStats::default();
    }

    /// Clears unused resources
    pub fn clear_unused(&mut self) {
        // Implement reference counting to clear unused resources
        // This would require tracking which resources are currently in use
    }

    /// Preloads resources for a level
    pub fn preload_level(&mut self, _level_name: &str) {
        // Load all resources needed for a specific level
        // This would be game-specific
    }

    /// Unloads resources for a level
    pub fn unload_level(&mut self, _level_name: &str) {
        // Unload resources that were loaded for a specific level
    }
}

/// Primitive mesh type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrimitiveMeshType {
    Cube,
    Sphere,
    Plane,
    Cylinder,
}

/// Shader stage
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShaderStage {
    Vertex,
    Fragment,
    Geometry,
    Compute,
    RayGen,
    AnyHit,
    ClosestHit,
    Miss,
}

/// Pipeline description
#[derive(Debug, Clone)]
pub struct PipelineDesc {
    pub name: String,
    pub vertex_shader: String,
    pub fragment_shader: Option<String>,
    pub geometry_shader: Option<String>,
    pub compute_shader: Option<String>,
    pub vertex_layout: VertexLayout,
    pub render_pass: String,
    pub pipeline_type: PipelineType,
}

/// Vertex layout
#[derive(Debug, Clone)]
pub struct VertexLayout {
    pub attributes: Vec<VertexAttribute>,
}

/// Vertex attribute
#[derive(Debug, Clone)]
pub struct VertexAttribute {
    pub name: String,
    pub format: Format,
    pub offset: u32,
}

/// Pipeline type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipelineType {
    Graphics,
    Compute,
    RayTracing,
}

/// PBR material configuration
#[derive(Debug, Clone)]
pub struct PbrMaterialConfig {
    pub albedo: Vec4,
    pub metallic: f32,
    pub roughness: f32,
    pub ao: f32,
    pub normal_scale: f32,
    pub emissive: Vec4,
    pub emissive_intensity: f32,
    pub alpha_cutoff: f32,
    pub two_sided: bool,
    pub albedo_texture: Option<String>,
    pub normal_texture: Option<String>,
    pub metallic_roughness_texture: Option<String>,
    pub ao_texture: Option<String>,
    pub emissive_texture: Option<String>,
}

/// Procedural texture generator
pub enum ProceduralTextureGenerator {
    White,
    Black,
    Gray(f32),
    Checkerboard {
        size: Vec2,
        color_a: Vec4,
        color_b: Vec4,
    },
    Gradient {
        direction: Vec2,
        colors: Vec<Vec4>,
    },
    Noise {
        scale: f32,
        octaves: u32,
    },
    NormalMap {
        strength: f32,
    },
}

/// Format type (placeholder - should use RHI Format)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    R32G32B32A32Float,
    R32G32B32Float,
    R32G32Float,
    R32Float,
    R8G8B8A8Unorm,
    R8G8B8A8Srgb,
}

impl From<Format> for crate::rhi::Format {
    fn from(f: Format) -> Self {
        match f {
            Format::R32G32B32A32Float => crate::rhi::Format::RGBA32_SFLOAT,
            Format::R32G32B32Float => crate::rhi::Format::R32G32B32_SFLOAT,
            Format::R32G32Float => crate::rhi::Format::R32G32_SFLOAT,
            Format::R32Float => crate::rhi::Format::R32_SFLOAT,
            Format::R8G8B8A8Unorm => crate::rhi::Format::RGBA8_UNORM,
            Format::R8G8B8A8Srgb => crate::rhi::Format::B8G8R8A8_SRGB,
        }
    }
}



/// Map a render filter mode to an RHI filter mode
fn map_filter_mode(mode: TextureFilterMode) -> FilterMode {
    match mode {
        TextureFilterMode::Nearest => FilterMode::Nearest,
        _ => FilterMode::Linear,
    }
}

/// Map a render wrap mode to an RHI address mode
fn map_wrap_mode(mode: TextureWrapMode) -> AddressMode {
    match mode {
        TextureWrapMode::Repeat => AddressMode::Repeat,
        TextureWrapMode::MirroredRepeat => AddressMode::MirroredRepeat,
        TextureWrapMode::ClampToEdge => AddressMode::ClampToEdge,
        TextureWrapMode::ClampToBorder => AddressMode::ClampToBorder,
    }
}
