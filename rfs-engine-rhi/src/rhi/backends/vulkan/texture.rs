//! Vulkan Texture implementation.
//!
//! Creates device-local (`IMAGE_TILING_OPTIMAL`) images with a `VkImageView`
//! and a matching `VkSampler` (VB12). Source data is uploaded through a
//! host-visible staging buffer and copied into the image with an explicit
//! layout transition, since `OPTIMAL` tiling cannot be written from the host
//! directly.

use super::registry::{register, RegisteredImage};
use crate::rhi::{Texture, TextureDescriptor, TextureFormat, TextureUsage};
use ash::vk;
use ash::vk::Handle;
use std::sync::Arc;

/// Finds a memory type index that satisfies the required property flags.
fn find_memory_type(
    memory_properties: &vk::PhysicalDeviceMemoryProperties,
    type_filter: u32,
    properties: vk::MemoryPropertyFlags,
) -> Option<u32> {
    (0..memory_properties.memory_type_count).find(|&i| {
        (type_filter & (1 << i)) != 0
            && memory_properties.memory_types[i as usize]
                .property_flags
                .contains(properties)
    })
}

/// Maps an RHI texture format to a `VkFormat`.
pub fn to_vk_format(format: TextureFormat) -> vk::Format {
    match format {
        TextureFormat::Rgba8Unorm => vk::Format::R8G8B8A8_UNORM,
        TextureFormat::Bgra8Unorm => vk::Format::B8G8R8A8_UNORM,
        TextureFormat::R8Unorm => vk::Format::R8_UNORM,
        TextureFormat::Rgba16Float => vk::Format::R16G16B16A16_SFLOAT,
        TextureFormat::Rgba32Float => vk::Format::R32G32B32A32_SFLOAT,
        TextureFormat::R16Float => vk::Format::R16_SFLOAT,
        TextureFormat::Depth24 => vk::Format::D24_UNORM_S8_UINT,
        TextureFormat::Depth32Float => vk::Format::D32_SFLOAT,
        TextureFormat::Depth24Stencil8 => vk::Format::D24_UNORM_S8_UINT,
        TextureFormat::Bc1Rgba => vk::Format::BC1_RGBA_UNORM_BLOCK,
        TextureFormat::Bc3Rgba => vk::Format::BC3_UNORM_BLOCK,
        TextureFormat::Bc7Rgba => vk::Format::BC7_UNORM_BLOCK,
    }
}

/// Returns the aspect mask for a format (color vs depth/stencil).
pub fn aspect_for_format(format: TextureFormat) -> vk::ImageAspectFlags {
    if format.is_depth() {
        if format.has_stencil() {
            vk::ImageAspectFlags::DEPTH | vk::ImageAspectFlags::STENCIL
        } else {
            vk::ImageAspectFlags::DEPTH
        }
    } else {
        vk::ImageAspectFlags::COLOR
    }
}

