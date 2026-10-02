//! Bloom Effect
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//!
//! Bloom effect extracts bright areas and blurs them to create a glow effect.

use super::BloomConfig;
use crate::render::core::RenderContext;
use crate::rhi::{CommandEncoder, Device, Format, Pipeline, Texture, TextureView};
use std::sync::Arc;

/// Bloom effect
pub struct BloomEffect {
    config: BloomConfig,
    enabled: bool,
    /// Bright pass pipeline
    bright_pass_pipeline: Option<Pipeline>,
    /// Blur pipelines (horizontal and vertical)
    blur_pipelines: [Option<Pipeline>; 2],
    /// Composite pipeline
    composite_pipeline: Option<Pipeline>,
    /// Bright pass texture
    bright_pass_texture: Option<Texture>,
    bright_pass_view: Option<TextureView>,
    /// Blur textures (ping-pong buffers)
    blur_textures: [Option<Texture>; 2],
    blur_views: [Option<TextureView>; 2],
    /// Output texture
    output_texture: Option<Texture>,
    output_view: Option<TextureView>,
}

impl BloomEffect {
    pub fn new(config: BloomConfig) -> Self {
        Self {
            config,
            enabled: true,
            bright_pass_pipeline: None,
            blur_pipelines: [None, None],
            composite_pipeline: None,
            bright_pass_texture: None,
            bright_pass_view: None,
            blur_textures: [None, None],
            blur_views: [None, None],
            output_texture: None,
            output_view: None,
        }
    }

    pub fn config(&self) -> &BloomConfig {
        &self.config
    }

