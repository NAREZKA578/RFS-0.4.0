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
    /// How many times each node has executed, parallel to `nodes`. Bug №313.
    ///
    /// On the graph rather than on the node on purpose. Adding a field to
    /// `RenderPassNode` breaks every struct-literal construction of it across the
    /// repository — I tried that first and broke `examples/cube.rs` on the first
    /// build, which is the exact trap bug №300 describes for `GeometryPassState`.
    /// And the counter belongs to the frame anyway: "did this pass run in this
    /// graph" is a question about the graph, not about the node.
    ///
    /// Not a global, because cargo runs tests in parallel threads in one process and
    /// a process-global registry would attribute one test's passes to another.
    execution_counts: Vec<u64>,
    /// What the last recorded frame drew. See [`FrameDraws`].
    last_frame_draws: FrameDraws,
    /// Dirty flag (needs re-sorting)
    dirty: bool,
}

/// What the last recorded frame drew, counted while the commands were recorded.
///
/// Draw commands carry their vertex and index counts only while they are still
/// commands: once `submit_recorded` has taken the list the backend owns it and
/// there is nothing left to ask. So the graph counts at the one point where the
/// list is inspectable, and the renderer picks the total up with
/// [`take_last_frame_draws`](RenderGraph::take_last_frame_draws) — "picks up"
/// rather than "reads", because the number describes a single frame and leaving
/// it in place would let the next frame count the same draws twice.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FrameDraws {
    /// `Draw` and `DrawIndexed` commands recorded.
    pub draws: u32,
    /// Indices ÷ 3, summed over instances.
    pub triangles: u32,
    /// Vertices or indices touched, summed over instances.
    pub vertices: u32,
}

