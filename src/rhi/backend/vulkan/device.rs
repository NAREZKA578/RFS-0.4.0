//! Vulkan logical device creation and backend resources.
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use crate::backend::common::BackendResource;
use crate::backend::vulkan::convert;
use crate::backend::vulkan::instance::{vk_fail, DestroyableInstance};
use crate::backend::vulkan::memory::MemoryAllocator;
use crate::backend::vulkan::pipeline_cache;
use crate::backend::vulkan::record;
use crate::core::{DeviceDesc, MemoryProperties, PhysicalDevice, Queue};
use crate::command::pass::render::{Framebuffer, FramebufferDesc, RenderPass, RenderPassDesc};
use crate::error::*;
use crate::descriptor::{
    DescriptorInfo, DescriptorSet, DescriptorSetLayout, DescriptorSetLayoutDesc, DescriptorWrite,
};
use crate::pipeline::compute::{ComputePipeline, ComputePipelineDesc};
use crate::pipeline::graphics::{GraphicsPipeline, GraphicsPipelineDesc};
use crate::resource::{
    Buffer, BufferDesc, GpuResource, Sampler, SamplerDesc, Texture, TextureDesc, TextureView,
    TextureViewDesc,
};
use crate::shader::module::{ShaderFormat, ShaderModule, ShaderModuleDesc};
use crate::types::{
    BufferUsage, ClearValue, Extent2D, GraphicsApi, IndexType, Offset2D, QueueFlags, Rect2D,
    SampleCount, SharingMode, Viewport,
};
use crate::resource::TextureLayout;
use ash::vk;
use ash::vk::Handle;
use std::ffi::{c_char, c_void, CStr, CString};
use std::sync::Arc;

pub const KHR_SWAPCHAIN: &CStr = c"VK_KHR_swapchain";
pub const KHR_MAINTENANCE_1: &CStr = c"VK_KHR_maintenance1";
pub const KHR_MAINTENANCE_4: &CStr = c"VK_KHR_maintenance4";

const DEVICE_EXTENSIONS: &[&CStr] = &[KHR_SWAPCHAIN, KHR_MAINTENANCE_1, KHR_MAINTENANCE_4];

pub(crate) fn supported_device_extensions(
    instance: &ash::Instance,
    physical: vk::PhysicalDevice,
) -> RhiResult<Vec<&'static CStr>> {
    let properties = unsafe { instance.enumerate_device_extension_properties(physical) }
        .map_err(|e| vk_fail(e, "enumerate device extensions"))?;
    Ok(DEVICE_EXTENSIONS
        .iter()
        .filter(|candidate| {
            properties
                .iter()
                .any(|p| unsafe { CStr::from_ptr(p.extension_name.as_ptr()) } == **candidate)
        })
        .copied()
        .collect())
}

pub(crate) struct LogicalDevice {
    pub device: ash::Device,
    pub queues: Vec<Queue>,
}

pub(crate) fn create_logical_device(
    instance: &ash::Instance,
    physical: vk::PhysicalDevice,
    physical_report: &PhysicalDevice,
    desc: &DeviceDesc,
    extensions: &[&CStr],
) -> RhiResult<LogicalDevice> {
    let requested_families: Vec<u32> = if desc.queue_family_indices.is_empty() {
        physical_report
            .get_queue_family(QueueFlags::GRAPHICS)
            .map(|family| vec![family.index])
            .unwrap_or_default()
    } else {
        desc.queue_family_indices.clone()
    };

    if requested_families.is_empty() {
        return Err(RhiError::DeviceCreationError(
            "no queue family with graphics support found on physical device".into(),
        ));
    }
    for &family in &requested_families {
        if !physical_report
            .queue_families
            .iter()
            .any(|q| q.index == family)
        {
            return Err(RhiError::DeviceCreationError(format!(
                "queue family {family} was not advertised by the physical device"
            )));
        }
    }

    let priority = [1.0f32];
    let queue_create_infos: Vec<vk::DeviceQueueCreateInfo> = requested_families
        .iter()
        .map(|&family| vk::DeviceQueueCreateInfo {
            queue_family_index: family,
            queue_count: 1,
            p_queue_priorities: priority.as_ptr(),
            ..Default::default()
        })
        .collect();

    let mut features = vk::PhysicalDeviceFeatures::default();
    features.geometry_shader = desc.features.geometry_shader as vk::Bool32;
    features.tessellation_shader = desc.features.tessellation_shader as vk::Bool32;
    features.shader_float64 = desc.features.shader_float64 as vk::Bool32;
    features.shader_int64 = desc.features.shader_int64 as vk::Bool32;
    features.multi_draw_indirect = desc.features.multi_draw_indirect as vk::Bool32;
    features.depth_bounds = desc.features.depth_bounds as vk::Bool32;
    features.depth_clamp = desc.features.depth_clamp as vk::Bool32;
    features.texture_compression_bc = desc.features.texture_compression_bc as vk::Bool32;
    features.texture_compression_astc_ldr = desc.features.texture_compression_astc as vk::Bool32;
    features.texture_compression_etc2 = desc.features.texture_compression_etc2 as vk::Bool32;
    features.sampler_anisotropy = desc.features.sampler_anisotropy as vk::Bool32;
    features.vertex_pipeline_stores_and_atomics = desc.features.storage_buffer as vk::Bool32;
    features.fragment_stores_and_atomics = desc.features.storage_image as vk::Bool32;
    features.sparse_binding = desc.features.sparse_binding as vk::Bool32;

    // Enable buffer device addresses (Vulkan 1.2 feature) so persistent
    // buffers can expose `VkDeviceAddress` to shaders.
    let vulkan_12 = vk::PhysicalDeviceVulkan12Features {
        buffer_device_address: desc.features.buffer_device_address as vk::Bool32,
        ..Default::default()
    };

    let extension_ptrs: Vec<*const c_char> = extensions.iter().map(|e| e.as_ptr()).collect();

    let create_info = vk::DeviceCreateInfo {
        queue_create_info_count: queue_create_infos.len() as u32,
        p_queue_create_infos: queue_create_infos.as_ptr(),
        enabled_extension_count: extension_ptrs.len() as u32,
        pp_enabled_extension_names: if extension_ptrs.is_empty() {
            std::ptr::null()
        } else {
            extension_ptrs.as_ptr()
        },
        p_enabled_features: &features,
        p_next: &vulkan_12 as *const vk::PhysicalDeviceVulkan12Features as *const c_void,
        ..Default::default()
    };

    let device = unsafe { instance.create_device(physical, &create_info, None) }
        .map_err(|e| RhiError::DeviceCreationError(format!("Vulkan device creation failed: {e:?}")))?;

    let queues: Vec<Queue> = requested_families
        .iter()
        .map(|&family| {
            let flags = physical_report
                .queue_families
                .iter()
                .find(|q| q.index == family)
                .map(|q| q.flags)
                .unwrap_or(QueueFlags::empty());
            Queue {
                family_index: family,
                index: 0,
                flags,
            }
        })
        .collect();

    Ok(LogicalDevice { device, queues })
}

pub(crate) struct VulkanLogicalDeviceResource {
    pub device: ash::Device,
    pub allocator: MemoryAllocator,
    /// Graphics queue family and the queue used for one-shot transfers.
    queue_family_index: u32,
    queue: vk::Queue,
    /// Kept because swapchain creation queries the physical device directly
    /// (surface capabilities, formats, present modes). The public
    /// `PhysicalDevice` report carries no native handle, so it would otherwise
    /// be unreachable by the time presentation is set up.
    physical_device: vk::PhysicalDevice,
    command_pool: vk::CommandPool,
    command_buffer: vk::CommandBuffer,
    /// Bug №177: guards `command_pool` and `command_buffer` for the whole
    /// record-and-submit window. See `submit_recorded`.
    submit_lock: std::sync::Mutex<()>,
    /// Pipeline cache for improved pipeline creation performance
    pipeline_cache: pipeline_cache::PipelineCache,
    /// Keeps the owning `VkInstance` alive for as long as this device lives.
    instance_guard: Arc<DestroyableInstance>,
}

impl VulkanLogicalDeviceResource {
    pub(crate) fn new(
        device: ash::Device,
        memory: &MemoryProperties,
        queue_family_index: u32,
        physical_device: vk::PhysicalDevice,
        instance_guard: Arc<DestroyableInstance>,
    ) -> RhiResult<Self> {
        let allocator = MemoryAllocator::new(device.clone(), memory);
        let queue = unsafe { device.get_device_queue(queue_family_index, 0) };
        let allocate_info = vk::CommandPoolCreateInfo {
            queue_family_index,
            ..Default::default()
        };
        // Both of these used to fall back to a null handle, so a device whose
        // pool or buffer could not be created still came up "successfully" and
        // then failed on every submit with an error that pointed at the wrong
        // thing. Failing here names the actual cause.
        let command_pool = unsafe { device.create_command_pool(&allocate_info, None) }
            .map_err(|e| vk_fail(e, "create command pool"))?;
        let allocate_info = vk::CommandBufferAllocateInfo {
            command_pool,
            level: vk::CommandBufferLevel::PRIMARY,
            command_buffer_count: 1,
            ..Default::default()
        };
        let command_buffer = unsafe { device.allocate_command_buffers(&allocate_info) }
            .map_err(|e| vk_fail(e, "allocate command buffer"))?
            .into_iter()
            .next()
            .ok_or_else(|| {
                RhiError::BackendError(
                    "command buffer allocation succeeded but returned no buffers".into(),
                )
            })?;
        let pipeline_cache = pipeline_cache::PipelineCache::new(device.clone())?;
        Ok(Self {
            device,
            allocator,
            queue_family_index,
            queue,
            physical_device,
            command_pool,
            command_buffer,
            submit_lock: std::sync::Mutex::new(()),
            pipeline_cache,
            instance_guard,
        })
    }

