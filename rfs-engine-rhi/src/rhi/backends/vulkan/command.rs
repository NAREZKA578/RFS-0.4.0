//! Vulkan Command Encoder implementation.
//!
//! Records commands into a `VkCommandBuffer` for submission via
//! [`VulkanCommandEncoder::submit_batch`]. Uses dynamic rendering
//! (core in Vulkan 1.3) so pipelines do not need a fixed `VkRenderPass` and
//! attachments can be resolved per-frame from the image registry. Uniforms and
//! vectors are pushed through a 128-byte push-constant block; textures are
//! bound through a single combined-image-sampler descriptor set.

use crate::rhi::command::{
    ColorAttachment, CommandEncoder, DepthAttachment, LoadOp, PrimitiveTopology,
};
use crate::rhi::types::Rect;
use crate::rhi::{Buffer, Pipeline, Texture};
use ash::vk;
use ash::vk::Handle;
use std::sync::Arc;

use super::registry;

/// Size of the push-constant block shared across Vulkan pipelines.
pub const PUSH_CONSTANT_SIZE: u32 = 256;

/// Maximum number of texture bindings a Vulkan pipeline exposes in its
/// descriptor set (bindings `0..texture_bindings`).
pub const MAX_TEXTURE_BINDINGS: u32 = 8;

/// First descriptor-set binding index used for uniform buffers (UBOs).
pub const UBO_BASE_BINDING: u32 = MAX_TEXTURE_BINDINGS;
/// Number of uniform-buffer slots exposed by every Vulkan pipeline.
pub const MAX_UBO_BINDINGS: u32 = 8;

/// First descriptor-set binding index used for storage buffers (SSBOs).
pub const SSBO_BASE_BINDING: u32 = UBO_BASE_BINDING + MAX_UBO_BINDINGS;
/// Number of storage-buffer slots exposed by every Vulkan pipeline.
pub const MAX_SSBO_BINDINGS: u32 = 8;

/// Total number of descriptor-set bindings across all descriptor types.
pub const TOTAL_BINDINGS: u32 = SSBO_BASE_BINDING + MAX_SSBO_BINDINGS;

/// Returns the number of texture bindings used by every Vulkan pipeline.
pub fn texture_bindings() -> u32 {
    MAX_TEXTURE_BINDINGS
}

/// Looks up a registered color attachment as (registry info, VkFormat).
fn resolve_color(handle: u64) -> Option<(registry::RegisteredImage, vk::Format)> {
    let info = registry::lookup(handle)?;
    if info.view.is_null() {
        return None;
    }
    Some((info, super::texture::to_vk_format(info.format)))
}

fn resolve_depth(handle: u64) -> Option<(registry::RegisteredImage, vk::Format)> {
    resolve_color(handle)
}

/// Transitions an image layout and inserts an execution+memory barrier.
#[allow(clippy::too_many_arguments)]
unsafe fn transition_image(
    device: &ash::Device,
    cmd: vk::CommandBuffer,
    image: vk::Image,
    aspect: vk::ImageAspectFlags,
    layers: u32,
    old_layout: vk::ImageLayout,
    new_layout: vk::ImageLayout,
    src_stage: vk::PipelineStageFlags,
    dst_stage: vk::PipelineStageFlags,
    src_access: vk::AccessFlags,
    dst_access: vk::AccessFlags,
) {
    let barrier = vk::ImageMemoryBarrier::default()
        .old_layout(old_layout)
        .new_layout(new_layout)
        .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .image(image)
        .subresource_range(vk::ImageSubresourceRange {
            aspect_mask: aspect,
            base_mip_level: 0,
            level_count: vk::REMAINING_MIP_LEVELS,
            base_array_layer: 0,
            layer_count: layers,
        })
        .src_access_mask(src_access)
        .dst_access_mask(dst_access);
    device.cmd_pipeline_barrier(
        cmd,
        src_stage,
        dst_stage,
        vk::DependencyFlags::empty(),
        &[],
        &[],
        std::slice::from_ref(&barrier),
    );
}

/// Vulkan command encoder.
pub struct VulkanCommandEncoder {
    device: Arc<ash::Device>,
    command_pool: vk::CommandPool,
    command_buffer: vk::CommandBuffer,
    #[allow(dead_code)]
    queue_family: u32,

    /// True while recording (between `begin` and `end`).
    recording: bool,
    /// True while a render pass (dynamic rendering) is active.
    render_pass_active: bool,

    /// Current pipeline layout (for push constants / descriptor binding).
    pipeline_layout: Option<vk::PipelineLayout>,
    /// Current pipeline (raw handle) for `cmd_bind_pipeline` on first draw.
    pipeline: Option<vk::Pipeline>,
    /// Primitive topology to record on the pipeline-less encoder path.
    topology: PrimitiveTopology,

