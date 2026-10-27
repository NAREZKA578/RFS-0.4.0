//! Main Renderer
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use std::sync::Arc;
use std::time::Instant;

use super::{GraphicsSettings, RenderApi, RenderContext, RendererConfig, RendererSettings};
use crate::render::graph::RenderGraph;
use crate::render::scene::Scene;
use crate::rhi::core::{DeviceDesc, Queue};
use crate::rhi::error::{RhiError, RhiResult};
use crate::rhi::swapchain::{SwapChain, SwapChainDesc};
use crate::rhi::types::{Extent2D, Features, Format};
use crate::rhi::{backend::create_backend, Device, GraphicsApi, RhiConfig};

/// Main renderer that orchestrates all rendering operations
pub struct Renderer {
    /// RHI Device
    device: Arc<Device>,
    /// Graphics queue
    _graphics_queue: Queue,
    /// Present queue
    _present_queue: Queue,
    /// Render graph
    render_graph: RenderGraph,
    /// Render context
    context: RenderContext,
    /// Settings
    settings: RendererSettings,
    /// Configuration
    config: RendererConfig,
    /// Last frame time
    last_frame_time: Instant,
    /// Is running
    running: bool,
    /// What the last frame cost and drew.
    ///
    /// Kept on the renderer rather than in the graph because it is a fact about
    /// the frame as a whole, and the graph is rebuilt while the renderer is not.
    stats: super::RenderStats,
    /// The last execution summary that differed from the one before it.
    ///
    /// Kept so a change in "which passes ran" is logged once instead of once per
    /// frame — a per-frame log of an unchanged summary is noise, and the summary
    /// only changes when the graph does.
    last_execution_summary: String,
    /// The real window presentation, when the renderer was given one.
    ///
    /// `None` means the renderer runs headless: the graph still records and
    /// submits (§209), but there is no surface to acquire from or present to,
    /// so `render_frame` reports that instead of pretending. Previously the
    /// renderer always held a CPU-only `SwapChain` stub whose `present` merely
    /// remembered an index, which made a headless run indistinguishable from a
    /// successful present.
    ///
    /// The surface is kept alive here because it owns the loader that destroys
    /// the `VkSurfaceKHR`, and it must outlive the swapchain.
    presentation: Option<Presentation>,
}

/// A surface and the swapchain presenting to it.
///
/// Bug №295: the field order below is load-bearing, and it was previously the other
/// way round. Rust drops struct fields in declaration order, so declaring the
/// surface first destroyed it *before* the swapchain built from it — which is
/// `VUID-vkDestroySurfaceKHR-surface-01266` on every exit. The struct comment used
/// to claim the opposite of what the code did ("ordered so the surface outlives the
/// chain" with the surface declared first), which is how the mistake survived review.
///
/// So: swapchain first, surface second, because the chain must die first. Reordering
/// these two fields reintroduces the bug, and the comment is here so that the next
/// reader takes the order as intentional rather than accidental.
struct Presentation {
    /// Destroyed first, so the surface below is still alive when it happens. That
    /// ordering is required, not stylistic — see the note above.
    swapchain: crate::rhi::present::Swapchain,
    /// Kept, not prefixed with `_`, because `resize` needs the same surface to
    /// rebuild the chain on. A surface is tied to the window handle and outlives
    /// any particular swapchain, so one surface is built once per window and
    /// every rebuild reuses it.
    surface: crate::rhi::present::WindowSurface,
}