    /// Record and submit a one-shot transfer on the graphics queue and block
    /// until it has completed.
    ///
    /// Bug №177: this used a single `command_pool` and a single
    /// `command_buffer` shared by the whole device, with no lock. Two threads
    /// submitting at once would reset the pool out from under each other, hand
    /// the same buffer to two `vkBeginCommandBuffer` calls, and produce
    /// undefined behaviour rather than an error.
    ///
    /// The lock covers the whole record-and-wait window, not just the pool
    /// reset: the buffer is in use from `begin` to `end`, and reusing it while
    /// a previous submission is still in flight is the same hazard. One frame
    /// in flight today makes this unobservable, which is exactly why it was
    /// worth fixing now rather than after the race reports itself as visual
    /// corruption nobody can reproduce.
    fn submit_recorded(
        &self,
        record: impl FnOnce(vk::CommandBuffer) -> RhiResult<()>,
    ) -> RhiResult<()> {
        let _guard = self.submit_lock.lock();
        unsafe {
            self.device
                .reset_command_pool(self.command_pool, vk::CommandPoolResetFlags::empty())
                .map_err(|e| vk_fail(e, "reset command pool"))?;
            let begin = vk::CommandBufferBeginInfo::default();
            self.device
                .begin_command_buffer(self.command_buffer, &begin)
                .map_err(|e| vk_fail(e, "begin command buffer"))?;
            record(self.command_buffer)?;
            self.device
                .end_command_buffer(self.command_buffer)
                .map_err(|e| vk_fail(e, "end command buffer"))?;

            let fence = self
                .device
                .create_fence(&vk::FenceCreateInfo::default(), None)
                .map_err(|e| vk_fail(e, "create fence"))?;
            let submit = vk::SubmitInfo {
                command_buffer_count: 1,
                p_command_buffers: &self.command_buffer,
                ..Default::default()
            };
            self.device
                .queue_submit(self.queue, &[submit], fence)
                .map_err(|e| vk_fail(e, "queue submit"))?;
            self.device
                .wait_for_fences(&[fence], true, u64::MAX)
                .map_err(|e| vk_fail(e, "wait for fence"))?;
            self.device.destroy_fence(fence, None);
        }
        Ok(())
    }

    /// Create a host-visible, host-coherent staging buffer for `size` bytes.
    fn create_staging_buffer(
        &self,
        size: u64,
        usage: vk::BufferUsageFlags,
    ) -> RhiResult<(vk::Buffer, vk::DeviceMemory, *mut u8)> {
        let info = vk::BufferCreateInfo {
            size,
            usage,
            sharing_mode: vk::SharingMode::EXCLUSIVE,
            ..Default::default()
        };
        let buffer = unsafe { self.device.create_buffer(&info, None) }
            .map_err(|e| vk_fail(e, "create staging buffer"))?;
        let requirements = unsafe { self.device.get_buffer_memory_requirements(buffer) };
        let (memory, _) = self
            .allocator
            .allocate(
                requirements.size,
                requirements.memory_type_bits,
                // Host-visible only. Coherence used to be demanded here, which
                // made staging allocation fail outright on a device whose only
                // host-visible type is non-coherent; the write is flushed
                // instead, so the extra requirement bought nothing.
                crate::types::MemoryPropertyFlags::HOST_VISIBLE,
            )
            .map_err(|e| RhiError::BufferCreationError(format!("staging memory: {e}")))?;
        unsafe { self.device.bind_buffer_memory(buffer, memory, 0) }
            .map_err(|e| RhiError::BufferCreationError(format!("bind staging memory: {e:?}")))?;
        let ptr = self.allocator.map(memory, size)?;
        Ok((buffer, memory, ptr))
    }

    /// Release a staging buffer.
    ///
    /// The memory is unmapped first. It is still mapped here, because
    /// `create_staging_buffer` maps it and nothing ever unmapped it —
    /// freeing memory that is still mapped is a Vulkan validation error, and on
    /// some drivers it leaks the mapping as well. Buffer before memory, and
    /// only once the GPU is idle: `submit_recorded` already waits on a fence.
    fn destroy_staging_buffer(&self, buffer: vk::Buffer, memory: vk::DeviceMemory) {
        unsafe {
            self.device.destroy_buffer(buffer, None);
            self.allocator.unmap(memory);
            self.device.free_memory(memory, None);
        }
    }

    /// Bytes per texel for a format, used to build staging layouts.
    ///
    /// Bug №182: this used to carry its own hand-maintained table, and it had
    /// drifted: D16_UNORM was 4 (should be 2), RGBA16_* were 4 (should be 8),
    /// D32_SFLOAT_S8_UINT was 4 (should be 8) and S8_UINT was 4 (should be 1).
    /// Since this function sizes staging copies, each of those mistakes either
    /// under- or over-allocated a staging buffer and corrupted the upload.
    ///
    /// There is now a single source of truth — `pipeline::format_size`, fixed in
    /// №221 — so the two tables cannot disagree again.
    ///
    /// Returns `None` for block-compressed formats: their size depends on the
    /// mip extents, so a per-texel figure is meaningless and a staging layout
    /// built from one would be wrong. `mip_level_size` handles those.
    fn bytes_per_texel(format: crate::types::Format) -> Option<u32> {
        use crate::pipeline::{format_size, is_compressed_format};
        if is_compressed_format(format) {
            return None;
        }
        let bpp = format_size(format);
        if bpp == 0 {
            None
        } else {
            Some(bpp)
        }
    }
}

impl BackendResource for VulkanLogicalDeviceResource {
    fn as_raw(&self) -> *const () {
        self.device.handle().as_raw() as usize as *const ()
    }

    fn backend_type(&self) -> GraphicsApi {
        GraphicsApi::Vulkan
    }

    fn wait_idle(&self) -> RhiResult<()> {
        unsafe { self.device.device_wait_idle() }.map_err(|e| vk_fail(e, "device wait idle"))
    }

    fn create_buffer(&self, desc: &BufferDesc) -> RhiResult<Buffer> {
        if desc.sharing_mode == SharingMode::Concurrent {
            return Err(RhiError::NotSupported(
                "concurrent buffer sharing is not supported for buffer creation".into(),
            ));
        }
        let create_info = vk::BufferCreateInfo {
            size: desc.size,
            usage: convert::buffer_usage(desc.usage),
            sharing_mode: vk::SharingMode::EXCLUSIVE,
            ..Default::default()
        };
        let buffer = unsafe { self.device.create_buffer(&create_info, None) }
            .map_err(|e| vk_fail(e, "create buffer"))?;
        let requirements = unsafe { self.device.get_buffer_memory_requirements(buffer) };
        let (memory, _) = self
            .allocator
            .allocate(requirements.size, requirements.memory_type_bits, desc.memory_flags)
            .map_err(|e| RhiError::BufferCreationError(format!("{e}")))?;
        unsafe { self.device.bind_buffer_memory(buffer, memory, 0) }
            .map_err(|e| RhiError::BufferCreationError(format!("{e:?}")))?;

        let mut mapped = 0usize;
        if desc.is_host_mappable() {
            let ptr = self.allocator.map(memory, desc.size)?;
            mapped = ptr as usize;
        }

        let mut out = Buffer::from_desc(desc.clone());
        out.set_backend(GpuResource {
            handle: buffer.as_raw(),
            memory: memory.as_raw(),
            mapped,
        });

        if desc.usage.contains(BufferUsage::SHADER_DEVICE_ADDRESS) {
            let address_info = vk::BufferDeviceAddressInfo {
                buffer,
                ..Default::default()
            };
            let address = unsafe { self.device.get_buffer_device_address(&address_info) };
            if address != 0 {
                out.set_device_address(address);
            }
        }

        Ok(out)
    }