    /// Vertex buffers bound per binding index.
    vertex_buffers: Vec<Option<vk::Buffer>>,
    /// Instance buffers bound per binding index (from bind_instance_buffer).
    instance_buffers: Vec<Option<(vk::Buffer, u32)>>,
    /// Index buffer bound for index draws.
    index_buffer: Option<vk::Buffer>,

    /// Push-constant arena (128 bytes).
    push_constants: [u8; PUSH_CONSTANT_SIZE as usize],
    /// Number of valid bytes written into the push-constant arena.
    pc_len: usize,

    /// Textures bound per descriptor binding (index = binding), for the next draw.
    bound_textures: Vec<Option<(vk::ImageView, vk::Sampler)>>,
    /// Uniform buffers bound per UBO slot (index = slot, 0-based within UBO range).
    bound_ubos: Vec<Option<(vk::Buffer, u64)>>,
    /// Storage buffers bound per SSBO slot (index = slot, 0-based within SSBO range).
    bound_ssbos: Vec<Option<(vk::Buffer, u64)>>,

    /// Lazily-created descriptor pool for the single combined image sampler.
    descriptor_pool: Option<vk::DescriptorPool>,
    /// Descriptor set updated per draw.
    descriptor_set: Option<vk::DescriptorSet>,
}

impl VulkanCommandEncoder {
    /// Creates a new Vulkan command encoder.
    pub fn new(
        device: &Arc<ash::Device>,
        queue_family: u32,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let pool_info = vk::CommandPoolCreateInfo::default()
            .queue_family_index(queue_family)
            .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER);

        let command_pool = unsafe { device.create_command_pool(&pool_info, None)? };

        let allocate_info = vk::CommandBufferAllocateInfo::default()
            .command_pool(command_pool)
            .level(vk::CommandBufferLevel::PRIMARY)
            .command_buffer_count(1);

        let command_buffer = unsafe { device.allocate_command_buffers(&allocate_info)?[0] };

