//! Depth of Field Effect
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//!
//! Honest shell (Bug №204): the former placeholder "applied" DoF by copying
//! frames through the silent `copy_texture` encoder stub — no circle-of-
//! confusion blur existed, and the depth/output textures it prepared were
//! read by nothing. The effect keeps its enable knob (disabled by default,
//! the original intent) and refuses to apply while enabled.

use super::DepthOfFieldConfig;
use crate::render::core::RenderContext;
use crate::rhi::{CommandEncoder, Device, Format, RhiError, RhiResult, TextureView};
use std::sync::Arc;

/// Depth of Field effect
pub struct DepthOfFieldEffect {
    config: DepthOfFieldConfig,
    enabled: bool,
}

impl DepthOfFieldEffect {
    pub fn new(config: DepthOfFieldConfig) -> Self {
        Self {
            config,
            enabled: false, // Disabled by default (performance intensive)
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

    /// Prepare the effect's targets — **refused** (Bug №204).
    ///
    /// The former depth and output targets were written only by the fake
    /// pass-through copy and read by nobody. Refusing keeps the shell from
    /// spending device memory on textures that serve nothing.
    pub fn initialize(
        &mut self,
        _device: &Arc<Device>,
        _context: &RenderContext,
        _input_format: Format,
    ) -> RhiResult<()> {
        Err(RhiError::NotSupported(
            "depth of field is not built: the effect has no pipeline stage \
             (Bug №204); refusing to prepare targets nothing would read"
                .into(),
        ))
    }

    /// Apply the effect — **refused** while enabled (Bug №204).
    ///
    /// A disabled effect is an honest no-op. An enabled one refuses: there is
    /// no pipeline to run, and the former pass-through copy hid exactly that.
    pub fn apply(
        &mut self,
        _encoder: &mut CommandEncoder,
        _context: &RenderContext,
        _input: &TextureView,
        _depth: &TextureView,
        _output: &TextureView,
    ) -> RhiResult<()> {
        if !self.enabled {
            return Ok(());
        }
        Err(RhiError::NotSupported(
            "depth of field is not built: the effect has no pipeline stage \
             (Bug №204); a frame cannot be DoF-processed, and none was faked either"
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

impl Default for DepthOfFieldEffect {
    fn default() -> Self {
        Self::new(DepthOfFieldConfig::default())
    }
}
