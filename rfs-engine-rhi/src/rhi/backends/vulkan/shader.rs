//! Vulkan Shader implementation.
//!
//! Produces a `VkShaderModule` from either:
//!   * hand-authored GLSL (`#version 450`, language `Glsl`) — compiled to SPIR-V
//!     at runtime via `naga` (no external SDK needed); or
//!   * already-compiled SPIR-V bytecode (language `Spirv`) — used verbatim.
//!
//! Because the Vulkan pipeline consumes a conventional descriptor/push-constant
//! layout (see `VulkanPipeline`), Vulkan GLSL sources must be written explicitly
//! for Vulkan: `layout(push_constant)` blocks with matching member order and
//! `layout(set = 0, binding = N) uniform sampler2D` per texture.

use crate::rhi::shader::{Shader as ShaderTrait, ShaderDescriptor, ShaderLanguage, ShaderStage};
use ash::vk;
use std::sync::Arc;

/// Vulkan shader module.
pub struct VulkanShader {
    device: Arc<ash::Device>,
    module: vk::ShaderModule,
    stage: ShaderStage,
    language: ShaderLanguage,
}

impl VulkanShader {
    pub fn new(
        device: &Arc<ash::Device>,
        descriptor: &ShaderDescriptor,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let spir_v: Vec<u8> = match descriptor.source.language {
            ShaderLanguage::Glsl => compile_glsl(
                descriptor
                    .source
                    .effective_source(crate::rhi::backend::BackendType::Vulkan),
                &descriptor.source.entry_point,
                descriptor.source.stage,
            )?,
            ShaderLanguage::Spirv => descriptor
                .source
                .effective_source(crate::rhi::backend::BackendType::Vulkan)
                .as_bytes()
                .to_vec(),
            _ => {
                return Err("VulkanShader: unsupported shader language".into());
            }
        };

        if !spir_v.len().is_multiple_of(4) {
            return Err("Invalid SPIR-V bytecode length".into());
        }

        let code =
            unsafe { std::slice::from_raw_parts(spir_v.as_ptr() as *const u32, spir_v.len() / 4) };

        let create_info = vk::ShaderModuleCreateInfo::default().code(code);

        let module = unsafe { device.create_shader_module(&create_info, None)? };

        Ok(Self {
            device: device.clone(),
            module,
            stage: descriptor.stage,
            language: descriptor.source.language,
        })
    }

    pub fn vk_module(&self) -> vk::ShaderModule {
        self.module
    }
}

/// Compiles a GLSL source string into SPIR-V words using naga.
fn compile_glsl(
    source: &str,
    entry_point: &str,
    stage: ShaderStage,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    use naga::front::glsl::Options;
    use naga::ShaderStage as NagaStage;

    let entry = if entry_point.is_empty() {
        "main"
    } else {
        entry_point
    };

    let naga_stage = match stage {
        ShaderStage::Vertex => NagaStage::Vertex,
        ShaderStage::Fragment => NagaStage::Fragment,
        ShaderStage::Compute => NagaStage::Compute,
        other => {
            return Err(format!(
                "VulkanShader: GLSL stage {:?} not supported by naga front-end",
                other
            )
            .into());
        }
    };

    let options = Options::from(naga_stage);

    let module = naga::front::glsl::Frontend::default()
        .parse(&options, source)
        .map_err(|e| format!("GLSL parse error: {e}"))?;

    let info = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .map_err(|e| format!("GLSL validation error: {e}"))?;

    let spv_options = naga::back::spv::Options::default();
    let pipeline_options = naga::back::spv::PipelineOptions {
        shader_stage: naga_stage,
        entry_point: entry.to_string(),
    };

    let words = naga::back::spv::write_vec(&module, &info, &spv_options, Some(&pipeline_options))
        .map_err(|e| format!("SPIR-V write error: {e}"))?;

    let mut bytes = Vec::with_capacity(words.len() * 4);
    for w in &words {
        bytes.extend_from_slice(&w.to_le_bytes());
    }
    Ok(bytes)
}

impl Drop for VulkanShader {
    fn drop(&mut self) {
        unsafe {
            self.device.destroy_shader_module(self.module, None);
        }
    }
}

impl std::fmt::Debug for VulkanShader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VulkanShader")
            .field("stage", &self.stage)
            .field("language", &self.language)
            .finish()
    }
}

impl ShaderTrait for VulkanShader {
    fn stage(&self) -> ShaderStage {
        self.stage
    }
    fn language(&self) -> ShaderLanguage {
        self.language
    }
    fn entry_point(&self) -> &str {
        "main"
    }
    fn native_handle(&self) -> u64 {
        unsafe { std::mem::transmute::<vk::ShaderModule, u64>(self.module) }
    }
}