impl Renderer {
    /// Create a new renderer
    pub fn new(_window: *mut std::ffi::c_void, config: RhiConfig) -> RhiResult<Self> {
        // Create RHI device
        let backend = create_backend(&config)?;
        let physical_device = backend
            .enumerate_physical_devices()?
            .into_iter()
            .next()
            .ok_or(RhiError::NoPhysicalDevices)?;
        // The particle pass's shader reads its system's storage buffer in the
        // VERTEX stage (one quad per particle, picked by
        // `@builtin(instance_index)`). Vulkan gates vertex-stage storage reads
        // behind `vertexPipelineStoresAndAtomics`, which the backend maps onto
        // exactly this flag (`features.storage_buffer`). Without the flag the
        // particle draw is undefined behaviour: the driver survives the
        // submissions that happen never to read and loses the device at the
        // first one that does — bisected against an RTX 2070 Super, see the
        // composite pixel test for the matrix. The D3D backends do not read
        // device features at creation time at all, so asking for the feature
        // costs nothing there.
        let device = Arc::new(backend.create_device(
            &physical_device,
            &DeviceDesc {
                features: Features {
                    storage_buffer: true,
                    ..Features::default()
                },
                ..Default::default()
            },
        )?);

        let graphics_queue = device.graphics_queue().cloned().unwrap_or(Queue {
            family_index: 0,
            index: 0,
            flags: crate::rhi::types::QueueFlags::GRAPHICS,
        });
        let present_queue = graphics_queue.clone();

        // Create swapchain
        let swapchain = SwapChain::new(SwapChainDesc::default());

        // Detect hardware capabilities
        let renderer_config = Self::detect_config(&device, config.api);

        // Apply settings based on hardware
        let mut settings = GraphicsSettings::default();
        if !renderer_config.supports_ray_tracing {
            settings.advanced.ray_tracing = false;
        }

        // Create render context
        let context = RenderContext::new(
            device.clone(),
            graphics_queue.clone(),
            present_queue.clone(),
            swapchain,
            renderer_config.clone(),
            settings,
        );

        Ok(Self {
            device,
            _graphics_queue: graphics_queue,
            _present_queue: present_queue,
            render_graph: RenderGraph::new(),
            context,
            settings: RendererSettings::default(),
            config: renderer_config,
            last_frame_time: Instant::now(),
            running: true,
            stats: super::RenderStats::new(),
            last_execution_summary: String::new(),
            presentation: None,
        })
    }

    /// Give the renderer a real window to present to.
    ///
    /// Separate from `new` rather than a new argument because `new` is already
    /// used headless — by tests and by anything that renders to an offscreen
    /// target — and a window is not always available. `hinstance` and `hwnd` are
    /// the raw Win32 handles from `raw-window-handle`; there is no point taking
    /// a `*mut c_void` that has already been thrown away.
    ///
    /// Any existing presentation is replaced, which is also how a resize is
    /// handled: the old chain is handed to Vulkan as `oldSwapchain` so the
    /// window is never momentarily without one.
    pub fn attach_presentation(
        &mut self,
        hinstance: u64,
        hwnd: u64,
        width: u32,
        height: u32,
    ) -> RhiResult<()> {
        let surface = self.device.create_window_surface(hinstance, hwnd)?;
        let request = crate::rhi::present::SwapchainRequest {
            width: width.max(1),
            height: height.max(1),
            present_mode: crate::rhi::present::PresentMode::Fifo,
        };
        // Taken by value, not borrowed: the handover needs `&mut` to mark the old
        // chain as handed over, and `self` is borrowed immutably below.
        let mut old = self.presentation.take();
        let swapchain = match old.as_mut() {
            Some(existing) => self
                .device
                .create_swapchain(&surface, request, Some(&mut existing.swapchain))?,
            None => self.device.create_swapchain(&surface, request, None)?,
        };
        self.presentation = Some(Presentation { surface, swapchain });
        // `old` drops here. Its swapchain handle was consumed by Vulkan during
        // the handover, so `Drop` leaves it alone; its surface goes with it.
        //
        // Bug #645: the presentation state was not published here, and this is
        // the only path that creates the first chain. `refresh_presentation_state`
        // is the sole producer of `context.presentation_texture_views`, and it
        // was called from `resize` and `rebuild_presentation` — so on a fresh
        // window the vector was empty until the user dragged the window, and
        // `UIPass::execute` returned before drawing anything:
        //
        //     let Some(view) = context.presentation_texture_views.get(..) else { return; };
        //
        // The example written to answer "does a UI pixel reach the screen" could
        // not answer it, because the path it checked was cut short before it
        // got there.
        self.refresh_presentation_state();
        Ok(())
    }

    /// Whether this renderer has a real surface to present to.
    pub fn has_presentation(&self) -> bool {
        self.presentation.is_some()
    }

    /// The format the presentation surface actually settled on.
    ///
    /// `None` without a surface. A pipeline must be built for this format and
    /// not for whatever the RHI would have preferred, so anything registering
    /// a pass has to read it from here after `attach_presentation`.
    pub fn presentation_format(&self) -> Option<Format> {
        self.presentation.as_ref().and_then(|p| p.swapchain.rhi_format())
    }

