//! Base Render Pass Trait
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use crate::render::core::RenderContext;
use crate::render::graph::node::{PassDependency, RenderPass};
use crate::render::graph::resource::GraphResource;
use crate::render::scene::Scene;
use crate::rhi::CommandEncoder;
use std::collections::HashMap;

/// Base implementation for render passes
/// This can be used as a starting point for custom passes
pub struct BaseRenderPass {
    name: String,
    dependencies: Vec<PassDependency>,
    outputs: Vec<String>,
    enabled: bool,
}

impl BaseRenderPass {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            dependencies: Vec::new(),
            outputs: Vec::new(),
            enabled: true,
        }
    }

    pub fn add_dependency(
        &mut self,
        pass_name: &str,
        resource_name: &str,
        usage: crate::render::graph::types::ResourceUsage,
    ) {
        self.dependencies
            .push(PassDependency::new(pass_name, resource_name, usage));
    }

    pub fn add_output(&mut self, resource_name: &str) {
        self.outputs.push(resource_name.to_string());
    }
}

impl RenderPass for BaseRenderPass {
    fn name(&self) -> &str {
        &self.name
    }

    fn dependencies(&self) -> &[PassDependency] {
        &self.dependencies
    }

    fn outputs(&self) -> &[String] {
        &self.outputs
    }

    fn execute(
        &mut self,
        _encoder: &mut CommandEncoder,
        _context: &mut RenderContext,
        _scene: &mut Scene,
        _resources: &HashMap<String, GraphResource>,
    ) {
        // Base implementation does nothing
    }

    fn is_enabled(&self) -> bool {
        self.enabled
    }

    fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }
}

/// Helper to get a texture view from resources
pub fn get_texture_view<'a>(
    resources: &'a HashMap<String, GraphResource>,
    name: &str,
) -> Option<&'a crate::rhi::TextureView> {
    resources.get(name).and_then(|r| r.view())
}

/// Helper to get a texture from resources
pub fn get_texture<'a>(
    resources: &'a HashMap<String, GraphResource>,
    name: &str,
) -> Option<&'a crate::rhi::Texture> {
    resources.get(name).and_then(|r| r.texture())
}

/// Helper to get a buffer from resources
pub fn get_buffer<'a>(
    resources: &'a HashMap<String, GraphResource>,
    name: &str,
) -> Option<&'a crate::rhi::Buffer> {
    resources.get(name).and_then(|r| r.buffer())
}
