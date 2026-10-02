//! Shader Module
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//! **TODO: Full implementation required**

use serde::{Deserialize, Serialize};

use crate::resource::GpuResource;

/// Shader format
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum ShaderFormat {
    #[default]
    SpirV,
    Dxil,
    Msl,
    Wgsl,
    Glsl,
    GlslEs,
}

/// Shader module description
#[derive(Debug, Clone, Default)]
pub struct ShaderModuleDesc {
    pub code: Vec<u8>,
    pub format: ShaderFormat,
    pub entry_point: Option<String>,
    pub name: Option<String>,
}

/// Shader module
#[derive(Debug, Clone, Default)]
pub struct ShaderModule {
    desc: ShaderModuleDesc,
    pub(crate) backend: Option<GpuResource>,
}

impl ShaderModule {
    pub fn new(desc: ShaderModuleDesc) -> Self {
        Self {
            desc,
            backend: None,
        }
    }

    /// Attaches a native handle produced by the active backend.
    pub(crate) fn set_backend(&mut self, backend: GpuResource) {
        self.backend = Some(backend);
    }

    /// Returns `true` when the module has a compiled native shader handle.
    pub fn has_gpu_backing(&self) -> bool {
        self.backend.is_some()
    }

    pub(crate) fn backend(&self) -> Option<GpuResource> {
        self.backend
    }

    pub fn desc(&self) -> &ShaderModuleDesc {
        &self.desc
    }

    pub fn format(&self) -> ShaderFormat {
        self.desc.format
    }

    pub fn name(&self) -> Option<&str> {
        self.desc.name.as_deref()
    }

    pub fn entry_point(&self) -> Option<&str> {
        self.desc.entry_point.as_deref()
    }

    pub fn code_size(&self) -> usize {
        self.desc.code.len()
    }

    pub fn has_code(&self) -> bool {
        !self.desc.code.is_empty()
    }
}
