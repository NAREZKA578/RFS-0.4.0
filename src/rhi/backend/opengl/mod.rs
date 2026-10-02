//! OpenGL Backend
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

mod device;

use crate::{backend::Backend, config::RhiConfig, core::*, error::*, types::GraphicsApi};

/// OpenGL Backend implementation
pub struct OpenGLBackend;

impl OpenGLBackend {
    pub fn new(_config: &RhiConfig) -> RhiResult<Self> {
        // In a real implementation, this would:
        // 1. Initialize GL context (WGL/GLX)
        // 2. Load GL functions (GLEW/glad)
        // 3. Initialize extensions
        Ok(Self)
    }
}

impl Backend for OpenGLBackend {
    fn name(&self) -> &str {
        "OpenGL"
    }
    
    fn api(&self) -> GraphicsApi {
        GraphicsApi::OpenGL
    }
    
    fn enumerate_physical_devices(&self) -> RhiResult<Vec<PhysicalDevice>> {
        // Not implemented yet: this backend has no adapter enumeration, so
        // reporting a device here would let the caller pick it and then fail
        // at device creation with no indication why.
        Err(RhiError::NotSupported(format!(
            "{} device enumeration is not implemented",
            self.name()
        )))
    }

    fn create_device(&self, _physical: &PhysicalDevice, _desc: &DeviceDesc) -> RhiResult<Device> {
        Err(RhiError::NotSupported(format!(
            "{} device creation is not implemented",
            self.name()
        )))
    }

    fn wait_idle(&self, _device: &Device) -> RhiResult<()> {
        Ok(())
    }
}