    fn create_texture(&self, desc: &TextureDesc) -> RhiResult<Texture> {
        let vk_format = convert::format(desc.format)
            .ok_or_else(|| RhiError::NotSupported(format!("unsupported format {:?}", desc.format)))?;
        if desc.sharing_mode == SharingMode::Concurrent {
            return Err(RhiError::NotSupported(
                "concurrent texture sharing is not supported for texture creation".into(),
            ));
        }
        let create_info = vk::ImageCreateInfo {
            image_type: convert::image_type(desc.dimensions),
            format: vk_format,
            extent: vk::Extent3D {
                width: desc.width,
                height: desc.height,
                depth: desc.depth,
            },
            mip_levels: desc.mip_levels,
            array_layers: desc.array_layers,
            samples: convert::sample_count(desc.sample_count),
            tiling: vk::ImageTiling::OPTIMAL,
            usage: convert::texture_usage(desc.usage),
            sharing_mode: vk::SharingMode::EXCLUSIVE,
            initial_layout: vk::ImageLayout::UNDEFINED,
            ..Default::default()
        };
        let image = unsafe { self.device.create_image(&create_info, None) }
            .map_err(|e| RhiError::TextureCreationError(format!("{e:?}")))?;
        let requirements = unsafe { self.device.get_image_memory_requirements(image) };
        // GPU-resident images use device-local memory; staging uploads later
        // go through an explicit host-visible staging buffer.
        let required = crate::types::MemoryPropertyFlags::DEVICE_LOCAL;
        let (memory, _) = self
            .allocator
            .allocate(requirements.size, requirements.memory_type_bits, required)
            .map_err(|e| RhiError::TextureCreationError(format!("{e}")))?;
        unsafe { self.device.bind_image_memory(image, memory, 0) }
            .map_err(|e| RhiError::TextureCreationError(format!("{e:?}")))?;

        let mut out = Texture::from_desc(desc.clone());
        out.set_backend(GpuResource {
            handle: image.as_raw(),
            memory: memory.as_raw(),
            mapped: 0,
        });
        Ok(out)
    }

    fn create_texture_view(&self, texture: &Texture, desc: &TextureViewDesc) -> RhiResult<TextureView> {
        let Some(gpu) = texture.backend() else {
            return Err(RhiError::BackendError(
                "texture has no GPU image backing".into(),
            ));
        };
        let format = desc
            .format
            .and_then(convert::format)
            .or_else(|| convert::format(texture.format()))
            .ok_or_else(|| RhiError::NotSupported("unsupported view format".into()))?;

        let range = vk::ImageSubresourceRange {
            aspect_mask: convert::image_aspects(desc.aspects),
            base_mip_level: desc.base_mip_level,
            level_count: desc.mip_level_count,
            base_array_layer: desc.base_array_layer,
            layer_count: desc.array_layer_count,
        };
        let create_info = vk::ImageViewCreateInfo {
            image: vk::Image::from_raw(gpu.handle),
            view_type: convert::image_view_type(desc.view_type),
            format,
            components: vk::ComponentMapping::default(),
            subresource_range: range,
            ..Default::default()
        };
        let view = unsafe { self.device.create_image_view(&create_info, None) }
            .map_err(|e| RhiError::TextureCreationError(format!("image view creation failed: {e:?}")))?;

        let mut out = TextureView::from_parts(texture.clone(), desc.clone());
        out.set_backend(GpuResource {
            handle: view.as_raw(),
            memory: 0,
            mapped: 0,
        });
        Ok(out)
    }

    fn create_sampler(&self, desc: &SamplerDesc) -> RhiResult<Sampler> {
        let create_info = vk::SamplerCreateInfo {
            mag_filter: convert::filter(desc.mag_filter),
            min_filter: convert::filter(desc.min_filter),
            mipmap_mode: convert::mipmap_mode(desc.mipmap_mode),
            address_mode_u: convert::address_mode(desc.address_mode_u),
            address_mode_v: convert::address_mode(desc.address_mode_v),
            address_mode_w: convert::address_mode(desc.address_mode_w),
            mip_lod_bias: desc.mip_lod_bias,
            anisotropy_enable: (desc.max_anisotropy > 0.0) as vk::Bool32,
            max_anisotropy: desc.max_anisotropy,
            compare_enable: desc.compare_enable as vk::Bool32,
            compare_op: convert::compare_op(desc.compare_op),
            min_lod: desc.min_lod,
            max_lod: desc.max_lod,
            border_color: convert::border_color(desc.border_color),
            unnormalized_coordinates: desc.unnormalized_coordinates as vk::Bool32,
            ..Default::default()
        };
        let sampler = unsafe { self.device.create_sampler(&create_info, None) }
            .map_err(|e| vk_fail(e, "create sampler"))?;
        let mut out = Sampler::from_desc(desc.clone());
        out.set_backend(GpuResource {
            handle: sampler.as_raw(),
            memory: 0,
            mapped: 0,
        });
        Ok(out)
    }

    fn upload_buffer(&self, buffer: &Buffer, data: &[u8]) -> RhiResult<()> {
        let Some(gpu) = buffer.backend() else {
            return Err(RhiError::BackendError("buffer has no GPU backing".into()));
        };
        if data.len() as u64 > buffer.size() {
            return Err(RhiError::BackendError(format!(
                "upload size {} exceeds buffer size {}",
                data.len(),
                buffer.size()
            )));
        }
        if gpu.mapped != 0 {
            unsafe {
                std::ptr::copy_nonoverlapping(data.as_ptr(), gpu.mapped as *mut u8, data.len());
            }
            // Without this the device may read the previous contents of the
            // range: fine on a coherent integrated heap, stale on a discrete
            // card, where host writes only reach the GPU after a flush.
            self.allocator
                .flush(vk::DeviceMemory::from_raw(gpu.memory), data.len() as u64)?;
            return Ok(());
        }

        // Device-local buffer: upload through a host-visible staging buffer.
        if !buffer.supports_transfer() {
            return Err(RhiError::BackendError(
                "buffer memory is not host mapped and the buffer lacks transfer usage".into(),
            ));
        }
        let (staging, staging_memory, ptr) = self.create_staging_buffer(data.len() as u64, vk::BufferUsageFlags::TRANSFER_SRC)?;
        unsafe {
            std::ptr::copy_nonoverlapping(data.as_ptr(), ptr, data.len());
        }
        self.allocator.flush(staging_memory, data.len() as u64)?;
        let dst = vk::Buffer::from_raw(gpu.handle);
        let result = self.submit_recorded(|cb| {
            unsafe {
                let region = vk::BufferCopy {
                    src_offset: 0,
                    dst_offset: 0,
                    size: data.len() as u64,
                };
                self.device.cmd_copy_buffer(cb, staging, dst, &[region]);
            }
            Ok(())
        });
        self.destroy_staging_buffer(staging, staging_memory);
        result
    }

    fn upload_texture(&self, texture: &Texture, data: &[u8]) -> RhiResult<()> {
        let Some(gpu) = texture.backend() else {
            return Err(RhiError::BackendError("texture has no GPU image backing".into()));
        };
        if texture.array_layers() > 1 {
            return Err(RhiError::NotSupported(
                "multi-layer texture upload is not supported yet".into(),
            ));
        }
        let Some(bpp) = Self::bytes_per_texel(texture.format()) else {
            return Err(RhiError::NotSupported(format!(
                "unsupported upload format {:?}",
                texture.format()
            )));
        };

        let mut regions = Vec::new();
        let mut total: u64 = 0;
        let mut aspects = crate::types::TextureAspectFlags::COLOR;
        if texture.format().is_depth() {
            aspects = crate::types::TextureAspectFlags::DEPTH;
        }
        if texture.format().is_stencil() {
            aspects |= crate::types::TextureAspectFlags::STENCIL;
        }
        let aspects = convert::image_aspects(aspects);
        for mip in 0..texture.mip_levels() {
            let (w, h, d) = texture.mip_size(mip);
            let row_pitch = align_up(w as u64 * bpp as u64, 4) as u32;
            regions.push(vk::BufferImageCopy {
                buffer_offset: total,
                buffer_row_length: row_pitch / bpp,
                buffer_image_height: 0,
                image_subresource: vk::ImageSubresourceLayers {
                    aspect_mask: aspects,
                    mip_level: mip,
                    base_array_layer: 0,
                    layer_count: 1,
                },
                image_offset: vk::Offset3D::default(),
                image_extent: vk::Extent3D { width: w, height: h, depth: d },
            });
            total += row_pitch as u64 * h as u64 * d as u64;
        }
        if total > data.len() as u64 {
            return Err(RhiError::BackendError(format!(
                "texture upload data too small: {} bytes, {total} needed for {:?} {}x{}x{} ({} mips)",
                data.len(),
                texture.format(),
                texture.width(),
                texture.height(),
                texture.depth(),
                texture.mip_levels()
            )));
        }

        let image = vk::Image::from_raw(gpu.handle);
        let range = vk::ImageSubresourceRange {
            aspect_mask: aspects,
            base_mip_level: 0,
            level_count: texture.mip_levels(),
            base_array_layer: 0,
            layer_count: texture.array_layers(),
        };
        let (staging, staging_memory, ptr) = self.create_staging_buffer(total, vk::BufferUsageFlags::TRANSFER_SRC)?;
        unsafe {
            std::ptr::copy_nonoverlapping(data.as_ptr(), ptr, total as usize);
        }
        self.allocator.flush(staging_memory, total)?;
        let sampled = texture.is_sampled();
        let result = self.submit_recorded(|cb| {
            unsafe {
                let to_transfer = vk::ImageMemoryBarrier {
                    src_access_mask: vk::AccessFlags::empty(),
                    dst_access_mask: vk::AccessFlags::TRANSFER_WRITE,
                    old_layout: vk::ImageLayout::UNDEFINED,
                    new_layout: vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    src_queue_family_index: vk::QUEUE_FAMILY_IGNORED,
                    dst_queue_family_index: vk::QUEUE_FAMILY_IGNORED,
                    image,
                    subresource_range: range,
                    ..Default::default()
                };
                self.device.cmd_pipeline_barrier(
                    cb,
                    vk::PipelineStageFlags::TOP_OF_PIPE,
                    vk::PipelineStageFlags::TRANSFER,
                    vk::DependencyFlags::empty(),
                    &[],
                    &[],
                    &[to_transfer],
                );
                self.device.cmd_copy_buffer_to_image(
                    cb,
                    staging,
                    image,
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    &regions,
                );
                if sampled {
                    let to_sampled = vk::ImageMemoryBarrier {
                        src_access_mask: vk::AccessFlags::TRANSFER_WRITE,
                        dst_access_mask: vk::AccessFlags::SHADER_READ,
                        old_layout: vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                        new_layout: vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                        src_queue_family_index: vk::QUEUE_FAMILY_IGNORED,
                        dst_queue_family_index: vk::QUEUE_FAMILY_IGNORED,
                        image,
                        subresource_range: range,
                        ..Default::default()
                    };
                    self.device.cmd_pipeline_barrier(
                        cb,
                        vk::PipelineStageFlags::TRANSFER,
                        vk::PipelineStageFlags::FRAGMENT_SHADER,
                        vk::DependencyFlags::empty(),
                        &[],
                        &[],
                        &[to_sampled],
                    );
                }
            }
            Ok(())
        });
        self.destroy_staging_buffer(staging, staging_memory);
        result
    }

