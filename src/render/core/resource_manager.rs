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
use crate::rhi::command::pass::render::RenderPass;
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
    /// Cache of render passes, parallel to `pipelines`.
    ///
    /// Present because of bug №314: a graphics pipeline cannot be built without
    /// the pass it is compatible with, so a manager that caches pipelines but
    /// not passes has nothing to hand to the device. Keyed by name, which is why
    /// `PipelineDesc` carries `render_pass` as a name — it was ignored before,
    /// and an ignored field is a description of a dependency that does not exist.
    render_passes: HashMap<String, Arc<RenderPass>>,
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
            render_passes: HashMap::new(),
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

    /// Creates a solid color texture.
    ///
    /// Returns the backend's refusal (bug №642) — the texture either exists on
    /// the device or the caller gets an error; there is no handle-less stand-in.
    pub fn create_color_texture(
        &mut self,
        name: &str,
        color: Vec4,
        size: Vec2,
    ) -> crate::rhi::RhiResult<Arc<Texture>> {
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
        )
        .map_err(|e| crate::rhi::RhiError::BackendError(format!("the '{name}' texture: {e}")))?;
        // The counter must not claim a texture arrived when the upload failed:
        // it is used as evidence that assets are present on the GPU.
        // Registered in the library rather than wrapped in a bare `Arc`: an
        // unregistered texture is unreachable from `clear()`, so it outlives the
        // manager and its GPU memory is never released.
        if texture.upload_data(&data_bytes, &self.device) {
            self.stats.textures_loaded += 1;
        }
        Ok(self.texture_library.add(name, texture))
    }

    /// Creates a procedural texture (bug №642: the backend's refusal is
    /// returned instead of dropped).
    pub fn create_procedural_texture(
        &mut self,
        name: &str,
        generator: ProceduralTextureGenerator,
    ) -> crate::rhi::RhiResult<Arc<Texture>> {
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
        )
        .map_err(|e| crate::rhi::RhiError::BackendError(format!("the '{name}' texture: {e}")))?;
        // Same reason as `create_color_texture`: the library owns the texture, so
        // `clear()` can actually reach it.
        if texture.upload_data(&data_bytes, &self.device) {
            self.stats.textures_loaded += 1;
        }
        Ok(self.texture_library.add(name, texture))
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

        // Load shader from file.
        //
        // Two things were wrong here. The module was built with
        // `ShaderModule::new`, which leaves `backend: None`, so every cached
        // "shader" was a descriptor with no native handle. And `..Default::default()`
        // left the format unset, so the bytes were handed to the backend without
        // being declared as SPIR-V — the file was read correctly and then
        // interpreted as whatever the default happened to be.
        let code = match std::fs::read(path) {
            Ok(code) => code,
            Err(e) => {
                crate::error!(
                    "resource_manager",
                    "shader {}: cannot read {}: {}",
                    name,
                    path,
                    e
                );
                return None;
            }
        };

        // SPIR-V magic. Cheap, and it turns a wrong path or a stale source file
        // into a named error instead of a backend complaint much later.
        const SPIRV_MAGIC: [u8; 4] = [0x03, 0x02, 0x23, 0x07];
        if code.len() < 4 || code[..4] != SPIRV_MAGIC {
            crate::error!(
                "resource_manager",
                "shader {}: {} is not a SPIR-V module (bad magic); shaders must be \
                 compiled to .spv before loading",
                name,
                path
            );
            return None;
        }

        let shader = match self.device.create_shader_module(&ShaderModuleDesc {
            code,
            format: crate::rhi::ShaderFormat::SpirV,
            entry_point: None,
            name: Some(name.to_string()),
        }) {
            Ok(shader) => shader,
            Err(e) => {
                // Bug №642: the backend's refusal is the caller's answer —
                // there is no handle-less stand-in to guard against with
                // has_gpu_backing() any more.
                crate::error!(
                    "resource_manager",
                    "shader {}: the device refused to create the module: {e}",
                    name
                );
                return None;
            }
        };
        let arc_shader = Arc::new(shader);
        self.shaders.insert(key, arc_shader.clone());
        self.stats.shaders_loaded += 1;

        Some(arc_shader)
    }

    /// Creates a pipeline.
    ///
    /// Two defects made this function incapable of producing anything drawable,
    /// and both are worth recording because they failed *quietly*.
    ///
    /// First, the shader name was passed as the file path. `std::fs::read` on a
    /// name like `"water"` fails, the `?` returned `None`, and the caller had no
    /// way to tell that apart from a legitimately absent pipeline. `PipelineDesc`
    /// now carries a path separately from the name.
    ///
    /// Second, the result came from `GraphicsPipeline::new`, which builds a
    /// descriptor and leaves `backend: None`. Such a pipeline looks complete,
    /// prints correctly, and has `has_gpu_backing() == false`, so nothing could
    /// ever draw with it. Only the device attaches a native handle, so only the
    /// device is used here.
    ///
    /// A requested-but-unsupported pipeline is now reported rather than built as
    /// graphics. Silently substituting a graphics pipeline for a compute request
    /// produces a green build and a wrong frame, which is the failure mode this
    /// whole subsystem was written to avoid.
    pub fn create_pipeline(&mut self, name: &str, desc: PipelineDesc) -> Option<Arc<Pipeline>> {
        if let Some(pipeline) = self.pipelines.get(name) {
            return Some(pipeline.clone());
        }

        if !pipeline_request_is_supported(name, &desc) {
            return None;
        }

        use crate::rhi::{
            BlendFactor, BlendOp, ColorBlendAttachment, ColorBlendState, CompareOp,
            CullMode as RhiCullMode, DepthStencilState, FrontFace, InputAssemblyState,
            PipelineShaderStage, PolygonMode, RasterizerState,
            ShaderStage as RhiShaderStage, VertexInputState,
        };

        let vertex_shader = self.load_shader(
            &desc.vertex_shader,
            &desc.vertex_shader_path,
            ShaderStage::Vertex,
        )?;
        let fragment_shader = desc.fragment_shader.as_ref().and_then(|s| {
            let path = desc.fragment_shader_path.as_deref().unwrap_or(s);
            self.load_shader(s, path, ShaderStage::Fragment)
        });

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

        let render_pass = self.resolve_render_pass(&desc)?;

        // The device is the only path that attaches a native pipeline handle.
        // `GraphicsPipeline::new` would leave `backend: None` and the pipeline
        // would be unusable while still looking complete.
        let pipeline = match self.device.create_graphics_pipeline(&pipeline_desc, &render_pass, &[]) {
            Ok(pipeline) => pipeline,
            Err(e) => {
                crate::error!(
                    "resource_manager",
                    "pipeline {}: device refused to create it: {}",
                    name,
                    e
                );
                return None;
            }
        };
        // A pipeline with no native handle cannot be drawn with. Returning it would be
        // worse than returning `None`: it has a complete descriptor, it prints,
        // and `is_some()` says yes. Callers would bind it and draw nothing. The
        // cache is left untouched so a later attempt can succeed.
        if !pipeline.has_gpu_backing() {
            crate::error!(
                "resource_manager",
                "pipeline {}: created without GPU backing; it cannot be drawn with",
                name
            );
            return None;
        }
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

    /// The render pass a pipeline needs, creating and caching it on first use.
    ///
    /// A pipeline is incompatible with a pass it was not built against: the
    /// attachment formats are baked into the pipeline at creation, and a
    /// mismatch is an error from the driver rather than a warning anyone reads.
    ///
    /// Returns `None` when no description was supplied, rather than inventing a
    /// pass. An invented pass would be compatible with nothing and would fail
    /// later, further from the cause.
    pub fn resolve_render_pass(&mut self, desc: &PipelineDesc) -> Option<Arc<RenderPass>> {
        if let Some(pass) = self.render_passes.get(&desc.render_pass) {
            return Some(pass.clone());
        }

        let pass_desc = desc.render_pass_desc.as_ref()?;
        let pass = match self.device.create_render_pass(pass_desc) {
            Ok(pass) => pass,
            Err(e) => {
                crate::error!(
                    "resource_manager",
                    "render pass {}: device refused to create it: {}",
                    desc.render_pass,
                    e
                );
                return None;
            }
        };
        if !pass.has_gpu_backing() {
            crate::error!(
                "resource_manager",
                "render pass {}: created without GPU backing; every pipeline built \
                 against it would be unusable",
                desc.render_pass
            );
            return None;
        }
        let arc = Arc::new(pass);
        self.render_passes.insert(desc.render_pass.clone(), arc.clone());
        Some(arc)
    }
}

