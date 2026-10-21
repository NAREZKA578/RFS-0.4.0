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

/// What a pass did when it ran.
///
/// Bug №653: the graph's counter was incremented *before* `execute` was
/// called, so a pass that returned early — no framebuffer, no pipeline, no
/// state — was counted as having run. The doc comment claimed the counts were
/// "of real executions", and nothing in the code kept that claim true.
///
/// The difference is not academic. Five of the seven passes were in exactly
/// that state: their pipelines were never created, so their `execute` returned
/// before recording anything, and the counter said they ran every frame. An
/// instrument that cannot tell "was called" from "did the work" is blind to
/// the defect class it was built for.
///
/// So the pass says what it did. The reason matters as much as the fact: a
/// pass that skipped because it has no pipeline says so, and the log says
/// which.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PassOutcome {
    /// Whether the pass recorded anything into the command stream.
    pub recorded: bool,
    /// Why the pass recorded nothing, when it did not.
    pub reason: Option<String>,
}

impl PassOutcome {
    /// The pass recorded work.
    pub fn recorded() -> Self {
        Self {
            recorded: true,
            reason: None,
        }
    }

    /// The pass did nothing, for this reason.
    pub fn skipped(reason: impl Into<String>) -> Self {
        Self {
            recorded: false,
            reason: Some(reason.into()),
        }
    }

    /// A one-line description, for a log.
    pub fn describe(&self) -> String {
        match &self.reason {
            Some(reason) => format!("skipped: {reason}"),
            None => "recorded".to_string(),
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

    /// Initialize the pass.
    ///
    /// The resource map is handed over so a pass can take the targets it was
    /// promised and produce the ones it owns. It was not there before, and
    /// that is why the graph's resources never fed a pass: the passes could
    /// not reach them, so they made their own — and the graph's map stayed a
    /// catalogue nobody read. Bug №652.
    fn initialize(
        &mut self,
        _device: &crate::rhi::Device,
        _context: &RenderContext,
        _resources: &mut HashMap<String, GraphResource>,
    ) {
        // Default: do nothing
    }

    /// Execute the pass.
    ///
    /// Returns what the pass did, so the graph can tell "was called" from
    /// "contributed to the frame". See [`PassOutcome`].
    fn execute(
        &mut self,
        encoder: &mut CommandEncoder,
        context: &mut RenderContext,
        scene: &mut Scene,
        resources: &HashMap<String, GraphResource>,
    ) -> PassOutcome;

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
    ) -> PassOutcome {
        if self.enabled {
            self.pass.execute(encoder, context, scene, resources)
        } else {
            // A disabled pass is not a pass that did nothing — it is a pass
            // that was not asked to. Saying so keeps the two apart: the graph
            // counts this separately from a pass that was called and skipped.
            PassOutcome::skipped("disabled")
        }
    }

    /// Initialize the pass
    pub fn initialize(
        &mut self,
        device: &crate::rhi::Device,
        context: &RenderContext,
        resources: &mut HashMap<String, GraphResource>,
    ) {
        self.pass.initialize(device, context, resources);
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