    fn create_shader_module(&self, desc: &ShaderModuleDesc) -> RhiResult<ShaderModule> {
        if desc.format != ShaderFormat::SpirV {
            return Err(RhiError::NotSupported(format!(
                "unsupported shader format {:?}; only SpirV is accepted by the Vulkan backend",
                desc.format
            )));
        }
        if !desc.code.len().is_multiple_of(4) {
            return Err(RhiError::ShaderCompilationError(format!(
                "SPIR-V bytecode length {} is not a multiple of 4",
                desc.code.len()
            )));
        }
        let words: Vec<u32> = desc
            .code
            .as_chunks::<4>().0.iter()
            .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect();
        let create_info = vk::ShaderModuleCreateInfo {
            code_size: words.len() * 4,
            p_code: words.as_ptr(),
            ..Default::default()
        };
        let shader = unsafe { self.device.create_shader_module(&create_info, None) }
            .map_err(|e| vk_fail(e, "create shader module"))?;
        let mut out = ShaderModule::new(desc.clone());
        out.set_backend(GpuResource {
            handle: shader.as_raw(),
            memory: 0,
            mapped: 0,
        });
        Ok(out)
    }

    fn create_descriptor_set_layout(
        &self,
        desc: &DescriptorSetLayoutDesc,
    ) -> RhiResult<DescriptorSetLayout> {
        let bindings: Vec<vk::DescriptorSetLayoutBinding> = desc
            .bindings
            .iter()
            .map(|b| vk::DescriptorSetLayoutBinding {
                binding: b.binding,
                descriptor_type: convert::descriptor_type(b.ty),
                descriptor_count: b.count,
                stage_flags: convert::shader_stage_flags(b.stages),
                ..Default::default()
            })
            .collect();
        let create_info = vk::DescriptorSetLayoutCreateInfo {
            binding_count: bindings.len() as u32,
            p_bindings: bindings.as_ptr(),
            ..Default::default()
        };
        let layout = unsafe { self.device.create_descriptor_set_layout(&create_info, None) }
            .map_err(|e| vk_fail(e, "create descriptor set layout"))?;
        let mut out = DescriptorSetLayout::new(desc.clone());
        out.set_backend(GpuResource {
            handle: layout.as_raw(),
            memory: 0,
            mapped: 0,
        });
        Ok(out)
    }

    fn create_descriptor_set(&self, layout: &DescriptorSetLayout) -> RhiResult<DescriptorSet> {
        let Some(layout_backend) = layout.backend() else {
            return Err(RhiError::BackendError(
                "descriptor set layout has no GPU backing".into(),
            ));
        };
        let vk_layout = vk::DescriptorSetLayout::from_raw(layout_backend.handle);
        let mut sizes: Vec<vk::DescriptorPoolSize> = layout
            .bindings()
            .iter()
            .map(|b| vk::DescriptorPoolSize {
                ty: convert::descriptor_type(b.ty),
                descriptor_count: b.count,
            })
            .collect();
        if sizes.is_empty() {
            sizes.push(vk::DescriptorPoolSize {
                ty: vk::DescriptorType::UNIFORM_BUFFER,
                descriptor_count: 1,
            });
        }
        let pool_info = vk::DescriptorPoolCreateInfo {
            max_sets: 1,
            pool_size_count: sizes.len() as u32,
            p_pool_sizes: sizes.as_ptr(),
            ..Default::default()
        };
        let pool = unsafe { self.device.create_descriptor_pool(&pool_info, None) }
            .map_err(|e| vk_fail(e, "create descriptor pool"))?;
        let alloc_info = vk::DescriptorSetAllocateInfo {
            descriptor_pool: pool,
            descriptor_set_count: 1,
            p_set_layouts: &vk_layout,
            ..Default::default()
        };
        let vk_set = unsafe { self.device.allocate_descriptor_sets(&alloc_info) }
            .map_err(|e| vk_fail(e, "allocate descriptor set"))?
            .remove(0);

        let mut out = DescriptorSet::new(layout.clone());
        out.set_backend(GpuResource {
            handle: vk_set.as_raw(),
            memory: pool.as_raw(),
            mapped: 0,
        });
        Ok(out)
    }

    fn write_descriptors(&self, set: &DescriptorSet, writes: &[DescriptorWrite]) -> RhiResult<()> {
        let Some(set_backend) = set.backend() else {
            return Err(RhiError::BackendError("descriptor set has no GPU backing".into()));
        };
        let dst_set = vk::DescriptorSet::from_raw(set_backend.handle);
        let mut buffer_infos: Vec<vk::DescriptorBufferInfo> = Vec::new();
        let mut image_infos: Vec<vk::DescriptorImageInfo> = Vec::new();
        let mut planned: Vec<(u32, u32, vk::DescriptorType, usize, usize)> = Vec::new();

        for write in writes {
            let binding = set
                .layout()
                .binding(write.dst_binding)
                .ok_or_else(|| {
                    RhiError::BackendError(format!(
                        "descriptor binding {} not declared in layout",
                        write.dst_binding
                    ))
                })?;
            let ty = convert::descriptor_type(binding.ty);
            for (i, info) in write.descriptors.iter().enumerate() {
                let array_element = write.dst_array_element + i as u32;
                match info {
                    DescriptorInfo::Buffer(buffer, offset, size) => {
                        let Some(b) = buffer.backend() else {
                            return Err(RhiError::BackendError(
                                "descriptor buffer has no GPU backing".into(),
                            ));
                        };
                        buffer_infos.push(vk::DescriptorBufferInfo {
                            buffer: vk::Buffer::from_raw(b.handle),
                            offset: *offset,
                            range: *size,
                        });
                        planned.push((write.dst_binding, array_element, ty, buffer_infos.len() - 1, usize::MAX));
                    }
                    DescriptorInfo::Sampler(sampler) => {
                        let sampler_handle = sampler
                            .backend()
                            .ok_or_else(|| {
                                RhiError::BackendError(
                                    "descriptor sampler has no GPU backing".into(),
                                )
                            })?
                            .handle;
                        image_infos.push(vk::DescriptorImageInfo {
                            sampler: vk::Sampler::from_raw(sampler_handle),
                            image_view: vk::ImageView::null(),
                            image_layout: vk::ImageLayout::UNDEFINED,
                        });
                        planned.push((write.dst_binding, array_element, ty, usize::MAX, image_infos.len() - 1));
                    }
                    DescriptorInfo::Texture(view, sampler) => {
                        let Some(v) = view.backend() else {
                            return Err(RhiError::BackendError(
                                "descriptor texture view has no GPU backing".into(),
                            ));
                        };
                        let sampler_handle = if let Some(s) = sampler {
                            s.backend()
                                .ok_or_else(|| {
                                    RhiError::BackendError(
                                        "descriptor sampler has no GPU backing".into(),
                                    )
                                })?
                                .handle
                        } else {
                            0
                        };
                        image_infos.push(vk::DescriptorImageInfo {
                            sampler: vk::Sampler::from_raw(sampler_handle),
                            image_view: vk::ImageView::from_raw(v.handle),
                            image_layout: vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                        });
                        planned.push((write.dst_binding, array_element, ty, usize::MAX, image_infos.len() - 1));
                    }
                    DescriptorInfo::AccelerationStructure(_) => {
                        return Err(RhiError::NotSupported(
                            "acceleration structure descriptors are not supported yet".into(),
                        ));
                    }
                    DescriptorInfo::InlineUniformBlock(_) => {
                        return Err(RhiError::NotSupported(
                            "inline uniform block descriptors are not supported".into(),
                        ));
                    }
                }
            }
        }

        let vk_writes: Vec<vk::WriteDescriptorSet> = planned
            .iter()
            .map(|&(binding, array, ty, buf_idx, img_idx)| {
                let mut write = vk::WriteDescriptorSet {
                    dst_set,
                    dst_binding: binding,
                    dst_array_element: array,
                    descriptor_count: 1,
                    descriptor_type: ty,
                    ..Default::default()
                };
                if buf_idx != usize::MAX {
                    write.p_buffer_info = &buffer_infos[buf_idx] as *const _;
                } else if img_idx != usize::MAX {
                    write.p_image_info = &image_infos[img_idx] as *const _;
                }
                write
            })
            .collect();

        unsafe { self.device.update_descriptor_sets(&vk_writes, &[]) };
        Ok(())
    }