    /// The size of the presentation images, which follows the window.
    pub fn presentation_extent(&self) -> Extent2D {
        self.presentation
            .as_ref()
            .map(|p| p.swapchain.extent())
            .unwrap_or(self.context.resolution)
    }

    /// The render graph, for registering passes.
    ///
    /// Needed because nothing else registers them: `RenderGraph::initialize`
    /// does not add passes, and `add_pass` has no caller, so without this a
    /// pass could not be added to a graph at all.
    pub fn render_graph_mut(&mut self) -> &mut RenderGraph {
        &mut self.render_graph
    }

    /// Detect hardware configuration
    fn detect_config(device: &Device, requested_api: GraphicsApi) -> RendererConfig {
        let mut config = RendererConfig::default();
        config.backend = match requested_api {
            GraphicsApi::Vulkan => RenderApi::Vulkan,
            GraphicsApi::Direct3D12 => RenderApi::Direct3D12,
            GraphicsApi::Direct3D11 => RenderApi::Direct3D11,
            GraphicsApi::OpenGL => RenderApi::OpenGL,
        };
        let physical_device = device.physical_device();
        config.device_name = physical_device.name.clone();
        config.vendor = format!(
            "{}:{}",
            physical_device.vendor_id, physical_device.device_id
        );
        config.memory = physical_device
            .memory_properties
            .memory_heaps
            .first()
            .map_or(0, |h| h.size);

        // Check feature support
        let features = &physical_device.features;
        config.supports_ray_tracing = features.ray_tracing;
        config.supports_mesh_shading = features.mesh_shader;
        config.supports_bindless = features.descriptor_indexing;

        // Get limits
        let limits = &physical_device.limits;
        config.max_texture_size = limits.max_texture_size;
        config.max_compute_work_group_size = limits.max_compute_work_group_size;

        config
    }

    /// Initialize the renderer with a scene
    pub fn initialize(&mut self, scene: &mut Scene) {
        // Initialize render graph passes
        self.render_graph
            .initialize(&self.device, &self.context, scene);
    }

