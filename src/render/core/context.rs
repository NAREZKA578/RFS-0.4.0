//! Render Context
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use std::sync::Arc;
use std::time::Duration;

use super::settings::GraphicsSettings;
use crate::rhi::core::Queue;
use crate::rhi::resource::{Texture, TextureView, TextureViewDesc, TextureViewType};
use crate::rhi::swapchain::{SwapChain, SwapChainDesc};
use crate::rhi::types::{
    Extent2D, Format, QueueFlags, TextureAspectFlags, TextureUsage,
};
use crate::rhi::Device;
use glam::Vec3;

/// Render context contains all the RHI resources needed for rendering
pub struct RenderContext {
    /// RHI Device
    pub device: Arc<Device>,
    /// Graphics queue
    pub graphics_queue: Queue,
    /// Present queue (may be same as graphics)
    pub present_queue: Queue,
    /// Swapchain
    pub swapchain: SwapChain,
    /// Current frame index
    pub frame_index: u32,
    /// Index of the swapchain image acquired for the frame being rendered.
    ///
    /// Bug №186: nothing recorded the acquired image, so the UI pass reached for
    /// `swapchain.images().first()`. On a multi-image swapchain that is not the
    /// image being presented, so the UI rendered into the wrong image and the
    /// presented frame was missing it (or showed a stale one).
    pub swapchain_image_index: u32,
    /// Delta time since last frame
    pub delta_time: Duration,
    /// Total time since start
    pub total_time: Duration,
    /// Current resolution
    pub resolution: Extent2D,
    /// Graphics settings
    pub settings: GraphicsSettings,
    /// Renderer configuration
    pub config: super::RendererConfig,
    /// Depth texture for current frame
    pub depth_texture: Option<Texture>,
    /// Depth texture view
    pub depth_view: Option<TextureView>,
    /// Current render target (swapchain image)
    pub render_target: Option<TextureView>,
    /// Format of the presentation surface, `None` when there is no surface.
    ///
    /// A pass needs this to build a framebuffer for a swapchain image, and it
    /// cannot ask the swapchain itself: that is a backend type the render layer
    /// is not supposed to know about. The renderer fills it in after acquire.
    pub presentation_format: Option<Format>,
    /// Extent of the presentation images, which follows the window.
    pub presentation_extent: Extent2D,
    /// Raw handles of every image in the presentation swapchain, parallel to
    /// `presentation_views`.
    ///
    /// These stay owned by the swapchain; the context only copies the handles
    /// so a pass can build a framebuffer without reaching into the backend.
    pub presentation_images: Vec<u64>,
    /// Raw image-view handles, parallel to `presentation_images`.
    pub presentation_views: Vec<u64>,
    /// Frame number (increments every frame)
    pub frame_number: u64,
    camera_position_value: Vec3,
}

impl RenderContext {
    /// Create a new render context
    /// A context with no window and no presentation.
    ///
    /// For offscreen work and for tests that drive a pass directly. The
    /// swapchain is the CPU stub with no images, so a pass that needs to
    /// present will find nothing to present to — which is the honest state, and
    /// preferable to a context that silently pretends to have a screen.
    pub fn headless(device: Arc<Device>) -> Self {
        Self::new(
            device.clone(),
            device.graphics_queue().cloned().unwrap_or(Queue {
                family_index: 0,
                index: 0,
                flags: QueueFlags::GRAPHICS,
            }),
            device.graphics_queue().cloned().unwrap_or(Queue {
                family_index: 0,
                index: 0,
                flags: QueueFlags::GRAPHICS,
            }),
            SwapChain::new(SwapChainDesc::default()),
            super::RendererConfig::default(),
            GraphicsSettings::default(),
        )
    }

    pub fn new(
        device: Arc<Device>,
        graphics_queue: Queue,
        present_queue: Queue,
        swapchain: SwapChain,
        config: super::RendererConfig,
        settings: GraphicsSettings,
    ) -> Self {
        let resolution = Extent2D::new(1920, 1080);

        Self {
            device,
            graphics_queue,
            present_queue,
            swapchain,
            frame_index: 0,
            swapchain_image_index: 0,
            delta_time: Duration::from_secs_f32(1.0 / 60.0),
            total_time: Duration::ZERO,
            resolution,
            settings,
            config,
            depth_texture: None,
            depth_view: None,
            render_target: None,
            presentation_format: None,
            presentation_extent: resolution,
            presentation_images: Vec::new(),
            presentation_views: Vec::new(),
            frame_number: 0,
            camera_position_value: Vec3::ZERO,
        }
    }