    fn create_compute_pipeline(
        &self,
        desc: &ComputePipelineDesc,
        layout: &DescriptorSetLayout,
    ) -> RhiResult<ComputePipeline> {
        let Some(shader_backend) = desc.shader_module().backend() else {
            return Err(RhiError::BackendError(
                "compute shader module has no GPU backing".into(),
            ));
        };
        let Some(layout_backend) = layout.backend() else {
            return Err(RhiError::BackendError(
                "descriptor set layout has no GPU backing".into(),
            ));
        };
        let entry_name = CString::new(desc.entry_point().as_bytes())
            .map_err(|_| RhiError::ShaderCompilationError("invalid entry point name".into()))?;
        let stage = vk::PipelineShaderStageCreateInfo {
            stage: vk::ShaderStageFlags::COMPUTE,
            module: vk::ShaderModule::from_raw(shader_backend.handle),
            p_name: entry_name.as_ptr(),
            ..Default::default()
        };
        let layout_handle = vk::DescriptorSetLayout::from_raw(layout_backend.handle);
        let layout_info = vk::PipelineLayoutCreateInfo {
            set_layout_count: 1,
            p_set_layouts: &layout_handle,
            ..Default::default()
        };
        let pipeline_layout = unsafe { self.device.create_pipeline_layout(&layout_info, None) }
            .map_err(|e| {
                RhiError::PipelineCreationError(format!("create pipeline layout: {e:?}"))
            })?;
        let create_info = vk::ComputePipelineCreateInfo {
            stage,
            layout: pipeline_layout,
            ..Default::default()
        };
        let mut pipeline = unsafe {
            self.device.create_compute_pipelines(
                self.pipeline_cache.handle(),
                &[create_info],
                None,
            )
        }
        .map_err(|e| RhiError::PipelineCreationError(format!("create compute pipeline: {e:?}")))?;
        let pipeline = pipeline.remove(0);

        let mut out = ComputePipeline::new(desc.clone());
        out.set_backend(GpuResource {
            handle: pipeline.as_raw(),
            memory: pipeline_layout.as_raw(),
            mapped: 0,
        });
        Ok(out)
    }

    fn dispatch_compute(
        &self,
        pipeline: &ComputePipeline,
        sets: &[&DescriptorSet],
        group_count_x: u32,
        group_count_y: u32,
        group_count_z: u32,
    ) -> RhiResult<()> {
        let Some(backend) = pipeline.backend() else {
            return Err(RhiError::BackendError("compute pipeline has no GPU backing".into()));
        };
        let vk_pipeline = vk::Pipeline::from_raw(backend.handle);
        let vk_layout = vk::PipelineLayout::from_raw(backend.memory);
        let vk_sets: Option<Vec<vk::DescriptorSet>> = sets
            .iter()
            .map(|s| {
                s.backend().map(|g| vk::DescriptorSet::from_raw(g.handle))
            })
            .collect();
        let vk_sets = vk_sets.ok_or_else(|| {
            RhiError::BackendError("bound descriptor set has no GPU backing".into())
        })?;

        self.submit_recorded(|cb| {
            unsafe {
                self.device
                    .cmd_bind_pipeline(cb, vk::PipelineBindPoint::COMPUTE, vk_pipeline);
                if !vk_sets.is_empty() {
                    self.device.cmd_bind_descriptor_sets(
                        cb,
                        vk::PipelineBindPoint::COMPUTE,
                        vk_layout,
                        0,
                        &vk_sets,
                        &[],
                    );
                }
                self.device.cmd_dispatch(cb, group_count_x, group_count_y, group_count_z);
                let barrier = vk::MemoryBarrier {
                    src_access_mask: vk::AccessFlags::SHADER_WRITE,
                    dst_access_mask: vk::AccessFlags::SHADER_READ
                        | vk::AccessFlags::TRANSFER_READ,
                    ..Default::default()
                };
                self.device.cmd_pipeline_barrier(
                    cb,
                    vk::PipelineStageFlags::COMPUTE_SHADER,
                    vk::PipelineStageFlags::COMPUTE_SHADER | vk::PipelineStageFlags::TRANSFER,
                    vk::DependencyFlags::empty(),
                    &[barrier],
                    &[],
                    &[],
                );
            }
            Ok(())
        })
    }

    fn invalidate_mapped_buffer(&self, buffer: &Buffer) -> RhiResult<()> {
        let Some(backend) = buffer.backend() else {
            return Ok(());
        };
        if backend.mapped == 0 {
            return Ok(());
        }
        self.allocator
            .invalidate(vk::DeviceMemory::from_raw(backend.memory), buffer.size())
    }

    fn download_buffer(&self, buffer: &Buffer, offset: u64, size: u64) -> RhiResult<Vec<u8>> {
        let Some(backend) = buffer.backend() else {
            return Err(RhiError::BackendError("buffer has no GPU backing".into()));
        };
        if size == 0 {
            return Ok(Vec::new());
        }
        if offset.checked_add(size).is_none_or(|end| end > buffer.size()) {
            return Err(RhiError::BackendError(format!(
                "download range {}..{} exceeds buffer size {}",
                offset,
                offset + size,
                buffer.size()
            )));
        }
        let src = vk::Buffer::from_raw(backend.handle);
        let (staging, staging_memory, ptr) = self.create_staging_buffer(size, vk::BufferUsageFlags::TRANSFER_DST)?;
        let result = self.submit_recorded(|cb| {
            unsafe {
                let barrier = vk::BufferMemoryBarrier {
                    src_access_mask: vk::AccessFlags::empty(),
                    dst_access_mask: vk::AccessFlags::TRANSFER_READ,
                    src_queue_family_index: vk::QUEUE_FAMILY_IGNORED,
                    dst_queue_family_index: vk::QUEUE_FAMILY_IGNORED,
                    buffer: src,
                    offset,
                    size,
                    ..Default::default()
                };
                self.device.cmd_pipeline_barrier(
                    cb,
                    vk::PipelineStageFlags::TOP_OF_PIPE,
                    vk::PipelineStageFlags::TRANSFER,
                    vk::DependencyFlags::empty(),
                    &[],
                    &[barrier],
                    &[],
                );
                let region = vk::BufferCopy {
                    src_offset: offset,
                    dst_offset: 0,
                    size,
                };
                self.device.cmd_copy_buffer(cb, src, staging, &[region]);
            }
            Ok(())
        });
        if let Err(e) = result {
            self.destroy_staging_buffer(staging, staging_memory);
            return Err(e);
        }
        // The device wrote this range; without an invalidate the host may read the
        // contents the range held before the copy.
        self.allocator.invalidate(staging_memory, size)?;
        let data = unsafe { std::slice::from_raw_parts(ptr, size as usize) }.to_vec();
        self.destroy_staging_buffer(staging, staging_memory);
        Ok(data)
    }