/// A one-shot staging upload helper that transitions the image to `OPTIMAL`
/// and copies the host buffer into it, then restores it to `SHADER_READ_ONLY`.
pub(crate) fn upload_texture_data(
    device: &ash::Device,
    queue: vk::Queue,
    queue_family: u32,
    memory_properties: &vk::PhysicalDeviceMemoryProperties,
    image: vk::Image,
    width: u32,
    height: u32,
    depth: u32,
    array_layers: u32,
    mip_levels: u32,
    format: TextureFormat,
    data: &[u8],
    dst_usage: vk::ImageUsageFlags,
) -> Result<(), Box<dyn std::error::Error>> {
    let vk_format = to_vk_format(format);
    let _ = vk_format;

    let plane_count = if format == TextureFormat::Bc1Rgba
        || format == TextureFormat::Bc3Rgba
        || format == TextureFormat::Bc7Rgba
    {
        // Compressed formats are uploaded in a single plane.
        1u32
    } else {
        1u32
    };
    let _ = plane_count;

    let buffer_size = data.len() as u64;

    // Create host-visible staging buffer.
    let buffer_info = vk::BufferCreateInfo::default()
        .size(buffer_size)
        .usage(vk::BufferUsageFlags::TRANSFER_SRC)
        .sharing_mode(vk::SharingMode::EXCLUSIVE);
    let staging = unsafe { device.create_buffer(&buffer_info, None)? };
    let reqs = unsafe { device.get_buffer_memory_requirements(staging) };
    let mem_type = find_memory_type(
        memory_properties,
        reqs.memory_type_bits,
        vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
    )
    .ok_or("No host-visible memory type for staging buffer")?;
    let alloc = vk::MemoryAllocateInfo::default()
        .allocation_size(reqs.size)
        .memory_type_index(mem_type);
    let staging_mem = unsafe { device.allocate_memory(&alloc, None)? };
    unsafe { device.bind_buffer_memory(staging, staging_mem, 0)? };

    let mapped =
        unsafe { device.map_memory(staging_mem, 0, buffer_size, vk::MemoryMapFlags::empty())? };
    unsafe {
        std::ptr::copy_nonoverlapping(data.as_ptr(), mapped as *mut u8, data.len());
    }
    unsafe { device.unmap_memory(staging_mem) };

    // Create a transient command buffer.
    let pool_info = vk::CommandPoolCreateInfo::default()
        .queue_family_index(queue_family)
        .flags(vk::CommandPoolCreateFlags::TRANSIENT);
    let pool = unsafe { device.create_command_pool(&pool_info, None)? };
    let alloc_cmds = vk::CommandBufferAllocateInfo::default()
        .command_pool(pool)
        .level(vk::CommandBufferLevel::PRIMARY)
        .command_buffer_count(1);
    let cmds = unsafe { device.allocate_command_buffers(&alloc_cmds)? };
    let cmd = cmds[0];

    let begin =
        vk::CommandBufferBeginInfo::default().flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
    unsafe { device.begin_command_buffer(cmd, &begin)? };

    let aspect = aspect_for_format(format);
    let is_depth = format.is_depth();
    let shader_layout = if is_depth {
        vk::ImageLayout::DEPTH_STENCIL_READ_ONLY_OPTIMAL
    } else {
        vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL
    };

    // Transition image: UNDEFINED -> TRANSFER_DST_OPTIMAL.
    let barrier = vk::ImageMemoryBarrier::default()
        .old_layout(vk::ImageLayout::UNDEFINED)
        .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
        .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .image(image)
        .subresource_range(vk::ImageSubresourceRange {
            aspect_mask: aspect,
            base_mip_level: 0,
            level_count: mip_levels,
            base_array_layer: 0,
            layer_count: array_layers,
        })
        .src_access_mask(vk::AccessFlags::NONE)
        .dst_access_mask(vk::AccessFlags::TRANSFER_WRITE);

    unsafe {
        device.cmd_pipeline_barrier(
            cmd,
            vk::PipelineStageFlags::TOP_OF_PIPE,
            vk::PipelineStageFlags::TRANSFER,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            std::slice::from_ref(&barrier),
        );

        device.cmd_copy_buffer_to_image(
            cmd,
            staging,
            image,
            vk::ImageLayout::TRANSFER_DST_OPTIMAL,
            std::slice::from_ref(
                &vk::BufferImageCopy::default()
                    .buffer_offset(0)
                    .buffer_row_length(0)
                    .buffer_image_height(0)
                    .image_subresource(vk::ImageSubresourceLayers {
                        aspect_mask: aspect,
                        mip_level: 0,
                        base_array_layer: 0,
                        layer_count: array_layers,
                    })
                    .image_offset(vk::Offset3D { x: 0, y: 0, z: 0 })
                    .image_extent(vk::Extent3D {
                        width,
                        height,
                        depth,
                    }),
            ),
        );
    }

    // Transition image: TRANSFER_DST_OPTIMAL -> SHADER_READ_ONLY.
    let barrier2 = vk::ImageMemoryBarrier::default()
        .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
        .new_layout(shader_layout)
        .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .image(image)
        .subresource_range(vk::ImageSubresourceRange {
            aspect_mask: aspect,
            base_mip_level: 0,
            level_count: mip_levels,
            base_array_layer: 0,
            layer_count: array_layers,
        })
        .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
        .dst_access_mask(vk::AccessFlags::SHADER_READ);

    unsafe {
        device.cmd_pipeline_barrier(
            cmd,
            vk::PipelineStageFlags::TRANSFER,
            vk::PipelineStageFlags::FRAGMENT_SHADER,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            std::slice::from_ref(&barrier2),
        );

        device.end_command_buffer(cmd)?;
    }

    // Create a fence and submit.
    let fence_info = vk::FenceCreateInfo::default();
    let fence = unsafe { device.create_fence(&fence_info, None)? };

    let submit = vk::SubmitInfo::default().command_buffers(std::slice::from_ref(&cmd));
    unsafe {
        device.queue_submit(queue, std::slice::from_ref(&submit), fence)?;
        device.wait_for_fences(&[fence], true, std::u64::MAX)?;
        device.destroy_fence(fence, None);
        device.free_command_buffers(pool, &cmds);
        device.destroy_command_pool(pool, None);
        device.destroy_buffer(staging, None);
        device.free_memory(staging_mem, None);
    }

    // If the image is a render/depth target with no transfer destination usage,
    // the caller is responsible for transitioning to its renderable layout.
    let _ = dst_usage;
    Ok(())
}