    /// Render a frame
    pub fn render_frame(&mut self, scene: &mut Scene) {
        if !self.running {
            return;
        }
        self.stats.begin_frame();

        // Calculate delta time
        let now = Instant::now();
        let delta_time = now.duration_since(self.last_frame_time);
        self.last_frame_time = now;

        // Begin frame
        self.context.begin_frame(delta_time);
        // Sync camera position from the active scene camera so LOD,
        // transparent sorting and distance culling measure from the camera.
        self.context.set_camera_position(scene.camera_position());

        // Initialize the graph if nobody has.
        //
        // A graph that was never initialized has no resources and no
        // initialized passes, so every pass in it records nothing and the
        // frame is submitted empty. That failure is invisible from outside:
        // the instrument that knows is an info line, and the default threshold
        // prints warnings and not info. So the first frame initializes the
        // graph by itself, and a caller who added passes and rendered gets
        // them on the screen without having to know about a separate step.
        if !self.render_graph.is_initialized() {
            self.render_graph
                .initialize(&self.device, &self.context, scene);
        }

        // Update scene
        scene.update(delta_time, &self.context);

        // Acquire the image to draw into, before anything records.
        //
        // Previously the graph ran first and presentation second, so a pass
        // that targets the swapchain had no acquired image to bind: it would
        // have had to guess image 0, and the guessed image is not the one being
        // presented. Acquire is a prerequisite of recording, not something that
        // happens after it.
        //
        // This is also the only point where the image being presented to
        // becomes known, so it is where the per-image presentation state is
        // published for the passes.
        let acquired_index = match self.presentation.as_mut() {
            Some(presentation) => {
                // Bug №282: the acquire hands the presentation engine's semaphore
                // to the frame's submit instead of blocking this thread on a
                // fence. The CPU now gets ahead of the GPU by up to
                // `FRAMES_IN_FLIGHT` frames, which is the entire point of the ring.
                let image_acquired = self
                    .device
                    .backend_resource()
                    .map(|backend| backend.image_acquired_semaphore())
                    .unwrap_or_else(crate::rhi::ash::vk::Semaphore::null);
                let acquired = match presentation.swapchain.acquire(u64::MAX, image_acquired) {
                    Ok(acquired) => acquired,
                    // Out of date means the chain no longer matches the surface,
                    // which happens on every resize and on entering fullscreen.
                    // It used to be a silent `return`, so the frame was skipped
                    // before anything was recorded and nothing was logged — the
                    // window silently stopped updating, and repeated resizes
                    // left it that way permanently because nothing rebuilt the
                    // chain.
                    //
                    // Rebuild now and let this frame go; the next one acquires
                    // from the new chain. Logging matters: without it, a failed
                    // rebuild and a stale acquire look identical from outside.
                    Err(RhiError::SwapchainOutOfDate) => {
                        self.rebuild_presentation();
                        return;
                    }
                    Err(e) => {
                        crate::error!("renderer", "swapchain acquire failed: {e}");
                        return;
                    }
                };
                let swapchain = &presentation.swapchain;
                self.context.set_presentation(
                    swapchain.rhi_format(),
                    swapchain.extent(),
                    swapchain.images(),
                    swapchain.image_views(),
                );
                acquired.image_index
            }
            None => {
                // Headless. Say so once per frame rather than silently.
                self.context.set_presentation(None, self.context.resolution, Vec::new(), Vec::new());
                0
            }
        };

        // Bug №186: record the acquired index so the passes that target the
        // swapchain render into the image about to be presented, instead of
        // always reaching for image 0.
        self.context.swapchain_image_index = acquired_index;
        // The present waits on the semaphore of the *image* being presented, so
        // the submit has to know which image it renders into. The frame slot and
        // the image are unrelated — a slot comes round again after
        // `FRAMES_IN_FLIGHT` frames, and the present of that image may still be
        // pending — and sharing one semaphore between them is
        // VUID-vkQueueSubmit-pSignalSemaphores-00067.
        self.device.set_present_image(acquired_index);

        // Execute render graph
        //
        // Recorded after acquire, so a pass targeting the swapchain can bind
        // the image that is about to be presented (§209, §186).
        // Bug №282: armed so the submit inside `execute` signals the semaphore that
        // `present` below waits on. The submit has no other way to know that this
        // particular submission is the one being presented.
        // Bug №282: a single present semaphore is NOT sufficient and was measured not to
        // be. The submit waits on a fence, but `queue_present` only *queues* its
        // wait on the semaphore — the semaphore is consumed later, asynchronously.
        // Re-signalling it on the next frame therefore trips
        // VUID-vkQueueSubmit-pSignalSemaphores-00067 ("may still be in use by
        // VkSwapchainKHR") every frame. With the signal disabled the index stayed
        // at 0 instead of cycling, which is the same defect as before, so this is
        // a reverted experiment rather than a partial fix. The real answer is a
        // ring of frames in flight with one semaphore per slot.
        //
        // Bug №282, closing: `arm_presentation_signal` was never called from
        // anywhere in the tree. The flag existed, the submit honoured it, and the
        // renderer passed a null semaphore to `present` while explaining in a
        // comment that nothing was signalled — which is exactly what was
        // happening. The comment described the defect instead of causing its
        // removal.
        //
        // Arming here, not inside `execute`, because only the renderer knows
        // whether this frame is bound for the swapchain. An offscreen frame must
        // not signal a semaphore nothing waits on, or it stays signalled forever
        // and the next present consumes it without waiting.
        if self.presentation.is_some() {
            if let Some(backend) = self.device.backend_resource() {
                backend.arm_presentation_signal();
            }
        }
        self.render_graph
            .execute(&self.device, &mut self.context, scene);

        // Picked up rather than read: the graph counted these while it recorded,
        // and the number describes the frame that was just submitted.
        let draws = self.render_graph.take_last_frame_draws();
        self.stats
            .add_draws(draws.draws, draws.triangles, draws.vertices);

        // A change in "which passes ran" is worth one line; the same summary every
        // frame is not. A pass that never runs is raised to a warning because the
        // default threshold prints warnings and not info — the finding must be
        // visible without anyone remembering to raise it (§313). The draw counts
        // ride along on purpose: `RenderStats` used to be written and read by
        // nothing, and a counter with no reader is not a diagnostic. One line per
        // change keeps the log quiet.
        //
        // Bug №653: a pass that was *called* and did nothing is the finding
        // this instrument was built for, and it is not the same as a pass that
        // was never reached. Five of the seven passes were in exactly that
        // state: dispatched every frame, contributing nothing, and the old
        // counter called that "running". So the two lists are separated, and
        // the reasons are printed — "no framebuffer" and "no pipeline" are two
        // different bugs, and a list of names cannot tell them apart.
        let summary = self.render_graph.execution_summary();
        if summary != self.last_execution_summary {
            self.last_execution_summary = summary.clone();
            let line = format!(
                "graph: {summary}; frame: {} draw(s), {} triangle(s), {} vertex/indices",
                draws.draws, draws.triangles, draws.vertices
            );
            let never = self.render_graph.never_executed();
            let idle = self.render_graph.passes_without_work();
            if never.is_empty() && idle.is_empty() {
                crate::info!("renderer", "{line}");
            } else {
                crate::warn!("renderer", "{line}");
                for name in &never {
                    crate::warn!("renderer", "the pass {name:?} was never reached");
                }
                for (name, reason) in &idle {
                    crate::warn!("renderer", "the pass {name:?} did no work: {reason}");
                }
            }
        }

        if self.presentation.is_some() {
            let presentation = self
                .presentation
                .as_mut()
                .expect("presentation presence was just checked");
            // The submit that just ran signalled this slot's semaphore, and
            // consuming it is what releases the image for the next acquire.
            // `render_finished_semaphore` returns the slot the *last* submit
            // used, which is the one being presented here.
            let render_finished = self
                .device
                .backend_resource()
                .map(|backend| backend.render_finished_semaphore())
                .unwrap_or_else(crate::rhi::ash::vk::Semaphore::null);
            if let Err(e) = presentation.swapchain.present(acquired_index, render_finished) {
                // Bug #649: an out-of-date chain has to be rebuilt here too, not
                // only on the acquire path. The present is where a suboptimal
                // chain is first reported (Bug #640), and the acquire only
                // reports it on the *next* frame — so a window that was resized
                // or minimised kept presenting a stale image for one frame, and
                // the error was swallowed here, so nothing rebuilt it.
                //
                // The acquire path rebuilds and returns; the present path cannot
                // return, because the frame has already been recorded and
                // submitted. It rebuilds and lets the frame go.
                if matches!(e, RhiError::SwapchainOutOfDate) {
                    self.rebuild_presentation();
                } else {
                    crate::error!("renderer", "present failed: {e}");
                }
            }
        }

        // No frame counter increment here.
        //
        // Bug №286: `context.begin_frame`, called at the top of this function,
        // already increments `context.frame_number`. This used to increment it a
        // second time, so the counter advanced twice per rendered frame and
        // anything deriving a phase or an id from it — TAA jitter, animation
        // timing, a per-frame index — was wrong by a factor of two. It was
        // invisible while the number was only ever printed.
        //
        // `begin_frame` is the single place that advances the frame, because it
        // is also what advances `frame_index`, `delta_time` and `total_time`.
        //
        // `end_frame` is here for the same reason in reverse: it is the only place
        // that measures the frame that just finished, and it must run after the
        // present so the measurement covers the whole frame rather than the part
        // of it before the swapchain was handed the image.
        self.stats.end_frame();
    }