    fn download_texture(
        &self,
        texture: &Texture,
        mip_level: u32,
        array_layer: u32,
    ) -> RhiResult<Vec<u8>> {
        let Some(backend) = texture.backend() else {
            return Err(RhiError::BackendError("texture has no GPU backing".into()));
        };
        let bpp = Self::bytes_per_texel(texture.format()).ok_or_else(|| {
            RhiError::NotSupported(format!(
                "downloads for texture format {:?} are not supported",
                texture.format()
            ))
        })? as u64;
        let (width, height, depth) = texture.mip_size(mip_level);
        let row_pitch = align_up(width as u64 * bpp, 4);
        let total = row_pitch
            .checked_mul(height as u64)
            .and_then(|v| v.checked_mul(depth as u64))
            .ok_or_else(|| RhiError::BackendError("texture download size overflow".into()))?;
        if total == 0 {
            return Ok(Vec::new());
        }

        let image = vk::Image::from_raw(backend.handle);
        let (staging, staging_memory, ptr) =
            self.create_staging_buffer(total, vk::BufferUsageFlags::TRANSFER_DST)?;

        let old_layout = if texture.is_color_attachment() {
            vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL
        } else if texture.is_depth_stencil() {
            vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL
        } else {
            vk::ImageLayout::GENERAL
        };
        let src_access = if texture.is_color_attachment() {
            vk::AccessFlags::COLOR_ATTACHMENT_WRITE
        } else if texture.is_depth_stencil() {
            vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE
        } else {
            vk::AccessFlags::SHADER_WRITE
        };

        let result = self.submit_recorded(|cb| {
            unsafe {
                let barrier = vk::ImageMemoryBarrier {
                    src_access_mask: src_access,
                    dst_access_mask: vk::AccessFlags::TRANSFER_READ,
                    old_layout,
                    new_layout: vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                    src_queue_family_index: vk::QUEUE_FAMILY_IGNORED,
                    dst_queue_family_index: vk::QUEUE_FAMILY_IGNORED,
                    image,
                    subresource_range: vk::ImageSubresourceRange {
                        aspect_mask: if texture.is_depth_stencil() {
                            vk::ImageAspectFlags::DEPTH
                        } else {
                            vk::ImageAspectFlags::COLOR
                        },
                        base_mip_level: mip_level,
                        level_count: 1,
                        base_array_layer: array_layer,
                        layer_count: 1,
                    },
                    ..Default::default()
                };
                self.device.cmd_pipeline_barrier(
                    cb,
                    vk::PipelineStageFlags::ALL_COMMANDS,
                    vk::PipelineStageFlags::TRANSFER,
                    vk::DependencyFlags::empty(),
                    &[],
                    &[],
                    &[barrier],
                );
                let region = vk::BufferImageCopy {
                    buffer_offset: 0,
                    buffer_row_length: (row_pitch / bpp) as u32,
                    buffer_image_height: 0,
                    image_subresource: vk::ImageSubresourceLayers {
                        aspect_mask: if texture.is_depth_stencil() {
                            vk::ImageAspectFlags::DEPTH
                        } else {
                            vk::ImageAspectFlags::COLOR
                        },
                        mip_level,
                        base_array_layer: array_layer,
                        layer_count: 1,
                    },
                    image_offset: vk::Offset3D::default(),
                    image_extent: vk::Extent3D {
                        width,
                        height,
                        depth,
                    },
                };
                self.device.cmd_copy_image_to_buffer(
                    cb,
                    image,
                    vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                    staging,
                    &[region],
                );
            }
            Ok(())
        });
        if let Err(e) = result {
            self.destroy_staging_buffer(staging, staging_memory);
            return Err(e);
        }

        // Same reason as the buffer readback: the device wrote this range, so the host
        // must not read it before the write is made visible.
        self.allocator.invalidate(staging_memory, total)?;
        let raw = unsafe { std::slice::from_raw_parts(ptr, total as usize) };
        let tight_row = width as usize * bpp as usize;
        let mut out = Vec::with_capacity(tight_row * height as usize * depth as usize);
        for z in 0..depth as usize {
            for y in 0..height as usize {
                let start = (z * height as usize + y) * row_pitch as usize;
                out.extend_from_slice(&raw[start..start + tight_row]);
            }
        }
        self.destroy_staging_buffer(staging, staging_memory);
        Ok(out)
    }

    fn create_render_pass(&self, desc: &RenderPassDesc) -> RhiResult<RenderPass> {
        let attachments: Vec<vk::AttachmentDescription> = desc
            .attachments
            .iter()
            .map(|a| -> RhiResult<vk::AttachmentDescription> {
                Ok(vk::AttachmentDescription {
                    format: convert::format(a.format).ok_or_else(|| {
                        RhiError::NotSupported(format!(
                            "attachment format {:?} is not supported",
                            a.format
                        ))
                    })?,
                    samples: convert::sample_count(a.samples),
                    load_op: convert::attachment_load_op(a.load_op),
                    store_op: convert::attachment_store_op(a.store_op),
                    stencil_load_op: convert::attachment_load_op(a.stencil_load_op),
                    stencil_store_op: convert::attachment_store_op(a.stencil_store_op),
                    initial_layout: convert::texture_layout(a.initial_layout),
                    final_layout: convert::texture_layout(a.final_layout),
                    ..Default::default()
                })
            })
            .collect::<RhiResult<Vec<_>>>()?;

        struct SubpassRefs {
            input: Vec<vk::AttachmentReference>,
            color: Vec<vk::AttachmentReference>,
            resolve: Vec<vk::AttachmentReference>,
            depth: Option<vk::AttachmentReference>,
            preserve: Vec<u32>,
        }

        let refs: Vec<SubpassRefs> = desc
            .subpasses
            .iter()
            .map(|s| SubpassRefs {
                input: s
                    .input_attachments
                    .iter()
                    .map(|r| vk::AttachmentReference {
                        attachment: r.attachment,
                        layout: convert::texture_layout(r.layout),
                    })
                    .collect(),
                color: s
                    .color_attachments
                    .iter()
                    .map(|r| vk::AttachmentReference {
                        attachment: r.attachment,
                        layout: convert::texture_layout(r.layout),
                    })
                    .collect(),
                resolve: s
                    .resolve_attachments
                    .iter()
                    .map(|r| vk::AttachmentReference {
                        attachment: r.attachment,
                        layout: convert::texture_layout(r.layout),
                    })
                    .collect(),
                depth: s.depth_stencil_attachment.as_ref().map(|r| vk::AttachmentReference {
                    attachment: r.attachment,
                    layout: convert::texture_layout(r.layout),
                }),
                preserve: s.preserve_attachments.clone(),
            })
            .collect();

        let subpasses: Vec<vk::SubpassDescription> = desc
            .subpasses
            .iter()
            .zip(refs.iter())
            .map(|(s, r)| vk::SubpassDescription {
                flags: vk::SubpassDescriptionFlags::empty(),
                pipeline_bind_point: match s.pipeline_bind_point {
                    crate::command::pass::render::PipelineBindPoint::Graphics => {
                        vk::PipelineBindPoint::GRAPHICS
                    }
                    crate::command::pass::render::PipelineBindPoint::Compute => {
                        vk::PipelineBindPoint::COMPUTE
                    }
                },
                input_attachment_count: r.input.len() as u32,
                p_input_attachments: if r.input.is_empty() {
                    std::ptr::null()
                } else {
                    r.input.as_ptr()
                },
                color_attachment_count: r.color.len() as u32,
                p_color_attachments: if r.color.is_empty() {
                    std::ptr::null()
                } else {
                    r.color.as_ptr()
                },
                // The resolve attachment count is derived from the color
                // attachment count, so a null pointer must be used when there
                // are no resolve attachments.
                p_resolve_attachments: if r.resolve.is_empty() {
                    std::ptr::null()
                } else {
                    r.resolve.as_ptr()
                },
                p_depth_stencil_attachment: r
                    .depth
                    .as_ref()
                    .map(|d| d as *const vk::AttachmentReference)
                    .unwrap_or(std::ptr::null()),
                preserve_attachment_count: r.preserve.len() as u32,
                p_preserve_attachments: if r.preserve.is_empty() {
                    std::ptr::null()
                } else {
                    r.preserve.as_ptr()
                },
                ..Default::default()
            })
            .collect();

        let dependencies: Vec<vk::SubpassDependency> = desc
            .dependencies
            .iter()
            .map(|d| vk::SubpassDependency {
                src_subpass: d.src_subpass,
                dst_subpass: d.dst_subpass,
                src_stage_mask: convert::pipeline_stage(d.src_stage_mask),
                dst_stage_mask: convert::pipeline_stage(d.dst_stage_mask),
                src_access_mask: convert::access_flags(d.src_access_mask),
                dst_access_mask: convert::access_flags(d.dst_access_mask),
                dependency_flags: convert::dependency_flags(d.dependency_flags),
            })
            .collect();

        let create_info = vk::RenderPassCreateInfo {
            attachment_count: attachments.len() as u32,
            p_attachments: attachments.as_ptr(),
            subpass_count: subpasses.len() as u32,
            p_subpasses: subpasses.as_ptr(),
            dependency_count: dependencies.len() as u32,
            p_dependencies: dependencies.as_ptr(),
            ..Default::default()
        };
        let render_pass = unsafe { self.device.create_render_pass(&create_info, None) }
            .map_err(|e| vk_fail(e, "create render pass"))?;

        let mut out = RenderPass::new(desc.clone());
        out.set_backend(GpuResource {
            handle: render_pass.as_raw(),
            memory: 0,
            mapped: 0,
        });
        Ok(out)
    }