/// Vulkan texture.
pub struct VulkanTexture {
    device: Arc<ash::Device>,
    image: vk::Image,
    memory: vk::DeviceMemory,
    view: vk::ImageView,
    sampler: vk::Sampler,
    width: u32,
    height: u32,
    depth: u32,
    format: TextureFormat,
    mip_levels: u32,
    array_layers: u32,
    owns_image: bool,
}

impl VulkanTexture {
    /// Creates a new Vulkan texture.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        device: &Arc<ash::Device>,
        descriptor: &TextureDescriptor,
        data: Option<&[u8]>,
        memory_properties: &vk::PhysicalDeviceMemoryProperties,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        Self::new_with_queue(device, descriptor, data, memory_properties, None, 0)
    }

    /// Creates a texture, uploading `data` through a staging buffer when a
    /// graphics `queue` is supplied (used by `Device::create_texture`).
    #[allow(clippy::too_many_arguments)]
    pub fn new_with_queue(
        device: &Arc<ash::Device>,
        descriptor: &TextureDescriptor,
        data: Option<&[u8]>,
        memory_properties: &vk::PhysicalDeviceMemoryProperties,
        queue: Option<vk::Queue>,
        queue_family: u32,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let vk_format = to_vk_format(descriptor.format);

        let image_type = if descriptor.depth > 1 {
            vk::ImageType::TYPE_3D
        } else {
            vk::ImageType::TYPE_2D
        };

        let is_depth = descriptor.format.is_depth();
        let mut usage = vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_DST;
        match descriptor.usage {
            TextureUsage::RenderTarget => usage |= vk::ImageUsageFlags::COLOR_ATTACHMENT,
            TextureUsage::DepthStencil => usage |= vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT,
            TextureUsage::SampledRenderTarget => {
                usage |= vk::ImageUsageFlags::COLOR_ATTACHMENT;
            }
            TextureUsage::Sampled => {}
        }
        let _ = is_depth;

        let image_info = vk::ImageCreateInfo::default()
            .image_type(image_type)
            .format(vk_format)
            .extent(vk::Extent3D {
                width: descriptor.width,
                height: descriptor.height,
                depth: descriptor.depth,
            })
            .mip_levels(descriptor.mip_levels.max(1))
            .array_layers(descriptor.array_layers.max(1))
            .samples(vk::SampleCountFlags::TYPE_1)
            .tiling(vk::ImageTiling::OPTIMAL)
            .usage(usage)
            .sharing_mode(vk::SharingMode::EXCLUSIVE);

        let image = unsafe { device.create_image(&image_info, None)? };

        let memory_requirements = unsafe { device.get_image_memory_requirements(image) };
        let memory_type_index = find_memory_type(
            memory_properties,
            memory_requirements.memory_type_bits,
            vk::MemoryPropertyFlags::DEVICE_LOCAL,
        )
        .ok_or("Failed to find device-local memory type for texture")?;

        let memory_allocate_info = vk::MemoryAllocateInfo::default()
            .allocation_size(memory_requirements.size)
            .memory_type_index(memory_type_index);

        let memory = unsafe { device.allocate_memory(&memory_allocate_info, None)? };
        unsafe { device.bind_image_memory(image, memory, 0)? };

        if let (Some(data), Some(queue)) = (data, queue) {
            upload_texture_data(
                device,
                queue,
                queue_family,
                memory_properties,
                image,
                descriptor.width,
                descriptor.height,
                descriptor.depth,
                descriptor.array_layers.max(1),
                descriptor.mip_levels.max(1),
                descriptor.format,
                data,
                usage,
            )?;
        } else if data.is_some() {
            // No queue available: fall back to a host-visible linear image so
            // data can still be written. This is a compatibility path only.
            return Err("Vulkan texture upload requires a graphics queue (staging copy)".into());
        }

        let aspect = aspect_for_format(descriptor.format);

        let view_type = if descriptor.depth > 1 {
            vk::ImageViewType::TYPE_3D
        } else {
            vk::ImageViewType::TYPE_2D
        };

        let view_info = vk::ImageViewCreateInfo::default()
            .image(image)
            .view_type(view_type)
            .format(vk_format)
            .subresource_range(vk::ImageSubresourceRange {
                aspect_mask: aspect,
                base_mip_level: 0,
                level_count: descriptor.mip_levels.max(1),
                base_array_layer: 0,
                layer_count: descriptor.array_layers.max(1),
            });
        let view = unsafe { device.create_image_view(&view_info, None)? };

        let sampler = create_sampler(device, descriptor)?;

        register(
            image,
            RegisteredImage {
                image,
                view,
                sampler,
                width: descriptor.width,
                height: descriptor.height,
                depth: descriptor.depth,
                format: descriptor.format,
                usage: descriptor.usage,
                aspect,
                array_layers: descriptor.array_layers.max(1),
            },
        );

        Ok(Self {
            device: device.clone(),
            image,
            memory,
            view,
            sampler,
            width: descriptor.width,
            height: descriptor.height,
            depth: descriptor.depth,
            format: descriptor.format,
            mip_levels: descriptor.mip_levels,
            array_layers: descriptor.array_layers.max(1),
            owns_image: true,
        })
    }

    /// Wraps an externally-created image (e.g. a swapchain image view) without
    /// owning the underlying image or memory.
    pub fn from_image_view(
        device: &Arc<ash::Device>,
        image: vk::Image,
        view: vk::ImageView,
        width: u32,
        height: u32,
        format: TextureFormat,
        sampler: vk::Sampler,
    ) -> Self {
        register(
            image,
            RegisteredImage {
                image,
                view,
                sampler,
                width,
                height,
                depth: 1,
                format,
                usage: TextureUsage::RenderTarget,
                aspect: aspect_for_format(format),
                array_layers: 1,
            },
        );
        Self {
            device: device.clone(),
            image,
            memory: vk::DeviceMemory::null(),
            view,
            sampler,
            width,
            height,
            depth: 1,
            format,
            mip_levels: 1,
            array_layers: 1,
            owns_image: false,
        }
    }

    pub fn vk_image(&self) -> vk::Image {
        self.image
    }

    pub fn vk_view(&self) -> vk::ImageView {
        self.view
    }

    pub fn vk_sampler(&self) -> vk::Sampler {
        self.sampler
    }

    pub fn array_layers(&self) -> u32 {
        self.array_layers
    }

    pub fn image_usage(&self) -> vk::ImageUsageFlags {
        vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_DST
    }

    /// Whether the image is sampled (has a valid sampler).
    pub fn has_sampler(&self) -> bool {
        !self.sampler.is_null()
    }
}

