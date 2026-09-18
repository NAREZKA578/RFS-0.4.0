//! Render Pass Node
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use super::resource::GraphResource;
use super::types::ResourceUsage;
use crate::render::core::RenderContext;
use crate::render::scene::Scene;
use crate::rhi::CommandEncoder;
use std::collections::HashMap;

/// Dependency between render passes
#[derive(Debug, Clone)]
pub struct PassDependency {
    /// Name of the pass this depends on
    pub pass_name: String,
    /// Resource this pass needs from the dependency
    pub resource_name: String,
    /// Usage of the resource
    pub usage: ResourceUsage,
}

impl PassDependency {
    /// Create a new dependency
    pub fn new(pass_name: &str, resource_name: &str, usage: ResourceUsage) -> Self {
        Self {
            pass_name: pass_name.to_string(),
            resource_name: resource_name.to_string(),
            usage,
        }
    }
}

/// Trait for render passes
pub trait RenderPass {
    /// Get the name of the pass
    fn name(&self) -> &str;

    /// Get dependencies of this pass
    fn dependencies(&self) -> &[PassDependency];

    /// Get outputs of this pass
    fn outputs(&self) -> &[String];

    /// Initialize the pass
    fn initialize(&mut self, _device: &crate::rhi::Device, _context: &RenderContext) {
        // Default: do nothing
    }

    /// Execute the pass
    fn execute(
        &mut self,
        encoder: &mut CommandEncoder,
        context: &mut RenderContext,
        scene: &mut Scene,
        resources: &HashMap<String, GraphResource>,
    );

    /// Resize the pass
    fn resize(&mut self, _width: u32, _height: u32) {
        // Default: do nothing
    }

    /// Is the pass enabled?
    fn is_enabled(&self) -> bool {
        true
    }

    /// Set enabled state
    fn set_enabled(&mut self, _enabled: bool) {
        // Default: do nothing
    }
}

/// Render pass node that can be added to the graph
pub struct RenderPassNode {
    /// Name of the pass
    pub name: String,
    /// The actual pass implementation
    pub pass: Box<dyn RenderPass>,
    /// Dependencies of this pass
    pub dependencies: Vec<PassDependency>,
    /// Output resources
    pub outputs: Vec<String>,
    /// Is this pass enabled?
    pub enabled: bool,
    /// Execution order (set by graph sort)
    pub execution_order: usize,
}

impl RenderPassNode {
    /// Create a new render pass node
    pub fn new(name: &str, pass: Box<dyn RenderPass>) -> Self {
        let dependencies = pass.dependencies().to_vec();
        let outputs = pass.outputs().to_vec();
        let enabled = pass.is_enabled();
        Self {
            name: name.to_string(),
            pass,
            dependencies,
            outputs,
            enabled,
            execution_order: 0,
        }
    }

    /// Execute the pass
    pub fn execute(
        &mut self,
        encoder: &mut CommandEncoder,
        context: &mut RenderContext,
        scene: &mut Scene,
        resources: &HashMap<String, GraphResource>,
    ) {
        if self.enabled {
            self.pass.execute(encoder, context, scene, resources);
        }
    }

    /// Initialize the pass
    pub fn initialize(&mut self, device: &crate::rhi::Device, context: &RenderContext) {
        self.pass.initialize(device, context);
    }

    /// Resize the pass
    pub fn resize(&mut self, width: u32, height: u32) {
        self.pass.resize(width, height);
    }

    /// Set enabled state
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        self.pass.set_enabled(enabled);
    }
}

/// Helper macro to implement RenderPass for a struct
#[macro_export]
macro_rules! impl_render_pass {
    ($type:ty, $name:expr, $deps:expr, $outputs:expr) => {
        impl RenderPass for $type {
            fn name(&self) -> &str {
                $name
            }

            fn dependencies(&self) -> &[PassDependency] {
                &$deps
            }

            fn outputs(&self) -> &[String] {
                &$outputs
            }
        }
    };
}
