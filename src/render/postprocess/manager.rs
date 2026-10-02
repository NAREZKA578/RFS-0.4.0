//! Post-Processing Manager
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//!
//! Manages all post-processing effects and their execution order.

use super::{
    BloomConfig, BloomEffect, DepthOfFieldConfig, DepthOfFieldEffect, FXAAEffect, HDRConfig,
    HDREffect, MotionBlurConfig, MotionBlurEffect,
};
use crate::render::core::RenderContext;
use crate::render::scene::Scene;
use crate::rhi::{CommandEncoder, Device, Format, TextureView};
use std::sync::Arc;

/// Post-processing manager
pub struct PostProcessManager {
    device: Arc<Device>,
    bloom: BloomEffect,
    motion_blur: MotionBlurEffect,
    depth_of_field: DepthOfFieldEffect,
    hdr: HDREffect,
    fxaa: FXAAEffect,
    enabled: bool,
}

impl PostProcessManager {
    pub fn new(device: Arc<Device>) -> Self {
        Self {
            device,
            bloom: BloomEffect::new(BloomConfig::default()),
            motion_blur: MotionBlurEffect::new(MotionBlurConfig::default()),
            depth_of_field: DepthOfFieldEffect::new(DepthOfFieldConfig::default()),
            hdr: HDREffect::new(HDRConfig::default()),
            fxaa: FXAAEffect::new(),
            enabled: true,
        }
    }

    pub fn initialize(&mut self, context: &RenderContext, input_format: Format) {
        self.bloom.initialize(&self.device, context, input_format);
        self.motion_blur
            .initialize(&self.device, context, input_format);
        self.depth_of_field
            .initialize(&self.device, context, input_format);
        self.hdr.initialize(&self.device, context, input_format);
        self.fxaa.initialize(&self.device, context, input_format);
    }

    pub fn apply(
        &mut self,
        encoder: &mut CommandEncoder,
        context: &RenderContext,
        _scene: &Scene,
        input: &TextureView,
        depth: Option<&TextureView>,
        output: &TextureView,
    ) {
        if !self.enabled {
            return;
        }

        let mut current = input;
        let mut temp = output;

        // Apply HDR first
        if self.hdr.enabled() {
            self.hdr.apply(encoder, context, current, temp);
            std::mem::swap(&mut current, &mut temp);
        }

        // Apply bloom
        if self.bloom.enabled() {
            self.bloom.apply(encoder, context, current, temp);
            std::mem::swap(&mut current, &mut temp);
        }

        // Apply depth of field
        if self.depth_of_field.enabled() {
            if let Some(depth) = depth {
                self.depth_of_field
                    .apply(encoder, context, current, depth, temp);
                std::mem::swap(&mut current, &mut temp);
            }
        }

        // Apply motion blur
        if self.motion_blur.enabled() {
            self.motion_blur.apply(encoder, context, current, temp);
            std::mem::swap(&mut current, &mut temp);
        }

        // Apply FXAA last
        if self.fxaa.enabled() {
            self.fxaa.apply(encoder, context, current, output);
        } else if !std::ptr::eq(current, output) {
            encoder.copy_texture(current, output);
        }
    }

    /// Getters for individual effects
    pub fn bloom(&self) -> &BloomEffect {
        &self.bloom
    }

    pub fn bloom_mut(&mut self) -> &mut BloomEffect {
        &mut self.bloom
    }

    pub fn motion_blur(&self) -> &MotionBlurEffect {
        &self.motion_blur
    }

    pub fn motion_blur_mut(&mut self) -> &mut MotionBlurEffect {
        &mut self.motion_blur
    }

    pub fn depth_of_field(&self) -> &DepthOfFieldEffect {
        &self.depth_of_field
    }

    pub fn depth_of_field_mut(&mut self) -> &mut DepthOfFieldEffect {
        &mut self.depth_of_field
    }

    pub fn hdr(&self) -> &HDREffect {
        &self.hdr
    }

    pub fn hdr_mut(&mut self) -> &mut HDREffect {
        &mut self.hdr
    }

    pub fn fxaa(&self) -> &FXAAEffect {
        &self.fxaa
    }

    pub fn fxaa_mut(&mut self) -> &mut FXAAEffect {
        &mut self.fxaa
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }
}
