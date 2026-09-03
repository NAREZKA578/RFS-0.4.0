//! OpenGL Shader implementation.

use crate::rhi::shader::{Shader as ShaderTrait, ShaderDescriptor, ShaderLanguage, ShaderStage};
use glow::HasContext;
use std::sync::Arc;

/// OpenGL shader.
#[derive(Debug)]
pub struct OpenGLShader {
    gl: Arc<glow::Context>,
    stage: ShaderStage,
    language: ShaderLanguage,
    id: glow::NativeShader,
}

impl OpenGLShader {
    /// Creates a new OpenGL shader.
    pub fn new(
        gl: &Arc<glow::Context>,
        descriptor: &ShaderDescriptor,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let gl_stage = match descriptor.stage {
            ShaderStage::Vertex => glow::VERTEX_SHADER,
            ShaderStage::Fragment => glow::FRAGMENT_SHADER,
            ShaderStage::Geometry => glow::GEOMETRY_SHADER,
            ShaderStage::Compute => glow::COMPUTE_SHADER,
            _ => return Err("Unsupported shader stage".into()),
        };

        let id = unsafe { gl.create_shader(gl_stage)? };
        let src = descriptor
            .source
            .effective_source(crate::rhi::backend::BackendType::OpenGL)
            .to_owned();
        unsafe {
            gl.shader_source(id, &src);
            gl.compile_shader(id);
            if !gl.get_shader_compile_status(id) {
                let log = gl.get_shader_info_log(id);
                gl.delete_shader(id);
                return Err(format!("Shader compilation failed: {}", log).into());
            }
        }

        Ok(Self {
            gl: gl.clone(),
            stage: descriptor.stage,
            language: descriptor.source.language,
            id,
        })
    }

    /// Returns the GL shader ID.
    pub fn id(&self) -> glow::NativeShader {
        self.id
    }
}

impl Drop for OpenGLShader {
    fn drop(&mut self) {
        unsafe { self.gl.delete_shader(self.id) };
    }
}

impl ShaderTrait for OpenGLShader {
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
        self.id.0.get() as u64
    }
}