/// Whether a pipeline request can be honoured as written.
///
/// Pure, and deliberately separate from `create_pipeline`, because a check
/// buried inside a function that also opens files and calls the driver is
/// untestable: the function returns `None` for a dozen unrelated reasons, so an
/// assertion on `None` proves nothing about which guard fired. Separated, this
/// can be tested directly, and it is the only way to tell "refused on purpose"
/// apart from "failed by accident".
pub fn pipeline_request_is_supported(name: &str, desc: &PipelineDesc) -> bool {
    if desc.pipeline_type != PipelineType::Graphics {
        crate::error!(
            "resource_manager",
            "pipeline {}: only graphics pipelines are implemented, refusing to build {:?} as graphics",
            name,
            desc.pipeline_type
        );
        return false;
    }

    if desc.geometry_shader.is_some() {
        crate::error!(
            "resource_manager",
            "pipeline {}: a geometry stage is declared but never compiled into the stage list; \
             refusing to build a pipeline that would silently drop it",
            name
        );
        return false;
    }

    true
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

/// Pipeline description.
///
/// The shader fields are names for the cache key; the `*_path` fields are what
/// is actually opened. Collapsing the two is what made `create_pipeline` fail
/// silently for every pipeline it was handed.
#[derive(Debug, Clone)]
pub struct PipelineDesc {
    pub name: String,
    pub vertex_shader: String,
    /// File path to the vertex shader. Defaults to the name if left empty,
    /// which is only correct when the name is itself a path.
    pub vertex_shader_path: String,
    pub fragment_shader: Option<String>,
    /// File path to the fragment shader, when it differs from its name.
    pub fragment_shader_path: Option<String>,
    pub geometry_shader: Option<String>,
    pub compute_shader: Option<String>,
    pub vertex_layout: VertexLayout,
    /// Name the pass is cached under.
    pub render_pass: String,
    /// The pass itself, for the first request under that name.
    ///
    /// `render_pass` alone is not enough: a pass needs colour attachment
    /// formats, and those come from the target being rendered into, not from
    /// the name. Both are kept because the first request supplies the description
    /// and later ones reuse whatever was built.
    pub render_pass_desc: Option<crate::rhi::command::pass::render::RenderPassDesc>,
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
