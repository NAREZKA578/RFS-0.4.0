//! OpenGL Backend
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use crate::{backend::Backend, config::RhiConfig, core::*, error::*, types::GraphicsApi};

pub struct OpenGLBackend;

impl OpenGLBackend {
    pub fn new(_config: &RhiConfig) -> RhiResult<Self> {
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
        Err(RhiError::NotSupported(
            "OpenGL backend is a stub without a real GPU API".into(),
        ))
    }
    fn create_device(&self, _physical: &PhysicalDevice, _desc: &DeviceDesc) -> RhiResult<Device> {
        Err(RhiError::NotSupported(
            "OpenGL backend is a stub without a real GPU API".into(),
        ))
    }
    fn wait_idle(&self, _device: &Device) -> RhiResult<()> {
        Ok(())
    }
}