        Ok(Self {
            device: device.clone(),
            command_pool,
            command_buffer,
            queue_family,
            recording: false,
            render_pass_active: false,
            pipeline_layout: None,
            pipeline: None,
            topology: PrimitiveTopology::TriangleList,
            vertex_buffers: Vec::new(),
            instance_buffers: Vec::new(),
            index_buffer: None,
            push_constants: [0u8; PUSH_CONSTANT_SIZE as usize],
            pc_len: 0,
            bound_textures: Vec::new(),
            bound_ubos: Vec::new(),
            bound_ssbos: Vec::new(),
            descriptor_pool: None,
            descriptor_set: None,
        })
    }

    /// Begins recording commands.
    pub fn begin(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if self.rendering_active() {
            return Err("Cannot begin: render pass still active".into());
        }
        let begin_info = vk::CommandBufferBeginInfo::default()
            .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
        unsafe {
            self.device
                .begin_command_buffer(self.command_buffer, &begin_info)?
        };
        self.recording = true;
        self.reset_draw_state();
        Ok(())
    }

    /// Ends recording commands.
    pub fn end(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if !self.recording {
            return Err("Cannot end: not recording".into());
        }
        unsafe { self.device.end_command_buffer(self.command_buffer)? };
        self.recording = false;
        Ok(())
    }

    /// Returns true if a render pass is currently active.
    pub fn rendering_active(&self) -> bool {
        self.render_pass_active
    }

    /// Resets per-frame/per-pass draw state.
    fn reset_draw_state(&mut self) {
        self.pipeline = None;
        self.pipeline_layout = None;
        self.vertex_buffers.clear();
        self.instance_buffers.clear();
        self.index_buffer = None;
        self.pc_len = 0;
        self.bound_textures.clear();
        self.bound_ubos.clear();
        self.bound_ssbos.clear();
    }

    /// Records a `cmd_bind_pipeline` if the pipeline changed.
    fn bind_pipeline_vk(&mut self, pipeline: vk::Pipeline) {
        if self.pipeline != Some(pipeline) {
            unsafe {
                self.device.cmd_bind_pipeline(
                    self.command_buffer,
                    vk::PipelineBindPoint::GRAPHICS,
                    pipeline,
                );
            }
            self.pipeline = Some(pipeline);
        }
    }

    /// Flushes pending push constants to the command buffer.
    fn flush_push_constants(&mut self) {
        let Some(layout) = self.pipeline_layout else {
            return;
        };
        if self.pc_len == 0 {
            return;
        }
        unsafe {
            self.device.cmd_push_constants(
                self.command_buffer,
                layout,
                vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT,
                0,
                &self.push_constants[..self.pc_len],
            );
        }
        self.pc_len = 0;
    }

    /// Binds all recorded vertex+instance buffers (stored by binding index).
    fn bind_recorded_buffers(&mut self) {
        // Per-instance buffers are bound at binding = first_location / 4.
        let mut binding_slots: Vec<Option<vk::Buffer>> = Vec::new();
        for b in &self.vertex_buffers {
            binding_slots.push(*b);
        }
        for (idx, entry) in self.instance_buffers.iter().enumerate() {
            if let Some((buf, first_location)) = entry {
                let binding = (first_location / 4) as usize;
                while binding_slots.len() <= binding {
                    binding_slots.push(None);
                }
                binding_slots[binding] = Some(*buf);
            }
            let _ = idx;
        }

        let mut buffers: Vec<vk::Buffer> = Vec::new();
        let mut offsets: Vec<vk::DeviceSize> = Vec::new();
        for slot in &binding_slots {
            if let Some(b) = slot {
                buffers.push(*b);
                offsets.push(0);
            }
        }
        if buffers.is_empty() {
            return;
        }
        unsafe {
            self.device
                .cmd_bind_vertex_buffers(self.command_buffer, 0, &buffers, &offsets);
        }
    }

    /// Binds the recorded index buffer.
    fn bind_recorded_index_buffer(&mut self) {
        if let Some(buf) = self.index_buffer {
            unsafe {
                self.device.cmd_bind_index_buffer(
                    self.command_buffer,
                    buf,
                    0,
                    vk::IndexType::UINT32,
                );
            }
        }
    }

    /// Ensures the descriptor set exists and points at the bound textures, UBOs, and SSBOs.
    fn bind_texture_descriptor(&mut self) {
        let Some(layout) = self.pipeline_layout else {
            return;
        };
        let set_layout = registry::lookup_descriptor_set_layout(layout);
        if set_layout.is_null() {
            return;
        }

        // Check if anything is bound at all.
        let has_textures = self.bound_textures.iter().any(|e| e.is_some());
        let has_ubos = self.bound_ubos.iter().any(|e| e.is_some());
        let has_ssbos = self.bound_ssbos.iter().any(|e| e.is_some());
        if !has_textures && !has_ubos && !has_ssbos {
            return;
        }

        // Create the descriptor pool with all three descriptor types.
        if self.descriptor_pool.is_none() {
            let sizes = [
                vk::DescriptorPoolSize::default()
                    .ty(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                    .descriptor_count(MAX_TEXTURE_BINDINGS),
                vk::DescriptorPoolSize::default()
                    .ty(vk::DescriptorType::UNIFORM_BUFFER)
                    .descriptor_count(MAX_UBO_BINDINGS),
                vk::DescriptorPoolSize::default()
                    .ty(vk::DescriptorType::STORAGE_BUFFER)
                    .descriptor_count(MAX_SSBO_BINDINGS),
            ];
            let pool_info = vk::DescriptorPoolCreateInfo::default()
                .max_sets(1)
                .pool_sizes(&sizes);
            match unsafe { self.device.create_descriptor_pool(&pool_info, None) } {
                Ok(pool) => self.descriptor_pool = Some(pool),
                Err(e) => {
                    tracing::warn!("create_descriptor_pool failed: {e}");
                    return;
                }
            }
        }
        if self.descriptor_set.is_none() {
            let alloc = vk::DescriptorSetAllocateInfo::default()
                .descriptor_pool(self.descriptor_pool.unwrap())
                .set_layouts(std::slice::from_ref(&set_layout));
            match unsafe { self.device.allocate_descriptor_sets(&alloc) } {
                Ok(sets) => self.descriptor_set = sets.first().copied(),
                Err(e) => {
                    tracing::warn!("allocate_descriptor_sets failed: {e}");
                    return;
                }
            }
        }
        let Some(set) = self.descriptor_set else {
            return;
        };

        // Collect all descriptors, then write in two passes to satisfy the borrow checker.
        // Pass 1: collect info objects.
        let mut img_infos: Vec<vk::DescriptorImageInfo> = Vec::new();
        let mut img_bindings: Vec<u32> = Vec::new();
        for (i, entry) in self.bound_textures.iter().enumerate() {
            if let Some((view, sampler)) = entry {
                img_bindings.push(i as u32);
                img_infos.push(
                    vk::DescriptorImageInfo::default()
                        .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                        .image_view(*view)
                        .sampler(*sampler),
                );
            }
        }

        let mut ubo_infos: Vec<vk::DescriptorBufferInfo> = Vec::new();
        let mut ubo_bindings: Vec<u32> = Vec::new();
        for (i, entry) in self.bound_ubos.iter().enumerate() {
            if let Some((buf, size)) = entry {
                ubo_bindings.push(UBO_BASE_BINDING + i as u32);
                ubo_infos.push(
                    vk::DescriptorBufferInfo::default()
                        .buffer(*buf)
                        .offset(0)
                        .range(*size),
                );
            }
        }

        let mut ssbo_infos: Vec<vk::DescriptorBufferInfo> = Vec::new();
        let mut ssbo_bindings: Vec<u32> = Vec::new();
        for (i, entry) in self.bound_ssbos.iter().enumerate() {
            if let Some((buf, size)) = entry {
                ssbo_bindings.push(SSBO_BASE_BINDING + i as u32);
                ssbo_infos.push(
                    vk::DescriptorBufferInfo::default()
                        .buffer(*buf)
                        .offset(0)
                        .range(*size),
                );
            }
        }

        // Pass 2: build write descriptors referencing the collected infos.
        let mut writes: Vec<vk::WriteDescriptorSet> = Vec::new();
        for (k, &binding) in img_bindings.iter().enumerate() {
            writes.push(
                vk::WriteDescriptorSet::default()
                    .dst_set(set)
                    .dst_binding(binding)
                    .descriptor_count(1)
                    .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                    .image_info(&img_infos[k..k + 1]),
            );
        }
        for (k, &binding) in ubo_bindings.iter().enumerate() {
            writes.push(
                vk::WriteDescriptorSet::default()
                    .dst_set(set)
                    .dst_binding(binding)
                    .descriptor_count(1)
                    .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
                    .buffer_info(&ubo_infos[k..k + 1]),
            );
        }
        for (k, &binding) in ssbo_bindings.iter().enumerate() {
            writes.push(
                vk::WriteDescriptorSet::default()
                    .dst_set(set)
                    .dst_binding(binding)
                    .descriptor_count(1)
                    .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                    .buffer_info(&ssbo_infos[k..k + 1]),
            );
        }
        if writes.is_empty() {
            return;
        }

        unsafe {
            self.device.update_descriptor_sets(&writes, &[]);
            self.device.cmd_bind_descriptor_sets(
                self.command_buffer,
                vk::PipelineBindPoint::GRAPHICS,
                layout,
                0,
                std::slice::from_ref(&set),
                &[],
            );
        }
    }

    /// Records a dynamic-rendering begin for the given attachments.
    fn record_begin_rendering(
        &mut self,
        color_attachments: &[ColorAttachment],
        depth_attachment: Option<&DepthAttachment>,
    ) {
        let mut color_infos: Vec<vk::RenderingAttachmentInfo> = Vec::new();
        let mut extent = vk::Extent2D {
            width: 1,
            height: 1,
        };

        for att in color_attachments {
            // handle-0 = default framebuffer (OpenGL semantics). Under Vulkan we
            // resolve it to the app-designated default color target (the current
            // swapchain image) registered in the registry.
            let handle = if att.texture_handle == 0 {
                registry::lookup_default_color()
                    .map(|i| i.image.as_raw())
                    .unwrap_or(0)
            } else {
                att.texture_handle
            };
            if handle == 0 {
                continue;
            }
            let Some((info, _fmt)) = resolve_color(handle) else {
                tracing::warn!(
                    "begin_render_pass: color attachment {} not registered",
                    handle
                );
                continue;
            };
            if info.usage == crate::rhi::TextureUsage::DepthStencil {
                continue;
            }
            extent.width = info.width;
            extent.height = info.height;
            unsafe {
                transition_image(
                    &self.device,
                    self.command_buffer,
                    info.image,
                    info.aspect,
                    info.array_layers,
                    vk::ImageLayout::UNDEFINED,
                    vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
                    vk::PipelineStageFlags::TOP_OF_PIPE,
                    vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
                    vk::AccessFlags::NONE,
                    vk::AccessFlags::COLOR_ATTACHMENT_WRITE,
                );
            }
            let (load_op, clear_value) = match att.load_op {
                LoadOp::Clear => (
                    vk::AttachmentLoadOp::CLEAR,
                    Some(vk::ClearValue {
                        color: vk::ClearColorValue {
                            float32: [
                                att.clear_color.r,
                                att.clear_color.g,
                                att.clear_color.b,
                                att.clear_color.a,
                            ],
                        },
                    }),
                ),
                LoadOp::Load => (vk::AttachmentLoadOp::LOAD, None),
                LoadOp::DontCare => (vk::AttachmentLoadOp::DONT_CARE, None),
            };
            color_infos.push(
                vk::RenderingAttachmentInfo::default()
                    .image_view(info.view)
                    .image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
                    .load_op(load_op)
                    .store_op(vk::AttachmentStoreOp::STORE)
                    .clear_value(clear_value.unwrap_or_default()),
            );
        }

        let mut depth_info: Option<vk::RenderingAttachmentInfo> = None;
        if let Some(depth) = depth_attachment {
            // Resolve the OpenGL default-framebuffer handle (0) to the default
            // depth target registered by the app.
            let dhandle = if depth.texture_handle == 0 {
                registry::lookup_default_depth()
                    .map(|i| i.image.as_raw())
                    .unwrap_or(0)
            } else {
                depth.texture_handle
            };
            if dhandle != 0 {
                if let Some((info, _fmt)) = resolve_depth(dhandle) {
                    extent.width = info.width;
                    extent.height = info.height;
                    unsafe {
                        transition_image(
                            &self.device,
                            self.command_buffer,
                            info.image,
                            info.aspect,
                            info.array_layers,
                            vk::ImageLayout::UNDEFINED,
                            vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL,
                            vk::PipelineStageFlags::TOP_OF_PIPE,
                            vk::PipelineStageFlags::EARLY_FRAGMENT_TESTS,
                            vk::AccessFlags::NONE,
                            vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE,
                        );
                    }
                    let load_op = match depth.depth_load_op {
                        LoadOp::Clear => vk::AttachmentLoadOp::CLEAR,
                        LoadOp::Load => vk::AttachmentLoadOp::LOAD,
                        LoadOp::DontCare => vk::AttachmentLoadOp::DONT_CARE,
                    };
                    depth_info = Some(
                        vk::RenderingAttachmentInfo::default()
                            .image_view(info.view)
                            .image_layout(vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL)
                            .load_op(load_op)
                            .store_op(vk::AttachmentStoreOp::STORE)
                            .clear_value(vk::ClearValue {
                                depth_stencil: vk::ClearDepthStencilValue {
                                    depth: depth.clear_depth,
                                    stencil: 0,
                                },
                            }),
                    );
                }
            }
        }

        let mut rendering_info = vk::RenderingInfo::default()
            .render_area(vk::Rect2D {
                offset: vk::Offset2D { x: 0, y: 0 },
                extent,
            })
            .layer_count(1)
            .color_attachments(&color_infos);
        if let Some(di) = &depth_info {
            rendering_info = rendering_info.depth_attachment(di);
        }

        unsafe {
            self.device
                .cmd_begin_rendering(self.command_buffer, &rendering_info);
        }
        self.render_pass_active = true;
    }

    /// Submits a batch of command encoders.
    pub fn submit_batch(
        device: &ash::Device,
        queue: vk::Queue,
        commands: &[&dyn CommandEncoder],
        fence: Option<&mut crate::rhi::sync::Fence>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut buffers: Vec<vk::CommandBuffer> = Vec::new();
        for &encoder in commands {
            let Some(vk_enc) = SuperVulkan::downcast(encoder) else {
                return Err("submit_batch: non-Vulkan encoder provided".into());
            };
            // The encoder is expected to have been ended before submit
            // (Device::submit is a shared reference). Only collect its buffer.
            buffers.push(vk_enc.command_buffer);
        }
        if buffers.is_empty() {
            return Ok(());
        }

        let submit = vk::SubmitInfo::default().command_buffers(&buffers);
        let fence_handle = fence
            .as_ref()
            .and_then(|f| f.handle())
            .map(vk::Fence::from_raw);
        match fence_handle {
            Some(f) => {
                unsafe { device.queue_submit(queue, std::slice::from_ref(&submit), f)? };
            }
            None => {
                let tmp = unsafe { device.create_fence(&vk::FenceCreateInfo::default(), None)? };
                unsafe {
                    device.queue_submit(queue, std::slice::from_ref(&submit), tmp)?;
                    device.wait_for_fences(&[tmp], true, std::u64::MAX)?;
                    device.destroy_fence(tmp, None);
                }
            }
        }
        Ok(())
    }

    /// Returns the Vulkan command buffer.
    pub fn vk_command_buffer(&self) -> vk::CommandBuffer {
        self.command_buffer
    }
}

