//! Vulkan Swap Chain implementation (WSI present).

use super::device::VulkanDevice;
use crate::rhi::surface::Surface;
use crate::rhi::swapchain::{SwapChain, SwapChainDescriptor};
use crate::rhi::texture::{Texture, TextureFormat, TextureUsage};
use ash::vk;
use ash::vk::Handle;
use std::sync::Arc;

/// Lightweight wrapper around a swap chain image usable as an RHI `Texture`.
struct VulkanSwapChainImage {
    handle: u64,
    image: vk::Image,
    view: vk::ImageView,
    format: TextureFormat,
    width: u32,
    height: u32,
}

impl std::fmt::Debug for VulkanSwapChainImage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VulkanSwapChainImage")
            .field("handle", &self.handle)
            .field("width", &self.width)
            .field("height", &self.height)
            .field("format", &self.format)
            .finish()
    }
}

impl Texture for VulkanSwapChainImage {
    fn width(&self) -> u32 {
        self.width
    }
    fn height(&self) -> u32 {
        self.height
    }
    fn depth(&self) -> u32 {
        1
    }
    fn format(&self) -> TextureFormat {
        self.format
    }
    fn mip_levels(&self) -> u32 {
        1
    }
    fn update(
        &mut self,
        _data: &[u8],
        _mip_level: u32,
        _layer: u32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        Err("swap chain images cannot be updated directly".into())
    }
    fn bind(&self, _unit: u32) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }
    fn native_handle(&self) -> u64 {
        self.handle
    }
}

/// Converts a swap chain base type to the RHI texture format.
fn to_texture_format(_fmt: vk::Format) -> TextureFormat {
    // Swap chains typically use RGBA8/BGRA8; the descriptor drives the RHI format.
    TextureFormat::Rgba8Unorm
}

/// Vulkan swap chain backed by a real `VkSwapchainKHR`.
pub struct VulkanSwapChain {
    device: Arc<VulkanDevice>,
    surface: vk::SurfaceKHR,
    swapchain_fns: ash::khr::swapchain::Device,
    surface_fns: ash::khr::surface::Instance,
    swapchain: vk::SwapchainKHR,
    descriptor: SwapChainDescriptor,
    image_format: vk::Format,
    extent: vk::Extent2D,
    present_mode: vk::PresentModeKHR,
    images: Vec<VulkanSwapChainImage>,
    current_index: u32,
    acquire_semaphore: vk::Semaphore,
}

impl VulkanSwapChain {
    pub fn new(
        device: Arc<VulkanDevice>,
        surface: &dyn Surface,
        descriptor: SwapChainDescriptor,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        // The RHI Surface trait exposes the real VkSurfaceKHR handle opaque.
        let vk_surface = vk::SurfaceKHR::from_raw(surface.native_handle());
        let extent = vk::Extent2D {
            width: descriptor.width,
            height: descriptor.height,
        };

        let swapchain_fns =
            ash::khr::swapchain::Device::new(device.instance(), device.device_arc());
        let surface_fns = ash::khr::surface::Instance::new(device.entry(), device.instance());

        let mut sc = Self {
            device: device.clone(),
            surface: vk_surface,
            swapchain_fns,
            surface_fns,
            swapchain: vk::SwapchainKHR::null(),
            descriptor: descriptor.clone(),
            image_format: vk::Format::B8G8R8A8_UNORM,
            extent,
            present_mode: vk::PresentModeKHR::FIFO,
            images: Vec::new(),
            current_index: 0,
            acquire_semaphore: vk::Semaphore::null(),
        };

        sc.image_format = sc.pick_format()?;
        sc.present_mode = sc.pick_present_mode(descriptor.vsync)?;
        sc.create_images(vk::SwapchainKHR::null())?;
        Ok(sc)
    }

