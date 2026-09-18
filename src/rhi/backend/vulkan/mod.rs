//! Vulkan Backend
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//! **TODO: Full implementation required**

use crate::{backend::Backend, config::RhiConfig, core::*, error::*, types::GraphicsApi};

pub struct VulkanBackend;

impl VulkanBackend {
    pub fn new(_config: &RhiConfig) -> RhiResult<Self> {
        unimplemented!("Vulkan backend not implemented yet")
    }
}

impl Backend for VulkanBackend {
    fn name(&self) -> &str {
        "Vulkan"
    }
    fn api(&self) -> GraphicsApi {
        GraphicsApi::Vulkan
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