/// Allows downcasting `&dyn CommandEncoder` back to `&VulkanCommandEncoder`
/// during `submit_batch`, via trait object upcasting to `dyn Any`.
struct SuperVulkan;

impl SuperVulkan {
    fn downcast(encoder: &dyn CommandEncoder) -> Option<&VulkanCommandEncoder> {
        let any: &dyn std::any::Any = encoder;
        any.downcast_ref::<VulkanCommandEncoder>()
    }
}

impl Drop for VulkanCommandEncoder {
    fn drop(&mut self) {
        unsafe {
            if let Some(pool) = self.descriptor_pool.take() {
                self.device.destroy_descriptor_pool(pool, None);
            }
            self.device
                .free_command_buffers(self.command_pool, &[self.command_buffer]);
            self.device.destroy_command_pool(self.command_pool, None);
        }
    }
}

// ── uniform push-constant helpers ─────────────────────────────────────────

impl CommandEncoder for VulkanCommandEncoder {
    fn begin_render_pass(
        &mut self,
        color_attachments: &[ColorAttachment],
        depth_attachment: Option<&DepthAttachment>,
    ) {
        if self.render_pass_active {
            tracing::warn!("begin_render_pass while another pass is active");
            self.end_render_pass();
        }
        self.record_begin_rendering(color_attachments, depth_attachment);
    }

