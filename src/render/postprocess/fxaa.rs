//! FXAA Effect
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//!
//! Fast Approximate Anti-Aliasing (FXAA) for post-process anti-aliasing.
//!
//! Honest shell (Bug №204): the former placeholder copied frames through the
//! silent `copy_texture` encoder stub and called it anti-aliasing — nothing
//! was ever filtered, and the target texture it created was read by no one.
//! The effect keeps its enable knob and refuses to apply while enabled.

use crate::render::core::RenderContext;
use crate::rhi::{CommandEncoder, Device, Format, RhiError, RhiResult, TextureView};
use std::sync::Arc;

/// FXAA effect
pub struct FXAAEffect {
    enabled: bool,
}

impl FXAAEffect {
    pub fn new() -> Self {
        Self { enabled: true }
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Prepare the effect's target — **refused** (Bug №204).
    ///
    /// The former target was written only by the fake pass-through copy and
    /// read by nobody. Refusing keeps the shell from spending device memory
    /// on a texture that serves nothing.
    pub fn initialize(
        &mut self,
        _device: &Arc<Device>,
        _context: &RenderContext,
        _input_format: Format,
    ) -> RhiResult<()> {
        Err(RhiError::NotSupported(
            "FXAA is not built: the effect has no pipeline stage (Bug №204); \
             refusing to prepare a target nothing would read"
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
        _output: &TextureView,
    ) -> RhiResult<()> {
        if !self.enabled {
            return Ok(());
        }
        Err(RhiError::NotSupported(
            "FXAA is not built: the effect has no pipeline stage (Bug №204); \
             a frame cannot be anti-aliased, and none was faked either"
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

impl Default for FXAAEffect {
    fn default() -> Self {
        Self::new()
    }
}
