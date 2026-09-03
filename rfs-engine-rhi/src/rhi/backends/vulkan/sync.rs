//! Vulkan synchronization primitives.

use crate::rhi::sync::Fence;
use ash::vk;
use ash::vk::Handle;

/// Vulkan fence wrapper.
pub struct VulkanFence;

impl VulkanFence {
    pub fn create(device: &ash::Device, signaled: bool) -> Fence {
        let create_info = vk::FenceCreateInfo::default().flags(if signaled {
            vk::FenceCreateFlags::SIGNALED
        } else {
            vk::FenceCreateFlags::empty()
        });

        let fence = unsafe { device.create_fence(&create_info, None) };
        match fence {
            Ok(f) => Fence::Vulkan(f.as_raw()),
            Err(_) => Fence::Uninitialized,
        }
    }

    pub fn wait(
        device: &ash::Device,
        fence: &Fence,
        timeout_ns: u64,
    ) -> Result<(), Box<dyn std::error::Error>> {
        match fence.handle() {
            Some(handle) => {
                let vk_fence = vk::Fence::from_raw(handle);
                unsafe { device.wait_for_fences(&[vk_fence], true, timeout_ns)? };
                Ok(())
            }
            None => Err("Invalid fence".into()),
        }
    }

    pub fn destroy(device: &ash::Device, fence: Fence) {
        if let Some(handle) = fence.handle() {
            let vk_fence = vk::Fence::from_raw(handle);
            unsafe { device.destroy_fence(vk_fence, None) };
        }
    }
}

/// Vulkan semaphore.
pub struct VulkanSemaphore;

impl VulkanSemaphore {
    pub fn create(device: &ash::Device) -> crate::rhi::sync::Semaphore {
        let create_info = vk::SemaphoreCreateInfo::default();
        match unsafe { device.create_semaphore(&create_info, None) } {
            Ok(s) => crate::rhi::sync::Semaphore::Vulkan(s.as_raw()),
            Err(_) => crate::rhi::sync::Semaphore::Uninitialized,
        }
    }
}
