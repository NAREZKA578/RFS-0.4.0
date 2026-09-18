//! Render Context
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use std::sync::Arc;
use std::time::Duration;

use super::settings::GraphicsSettings;
use crate::rhi::core::Queue;
use crate::rhi::resource::{
    Texture, TextureDesc, TextureView, TextureViewDesc, TextureViewType,
};
use crate::rhi::swapchain::SwapChain;
use crate::rhi::types::{
    Extent2D, Format, SampleCount, SharingMode, TextureAspectFlags, TextureDimensions,
    TextureUsage,
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
    /// Frame number (increments every frame)
    pub frame_number: u64,
}

impl RenderContext {
    /// Create a new render context
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
            delta_time: Duration::from_secs_f32(1.0 / 60.0),
            total_time: Duration::ZERO,
            resolution,
            settings,
            config,
            depth_texture: None,
            depth_view: None,
            render_target: None,
            frame_number: 0,
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
            || (self.depth_texture.as_ref().map_or(false, |t| {
                t.width() != self.resolution.width || t.height() != self.resolution.height
            }))
        {
            self.create_depth_texture();
        }
    }

    /// Create depth texture
    fn create_depth_texture(&mut self) {
        let depth_format = Format::D32_SFLOAT;

        self.depth_texture = Some(Texture::new(TextureDesc {
            width: self.resolution.width,
            height: self.resolution.height,
            depth: 1,
            mip_levels: 1,
            array_layers: 1,
            format: depth_format,
            usage: TextureUsage::DEPTH_STENCIL_ATTACHMENT | TextureUsage::SAMPLED,
            sample_count: SampleCount::X1,
            dimensions: TextureDimensions::D2,
            sharing_mode: SharingMode::Exclusive,
            queue_family_indices: vec![],
        }));

        if let Some(ref texture) = self.depth_texture {
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
            self.depth_view = Some(texture.create_view(desc));
        }
    }

    /// Get aspect ratio
    pub fn aspect_ratio(&self) -> f32 {
        self.resolution.width as f32 / self.resolution.height as f32
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

    /// Get the current camera position (placeholder — always origin)
    pub fn camera_position(&self) -> Vec3 {
        Vec3::ZERO
    }
}