    fn end_render_pass(&mut self) {
        if !self.render_pass_active {
            return;
        }
        unsafe {
            self.device.cmd_end_rendering(self.command_buffer);
        }
        self.render_pass_active = false;
    }

    fn bind_pipeline(&mut self, pipeline: &dyn Pipeline) {
        let Some(reg) = registry::lookup_pipeline(pipeline.native_handle()) else {
            tracing::warn!(
                "bind_pipeline: pipeline {} not in registry",
                pipeline.native_handle()
            );
            return;
        };
        self.pipeline_layout = Some(reg.layout);
        self.bind_pipeline_vk(reg.pipeline);
        self.bound_textures.clear();
    }

    fn bind_vertex_buffer(&mut self, binding: u32, buffer: &dyn Buffer) {
        let Some(vk_buf) = registry::lookup_buffer(buffer.native_handle()) else {
            tracing::warn!("bind_vertex_buffer: buffer not registered");
            return;
        };
        let idx = binding as usize;
        if self.vertex_buffers.len() <= idx {
            self.vertex_buffers.resize(idx + 1, None);
        }
        self.vertex_buffers[idx] = Some(vk_buf);
    }

    fn bind_instance_buffer(&mut self, first_location: u32, buffer: &dyn Buffer, stride: u32) {
        let Some(vk_buf) = registry::lookup_buffer(buffer.native_handle()) else {
            tracing::warn!("bind_instance_buffer: buffer not registered");
            return;
        };
        let idx = first_location as usize;
        if self.instance_buffers.len() <= idx {
            self.instance_buffers.resize(idx + 1, None);
        }
        self.instance_buffers[idx] = Some((vk_buf, stride));
    }

