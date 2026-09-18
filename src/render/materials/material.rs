//! Base Material
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use crate::render::textures::Texture;
use crate::rhi::{Buffer, DescriptorSet, DescriptorSetLayout, Device, Pipeline, ShaderModule};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

/// Material type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MaterialType {
    /// Standard PBR material
    Pbr,
    /// Water material
    Water,
    /// Unlit material (no lighting)
    Unlit,
    /// Custom shader material
    Custom,
}

impl Default for MaterialType {
    fn default() -> Self {
        Self::Pbr
    }
}

/// Blend mode for transparent materials
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BlendMode {
    /// Opaque (no blending)
    Opaque,
    /// Standard alpha blending
    Alpha,
    /// Additive blending
    Additive,
    /// Multiplicative blending
    Multiplicative,
    /// Screen blending
    Screen,
}

impl Default for BlendMode {
    fn default() -> Self {
        Self::Opaque
    }
}

/// Cull mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CullMode {
    /// No culling
    None,
    /// Cull front faces
    Front,
    /// Cull back faces
    Back,
}

impl Default for CullMode {
    fn default() -> Self {
        Self::Back
    }
}

/// Material parameters that can be updated at runtime
#[derive(Debug, Clone)]
pub struct MaterialParameters {
    /// Custom float parameters
    pub floats: HashMap<String, f32>,
    /// Custom vector parameters
    pub vectors: HashMap<String, [f32; 4]>,
    /// Custom color parameters
    pub colors: HashMap<String, [f32; 4]>,
    /// Custom texture parameters
    pub textures: HashMap<String, Arc<Texture>>,
}

impl Default for MaterialParameters {
    fn default() -> Self {
        Self {
            floats: HashMap::new(),
            vectors: HashMap::new(),
            colors: HashMap::new(),
            textures: HashMap::new(),
        }
    }
}

/// Base material struct
#[derive(Debug, Clone)]
pub struct Material {
    /// Material name
    pub name: String,
    /// Material type
    pub material_type: MaterialType,
    /// Shader module
    pub shader: Option<Arc<ShaderModule>>,
    /// Pipeline
    pub pipeline: Option<Pipeline>,
    /// Descriptor set layout
    pub descriptor_set_layout: Option<Arc<DescriptorSetLayout>>,
    /// Descriptor set
    pub descriptor_set: Option<DescriptorSet>,
    /// Uniform buffer
    pub uniform_buffer: Option<Buffer>,
    /// Parameters
    pub parameters: MaterialParameters,
    /// Blend mode
    pub blend_mode: BlendMode,
    /// Cull mode
    pub cull_mode: CullMode,
    /// Depth test enabled
    pub depth_test: bool,
    /// Depth write enabled
    pub depth_write: bool,
    /// Two-sided rendering
    pub two_sided: bool,
    /// Wireframe mode
    pub wireframe: bool,
}

impl Material {
    /// Create a new material
    pub fn new(name: &str, material_type: MaterialType) -> Self {
        Self {
            name: name.to_string(),
            material_type,
            shader: None,
            pipeline: None,
            descriptor_set_layout: None,
            descriptor_set: None,
            uniform_buffer: None,
            parameters: MaterialParameters::default(),
            blend_mode: BlendMode::default(),
            cull_mode: CullMode::default(),
            depth_test: true,
            depth_write: true,
            two_sided: false,
            wireframe: false,
        }
    }

    /// Set shader
    pub fn set_shader(&mut self, shader: Arc<ShaderModule>) {
        self.shader = Some(shader);
    }

    /// Create pipeline for this material
    pub fn create_pipeline(&mut self, device: &Device) {
        if let Some(shader) = &self.shader {
            // Create pipeline
            // This is a placeholder - actual implementation would create
            // a pipeline with the appropriate shader stages, vertex input, etc.

            let pipeline_desc = crate::rhi::PipelineDesc {
                shader_stages: vec![crate::rhi::PipelineShaderStage {
                    stage: crate::rhi::ShaderStage::VERTEX,
                    module: (**shader).clone(),
                    entry_point: "main".to_string(),
                }],
                vertex_input_state: Some(crate::rhi::VertexInputDesc::default()),
                rasterizer_state: Some(crate::rhi::RasterizerDesc {
                    polygon_mode: if self.wireframe {
                        crate::rhi::PolygonMode::Line
                    } else {
                        crate::rhi::PolygonMode::Fill
                    },
                    cull_mode: match self.cull_mode {
                        CullMode::None => crate::rhi::CullMode::None,
                        CullMode::Front => crate::rhi::CullMode::Front,
                        CullMode::Back => crate::rhi::CullMode::Back,
                    },
                    front_face: crate::rhi::FrontFace::CounterClockwise,
                    depth_clamp_enable: false,
                    ..Default::default()
                }),
                depth_stencil_state: Some(crate::rhi::DepthStencilDesc {
                    depth_test_enable: self.depth_test,
                    depth_write_enable: self.depth_write,
                    depth_compare_op: crate::rhi::CompareOp::Less,
                    ..Default::default()
                }),
                color_blend_state: Some(match self.blend_mode {
                    BlendMode::Opaque => crate::rhi::BlendDesc::disabled(),
                    BlendMode::Alpha => crate::rhi::BlendDesc::alpha(),
                    BlendMode::Additive => crate::rhi::BlendDesc::additive(),
                    BlendMode::Multiplicative => crate::rhi::BlendDesc::multiplicative(),
                    BlendMode::Screen => crate::rhi::BlendDesc::screen(),
                }),
                ..Default::default()
            };

            self.pipeline = Some(device.create_graphics_pipeline(&pipeline_desc).unwrap());
        }
    }

