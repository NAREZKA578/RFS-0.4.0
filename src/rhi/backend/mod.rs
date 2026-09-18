//! Backend Implementations
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use crate::{config::RhiConfig, core::*, error::*, types::GraphicsApi};

/// Trait for all RHI backends
pub trait Backend: Send + Sync {
    fn name(&self) -> &str;
    fn api(&self) -> GraphicsApi;
    fn enumerate_physical_devices(&self) -> RhiResult<Vec<PhysicalDevice>>;
    fn create_device(&self, physical: &PhysicalDevice, desc: &DeviceDesc) -> RhiResult<Device>;
    fn wait_idle(&self, device: &Device) -> RhiResult<()>;
}

pub mod common;
#[cfg(all(target_os = "windows", feature = "d3d11"))]
pub mod d3d11;
#[cfg(all(target_os = "windows", feature = "d3d12"))]
pub mod d3d12;
#[cfg(feature = "opengl")]
pub mod opengl;
#[cfg(feature = "vulkan")]
pub mod vulkan;

/// Creates a backend based on config
pub fn create_backend(config: &RhiConfig) -> RhiResult<Box<dyn Backend>> {
    match config.api {
        GraphicsApi::Vulkan => {
            #[cfg(feature = "vulkan")]
            return Ok(Box::new(vulkan::VulkanBackend::new(config)?));
            #[cfg(not(feature = "vulkan"))]
            Err(RhiError::NoBackendForApi("Vulkan".into()))
        }
        GraphicsApi::Direct3D12 => {
            #[cfg(all(target_os = "windows", feature = "d3d12"))]
            return Ok(Box::new(d3d12::D3D12Backend::new(config)?));
            #[cfg(not(all(target_os = "windows", feature = "d3d12")))]
            Err(RhiError::NoBackendForApi("Direct3D 12".into()))
        }
        GraphicsApi::Direct3D11 => {
            #[cfg(all(target_os = "windows", feature = "d3d11"))]
            return Ok(Box::new(d3d11::D3D11Backend::new(config)?));
            #[cfg(not(all(target_os = "windows", feature = "d3d11")))]
            Err(RhiError::NoBackendForApi("Direct3D 11".into()))
        }
        GraphicsApi::OpenGL => {
            #[cfg(feature = "opengl")]
            return Ok(Box::new(opengl::OpenGLBackend::new(config)?));
            #[cfg(not(feature = "opengl"))]
            Err(RhiError::NoBackendForApi("OpenGL".into()))
        }
    }
}
