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
        })
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

        // Calculate delta time
        let now = Instant::now();
        let delta_time = now.duration_since(self.last_frame_time);
        self.last_frame_time = now;

        // Begin frame
        self.context.begin_frame(delta_time);

        // Update scene
        scene.update(delta_time, &self.context);

        // Acquire next swapchain image
        let _ = self
            .context
            .swapchain
            .acquire_next_image()
            .expect("swapchain acquire not implemented");

        // Execute render graph
        self.render_graph
            .execute(&self.device, &mut self.context, scene);

        // Present
        let _ = self
            .context
            .swapchain
            .present(0)
            .expect("swapchain present not implemented");

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
        if self.context.settings.shadow_quality != self.settings.shadow_quality {
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
        let _ = self
            .device
            .wait_idle()
            .expect("device wait_idle not implemented");
    }
}
