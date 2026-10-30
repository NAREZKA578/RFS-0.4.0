//! Integration tests for the render::graph module.
//!
//! Covers: RenderGraph, RenderPassNode, RenderPass trait (mock), PassDependency,
//! GraphResource, ResourceType, ResourceUsage.

use rfs_client::render::graph::node::RenderPass;
use rfs_client::render::graph::{
    GraphResource, PassDependency, RenderGraph, RenderPassNode, ResourceType, ResourceUsage,
};
use rfs_client::render::{RenderContext, Renderer, Scene};
use rfs_client::rhi::config::RhiConfig;
use rfs_client::rhi::types::Extent2D;
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
    ) -> rfs_client::render::graph::node::PassOutcome {
        // The mock records nothing, and says so — see `PassOutcome`.
        rfs_client::render::graph::node::PassOutcome::skipped("the mock records nothing")
    }
}

#[test]
fn empty_graph_has_no_passes() {
    let graph = RenderGraph::new();
    assert!(graph.passes().is_empty());
    assert!(graph.execution_order().is_empty());
}

/// Bug №651: a screen-sized target is sized from the presentation extent, not
/// from `context.resolution`.
///
/// `resolution` is the window's *logical* size, and until the first resize it
/// is not even that — it starts as a hard-coded 1920×1080, because
/// `Renderer::new` takes a window handle and does not use it. The physical size
/// is the swapchain's extent, and the two disagree at any scale factor other
/// than 100%: a 1280×720 window at 125% has a 1600×900 swapchain, so a target
/// sized from the resolution covers four fifths of the screen and the rest is
/// whatever was there before.
///
/// The rule now lives in one place, `RenderContext::drawable_extent`. It used
/// to be written out again in every pass that needed a size, and written out
/// wrongly in five of them.
#[test]
fn the_graph_sizes_its_targets_from_the_presentation_extent() {
    let mut renderer = Renderer::new(std::ptr::null_mut(), RhiConfig::default())
        .expect("a headless renderer needs no window");

    // The window is logical; the swapchain is physical. At 125% scaling these
    // are 1280x720 and 1600x900.
    renderer.context_mut().resolution = Extent2D::new(1280, 720);
    renderer.context_mut().presentation_extent = Extent2D::new(1600, 900);

    let mut scene = Scene::new("sizing");
    renderer.initialize(&mut scene);

    // Every screen-sized resource the graph declares.
    for name in [
        "depth",
        "postprocess_bloom",
        "final",
        "swapchain",
    ] {
        let resource = renderer
            .render_graph_mut()
            .get_resource(name)
            .unwrap_or_else(|| panic!("the graph declares a resource named {name:?}"));
        assert_eq!(
            (resource.width, resource.height),
            (1600, 900),
            "resource {name:?} was sized from the logical window size instead of \
             the physical extent"
        );
    }
}

/// The shadow map is not window-sized: it covers the part of the world the
/// light can see, and its size comes from the shadow configuration. A pass
/// that sized it from the resolution would allocate a 1600×900 depth target
/// for a 2048×2048 cascade.
#[test]
fn the_shadow_map_keeps_its_own_size() {
    let mut renderer = Renderer::new(std::ptr::null_mut(), RhiConfig::default())
        .expect("a headless renderer needs no window");
    renderer.context_mut().resolution = Extent2D::new(1280, 720);
    renderer.context_mut().presentation_extent = Extent2D::new(1600, 900);

    let mut scene = Scene::new("sizing");
    renderer.initialize(&mut scene);

    let resource = renderer
        .render_graph_mut()
        .get_resource("shadow_map")
        .expect("the graph declares a shadow map");
    assert_eq!(
        (resource.width, resource.height),
        (2048, 2048),
        "the shadow map is sized by the shadow configuration, not by the window"
    );
}

