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
use crate::rhi::types::Format;
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
    /// How many times each node actually contributed to a frame, parallel to
    /// `nodes`. Bug №653.
    ///
    /// `execution_counts` says the graph *reached* the pass. This says the pass
    /// *did something*. The two are different questions, and the instrument
    /// that was built to answer "was this subsystem ever really used" could
    /// only answer the first one — so five passes that dispatched every frame
    /// and recorded nothing read as "running".
    recorded_counts: Vec<u64>,
    /// Why each pass last recorded nothing, by name.
    ///
    /// The reason is the actionable part: "no pipeline" and "no framebuffer"
    /// are two different bugs, and a list of names cannot tell them apart.
    skip_reasons: HashMap<String, String>,
    /// What the last recorded frame drew. See [`FrameDraws`].
    last_frame_draws: FrameDraws,
    /// Whether the graph has been initialized.
    ///
    /// Bug: the graph could be executed before it was initialized, and then
    /// every pass did no work — no targets, no pipelines, nothing recorded —
    /// and the frame was submitted as an empty command buffer. The reason was
    /// not printed at the level a caller sees: the instrument that knows is an
    /// *info* line, and the default threshold prints warnings and not info.
    ///
    /// The flag lets the renderer initialize the graph on the first frame that
    /// needs it, so a caller who adds passes and renders does not have to
    /// remember a separate step to get anything on the screen.
    initialized: bool,
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
            recorded_counts: Vec::new(),
            skip_reasons: HashMap::new(),
            last_frame_draws: FrameDraws::default(),
            initialized: false,
            dirty: false,
        }
    }

    /// Whether the graph has been initialized.
    ///
    /// A graph that was never initialized has no resources and no initialized
    /// passes, so every pass in it does no work. The renderer asks this before
    /// the first frame, so that "added passes and rendered" is enough.
    pub fn is_initialized(&self) -> bool {
        self.initialized
    }

    /// Add a render pass node to the graph
    pub fn add_pass(&mut self, node: RenderPassNode) {
        let index = self.nodes.len();
        self.nodes.push(node);
        self.node_indices
            .insert(self.nodes[index].name.clone(), index);
        self.execution_counts.push(0);
        self.recorded_counts.push(0);
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
        // Bug №651: the size of a screen-sized target is the presentation
        // extent, not `context.resolution`.
        //
        // `resolution` is the window's *logical* size, and until the first
        // resize it is not even that — it starts as a hard-coded 1920×1080,
        // because `Renderer::new` takes a window handle and does not use it. The
        // physical size is the swapchain's extent, and the two disagree at any
        // scale factor other than 100%: a 1280×720 window at 125% has a
        // 1600×900 swapchain, so a target sized from `resolution` covers four
        // fifths of the screen.
        //
        // The rule lives in one place now — `RenderContext::drawable_extent` —
        // because it was written out again in every pass that needed a size,
        // and written out wrongly in most of them.
        let (width, height) = context.drawable_size();

        // Add default resources
        self.add_resource(
            "swapchain",
            GraphResource::new(
                "swapchain".to_string(),
                ResourceType::Texture,
                ResourceUsage::RenderTarget,
                width,
                height,
            ),
        );

        self.add_resource(
            "depth",
            GraphResource::new_texture(
                "depth".to_string(),
                width,
                height,
                Format::D32_SFLOAT,
                ResourceUsage::DepthStencil | ResourceUsage::Sampled,
            ),
        );

        // Add shadow map resource
        //
        // A shadow map is not window-sized: it covers the part of the world the
        // light can see, and its size comes from the shadow configuration. 2048
        // is the cascade resolution, and it stays as it is — the same reason
        // `shadow.rs` reads its size from the config rather than from the
        // resolution.
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

        // Add post-process resources
        self.add_resource(
            "postprocess_bloom",
            GraphResource::new(
                "postprocess_bloom".to_string(),
                ResourceType::Texture,
                ResourceUsage::ColorAttachment | ResourceUsage::Sampled,
                width,
                height,
            ),
        );

        self.add_resource(
            "final",
            GraphResource::new(
                "final".to_string(),
                ResourceType::Texture,
                ResourceUsage::RenderTarget,
                width,
                height,
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
        // The resources the graph's passes actually ask for.
        //
        // Bug №652: the graph declared ten resources and created all of them,
        // which was wrong twice over. The objects were CPU-side descriptions
        // that no pass could bind — the passes that look resources up at
        // execute time (`transparent` asks for "depth", `water` for "depth"
        // and "lighting", `lighting` binds every view it can find) found
        // nothing, and the graph's resource map never fed a single pass. And
        // ten screen-sized textures nobody reads is a hundred megabytes of
        // video memory for a frame that does not use them.
        //
        // So the catalogue stays — it is the render layer's vocabulary, and it
        // says what a frame *can* have — and the creation follows the demand:
        // a resource is created when a pass in this frame produces it, or when
        // a pass in this frame reads it from another pass in this frame.
        //
        // The second half of the rule matters: a pass that reads a resource
        // whose producer is not in the frame is reading from the future, and
        // creating that resource would allocate a screen-sized texture for a
        // frame that has nothing to put in it.
        //
        // `PassDependency::resource_name` and `outputs` are what say this, and
        // before this they were never read by anything.
        let pass_names: std::collections::HashSet<String> = self
            .nodes
            .iter()
            .map(|node| node.name.clone())
            .collect();

        let mut wanted: Vec<String> = Vec::new();
        for node in &self.nodes {
            for output in &node.outputs {
                if !output.is_empty() && !wanted.contains(output) {
                    wanted.push(output.clone());
                }
            }
            for dependency in &node.dependencies {
                if pass_names.contains(&dependency.pass_name)
                    && !dependency.resource_name.is_empty()
                    && !wanted.contains(&dependency.resource_name)
                {
                    wanted.push(dependency.resource_name.clone());
                }
            }
        }
        for name in wanted {
            // The swapchain is the presentation's. The geometry pass draws into
            // the image the renderer acquired, not into a texture the graph
            // made, and a second copy of it would be a full-screen target
            // nobody reads.
            if name == "swapchain" {
                continue;
            }
            if width == 0 || height == 0 {
                // No size, no texture. Creating a zero-sized one is refused by
                // the driver, and the frame has nothing to draw into anyway.
                continue;
            }
            if let Some(resource) = self.resources.get_mut(&name) {
                if resource.texture.is_none() && resource.buffer.is_none() {
                    resource.create(device);
                }
            }
        }

        if self.dirty {
            self.sort();
        }
        for index in 0..self.nodes.len() {
            let node = &mut self.nodes[index];
            node.initialize(device, context, &mut self.resources);
        }
        self.initialized = true;
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

        // Bug №653: Kahn's algorithm places every node it can and silently
        // drops the rest. A pass in a dependency cycle — or one that depends on
        // itself — never reaches in-degree zero, so it never enters the
        // execution order, and since the counters were read off that order, it
        // was invisible to the instrument built to find exactly this.
        //
        // So the cycle is reported rather than swallowed, and the nodes that
        // could not be placed are still executed, in the order they were added:
        // a pass that cannot be ordered is still a pass the caller asked for,
        // and dropping it silently is how a frame ends up empty with no
        // explanation.
        if self.execution_order.len() < self.nodes.len() {
            let unplaced: Vec<&str> = self
                .nodes
                .iter()
                .enumerate()
                .filter(|(i, _)| !self.execution_order.contains(i))
                .map(|(_, node)| node.name.as_str())
                .collect();
            crate::error!(
                "graph",
                "the pass dependencies contain a cycle, so these passes cannot be \
                 ordered: {unplaced:?}; they will run in the order they were added"
            );
            for i in 0..self.nodes.len() {
                if !self.execution_order.contains(&i) {
                    self.execution_order.push(i);
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

            // Execute the pass.
            //
            // Bug №653: the counter was incremented *before* the call, so a
            // pass that returned early — no framebuffer, no pipeline, no
            // state — was counted as having run. The doc comment claimed the
            // counts were "of real executions", and nothing kept that true.
            //
            // Two numbers now: how many times the pass was dispatched, and how
            // many times it actually contributed something to the frame. The
            // first says the graph reached it; the second says it did the
            // work. Five of the seven passes were dispatching every frame and
            // contributing nothing, and the old counter called that "running".
            self.execution_counts[node_index] += 1;
            let outcome = node.execute(&mut encoder, context, scene, &self.resources);
            if outcome.recorded {
                self.recorded_counts[node_index] += 1;
            } else if let Some(reason) = &outcome.reason {
                // The reason is worth a line, but not a line per frame: the
                // renderer logs a summary when it changes, and this is the
                // detail behind it.
                self.skip_reasons.insert(node.name.clone(), reason.clone());
            }
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
            //
            // And counted the same way as on the submitting path: dispatched
            // and recorded are two numbers, not one. Bug №653.
            self.execution_counts[node_index] += 1;
            let outcome = node.execute(&mut encoder, context, scene, &self.resources);
            if outcome.recorded {
                self.recorded_counts[node_index] += 1;
            } else if let Some(reason) = &outcome.reason {
                self.skip_reasons.insert(node.name.clone(), reason.clone());
            }
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

    /// How many times each pass has been dispatched, in execution order.
    ///
    /// Bug №313. The question this answers is "did the graph reach this
    /// subsystem", and nothing else in the build can answer it. A pass can be
    /// written, compiled, linted, documented and exported, and never execute
    /// once — which is precisely the state five of the seven passes were in:
    /// their pipelines were never created, their resources never allocated, and
    /// every build stayed green because there were no commands to object to.
    ///
    /// **"Dispatched", not "did the work".** A pass that was called and
    /// returned early is counted here. For the question that actually matters —
    /// did the pass contribute to a frame — see
    /// [`passes_without_work`](Self::passes_without_work) and
    /// [`recorded_counts`](Self::recorded_counts).
    ///
    /// Reported in execution order rather than insertion order, because the
    /// interesting comparison is against the order the frame is supposed to
    /// draw in, and insertion order would hide that two passes claiming to be
    /// sequential are not.
    ///
    /// Bug №653: every pass is listed, including one the topological sort
    /// could not place. The sort used to drop the unplaceable nodes silently,
    /// and since the counters were read off that order, a pass in a dependency
    /// cycle was invisible here — to the instrument built to find exactly that.
    pub fn pass_execution_counts(&self) -> Vec<(String, u64)> {
        let mut counts: Vec<(String, u64)> = self
            .execution_order
            .iter()
            .map(|&i| {
                let node = &self.nodes[i];
                (node.name.clone(), self.execution_counts.get(i).copied().unwrap_or(0))
            })
            .collect();
        // Anything the sort could not place, in the order it was added.
        for (i, node) in self.nodes.iter().enumerate() {
            if !self.execution_order.contains(&i) {
                counts.push((
                    node.name.clone(),
                    self.execution_counts.get(i).copied().unwrap_or(0),
                ));
            }
        }
        counts
    }

    /// How many times each pass actually contributed to a frame, in execution
    /// order. Bug №653.
    ///
    /// The counterpart to [`pass_execution_counts`](Self::pass_execution_counts):
    /// that one says the graph reached the pass, this one says the pass did
    /// something. A pass can be dispatched every frame and contribute nothing,
    /// and that is exactly the state the instrument could not see before.
    pub fn recorded_counts(&self) -> Vec<(String, u64)> {
        let mut counts: Vec<(String, u64)> = self
            .execution_order
            .iter()
            .map(|&i| {
                let node = &self.nodes[i];
                (node.name.clone(), self.recorded_counts.get(i).copied().unwrap_or(0))
            })
            .collect();
        for (i, node) in self.nodes.iter().enumerate() {
            if !self.execution_order.contains(&i) {
                counts.push((
                    node.name.clone(),
                    self.recorded_counts.get(i).copied().unwrap_or(0),
                ));
            }
        }
        counts
    }

    /// Passes that were dispatched but never contributed anything, with the
    /// reason. Bug №653.
    ///
    /// This is the list that answers "which subsystems are written and
    /// connected to nothing". A name on it is a pass the graph called and the
    /// pass declined to work — no pipeline, no framebuffer, no state — and the
    /// reason says which.
    pub fn passes_without_work(&self) -> Vec<(String, String)> {
        self.execution_order
            .iter()
            .filter_map(|&i| {
                let dispatched = self.execution_counts.get(i).copied().unwrap_or(0);
                let recorded = self.recorded_counts.get(i).copied().unwrap_or(0);
                if dispatched == 0 || recorded >= dispatched {
                    return None;
                }
                let name = self.nodes[i].name.clone();
                let reason = self
                    .skip_reasons
                    .get(&name)
                    .cloned()
                    .unwrap_or_else(|| "recorded nothing".to_string());
                Some((name, reason))
            })
            .collect()
    }

    /// Names of passes that have never been dispatched.
    ///
    /// The same defect class as [`pass_execution_counts`](Self::pass_execution_counts),
    /// narrowed to the part that is actionable: a pass on this list is on this list
    /// because it was added to the graph and then not reached — disabled, or
    /// unreachable through the dependency sort.
    ///
    /// A pass that *was* dispatched and declined to work is not on this list; it
    /// is on [`passes_without_work`](Self::passes_without_work), which says why.
    /// The two are different failures and the names must not be conflated.
    pub fn never_executed(&self) -> Vec<String> {
        self.pass_execution_counts()
            .into_iter()
            .filter(|(_, count)| *count == 0)
            .map(|(name, _)| name)
            .collect()
    }

    /// A one-line summary of what actually ran, for a diagnostic overlay or a log.
    ///
    /// Bug №653: the summary now says both things the graph knows — how many
    /// times each pass was dispatched, and how many of those dispatches
    /// contributed to the frame. A pass dispatched 60 times and recorded 0
    /// times reads `geometryx60!0`, and the `!` is the finding: the pass is
    /// being called sixty times a second and doing nothing.
    pub fn execution_summary(&self) -> String {
        if self.nodes.is_empty() {
            return "<no passes>".to_string();
        }
        // Looked up by name rather than zipped: the two lists are built
        // separately, and a zip would silently pair the wrong numbers if their
        // order ever diverged — which is exactly the kind of quiet wrong
        // answer this instrument exists to avoid.
        let worked: HashMap<String, u64> = self.recorded_counts().into_iter().collect();
        let parts: Vec<String> = self
            .pass_execution_counts()
            .into_iter()
            .map(|(name, count)| match worked.get(&name) {
                Some(worked) if *worked == count => format!("{name}x{count}"),
                Some(worked) => format!("{name}x{count}!{worked}"),
                None => format!("{name}x{count}"),
            })
            .collect();
        let idle = self.passes_without_work().len();
        let never = self.never_executed().len();
        format!(
            "{} ({} never ran, {} did no work)",
            parts.join(" "),
            never,
            idle
        )
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

#[cfg(test)]
mod tests {
    use crate::rhi::command::commands::Command;

    use super::RenderGraph;

    /// A frame that recorded nothing must count as nothing, not as a busy one:
    /// the summary line is what says "did this graph do work", and a non-zero
    /// on an empty list would answer that question with a lie.
    #[test]
    fn an_empty_command_list_counts_as_nothing_drawn() {
        let counts = RenderGraph::count_draws(&[]);
        assert_eq!(counts.draws, 0);
        assert_eq!(counts.triangles, 0);
        assert_eq!(counts.vertices, 0);
    }

    /// Draw and DrawIndexed are draws; a dispatch is not, and a non-indexed
    /// draw touches vertices without producing triangles.
    #[test]
    fn only_draws_are_counted_and_triangles_come_from_indices() {
        let counts = RenderGraph::count_draws(&[
            Command::Dispatch { x: 1, y: 1, z: 1 },
            Command::Draw {
                vertex_count: 100,
                instance_count: 2,
                first_vertex: 0,
                first_instance: 0,
            },
            Command::EndRenderPass,
            Command::DrawIndexed {
                index_count: 300,
                instance_count: 1,
                first_index: 0,
                vertex_offset: 0,
                first_instance: 0,
            },
        ]);
        assert_eq!(counts.draws, 2);
        assert_eq!(counts.vertices, 100 * 2 + 300);
        assert_eq!(counts.triangles, 300 / 3);
    }

    /// Instances multiply, and the multiplication saturates: a wrapped product
    /// would report a tiny frame from a huge one, and the summary is read by
    /// humans deciding whether the renderer is doing anything at all.
    #[test]
    fn an_overflowing_draw_saturates_instead_of_wrapping_to_a_tiny_number() {
        let counts = RenderGraph::count_draws(&[Command::Draw {
            vertex_count: u32::MAX,
            instance_count: 2,
            first_vertex: 0,
            first_instance: 0,
        }]);
        assert_eq!(counts.draws, 1);
        assert_eq!(counts.vertices, u32::MAX);
    }
}