    fn create_framebuffer(&self, desc: &FramebufferDesc) -> RhiResult<Framebuffer> {
        let Some(render_pass) = desc.render_pass.backend() else {
            return Err(RhiError::BackendError(
                "framebuffer render pass has no GPU backing".into(),
            ));
        };
        let views: Vec<vk::ImageView> = desc
            .attachments
            .iter()
            .map(|a| {
                a.texture_view
                    .backend()
                    .map(|g| vk::ImageView::from_raw(g.handle))
                    .ok_or_else(|| {
                        RhiError::BackendError("framebuffer attachment has no GPU backing".into())
                    })
            })
            .collect::<RhiResult<Vec<_>>>()?;

        let create_info = vk::FramebufferCreateInfo {
            render_pass: vk::RenderPass::from_raw(render_pass.handle),
            attachment_count: views.len() as u32,
            p_attachments: views.as_ptr(),
            width: desc.width.max(1),
            height: desc.height.max(1),
            layers: desc.layers.max(1),
            ..Default::default()
        };
        let framebuffer = unsafe { self.device.create_framebuffer(&create_info, None) }
            .map_err(|e| vk_fail(e, "create framebuffer"))?;

        let mut out = Framebuffer::new(desc.clone());
        out.set_backend(GpuResource {
            handle: framebuffer.as_raw(),
            memory: 0,
            mapped: 0,
        });
        Ok(out)
    }

    fn create_graphics_pipeline(
        &self,
        desc: &GraphicsPipelineDesc,
        render_pass: &RenderPass,
        set_layouts: &[&DescriptorSetLayout],
    ) -> RhiResult<GraphicsPipeline> {
        let Some(render_pass_backend) = render_pass.backend() else {
            return Err(RhiError::BackendError(
                "graphics pipeline render pass has no GPU backing".into(),
            ));
        };

        let mut module_handles = Vec::with_capacity(desc.shader_stages.len());
        let mut entry_names = Vec::with_capacity(desc.shader_stages.len());
        for stage in &desc.shader_stages {
            let Some(module) = stage.module.backend() else {
                return Err(RhiError::BackendError(
                    "pipeline shader module has no GPU backing".into(),
                ));
            };
            module_handles.push(vk::ShaderModule::from_raw(module.handle));
            entry_names.push(
                CString::new(stage.entry_point.as_bytes()).map_err(|_| {
                    RhiError::ShaderCompilationError("invalid entry point name".into())
                })?,
            );
        }
        let stages: Vec<vk::PipelineShaderStageCreateInfo> = desc
            .shader_stages
            .iter()
            .enumerate()
            .map(|(i, stage)| vk::PipelineShaderStageCreateInfo {
                stage: convert::shader_stage_flags(stage.stage),
                module: module_handles[i],
                p_name: entry_names[i].as_ptr(),
                ..Default::default()
            })
            .collect();

        let (bindings, attributes) = match &desc.vertex_input_state {
            Some(state) => {
                let bindings: Vec<vk::VertexInputBindingDescription> = state
                    .bindings
                    .iter()
                    .map(|b| vk::VertexInputBindingDescription {
                        binding: b.binding,
                        stride: b.stride,
                        input_rate: convert::vertex_input_rate(b.input_rate),
                    })
                    .collect();
                let attributes: Vec<vk::VertexInputAttributeDescription> = state
                    .attributes
                    .iter()
                    .map(|a| -> RhiResult<vk::VertexInputAttributeDescription> {
                        Ok(vk::VertexInputAttributeDescription {
                            location: a.location,
                            binding: a.binding,
                            format: convert::format(a.format).ok_or_else(|| {
                                RhiError::NotSupported(format!(
                                    "vertex attribute format {:?} is not supported",
                                    a.format
                                ))
                            })?,
                            offset: a.offset,
                        })
                    })
                    .collect::<RhiResult<Vec<_>>>()?;
                (bindings, attributes)
            }
            None => (Vec::new(), Vec::new()),
        };
        let vertex_input = vk::PipelineVertexInputStateCreateInfo {
            vertex_binding_description_count: bindings.len() as u32,
            p_vertex_binding_descriptions: bindings.as_ptr(),
            vertex_attribute_description_count: attributes.len() as u32,
            p_vertex_attribute_descriptions: attributes.as_ptr(),
            ..Default::default()
        };

        let input_assembly = vk::PipelineInputAssemblyStateCreateInfo {
            topology: convert::topology(desc.input_assembly_state.topology),
            primitive_restart_enable: desc.input_assembly_state.primitive_restart_enable as u32,
            ..Default::default()
        };

        let viewport_state = vk::PipelineViewportStateCreateInfo {
            viewport_count: 1,
            scissor_count: 1,
            ..Default::default()
        };

        let raster = desc.rasterizer_state.clone().unwrap_or_default();
        let rasterizer = vk::PipelineRasterizationStateCreateInfo {
            depth_clamp_enable: raster.depth_clamp_enable as u32,
            rasterizer_discard_enable: raster.rasterizer_discard_enable as u32,
            polygon_mode: convert::polygon_mode(raster.polygon_mode),
            cull_mode: convert::cull_mode(raster.cull_mode),
            front_face: convert::front_face(raster.front_face),
            depth_bias_enable: raster.depth_bias_enable as u32,
            depth_bias_constant_factor: raster.depth_bias_constant,
            depth_bias_clamp: raster.depth_bias_clamp,
            depth_bias_slope_factor: raster.depth_bias_slope,
            line_width: if raster.line_width > 0.0 {
                raster.line_width
            } else {
                1.0
            },
            ..Default::default()
        };

        let multisample = vk::PipelineMultisampleStateCreateInfo {
            rasterization_samples: convert::sample_count(desc.multisample_state.sample_count),
            alpha_to_coverage_enable: desc.multisample_state.alpha_to_coverage_enable as u32,
            alpha_to_one_enable: desc.multisample_state.alpha_to_one_enable as u32,
            ..Default::default()
        };

        let depth_stencil = desc.depth_stencil_state.as_ref().map(|d| {
            vk::PipelineDepthStencilStateCreateInfo {
                depth_test_enable: d.depth_test_enable as u32,
                depth_write_enable: d.depth_write_enable as u32,
                depth_compare_op: convert::compare_op(d.depth_compare_op),
                depth_bounds_test_enable: d.depth_bounds_test_enable as u32,
                min_depth_bounds: d.min_depth_bounds,
                max_depth_bounds: d.max_depth_bounds,
                stencil_test_enable: d.stencil_test_enable as u32,
                front: convert::stencil_op_state(&d.front),
                back: convert::stencil_op_state(&d.back),
                ..Default::default()
            }
        });

        let blend_attachments: Vec<vk::PipelineColorBlendAttachmentState> = desc
            .color_blend_state
            .as_ref()
            .map(|c| {
                c.attachments
                    .iter()
                    .map(|a| vk::PipelineColorBlendAttachmentState {
                        blend_enable: a.blend_enable as u32,
                        src_color_blend_factor: convert::blend_factor(a.src_color_blend_factor),
                        dst_color_blend_factor: convert::blend_factor(a.dst_color_blend_factor),
                        color_blend_op: convert::blend_op(a.color_blend_op),
                        src_alpha_blend_factor: convert::blend_factor(a.src_alpha_blend_factor),
                        dst_alpha_blend_factor: convert::blend_factor(a.dst_alpha_blend_factor),
                        alpha_blend_op: convert::blend_op(a.alpha_blend_op),
                        color_write_mask: convert::color_components(a.color_write_mask),
                    })
                    .collect()
            })
            .unwrap_or_default();
        let blend = desc.color_blend_state.clone().unwrap_or_default();
        let color_blend = vk::PipelineColorBlendStateCreateInfo {
            logic_op_enable: blend.logic_op_enable as u32,
            logic_op: convert::logic_op(blend.logic_op),
            attachment_count: blend_attachments.len() as u32,
            p_attachments: blend_attachments.as_ptr(),
            blend_constants: blend.blend_constants,
            ..Default::default()
        };

        let dynamic_states = [vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];
        let dynamic_state = vk::PipelineDynamicStateCreateInfo {
            dynamic_state_count: dynamic_states.len() as u32,
            p_dynamic_states: dynamic_states.as_ptr(),
            ..Default::default()
        };

        let vk_layouts: Vec<vk::DescriptorSetLayout> = set_layouts
            .iter()
            .map(|l| {
                l.backend()
                    .map(|g| vk::DescriptorSetLayout::from_raw(g.handle))
                    .ok_or_else(|| {
                        RhiError::BackendError("pipeline set layout has no GPU backing".into())
                    })
            })
            .collect::<RhiResult<Vec<_>>>()?;
        let layout_info = vk::PipelineLayoutCreateInfo {
            set_layout_count: vk_layouts.len() as u32,
            p_set_layouts: vk_layouts.as_ptr(),
            ..Default::default()
        };
        let pipeline_layout = unsafe { self.device.create_pipeline_layout(&layout_info, None) }
            .map_err(|e| {
                RhiError::PipelineCreationError(format!("create pipeline layout: {e:?}"))
            })?;

        let create_info = vk::GraphicsPipelineCreateInfo {
            stage_count: stages.len() as u32,
            p_stages: stages.as_ptr(),
            p_vertex_input_state: &vertex_input,
            p_input_assembly_state: &input_assembly,
            p_viewport_state: &viewport_state,
            p_rasterization_state: &rasterizer,
            p_multisample_state: &multisample,
            p_depth_stencil_state: depth_stencil
                .as_ref()
                .map(|d| d as *const vk::PipelineDepthStencilStateCreateInfo)
                .unwrap_or(std::ptr::null()),
            p_color_blend_state: &color_blend,
            p_dynamic_state: &dynamic_state,
            layout: pipeline_layout,
            render_pass: vk::RenderPass::from_raw(render_pass_backend.handle),
            subpass: 0,
            ..Default::default()
        };

        let mut pipelines = unsafe {
            self.device.create_graphics_pipelines(
                self.pipeline_cache.handle(),
                &[create_info],
                None,
            )
        }
        .map_err(|e| RhiError::PipelineCreationError(format!("create graphics pipeline: {e:?}")))?;
        let pipeline = pipelines.remove(0);

        let mut out = GraphicsPipeline::new(desc.clone());
        out.set_backend(GpuResource {
            handle: pipeline.as_raw(),
            memory: pipeline_layout.as_raw(),
            mapped: 0,
        });
        Ok(out)
    }

