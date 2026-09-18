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
        let _ = (name, color, size);
        unimplemented!("Solid color textures are provided by the texture subsystem")
    }

    /// Creates a procedural texture
    pub fn create_procedural_texture(
        &mut self,
        name: &str,
        generator: ProceduralTextureGenerator,
    ) -> Arc<Texture> {
        let _ = (name, generator);
        unimplemented!("Procedural textures are provided by the texture subsystem")
    }

    /// Loads a material
    pub fn load_material(&mut self, _path: &str) -> Option<Arc<Material>> {
        unimplemented!("Material file loading is provided by the material subsystem")
    }

    /// Creates a PBR material
    pub fn create_pbr_material(
        &mut self,
        _name: &str,
        _config: PbrMaterialConfig,
    ) -> Arc<crate::render::materials::PbrMaterial> {
        unimplemented!("PBR material creation is provided by the material subsystem")
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

        // Pipeline creation is delegated to the RHI pipeline system
        let _ = (desc, &self.device);
        None
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

impl Default for ResourceManager {
    fn default() -> Self {
        unimplemented!("ResourceManager requires a real RHI device")
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
