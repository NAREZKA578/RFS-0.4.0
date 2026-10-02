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
use crate::rhi::ash::vk::Handle;
use crate::rhi::swapchain::{SwapChain, SwapChainDesc};
use crate::rhi::types::{Extent2D, Format};
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
/// Ordered so the surface outlives the chain: fields drop in declaration order,
/// and destroying a swapchain after its surface is invalid.
struct Presentation {
    _surface: crate::rhi::backend::vulkan::WindowSurface,
    swapchain: crate::rhi::backend::vulkan::Swapchain,
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
        let device = Arc::new(backend.create_device(&physical_device, &DeviceDesc::default())?);

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
        let request = crate::rhi::backend::vulkan::SwapchainRequest {
            width: width.max(1),
            height: height.max(1),
            present_mode: crate::rhi::ash::vk::PresentModeKHR::FIFO,
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
        self.presentation = Some(Presentation { _surface: surface, swapchain });
        // `old` drops here. Its swapchain handle was consumed by Vulkan during
        // the handover, so `Drop` leaves it alone; its surface goes with it.
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
            .map(|p| Extent2D::new(p.swapchain.extent.width, p.swapchain.extent.height))
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
        let backend = match requested_api {
            GraphicsApi::Vulkan => RenderApi::Vulkan,
            GraphicsApi::Direct3D12 => RenderApi::Direct3D12,
            GraphicsApi::Direct3D11 => RenderApi::Direct3D11,
            GraphicsApi::OpenGL => RenderApi::OpenGL,
        };
        let mut config = RendererConfig {
            backend,
            ..Default::default()
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

        // Calculate delta time
        let now = Instant::now();
        let delta_time = now.duration_since(self.last_frame_time);
        self.last_frame_time = now;

        // Begin frame
        self.context.begin_frame(delta_time);
        // Sync camera position from the active scene camera so LOD,
        // transparent sorting and distance culling measure from the camera.
        self.context.set_camera_position(scene.camera_position());

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
                let acquired = match presentation.swapchain.acquire(u64::MAX) {
                    Ok(acquired) => acquired,
                    Err(RhiError::SwapchainOutOfDate) => return,
                    Err(e) => {
                        crate::error!("renderer", "swapchain acquire failed: {e}");
                        return;
                    }
                };
                let swapchain = &presentation.swapchain;
                self.context.set_presentation(
                    swapchain.rhi_format(),
                    Extent2D::new(swapchain.extent.width, swapchain.extent.height),
                    swapchain
                        .images
                        .iter()
                        .map(|image| image.as_raw())
                        .collect(),
                    swapchain
                        .image_views
                        .iter()
                        .map(|view| view.as_raw())
                        .collect(),
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

        // Execute render graph
        //
        // Recorded after acquire, so a pass targeting the swapchain can bind
        // the image that is about to be presented (§209, §186).
        self.render_graph
            .execute(&self.device, &mut self.context, scene);

        if let Some(presentation) = self.presentation.as_mut() {
            // No fence wait here: the graph's submit path already blocks on its
            // own fence before returning, so the work is finished. There used to
            // be a wait here, on a fence that the *acquire* had signalled rather
            // than the render, so it never actually waited for anything.
            if let Err(e) = presentation.swapchain.present(acquired_index) {
                if !matches!(e, RhiError::SwapchainOutOfDate) {
                    crate::error!("renderer", "present failed: {e}");
                }
            }
        }

        // Increment frame counter
        self.context.frame_number += 1;
    }

    /// Resize the renderer
    pub fn resize(&mut self, width: u32, height: u32) {
        // self.swapchain.resize(width, height).expect("swapchain resize not implemented");
        self.context.resolution = crate::rhi::types::Extent2D::new(width, height);

        // Recreate depth texture
        self.context.depth_texture = None;
        self.context.depth_view = None;

        // Reinitialize render graph
        // self.render_graph.resize(&self.device, &self.context);
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
        // Wait for GPU to finish
        self
            .device
            .wait_idle()
            .expect("device wait_idle not implemented");
    }
}
