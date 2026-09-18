//! Direct3D 11 Backend
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//! **TODO: Full implementation required**

use crate::{backend::Backend, config::RhiConfig, core::*, error::*, types::GraphicsApi};

pub struct D3D11Backend;

impl D3D11Backend {
    pub fn new(_config: &RhiConfig) -> RhiResult<Self> {
        Ok(Self)
    }
}

impl Backend for D3D11Backend {
    fn name(&self) -> &str {
        "Direct3D 11"
    }
    fn api(&self) -> GraphicsApi {
        GraphicsApi::Direct3D11
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
