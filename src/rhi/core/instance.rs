//! RHI Instance
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use crate::backend::Backend;
use crate::{
    backend,
    config::RhiConfig,
    core::device::{Device, DeviceDesc, PhysicalDevice},
    error::*,
    types::GraphicsApi,
};
use std::sync::{Arc, RwLock};

/// Main RHI instance (singleton)
pub struct Rhi {
    config: RhiConfig,
    backend: Box<dyn Backend>,
    physical_devices: Vec<PhysicalDevice>,
    devices: Vec<Arc<RwLock<Device>>>,
    default_device: Option<Arc<RwLock<Device>>>,
}

impl Rhi {
    /// Creates a new RHI instance
    pub fn new(config: RhiConfig) -> RhiResult<Self> {
        let backend = backend::create_backend(&config)?;
        let physical_devices = backend.enumerate_physical_devices()?;

        Ok(Self {
            config,
            backend,
            physical_devices,
            devices: Vec::new(),
            default_device: None,
        })
    }

    /// Returns the configuration
    pub fn config(&self) -> &RhiConfig {
        &self.config
    }

    /// Returns the API backend
    pub fn api(&self) -> GraphicsApi {
        self.backend.api()
    }

    /// Returns available physical devices
    pub fn physical_devices(&self) -> &[PhysicalDevice] {
        &self.physical_devices
    }

    /// Returns the default physical device
    pub fn default_physical_device(&self) -> Option<&PhysicalDevice> {
        self.physical_devices.first()
    }

    /// Returns a physical device by index, or `InvalidPhysicalDeviceIndex`.
    pub fn physical_device(&self, index: usize) -> RhiResult<&PhysicalDevice> {
        self.physical_devices
            .get(index)
            .ok_or(RhiError::InvalidPhysicalDeviceIndex(index))
    }

    /// Returns the number of available physical devices.
    pub fn physical_device_count(&self) -> usize {
        self.physical_devices.len()
    }

    /// Creates a new logical device from the default physical device.
    pub fn create_device(&mut self, desc: &DeviceDesc) -> RhiResult<Arc<RwLock<Device>>> {
        self.create_device_at(0, desc)
    }

    /// Creates a new logical device from a specific physical device.
    pub fn create_device_at(
        &mut self,
        physical_index: usize,
        desc: &DeviceDesc,
    ) -> RhiResult<Arc<RwLock<Device>>> {
        let physical = self.physical_device(physical_index)?;

        let device = self.backend.create_device(physical, desc)?;
        let device_arc = Arc::new(RwLock::new(device));

        if self.devices.is_empty() {
            self.default_device = Some(device_arc.clone());
        }

        self.devices.push(device_arc.clone());
        Ok(device_arc)
    }

    /// Returns the default device
    pub fn default_device(&self) -> Option<Arc<RwLock<Device>>> {
        self.default_device.clone()
    }

    /// Waits for all devices to become idle
    pub fn wait_idle(&self) -> RhiResult<()> {
        for device in &self.devices {
            device.read().unwrap().wait_idle()?;
        }
        Ok(())
    }
}

impl Drop for Rhi {
    fn drop(&mut self) {
        let _ = self.wait_idle();
        self.devices.clear();
    }
}
