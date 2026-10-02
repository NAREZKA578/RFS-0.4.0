//! Direct3D 11 Backend
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

mod device;

use crate::{backend::Backend, config::RhiConfig, core::*, error::*, types::GraphicsApi};

/// Direct3D 11 Backend implementation
pub struct D3D11Backend;

impl D3D11Backend {
    pub fn new(_config: &RhiConfig) -> RhiResult<Self> {
        // Initialize COM for Direct3D
        // Note: In a real implementation, this would:
        // 1. Initialize COM (CoInitializeEx)
        // 2. Create DXGI factory (IDXGIFactory)
        // 3. Enumerate adapters
        // For mock purposes, we just return Ok
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

