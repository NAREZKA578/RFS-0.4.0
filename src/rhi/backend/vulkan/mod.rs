//! Vulkan Backend
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

mod convert;
mod device;
mod instance;
mod memory;
mod pipeline_cache;
mod present;
mod record;

// The presentation types are `pub(crate)`-constructible because building a
// surface needs the crate-internal `DestroyableInstance`. They are re-exported
// so the render layer can *hold* a `Swapchain` and be handed an already-built
// one by the backend, without being able to construct one behind its back.
pub use present::{AcquiredImage, Swapchain, SwapchainRequest, WindowSurface};

use crate::backend::Backend;
use crate::config::RhiConfig;
use crate::core::{Device, DeviceDesc, PhysicalDevice};
use crate::error::*;
use crate::types::GraphicsApi;
use ash::vk;
use std::sync::Arc;

pub struct VulkanBackend {
    destroyable: Arc<instance::DestroyableInstance>,
    physical_handles: Vec<vk::PhysicalDevice>,
    reports: Vec<PhysicalDevice>,
}

impl VulkanBackend {
    pub fn new(config: &RhiConfig) -> RhiResult<Self> {
        let destroyable = instance::create_instance(config)?;
        let physical_handles = instance::enumerate_physical_devices(&destroyable.instance)?;
        let reports = physical_handles
            .iter()
            .map(|&handle| instance::physical_device_report(&destroyable.instance, handle))
            .collect();
        Ok(Self {
            destroyable,
            physical_handles,
            reports,
        })
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
        Ok(self.reports.clone())
    }

    fn create_device(&self, physical: &PhysicalDevice, desc: &DeviceDesc) -> RhiResult<Device> {
        let index = self
            .reports
            .iter()
            .position(|report| {
                report.name == physical.name
                    && report.vendor_id == physical.vendor_id
                    && report.device_id == physical.device_id
                    && report.device_type == physical.device_type
            })
            .ok_or_else(|| {
                RhiError::BackendError(
                    "physical device does not belong to this Vulkan backend".into(),
                )
            })?;
        let handle = self.physical_handles[index];
        let extensions = device::supported_device_extensions(&self.destroyable.instance, handle)?;
        let logical = device::create_logical_device(
            &self.destroyable.instance,
            handle,
            &self.reports[index],
            desc,
            &extensions,
        )?;
        let resource = Box::new(
            device::VulkanLogicalDeviceResource::new(
                logical.device,
                &self.reports[index].memory_properties,
                logical.queues[0].family_index,
                handle,
                self.destroyable.clone(),
            )?,
        );
        Ok(Device::from_parts(
            self.reports[index].clone(),
            logical.queues,
            resource,
        ))
    }

    fn wait_idle(&self, device: &Device) -> RhiResult<()> {
        device.wait_idle()
    }
}