    pub fn config_mut(&mut self) -> &mut BloomConfig {
        &mut self.config
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Initialize the effect
    pub fn initialize(&mut self, device: &Arc<Device>, context: &RenderContext, _input_format: Format) {
        let width = context.resolution.width;
        let height = context.resolution.height;

        // Create bright pass texture
        self.bright_pass_texture = Some(device.create_texture(
            width,
            height,
            1,
            Format::RGBA32_SFLOAT,
            crate::rhi::TextureUsage::COLOR_ATTACHMENT | crate::rhi::TextureUsage::SAMPLED,
            1,
        ));
        self.bright_pass_view = Some(
            self.bright_pass_texture
                .as_ref()
                .unwrap()
                .create_view(Default::default()),
        );

        // Create blur textures (ping-pong buffers)
        for i in 0..2 {
            self.blur_textures[i] = Some(device.create_texture(
                width,
                height,
                1,
                Format::RGBA32_SFLOAT,
                crate::rhi::TextureUsage::COLOR_ATTACHMENT | crate::rhi::TextureUsage::SAMPLED,
                1,
            ));
            self.blur_views[i] = Some(
                self.blur_textures[i]
                    .as_ref()
                    .unwrap()
                    .create_view(Default::default()),
            );
        }

        // Create output texture
        self.output_texture = Some(device.create_texture(
            width,
            height,
            1,
            Format::RGBA32_SFLOAT,
            crate::rhi::TextureUsage::COLOR_ATTACHMENT | crate::rhi::TextureUsage::SAMPLED,
            1,
        ));
        self.output_view = Some(
            self.output_texture
                .as_ref()
                .unwrap()
                .create_view(Default::default()),
        );

        // Create pipelines
        // These would be created with appropriate shaders
        // For now, we'll leave them as None
    }

    /// Apply the bloom effect
    pub fn apply(
        &mut self,
        encoder: &mut CommandEncoder,
        context: &RenderContext,
        input: &TextureView,
        output: &TextureView,
    ) {
        if !self.enabled {
            return;
        }

        let width = context.resolution.width;
        let height = context.resolution.height;

        // Step 1: Bright pass
        self.bright_pass(encoder, input, width, height);

        // Step 2: seed blur chain from the bright-pass output (not from a
        // stale blur target), then blur.
        self.seed_blur_from_bright(encoder);
        for _ in 0..self.config.blur_passes {
            self.blur_pass(encoder, width, height);
        }

        // Step 3: Composite (placeholder still forwards the image).
        self.composite_pass(encoder, input, width, height);
        if !std::ptr::eq(input, output) {
            encoder.copy_texture(input, output);
        }
    }

    /// Seed blur chain from the bright-pass output so the first blur reads
    /// thresholded pixels (not a stale blur target).
    fn seed_blur_from_bright(&mut self, encoder: &mut CommandEncoder) {
        if let (Some(src), Some(dst)) = (&self.bright_pass_view, &self.blur_views[0]) {
            if !std::ptr::eq(src, dst) {
                encoder.copy_texture(src, dst);
            }
        }
    }

    /// Bright pass: extract bright pixels
    fn bright_pass(
        &mut self,
        encoder: &mut CommandEncoder,
        input_texture: &TextureView,
        _width: u32,
        _height: u32,
    ) {
        // Bind bright pass pipeline
        if let Some(pipeline) = &self.bright_pass_pipeline {
            encoder.bind_pipeline(pipeline);
        }

        // Bind input texture
        encoder.bind_texture(input_texture, 0);

        // Set threshold uniform
        // This would set the threshold value in the shader

        // Render to bright pass texture
        // This would use a full-screen quad

        // For now, this is a placeholder
    }

    /// Blur pass: apply Gaussian blur
    fn blur_pass(&mut self, encoder: &mut CommandEncoder, width: u32, height: u32) {
        // Blur horizontally
        self.blur(encoder, 0, true, width, height);

        // Blur vertically
        self.blur(encoder, 1, false, width, height);
    }

    /// Blur in one direction
    fn blur(
        &mut self,
        encoder: &mut CommandEncoder,
        pass: usize,
        horizontal: bool,
        _width: u32,
        _height: u32,
    ) {
        let input_index = pass % 2;
        let output_index = (pass + 1) % 2;

        if let (Some(input_view), Some(_output_texture), Some(_output_view)) = (
            &self.blur_views[input_index],
            &self.blur_textures[output_index],
            &self.blur_views[output_index],
        ) {
            // Bind blur pipeline
            let pipeline_index = if horizontal { 0 } else { 1 };
            if let Some(pipeline) = &self.blur_pipelines[pipeline_index] {
                encoder.bind_pipeline(pipeline);
            }

            // Bind input texture
            encoder.bind_texture(input_view, 0);

            // Set blur parameters
            // This would set the blur radius and direction in the shader

            // Render to output texture
            // This would use a full-screen quad

            // For now, this is a placeholder
        }
    }

    /// Composite: combine original and bloomed image
    fn composite_pass(
        &mut self,
        encoder: &mut CommandEncoder,
        input_texture: &TextureView,
        _width: u32,
        _height: u32,
    ) {
        // Prefer the blurred result; fall back to bright-pass only when blur
        // targets are missing.
        let bloom_view = self.blur_views[1]
            .as_ref()
            .or(self.blur_views[0].as_ref())
            .or(self.bright_pass_view.as_ref());
        if let (Some(bloomed), Some(_output_view)) = (bloom_view, &self.output_view) {
            // Bind composite pipeline
            if let Some(pipeline) = &self.composite_pipeline {
                encoder.bind_pipeline(pipeline);
            }

            // Bind textures
            encoder.bind_texture(input_texture, 0); // Original
            encoder.bind_texture(bloomed, 1); // Blurred bloom

            // Set intensity uniform
            // This would set the bloom intensity in the shader

            // Render to output texture
            // This would use a full-screen quad

            // For now, this is a placeholder
        }
    }

    /// Resize the effect
    pub fn resize(&mut self, device: &Arc<Device>, context: &RenderContext, input_format: Format) {
        // Recreate all textures with new dimensions
        self.initialize(device, context, input_format);
    }

    /// Clean up resources
    pub fn cleanup(&mut self) {
        self.bright_pass_pipeline = None;
        self.blur_pipelines = [None, None];
        self.composite_pipeline = None;
        self.bright_pass_texture = None;
        self.bright_pass_view = None;
        self.blur_textures = [None, None];
        self.blur_views = [None, None];
        self.output_texture = None;
        self.output_view = None;
    }
}

impl Default for BloomEffect {
    fn default() -> Self {
        Self::new(BloomConfig::default())
    }
}

/// Bloom effect builder
pub struct BloomEffectBuilder {
    effect: BloomEffect,
}

impl Default for BloomEffectBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl BloomEffectBuilder {
    pub fn new() -> Self {
        Self {
            effect: BloomEffect::default(),
        }
    }

    pub fn with_config(mut self, config: BloomConfig) -> Self {
        self.effect.config = config;
        self
    }

    pub fn with_threshold(mut self, threshold: f32) -> Self {
        self.effect.config.threshold = threshold;
        self
    }

    pub fn with_intensity(mut self, intensity: f32) -> Self {
        self.effect.config.intensity = intensity;
        self
    }

    pub fn with_blur_passes(mut self, passes: u32) -> Self {
        self.effect.config.blur_passes = passes;
        self
    }

    pub fn with_blur_radius(mut self, radius: f32) -> Self {
        self.effect.config.blur_radius = radius;
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.effect.enabled = enabled;
        self
    }

    pub fn build(self) -> BloomEffect {
        self.effect
    }
}