    /// Update descriptor set with material parameters
    pub fn update_descriptor_set(&mut self, _device: &Device) {
        if let Some(_descriptor_set) = &mut self.descriptor_set {
            // Update descriptor set with current parameters
            // This would bind textures, buffers, etc.

            // For now, this is a placeholder
        }
    }

    /// Set a float parameter
    pub fn set_float(&mut self, name: &str, value: f32) {
        self.parameters.floats.insert(name.to_string(), value);
    }

    /// Set a vector parameter
    pub fn set_vector(&mut self, name: &str, value: [f32; 4]) {
        self.parameters.vectors.insert(name.to_string(), value);
    }

    /// Set a color parameter
    pub fn set_color(&mut self, name: &str, value: [f32; 4]) {
        self.parameters.colors.insert(name.to_string(), value);
    }

    /// Set a texture parameter
    pub fn set_texture(&mut self, name: &str, texture: Arc<Texture>) {
        self.parameters.textures.insert(name.to_string(), texture);
    }

    /// Get a float parameter
    pub fn get_float(&self, name: &str) -> Option<f32> {
        self.parameters.floats.get(name).copied()
    }

    /// Get a vector parameter
    pub fn get_vector(&self, name: &str) -> Option<[f32; 4]> {
        self.parameters.vectors.get(name).copied()
    }

    /// Get a color parameter
    pub fn get_color(&self, name: &str) -> Option<[f32; 4]> {
        self.parameters.colors.get(name).copied()
    }

    /// Get a texture parameter
    pub fn get_texture(&self, name: &str) -> Option<Arc<Texture>> {
        self.parameters.textures.get(name).cloned()
    }

    /// Bind the material for rendering
    pub fn bind(&self, encoder: &mut crate::rhi::CommandEncoder) {
        if let Some(pipeline) = &self.pipeline {
            encoder.bind_pipeline(pipeline);
        }

        if let Some(descriptor_set) = &self.descriptor_set {
            encoder.bind_descriptor_set(descriptor_set, 0);
        }
    }

    /// Clean up resources
    pub fn cleanup(&mut self) {
        self.pipeline = None;
        self.descriptor_set = None;
        self.uniform_buffer = None;
    }
}

impl Default for Material {
    fn default() -> Self {
        Self::new("default", MaterialType::Pbr)
    }
}

/// Material builder for easier material creation
pub struct MaterialBuilder {
    material: Material,
}

impl MaterialBuilder {
    pub fn new(name: &str, material_type: MaterialType) -> Self {
        Self {
            material: Material::new(name, material_type),
        }
    }

    pub fn shader(mut self, shader: Arc<ShaderModule>) -> Self {
        self.material.set_shader(shader);
        self
    }

    pub fn blend_mode(mut self, blend_mode: BlendMode) -> Self {
        self.material.blend_mode = blend_mode;
        self
    }

    pub fn cull_mode(mut self, cull_mode: CullMode) -> Self {
        self.material.cull_mode = cull_mode;
        self
    }

    pub fn depth_test(mut self, depth_test: bool) -> Self {
        self.material.depth_test = depth_test;
        self
    }

    pub fn depth_write(mut self, depth_write: bool) -> Self {
        self.material.depth_write = depth_write;
        self
    }

    pub fn two_sided(mut self, two_sided: bool) -> Self {
        self.material.two_sided = two_sided;
        self
    }

    pub fn wireframe(mut self, wireframe: bool) -> Self {
        self.material.wireframe = wireframe;
        self
    }

    pub fn float(mut self, name: &str, value: f32) -> Self {
        self.material.set_float(name, value);
        self
    }

    pub fn vector(mut self, name: &str, value: [f32; 4]) -> Self {
        self.material.set_vector(name, value);
        self
    }

    pub fn color(mut self, name: &str, value: [f32; 4]) -> Self {
        self.material.set_color(name, value);
        self
    }

    pub fn texture(mut self, name: &str, texture: Arc<Texture>) -> Self {
        self.material.set_texture(name, texture);
        self
    }

    pub fn build(self) -> Material {
        self.material
    }
}
