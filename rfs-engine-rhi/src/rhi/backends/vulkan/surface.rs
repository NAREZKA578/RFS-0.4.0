//! Vulkan Surface implementation (Win32 WSI).

use super::device::VulkanDevice;
use crate::rhi::surface::{Surface, SurfaceDescriptor};
use ash::vk;
use ash::vk::Handle;
use std::sync::Arc;

/// Vulkan surface backed by a real `VkSurfaceKHR`.
pub struct VulkanSurface {
    // Kept for lifecycle: keeps the owning device (and thus the instance) alive.
    #[allow(dead_code)]
    device: Arc<VulkanDevice>,
    surface_fns: ash::khr::surface::Instance,
    // Kept for lifecycle: keeps the Win32 loader alive.
    #[allow(dead_code)]
    win32_fns: ash::khr::win32_surface::Instance,
    surface: vk::SurfaceKHR,
    descriptor: SurfaceDescriptor,
}

impl VulkanSurface {
    /// Creates a Win32 surface from the device's instance and the descriptor's
    /// HWND/HINSTANCE handles.
    pub fn new(
        device: Arc<VulkanDevice>,
        descriptor: SurfaceDescriptor,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let entry = device.entry();
        let instance = device.instance();

        let surface_fns = ash::khr::surface::Instance::new(entry, instance);

        #[cfg(target_os = "windows")]
        {
            let win32_fns = ash::khr::win32_surface::Instance::new(entry, instance);
            let hwnd = descriptor.window_handle as isize;
            let hinstance = descriptor.instance_handle as isize;
            let create_info = vk::Win32SurfaceCreateInfoKHR::default()
                .hinstance(hinstance)
                .hwnd(hwnd);
            let surface = unsafe { win32_fns.create_win32_surface(&create_info, None)? };
            Ok(Self {
                device,
                surface_fns,
                win32_fns,
                surface,
                descriptor,
            })
        }

        #[cfg(not(target_os = "windows"))]
        {
            let _ = surface_fns;
            Err("VulkanSurface: only the Win32 WSI backend is implemented".into())
        }
    }

    /// Returns the underlying `VkSurfaceKHR`.
    #[allow(dead_code)]
    pub fn surface(&self) -> vk::SurfaceKHR {
        self.surface
    }
}

impl Drop for VulkanSurface {
    fn drop(&mut self) {
        unsafe {
            self.surface_fns.destroy_surface(self.surface, None);
        }
    }
}

impl Surface for VulkanSurface {
    fn size(&self) -> crate::rhi::types::Extent2D {
        crate::rhi::types::Extent2D::new(self.descriptor.width, self.descriptor.height)
    }

    fn resize(&mut self, width: u32, height: u32) {
        self.descriptor.width = width;
        self.descriptor.height = height;
    }

    fn native_handle(&self) -> u64 {
        self.surface.as_raw()
    }
}
