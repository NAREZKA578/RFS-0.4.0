//! Vulkan 1.3+ backend implementation for RHI.

mod buffer;
mod command;
mod device;
mod pipeline;
mod registry;
mod shader;
mod surface;
mod swapchain;
mod sync;
mod texture;

pub use buffer::VulkanBuffer;
pub use command::VulkanCommandEncoder;
pub use device::VulkanDevice;
pub use pipeline::VulkanPipeline;
pub use registry::{set_default_color_target, set_default_depth_target};
pub use shader::VulkanShader;
pub use surface::VulkanSurface;
pub use swapchain::VulkanSwapChain;
pub use sync::{VulkanFence, VulkanSemaphore};
pub use texture::VulkanTexture;

use crate::rhi::{Backend, BackendDescriptor, BackendType, Device, GpuInfo};
use std::sync::Arc;

/// Vulkan backend.
pub struct VulkanBackend {
    device: Arc<dyn Device>,
    vulkan_device: Arc<VulkanDevice>,
}

impl VulkanBackend {
    /// Creates a new Vulkan backend.
    pub fn new(desc: &BackendDescriptor) -> Result<Self, Box<dyn std::error::Error>> {
        let vulkan_device = Arc::new(device::VulkanDevice::new(desc)?);
        let handle = device::VulkanDeviceHandle::new(vulkan_device.clone());
        Ok(Self {
            device: Arc::new(handle),
            vulkan_device,
        })
    }
}

impl Backend for VulkanBackend {
    fn backend_type(&self) -> BackendType {
        BackendType::Vulkan
    }

    fn device(&self) -> &Arc<dyn Device> {
        &self.device
    }

    fn device_mut(&mut self) -> &mut Arc<dyn Device> {
        &mut self.device
    }

    fn create_surface(
        &self,
        descriptor: &crate::rhi::surface::SurfaceDescriptor,
    ) -> Result<Box<dyn crate::rhi::surface::Surface>, Box<dyn std::error::Error>> {
        Ok(Box::new(VulkanSurface::new(
            self.vulkan_device.clone(),
            descriptor.clone(),
        )?))
    }

    fn create_swap_chain(
        &self,
        surface: &dyn crate::rhi::surface::Surface,
        descriptor: &crate::rhi::swapchain::SwapChainDescriptor,
    ) -> Result<Box<dyn crate::rhi::swapchain::SwapChain>, Box<dyn std::error::Error>> {
        Ok(Box::new(VulkanSwapChain::new(
            self.vulkan_device.clone(),
            surface,
            descriptor.clone(),
        )?))
    }

    fn present(
        &self,
        swap_chain: &dyn crate::rhi::swapchain::SwapChain,
    ) -> Result<(), Box<dyn std::error::Error>> {
        swap_chain.present()
    }

    fn gpu_info(&self) -> GpuInfo {
        self.device.gpu_info()
    }
}
