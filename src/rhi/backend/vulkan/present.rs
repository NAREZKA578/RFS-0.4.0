//! Window presentation: `VkSurfaceKHR` + `VkSwapchainKHR`.
//!
//! Step 2 of the render plan. The RHI had a CPU-side `SwapChain` struct with no
//! Vulkan backing, and `Renderer::new` ignored its window argument, so there
//! was no path from a framebuffer to a screen at all.
//!
//! Ownership follows what Vulkan requires, and that is the easy thing to get
//! wrong: `VkSurfaceKHR` belongs to the **instance** and needs
//! `VK_KHR_surface` plus a platform extension (`VK_KHR_win32_surface`), while
//! `VkSwapchainKHR` belongs to the **device** and needs `VK_KHR_swapchain`.
//! `ash` reflects that with three separate loaders, all of which are held here:
//! `khr::surface::Instance`, `khr::win32_surface::Instance` and
//! `khr::swapchain::Device`.
//!
//! Frames in flight is deliberately **one**. With a single acquire semaphore and
//! a single render fence the ordering is trivially correct, and the point of
//! this step is to prove pixels reach a screen. Because the CPU blocks on the
//! render fence before presenting, the present does not need a wait semaphore —
//! which also sidesteps `ash 0.38`, whose `queue_present` does not accept one.
//! Scaling to several frames in flight is a separate change that must add
//! per-frame state rather than mutate this.

use crate::backend::vulkan::instance::DestroyableInstance;
use crate::error::{RhiError, RhiResult};
use ash::khr::surface::Instance as SurfaceLoader;
use ash::khr::swapchain::Device as SwapchainLoader;
use ash::khr::win32_surface::Instance as Win32SurfaceLoader;
use ash::vk;
use std::sync::Arc;

/// A window surface, plus the loaders that can create and destroy it.
pub struct WindowSurface {
    pub handle: vk::SurfaceKHR,
    surface: SurfaceLoader,
    win32: Win32SurfaceLoader,
    /// Keeps the owning instance alive at least as long as the surface.
    _instance: Arc<DestroyableInstance>,
}

impl WindowSurface {
    /// Build a Win32 surface from window handles obtained via
    /// `raw-window-handle`.
    pub(crate) fn new_win32(
        instance: &Arc<DestroyableInstance>,
        hinstance: vk::HINSTANCE,
        hwnd: vk::HWND,
    ) -> RhiResult<Self> {
        let win32 = Win32SurfaceLoader::new(&instance.entry, &instance.instance);
        let create_info = vk::Win32SurfaceCreateInfoKHR {
            hinstance,
            hwnd,
            ..Default::default()
        };
        let handle = unsafe { win32.create_win32_surface(&create_info, None) }
            .map_err(|e| RhiError::BackendError(format!("create Win32 surface: {e}")))?;
        Ok(Self {
            handle,
            surface: SurfaceLoader::new(&instance.entry, &instance.instance),
            win32,
            _instance: instance.clone(),
        })
    }

    fn surface(&self) -> &SurfaceLoader {
        &self.surface
    }
}

impl Drop for WindowSurface {
    fn drop(&mut self) {
        unsafe {
            self.surface.destroy_surface(self.handle, None);
        }
    }
}

/// What the caller wants from the swapchain. Availability is negotiated rather
/// than assumed: a surface that cannot provide a mode gets the nearest thing
/// that works instead of a failure.
#[derive(Debug, Clone, Copy)]
pub struct SwapchainRequest {
    pub width: u32,
    pub height: u32,
    /// Preferred present mode; falls back to FIFO, the only guaranteed one.
    pub present_mode: vk::PresentModeKHR,
}

impl Default for SwapchainRequest {
    fn default() -> Self {
        // FIFO is guaranteed by the spec and never tears, so it is the sane
        // default. MAILBOX trades a little latency for no tearing and is
        // preferred when the driver offers it.
        Self {
            width: 1280,
            height: 720,
            present_mode: vk::PresentModeKHR::FIFO,
        }
    }
}

