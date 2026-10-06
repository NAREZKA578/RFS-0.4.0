//! Direct3D 12 Backend
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use crate::{backend::Backend, config::RhiConfig, core::*, error::*, types::GraphicsApi};

pub struct D3D12Backend;

impl D3D12Backend {
    pub fn new(_config: &RhiConfig) -> RhiResult<Self> {
        Ok(Self)
    }
}

impl Backend for D3D12Backend {
    fn name(&self) -> &str {
        "Direct3D 12"
    }
    fn api(&self) -> GraphicsApi {
        GraphicsApi::Direct3D12
    }
    fn enumerate_physical_devices(&self) -> RhiResult<Vec<PhysicalDevice>> {
        Err(RhiError::NotSupported(
            "D3D12 backend is a stub without a real GPU API".into(),
        ))
    }
    fn create_device(&self, _physical: &PhysicalDevice, _desc: &DeviceDesc) -> RhiResult<Device> {
        Err(RhiError::NotSupported(
            "D3D12 backend is a stub without a real GPU API".into(),
        ))
    }
    fn wait_idle(&self, _device: &Device) -> RhiResult<()> {
        Ok(())
    }
}