//! Post-Processing Manager
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//!
//! Honest shell (Bug №204). The manager used to look like a pipeline: it
//! chained five effects, each of which "processed" the frame by copying it
//! through the silent encoder stubs. Nothing rendered. The chain remains —
//! the one piece worth keeping, because the execution order is a real design
//! fact — but every stage now either runs for real or refuses, and the
//! manager propagates those refusals instead of hiding them. Building the
//! subsystem for real (pipelines, shaders, quads) is a separate move that
//! waits on the owner's shader decision — see the postprocess module docs.

use super::{
    BloomConfig, BloomEffect, DepthOfFieldConfig, DepthOfFieldEffect, FXAAEffect, HDRConfig,
    HDREffect, MotionBlurConfig, MotionBlurEffect,
};
use crate::render::core::RenderContext;
use crate::render::scene::Scene;
use crate::rhi::{CommandEncoder, Device, Format, RhiError, RhiResult, TextureView};
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

    /// Prepare the effects — refuses as soon as an unbuilt effect is reached
    /// (Bug №204); the first refusal is propagated, not swallowed.
    pub fn initialize(
        &mut self,
        context: &RenderContext,
        input_format: Format,
    ) -> RhiResult<()> {
        self.bloom.initialize(&self.device, context, input_format)?;
        self.motion_blur
            .initialize(&self.device, context, input_format)?;
        self.depth_of_field
            .initialize(&self.device, context, input_format)?;
        self.hdr.initialize(&self.device, context, input_format)?;
        self.fxaa.initialize(&self.device, context, input_format)?;
        Ok(())
    }

    /// Run the post-processing chain — **refused** while any enabled effect
    /// is unbuilt (Bug №204).
    ///
    /// A disabled manager is an honest no-op. An enabled one runs the chain,
    /// and every enabled effect in it refuses until pipelines exist; the
    /// manager passes the refusal up instead of dropping the frame silently.
    /// With every effect disabled the chain degenerates to delivering the
    /// frame: when input and output are the same view that is a no-op, and a
    /// real GPU copy between distinct views does not exist yet, so the
    /// encoder's refusal travels through unchanged.
    pub fn apply(
        &mut self,
        encoder: &mut CommandEncoder,
        context: &RenderContext,
        _scene: &Scene,
        input: &TextureView,
        depth: Option<&TextureView>,
        output: &TextureView,
    ) -> RhiResult<()> {
        if !self.enabled {
            return Ok(());
        }

        let mut current = input;
        let mut temp = output;

        // Apply HDR first
        if self.hdr.enabled() {
            self.hdr.apply(encoder, context, current, temp)?;
            std::mem::swap(&mut current, &mut temp);
        }

        // Apply bloom
        if self.bloom.enabled() {
            self.bloom.apply(encoder, context, current, temp)?;
            std::mem::swap(&mut current, &mut temp);
        }

        // Apply depth of field
        if self.depth_of_field.enabled() {
            match depth {
                Some(depth) => {
                    self.depth_of_field
                        .apply(encoder, context, current, depth, temp)?;
                    std::mem::swap(&mut current, &mut temp);
                }
                // An enabled effect that silently skipped instead of running
                // is the same lie a stub was; refuse.
                None => {
                    return Err(RhiError::NotSupported(
                        "depth of field is enabled but no depth view was \
                         given to PostprocessManager::apply (Bug №204)"
                            .into(),
                    ))
                }
            }
        }

        // Apply motion blur
        if self.motion_blur.enabled() {
            self.motion_blur.apply(encoder, context, current, temp)?;
            std::mem::swap(&mut current, &mut temp);
        }

        // Apply FXAA last
        if self.fxaa.enabled() {
            self.fxaa.apply(encoder, context, current, output)?;
        } else if !std::ptr::eq(current, output) {
            encoder.copy_texture(current, output)?;
        }

        Ok(())
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