/// One image from `acquire`, plus the synchronisation that goes with it.
///
/// There is no "still usable but rebuild soon" flag here. `SUBOPTIMAL` used to
/// be reported that way, but it left two paths for the same condition and the
/// flag was always `false` once the acquire path also rejected the outright
/// `OUT_OF_DATE`. Both now surface as `RhiError::SwapchainOutOfDate`.
pub struct AcquiredImage {
    /// The image to render into.
    pub image_index: u32,
    /// Image view matching `image_index`, for building the render pass.
    pub image_view: vk::ImageView,
}

/// A live swapchain and the per-image resources sized to it.
pub struct Swapchain {
    loader: SwapchainLoader,
    surface: SurfaceLoader,
    device: ash::Device,
    handle: vk::SwapchainKHR,
    pub images: Vec<vk::Image>,
    pub image_views: Vec<vk::ImageView>,
    pub format: vk::Format,
    pub color_space: vk::ColorSpaceKHR,
    pub extent: vk::Extent2D,
    /// The queue this chain presents on. Captured at creation because a
    /// swapchain belongs to the queue family it was built for.
    queue: vk::Queue,

    acquire_fence: vk::Fence,
    /// Set once this chain's handle has been handed to a successor through
    /// `oldSwapchain`.
    ///
    /// When `vkCreateSwapchainKHR` succeeds with a non-null `oldSwapchain`,
    /// Vulkan has already destroyed the old chain. Dropping it would then
    /// destroy the handle a second time — a double free that only shows up on
    /// the second window resize, long after the change that caused it. The
    /// per-image views and semaphores are still this object's to release;
    /// only the swapchain handle itself is no longer ours.
    handed_over: bool,
}