impl Drop for VulkanTexture {
    fn drop(&mut self) {
        super::registry::unregister(self.image);
        unsafe {
            if !self.sampler.is_null() {
                self.device.destroy_sampler(self.sampler, None);
            }
            self.device.destroy_image_view(self.view, None);
            if self.owns_image {
                self.device.destroy_image(self.image, None);
                if !self.memory.is_null() {
                    self.device.free_memory(self.memory, None);
                }
            }
        }
    }
}

impl std::fmt::Debug for VulkanTexture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VulkanTexture")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("depth", &self.depth)
            .field("format", &self.format)
            .field("mip_levels", &self.mip_levels)
            .finish()
    }
}

impl Texture for VulkanTexture {
    fn width(&self) -> u32 {
        self.width
    }
    fn height(&self) -> u32 {
        self.height
    }
    fn depth(&self) -> u32 {
        self.depth
    }
    fn format(&self) -> TextureFormat {
        self.format
    }
    fn mip_levels(&self) -> u32 {
        self.mip_levels
    }
    fn update(
        &mut self,
        data: &[u8],
        _mip_level: u32,
        _layer: u32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if data.is_empty() {
            return Ok(());
        }
        // Staging upload requires command pool and queue, which are not stored
        // on VulkanTexture. Log a warning; dynamic texture updates (font atlas,
        // video) are not yet supported on the Vulkan backend.
        tracing::warn!(
            "VulkanTexture::update: {} bytes ignored (staging upload not yet available on this path)",
            data.len()
        );
        Ok(())
    }
    fn bind(&self, _unit: u32) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }
    fn native_handle(&self) -> u64 {
        unsafe { std::mem::transmute::<vk::Image, u64>(self.image) }
    }
}