    /// Resize the renderer
    pub fn resize(&mut self, width: u32, height: u32) {
        let width = width.max(1);
        let height = height.max(1);
        self.context.resolution = crate::rhi::types::Extent2D::new(width, height);

        // The context's own depth texture is deliberately *not* cleared here.
        //
        // Bug №289: these two lines read as "drop the stale depth on resize" and
        // were exactly a leak — assigning `None` drops the Rust handle while the
        // Vulkan image and view stay in the teardown list, one pair per resize.
        //
        // They were also redundant. `resolution` is set above, and
        // `RenderContext::begin_frame` rebuilds the depth texture whenever its
        // dimensions no longer match, so the recreation happens on the next frame
        // either way — and `create_depth_texture` now releases what it replaces.

        // Passes are told about the new size before the chain is rebuilt. Bug №311:
        // `RenderPassNode::resize` had no call sites, so a pass holding
        // size-dependent resources would have kept the old ones and rendered at
        // stale dimensions. Order does not matter between this and the rebuild
        // below — both happen before the next frame is recorded — but both must
        // happen before it, which is why neither is deferred to `render_frame`.
        self.render_graph.resize(width, height);

        // Rebuild the swapchain at the new extent.
        //
        // This line used to be commented out, and the consequence was a dead
        // render loop rather than a visible error: after a resize `acquire` keeps
        // returning `SUBOPTIMAL`/`OUT_OF_DATE`, `render_frame` returns before
        // recording anything, and the window keeps showing the last frame that
        // was drawn. Nothing was logged, so switching to fullscreen looked
        // exactly like a hang.
        //
        // The handover form is required rather than a fresh chain: Vulkan forbids
        // destroying a chain whose images are still in flight, and passing the old
        // one lets the driver retire them in order.
        // A zero extent is what a minimised window reports, and Vulkan rejects a
        // swapchain with a zero `imageExtent` outright. Attempting the rebuild
        // anyway is what produced VUID-VkSwapchainCreateInfoKHR-imageExtent-01689.
        // There is also nothing to rebuild: the surface has no size yet, so the
        // chain is rebuilt when the window comes back, which arrives as a resize.
        if width == 0 || height == 0 {
            return;
        }
        if let Some(presentation) = self.presentation.as_mut() {
            let request = crate::rhi::present::SwapchainRequest {
                width,
                height,
                present_mode: crate::rhi::present::PresentMode::Fifo,
            };
            match self
                    .device
                    .create_swapchain(&presentation.surface, request, Some(&mut presentation.swapchain))
                {
                    Ok(swapchain) => {
                        presentation.swapchain = swapchain;
                        // Bug №289: assigning above drops the previous `Swapchain`,
                        // and its `Drop` destroys the image views that any framebuffer
                        // cached for those images still references. Bumping the
                        // generation is what tells passes their cache is stale; without
                        // it a resize that leaves the extent unchanged serves a dead
                        // framebuffer and the driver reports an invalid `VkImageView`
                        // at the next `beginRenderPass`.
                        self.context.swapchain_generation =
                            self.context.swapchain_generation.wrapping_add(1);
                    }
                    Err(e) => {
                        // Reported rather than swallowed: a resize that could not
                        // rebuild the chain leaves presentation permanently broken,
                        // and the next frames will fail to acquire for a reason that
                        // has nothing to do with acquire itself.
                        crate::error!("renderer", "swapchain rebuild on resize failed: {e}");
                    }
                }
                // Same reason as in `rebuild_presentation`: the new handles have to
                // reach the context now, or the next frame renders through the
                // retired chain.
                self.refresh_presentation_state();
            }
    }