    fn bind_index_buffer(&mut self, buffer: &dyn Buffer) {
        let Some(vk_buf) = registry::lookup_buffer(buffer.native_handle()) else {
            tracing::warn!("bind_index_buffer: buffer not registered");
            return;
        };
        self.index_buffer = Some(vk_buf);
    }

    fn bind_uniform_buffer(&mut self, binding: u32, buffer: &dyn Buffer) {
        let Some(vk_buf) = registry::lookup_buffer(buffer.native_handle()) else {
            tracing::warn!("bind_uniform_buffer: buffer not registered");
            return;
        };
        let slot = binding as usize;
        if self.bound_ubos.len() <= slot {
            self.bound_ubos.resize(slot + 1, None);
        }
        self.bound_ubos[slot] = Some((vk_buf, buffer.size()));
    }

    fn bind_storage_buffer(&mut self, binding: u32, buffer: &dyn Buffer) {
        let Some(vk_buf) = registry::lookup_buffer(buffer.native_handle()) else {
            tracing::warn!("bind_storage_buffer: buffer not registered");
            return;
        };
        let slot = binding as usize;
        if self.bound_ssbos.len() <= slot {
            self.bound_ssbos.resize(slot + 1, None);
        }
        self.bound_ssbos[slot] = Some((vk_buf, buffer.size()));
    }

    fn bind_texture(&mut self, binding: u32, texture: &dyn Texture) {
        let Some(info) = registry::lookup(texture.native_handle()) else {
            tracing::warn!(
                "bind_texture: texture {} not registered",
                texture.native_handle()
            );
            return;
        };
        let idx = binding as usize;
        if self.bound_textures.len() <= idx {
            self.bound_textures.resize(idx + 1, None);
        }
        self.bound_textures[idx] = Some((info.view, info.sampler));
    }

    fn set_uniform_f32(&mut self, _name: &str, x: f32) {
        self.push_scalar(&x.to_ne_bytes());
    }

    fn set_uniform_vec2(&mut self, _name: &str, x: f32, y: f32) {
        self.push_align(8); // vec2 alignment in std430 is 8
        self.push_scalar(&x.to_ne_bytes());
        self.push_scalar(&y.to_ne_bytes());
    }

    fn set_uniform_vec3(&mut self, _name: &str, x: f32, y: f32, z: f32) {
        self.push_align(16); // vec3 alignment in std430 is 16
        self.push_scalar(&x.to_ne_bytes());
        self.push_scalar(&y.to_ne_bytes());
        self.push_scalar(&z.to_ne_bytes());
    }