impl RenderGraph {
    /// Create a new empty render graph
    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
            node_indices: HashMap::new(),
            resources: HashMap::new(),
            execution_order: Vec::new(),
            execution_counts: Vec::new(),
            last_frame_draws: FrameDraws::default(),
            dirty: false,
        }
    }

    /// Add a render pass node to the graph
    pub fn add_pass(&mut self, node: RenderPassNode) {
        let index = self.nodes.len();
        self.nodes.push(node);
        self.node_indices
            .insert(self.nodes[index].name.clone(), index);
        self.execution_counts.push(0);
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
        device: &Arc<Device>,
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

        // Bug №311: the passes are created here, but their `initialize` was never
        // called — not here, not anywhere. `RenderPassNode::initialize` had zero
        // call sites in the whole repository, which meant every pass's
        // `initialize` body was dead code: the textures, pipelines and framebuffers
        // it creates did not exist, so any `execute` that checked its framebuffer
        // returned early.
        //
        // It stayed invisible for the same reason the rest of this class does:
        // nothing fails when code does not run. The passes compiled, the tests were
        // green, and the five unwired ones had `execute` bodies that were empty
        // anyway.
        //
        // The device parameter was named `_device` and unused, which is the same
        // fact stated more quietly.
        // Bug: the resources above were only ever *declared*. `GraphResource::create`
        // had no call sites anywhere in the repository, so every entry stayed a
        // descriptor with `texture: None` — and the passes that look resources up
        // at execute time (`transparent` asks for "depth", `water` for "depth" and
        // "lighting", `lighting` binds every view it can find) found nothing, so
        // the graph's resource map never fed a single pass.
        //
        // Created here rather than lazily on first use, because initialize is the
        // one place that has the device and runs before any pass records. The
        // guard makes a second `initialize` a no-op instead of a second
        // allocation.
        for resource in self.resources.values_mut() {
            if resource.texture.is_none() && resource.buffer.is_none() {
                resource.create(device);
            }
        }

        if self.dirty {
            self.sort();
        }
        for index in 0..self.nodes.len() {
            let node = &mut self.nodes[index];
            node.initialize(device, context);
        }
    }

    /// Tell every pass the target changed size. Bug №311.
    ///
    /// The counterpart of the fix above. `RenderPassNode::resize` also had no call
    /// sites, so a pass that allocated size-dependent resources had no way to learn
    /// about a resize and would keep rendering at the old dimensions until it was
    /// rebuilt by hand.
    pub fn resize(&mut self, width: u32, height: u32) {
        for node in &mut self.nodes {
            node.resize(width, height);
        }
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
        device: &Arc<Device>,
        context: &mut RenderContext,
        scene: &mut Scene,
    ) {
        if self.dirty {
            self.sort();
        }

        let pool = crate::rhi::command::pool::CommandPool::new(
            crate::rhi::command::pool::CommandPoolDesc::default(),
        );
        let mut buffer = crate::rhi::command::buffer::CommandBuffer::new(
            pool,
            crate::rhi::command::buffer::CommandBufferDesc::default(),
        );

        // Bug №209: this created the encoder, ran the passes and then dropped
        // it on the floor — no `begin`, no `finish`, no submit. Everything the
        // passes recorded was discarded, so the graph could never produce a
        // pixel no matter how correct the passes were. It is now a real
        // record-and-submit.
if let Err(e) = buffer.begin() {
              crate::error!("graph", "command buffer begin failed: {e}");
              device.cancel_presentation_frame();
              return;
          }
        let mut encoder = crate::rhi::command::encoder::CommandEncoder::new(buffer);

        // Execute passes in order
        for &node_index in &self.execution_order {
            let node = &mut self.nodes[node_index];

            // Check if pass is enabled
            if !node.enabled {
                continue;
            }

            // Execute the pass
            // Bug №313: counted here, at the one point every pass in a frame goes
            // through, so "did this subsystem run" becomes a number instead of an
            // assumption. See `pass_execution_counts`.
            self.execution_counts[node_index] += 1;
            node.execute(&mut encoder, context, scene, &self.resources);
        }

        let recorded = encoder.command_count();
        let finished = match encoder.finish() {
            Ok(buffer) => buffer,
Err(e) => {
                  crate::error!("graph", "command buffer finish failed: {e}");
                  device.cancel_presentation_frame();
                  return;
              }
        };

        // An empty list is not submitted: `submit_commands` refuses it, and
        // reporting "nothing was recorded" is more useful than a backend error
        // about an empty recording. This used to be the normal case, while every
        // pass was still a stub (#186), and the emptiness is what told us the
        // graph was inert. Passes record now, so an empty frame means something
        // broke on the way to the recorder, and the warning below says so.
if recorded == 0 {
              crate::warn!("graph", "recorded no commands; nothing submitted");
              // Bug №282: the renderer already acquired the image and armed a
              // presentation frame, so an acquire semaphore is signalled with
              // nothing left to wait on it. The next acquire of that slot then
              // fails with "semaphore must not have any pending operations".
              device.cancel_presentation_frame();
              return;
          }

        // Counted before submission, while the commands are still ours to inspect:
        // `submit_recorded` hands the list to the backend, and a draw's vertex and
        // index counts exist only while the command is a command. See `FrameDraws`.
        self.last_frame_draws = Self::count_draws(finished.commands());

        if let Err(e) = device.submit_recorded(finished.commands()) {
            crate::error!("graph", "command submission failed: {e}");
        }
    }

    /// Record the graph's passes and hand back the commands, without
    /// submitting them.
    ///
    /// Exists so the recording can be asserted on directly. `execute` submits,
    /// which needs a real swapchain and produces no observable value, so the
    /// command list was previously untestable — and an untestable list is how a
    /// graph that records nothing passes every test.
    pub fn record(
        &mut self,
        context: &mut RenderContext,
        scene: &mut Scene,
    ) -> Vec<crate::rhi::command::commands::Command> {
        if self.dirty {
            self.sort();
        }

        let pool = crate::rhi::command::pool::CommandPool::new(
            crate::rhi::command::pool::CommandPoolDesc::default(),
        );
        let mut buffer = crate::rhi::command::buffer::CommandBuffer::new(
            pool,
            crate::rhi::command::buffer::CommandBufferDesc::default(),
        );
        if let Err(e) = buffer.begin() {
            crate::error!("graph", "command buffer begin failed: {e}");
            return Vec::new();
        }
        let mut encoder = crate::rhi::command::encoder::CommandEncoder::new(buffer);

        for &node_index in &self.execution_order {
            let node = &mut self.nodes[node_index];
            if !node.enabled {
                continue;
            }
            // Counted on the recording path too, not only on the submitting one.
            // Tests drive passes through `record`, and a counter that only moved on
            // `execute` would report every inspected pass as never run — which is
            // worse than having no counter, because it looks like a finding.
            self.execution_counts[node_index] += 1;
            node.execute(&mut encoder, context, scene, &self.resources);
        }

        // `finish` moves the commands into the buffer; cloning them out is what
        // makes the list inspectable without a GPU round trip.
        match encoder.finish() {
            Ok(buffer) => {
                // Same reason as in `execute`: counted while the list is still ours.
                self.last_frame_draws = Self::count_draws(buffer.commands());
                buffer.commands().to_vec()
            }
            Err(e) => {
                crate::error!("graph", "command buffer finish failed: {e}");
                Vec::new()
            }
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

    /// Count the draws in a recorded command list.
    ///
    /// Only `Draw` and `DrawIndexed` are counted: a dispatch is not a draw, and
    /// counting it would make "draw calls" mean something else on machines that
    /// happen to compute more than they draw.
    fn count_draws(commands: &[crate::rhi::command::commands::Command]) -> FrameDraws {
        use crate::rhi::command::commands::Command;
        let mut out = FrameDraws::default();
        for command in commands {
            match command {
                Command::Draw {
                    vertex_count,
                    instance_count,
                    ..
                } => {
                    out.draws += 1;
                    out.vertices += vertex_count.saturating_mul(*instance_count);
                }
                Command::DrawIndexed {
                    index_count,
                    instance_count,
                    ..
                } => {
                    out.draws += 1;
                    let touched = index_count.saturating_mul(*instance_count);
                    out.vertices += touched;
                    out.triangles += touched / 3;
                }
                _ => {}
            }
        }
        out
    }

    /// What the last recorded frame drew, taken so it cannot be counted twice.
    pub fn take_last_frame_draws(&mut self) -> FrameDraws {
        std::mem::take(&mut self.last_frame_draws)
    }

    /// How many times each pass has actually executed, in execution order.
    ///
    /// Bug №313. The question this answers is "did this subsystem run", and nothing
    /// else in the build can answer it. A pass can be written, compiled, linted,
    /// documented and exported, and never execute once — which is precisely the
    /// state five of the seven passes were in: their pipelines were never created,
    /// their resources never allocated, and every build stayed green because there
    /// were no commands to object to.
    ///
    /// Counts are of real executions, so a node that is disabled or never reached
    /// reads `0`. Reported in execution order rather than insertion order, because
    /// the interesting comparison is against the order the frame is supposed to
    /// draw in, and insertion order would hide that two passes claiming to be
    /// sequential are not.
    pub fn pass_execution_counts(&self) -> Vec<(String, u64)> {
        self.execution_order
            .iter()
            .map(|&i| {
                let node = &self.nodes[i];
                (node.name.clone(), self.execution_counts.get(i).copied().unwrap_or(0))
            })
            .collect()
    }

    /// Names of passes that have never executed.
    ///
    /// The same defect class as [`pass_execution_counts`](Self::pass_execution_counts),
    /// narrowed to the part that is actionable: a pass on this list is on this list
    /// because it was added to the graph and then did not run — either disabled, or
    /// unreachable through the dependency sort, or its `execute` returned early. All
    /// three look identical from outside, which is why the name and count are
    /// reported rather than a diagnosis.
    pub fn never_executed(&self) -> Vec<String> {
        self.pass_execution_counts()
            .into_iter()
            .filter(|(_, count)| *count == 0)
            .map(|(name, _)| name)
            .collect()
    }

    /// A one-line summary of what actually ran, for a diagnostic overlay or a log.
    pub fn execution_summary(&self) -> String {
        if self.nodes.is_empty() {
            return "<no passes>".to_string();
        }
        let parts: Vec<String> = self
            .pass_execution_counts()
            .into_iter()
            .map(|(name, count)| format!("{name}x{count}"))
            .collect();
        format!("{} ({} never ran)", parts.join(" "), self.never_executed().len())
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
