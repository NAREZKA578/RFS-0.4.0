//! Shader Compiler
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//! **TODO: Full implementation required**

use super::module::{ShaderModule, ShaderModuleDesc};
use super::reflection::ShaderReflection;
use crate::types::ShaderStage;

/// Shader compiler trait
pub trait ShaderCompiler: Send + Sync {
    fn compile(&self, desc: &ShaderModuleDesc) -> Result<ShaderModule, String>;
    fn compile_to_spirv(&self, glsl: &str, stage: ShaderStage) -> Result<Vec<u8>, String>;
    fn compile_to_dxil(&self, hlsl: &str, stage: ShaderStage) -> Result<Vec<u8>, String>;
    fn compile_to_glsl(&self, spirv: &[u8], stage: ShaderStage) -> Result<String, String>;
    fn reflect(&self, code: &[u8], format: ShaderFormat) -> Result<ShaderReflection, String>;
}

/// Compile options
#[derive(Debug, Clone, Default)]
pub struct CompileOptions {
    pub optimization_level: OptimizationLevel,
    pub generate_debug_info: bool,
    pub strip_debug_info: bool,
    pub target_env: ShaderTargetEnv,
}

/// Optimization level
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum OptimizationLevel {
    #[default]
    None,
    Performance,
    Size,
}

/// Shader target environment
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ShaderTargetEnv {
    #[default]
    Vulkan,
    DirectX,
    OpenGL,
}

/// Shader format (re-export)
pub use super::module::ShaderFormat;
