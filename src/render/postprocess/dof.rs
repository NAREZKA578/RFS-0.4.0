//! Depth of Field Effect
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use super::DepthOfFieldConfig;
use crate::render::core::RenderContext;
use crate::rhi::{
    CommandEncoder, Device, Format, Pipeline, RhiError, RhiResult, Texture, TextureView,
};
use std::sync::Arc;

/// Depth of Field effect
pub struct DepthOfFieldEffect {
    config: DepthOfFieldConfig,
    enabled: bool,
    /// Depth texture
    depth_texture: Option<Texture>,
    depth_view: Option<TextureView>,
    /// Output texture
    output_texture: Option<Texture>,
    output_view: Option<TextureView>,
    /// Pipeline
    pipeline: Option<Pipeline>,
}

impl DepthOfFieldEffect {
    pub fn new(config: DepthOfFieldConfig) -> Self {
        Self {
            config,
            enabled: false, // Disabled by default (performance intensive)
            depth_texture: None,
            depth_view: None,
            output_texture: None,
            output_view: None,
            pipeline: None,
        }
    }

    pub fn config(&self) -> &DepthOfFieldConfig {
        &self.config
    }

    pub fn config_mut(&mut self) -> &mut DepthOfFieldConfig {
        &mut self.config
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    pub fn initialize(
        &mut self,
        device: &Arc<Device>,
        context: &RenderContext,
        _input_format: Format,
    ) -> RhiResult<()> {
        // Bug №651: the size comes from the presentation extent, not from
        // `context.resolution` — see `RenderContext::drawable_extent`.
        let (width, height) = context.drawable_size();
        let depth = device
            .create_texture(
                width,
                height,
                1,
                Format::D32_SFLOAT,
                crate::rhi::TextureUsage::DEPTH_STENCIL_ATTACHMENT | crate::rhi::TextureUsage::SAMPLED,
                1,
            )
            .map_err(|e| RhiError::BackendError(format!("the DoF depth texture: {e}")))?;
        self.depth_view = Some(depth.create_view(Default::default()));
        self.depth_texture = Some(depth);

        let output = device
            .create_texture(
                width,
                height,
                1,
                Format::RGBA32_SFLOAT,
                crate::rhi::TextureUsage::COLOR_ATTACHMENT | crate::rhi::TextureUsage::SAMPLED,
                1,
            )
            .map_err(|e| RhiError::BackendError(format!("the DoF output texture: {e}")))?;
        self.output_view = Some(output.create_view(Default::default()));
        self.output_texture = Some(output);
        Ok(())
    }

    pub fn apply(
        &mut self,
        encoder: &mut CommandEncoder,
        _context: &RenderContext,
        input: &TextureView,
        _depth_texture: &TextureView,
        output: &TextureView,
    ) {
        if !self.enabled {
            return;
        }
        // Placeholder pass-through until the real CoC blur exists.
        if !std::ptr::eq(input, output) {
            encoder.copy_texture(input, output);
        }
    }

    pub fn resize(
        &mut self,
        device: &Arc<Device>,
        context: &RenderContext,
        input_format: Format,
    ) -> RhiResult<()> {
        self.initialize(device, context, input_format)
    }

    pub fn cleanup(&mut self) {
        self.depth_texture = None;
        self.depth_view = None;
        self.output_texture = None;
        self.output_view = None;
        self.pipeline = None;
    }
}

impl Default for DepthOfFieldEffect {
    fn default() -> Self {
        Self::new(DepthOfFieldConfig::default())
    }
}
