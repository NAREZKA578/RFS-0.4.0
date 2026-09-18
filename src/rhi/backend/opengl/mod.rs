//! OpenGL Backend
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//! **TODO: Full implementation required**

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
        unimplemented!()
    }
    fn create_device(&self, _physical: &PhysicalDevice, _desc: &DeviceDesc) -> RhiResult<Device> {
        unimplemented!()
    }
    fn wait_idle(&self, _device: &Device) -> RhiResult<()> {
        unimplemented!()
    }
}