    fn set_uniform_vec4(&mut self, _name: &str, x: f32, y: f32, z: f32, w: f32) {
        self.push_align(16); // vec4 alignment in std430 is 16
        self.push_scalar(&x.to_ne_bytes());
        self.push_scalar(&y.to_ne_bytes());
        self.push_scalar(&z.to_ne_bytes());
        self.push_scalar(&w.to_ne_bytes());
    }

    fn set_uniform_i32(&mut self, _name: &str, x: i32) {
        self.push_scalar(&x.to_ne_bytes());
    }

    fn set_uniform_mat3(&mut self, _name: &str, value: &[f32; 9]) {
        self.push_align(16); // mat3 alignment in std430 is 16
        for col in 0..3 {
            if col > 0 {
                self.push_align(16); // pad to next column boundary (each column at 16-byte stride)
            }
            for row in 0..3 {
                self.push_scalar(&value[col * 3 + row].to_ne_bytes());
            }
        }
    }

    fn set_uniform_mat4(&mut self, _name: &str, value: &[f32; 16]) {
        self.push_align(16); // mat4 alignment in std430 is 16
        for v in value {
            self.push_scalar(&v.to_ne_bytes());
        }
    }

    fn set_uniform_texture(&mut self, _name: &str, _unit: u32) {
        // Texture binding is handled through bind_texture + descriptor binding.
    }

    fn set_viewport(
        &mut self,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        min_depth: f32,
        max_depth: f32,
    ) {
        let viewport = vk::Viewport::default()
            .x(x)
            .y(y)
            .width(width)
            .height(height)
            .min_depth(min_depth)
            .max_depth(max_depth);
        unsafe {
            self.device
                .cmd_set_viewport(self.command_buffer, 0, std::slice::from_ref(&viewport));
        }
    }

    fn set_scissor(&mut self, rect: &Rect) {
        let scissor = vk::Rect2D::default()
            .offset(vk::Offset2D {
                x: rect.x,
                y: rect.y,
            })
            .extent(vk::Extent2D {
                width: rect.width,
                height: rect.height,
            });
        unsafe {
            self.device
                .cmd_set_scissor(self.command_buffer, 0, std::slice::from_ref(&scissor));
        }
    }

    fn set_primitive_topology(&mut self, topology: PrimitiveTopology) {
        self.topology = topology;
    }

    fn draw(
        &mut self,
        vertex_count: u32,
        instance_count: u32,
        first_vertex: u32,
        first_instance: u32,
    ) {
        if !self.render_pass_active {
            tracing::warn!("draw outside a render pass; ignoring");
            return;
        }
        self.bind_recorded_buffers();
        self.flush_push_constants();
        self.bind_texture_descriptor();
        unsafe {
            self.device.cmd_draw(
                self.command_buffer,
                vertex_count,
                instance_count,
                first_vertex,
                first_instance,
            );
        }
    }

    fn draw_indexed(
        &mut self,
        index_count: u32,
        instance_count: u32,
        first_index: u32,
        vertex_offset: i32,
        first_instance: u32,
    ) {
        if !self.render_pass_active {
            tracing::warn!("draw_indexed outside a render pass; ignoring");
            return;
        }
        self.bind_recorded_buffers();
        self.bind_recorded_index_buffer();
        self.flush_push_constants();
        self.bind_texture_descriptor();
        unsafe {
            self.device.cmd_draw_indexed(
                self.command_buffer,
                index_count,
                instance_count,
                first_index,
                vertex_offset,
                first_instance,
            );
        }
    }

    fn update_buffer(&mut self, buffer: &mut dyn Buffer, data: &[u8], offset: u64) {
        if let Err(e) = buffer.update(data, offset) {
            tracing::warn!("update_buffer failed: {}", e);
        }
    }

    fn copy_buffer_to_buffer(
        &mut self,
        src: &dyn Buffer,
        dst: &dyn Buffer,
        src_offset: u64,
        dst_offset: u64,
        size: u64,
    ) {
        let (Some(src_buf), Some(dst_buf)) = (
            registry::lookup_buffer(src.native_handle()),
            registry::lookup_buffer(dst.native_handle()),
        ) else {
            return;
        };
        let region = vk::BufferCopy::default()
            .src_offset(src_offset)
            .dst_offset(dst_offset)
            .size(size);
        unsafe {
            self.device.cmd_copy_buffer(
                self.command_buffer,
                src_buf,
                dst_buf,
                std::slice::from_ref(&region),
            );
        }
    }