    /// Rebuild the presentation chain from the surface's current extent.
    ///
    /// Two callers: `resize`, which knows the new size, and `acquire` reporting
    /// out of date, which does not — the surface itself is then the authority.
    /// Falling back to `context.resolution` only when the surface reports nothing
    /// keeps a zero extent from producing a 1x1 chain that then fails again.
    fn rebuild_presentation(&mut self) {
        if self.presentation.is_none() {
            crate::warn!("renderer", "out-of-date presentation but no surface to rebuild from");
            return;
        }
        // The surface is the authority here, because the caller that reaches this
        // path (a failed acquire) does not know the new size — only the surface
        // does. `context.resolution` is the fallback when the surface reports
        // nothing.
        let extent = self.presentation_extent();
        let (width, height) = if extent.width > 0 && extent.height > 0 {
            (extent.width, extent.height)
        } else {
            (
                self.context.resolution.width.max(1),
                self.context.resolution.height.max(1),
            )
        };
        // A minimised window reports zero on both axes. Vulkan rejects a chain with
        // a zero imageExtent, and there is nothing useful to rebuild yet, so this
        // is not an error — the chain is rebuilt when the window returns, which
        // arrives as a resize.
        if width == 0 || height == 0 {
            return;
        }
        let Some(presentation) = self.presentation.as_mut() else {
            return;
        };
        let request = crate::rhi::present::SwapchainRequest {
            width,
            height,
            present_mode: crate::rhi::present::PresentMode::Fifo,
        };
        match self
            .device
            .create_swapchain(&presentation.surface, request, Some(&mut presentation.swapchain))
        {
            Ok(swapchain) => {
                crate::warn!("renderer", "presentation rebuilt at {width}x{height}");
                presentation.swapchain = swapchain;
                // Republish immediately: until the context knows the new handles,
                // a pass would build a framebuffer from the retired chain.
                self.refresh_presentation_state();
            }
            Err(e) => crate::error!("renderer", "presentation rebuild failed: {e}"),
        }
        // The context must be republish now, not on the next acquire.
        //
        // Without this the passes keep building framebuffers from the *previous*
        // chain's image handles, and rendering continues against views the driver
        // has already retired — which surfaced as
        // VUID-VkRenderPassBeginInfo-framebuffer-parameter: pAttachments[0]
        // VkImageView ... is invalid. The acquire path does this on every frame,
        // which is why the frame after a successful rebuild is the first one with
        // valid handles.
        self.refresh_presentation_state();
    }