impl Swapchain {
    /// Build a swapchain sized to the surface's capabilities.
    ///
    /// Takes the owning instance rather than a bare `ash::Instance` because the
    /// surface loader needs the `Entry` to resolve its function pointers, and
    /// holding the `Arc` also keeps the instance alive for the chain's life.
    /// `old` is handed to Vulkan as `oldSwapchain`, which is the only way a
    /// chain can be replaced while its surface is still in use.
    ///
    /// Without it, creating a second swapchain for the same window fails with
    /// `ERROR_NATIVE_WINDOW_IN_USE_KHR` — observed, not theorised. Passing null
    /// and dropping the old chain afterwards is not equivalent: the new chain
    /// cannot be created until the old one is destroyed, which means tearing
    /// down and rebuilding all the per-image views, and leaves a window in
    /// which nothing can be presented.
    pub(crate) fn create(
        instance: &Arc<DestroyableInstance>,
        device: ash::Device,
        surface: &WindowSurface,
        physical_device: vk::PhysicalDevice,
        queue: vk::Queue,
        request: SwapchainRequest,
        old: Option<&mut Swapchain>,
    ) -> RhiResult<Self> {
        let swapchain_loader = SwapchainLoader::new(&instance.instance, &device);
        let surface_loader = SurfaceLoader::new(&instance.entry, &instance.instance);

        let caps = unsafe {
            surface_loader.get_physical_device_surface_capabilities(physical_device, surface.handle)
        }
        .map_err(|e| RhiError::BackendError(format!("surface capabilities: {e}")))?;
        let (format, color_space) =
            pick_surface_format(&surface_loader, surface.handle, physical_device)?;
        let extent = pick_extent(&caps, request.width, request.height)?;
        // One image beyond the minimum so the CPU can prepare the next frame
        // while the GPU finishes the current one.
        let image_count = caps.min_image_count.saturating_add(1).max(2);
        let present_mode = pick_present_mode(
            &surface_loader,
            surface.handle,
            physical_device,
            request.present_mode,
        )?;

        let create_info = vk::SwapchainCreateInfoKHR {
            surface: surface.handle,
            min_image_count: image_count,
            image_format: format,
            image_color_space: color_space,
            image_extent: extent,
            image_array_layers: 1,
            image_usage: vk::ImageUsageFlags::COLOR_ATTACHMENT,
            // One queue family presents, so EXCLUSIVE sharing needs no index
            // list. Supplying indices alongside EXCLUSIVE is a validation error.
            image_sharing_mode: vk::SharingMode::EXCLUSIVE,
            queue_family_index_count: 0,
            p_queue_family_indices: std::ptr::null(),
            pre_transform: caps.current_transform,
            composite_alpha: pick_composite_alpha(&caps),
            present_mode,
            clipped: vk::TRUE,
            old_swapchain: old
                .as_ref()
                .map_or(vk::SwapchainKHR::null(), |c| c.handle),
            ..Default::default()
        };

        let handle = match unsafe { swapchain_loader.create_swapchain(&create_info, None) } {
            Ok(handle) => handle,
            Err(e) => {
                return Err(RhiError::BackendError(format!("create swapchain: {e}")));
            }
        };
        // Only now, with a swapchain in hand, is the old one actually gone.
        if let Some(old) = old {
            old.handed_over = true;
        }

        let images = match unsafe { swapchain_loader.get_swapchain_images(handle) } {
            Ok(images) if !images.is_empty() => images,
            Ok(_) => {
                unsafe { swapchain_loader.destroy_swapchain(handle, None) };
                return Err(RhiError::BackendError("swapchain reported zero images".into()));
            }
            Err(e) => {
                unsafe { swapchain_loader.destroy_swapchain(handle, None) };
                return Err(RhiError::BackendError(format!("swapchain images: {e}")));
            }
        };

        let mut image_views = Vec::with_capacity(images.len());
        for &image in &images {
            let view_info = vk::ImageViewCreateInfo {
                image,
                view_type: vk::ImageViewType::TYPE_2D,
                format,
                components: vk::ComponentMapping::default(),
                subresource_range: vk::ImageSubresourceRange {
                    aspect_mask: vk::ImageAspectFlags::COLOR,
                    base_mip_level: 0,
                    level_count: 1,
                    base_array_layer: 0,
                    layer_count: 1,
                },
                ..Default::default()
            };
            match unsafe { device.create_image_view(&view_info, None) } {
                Ok(view) => image_views.push(view),
                Err(e) => {
                    // Do not leak the views already created.
                    for made in &image_views {
                        unsafe { device.destroy_image_view(*made, None) };
                    }
                    unsafe { swapchain_loader.destroy_swapchain(handle, None) };
                    return Err(RhiError::BackendError(format!("swapchain image view: {e}")));
                }
            }
        }

        // A fence, not a semaphore, and not both.
        //
        // A fence passed to `vkAcquireNextImageKHR` is signalled *by the
        // acquire* once the image is ready, so it can only ever mean "the image
        // is here". It can never mean "the rendering finished", because nothing
        // ever signals it again afterwards. Passing a semaphore as well left a
        // semaphore that no submission waited on and a fence that was waited on
        // as if it tracked the render — the GPU was told to write into an image
        // the display engine might still be reading, which is what flickered.
        //
        // The render side does not need a fence from here: the RHI submit path
        // blocks on its own fence before returning, so the work is already
        // finished by the time `present` is called.
        let acquire_fence = match create_fence(&device) {
            Ok(f) => f,
            Err(e) => {
                unsafe { Self::cleanup(&device, &swapchain_loader, handle, &image_views, vk::Fence::null()) };
                return Err(e);
            }
        };

        Ok(Self {
            loader: swapchain_loader,
            surface: surface_loader,
            device,
            handle,
            images,
            image_views,
            format,
            color_space,
            extent,
            queue,
            acquire_fence,
            handed_over: false,
        })
    }