    fn copy_texture_to_texture(&mut self, src: &dyn Texture, dst: &dyn Texture) {
        let (Some(src_info), Some(dst_info)) = (
            registry::lookup(src.native_handle()),
            registry::lookup(dst.native_handle()),
        ) else {
            return;
        };
        let region = vk::ImageCopy::default()
            .src_subresource(vk::ImageSubresourceLayers {
                aspect_mask: src_info.aspect,
                mip_level: 0,
                base_array_layer: 0,
                layer_count: 1,
            })
            .src_offset(vk::Offset3D { x: 0, y: 0, z: 0 })
            .dst_subresource(vk::ImageSubresourceLayers {
                aspect_mask: dst_info.aspect,
                mip_level: 0,
                base_array_layer: 0,
                layer_count: 1,
            })
            .dst_offset(vk::Offset3D { x: 0, y: 0, z: 0 })
            .extent(vk::Extent3D {
                width: src_info.width.min(dst_info.width),
                height: src_info.height.min(dst_info.height),
                depth: 1,
            });
        unsafe {
            self.device.cmd_copy_image(
                self.command_buffer,
                src_info.image,
                vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                dst_info.image,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                std::slice::from_ref(&region),
            );
        }
    }

    fn copy_buffer_to_texture(&mut self, src: &dyn Buffer, dst: &dyn Texture) {
        let (Some(src_buf), Some(dst_info)) = (
            registry::lookup_buffer(src.native_handle()),
            registry::lookup(dst.native_handle()),
        ) else {
            return;
        };
        // Transition image layout: UNDEFINED → TRANSFER_DST_OPTIMAL
        let barrier = vk::ImageMemoryBarrier::default()
            .src_access_mask(vk::AccessFlags::empty())
            .dst_access_mask(vk::AccessFlags::TRANSFER_WRITE)
            .old_layout(vk::ImageLayout::UNDEFINED)
            .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
            .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .image(dst_info.image)
            .subresource_range(vk::ImageSubresourceRange {
                aspect_mask: dst_info.aspect,
                base_mip_level: 0,
                level_count: 1,
                base_array_layer: 0,
                layer_count: 1,
            });
        unsafe {
            self.device.cmd_pipeline_barrier(
                self.command_buffer,
                vk::PipelineStageFlags::TOP_OF_PIPE,
                vk::PipelineStageFlags::TRANSFER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                std::slice::from_ref(&barrier),
            );
        }
        let region = vk::BufferImageCopy::default()
            .image_subresource(vk::ImageSubresourceLayers {
                aspect_mask: dst_info.aspect,
                mip_level: 0,
                base_array_layer: 0,
                layer_count: 1,
            })
            .image_extent(vk::Extent3D {
                width: dst_info.width,
                height: dst_info.height,
                depth: 1,
            });
        unsafe {
            self.device.cmd_copy_buffer_to_image(
                self.command_buffer,
                src_buf,
                dst_info.image,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                std::slice::from_ref(&region),
            );
        }
    }

    fn push_debug_group(&mut self, _name: &str) {
        // Debug utils commands are optional; no-op.
    }

    fn pop_debug_group(&mut self) {
        // Debug utils commands are optional; no-op.
    }

    fn push_mat4(&mut self, value: &[f32; 16]) {
        let bytes = bytemuck::cast_slice(value);
        self.push_scalar(bytes);
    }

    fn push_i32(&mut self, value: i32) {
        self.push_scalar(&value.to_ne_bytes());
    }

    fn push_bytes(&mut self, bytes: &[u8]) {
        self.push_scalar(bytes);
    }
}

impl VulkanCommandEncoder {
    /// Appends a 4-byte aligned scalar to the push-constant arena.
    fn push_scalar(&mut self, bytes: &[u8]) {
        let len = bytes.len();
        let align = 4usize;
        let start = self.pc_len;
        let aligned_end = start + len;
        if aligned_end > PUSH_CONSTANT_SIZE as usize {
            tracing::warn!("push-constant block overflow (size 256)");
            return;
        }
        if start % align != 0 {
            let pad = align - (start % align);
            let new_start = start + pad;
            if new_start + len > PUSH_CONSTANT_SIZE as usize {
                tracing::warn!("push-constant block overflow (aligned)");
                return;
            }
            self.push_constants[new_start..new_start + len].copy_from_slice(bytes);
            self.pc_len = new_start + len;
        } else {
            self.push_constants[start..aligned_end].copy_from_slice(bytes);
            self.pc_len = aligned_end;
        }
    }

    /// Pads the push-constant write position to the given alignment boundary.
    fn push_align(&mut self, alignment: usize) {
        let current = self.pc_len;
        if current % alignment != 0 {
            let pad = alignment - (current % alignment);
            if current + pad > PUSH_CONSTANT_SIZE as usize {
                tracing::warn!("push-constant block overflow during align");
                return;
            }
            self.pc_len = current + pad;
        }
    }
}