    /// Copy the current chain's format, extent and image handles into the context.
    ///
    /// Split out because both the normal acquire path and the rebuild path need
    /// it, and forgetting it in the second place is exactly the bug above.
    fn refresh_presentation_state(&mut self) {
        let Some(presentation) = self.presentation.as_ref() else {
            self.context
                .set_presentation(None, self.context.resolution, Vec::new(), Vec::new());
            return;
        };
        let swapchain = &presentation.swapchain;
        self.context.set_presentation(
            swapchain.rhi_format(),
            swapchain.extent(),
            swapchain.images(),
            swapchain.image_views(),
        );
        self.publish_typed_presentation_views();
    }

    /// Build typed `TextureView`s over the live swapchain images. Bug №298.
    ///
    /// A pass needs a typed view to build a framebuffer: `Texture` and `TextureView`
    /// expose no `as_raw`, and a `TextureView` cannot be reconstructed from a `u64`
    /// because it needs the `Texture` it was made from. So the raw handles below are
    /// enough to describe the window and not enough to draw into it.
    ///
    /// Views are created, not adopted, because the swapchain's views are 2D and
    /// single-layer while a `TextureView` carries a description — the description has
    /// to exist either way. The underlying *images* are adopted, never destroyed:
    /// `Swapchain` owns those, and tracking them here as well would give two owners
    /// and a double free.
    ///
    /// A failure here must not take the frame down. Without these views a pass can
    /// fall back to the raw handles it had before, which is exactly the state this
    /// fixes but which does not crash.
    fn publish_typed_presentation_views(&mut self) {
        let Some(presentation) = self.presentation.as_ref() else {
            self.context.set_presentation_texture_views(Vec::new());
            return;
        };
        let swapchain = &presentation.swapchain;
        let extent = swapchain.extent();
        let width = extent.width.max(1);
        let height = extent.height.max(1);
        let Some(format) = swapchain.rhi_format() else {
            crate::error!(
                "renderer",
                "swapchain reports no format, so no typed presentation views can be \
                 built; passes needing one will fall back to the raw handles"
            );
            self.context.set_presentation_texture_views(Vec::new());
            return;
        };

        let mut views = Vec::with_capacity(swapchain.image_count());

        // The views published last time are ours — created by this function, not the
        // swapchain's — so nothing else will free them. Releasing them here is not an
        // optimisation: skipping it leaked three per swapchain rebuild, which the
        // resize self-check showed immediately as 23 → 26 → 29 tracked objects. Note
        // they are independent objects even though they look at the same images; the
        // swapchain destroying *its* views does not touch ours.
        let previous = std::mem::take(&mut self.context.presentation_texture_views);
        if !previous.is_empty() {
            let stale: Vec<crate::rhi::core::device::Releasable<'_>> =
                previous.iter().map(crate::rhi::core::device::Releasable::TextureView).collect();
            self.device.release_many(&stale);
        }

        for &image in swapchain.images().iter() {
            let texture = self.device.adopt_texture(
                crate::rhi::TextureDesc {
                    width,
                    height,
                    depth: 1,
                    mip_levels: 1,
                    array_layers: 1,
                    // The swapchain can report no format before presentation is
                    // attached. Falling back to `R8G8B8A8_UNORM` would silently
                    // describe the image wrong, and the view would then disagree with
                    // the render pass — better to publish nothing and let the caller
                    // fall back to the raw handles.
                    format: format,
                    usage: crate::rhi::TextureUsage::COLOR_ATTACHMENT,
                    sample_count: crate::rhi::SampleCount::X1,
                    dimensions: crate::rhi::TextureDimensions::D2,
                    sharing_mode: crate::rhi::SharingMode::Exclusive,
                    queue_family_indices: Vec::new(),
                },
                image,
            );
            match self.device.create_texture_view(
                &texture,
                &crate::rhi::TextureViewDesc {
                    texture: texture.clone(),
                    format: None,
                    view_type: crate::rhi::TextureViewType::D2,
                    aspects: crate::rhi::TextureAspectFlags::COLOR,
                    base_mip_level: 0,
                    mip_level_count: 1,
                    base_array_layer: 0,
                    array_layer_count: 1,
                },
            ) {
                Ok(view) => views.push(view),
                Err(e) => {
                    crate::error!(
                        "renderer",
                        "typed view for a swapchain image failed: {e}; passes needing a \
                         typed view will fall back to the raw handles"
                    );
                    self.context.set_presentation_texture_views(Vec::new());
                    return;
                }
            }
        }
        self.context.set_presentation_texture_views(views);
    }

