//! Integration tests for the render::graph module.
//!
//! Covers: RenderGraph, RenderPassNode, RenderPass trait (mock), PassDependency,
//! GraphResource, ResourceType, ResourceUsage.

use rfs_client::render::graph::node::RenderPass;
use rfs_client::render::graph::{
    GraphResource, PassDependency, RenderGraph, RenderPassNode, ResourceType, ResourceUsage,
};
use rfs_client::render::{RenderContext, Scene};
use rfs_client::rhi::CommandEncoder;
use rfs_client::rhi::Format;
use std::collections::HashMap;

struct MockPass {
    name: String,
    deps: Vec<PassDependency>,
    outputs: Vec<String>,
}

impl MockPass {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            deps: Vec::new(),
            outputs: Vec::new(),
        }
    }
}

impl RenderPass for MockPass {
    fn name(&self) -> &str {
        &self.name
    }

    fn dependencies(&self) -> &[PassDependency] {
        &self.deps
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
    }
}

#[test]
fn empty_graph_has_no_passes() {
    let graph = RenderGraph::new();
    assert!(graph.passes().is_empty());
    assert!(graph.execution_order().is_empty());
}

#[test]
fn add_pass_increases_pass_count() {
    let mut graph = RenderGraph::new();
    graph.add_pass(RenderPassNode::new("pass_a", Box::new(MockPass::new("pass_a"))));
    graph.add_pass(RenderPassNode::new("pass_b", Box::new(MockPass::new("pass_b"))));
    assert_eq!(graph.passes().len(), 2);
    assert_eq!(graph.passes()[0].name, "pass_a");
    assert_eq!(graph.passes()[1].pass.name(), "pass_b");
}

#[test]
fn render_pass_node_defaults() {
    let node = RenderPassNode::new("first", Box::new(MockPass::new("first")));
    assert!(node.enabled);
    assert_eq!(node.execution_order, 0);
    assert!(node.dependencies.is_empty());
    assert!(node.outputs.is_empty());
    assert!(node.pass.is_enabled());
}

#[test]
fn get_pass_mut_allows_changes() {
    let mut graph = RenderGraph::new();
    graph.add_pass(RenderPassNode::new("target", Box::new(MockPass::new("target"))));
    let node = graph
        .get_pass_mut("target")
        .expect("pass 'target' exists");
    node.enabled = false;
    node.outputs.push("color".to_string());
    let node = graph.get_pass_mut("target").unwrap();
    assert!(!node.enabled);
    assert_eq!(node.outputs, vec!["color".to_string()]);
    assert!(graph.get_pass_mut("missing").is_none());
}

#[test]
fn add_texture_resource_and_retrieve() {
    let mut graph = RenderGraph::new();
    let resource = GraphResource::new_texture(
        "color".to_string(),
        1024,
        768,
        Format::RGBA8_UNORM,
        ResourceUsage::RenderTarget,
    );
    graph.add_resource("color", resource);

    let got = graph.get_resource("color").expect("resource exists");
    assert_eq!(got.name, "color");
    assert_eq!(got.resource_type, ResourceType::Texture);
    assert_eq!(got.usage, ResourceUsage::RenderTarget);
    assert_eq!(got.width, 1024);
    assert_eq!(got.height, 768);
    assert_eq!(got.depth, 1);
    assert!(got.is_texture());
    assert!(!got.is_buffer());

    assert!(graph.get_resource("missing").is_none());
}

#[test]
fn get_resource_mut_modifies_resource() {
    let mut graph = RenderGraph::new();
    graph.add_resource(
        "depth",
        GraphResource::new_texture(
            "depth".to_string(),
            800,
            600,
            Format::D32_SFLOAT,
            ResourceUsage::DepthStencil,
        ),
    );
    let resource = graph.get_resource_mut("depth").unwrap();
    resource.width = 1280;
    resource.height = 720;
    resource.transient = true;

    let resource = graph.get_resource("depth").unwrap();
    assert_eq!(resource.width, 1280);
    assert_eq!(resource.height, 720);
    assert!(resource.transient);
    assert_eq!(resource.usage, ResourceUsage::DepthStencil);
}

