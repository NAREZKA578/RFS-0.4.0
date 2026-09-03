//! Shader abstraction.

use super::backend::BackendType;

/// Shader stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShaderStage {
    /// Vertex shader.
    Vertex,
    /// Fragment (pixel) shader.
    Fragment,
    /// Geometry shader.
    Geometry,
    /// Compute shader.
    Compute,
    /// Tessellation control shader.
    TessControl,
    /// Tessellation evaluation shader.
    TessEvaluation,
}

impl ShaderStage {
    /// Returns the name of the shader stage.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Vertex => "vertex",
            Self::Fragment => "fragment",
            Self::Geometry => "geometry",
            Self::Compute => "compute",
            Self::TessControl => "tess_control",
            Self::TessEvaluation => "tess_evaluation",
        }
    }
}

/// Shader language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShaderLanguage {
    /// GLSL (OpenGL Shader Language).
    Glsl,
    /// HLSL (High Level Shader Language).
    Hlsl,
    /// SPIR-V (Standard Portable Representation).
    Spirv,
}

/// Shader source, which may differ per backend.
///
/// The same RHI shader is compiled by different backends: OpenGL (via `glCompileShader`)
/// requires GLSL `#version 330` with named uniforms, whereas Vulkan (via naga) requires
/// `#version 450` with `layout(push_constant)` blocks and `layout(set, binding)` samplers.
/// These dialects are mutually incompatible, so a single string cannot serve both.
/// `Single` is used when one source works everywhere (e.g. precompiled SPIR-V); `Dual`
/// lets renderers ship an OpenGL source and a Vulkan source, with resolution happening
/// at the backend boundary via [`ShaderSource::effective_source`].
#[derive(Debug, Clone)]
pub enum ShaderSourceVariant {
    /// One source used for every backend.
    Single(String),
    /// Different sources for OpenGL and Vulkan backends.
    Dual {
        /// OpenGL (`#version 330`) source, used by the OpenGL backend.
        gl: String,
        /// Vulkan (`#version 450`) source, used by the Vulkan backend.
        vk: String,
    },
}

/// Shader source.
#[derive(Debug, Clone)]
pub struct ShaderSource {
    /// Shader stage.
    pub stage: ShaderStage,
    /// Shader source code.
    pub source: ShaderSourceVariant,
    /// Shader language.
    pub language: ShaderLanguage,
    /// Entry point function name.
    pub entry_point: String,
}

impl ShaderSource {
    pub fn vertex_glsl(source: &str) -> Self {
        Self {
            stage: ShaderStage::Vertex,
            source: ShaderSourceVariant::Single(source.to_string()),
            language: ShaderLanguage::Glsl,
            entry_point: "main".to_string(),
        }
    }

    pub fn fragment_glsl(source: &str) -> Self {
        Self {
            stage: ShaderStage::Fragment,
            source: ShaderSourceVariant::Single(source.to_string()),
            language: ShaderLanguage::Glsl,
            entry_point: "main".to_string(),
        }
    }

    /// Per-backend vertex shader: `gl` (OpenGL `#version 330`) and `vk`
    /// (Vulkan `#version 450`) sources.
    pub fn vertex_glsl_dual(gl: &str, vk: &str) -> Self {
        Self {
            stage: ShaderStage::Vertex,
            source: ShaderSourceVariant::Dual {
                gl: gl.to_string(),
                vk: vk.to_string(),
            },
            language: ShaderLanguage::Glsl,
            entry_point: "main".to_string(),
        }
    }

    /// Per-backend fragment shader: `gl` (OpenGL `#version 330`) and `vk`
    /// (Vulkan `#version 450`) sources.
    pub fn fragment_glsl_dual(gl: &str, vk: &str) -> Self {
        Self {
            stage: ShaderStage::Fragment,
            source: ShaderSourceVariant::Dual {
                gl: gl.to_string(),
                vk: vk.to_string(),
            },
            language: ShaderLanguage::Glsl,
            entry_point: "main".to_string(),
        }
    }

    pub fn vertex_hlsl(source: &str, entry: &str) -> Self {
        Self {
            stage: ShaderStage::Vertex,
            source: ShaderSourceVariant::Single(source.to_string()),
            language: ShaderLanguage::Hlsl,
            entry_point: entry.to_string(),
        }
    }

    pub fn fragment_hlsl(source: &str, entry: &str) -> Self {
        Self {
            stage: ShaderStage::Fragment,
            source: ShaderSourceVariant::Single(source.to_string()),
            language: ShaderLanguage::Hlsl,
            entry_point: entry.to_string(),
        }
    }

    /// Returns the effective shader source for the given backend.
    ///
    /// For `Single` sources the same string is returned for every backend. For `Dual`
    /// sources the backend-specific string is returned. DirectX backends fall back to
    /// the OpenGL source (they are not yet naga-compiled); this keeps behaviour safe.
    pub fn effective_source(&self, backend: BackendType) -> &str {
        match (&self.source, backend) {
            (ShaderSourceVariant::Single(s), _) => s,
            (ShaderSourceVariant::Dual { gl: _, vk }, BackendType::Vulkan) => vk,
            (ShaderSourceVariant::Dual { gl, .. }, _) => gl,
        }
    }
}

/// Shader descriptor.
#[derive(Debug, Clone)]
pub struct ShaderDescriptor {
    /// Shader source.
    pub source: ShaderSource,
    /// Shader stage.
    pub stage: ShaderStage,
    /// Whether to optimize the shader.
    pub optimize: bool,
}

impl ShaderDescriptor {
    pub fn from_source(source: ShaderSource) -> Self {
        Self {
            stage: source.stage,
            source,
            optimize: false,
        }
    }

    pub fn vertex(source: &str, language: ShaderLanguage) -> Self {
        Self {
            source: ShaderSource {
                stage: ShaderStage::Vertex,
                source: ShaderSourceVariant::Single(source.to_string()),
                language,
                entry_point: "main".to_string(),
            },
            stage: ShaderStage::Vertex,
            optimize: false,
        }
    }

    pub fn fragment(source: &str, language: ShaderLanguage) -> Self {
        Self {
            source: ShaderSource {
                stage: ShaderStage::Fragment,
                source: ShaderSourceVariant::Single(source.to_string()),
                language,
                entry_point: "main".to_string(),
            },
            stage: ShaderStage::Fragment,
            optimize: false,
        }
    }
}

/// Shader trait — implemented by each backend.
pub trait Shader: Send + Sync + std::fmt::Debug {
    /// Returns the shader stage.
    fn stage(&self) -> ShaderStage;

    /// Returns the shader language.
    fn language(&self) -> ShaderLanguage;

    /// Returns the entry point name.
    fn entry_point(&self) -> &str;

    /// Returns the backend-specific handle.
    fn native_handle(&self) -> u64;
}
