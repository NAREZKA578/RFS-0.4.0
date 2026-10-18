//! UI Shader Manager
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//!
//! Loads pre-compiled SPIR-V and turns it into real shader modules.
//!
//! SPIR-V arrives as bytes rather than being read from a path. `rhi` is its own
//! crate and must not know where assets live on disk: the caller embeds the
//! `.spv` with `include_bytes!` and hands the bytes over. That also keeps the
//! dependency pointing one way — `render` already depends on `rhi`, not the
//! reverse.
//!
//! Both entry points are named `vs_main` / `fs_main`, matching the shaders
//! already committed under `Tool/client_tests/assets/shaders`.

use std::collections::HashMap;
use std::sync::Arc;

use crate::shader::module::{ShaderFormat, ShaderModule, ShaderModuleDesc};
use crate::Device;

use super::{UiError, UiResult};

/// Which UI shader a module is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UiShaderType {
    /// Untextured vertex-coloured geometry: panels, buttons, bars, lines.
    Geometry,
    /// Text glyph quads. Reserved; the font atlas is a separate task.
    Text,
}

impl UiShaderType {
    /// Entry point name every UI shader is expected to export.
    pub const VERTEX_ENTRY: &'static str = "vs_main";
    pub const FRAGMENT_ENTRY: &'static str = "fs_main";

    /// Source file this shader type is authored in.
    pub fn source_filename(&self) -> &'static str {
        match self {
            UiShaderType::Geometry => "ui_geom.wgsl",
            UiShaderType::Text => "ui_text.wgsl",
        }
    }
}

/// Where a loaded module came from, kept for diagnostics.
///
/// A module that failed to build leaves no entry here, so an empty table is the
/// signal that something is missing rather than that shaders are simply absent.
#[derive(Debug, Clone)]
pub struct UiShaderSource {
    pub shader_type: UiShaderType,
    /// Length of the SPIR-V blob that was handed in.
    pub spirv_len: usize,
}

/// Owns the UI shader modules and the device they were built on.
pub struct UiShaderManager {
    device: Arc<Device>,
    modules: HashMap<UiShaderType, ShaderModule>,
    sources: HashMap<UiShaderType, UiShaderSource>,
}

impl UiShaderManager {
    pub fn new(device: Arc<Device>) -> Self {
        Self {
            device,
            modules: HashMap::new(),
            sources: HashMap::new(),
        }
    }

    /// Build one shader module from SPIR-V, replacing any previous one.
    ///
    /// Reports rather than swallows a module that came back without a GPU
    /// handle: the pipeline built from it would fail later, at draw time, with
    /// an error pointing at the wrong place.
    pub fn load(&mut self, shader_type: UiShaderType, spirv: &[u8]) -> UiResult<()> {
        if spirv.is_empty() {
            return Err(UiError::ShaderCompilationFailed(format!(
                "{}: SPIR-V is empty",
                shader_type.source_filename()
            )));
        }
// SPIR-V is five 32-bit words: magic, version, generator, bound, schema.
//        // A blob shorter than that cannot be a module, and handing it to the
// driver produces an error that does not name the real problem.
        let word_aligned = spirv.len().is_multiple_of(4);
        if !word_aligned || spirv.len() < 20 {
            return Err(UiError::ShaderCompilationFailed(format!(
                "{}: SPIR-V is {} bytes, which is neither a whole number of words nor at least a header",
                shader_type.source_filename(),
                spirv.len()
            )));
        }

        let module = self.device.create_shader_module(&ShaderModuleDesc {
            code: spirv.to_vec(),
            format: ShaderFormat::SpirV,
            entry_point: Some(UiShaderType::VERTEX_ENTRY.to_string()),
            name: Some(shader_type.source_filename().to_string()),
        });

        if !module.has_gpu_backing() {
            return Err(UiError::ShaderCompilationFailed(format!(
                "{}: the shader module was not created on the device",
                shader_type.source_filename()
            )));
        }

        self.sources.insert(
            shader_type,
            UiShaderSource {
                shader_type,
                spirv_len: spirv.len(),
            },
        );
        self.modules.insert(shader_type, module);
        Ok(())
    }

    /// Load every shader the renderer needs.
    ///
    /// `geometry` is required. `text` is optional until the font atlas lands:
    /// a missing text module must not stop panels and buttons from drawing, so
    /// its absence is reported through `is_loaded` instead of failing here.
    pub fn load_geometry(&mut self, spirv: &[u8]) -> UiResult<()> {
        self.load(UiShaderType::Geometry, spirv)
    }

    pub fn load_text(&mut self, spirv: &[u8]) -> UiResult<()> {
        self.load(UiShaderType::Text, spirv)
    }

    /// The module for `shader_type`, or an error naming what is missing.
    ///
    /// The message lists what *did* load, not only what did not. Without that
    /// the same error appears whether the manager has no shaders at all or every
    /// one but this, and the two want different fixes.
    pub fn module(&self, shader_type: UiShaderType) -> UiResult<&ShaderModule> {
        self.modules.get(&shader_type).ok_or_else(|| {
            UiError::ShaderCompilationFailed(format!(
                "{} has not been loaded into the UI shader manager; loaded: {}",
                shader_type.source_filename(),
                describe_loaded(&self.sources),
            ))
        })
    }

    pub fn is_loaded(&self, shader_type: UiShaderType) -> bool {
        self.modules.contains_key(&shader_type)
    }

    /// Provenance for a loaded shader, if it was recorded.
    pub fn source(&self, shader_type: UiShaderType) -> Option<&UiShaderSource> {
        self.sources.get(&shader_type)
    }

    pub fn loaded_count(&self) -> usize {
        self.modules.len()
    }
}

/// Names the shaders that did load, so an error can say what was expected
/// alongside what was missing.
///
/// Sorted, because a `HashMap` iterates in an unspecified order and an error
/// message that reorders itself between runs is hard to compare against a log.
fn describe_loaded(sources: &HashMap<UiShaderType, UiShaderSource>) -> String {
    if sources.is_empty() {
        return "none".to_string();
    }
    let mut names: Vec<String> = sources
        .values()
        .map(|s| {
            format!(
                "{} ({} bytes)",
                s.shader_type.source_filename(),
                s.spirv_len
            )
        })
        .collect();
    names.sort();
    names.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sourced(shader_type: UiShaderType, spirv_len: usize) -> UiShaderSource {
        UiShaderSource {
            shader_type,
            spirv_len,
        }
    }

    #[test]
    fn an_empty_source_table_says_none() {
        assert_eq!(describe_loaded(&HashMap::new()), "none");
    }

    #[test]
    fn loaded_sources_are_named_with_their_size() {
        let mut sources = HashMap::new();
        sources.insert(
            UiShaderType::Geometry,
            sourced(UiShaderType::Geometry, 2048),
        );
        sources.insert(UiShaderType::Text, sourced(UiShaderType::Text, 512));
        assert_eq!(
            describe_loaded(&sources),
            "ui_geom.wgsl (2048 bytes), ui_text.wgsl (512 bytes)"
        );
    }
}