    fn pick_format(&self) -> Result<vk::Format, Box<dyn std::error::Error>> {
        let formats = unsafe {
            self.surface_fns
                .get_physical_device_surface_formats(self.device.physical_device(), self.surface)?
        };
        for f in &formats {
            if f.format == vk::Format::B8G8R8A8_UNORM
                && f.color_space == vk::ColorSpaceKHR::SRGB_NONLINEAR
            {
                return Ok(f.format);
            }
        }
        Ok(formats
            .first()
            .ok_or("swap chain: no surface formats available")?
            .format)
    }

    fn pick_present_mode(
        &self,
        vsync: bool,
    ) -> Result<vk::PresentModeKHR, Box<dyn std::error::Error>> {
        let modes = unsafe {
            self.surface_fns.get_physical_device_surface_present_modes(
                self.device.physical_device(),
                self.surface,
            )?
        };
        if vsync {
            Ok(vk::PresentModeKHR::FIFO)
        } else if modes.contains(&vk::PresentModeKHR::MAILBOX) {
            Ok(vk::PresentModeKHR::MAILBOX)
        } else if modes.contains(&vk::PresentModeKHR::IMMEDIATE) {
            Ok(vk::PresentModeKHR::IMMEDIATE)
        } else {
            Ok(vk::PresentModeKHR::FIFO)
        }
    }

    fn pick_transform(&self) -> vk::SurfaceTransformFlagsKHR {
        let caps = unsafe {
            self.surface_fns.get_physical_device_surface_capabilities(
                self.device.physical_device(),
                self.surface,
            )
        };
        match caps {
            Ok(caps) if caps.current_transform != vk::SurfaceTransformFlagsKHR::IDENTITY => {
                caps.current_transform
            }
            _ => vk::SurfaceTransformFlagsKHR::IDENTITY,
        }
    }

    fn create_images(&mut self, old: vk::SwapchainKHR) -> Result<(), Box<dyn std::error::Error>> {
        let caps = unsafe {
            self.surface_fns.get_physical_device_surface_capabilities(
                self.device.physical_device(),
                self.surface,
            )?
        };
        let min_image_count = caps.min_image_count.max(self.descriptor.image_count);

        let create_info = vk::SwapchainCreateInfoKHR::default()
            .surface(self.surface)
            .min_image_count(min_image_count)
            .image_format(self.image_format)
            .image_color_space(vk::ColorSpaceKHR::SRGB_NONLINEAR)
            .image_extent(self.extent)
            .image_array_layers(1)
            .image_usage(vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::TRANSFER_DST)
            .image_sharing_mode(vk::SharingMode::EXCLUSIVE)
            .pre_transform(self.pick_transform())
            .composite_alpha(vk::CompositeAlphaFlagsKHR::OPAQUE)
            .present_mode(self.present_mode)
            .clipped(true)
            .old_swapchain(old);

        let swapchain = unsafe { self.swapchain_fns.create_swapchain(&create_info, None)? };
        self.swapchain = swapchain;

        let image_handles = unsafe { self.swapchain_fns.get_swapchain_images(swapchain)? };

        unsafe { self.device.device_arc().device_wait_idle()? };

        self.images.clear();
        for (i, img) in image_handles.iter().enumerate() {
            let view = unsafe {
                self.device.device_arc().create_image_view(
                    &vk::ImageViewCreateInfo::default()
                        .image(*img)
                        .view_type(vk::ImageViewType::TYPE_2D)
                        .format(self.image_format)
                        .components(vk::ComponentMapping {
                            r: vk::ComponentSwizzle::IDENTITY,
                            g: vk::ComponentSwizzle::IDENTITY,
                            b: vk::ComponentSwizzle::IDENTITY,
                            a: vk::ComponentSwizzle::IDENTITY,
                        })
                        .subresource_range(
                            vk::ImageSubresourceRange::default()
                                .aspect_mask(vk::ImageAspectFlags::COLOR)
                                .base_mip_level(0)
                                .level_count(1)
                                .base_array_layer(0)
                                .layer_count(1),
                        ),
                    None,
                )?
            };
            let mut wrapper = VulkanSwapChainImage {
                handle: img.as_raw(),
                image: *img,
                view,
                format: to_texture_format(self.image_format),
                width: self.extent.width,
                height: self.extent.height,
            };
            register_swapchain_image(&self.device, &mut wrapper);
            let _ = i;
            self.images.push(wrapper);
        }
        Ok(())
    }
}