    fn create_window_surface(
        &self,
        hinstance: u64,
        hwnd: u64,
    ) -> RhiResult<crate::backend::vulkan::present::WindowSurface> {
        crate::backend::vulkan::WindowSurface::new_win32(
            &self.instance_guard,
            hinstance as vk::HINSTANCE,
            hwnd as vk::HWND,
        )
    }

    fn create_swapchain(
        &self,
        surface: &crate::backend::vulkan::WindowSurface,
        request: crate::backend::vulkan::SwapchainRequest,
        old: Option<&mut crate::backend::vulkan::Swapchain>,
    ) -> RhiResult<crate::backend::vulkan::Swapchain> {
        let queue = unsafe { self.device.get_device_queue(self.queue_family_index, 0) };
        crate::backend::vulkan::Swapchain::create(
            &self.instance_guard,
            self.device.clone(),
            surface,
            self.physical_device,
            queue,
            request,
            old,
        )
    }

    fn presentation_render_pass_desc(
        &self,
        format: crate::types::Format,
    ) -> RhiResult<crate::command::pass::render::RenderPassDesc> {
        // Identical in shape to any single-colour-attachment pass; it exists
        // here so the render pass always matches the format the *surface*
        // actually gave us, which is negotiated and not chosen by us.
        Ok(crate::command::pass::render::RenderPassDesc {
            attachments: vec![crate::command::pass::render::AttachmentDescription {
                format,
                samples: SampleCount::X1,
                load_op: crate::command::pass::render::LoadOp::Clear,
                store_op: crate::command::pass::render::StoreOp::Store,
                stencil_load_op: crate::command::pass::render::LoadOp::DontCare,
                stencil_store_op: crate::command::pass::render::StoreOp::DontCare,
                initial_layout: TextureLayout::Undefined,
                // `PresentSrc`, not `ColorAttachmentOptimal`.
                //
                // This is what the pass is for: the image has to end the frame
                // in a layout the presentation engine can read. Leaving it as a
                // colour attachment hands `vkQueuePresentKHR` an image in a
                // layout it must not present from. That is a validation
                // violation, and what the compositor then shows is undefined —
                // in practice a violently flickering window, which is the
                // symptom that finally made this visible.
                final_layout: TextureLayout::PresentSrc,
            }],
            subpasses: vec![crate::command::pass::render::SubpassDescription {
                pipeline_bind_point: crate::command::pass::render::PipelineBindPoint::Graphics,
                color_attachments: vec![crate::command::pass::render::AttachmentReference {
                    attachment: 0,
                    layout: TextureLayout::ColorAttachmentOptimal,
                }],
                ..Default::default()
            }],
            dependencies: vec![],
        })
    }

    fn presentation_render_pass_desc_with_depth(
        &self,
        format: crate::types::Format,
        depth_format: crate::types::Format,
    ) -> RhiResult<crate::command::pass::render::RenderPassDesc> {
        // Same colour attachment as the depth-less version, plus the depth one.
        // The depth attachment is *not* cleared by load-op alone: a depth
        // attachment with a load operation and no clear value is invalid, so
        // `clear` is mandatory and the value must reach the CPU.
        let mut desc = self.presentation_render_pass_desc(format)?;
        desc.attachments.push(crate::command::pass::render::AttachmentDescription {
            format: depth_format,
            samples: SampleCount::X1,
            load_op: crate::command::pass::render::LoadOp::Clear,
            store_op: crate::command::pass::render::StoreOp::Store,
            stencil_load_op: crate::command::pass::render::LoadOp::DontCare,
            stencil_store_op: crate::command::pass::render::StoreOp::DontCare,
            initial_layout: TextureLayout::Undefined,
            // Stays a depth attachment: nothing else reads it, and the next
            // frame clears it again anyway.
            final_layout: TextureLayout::DepthStencilAttachmentOptimal,
        });
        desc.subpasses[0].depth_stencil_attachment =
            Some(crate::command::pass::render::AttachmentReference {
                attachment: 1,
                layout: TextureLayout::DepthStencilAttachmentOptimal,
            });
        Ok(desc)
    }

    fn submit_commands(&self, commands: &[crate::command::commands::Command]) -> RhiResult<()> {
        if commands.is_empty() {
            return Err(RhiError::BackendError(
                "refusing to submit an empty command list: the recording produced nothing, \
                 which almost always means the encoder dropped the commands"
                    .into(),
            ));
        }
        self.submit_recorded(|cb| record::record(&self.device, cb, commands))
    }

    fn draw_framebuffer_indexed(
        &self,
        framebuffer: &Framebuffer,
        pipeline: &GraphicsPipeline,
        sets: &[&DescriptorSet],
        vertex_buffer: Option<(&Buffer, u32)>,
        index_buffer: Option<(&Buffer, u32)>,
        index_type: IndexType,
        vertex_count: u32,
        first_vertex: u32,
        index_count: u32,
        first_index: u32,
        vertex_offset: i32,
        clear_values: &[ClearValue],
    ) -> RhiResult<()> {
        // Bug №203/№248: this used to inline every `cmd_*` call here, which
        // left `CommandEncoder` recording a list nothing could replay — the RHI
        // command path was decorative and this function was the only real
        // implementation. It now builds the command list and hands it to the
        // same translator any other caller would use, so there is one
        // implementation rather than two that can drift.
        let width = framebuffer.desc().width.max(1);
        let height = framebuffer.desc().height.max(1);

        let mut commands: Vec<crate::command::commands::Command> =
            Vec::with_capacity(8 + usize::from(index_buffer.is_some()));

        commands.push(crate::command::commands::Command::BeginRenderPass(
            crate::command::pass::render::RenderPassBeginInfo {
                render_pass: framebuffer.desc().render_pass.clone(),
                framebuffer: framebuffer.clone(),
                render_area: Rect2D {
                    offset: Offset2D { x: 0, y: 0 },
                    extent: Extent2D { width, height },
                },
                clear_values: clear_values.to_vec(),
            },
        ));

        commands.push(crate::command::commands::Command::BindGraphicsPipeline(
            pipeline.clone(),
        ));

        commands.push(crate::command::commands::Command::SetViewport(Viewport {
            x: 0.0,
            y: 0.0,
            width: width as f32,
            height: height as f32,
            min_depth: 0.0,
            max_depth: 1.0,
        }));
        commands.push(crate::command::commands::Command::SetScissor(Rect2D {
            offset: Offset2D { x: 0, y: 0 },
            extent: Extent2D { width, height },
        }));

        if !sets.is_empty() {
            commands.push(crate::command::commands::Command::BindDescriptorSets {
                pipeline: pipeline.clone(),
                first_set: 0,
                sets: sets.iter().map(|s| (*s).clone()).collect(),
            });
        }

        if let Some((buffer, offset)) = vertex_buffer {
            commands.push(crate::command::commands::Command::BindVertexBuffers {
                first_binding: 0,
                buffers: vec![(buffer.clone(), offset as u64)],
            });
        }
        if let Some((buffer, offset)) = index_buffer {
            commands.push(crate::command::commands::Command::BindIndexBuffer {
                buffer: buffer.clone(),
                offset: offset as u64,
                index_type,
            });
        }

        if index_buffer.is_some() {
            commands.push(crate::command::commands::Command::DrawIndexed {
                index_count,
                instance_count: 1,
                first_index,
                vertex_offset,
                first_instance: 0,
            });
        } else {
            commands.push(crate::command::commands::Command::Draw {
                vertex_count,
                instance_count: 1,
                first_vertex,
                first_instance: 0,
            });
        }

        commands.push(crate::command::commands::Command::EndRenderPass);

        self.submit_recorded(|cb| record::record(&self.device, cb, &commands))
    }
}

impl Drop for VulkanLogicalDeviceResource {
    fn drop(&mut self) {
        unsafe {
            self.device.device_wait_idle().ok();
            self.device.destroy_command_pool(self.command_pool, None);
            self.device.destroy_device(None);
        }
    }
}

fn align_up(value: u64, align: u64) -> u64 {
    debug_assert!(align.is_power_of_two());
    (value + (align - 1)) & !(align - 1)
}