/// Bug №652: a resource the graph's passes declare must become a real GPU
/// object.
///
/// The graph declared ten resources and created all of them — as CPU-side
/// descriptions that no pass could bind. The passes that look resources up at
/// execute time (`transparent` asks for "depth", `water` for "depth" and
/// "lighting") found nothing, so the graph's resource map never fed a single
/// pass.
///
/// Now the creation follows the demand: a resource is created when a pass
/// declares a dependency on it, through the device, so it has a native handle.
#[test]
fn a_resource_a_pass_declares_becomes_a_real_gpu_object() {
    let backend = rhi::backend::create_backend(&RhiConfig::default()).expect("create backend");
    let devices = backend.enumerate_physical_devices().expect("enumerate");
    assert!(!devices.is_empty(), "no Vulkan device available");
    let device = std::sync::Arc::new(
        backend
            .create_device(
                &devices[0],
                &rhi::core::device::DeviceDesc {
                    features: rhi::types::Features::default(),
                    ..Default::default()
                },
            )
            .expect("create device"),
    );

    let mut graph = RenderGraph::new();
    // A pass that produces the depth buffer, and one that reads it.
    //
    // Both ends have to be in the frame: a resource belongs to a frame when a
    // pass in it produces it, or when a pass in it reads it *from a pass in
    // it*. A pass reading a resource whose producer is not in the frame is
    // reading from the future, and creating that resource would allocate a
    // screen-sized texture for a frame that has nothing to put in it.
    graph.add_pass(RenderPassNode {
        name: "producer".into(),
        pass: Box::new(MockPass {
            name: "producer".into(),
            deps: Vec::new(),
            outputs: vec!["depth".to_string()],
        }),
        dependencies: Vec::new(),
        outputs: vec!["depth".to_string()],
        enabled: true,
        execution_order: 0,
    });
    graph.add_pass(RenderPassNode {
        name: "needs-depth".into(),
        pass: Box::new(MockPass {
            name: "needs-depth".into(),
            deps: vec![PassDependency::new(
                "producer",
                "depth",
                ResourceUsage::DEPTH_STENCIL,
            )],
            outputs: Vec::new(),
        }),
        dependencies: vec![PassDependency::new(
            "producer",
            "depth",
            ResourceUsage::DEPTH_STENCIL,
        )],
        outputs: Vec::new(),
        enabled: true,
        execution_order: 0,
    });
    graph.mark_dirty();

    let mut context = RenderContext::headless(device.clone());
    context.resolution = Extent2D::new(1280, 720);
    context.presentation_extent = Extent2D::new(1600, 900);
    let mut scene = Scene::new("resources");

    graph.initialize(&device, &mut context, &mut scene);

    let depth = graph
        .get_resource("depth")
        .expect("the graph declares a depth resource");
    let texture = depth
        .texture()
        .expect("a resource a pass declared must have been created");
    assert!(
        texture.has_gpu_backing(),
        "the depth resource must be a real GPU object with a native handle, not a \
         description of one"
    );
    assert_eq!(
        (texture.width(), texture.height()),
        (1600, 900),
        "the resource is sized from the physical extent"
    );

    // And a resource nobody asked for is not created: ten screen-sized
    // textures nobody reads is a hundred megabytes of video memory for a
    // frame that does not use them.
    let shadow = graph
        .get_resource("shadow_map")
        .expect("the graph declares a shadow map");
    assert!(
        shadow.texture().is_none(),
        "a resource no pass asked for must not be created"
    );
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

/// Bug №102: `|` used to mean "pick the dominant usage" and to depend on the
/// order the caller wrote, which meant `ColorAttachment | Sampled` silently lost
/// the `Sampled` bit. `graph.rs` declares exactly that combination for seven
/// resources, so those textures were created without `TextureUsage::SAMPLED` and
/// anything sampling them read a texture never made for sampling.
///
/// `|` is a union now, so the test asserts union rather than dominance. The name
/// changed with it on purpose: a test called `bitor_dominance` invites a future
/// reader to reintroduce the old behaviour, because the old name described the
/// defect as if it were the specification.
#[test]
fn resource_usage_union_keeps_every_bit_in_any_order() {
    // Order must not matter: this is what the old implementation got wrong.
    assert_eq!(
        ResourceUsage::RENDER_TARGET | ResourceUsage::COLOR_ATTACHMENT,
        ResourceUsage::COLOR_ATTACHMENT | ResourceUsage::RENDER_TARGET
    );
    assert_eq!(
        ResourceUsage::COLOR_ATTACHMENT | ResourceUsage::SAMPLED,
        ResourceUsage::SAMPLED | ResourceUsage::COLOR_ATTACHMENT
    );
    assert_eq!(
        ResourceUsage::DEPTH_STENCIL | ResourceUsage::SAMPLED,
        ResourceUsage::SAMPLED | ResourceUsage::DEPTH_STENCIL
    );
    assert_eq!(
        ResourceUsage::SAMPLED | ResourceUsage::STORAGE,
        ResourceUsage::STORAGE | ResourceUsage::SAMPLED
    );

    // The specific loss that mattered: the second usage used to disappear.
    assert!(
        (ResourceUsage::COLOR_ATTACHMENT | ResourceUsage::SAMPLED).contains(ResourceUsage::SAMPLED),
        "SAMPLED must survive the union"
    );
    assert!(
        (ResourceUsage::COLOR_ATTACHMENT | ResourceUsage::SAMPLED)
            .contains(ResourceUsage::COLOR_ATTACHMENT)
    );
    assert!(
        (ResourceUsage::DEPTH_STENCIL | ResourceUsage::SAMPLED).contains(ResourceUsage::SAMPLED)
    );

    // Three usages at once, which no single `match` arm could ever describe.
    let three = ResourceUsage::RENDER_TARGET
        | ResourceUsage::DEPTH_STENCIL
        | ResourceUsage::STORAGE;
    assert_eq!(three.iter().count(), 3);

    // The alias spellings must still work, because the passes use them.
    assert_eq!(
        ResourceUsage::ColorAttachment | ResourceUsage::Sampled,
        ResourceUsage::COLOR_ATTACHMENT | ResourceUsage::SAMPLED
    );
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