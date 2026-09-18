//! Render Graph
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//!
//! The RenderGraph manages the execution order of render passes
//! and their dependencies on resources (textures, buffers).

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use super::{GraphResource, RenderPassNode, ResourceType, ResourceUsage};
use crate::render::core::RenderContext;
use crate::render::scene::Scene;
use crate::rhi::Device;

/// Render Graph that manages render passes and their dependencies
pub struct RenderGraph {
    /// All render pass nodes
    nodes: Vec<RenderPassNode>,
    /// Node index map for quick lookup
    node_indices: HashMap<String, usize>,
    /// Resource map
    resources: HashMap<String, GraphResource>,
    /// Execution order (topologically sorted)
    execution_order: Vec<usize>,
    /// Dirty flag (needs re-sorting)
    dirty: bool,
}

impl RenderGraph {
    /// Create a new empty render graph
    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
            node_indices: HashMap::new(),
            resources: HashMap::new(),
            execution_order: Vec::new(),
            dirty: false,
        }
    }

    /// Add a render pass node to the graph
    pub fn add_pass(&mut self, node: RenderPassNode) {
        let index = self.nodes.len();
        self.nodes.push(node);
        self.node_indices
            .insert(self.nodes[index].name.clone(), index);
        self.dirty = true;
    }

    /// Add a resource to the graph
    pub fn add_resource(&mut self, name: &str, resource: GraphResource) {
        self.resources.insert(name.to_string(), resource);
    }

    /// Get a resource by name
    pub fn get_resource(&self, name: &str) -> Option<&GraphResource> {
        self.resources.get(name)
    }

    /// Get a mutable resource by name
    pub fn get_resource_mut(&mut self, name: &str) -> Option<&mut GraphResource> {
        self.resources.get_mut(name)
    }

    /// Initialize the render graph with default passes
    pub fn initialize(
        &mut self,
        _device: &Arc<Device>,
        context: &RenderContext,
        _scene: &mut Scene,
    ) {
        // Add default resources
        self.add_resource(
            "swapchain",
            GraphResource::new(
                "swapchain".to_string(),
                ResourceType::Texture,
                ResourceUsage::RenderTarget,
                context.resolution.width,
                context.resolution.height,
            ),
        );

        self.add_resource(
            "depth",
            GraphResource::new(
                "depth".to_string(),
                ResourceType::Texture,
                ResourceUsage::DepthStencil,
                context.resolution.width,
                context.resolution.height,
            ),
        );

        // Add GBuffer resources
        self.add_resource(
            "gbuffer_position",
            GraphResource::new(
                "gbuffer_position".to_string(),
                ResourceType::Texture,
                ResourceUsage::ColorAttachment | ResourceUsage::Sampled,
                context.resolution.width,
                context.resolution.height,
            ),
        );

        self.add_resource(
            "gbuffer_normal",
            GraphResource::new(
                "gbuffer_normal".to_string(),
                ResourceType::Texture,
                ResourceUsage::ColorAttachment | ResourceUsage::Sampled,
                context.resolution.width,
                context.resolution.height,
            ),
        );

        self.add_resource(
            "gbuffer_albedo",
            GraphResource::new(
                "gbuffer_albedo".to_string(),
                ResourceType::Texture,
                ResourceUsage::ColorAttachment | ResourceUsage::Sampled,
                context.resolution.width,
                context.resolution.height,
            ),
        );

        self.add_resource(
            "gbuffer_material",
            GraphResource::new(
                "gbuffer_material".to_string(),
                ResourceType::Texture,
                ResourceUsage::ColorAttachment | ResourceUsage::Sampled,
                context.resolution.width,
                context.resolution.height,
            ),
        );

        // Add shadow map resource
        self.add_resource(
            "shadow_map",
            GraphResource::new(
                "shadow_map".to_string(),
                ResourceType::Texture,
                ResourceUsage::DepthStencil | ResourceUsage::Sampled,
                2048,
                2048,
            ),
        );

        // Add lighting resource
        self.add_resource(
            "lighting",
            GraphResource::new(
                "lighting".to_string(),
                ResourceType::Texture,
                ResourceUsage::ColorAttachment | ResourceUsage::Sampled,
                context.resolution.width,
                context.resolution.height,
            ),
        );

        // Add post-process resources
        self.add_resource(
            "postprocess_bloom",
            GraphResource::new(
                "postprocess_bloom".to_string(),
                ResourceType::Texture,
                ResourceUsage::ColorAttachment | ResourceUsage::Sampled,
                context.resolution.width,
                context.resolution.height,
            ),
        );

        self.add_resource(
            "final",
            GraphResource::new(
                "final".to_string(),
                ResourceType::Texture,
                ResourceUsage::RenderTarget,
                context.resolution.width,
                context.resolution.height,
            ),
        );

        // Create and add default passes
        // Note: Actual pass creation will be done in the initialize method
        // after all systems are set up
    }

    /// Sort the graph topologically
    pub fn sort(&mut self) {
        if !self.dirty {
            return;
        }

        // Build adjacency list
        let mut adj: Vec<Vec<usize>> = vec![Vec::new(); self.nodes.len()];
        let mut in_degree: Vec<usize> = vec![0; self.nodes.len()];

        for (i, node) in self.nodes.iter().enumerate() {
            for dep in &node.dependencies {
                if let Some(&j) = self.node_indices.get(&dep.pass_name) {
                    adj[j].push(i);
                    in_degree[i] += 1;
                }
            }
        }

        // Topological sort using Kahn's algorithm
        let mut queue = VecDeque::new();
        for (i, &degree) in in_degree.iter().enumerate() {
            if degree == 0 {
                queue.push_back(i);
            }
        }

        self.execution_order.clear();
        while let Some(i) = queue.pop_front() {
            self.execution_order.push(i);
            for &neighbor in &adj[i] {
                in_degree[neighbor] -= 1;
                if in_degree[neighbor] == 0 {
                    queue.push_back(neighbor);
                }
            }
        }

        self.dirty = false;
    }

    /// Execute the render graph
    pub fn execute(
        &mut self,
        _device: &Arc<Device>,
        context: &mut RenderContext,
        scene: &mut Scene,
    ) {
        if self.dirty {
            self.sort();
        }

        let pool = crate::rhi::command::pool::CommandPool::new(
            crate::rhi::command::pool::CommandPoolDesc::default(),
        );
        let buffer = crate::rhi::command::buffer::CommandBuffer::new(
            pool,
            crate::rhi::command::buffer::CommandBufferDesc::default(),
        );
        let mut encoder = crate::rhi::command::encoder::CommandEncoder::new(buffer);

        // Execute passes in order
        for &node_index in &self.execution_order {
            let node = &mut self.nodes[node_index];

            // Check if pass is enabled
            if !node.enabled {
                continue;
            }

            // Execute the pass
            node.execute(&mut encoder, context, scene, &self.resources);
        }
    }

    /// Get a pass by name
    pub fn get_pass_mut(&mut self, name: &str) -> Option<&mut RenderPassNode> {
        self.node_indices.get(name).map(|&i| &mut self.nodes[i])
    }

    /// Get all passes
    pub fn passes(&self) -> &[RenderPassNode] {
        &self.nodes
    }

    /// Get execution order
    pub fn execution_order(&self) -> &[usize] {
        &self.execution_order
    }

    /// Mark graph as dirty (needs re-sorting)
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }
}

impl Default for RenderGraph {
    fn default() -> Self {
        Self::new()
    }
}