    /// Take the next image to draw into. `timeout` is nanoseconds;
    /// `u64::MAX` waits indefinitely.
    ///
    /// Returns only once the image is genuinely ready to be drawn into, so the
    /// caller may start recording without arranging any further waiting.
    pub fn acquire(&mut self, timeout: u64) -> RhiResult<AcquiredImage> {
        // Reset at acquire rather than at creation, so a second acquire after a
        // completed frame cannot observe an already-signalled fence.
        unsafe { self.device.reset_fences(&[self.acquire_fence]) }
            .map_err(|e| RhiError::BackendError(format!("reset acquire fence: {e}")))?;

        // `ash` reports SUBOPTIMAL as `Ok((index, true))`, but the
        // `Err(SUBOPTIMAL_KHR)` arm below is unreachable then, and the flag
        // carried no information a caller could act on differently. So the
        // suboptimal case is caught here by checking the flag ourselves, and
        // the returned value is treated as out-of-date.
        //
        // No semaphore: the wait is done on this thread below, which is what
        // lets the submit path stay a plain blocking submit. One frame is in
        // flight at a time, so there is no pipelining to preserve.
        let (index, suboptimal) = match unsafe {
            self.loader
                .acquire_next_image(self.handle, timeout, vk::Semaphore::null(), self.acquire_fence)
        } {
            Ok(pair) => pair,
            // A surface that is already out of date can refuse outright here
            // rather than handing back a suboptimal image. Both mean the same
            // thing to the caller, so both get the same error.
            Err(vk::Result::ERROR_OUT_OF_DATE_KHR) | Err(vk::Result::SUBOPTIMAL_KHR) => {
                return Err(RhiError::SwapchainOutOfDate)
            }
            Err(vk::Result::TIMEOUT) => {
                return Err(RhiError::BackendError("swapchain acquire timed out".into()))
            }
            Err(e) => return Err(RhiError::BackendError(format!("swapchain acquire: {e}"))),
        };

        if suboptimal {
            // Still safe to render into for one more frame, but the chain no
            // longer matches the surface, so tell the caller to rebuild rather
            // than letting it present a stale image.
            return Err(RhiError::SwapchainOutOfDate);
        }

        if index as usize >= self.image_views.len() {
            return Err(RhiError::BackendError(format!(
                "acquire returned image {index} but the swapchain has {} images",
                self.image_views.len()
            )));
        }

        // Wait here rather than handing a semaphore to a submission. The image
        // is not usable until this fence is signalled, and writing into it before
        // then is what produced torn frames.
        match unsafe { self.device.wait_for_fences(&[self.acquire_fence], true, timeout) } {
            Ok(()) => {}
            Err(vk::Result::TIMEOUT) => {
                return Err(RhiError::BackendError("swapchain acquire timed out".into()))
            }
            Err(e) => return Err(RhiError::BackendError(format!("wait acquire fence: {e}"))),
        }

        Ok(AcquiredImage {
            image_index: index,
            image_view: self.image_views[index as usize],
        })
    }

    /// Hand the finished image to the display.
    ///
    /// No wait semaphore is passed: with one frame in flight the CPU has
    /// already blocked on the render fence, so the image is complete.
    pub fn present(&self, image_index: u32) -> RhiResult<()> {
        if image_index as usize >= self.images.len() {
            return Err(RhiError::BackendError(format!(
                "present index {image_index} is outside the {} swapchain images",
                self.images.len()
            )));
        }
        let present = vk::PresentInfoKHR {
            swapchain_count: 1,
            p_swapchains: &self.handle,
            p_image_indices: &image_index,
            ..Default::default()
        };
        match unsafe { self.loader.queue_present(self.queue, &present) } {
            // `Ok(true)` means SUBOPTIMAL: the frame was shown but the chain
            // no longer matches the surface.
            Ok(_) => Ok(()),
            Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => Err(RhiError::SwapchainOutOfDate),
            Err(e) => Err(RhiError::BackendError(format!("queue present: {e}"))),
        }
    }

    /// The swapchain format expressed as an RHI format.
    ///
    /// A surface chooses the format, and a render pass has to be built for
    /// whatever it chose. `None` means the surface offered something the RHI
    /// cannot express, which is reported instead of being approximated — a
    /// near-miss format would produce a pass incompatible with the surface and a
    /// validation error at draw time rather than here.
    pub fn rhi_format(&self) -> Option<crate::types::Format> {
        crate::backend::vulkan::convert::format_from(self.format)
    }

    /// The `SurfaceCapabilitiesKHR` this chain was built against, kept so a
    /// rebuild can re-read the extent without re-querying from the caller.
    pub fn surface(&self) -> &SurfaceLoader {
        &self.surface
    }

    unsafe fn cleanup(
        device: &ash::Device,
        loader: &SwapchainLoader,
        handle: vk::SwapchainKHR,
        views: &[vk::ImageView],
        fence: vk::Fence,
    ) {
        for view in views {
            device.destroy_image_view(*view, None);
        }
        if fence != vk::Fence::null() {
            device.destroy_fence(fence, None);
        }
        device.device_wait_idle().ok();
        loader.destroy_swapchain(handle, None);
    }
}