impl Drop for VulkanSwapChain {
    fn drop(&mut self) {
        unsafe {
            for img in &self.images {
                self.device.device_arc().destroy_image_view(img.view, None);
            }
            if !self.acquire_semaphore.is_null() {
                self.device
                    .device_arc()
                    .destroy_semaphore(self.acquire_semaphore, None);
            }
            if !self.swapchain.is_null() {
                self.swapchain_fns.destroy_swapchain(self.swapchain, None);
            }
        }
    }
}

impl SwapChain for VulkanSwapChain {
    fn current_image_index(&self) -> u32 {
        self.current_index
    }

    fn image_count(&self) -> u32 {
        self.descriptor.image_count
    }

    fn get_image(&self, index: u32) -> &dyn Texture {
        &self.images[index as usize]
    }

    fn current_image(&self) -> &dyn Texture {
        &self.images[self.current_index as usize]
    }

    fn acquire_next_image(&mut self) -> Result<u32, Box<dyn std::error::Error>> {
        if self.acquire_semaphore.is_null() {
            self.acquire_semaphore = unsafe {
                self.device
                    .device_arc()
                    .create_semaphore(&vk::SemaphoreCreateInfo::default(), None)?
            };
        }
        let (index, _suboptimal) = unsafe {
            self.swapchain_fns.acquire_next_image(
                self.swapchain,
                u64::MAX,
                self.acquire_semaphore,
                vk::Fence::null(),
            )?
        };
        self.current_index = index;
        Ok(index)
    }

    fn present(&self) -> Result<(), Box<dyn std::error::Error>> {
        let wait_semas = if self.acquire_semaphore.is_null() {
            &[] as &[vk::Semaphore]
        } else {
            std::slice::from_ref(&self.acquire_semaphore)
        };
        let present_info = vk::PresentInfoKHR::default()
            .wait_semaphores(wait_semas)
            .swapchains(std::slice::from_ref(&self.swapchain))
            .image_indices(std::slice::from_ref(&self.current_index));
        unsafe {
            self.swapchain_fns
                .queue_present(self.device.present_queue(), &present_info)?
        };
        Ok(())
    }

    fn resize(&mut self, width: u32, height: u32) -> Result<(), Box<dyn std::error::Error>> {
        self.descriptor.width = width;
        self.descriptor.height = height;
        self.extent = vk::Extent2D { width, height };
        unsafe { self.device.device_arc().device_wait_idle()? };
        let old = self.swapchain;
        for img in &self.images {
            unsafe {
                self.device.device_arc().destroy_image_view(img.view, None);
            }
        }
        self.images.clear();
        self.create_images(old)?;
        self.current_index = 0;
        Ok(())
    }

    fn size(&self) -> crate::rhi::types::Extent2D {
        crate::rhi::types::Extent2D::new(self.descriptor.width, self.descriptor.height)
    }
}

/// Registers a swap chain image in the global image registry so it can be used
/// as a render target attachment by handle.
fn register_swapchain_image(device: &VulkanDevice, img: &VulkanSwapChainImage) {
    let _ = device;
    super::registry::register(
        img.image,
        super::registry::RegisteredImage {
            image: img.image,
            view: img.view,
            sampler: vk::Sampler::null(),
            width: img.width,
            height: img.height,
            depth: 1,
            format: img.format,
            usage: TextureUsage::RenderTarget,
            aspect: vk::ImageAspectFlags::COLOR,
            array_layers: 1,
        },
    );
}