    /// Update settings
    pub fn update_settings(&mut self, settings: RendererSettings) {
        // Compare BEFORE overwriting (old code compared the copy to itself).
        let shadow_changed = self.settings.shadow_quality != settings.shadow_quality;
        self.settings = settings;
        let graphics_settings = GraphicsSettings {
            advanced: crate::render::core::settings::AdvancedSettings {
                ray_tracing: self.settings.ray_tracing.enabled,
                ..Default::default()
            },
            shadow_quality: self.settings.shadow_quality,
            ..Default::default()
        };
        self.context.settings = graphics_settings;

        // Apply settings that require reconstruction
        if shadow_changed {
            // Recreate shadow passes
        }
    }

    /// Get renderer configuration
    pub fn config(&self) -> &RendererConfig {
        &self.config
    }

    /// What the last frame cost and drew.
    ///
    /// `RenderStats` used to be exported and never called: `begin_frame`,
    /// `end_frame` and `add_draw_call` had no call sites anywhere, so every
    /// field read zero — a statistic that always reports nothing is worse than
    /// no statistic, because it can be printed and believed.
    pub fn stats(&self) -> &super::RenderStats {
        &self.stats
    }

    /// One line describing which passes ran in the last frame and how often.
    ///
    /// Exists so a caller (an overlay, a test, a log on demand) can ask instead
    /// of only seeing the line `render_frame` emits when the summary changes.
    pub fn execution_summary(&self) -> &str {
        &self.last_execution_summary
    }

    /// Get current settings
    pub fn settings(&self) -> &RendererSettings {
        &self.settings
    }

    /// Get render context
    pub fn context(&self) -> &RenderContext {
        &self.context
    }

    /// Get mutable render context
    pub fn context_mut(&mut self) -> &mut RenderContext {
        &mut self.context
    }

    /// Get device
    pub fn device(&self) -> &Arc<Device> {
        &self.device
    }

    /// Get swapchain
    pub fn swapchain(&self) -> &SwapChain {
        &self.context.swapchain
    }

    /// Stop the renderer
    pub fn stop(&mut self) {
        self.running = false;
    }

    /// Start the renderer
    pub fn start(&mut self) {
        self.running = true;
        self.last_frame_time = Instant::now();
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        // Wait for the GPU to finish before the device is torn down underneath it.
        //
        // Bug №288: this used to `.expect("device wait_idle not implemented")`.
        // Two defects in one line. The message was stale and wrong — `wait_idle`
        // has been implemented for some time — so a genuine failure reported a
        // reason that pointed nowhere. And panicking in `Drop` is worse than the
        // condition it reports: a device that failed to go idle is already in a
        // bad state, and unwinding out of a destructor while the GPU may still be
        // reading destroyed objects turns a recoverable shutdown into an abort.
        //
        // Nothing useful can be done about it here, but reporting is better than
        // aborting, and it keeps the real cause in the output.
        if let Err(e) = self.device.wait_idle() {
            eprintln!("[render] device_wait_idle failed during shutdown: {e}");
        }

        // Bug №295: presentation is torn down here, explicitly, before the fields
        // drop.
        //
        // `device` is declared first, and Rust drops struct fields in declaration
        // order — so the device was destroyed before the swapchain that belongs to
        // it. Destroying a swapchain after its device has gone does not clean
        // anything up; the validation layer counts its three image views, its fence
        // and the swapchain itself as leaked on every exit, which is 5 of the objects
        // that were reported missing even with no resize involved.
        //
        // Relying on field order would be the same bug waiting for the next
        // reordering, so the order is stated instead of inherited.
        self.presentation = None;
    }
}
