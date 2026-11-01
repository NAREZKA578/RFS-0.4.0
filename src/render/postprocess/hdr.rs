//! HDR Effect
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//!
//! Honest shell (Bug №204): the former placeholder "tone-mapped" by copying
//! frames through the silent `copy_texture` encoder stub — twice when the
//! input and output views were the same object, once otherwise. No exposure,
//! gamma or ACES stage existed anywhere, and the output texture it prepared
//! was read by nobody (the manager passes the caller's views, not the
//! effect's own). The effect keeps its configuration and enable knob and
//! refuses to apply while enabled.

use super::HDRConfig;
use crate::render::core::RenderContext;
use crate::rhi::{CommandEncoder, Device, Format, RhiError, RhiResult, TextureView};
use std::sync::Arc;

/// HDR effect
pub struct HDREffect {
    config: HDRConfig,
    enabled: bool,
}

impl HDREffect {
    pub fn new(config: HDRConfig) -> Self {
        Self {
            config,
            enabled: true,
        }
    }

    pub fn config(&self) -> &HDRConfig {
        &self.config
    }

    pub fn config_mut(&mut self) -> &mut HDRConfig {
        &mut self.config
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Prepare the effect's target — **refused** (Bug №204).
    ///
    /// The former output target was never in the manager's frame path — the
    /// manager passes the caller's views — and its only writer was the fake
    /// pass-through copy. Preparing a target nothing reads is not done.
    pub fn initialize(
        &mut self,
        _device: &Arc<Device>,
        _context: &RenderContext,
        _input_format: Format,
    ) -> RhiResult<()> {
        Err(RhiError::NotSupported(
            "HDR is not built: the effect has no pipeline stage (Bug №204); \
             refusing to prepare a target nothing would read"
                .into(),
        ))
    }

    /// Apply the effect — **refused** while enabled (Bug №204).
    ///
    /// A disabled effect is an honest no-op. An enabled one refuses: there is
    /// no pipeline to run, and the former pass-through copies hid exactly that.
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
            "HDR is not built: the effect has no pipeline stage (Bug №204); \
             a frame cannot be tone-mapped, and none was faked either"
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

impl Default for HDREffect {
    fn default() -> Self {
        Self::new(HDRConfig::default())
    }
}