impl Drop for Swapchain {
    fn drop(&mut self) {
        unsafe {
            self.device.device_wait_idle().ok();
            for view in &self.image_views {
                self.device.destroy_image_view(*view, None);
            }
            self.device.destroy_fence(self.acquire_fence, None);
            // Skipped when Vulkan already destroyed the handle during a
            // handover; destroying it twice is undefined.
            if !self.handed_over {
                self.loader.destroy_swapchain(self.handle, None);
            }
        }
    }
}

fn create_semaphore(device: &ash::Device) -> RhiResult<vk::Semaphore> {
    let info = vk::SemaphoreCreateInfo::default();
    unsafe { device.create_semaphore(&info, None) }
        .map_err(|e| RhiError::BackendError(format!("create semaphore: {e}")))
}

fn create_fence(device: &ash::Device) -> RhiResult<vk::Fence> {
    let info = vk::FenceCreateInfo::default();
    unsafe { device.create_fence(&info, None) }
        .map_err(|e| RhiError::BackendError(format!("create fence: {e}")))
}

fn pick_surface_format(
    surface: &SurfaceLoader,
    handle: vk::SurfaceKHR,
    physical_device: vk::PhysicalDevice,
) -> RhiResult<(vk::Format, vk::ColorSpaceKHR)> {
    let formats = unsafe { surface.get_physical_device_surface_formats(physical_device, handle) }
        .map_err(|e| RhiError::BackendError(format!("surface formats: {e}")))?;
    if formats.is_empty() {
        return Err(RhiError::BackendError("surface reports no supported formats".into()));
    }
    // B8G8R8A8 sRGB is what the desktop compositor wants, so it needs no
    // conversion on the way out.
    for f in &formats {
        if f.format == vk::Format::B8G8R8A8_SRGB
            && f.color_space == vk::ColorSpaceKHR::SRGB_NONLINEAR
        {
            return Ok((f.format, f.color_space));
        }
    }
    let only = formats[0];
    if formats.len() == 1 && only.format == vk::Format::UNDEFINED {
        return Err(RhiError::BackendError(
            "surface exposes no usable format (only VK_FORMAT_UNDEFINED)".into(),
        ));
    }
    Ok((only.format, only.color_space))
}

fn pick_extent(
    caps: &vk::SurfaceCapabilitiesKHR,
    desired_width: u32,
    desired_height: u32,
) -> RhiResult<vk::Extent2D> {
    // `current_extent == u32::MAX` is how a surface says "you choose".
    if caps.current_extent.width != u32::MAX {
        return Ok(caps.current_extent);
    }
    let clamp = |value: u32, lo: u32, hi: u32| {
        if lo > hi {
            value
        } else {
            value.max(lo).min(hi)
        }
    };
    Ok(vk::Extent2D {
        width: clamp(desired_width, caps.min_image_extent.width, caps.max_image_extent.width),
        height: clamp(desired_height, caps.min_image_extent.height, caps.max_image_extent.height),
    })
}

fn pick_present_mode(
    surface: &SurfaceLoader,
    handle: vk::SurfaceKHR,
    physical_device: vk::PhysicalDevice,
    preferred: vk::PresentModeKHR,
) -> RhiResult<vk::PresentModeKHR> {
    let modes = unsafe { surface.get_physical_device_surface_present_modes(physical_device, handle) }
        .map_err(|e| RhiError::BackendError(format!("present modes: {e}")))?;
    if modes.contains(&preferred) {
        return Ok(preferred);
    }
    // FIFO is the only mode the spec guarantees to be supported.
    if modes.contains(&vk::PresentModeKHR::FIFO) {
        return Ok(vk::PresentModeKHR::FIFO);
    }
    Err(RhiError::BackendError(
        "surface supports no present mode, not even the mandatory FIFO".into(),
    ))
}

fn pick_composite_alpha(caps: &vk::SurfaceCapabilitiesKHR) -> vk::CompositeAlphaFlagsKHR {
    [
        vk::CompositeAlphaFlagsKHR::OPAQUE,
        vk::CompositeAlphaFlagsKHR::PRE_MULTIPLIED,
        vk::CompositeAlphaFlagsKHR::POST_MULTIPLIED,
    ]
    .into_iter()
    .find(|a| caps.supported_composite_alpha.contains(*a))
    .unwrap_or(vk::CompositeAlphaFlagsKHR::OPAQUE)
}
