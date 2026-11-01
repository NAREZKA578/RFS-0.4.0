//! Bloom Effect
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//!
//! Honest shell (Bug №204). What used to be here is a subsystem that only
//! *looked* assembled: real target textures created by `initialize`, literal
//! `None` pipelines, placeholder bright-pass/blur/composite functions whose
//! entire body was comments plus calls into the silent encoder stubs
//! (`bind_texture`, `copy_texture`), and a pass-through copy pretending the
//! blur chain ran. Nothing was ever rendered, and the targets nothing read
//! were allocations without a consumer.
//!
//! The shell now states its own contract instead of imitating work: config,
//! builder and the enable knob are real; every stage that would have rendered
//! or prepared targets refuses, so the unbuilt subsystem cannot be mistaken
//! for a working one. Building the real bloom (pipelines, fullscreen quads,
//! its own shaders) is a separate move that waits on the owner's shader
//! decision — see the postprocess module docs.

use super::BloomConfig;
use crate::render::core::RenderContext;
use crate::rhi::{CommandEncoder, Device, Format, RhiError, RhiResult, TextureView};
use std::sync::Arc;

/// Bloom effect
pub struct BloomEffect {
    config: BloomConfig,
    enabled: bool,
}

impl BloomEffect {
    pub fn new(config: BloomConfig) -> Self {
        Self { config, enabled: true }
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

    /// Prepare the effect's targets — **refused** (Bug №204).
    ///
    /// The former targets (bright pass, two blur ping-pong buffers, output)
    /// were read by no consumer: their only writers were the placeholder
    /// passes and the pass-through copy, both removed as part of the same
    /// honest-form move. Preparing targets nothing reads or writes is not
    /// done until the pipelines exist.
    pub fn initialize(
        &mut self,
        _device: &Arc<Device>,
        _context: &RenderContext,
        _input_format: Format,
    ) -> RhiResult<()> {
        Err(RhiError::NotSupported(
            "bloom is not built: the effect has no pipeline stage (Bug №204); \
             refusing to prepare targets nothing would read or write"
                .into(),
        ))
    }

    /// Apply the bloom effect — **refused** while enabled (Bug №204).
    ///
    /// A disabled effect is an honest no-op. An enabled one refuses: there is
    /// no pipeline to run, and the former pass-through copy hid exactly that.
    pub fn apply(
        &mut self,
        _encoder: &mut CommandEncoder,
        _context: &RenderContext,
        _input: &TextureView,
        _output: &TextureView,
    ) -> RhiResult<()> {
        if !self.enabled {
            return Ok(());
        }
        Err(RhiError::NotSupported(
            "bloom is not built: the effect has no pipeline stage (Bug №204); \
             a frame cannot be bloom-processed, and none was faked either"
                .into(),
        ))
    }

    /// Resize the effect.
    pub fn resize(
        &mut self,
        device: &Arc<Device>,
        context: &RenderContext,
        input_format: Format,
    ) -> RhiResult<()> {
        self.initialize(device, context, input_format)
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
