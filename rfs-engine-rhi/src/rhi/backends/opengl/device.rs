//! OpenGL Device implementation.

use crate::rhi::{
    BackendType, Buffer, BufferDescriptor, CommandEncoder, Device, Fence, GpuInfo, Shader,
    ShaderDescriptor, Texture, TextureDescriptor,
};
use std::sync::Arc;

/// OpenGL device.
pub struct OpenGLDevice {
    gl: Arc<glow::Context>,
    gpu_info: GpuInfo,
}

impl OpenGLDevice {
    /// Creates a new OpenGL device from an existing GL context.
    pub fn new(gl: Arc<glow::Context>) -> Result<Self, Box<dyn std::error::Error>> {
        let vendor = unsafe { glow::HasContext::get_parameter_string(&*gl, glow::VENDOR) };
        let renderer = unsafe { glow::HasContext::get_parameter_string(&*gl, glow::RENDERER) };
        let version = unsafe { glow::HasContext::get_parameter_string(&*gl, glow::VERSION) };

        let gpu_info = GpuInfo::new(&vendor, &renderer, &version, BackendType::OpenGL);

        Ok(Self { gl, gpu_info })
    }

    /// Returns a reference to the GL context.
    pub fn gl(&self) -> &Arc<glow::Context> {
        &self.gl
    }
}

impl Device for OpenGLDevice {
    fn create_buffer(
        &self,
        descriptor: &BufferDescriptor,
        data: Option<&[u8]>,
    ) -> Result<Box<dyn Buffer>, Box<dyn std::error::Error>> {
        Ok(Box::new(super::buffer::OpenGLBuffer::new(
            &self.gl, descriptor, data,
        )?))
    }

    fn create_texture(
        &self,
        descriptor: &TextureDescriptor,
        data: Option<&[u8]>,
    ) -> Result<Box<dyn Texture>, Box<dyn std::error::Error>> {
        Ok(Box::new(super::texture::OpenGLTexture::new(
            &self.gl, descriptor, data,
        )?))
    }

    fn create_shader(
        &self,
        descriptor: &ShaderDescriptor,
    ) -> Result<Box<dyn Shader>, Box<dyn std::error::Error>> {
        Ok(Box::new(super::shader::OpenGLShader::new(
            &self.gl, descriptor,
        )?))
    }

    fn create_pipeline(
        &self,
        descriptor: &crate::rhi::pipeline::PipelineDescriptor,
    ) -> Result<Box<dyn crate::rhi::pipeline::Pipeline>, Box<dyn std::error::Error>> {
        Ok(Box::new(super::pipeline::OpenGLPipeline::new(
            &self.gl, descriptor,
        )?))
    }

    fn create_command_encoder(
        &self,
    ) -> Result<Box<dyn CommandEncoder>, Box<dyn std::error::Error>> {
        Ok(Box::new(super::command::OpenGLCommandEncoder::new(
            self.gl.clone(),
        )))
    }

    fn submit(
        &self,
        _commands: &[&dyn CommandEncoder],
        _fence: Option<&mut Fence>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // OpenGL executes commands immediately, no submission needed
        Ok(())
    }

    fn create_fence(&self, signaled: bool) -> Result<Fence, Box<dyn std::error::Error>> {
        Ok(super::sync::OpenGLFence::create(signaled))
    }

    fn wait_for_fence(
        &self,
        fence: &Fence,
        timeout_ns: u64,
    ) -> Result<(), Box<dyn std::error::Error>> {
        super::sync::OpenGLFence::wait(fence, timeout_ns)
    }

    fn destroy_fence(&self, _fence: Fence) {
        // Fences are dropped automatically
    }

    fn wait_idle(&self) -> Result<(), Box<dyn std::error::Error>> {
        unsafe { glow::HasContext::finish(&*self.gl) }
        Ok(())
    }

    fn backend_type(&self) -> BackendType {
        BackendType::OpenGL
    }

    fn gpu_info(&self) -> GpuInfo {
        self.gpu_info.clone()
    }

    fn clear(&self, color: [f32; 4], depth: f32) {
        unsafe {
            glow::HasContext::clear_color(&*self.gl, color[0], color[1], color[2], color[3]);
            glow::HasContext::clear_depth_f64(&*self.gl, depth as f64);
            glow::HasContext::clear(&*self.gl, glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT);
        }
    }
}