    /// Update context for a new frame
    pub fn begin_frame(&mut self, delta_time: Duration) {
        self.frame_index = (self.frame_index + 1) % 3;
        self.delta_time = delta_time;
        self.total_time += delta_time;
        self.frame_number += 1;
        // self.render_target = self.swapchain.get_current_view();

        // Update depth texture if resolution changed
        if self.depth_texture.is_none()
            || (self.depth_texture.as_ref().is_some_and(|t| {
                t.width() != self.resolution.width || t.height() != self.resolution.height
            }))
        {
            self.create_depth_texture();
        }
    }

    /// Create depth texture
    ///
    /// Through the device, because `Texture::new` builds a CPU-side handle with
    /// no GPU backing. `depth_view()` is public and any pass that binds it
    /// would then fail at framebuffer or bind time with "no GPU backing",
    /// reported from the draw rather than from the creation. Same defect as
    /// №261, in a different place.
    fn create_depth_texture(&mut self) {
        let depth_format = Format::D32_SFLOAT;

        let texture = self.device.create_texture(
            self.resolution.width,
            self.resolution.height,
            1,
            depth_format,
            TextureUsage::DEPTH_STENCIL_ATTACHMENT | TextureUsage::SAMPLED,
            1,
        );

        let desc = TextureViewDesc {
            texture: texture.clone(),
            format: None,
            view_type: TextureViewType::D2,
            aspects: TextureAspectFlags::DEPTH,
            base_mip_level: 0,
            mip_level_count: 1,
            base_array_layer: 0,
            array_layer_count: 1,
        };
        match self.device.create_texture_view(&texture, &desc) {
            Ok(view) => {
                self.depth_texture = Some(texture);
                self.depth_view = Some(view);
            }
            Err(e) => {
                // Reported here rather than left as a silent CPU stub: a depth
                // view that looks valid and is not is worse than none at all.
                eprintln!("[render] depth view creation failed: {e}");
                self.depth_texture = None;
                self.depth_view = None;
            }
        }
    }

    /// Get aspect ratio (guard zero height -> fallback, never inf).
    pub fn aspect_ratio(&self) -> f32 {
        if self.resolution.height == 0 {
            16.0 / 9.0
        } else {
            self.resolution.width as f32 / self.resolution.height as f32
        }
    }

    /// Check if a feature is supported
    pub fn is_supported(&self, feature: &str) -> bool {
        match feature {
            "ray_tracing" => self.config.supports_ray_tracing,
            "mesh_shading" => self.config.supports_mesh_shading,
            "bindless" => self.config.supports_bindless,
            _ => false,
        }
    }

    /// Get current frame depth texture view
    pub fn depth_view(&self) -> Option<&TextureView> {
        self.depth_view.as_ref()
    }

    /// Current camera position, updated by the renderer each frame from the
    /// active scene camera (defaults to origin before the first frame).
    pub fn camera_position(&self) -> Vec3 {
        self.camera_position_value
    }

    /// Renderer calls this once per frame with the active camera position.
    pub fn set_camera_position(&mut self, pos: Vec3) {
        self.camera_position_value = if pos.is_finite() { pos } else { Vec3::ZERO };
    }

    /// The device, so a pass can allocate what it needs while recording.
    pub fn device(&self) -> &Arc<Device> {
        &self.device
    }

    /// Renderer calls this when presentation changes, and again after a resize
    /// rebuilt the swapchain. The handles stay owned by the swapchain.
    pub fn set_presentation(
        &mut self,
        format: Option<Format>,
        extent: Extent2D,
        images: Vec<u64>,
        views: Vec<u64>,
    ) {
        self.presentation_format = format;
        self.presentation_extent = extent;
        self.presentation_images = images;
        self.presentation_views = views;
    }

    /// The image and view of the image acquired for the current frame.
    ///
    /// `None` when there is no surface, or when the swapchain was rebuilt this
    /// frame and the recorded index no longer refers to a live image — which is
    /// exactly when a pass must not draw into it.
    pub fn acquired_image(&self) -> Option<(u64, u64)> {
        let index = self.swapchain_image_index as usize;
        match (self.presentation_images.get(index), self.presentation_views.get(index)) {
            (Some(&image), Some(&view)) => Some((image, view)),
            _ => None,
        }
    }
}
