//! OpenGL 3.3+ backend implementation for RHI.

mod buffer;
mod command;
mod device;
mod pipeline;
mod shader;
mod surface;
mod swapchain;
mod sync;
mod texture;

pub use buffer::OpenGLBuffer;
pub use command::OpenGLCommandEncoder;
pub use device::OpenGLDevice;
pub use pipeline::OpenGLPipeline;
pub use shader::OpenGLShader;
pub use surface::OpenGLSurface;
pub use swapchain::OpenGLSwapChain;
pub use sync::{OpenGLFence, OpenGLSemaphore};
pub use texture::OpenGLTexture;

use crate::rhi::{Backend, BackendType, Device, GpuInfo};
use std::sync::Arc;

/// OpenGL backend.
pub struct OpenGLBackend {
    device: Arc<dyn Device>,
}

impl OpenGLBackend {
    /// Creates a new OpenGL backend from an existing GL context.
    pub fn new(gl_context: Arc<glow::Context>) -> Result<Self, Box<dyn std::error::Error>> {
        let device = OpenGLDevice::new(gl_context)?;
        Ok(Self {
            device: Arc::new(device),
        })
    }
}

impl Backend for OpenGLBackend {
    fn backend_type(&self) -> BackendType {
        BackendType::OpenGL
    }

    fn device(&self) -> &Arc<dyn Device> {
        &self.device
    }

    fn device_mut(&mut self) -> &mut Arc<dyn Device> {
        &mut self.device
    }

    fn create_surface(
        &self,
        descriptor: &crate::rhi::surface::SurfaceDescriptor,
    ) -> Result<Box<dyn crate::rhi::surface::Surface>, Box<dyn std::error::Error>> {
        Ok(Box::new(OpenGLSurface::new(descriptor.clone())))
    }

    fn create_swap_chain(
        &self,
        surface: &dyn crate::rhi::surface::Surface,
        descriptor: &crate::rhi::swapchain::SwapChainDescriptor,
    ) -> Result<Box<dyn crate::rhi::swapchain::SwapChain>, Box<dyn std::error::Error>> {
        Ok(Box::new(OpenGLSwapChain::new(surface, descriptor.clone())))
    }

    fn present(
        &self,
        _swap_chain: &dyn crate::rhi::swapchain::SwapChain,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // In OpenGL, presenting is done by the window's swap buffers
        Ok(())
    }

    fn gpu_info(&self) -> GpuInfo {
        self.device.gpu_info()
    }
}