#[test]
fn graph_buffer_resource() {
    let buffer = GraphResource::new_buffer("ubo".to_string(), 64, ResourceUsage::Storage);
    assert_eq!(buffer.resource_type, ResourceType::Buffer);
    assert_eq!(buffer.size, 64);
    assert!(buffer.is_buffer());
    assert!(!buffer.is_texture());
    assert_eq!(buffer.format, Format::RGBA8_UNORM);
}

#[test]
fn graph_resource_set_transient_and_lifetime() {
    let mut resource = GraphResource::new_texture(
        "tmp".to_string(),
        64,
        64,
        Format::RGBA8_UNORM,
        ResourceUsage::Sampled,
    );
    assert!(!resource.should_destroy());
    resource.set_transient(true);
    resource.increment_lifetime();
    assert!(resource.transient);
    assert_eq!(resource.lifetime, 1);
    assert!(!resource.should_destroy(), "lifetime must exceed 1 to destroy");
    resource.increment_lifetime();
    assert_eq!(resource.lifetime, 2);
    assert!(resource.should_destroy());
}

#[test]
fn pass_dependency_creation() {
    let dependency = PassDependency::new("src_pass", "src_texture", ResourceUsage::Sampled);
    assert_eq!(dependency.pass_name, "src_pass");
    assert_eq!(dependency.resource_name, "src_texture");
    assert_eq!(dependency.usage, ResourceUsage::Sampled);
}

#[test]
fn resource_usage_bitor_dominance() {
    assert_eq!(ResourceUsage::RenderTarget | ResourceUsage::ColorAttachment, ResourceUsage::RenderTarget);
    assert_eq!(
        ResourceUsage::ColorAttachment | ResourceUsage::RenderTarget,
        ResourceUsage::RenderTarget
    );
    assert_eq!(
        ResourceUsage::ColorAttachment | ResourceUsage::Sampled,
        ResourceUsage::ColorAttachment
    );
    assert_eq!(
        ResourceUsage::DepthStencil | ResourceUsage::Sampled,
        ResourceUsage::DepthStencil
    );
    assert_eq!(ResourceUsage::Sampled | ResourceUsage::Storage, ResourceUsage::Sampled);
    assert_eq!(ResourceUsage::Storage | ResourceUsage::Sampled, ResourceUsage::Storage);
    assert_eq!(ResourceUsage::ColorAttachment | ResourceUsage::DepthStencil, ResourceUsage::ColorAttachment);
    assert_eq!(ResourceUsage::DepthStencil | ResourceUsage::ColorAttachment, ResourceUsage::DepthStencil);
}

#[test]
fn topological_sort_executes_dependencies_first() {
    let mut graph = RenderGraph::new();

    let mut pass_b = MockPass::new("pass_b");
    pass_b.outputs.push("color".to_string());

    let mut pass_a = MockPass::new("pass_a");
    pass_a.deps.push(PassDependency::new(
        "pass_b",
        "color",
        ResourceUsage::ColorAttachment,
    ));

    graph.add_pass(RenderPassNode::new("pass_b", Box::new(pass_b)));
    graph.add_pass(RenderPassNode::new("pass_a", Box::new(pass_a)));

    assert!(graph.execution_order().is_empty());
    graph.sort();

    let order = graph.execution_order();
    assert_eq!(order.len(), 2);
    assert_eq!(order[0], 0, "pass 'pass_b' (index 0) must run first");
    assert_eq!(order[1], 1, "dependent pass 'pass_a' (index 1) must run second");
}

#[test]
fn mark_dirty_triggers_resort_without_panic() {
    let mut graph = RenderGraph::new();
    graph.add_pass(RenderPassNode::new("solo", Box::new(MockPass::new("solo"))));
    graph.sort();
    assert_eq!(graph.execution_order().len(), 1);

    graph.mark_dirty();
    graph.sort();
    assert_eq!(graph.execution_order().len(), 1);
    assert_eq!(graph.passes().len(), 1);
}

#[test]
fn graph_resource_lifetime_helpers() {
    let mut resource =
        GraphResource::new_buffer("dynamic".to_string(), 128, ResourceUsage::Storage);
    assert_eq!(resource.lifetime, 0);
    resource.increment_lifetime();
    assert_eq!(resource.lifetime, 1);
    resource.set_transient(false);
    assert!(!resource.should_destroy());
}