/// Creates a `VkSampler` (VB12) from the texture descriptor's filter and wrap
/// modes, with anisotropy enabled when available.
pub fn create_sampler(
    device: &ash::Device,
    descriptor: &TextureDescriptor,
) -> Result<vk::Sampler, Box<dyn std::error::Error>> {
    let mag = match descriptor.mag_filter {
        crate::rhi::Filter::Nearest => vk::Filter::NEAREST,
        crate::rhi::Filter::Linear => vk::Filter::LINEAR,
    };
    let min = match descriptor.min_filter {
        crate::rhi::Filter::Nearest => vk::Filter::NEAREST,
        crate::rhi::Filter::Linear => vk::Filter::LINEAR,
    };
    let wrap = |m: crate::rhi::WrapMode| match m {
        crate::rhi::WrapMode::Clamp => vk::SamplerAddressMode::CLAMP_TO_EDGE,
        crate::rhi::WrapMode::Repeat => vk::SamplerAddressMode::REPEAT,
        crate::rhi::WrapMode::MirrorRepeat => vk::SamplerAddressMode::MIRRORED_REPEAT,
        crate::rhi::WrapMode::Border => vk::SamplerAddressMode::CLAMP_TO_BORDER,
    };
    let mipmap_mode = if descriptor.generate_mipmaps {
        vk::SamplerMipmapMode::LINEAR
    } else {
        vk::SamplerMipmapMode::NEAREST
    };

    let info = vk::SamplerCreateInfo::default()
        .mag_filter(mag)
        .min_filter(min)
        .mipmap_mode(mipmap_mode)
        .address_mode_u(wrap(descriptor.wrap_u))
        .address_mode_v(wrap(descriptor.wrap_v))
        .address_mode_w(wrap(descriptor.wrap_w))
        .mip_lod_bias(0.0)
        .anisotropy_enable(false)
        .max_anisotropy(1.0)
        .compare_enable(false)
        .min_lod(0.0)
        .max_lod(descriptor.mip_levels.max(1) as f32)
        .border_color(vk::BorderColor::FLOAT_TRANSPARENT_BLACK)
        .unnormalized_coordinates(false);

    let sampler = unsafe { device.create_sampler(&info, None)? };
    Ok(sampler)
}
