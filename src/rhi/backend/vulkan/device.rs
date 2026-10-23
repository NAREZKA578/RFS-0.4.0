//! Vulkan logical device creation and backend resources.
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use crate::backend::common::{BackendResource, ReleaseKind};
use crate::backend::vulkan::convert;
use crate::backend::vulkan::instance::{vk_fail, DestroyableInstance};
use crate::backend::vulkan::memory::MemoryAllocator;
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
    // SAFETY: instance and physical are valid Vulkan handles; the returned array lifetime is tied
    // to the call and we borrow properties locally.
    let properties = unsafe { instance.enumerate_device_extension_properties(physical) }
        .map_err(|e| vk_fail(e, "enumerate device extensions"))?;
    Ok(DEVICE_EXTENSIONS
        .iter()
        .filter(|candidate| {
            properties
                .iter()
                // SAFETY: p.extension_name is a valid null-terminated C string returned by Vulkan
                // for this enumeration; its lifetime persists for as long as properties vector.
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

    // SAFETY: all pointers in create_info (queue create infos, extension names, p_next chain) are valid
    // for the duration of the call; instance and physical are valid. Memory pools/allocators not used (None).
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

/// One GPU object owned by the logical device, with enough information to destroy it.
///
/// Bug №277. The kind is explicit rather than inferred from which handle is
/// non-zero, because several of these share the `GpuResource` shape and a wrong
/// destroy call on the wrong kind is undefined behaviour rather than a crash
/// that points at itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TrackedKind {
    ImageView,
    Image,
    Buffer,
    Sampler,
    ShaderModule,
    DescriptorSetLayout,
    DescriptorPool,
    /// Bug №277, second pass: a pipeline layout was left untracked, so it was one
    /// of the objects still leaking after every other creation site was recorded.
    PipelineLayout,
    RenderPass,
    Framebuffer,
    GraphicsPipeline,
    ComputePipeline,
}

impl TrackedKind {
    /// Translate the backend-neutral release vocabulary into this backend's own
    /// tracking kind. Bug №294.
    ///
    /// Exhaustive on purpose: adding a `ReleaseKind` without deciding what it means
    /// here should be a compile error, not a runtime no-op that quietly leaks.
    fn from_release(kind: ReleaseKind) -> TrackedKind {
        match kind {
            ReleaseKind::Image => TrackedKind::Image,
            ReleaseKind::ImageView => TrackedKind::ImageView,
            ReleaseKind::Framebuffer => TrackedKind::Framebuffer,
            ReleaseKind::Buffer => TrackedKind::Buffer,
            ReleaseKind::Sampler => TrackedKind::Sampler,
            ReleaseKind::ShaderModule => TrackedKind::ShaderModule,
            ReleaseKind::DescriptorSetLayout => TrackedKind::DescriptorSetLayout,
            ReleaseKind::DescriptorPool => TrackedKind::DescriptorPool,
            ReleaseKind::PipelineLayout => TrackedKind::PipelineLayout,
            ReleaseKind::RenderPass => TrackedKind::RenderPass,
            ReleaseKind::GraphicsPipeline => TrackedKind::GraphicsPipeline,
            ReleaseKind::ComputePipeline => TrackedKind::ComputePipeline,
        }
    }

    /// Human-readable, for the leak report. Without a name a count of 400 is
    /// not actionable.
    fn name(self) -> &'static str {
        match self {
            TrackedKind::ImageView => "image view",
            TrackedKind::Image => "image",
            TrackedKind::Buffer => "buffer",
            TrackedKind::Sampler => "sampler",
            TrackedKind::ShaderModule => "shader module",
            TrackedKind::DescriptorSetLayout => "descriptor set layout",
            TrackedKind::DescriptorPool => "descriptor pool",
            TrackedKind::PipelineLayout => "pipeline layout",
            TrackedKind::RenderPass => "render pass",
            TrackedKind::Framebuffer => "framebuffer",
            TrackedKind::GraphicsPipeline => "graphics pipeline",
            TrackedKind::ComputePipeline => "compute pipeline",
        }
    }
}

/// A tracked object plus the device memory backing it, if any.
///
/// `mapped` is kept because freeing memory that is still mapped is a validation
/// error: Vulkan requires `vkUnmapMemory` first, and by the time `Drop` runs
/// there is nothing left that knows whether the allocation was mapped.
#[derive(Debug, Clone, Copy)]
pub(crate) struct TrackedObject {
    pub kind: TrackedKind,
    pub handle: vk::DeviceMemory,
    pub object: u64,
    pub memory: vk::DeviceMemory,
    pub mapped: usize,
    /// The layout the image is currently in. Bug №299.
    ///
    /// Only meaningful for `TrackedKind::Image`. It exists because a barrier's
    /// `oldLayout` must be the layout the image is *actually* in, and nothing else in
    /// the system knows that: usage flags say what an image *may* be used for, not
    /// where it is. A texture declared `COLOR_ATTACHMENT` can equally be in
    /// `SHADER_READ_ONLY_OPTIMAL`, and `download_texture` used to guess from the flag,
    /// so it wrote a barrier claiming `COLOR_ATTACHMENT_OPTIMAL` for an image sitting
    /// in `TRANSFER_SRC_OPTIMAL`. The driver reported
    /// `VUID-vkCmdDraw-None-09600` far from the mistake, and the readback returned
    /// nothing.
    ///
    /// `vk::ImageLayout::UNDEFINED` initially, which is correct: an image starts
    /// undefined until something transitions it.
    pub layout: vk::ImageLayout,
}

impl TrackedObject {
    fn new(kind: TrackedKind, object: u64, memory: u64, mapped: usize) -> Self {
        Self {
            kind,
            handle: vk::DeviceMemory::null(),
            object,
            memory: vk::DeviceMemory::from_raw(memory),
            mapped,
            layout: vk::ImageLayout::UNDEFINED,
        }
    }
}

impl VulkanLogicalDeviceResource {
    /// The layout an image is currently in. Bug №299.
    ///
    /// `None` when the image is not tracked, which means it was never created through
    /// this device — a swapchain image, for instance, whose layout belongs to whoever
    /// owns it. Callers must treat that as "unknown", not as `UNDEFINED`: claiming
    /// `UNDEFINED` in a barrier discards the contents, and claiming anything else
    /// invents a fact.
    pub(crate) fn image_layout(&self, image: u64) -> Option<vk::ImageLayout> {
        let objects = self.tracked.lock().ok()?;
        objects
            .iter()
            .find(|o| o.kind == TrackedKind::Image && o.object == image)
            .map(|o| o.layout)
    }

    /// Record that an image moved to `layout`. Bug №299.
    ///
    /// Called from the submit path after the recorded commands are known, which is
    /// the earliest point where the new layout is a fact rather than an intention.
    /// Best-effort like [`track`](Self::track): a stale layout degrades the next
    /// barrier, it does not corrupt anything on its own.
    pub(crate) fn note_image_layout(&self, image: u64, layout: vk::ImageLayout) {
        let Ok(mut objects) = self.tracked.lock() else {
            return;
        };
        for object in objects.iter_mut() {
            if object.kind == TrackedKind::Image && object.object == image {
                object.layout = layout;
                return;
            }
        }
    }

    /// Update the per-image layout tracker from the barriers in this submit. Bug №299.
    ///
    /// Done at submit time rather than inside the translator, because that is
    /// the earliest moment a new layout is a fact rather than a request. Updating it
    /// during translation would let the tracker change even if the submit later
    /// failed, leaving it confidently wrong.
    ///
    /// Only texture barriers move a layout; memory and buffer barriers do not.
    ///
    /// **Known limit:** a render pass boundary is invisible here. A pass that
    /// transitions an image through its render pass `initial_layout`/`final_layout`
    /// rather than an explicit barrier leaves the tracker stale, and the next barrier
    /// that reads it will be wrong. Making this exact needs render-pass bookkeeping as
    /// well, which is a larger change than the problem it fixes — so callers that
    /// transition through a render pass must not rely on the tracker. Stated rather
    /// than hidden because the failure it produces is a validation error with no
    /// useful message.
    ///
    /// Bug #644: the commands that *overwrite* an image do their own transition
    /// (`transition_whole_image_to_transfer_dst`, Bug #638) and never appear as
    /// a `PipelineBarrier`, so the tracker used to keep saying `UNDEFINED` for
    /// an image that had just been cleared or copied into. The next barrier
    /// that read the tracker then named a stale `oldLayout`, and
    /// `download_texture` read the image with `oldLayout = UNDEFINED` — which
    /// discards the contents, so the readback returned nothing at all.
    ///
    /// The contract is the one Bug #638 established: after a clear or a copy
    /// into an image, the image is in `TRANSFER_DST_OPTIMAL`.
    pub(crate) fn note_layout_transitions(&self, commands: &[crate::command::commands::Command]) {
        use crate::command::commands::Command;
        for command in commands {
            match command {
                Command::PipelineBarrier { texture_barriers, .. } => {
                    for barrier in texture_barriers {
                        let Some(gpu) = barrier.texture.backend() else {
                            continue;
                        };
                        self.note_image_layout(gpu.handle, convert::texture_layout(barrier.new_layout));
                    }
                }
                // The clear commands carry a view; the image is reached
                // through it, the same way the translator reaches it.
                Command::ClearColor { texture, .. } | Command::ClearDepthStencil { texture, .. } => {
                    if let Some(gpu) = texture.texture().backend() {
                        self.note_image_layout(gpu.handle, vk::ImageLayout::TRANSFER_DST_OPTIMAL);
                    }
                }
                Command::CopyBufferToTexture { dst, .. } => {
                    if let Some(gpu) = dst.backend() {
                        self.note_image_layout(gpu.handle, vk::ImageLayout::TRANSFER_DST_OPTIMAL);
                    }
                }
                _ => {}
            }
        }
    }

    /// Record a created object so `Drop` can destroy it.
    ///
    /// Best-effort: a poisoned lock means a previous panic while tracking, and
    /// refusing to create the resource would turn a bookkeeping problem into an
    /// unusable device. Leaking is the lesser failure here, which is why the
    /// error is reported and creation continues.
    pub(crate) fn track(&self, kind: TrackedKind, object: u64, memory: u64, mapped: usize) {
        match self.tracked.lock() {
            Ok(mut objects) => {
                objects.push(TrackedObject::new(kind, object, memory, mapped));
            }
            Err(e) => {
                // `rhi` has no `crate::warn!` — the logging macros live in
                // `render` — so a backend diagnostic is an explicit print here.
                eprintln!(
                    "[rhi/vulkan] cannot track a new {}: the tracked-object list is \
                     poisoned ({e}); this object will leak",
                    kind.name()
                );
            }
        }
    }

    /// Destroy every tracked object in dependency order.
    fn destroy_tracked(&self) -> usize {
        let Ok(objects) = self.tracked.lock() else {
            return 0;
        };
        let count = objects.len();
        for kind in TEARDOWN_ORDER {
            for object in objects.iter().filter(|o| o.kind == kind) {
                // SAFETY: TODO(verify): ensure command buffer is in a state valid for begin (recording state
                // constraints per command pool/usage) before calling begin_command_buffer; caller must guarantee.
                unsafe {
                    // Unmap before freeing: Vulkan requires it, and freeing
                    // still-mapped memory is undefined rather than a shortcut.
                    if object.mapped != 0 {
                        self.device.unmap_memory(object.memory);
                    }
                    let raw = object.object;
                    match kind {
                        TrackedKind::ImageView => {
                            self.device
                                .destroy_image_view(vk::ImageView::from_raw(raw), None);
                        }
                        TrackedKind::Image => {
                            self.device.destroy_image(vk::Image::from_raw(raw), None);
                        }
                        TrackedKind::Buffer => {
                            self.device.destroy_buffer(vk::Buffer::from_raw(raw), None);
                        }
                        TrackedKind::Sampler => {
                            self.device.destroy_sampler(vk::Sampler::from_raw(raw), None);
                        }
                        TrackedKind::ShaderModule => {
                            self.device
                                .destroy_shader_module(vk::ShaderModule::from_raw(raw), None);
                        }
                        TrackedKind::DescriptorSetLayout => {
                            self.device
                                .destroy_descriptor_set_layout(
                                    vk::DescriptorSetLayout::from_raw(raw),
                                    None,
                                );
                        }
                        TrackedKind::DescriptorPool => {
                            self.device
                                .destroy_descriptor_pool(vk::DescriptorPool::from_raw(raw), None);
                        }
                        TrackedKind::PipelineLayout => {
                            self.device
                                .destroy_pipeline_layout(vk::PipelineLayout::from_raw(raw), None);
                        }
                        TrackedKind::RenderPass => {
                            self.device.destroy_render_pass(vk::RenderPass::from_raw(raw), None);
                        }
                        TrackedKind::Framebuffer => {
                            self.device
                                .destroy_framebuffer(vk::Framebuffer::from_raw(raw), None);
                        }
                        TrackedKind::GraphicsPipeline | TrackedKind::ComputePipeline => {
                            self.device.destroy_pipeline(vk::Pipeline::from_raw(raw), None);
                        }
                    }
                    if object.memory != vk::DeviceMemory::null() {
                        self.device.free_memory(object.memory, None);
                    }
                }
            }
        }
        count
    }
}

/// Order in which tracked objects may legally be destroyed.
///
/// Bug №277, second correction. The first version walked the list backwards,
/// which is only a heuristic: it assumes creation order matches dependency
/// order, and that assumption is false. A view created after the framebuffer that
/// uses it is perfectly legal to write, and unwinding backwards then destroys the
/// view first — which showed up as `STATUS_ACCESS_VIOLATION`, "memory cannot be
/// read", at teardown. That is a strictly worse failure than the leak it fixed,
/// because a leak is inert and a wrong free is not.
///
/// So the order below is by dependency, not by age:
///
/// 1. Pipelines — reference layouts, render passes, descriptor sets and modules.
/// 2. Descriptor pools, which free every set allocated from them, then the
///    layouts the sets were written against.
/// 3. Framebuffers — reference the render pass and the views they attach.
/// 4. Render passes and pipeline layouts — reference descriptor set layouts.
/// 5. Samplers and image views — views reference their image.
/// 6. Shader modules.
/// 7. Images and buffers last, each followed by its own device memory.
const TEARDOWN_ORDER: [TrackedKind; 12] = [
    TrackedKind::GraphicsPipeline,
    TrackedKind::ComputePipeline,
    TrackedKind::DescriptorPool,
    TrackedKind::DescriptorSetLayout,
    TrackedKind::Framebuffer,
    TrackedKind::RenderPass,
    TrackedKind::PipelineLayout,
    TrackedKind::Sampler,
    TrackedKind::ImageView,
    TrackedKind::ShaderModule,
    TrackedKind::Image,
    // Bug №284: `Buffer` was missing from this list while `create_buffer` was
    // tracking it, so `destroy_tracked` iterated eleven kinds and never reached
    // the twelfth. Every buffer and its memory survived to `vkDestroyDevice`,
    // which reported VUID-vkDestroyDevice-device-05137 on exit. It only showed up
    // at process exit, so no windowed run could see it.
    //
    // Last because buffers depend on nothing else here and nothing depends on
    // them. The position within the tail is not critical; the presence is.
    //
    // `teardown_order_covers_every_tracked_kind` exists so this cannot recur by
    // omission: adding a kind to `TrackedKind` without adding it here fails that
    // test rather than silently leaking.
    TrackedKind::Buffer,
];

/// Every kind the backend can track, for the coverage test below.
///
/// Duplicated on purpose. Deriving it from the enum would need a derive and a
/// registry, and the point of the test is to catch a hand-written list drifting,
/// so the second list is written out and compared against the real one.
const ALL_TRACKED_KINDS: [TrackedKind; 12] = [
    TrackedKind::ImageView,
    TrackedKind::Image,
    TrackedKind::Buffer,
    TrackedKind::Sampler,
    TrackedKind::ShaderModule,
    TrackedKind::DescriptorSetLayout,
    TrackedKind::DescriptorPool,
    TrackedKind::PipelineLayout,
    TrackedKind::RenderPass,
    TrackedKind::Framebuffer,
    TrackedKind::GraphicsPipeline,
    TrackedKind::ComputePipeline,
];

/// Bug №284: a tracked kind missing from the teardown order is a silent leak.
///
/// It fails only when the device is destroyed, which no test that renders a frame
/// can observe — so the invariant is asserted directly instead. Without this, the
/// omission I made went unnoticed through a full round of leak work.
/// How many frames may be recorded before the CPU has to wait for the GPU.
///
/// Bug №282: there was no such number — one command buffer and one fence meant
/// one frame, and the submit path blocked on the fence to keep it that way. Two is
/// the minimum that lets the CPU build frame N+1 while the GPU draws frame N; the
/// usual argument for more is latency hiding rather than correctness, so this is
/// deliberately not larger than it needs to be.
pub const FRAMES_IN_FLIGHT: usize = 2;

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
        /// One command buffer per frame slot. Bug №282: this was a single buffer,
        /// which forced the submit path to block until the GPU finished before the
        /// CPU could reuse it, and therefore capped the whole renderer at one
        /// frame in flight.
        command_buffers: Vec<vk::CommandBuffer>,
        /// The semaphore `vkQueuePresentKHR` waits on — **one per swapchain
        /// image**, not one per frame slot.
        ///
        /// Bug: it was one per frame slot, and a slot comes round again after
        /// `FRAMES_IN_FLIGHT` frames, while the present that consumed the
        /// semaphore may still be pending. Re-signalling it then is
        /// `VUID-vkQueueSubmit-pSignalSemaphores-00067` — "may still be in use
        /// by VkSwapchainKHR" — and the frame it invalidates is a frame whose
        /// image is shown before the rendering that produced it.
        ///
        /// One per *image* is the arrangement that cannot have that problem: an
        /// image is only handed out again by `vkAcquireNextImageKHR` after its
        /// present completed, so the semaphore belonging to that image is free
        /// by then. The slot-based scheme had no such guarantee: the slot and
        /// the image were unrelated.
        ///
        /// The pool only grows, and nothing is ever destroyed before the
        /// device goes away. A semaphore that a pending present of a retired
        /// chain still holds must not be destroyed, and a chain can be replaced
        /// at any moment by a resize.
        pub render_finished: std::sync::Mutex<Vec<vk::Semaphore>>,
        /// The swapchain image the frame being recorded will be presented into.
        ///
        /// Set by the renderer right after the acquire and before the graph
        /// records, because only the renderer knows which image was acquired.
        present_image: std::sync::atomic::AtomicU32,
        /// The swapchain image the most recent submit signalled for.
        ///
        /// The present asks for the semaphore immediately after the submit that
        /// signals it, so the submit has to record which image it used.
        last_present_image: std::sync::atomic::AtomicU32,
        /// Signalled by `vkAcquireNextImageKHR`, waited by the submit that first
        /// touches the image. One per slot, for the same reason as above: a
        /// semaphore cannot be re-signalled while a previous wait is pending.
        pub image_acquired: Vec<vk::Semaphore>,
    /// Set by the renderer before the graph records, consumed by the next submit.
    ///
    /// Bug №282: only the submit that feeds a swapchain image may signal
    /// `render_finished`. An offscreen submit must not, because a semaphore nobody
    /// waits on stays signalled forever — and then a later present would consume
    /// an already-signalled semaphore and not actually wait for anything.
    presentation_signal_armed: std::sync::atomic::AtomicBool,
    /// Every object created through the RHI, so `Drop` can release them.
    ///
    /// Bug №277: nothing owned a handle, so nothing could be destroyed and
    /// validation reported `VUID-vkDestroyDevice-device-05137: 400 leaked
    /// objects` at process exit.
    ///
    /// A `Drop` on `GpuResource` cannot fix that on its own: `GpuResource` is
    /// copied by value into `Texture`, `Buffer` and `TextureView`, and those are
    /// freely cloned by callers, so one handle has several owners and a
    /// per-value destructor would free it twice. Central ownership here is the
    /// only place with exactly one owner per handle.
    ///
    /// Released in reverse creation order. That is a Vulkan requirement, not
    /// tidiness: a child object must not be destroyed while something that still
    /// references it is alive, and creation order is a reasonable proxy for
    /// dependency order.
    tracked: std::sync::Mutex<Vec<TrackedObject>>,
/// Bug №177: guards `command_pool` and the per-slot command buffers for the
      /// whole record-and-submit window. See `submit_recorded`.
      submit_lock: std::sync::Mutex<()>,
      /// Whether the poisoning of the submit lock has already been reported.
      ///
      /// Bug №662: a poisoned mutex means the previous holder panicked, and the
      /// submit lock covers the whole record-and-submit window — so that panic
      /// happened in the middle of recording, which is exactly the place where
      /// the device could be left in an undefined state. The work continues
      /// anyway (a renderer that stops drawing is a renderer with a black
      /// screen), but the fact is reported once: "everything is fine" and "we
      /// survived a panic" look identical from outside, and only one of them is
      /// true. The report is once, because on every frame after it the log
      /// would say the same thing again and drown everything else.
      submit_lock_poison_reported: std::sync::atomic::AtomicBool,
      /// One fence per frame slot, created once with the device and reset on reuse.
      ///
      /// Bug №282. These used to be a single fence waited on with `u64::MAX` at the
      /// end of every submit, which is what made one frame in flight the only legal
      /// arrangement. The CPU still has to wait on one before reusing its slot —
      /// that wait is the correct place to block, and it no longer blocks the
      /// frame being recorded right now, only the one from `FRAMES_IN_FLIGHT`
      /// submissions ago.
      submit_fences: Vec<vk::Fence>,
      /// The slot the next frame will use.
      ///
      /// Atomic because it is read from two places: the submit path under the
      /// lock, and `image_acquired_semaphore`, which is called by `acquire` before
      /// the submit happens. Relaxed is enough and sufficient: it is a counter
      /// choosing among resources this thread owns, not synchronising anything.
      next_frame: std::sync::atomic::AtomicUsize,
      /// The slot the most recent submit used.
      ///
      /// Bug №282: `present` asks for the render-finished semaphore immediately
      /// after the submit that signals it. Recomputing the slot from `next_frame`
      /// at that point would return the *next* frame's semaphore, and the present
      /// would wait on a semaphore nothing has signalled.
      last_submitted_slot: std::sync::atomic::AtomicUsize,
      /// Whether the frame currently on screen was actually submitted.
      ///
      /// Bug №649: `render_finished_semaphore` used to return the semaphore of
      /// the last submit *ever*, which is the wrong answer whenever the current
      /// frame submitted nothing — the graph recorded no commands, or the submit
      /// failed. Then the present waited on a semaphore that had already been
      /// consumed by the previous present, or on one nothing had ever signalled,
      /// and the chain stalled.
      ///
      /// The arming flag cannot answer this on its own: it is cleared both by a
      /// submit and by a cancel, and "cancelled" and "submitted" need opposite
      /// answers.
      frame_submitted: std::sync::atomic::AtomicBool,
      /// Whether `samplerAnisotropy` is enabled on this device.
      ///
      /// A sampler that asks for anisotropy on a device without the feature is
      /// `VUID-VkSamplerCreateInfo-anisotropyEnable-01070`, so the sampler
      /// creation has to know. The feature is the caller's choice and is off
      /// unless asked for, which means a sampler asking for anisotropy is
      /// clamped rather than refused — the sampler is legal, it just samples
      /// once.
      sampler_anisotropy: bool,
    /// Keeps the owning `VkInstance` alive for as long as this device lives.
    instance_guard: Arc<DestroyableInstance>,
}

impl VulkanLogicalDeviceResource {
    pub(crate) fn new(
        device: ash::Device,
        memory: &MemoryProperties,
        queue_family_index: u32,
        physical_device: vk::PhysicalDevice,
        sampler_anisotropy: bool,
        instance_guard: Arc<DestroyableInstance>,
    ) -> RhiResult<Self> {
        let allocator = MemoryAllocator::new(device.clone(), memory);
        // SAFETY: device is valid; queue_family_index is a valid family for this device; index 0 is valid
        // as created by this device.
        let queue = unsafe { device.get_device_queue(queue_family_index, 0) };
let allocate_info = vk::CommandPoolCreateInfo {
              queue_family_index,
              // Bug №282: required now that buffers are reset individually. The
              // submit path used to call `vkResetCommandPool`, which resets every
              // buffer at once and needs no flag — but it cannot be used with a
              // ring, because it would invalidate the command buffers belonging to
              // the other frames still in flight.
              //
              // Resetting a single buffer without this flag is
              // VUID-vkResetCommandBuffer-commandBuffer-00046, which the validator
              // reported once the reset moved from pool to buffer.
              flags: vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER,
              ..Default::default()
          };
        // Both of these used to fall back to a null handle, so a device whose
        // pool or buffer could not be created still came up "successfully" and
        // then failed on every submit with an error that pointed at the wrong
        // thing. Failing here names the actual cause.
        // SAFETY: device valid; allocate_info fields consistent (queue_family_index valid).
        let command_pool = unsafe { device.create_command_pool(&allocate_info, None) }
            .map_err(|e| vk_fail(e, "create command pool"))?;
        let allocate_info = vk::CommandBufferAllocateInfo {
            command_pool,
            level: vk::CommandBufferLevel::PRIMARY,
            command_buffer_count: (FRAMES_IN_FLIGHT + 1) as u32,
            ..Default::default()
        };
        // SAFETY: device and command_pool (referenced by allocate_info) valid; count within limits.
        let command_buffers = unsafe { device.allocate_command_buffers(&allocate_info) }
            .map_err(|e| vk_fail(e, "allocate command buffers"))?;
        if command_buffers.len() != FRAMES_IN_FLIGHT + 1 {
            return Err(RhiError::BackendError(format!(
                "asked for {} command buffers, got {}",
                FRAMES_IN_FLIGHT,
                command_buffers.len()
            )));
        }

        // One of each per slot, not one shared. Bug №282.
        //
        // Everything below used to be a single handle shared by every frame,
        // which is only correct while exactly one frame is in flight at a time —
        // and the submit path enforced that by blocking on a fence before it
        // returned. So the single-fence rule was not a design choice, it was the
        // thing preventing more than one frame from existing.
        //
        // A Vulkan semaphore may be signalled only once per wait, and may not be
        // signalled again while an earlier wait is still pending. That is
        // VUID-vkQueueSubmit-pSignalSemaphores-00067, and it is why one semaphore
        // cannot serve N frames: re-signalling it next frame trips the rule
        // immediately. Per-slot semaphores remove the sharing, and the blocking
        // wait goes away with them.
        let mut submit_fences = Vec::with_capacity(FRAMES_IN_FLIGHT + 1);
        let mut render_finished = Vec::with_capacity(FRAMES_IN_FLIGHT);
        let mut image_acquired = Vec::with_capacity(FRAMES_IN_FLIGHT);
        // One extra buffer and fence, reserved for one-shot work.
        //
        // Bug #317: one-shot submits used to share the frame ring's counter. That
        // silently stole frame slots — a texture upload between the acquire and the
        // frame's submit advanced the counter, so the submit waited on the *next*
        // slot's acquire semaphore while the one just signalled stayed pending. The
        // next acquire of that slot then failed
        // VUID-vkAcquireNextImageKHR-semaphore-01779. The frame ring and the
        // transfers now have disjoint resources, which is the only way the frame
        // slot is stable for the whole of a frame.
        for slot in 0..=FRAMES_IN_FLIGHT {
            // SAFETY: device valid.
            let fence = unsafe { device.create_fence(&vk::FenceCreateInfo::default(), None) }
                .map_err(|e| vk_fail(e, format!("create submit fence for slot {slot}")))?;
            submit_fences.push(fence);
            // The presentation semaphores belong to frame slots only; the one-shot
            // slot has no image to present and nothing to wait for.
            if slot < FRAMES_IN_FLIGHT {
                // SAFETY: device valid.
                let finished = unsafe { device.create_semaphore(&vk::SemaphoreCreateInfo::default(), None) }
                    .map_err(|e| vk_fail(e, format!("create render-finished semaphore for slot {slot}")))?;
                // SAFETY: device valid.
                let acquired = unsafe { device.create_semaphore(&vk::SemaphoreCreateInfo::default(), None) }
                    .map_err(|e| vk_fail(e, format!("create image-acquired semaphore for slot {slot}")))?;
                render_finished.push(finished);
                image_acquired.push(acquired);
            }
        }

        Ok(Self {
            device,
            render_finished: std::sync::Mutex::new(render_finished),
            present_image: std::sync::atomic::AtomicU32::new(0),
            last_present_image: std::sync::atomic::AtomicU32::new(0),
            image_acquired,
            presentation_signal_armed: std::sync::atomic::AtomicBool::new(false),
            tracked: std::sync::Mutex::new(Vec::new()),
            allocator,
            queue_family_index,
            queue,
            physical_device,
            command_pool,
            command_buffers,
            submit_lock: std::sync::Mutex::new(()),
            submit_lock_poison_reported: std::sync::atomic::AtomicBool::new(false),
            submit_fences,
            next_frame: std::sync::atomic::AtomicUsize::new(0),
            last_submitted_slot: std::sync::atomic::AtomicUsize::new(0),
            frame_submitted: std::sync::atomic::AtomicBool::new(false),
            // The device was created with exactly the features the caller
            // asked for, so this is the same flag that went into the
            // `DeviceCreateInfo` — and if the physical device did not support
            // it, the device creation would have failed rather than silently
            // created without it.
            sampler_anisotropy,
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
/// Submit one-shot work: an upload, a copy, a readback, a legacy direct draw.
///
/// Bug #643: this path must not touch the presentation signal.
///
/// It used to swap `presentation_signal_armed` and take the *frame* slot's
/// semaphores — `image_acquired[frame_slot]` to wait on and
/// `render_finished[frame_slot]` to signal — while submitting into the
/// reserved one-shot slot, which is a different slot entirely
/// (`advances_frame = false`, so the slot is chosen further down).
///
/// A one-shot submit between the arm and the frame's submit therefore
/// consumed the presentation engine's handover signal, so the frame's submit
/// waited on nothing; it raised the semaphore the present waits on, so the
/// frame was shown on a signal from an upload rather than from the rendering;
/// and it cleared the flag, so the frame's submit signalled nothing at all.
/// The frame could be drawn into an image the compositor had not handed over,
/// and shown half-drawn.
///
/// The frame path is `submit_frame`, and it is the only one that may read the
/// arming flag. One-shot work has no presentation of its own, so it waits on
/// nothing and signals nothing.
fn submit_one_shot(
          &self,
          record: impl FnOnce(vk::CommandBuffer) -> RhiResult<()>,
) -> RhiResult<()> {
          let (wait, signal) = submit_semaphores(
              SubmitKind::OneShot,
              0,
              &self.image_acquired,
              None,
          );
          let waits: &[vk::Semaphore] = match &wait {
              Some(semaphore) => std::slice::from_ref(semaphore),
              None => &[],
          };
          let signal = signal.unwrap_or_else(vk::Semaphore::null);
          self.submit_recorded_signalling(record, waits, signal, true, false)
}

      /// Submit the recorded frame without waiting for the GPU.
      ///
      /// Bug №282: the only caller that must not block. Everything else on this
      /// path — uploads, copies, staging helpers — reads its result on return.
      fn submit_frame(&self, record: impl FnOnce(vk::CommandBuffer) -> RhiResult<()>) -> RhiResult<()> {
          use std::sync::atomic::Ordering;
          // `swap` rather than `load`: the flag belongs to exactly one submit, and a
          // `load` would let two consecutive submits both signal the same
          // semaphore, which Vulkan forbids (a semaphore may only be signalled once
          // per wait).
          //
          // Bug №282: the semaphore is now chosen per slot, because one shared
          // semaphore cannot serve two frames — re-signalling it while the previous
          // present is still waiting is VUID-vkQueueSubmit-pSignalSemaphores-00067.
          let armed = self
              .presentation_signal_armed
              .swap(false, Ordering::AcqRel);
          let slot = self.next_frame.load(Ordering::Relaxed) % FRAMES_IN_FLIGHT;
          // The present waits on the semaphore of the *image* this frame
          // renders into, not of the frame slot.
          //
          // A frame slot comes round again after `FRAMES_IN_FLIGHT` frames,
          // while the present that consumed its semaphore may still be
          // pending — and re-signalling a semaphore the swapchain still holds
          // is VUID-vkQueueSubmit-pSignalSemaphores-00067. A swapchain image
          // does not have that problem: it is handed out again only after its
          // present completed.
          let image = self.present_image.load(Ordering::Acquire) as usize;
          // An unarmed frame is not going to the swapchain — an offscreen frame,
          // or a frame whose presentation was cancelled — so it takes no
          // semaphore either.
          let (wait, signal) = if armed {
              submit_semaphores(
                  SubmitKind::Frame,
                  slot,
                  &self.image_acquired,
                  self.render_finished_for(image),
              )
          } else {
              (None, None)
          };
          let waits: &[vk::Semaphore] = match &wait {
              Some(semaphore) => std::slice::from_ref(semaphore),
              None => &[],
          };
          let signal = signal.unwrap_or_else(vk::Semaphore::null);
          let result = self.submit_recorded_signalling(record, waits, signal, false, true);
          // The present asks for the semaphore right after this submit, so the
          // submit has to record which image it signalled for. Only on success:
          // a failed submit signals nothing, and a present waiting on that
          // semaphore would wait forever.
          if result.is_ok() && armed {
              self.last_present_image.store(image as u32, Ordering::Release);
          }
          result
      }

    /// Submit and signal an optional semaphore in addition to the submit fence.
    ///
    /// Bug №282: the fence only covers `queue_submit`, and `vkQueuePresentKHR` was
    /// called with no wait semaphores at all. Nothing therefore connected the two
    /// halves of a frame — render to image, then show that image — so the driver
    /// had no way to know when an image became reusable and `acquire` stopped
    /// handing out more than one of them. The observed symptom was the acquired
    /// index moving from 0 to 2 and then sticking at 2 forever, which is also why
    /// a frame could be rendered into an image that had already been presented.
    ///
    /// The fence stays, because the CPU still needs to know the command buffer can
    /// be reset. The semaphore is what presentation waits on, and consuming it is
    /// what releases the image.
    ///
    /// A null semaphore means there is nothing to signal, which is the correct
    /// answer for offscreen work with no presentation: submitting with a signal
    /// semaphore nobody waits on would leak a signal forever.
    fn submit_recorded_signalling(
        &self,
record: impl FnOnce(vk::CommandBuffer) -> RhiResult<()>,
          waits: &[vk::Semaphore],
          signal: vk::Semaphore,
          block_until_complete: bool,
          advances_frame: bool,
      ) -> RhiResult<()> {
let _guard = lock_submit_lock(&self.submit_lock, &self.submit_lock_poison_reported);
            // Bug #317: one-shot work gets a reserved slot of its own and never advances the
            // frame counter. Sharing the ring's counter let a texture upload steal a
            // frame slot between the acquire and the frame's submit, so the submit
            // waited on the wrong acquire semaphore and the right one stayed pending.
            let slot = if advances_frame {
                self.next_frame.load(std::sync::atomic::Ordering::Relaxed) % FRAMES_IN_FLIGHT
            } else {
                FRAMES_IN_FLIGHT
            };
            let fence = self.submit_fences[slot];

            // A fresh fence starts *unsignalled*, so waiting on one that has never been
            // submitted to blocks forever. On the very first turn around the
            // ring there is nothing outstanding to wait for, and that is not a
            // special case bolted on: slots are visited round-robin, so slot `i`
            // has no previous submission exactly while fewer than
            // `FRAMES_IN_FLIGHT` frames have been submitted.
            //
            // Getting this wrong hangs the process rather than corrupting it —
            // which is how it was found, by watching a test suite time out.
            let frame_number = self.next_frame.load(std::sync::atomic::Ordering::Relaxed);
            let first_use_of_slot = frame_number < FRAMES_IN_FLIGHT || !advances_frame;

            // The CPU may only reuse a slot once its previous submit finished.
            // This is the one wait that remains, and it is the right one: it
            // blocks the frame from `FRAMES_IN_FLIGHT` submissions ago, not the
            // frame being recorded now. Before this existed the wait sat at the
            // end of the submit, which meant the CPU could never get ahead of the
            // GPU at all.
            //
            // Reset follows the wait, never precedes it: a fence cannot be reset
            // while the GPU may still be using it.
            if advances_frame {
                if !first_use_of_slot {
                    // SAFETY: fences array valid and owned by this device; wait parameters valid.
                    unsafe { self.device.wait_for_fences(&[fence], true, u64::MAX) }
                        .map_err(|e| vk_fail(e, "wait for frame fence"))?;
                    // SAFETY: fences belong to this device and are not in use by pending submits.
                    unsafe { self.device.reset_fences(&[fence]) }
                        .map_err(|e| vk_fail(e, "reset frame fence"))?;
                }
            } else {
                // One-shot work does not wait before submitting, because the
                // previous one-shot call already blocked until its fence was
                // signalled. It must still reset: handing `queue_submit` an
                // already-signalled fence is undefined, and the fence is never
                // fresh after the first call.
                // SAFETY: fence valid and owned by this device; previous one-shot call already
                // waited for completion, so fence is not pending.
                unsafe { self.device.reset_fences(&[fence]) }
                    .map_err(|e| vk_fail(e, "reset one-shot fence"))?;
            }

            // Per-buffer reset, not a pool reset. `vkResetCommandPool` invalidates
            // every buffer allocated from it, so resetting the pool here would
            // destroy the buffers belonging to the other in-flight frames — the
            // exact opposite of what a ring needs.
            // SAFETY: command buffer is allocated from a valid pool and not currently pending on GPU.
            unsafe {
                self.device
                    .reset_command_buffer(self.command_buffers[slot], vk::CommandBufferResetFlags::empty())
            }
            .map_err(|e| vk_fail(e, "reset command buffer"))?;

            let begin = vk::CommandBufferBeginInfo::default();
            // SAFETY: command buffer valid and reset; begin info valid.
            unsafe {
                self.device
                    .begin_command_buffer(self.command_buffers[slot], &begin)
            }
            .map_err(|e| vk_fail(e, "begin command buffer"))?;
            record(self.command_buffers[slot])?;
            // SAFETY: command buffer was in recording state and all commands recorded are valid.
            unsafe { self.device.end_command_buffer(self.command_buffers[slot]) }
                .map_err(|e| vk_fail(e, "end command buffer"))?;

            let command_buffer = self.command_buffers[slot];

            // A signal semaphore nobody waits on would stay signalled forever, so
            // the signal is only attached when there is one.
            // `pWaitDstStageMask` must not be null when there is a wait: the
            // stage mask is what says *which* stage waits, and there is
            // nothing sensible to point it at otherwise. Kept as a local
            // because Vulkan reads it at submit time, not at construction.
            let wait_stage = vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT;
            let submit = vk::SubmitInfo {
                command_buffer_count: 1,
                p_command_buffers: &command_buffer,
                wait_semaphore_count: waits.len() as u32,
                p_wait_semaphores: if waits.is_empty() {
                    std::ptr::null()
                } else {
                    waits.as_ptr()
                },
                p_wait_dst_stage_mask: if waits.is_empty() {
                    std::ptr::null()
                } else {
                    &wait_stage as *const _
                },
                signal_semaphore_count: u32::from(signal != vk::Semaphore::null()),
                p_signal_semaphores: if signal == vk::Semaphore::null() {
                    std::ptr::null()
                } else {
                    &signal
                },
                ..Default::default()
            };
            // SAFETY: all semaphores, command buffers and fence in submit are valid and belong to this device;
            // wait stage masks valid when waits present; pointers in submit live as long as call.
            unsafe { self.device.queue_submit(self.queue, &[submit], fence) }
                .map_err(|e| vk_fail(e, "queue submit"))?;
            // `present` runs after this returns and must wait on the semaphore
            // *this* submit signalled, so the slot is recorded rather than
            // recomputed. Only a frame records one: a one-shot submit uses the
            // reserved slot, which has no render-finished semaphore at all, and
            // storing it here would index past the end of that vector.
            if advances_frame {
                self.last_submitted_slot
                    .store(slot, std::sync::atomic::Ordering::Release);
                // Bug #649: the frame *was* submitted, so the present may wait
                // on this slot's render-finished semaphore. Set only after the
                // submit succeeded: a failed submit signals nothing, and a
                // present waiting on that semaphore would wait forever.
                self.frame_submitted
                    .store(true, std::sync::atomic::Ordering::Release);
            }

            // Two kinds of caller share this path, and they need opposite
            // behaviour.
            //
            // One-shot work — texture uploads, buffer copies, the staging helpers
            // — reads its result the moment the call returns. It relied on the
            // submit having completed, which is what the wait at the end of this
            // function used to provide for *every* caller. Removing that wait for
            // the ring silently broke those paths: they mapped staging memory and
            // destroyed buffers while the copy was still queued, which the
            // validation layer reports as a buffer in use and which ended in
            // ERROR_DEVICE_LOST.
            //
            // The frame loop is the opposite case: it must *not* wait, or there
            // is no ring at all.
            if block_until_complete {
                // SAFETY: fence valid and owned by this device; wait parameters valid (allFences=true,
                // single-element array); no concurrent use of this fence elsewhere.
                unsafe { self.device.wait_for_fences(&[fence], true, u64::MAX) }
                    .map_err(|e| vk_fail(e, "wait for one-shot submit"))?;
            }

            // One-shot work never moves the frame counter, so the next frame gets
            // the same slot it would have had.
            if advances_frame {
                self.next_frame.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
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
        // SAFETY: device valid; buffer create info valid (size>0, usage flags consistent).
        let buffer = unsafe { self.device.create_buffer(&info, None) }
            .map_err(|e| vk_fail(e, "create staging buffer"))?;
        // SAFETY: buffer is a valid buffer owned by this device.
        let requirements = unsafe { self.device.get_buffer_memory_requirements(buffer) };
        // The allocation is rounded up to the atom size and the *whole
        // allocation* is mapped, for the same reason as in `create_buffer`:
        // a flush with `VK_WHOLE_SIZE` is legal only if the end of the
        // mapping is a multiple of 64 or is the end of the memory object.
        // Mapping only the buffer's size ends the mapping mid-object, and a
        // staging buffer's size is whatever the copy happened to be — 24
        // bytes for a quad's indices.
        let allocation_size = align_up(requirements.size, 64);
        let (memory, _) = self
            .allocator
            .allocate(
                allocation_size,
                requirements.memory_type_bits,
                // Host-visible only. Coherence used to be demanded here, which
                // made staging allocation fail outright on a device whose only
                // host-visible type is non-coherent; the write is flushed
                // instead, so the extra requirement bought nothing.
                crate::types::MemoryPropertyFlags::HOST_VISIBLE,
            )
            .map_err(|e| RhiError::BufferCreationError(format!("staging memory: {e}")))?;
        // SAFETY: buffer and memory valid, owned by this device; offset 0 within allocation and
        // satisfies buffer memory requirements (memory allocated from allocator for this device).
        unsafe { self.device.bind_buffer_memory(buffer, memory, 0) }
            .map_err(|e| RhiError::BufferCreationError(format!("bind staging memory: {e:?}")))?;
        let ptr = self.allocator.map(memory, allocation_size)?;
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
        // SAFETY: buffer and memory are valid handles owned by this device; unmapping before free
        // prevents freeing a still-mapped allocation (validation error, possible driver leak).
        unsafe {
            self.device.destroy_buffer(buffer, None);
            self.allocator.unmap(memory);
            self.device.free_memory(memory, None);
        }
    }

    /// The present semaphore of a swapchain image, if the pool covers it.
    ///
    /// `None` means the pool does not reach that image — the chain was built
    /// with more images than the pool was told about — and the honest answer is
    /// that there is no semaphore to signal, rather than the semaphore of some
    /// other image.
    fn render_finished_for(&self, image: usize) -> Option<vk::Semaphore> {
        let pool = self.render_finished.lock().ok()?;
        pool.get(image).copied()
    }

    /// Bytes per texel for a format, used to build staging layouts.
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

    /// Bug №282: see `presentation_signal_armed`.
    fn arm_presentation_signal(&self) {
        self.presentation_signal_armed
            .store(true, std::sync::atomic::Ordering::Release);
        // Bug #649: a new frame is pending and nothing has been submitted for
        // it yet. Until a submit says otherwise, the present must not wait on
        // a render-finished semaphore.
        self.frame_submitted
            .store(false, std::sync::atomic::Ordering::Release);
    }

fn render_finished_semaphore(&self) -> vk::Semaphore {
        // Bug №282: per slot. This used to return one shared handle, which is
        // only correct while a single frame is in flight — the second frame would
        // re-signal a semaphore the first present is still waiting on.
        //
        // Bug #649: and only when the current frame was actually submitted. If
        // the graph recorded nothing, or the submit failed, there is no
        // semaphore to wait on: the last submit's semaphore was already
        // consumed by the previous present, and waiting on it again is
        // `VUID-vkQueuePresentKHR-pWaitSemaphores-01271`. A null semaphore
        // means "present without waiting", which is the honest answer for a
        // frame that rendered nothing.
        if !self
            .frame_submitted
            .load(std::sync::atomic::Ordering::Acquire)
        {
            return vk::Semaphore::null();
        }
        self.render_finished_for(self.last_present_image.load(std::sync::atomic::Ordering::Acquire) as usize)
            .unwrap_or_else(vk::Semaphore::null)
    }

    /// The swapchain image the frame being recorded will be presented into.
    ///
    /// Set by the renderer right after the acquire: only the renderer knows
    /// which image was acquired, and the submit has to signal the semaphore
    /// belonging to *that* image.
    fn set_present_image(&self, image_index: u32) {
        self.present_image
            .store(image_index, std::sync::atomic::Ordering::Release);
    }

    /// Make the pool of present semaphores cover `count` swapchain images.
    ///
    /// The pool only grows. A semaphore that a pending present of a retired
    /// chain still holds must not be destroyed, and a chain is replaced by a
    /// resize at any time; the old semaphores are released with the device.
    fn ensure_presentation_images(&self, count: usize) {
        let mut pool = match self.render_finished.lock() {
            Ok(pool) => pool,
            Err(_) => return,
        };
        while pool.len() < count {
            // SAFETY: device valid.
            let semaphore = match unsafe {
                self.device
                    .create_semaphore(&vk::SemaphoreCreateInfo::default(), None)
            } {
                Ok(semaphore) => semaphore,
                Err(e) => {
                    eprintln!("[rhi/vulkan] create render-finished semaphore: {e}");
                    return;
                }
            };
            pool.push(semaphore);
        }
    }

      /// The semaphore `vkAcquireNextImageKHR` should signal for the next frame.
      ///
      /// Bug №282: `acquire` used to block on a fence instead, so the CPU never
      /// got ahead of the presentation engine. Passing the semaphore lets the
      /// submit wait for the handover on the GPU timeline.
      ///
      /// Bug: the slot's fence is waited on **here**, before the acquire, and
      /// not only in the submit. The acquire signals `image_acquired[slot]`, and
      /// the frame that used that slot before is still waiting on it — so
      /// signalling it again is `VUID-vkAcquireNextImageKHR-semaphore-01779`,
      /// "semaphore must not have any pending operations". Waiting in the
      /// submit was too late: the submit happens *after* the acquire.
      ///
      /// The wait is on the slot's fence, which is signalled when the frame
      /// that used the slot finished. The submit resets the fence afterwards,
      /// so handing it to `queue_submit` is still legal.
      fn image_acquired_semaphore(&self) -> vk::Semaphore {
          let slot = self.next_frame.load(std::sync::atomic::Ordering::Relaxed) % FRAMES_IN_FLIGHT;
          // The first turn around the ring has no previous frame, and a fence
          // that was never submitted to would never be signalled.
          if self.next_frame.load(std::sync::atomic::Ordering::Relaxed) >= FRAMES_IN_FLIGHT {
              // SAFETY: the fence belongs to this device and is not in use by a
              // pending submit, because the frame that used it has finished.
              unsafe {
                  self.device
                      .wait_for_fences(&[self.submit_fences[slot]], true, u64::MAX)
              }
              .ok();
          }
          self.image_acquired[slot]
      }

      /// Bug №282: drain a presentation frame that will not be submitted.
      ///
      /// The acquire already signalled `image_acquired[slot]`. With no submit to
      /// wait on it, the semaphore stays signalled, and the slot's next acquire
      /// fails VUID-vkAcquireNextImageKHR-semaphore-01779. Draining is the only
      /// way back: a semaphore cannot be reset, only waited on.
    fn cancel_presentation_frame(&self) {
          let armed = self
              .presentation_signal_armed
              .swap(false, std::sync::atomic::Ordering::AcqRel);
          // Bug #649: a cancelled frame was not submitted, so the present must
          // not wait on a render-finished semaphore for it. The arm already
          // cleared this, but the cancel can also be reached without an arm —
          // and the answer has to be the same either way.
          self.frame_submitted
              .store(false, std::sync::atomic::Ordering::Release);
          if !armed {
              return;
          }
          let slot = self.next_frame.load(std::sync::atomic::Ordering::Relaxed) % FRAMES_IN_FLIGHT;
          let semaphore = self.image_acquired[slot];
          // The signal of a **binary** semaphore can only be consumed by a
          // *submission* that waits on it. `vkWaitSemaphores` is for timeline
          // semaphores only
          // (`VUID-VkSemaphoreWaitInfo-pSemaphores-03256`), and so is
          // `vkCmdWaitSemaphores` — so the drain is a one-shot submission whose
          // `pWaitSemaphores` names the semaphore, and the one-shot path waits
          // for its fence, so the drain is finished when this returns.
          let _ = self.submit_recorded_signalling(
              |_cb| Ok(()),
              &[semaphore],
              vk::Semaphore::null(),
              true,
              false,
          );
    }

    /// Bug #644: the layout this backend believes the image is in.
    ///
    /// Exposed because a stale layout is invisible by construction: the only
    /// thing that reads it is the readback, and the readback's wrong answer
    /// looks like a driver that kept the bits.
    fn image_layout(&self, image: u64) -> Option<crate::resource::TextureLayout> {
        self.image_layout(image)
            .map(convert::rhi_texture_layout)
    }

    /// Bug №289: destroy now rather than at `vkDestroyDevice`.
    ///
    /// The texture is more than the image: its device memory must go with it, and
    /// host-visible memory must be unmapped first. Forgetting any of those three
    /// is exactly the class of mistake bug №284 was.
    ///
    /// `device_wait_idle` is the price of correctness. Vulkan does not permit
    /// destroying an object the GPU may still be reading, and tracking in-flight
    /// usage per object is a real allocator. Callers release on resize rather than
    /// per frame, so the wait is affordable.
    fn release_texture(&self, texture: &crate::resource::Texture) {
        let Some(gpu) = texture.backend() else {
            return;
        };
        // An adopted swapchain image is not ours to destroy.
        //
        // The guard this used to have was `gpu.memory == 0 && gpu.handle == 0`,
        // which only catches a completely empty handle — not what "adopted"
        // means. An adopted image is `GpuResource { handle: image, memory:
        // image_view }`, so the guard never fired for it, and the comment
        // described an intent the code did not implement.
        //
        // The real protection is the tracking list: `release_many` destroys
        // only handles this device created and recorded, and an adopted image
        // was never recorded — `Swapchain` owns it. So the guard is gone and
        // the reason is stated here, where the next reader will look.
        self.release(ReleaseKind::Image, gpu.handle);
    }

    /// Bug №289: framebuffers are replaced when the acquired image or the extent
    /// changes, so the same reasoning as [`release_texture`] applies.
    fn release_framebuffer(&self, framebuffer: &crate::command::pass::render::Framebuffer) {
        let Some(gpu) = framebuffer.backend() else {
            return;
        };
        self.release(ReleaseKind::Framebuffer, gpu.handle);
    }

    /// Bug №289: see [`release_texture`] for why the view is released before the
    /// image it was created from.
    fn release_texture_view(&self, view: &crate::resource::TextureView) {
        let Some(gpu) = view.backend() else {
            return;
        };
        self.release(ReleaseKind::ImageView, gpu.handle);
    }

    /// Drop one object from the teardown list and destroy it immediately.
    ///
    /// Bug №289. Kept separate from `destroy_tracked` so an object that is no longer
    /// needed — a texture replaced by a resize, say — dies when its owner drops it
    /// instead of living until the device does.
    ///
    /// Removed from the list *first*, so a later `destroy_tracked` cannot destroy it
    /// a second time — the same double-free hazard that bug №283 was about.
    ///
    /// Only the handle is needed: memory and mapping come from the tracked entry, so
    /// they are not parameters. Passing them in would be a second source of truth for
    /// a value that must match the entry exactly, and a mismatch would free the wrong
    /// memory.
    fn release(&self, kind: ReleaseKind, object: u64) {
        self.release_many(&[(kind, object)]);
    }

    /// Bug №294: see [`BackendResource::release_many`] for why the wait is per
    /// batch rather than per object.
    fn release_many(&self, objects: &[(ReleaseKind, u64)]) {
        if objects.is_empty() {
            return;
        }
        // Translate first, then look up. The public vocabulary is
        // backend-neutral; `TrackedKind` is this backend's private bookkeeping, and
        // the translation is the single place the two are allowed to meet.
        let wanted: Vec<(TrackedKind, u64)> = objects
            .iter()
            .map(|(kind, handle)| (TrackedKind::from_release(*kind), *handle))
            .collect();

        // Collect first, destroy second. Removing an entry and destroying while still
        // holding the lock would serialise the driver calls behind the mutex, and
        // there is no reason to hold it across `destroy_*`.
        let Ok(mut tracked) = self.tracked.lock() else {
            return;
        };
        let mut entries = Vec::with_capacity(wanted.len());
        for (kind, object) in wanted {
            let Some(position) = tracked
                .iter()
                .position(|o| o.kind == kind && o.object == object)
            else {
                // Not tracked. Either it never was, or it was already released.
                // Either way there is nothing to destroy — and this is what makes a
                // double release harmless rather than a double free.
                continue;
            };
            entries.push(tracked.remove(position));
        }
        drop(tracked);

        if entries.is_empty() {
            return;
        }
        // SAFETY: every handle in `entries` was tracked as created by this device; unmapping before
        // free_memory, GPU idled first (device_wait_idle) so no object is still in use during destruction.
        unsafe {
            // One wait for the whole batch. Vulkan forbids destroying an object the
            // GPU may still be reading, so it cannot be skipped — but it does not
            // need repeating per object.
            self.device.device_wait_idle().ok();
            for entry in entries {
            if entry.mapped != 0 {
                self.device.unmap_memory(entry.memory);
            }
            let raw = entry.object;
            match entry.kind {
                TrackedKind::Image => {
                    self.device.destroy_image(vk::Image::from_raw(raw), None);
                }
                TrackedKind::Framebuffer => {
                    self.device
                        .destroy_framebuffer(vk::Framebuffer::from_raw(raw), None);
                }
                TrackedKind::Buffer => {
                    self.device.destroy_buffer(vk::Buffer::from_raw(raw), None);
                }
                TrackedKind::ImageView => {
                    self.device.destroy_image_view(vk::ImageView::from_raw(raw), None);
                }
                TrackedKind::Sampler => {
                    self.device.destroy_sampler(vk::Sampler::from_raw(raw), None);
                }
                TrackedKind::ShaderModule => {
                    self.device
                        .destroy_shader_module(vk::ShaderModule::from_raw(raw), None);
                }
                TrackedKind::DescriptorSetLayout => {
                    self.device
                        .destroy_descriptor_set_layout(
                            vk::DescriptorSetLayout::from_raw(raw),
                            None,
                        );
                }
                TrackedKind::DescriptorPool => {
                    self.device
                        .destroy_descriptor_pool(vk::DescriptorPool::from_raw(raw), None);
                }
                TrackedKind::PipelineLayout => {
                    self.device
                        .destroy_pipeline_layout(vk::PipelineLayout::from_raw(raw), None);
                }
                TrackedKind::RenderPass => {
                    self.device.destroy_render_pass(vk::RenderPass::from_raw(raw), None);
                }
                TrackedKind::GraphicsPipeline | TrackedKind::ComputePipeline => {
                    self.device.destroy_pipeline(vk::Pipeline::from_raw(raw), None);
                }
            }
            if entry.memory != vk::DeviceMemory::null() {
                self.device.free_memory(entry.memory, None);
            }
            }
        }
    }

    /// Bug №289: see [`BackendResource::tracked_object_count`] for why this exists.
    ///
    /// A poisoned lock would mean a panic while the list was held, which is already
    /// a broken state, but reporting zero would read as "nothing leaked" — the
    /// opposite of the truth and the most misleading answer available. Reporting the
    /// poison as a maximum keeps the observable direction right: the count goes up
    /// and cannot be mistaken for healthy.
    fn tracked_object_count(&self) -> usize {
        match self.tracked.lock() {
            Ok(objects) => objects.len(),
            Err(_) => usize::MAX,
        }
    }

    /// Bug №289: which objects are outstanding, not merely how many.
    ///
    /// A count alone cannot localise a leak — "+2" does not say whether it is two
    /// depth images, two image views, or a pair that grows on every frame. The kind
    /// breakdown does, and that is what identifies which of the three release sites
    /// in `GeometryPass::resolve_target` is missing.
    fn tracked_object_summary(&self) -> String {
        let Ok(objects) = self.tracked.lock() else {
            return "<poisoned>".to_string();
        };
        let mut counts: Vec<(&'static str, usize)> = Vec::new();
        for entry in objects.iter() {
            let name = entry.kind.name();
            match counts.iter_mut().find(|(n, _)| *n == name) {
                Some(slot) => slot.1 += 1,
                None => counts.push((name, 1)),
            }
        }
        counts.sort_unstable();
        if counts.is_empty() {
            return "<empty>".to_string();
        }
        counts
            .into_iter()
            .map(|(name, n)| format!("{name}={n}"))
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn backend_type(&self) -> GraphicsApi {
        GraphicsApi::Vulkan
    }

    fn wait_idle(&self) -> RhiResult<()> {
        // SAFETY: device is a valid logical device owned by this struct.
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
        // SAFETY: device valid; buffer create info valid (size>0, usage flags consistent).
        let buffer = unsafe { self.device.create_buffer(&create_info, None) }
            .map_err(|e| vk_fail(e, "create buffer"))?;
        // SAFETY: buffer is a valid buffer owned by this device.
        let requirements = unsafe { self.device.get_buffer_memory_requirements(buffer) };
        let (memory, _) = self
            .allocator
            .allocate(
                // The allocation is rounded up to the atom size, so the end of
                // the mapping is a multiple of it. `vkFlushMappedMemoryRanges`
                // with `VK_WHOLE_SIZE` is legal only if the end of the current
                // mapping is a multiple of `nonCoherentAtomSize` (64 bytes) or
                // is the end of the memory object, and a buffer whose size is
                // not a multiple of 64 — a quad's 224 bytes of vertices, for
                // one — otherwise fails the flush. The extra bytes are never
                // read or written.
                align_up(requirements.size, 64),
                requirements.memory_type_bits,
                desc.memory_flags,
            )
            .map_err(|e| RhiError::BufferCreationError(format!("{e}")))?;
        // SAFETY: buffer and memory are valid handles owned by this device; memory offset 0
        // within allocation, meets buffer memory requirements.
        unsafe { self.device.bind_buffer_memory(buffer, memory, 0) }
            .map_err(|e| RhiError::BufferCreationError(format!("{e:?}")))?;

        let mut mapped = 0usize;
        if desc.is_host_mappable() {
            // The whole **allocation** is mapped, not just the buffer's size.
            //
            // `vkFlushMappedMemoryRanges` with `VK_WHOLE_SIZE` is legal only if
            // the end of the current mapping is a multiple of
            // `nonCoherentAtomSize` (64 bytes) or is the end of the memory
            // object. Mapping only `desc.size` gives a mapping that ends
            // mid-object, so the flush is a validation error for every buffer
            // whose size is not a multiple of 64 — the 36-byte triangle of a
            // fullscreen pass, for one.
            //
            // Mapping the allocation is safe because the allocator gives every
            // allocation its own dedicated `vk::DeviceMemory`: there is no
            // neighbouring live data to disturb, which is the same reason the
            // flush uses `VK_WHOLE_SIZE` at all.
            let ptr = self.allocator.map(memory, requirements.size)?;
            mapped = ptr as usize;
        }

        let mut out = Buffer::from_desc(desc.clone());
        out.set_backend(GpuResource {
            handle: buffer.as_raw(),
            memory: memory.as_raw(),
            mapped,
        });
        // Bug №277: recorded here because this is the only place that owns the
        // handle exactly once.
        self.track(
            TrackedKind::Buffer,
            buffer.as_raw(),
            memory.as_raw(),
            mapped,
        );

        if desc.usage.contains(BufferUsage::SHADER_DEVICE_ADDRESS) {
            let address_info = vk::BufferDeviceAddressInfo {
                buffer,
                ..Default::default()
            };
            // SAFETY: buffer is a valid buffer created earlier with SHADER_DEVICE_ADDRESS usage; device valid.
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
        // SAFETY: device valid; image create info valid (extent>0, usage flags consistent, format supported).
        let image = unsafe { self.device.create_image(&create_info, None) }
            .map_err(|e| RhiError::TextureCreationError(format!("{e:?}")))?;
        // SAFETY: image is a valid image owned by this device.
        let requirements = unsafe { self.device.get_image_memory_requirements(image) };
        // GPU-resident images use device-local memory; staging uploads later
        // go through an explicit host-visible staging buffer.
        let required = crate::types::MemoryPropertyFlags::DEVICE_LOCAL;
        let (memory, _) = self
            .allocator
            .allocate(requirements.size, requirements.memory_type_bits, required)
            .map_err(|e| RhiError::TextureCreationError(format!("{e}")))?;
        // SAFETY: image and memory are valid handles owned by this device; offset 0 within allocation,
        // memory satisfies image memory requirements.
        unsafe { self.device.bind_image_memory(image, memory, 0) }
            .map_err(|e| RhiError::TextureCreationError(format!("{e:?}")))?;

        let mut out = Texture::from_desc(desc.clone());
        out.set_backend(GpuResource {
            handle: image.as_raw(),
            memory: memory.as_raw(),
            mapped: 0,
        });
        self.track(TrackedKind::Image, image.as_raw(), memory.as_raw(), 0);
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
        // SAFETY: device valid; image view create info valid (image is a live image on this device,
        // view type/format/range compatible with image properties).
        let view = unsafe { self.device.create_image_view(&create_info, None) }
            .map_err(|e| RhiError::TextureCreationError(format!("image view creation failed: {e:?}")))?;

        let mut out = TextureView::from_parts(texture.clone(), desc.clone());
        out.set_backend(GpuResource {
            handle: view.as_raw(),
            memory: 0,
            mapped: 0,
        });
        self.track(TrackedKind::ImageView, view.as_raw(), 0, 0);
        Ok(out)
    }

    fn create_sampler(&self, desc: &SamplerDesc) -> RhiResult<Sampler> {
        // Anisotropy means *more than one* sample, so the predicate is `> 1.0`.
        //
        // It used to be `> 0.0`, which meant a sampler asked for one sample
        // (`max_anisotropy = 1.0`) still had anisotropy switched on — and
        // `max_anisotropy = 0.0` with anisotropy on is not a legal combination
        // at all.
        //
        // And the feature has to be enabled on the device: `samplerAnisotropy`
        // is off unless the caller asked for it (`Features::default()` has it
        // off), and a sampler with anisotropy on a device without the feature
        // is `VUID-VkSamplerCreateInfo-anisotropyEnable-01070`. So the request
        // is clamped to what the device can actually do, and the sampler stays
        // legal either way. Enabling the feature is the caller's decision;
        // producing an invalid sampler is nobody's.
        let anisotropy_requested = desc.max_anisotropy > 1.0;
        let anisotropy_enable = anisotropy_requested && self.sampler_anisotropy;
        let max_anisotropy = if anisotropy_enable {
            desc.max_anisotropy
        } else {
            1.0
        };
        let create_info = vk::SamplerCreateInfo {
            mag_filter: convert::filter(desc.mag_filter),
            min_filter: convert::filter(desc.min_filter),
            mipmap_mode: convert::mipmap_mode(desc.mipmap_mode),
            address_mode_u: convert::address_mode(desc.address_mode_u),
            address_mode_v: convert::address_mode(desc.address_mode_v),
            address_mode_w: convert::address_mode(desc.address_mode_w),
            mip_lod_bias: desc.mip_lod_bias,
            anisotropy_enable: anisotropy_enable as vk::Bool32,
            max_anisotropy,
            compare_enable: desc.compare_enable as vk::Bool32,
            compare_op: convert::compare_op(desc.compare_op),
            min_lod: desc.min_lod,
            max_lod: desc.max_lod,
            border_color: convert::border_color(desc.border_color),
            unnormalized_coordinates: desc.unnormalized_coordinates as vk::Bool32,
            ..Default::default()
        };
        // SAFETY: device valid; sampler create info fields (filters, address modes, LOD range) valid.
        let sampler = unsafe { self.device.create_sampler(&create_info, None) }
            .map_err(|e| vk_fail(e, "create sampler"))?;
        let mut out = Sampler::from_desc(desc.clone());
        out.set_backend(GpuResource {
            handle: sampler.as_raw(),
            memory: 0,
            mapped: 0,
        });
        self.track(TrackedKind::Sampler, sampler.as_raw(), 0, 0);
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
            // SAFETY: gpu.mapped points into a live device memory mapping created by this allocator
            // (not freed elsewhere); data.len() was checked <= buffer.size() above (mapped region covers buffer).
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
        // SAFETY: ptr points into live device memory mapping (from create_staging_buffer), large enough for data.len().
        unsafe {
            std::ptr::copy_nonoverlapping(data.as_ptr(), ptr, data.len());
        }
        self.allocator.flush(staging_memory, data.len() as u64)?;
        let dst = vk::Buffer::from_raw(gpu.handle);
        let result = self.submit_one_shot(|cb| {
            // SAFETY: cb is a valid command buffer in recording state (owned by this device); staging
            // and dst are valid buffers on this device; region covers full valid range of both buffers.
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
        // SAFETY: ptr points into live staging buffer mapping, large enough for `total` bytes (checked above).
        unsafe {
            std::ptr::copy_nonoverlapping(data.as_ptr(), ptr, total as usize);
        }
        self.allocator.flush(staging_memory, total)?;
        let sampled = texture.is_sampled();
        let result = self.submit_one_shot(|cb| {
            // SAFETY: cb valid, in recording state; image valid and owned by this device; layout
            // transitions match the copy (UNDEFINED->TRANSFER_DST before copy, ->SHADER_READ after if sampled).
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
        // Bug #644: the tracker has to know where the image ended up.
        //
        // The two barriers above are written straight into the command buffer,
        // so they never pass through `note_layout_transitions` — the tracker
        // still said `UNDEFINED` for a texture that had just been uploaded.
        // `download_texture` reads the tracked layout, so a readback right
        // after an upload built a barrier with `oldLayout = UNDEFINED`, which
        // discards the contents: the readback returned nothing at all.
        //
        // The layout recorded is the one the upload ended in: a sampled image
        // is left in `SHADER_READ_ONLY_OPTIMAL` for the sampler to read, and
        // anything else stays in `TRANSFER_DST_OPTIMAL`.
        if result.is_ok() {
            self.note_image_layout(
                gpu.handle,
                if sampled {
                    vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL
                } else {
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL
                },
            );
        }
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
        if desc.code.len() < 4 {
            return Err(RhiError::ShaderCompilationError(format!(
                "SPIR-V bytecode is {} bytes, too short to hold a magic number",
                desc.code.len()
            )));
        }
        // The magic number. A length that happens to be a multiple of four is
        // not evidence that the bytes are SPIR-V: a GLSL source file whose
        // length is a multiple of four would pass the check above and be
        // handed to the driver, which rejects it much later and with a message
        // about the shader rather than about the bytes.
        //
        // The repository has a shader compiler that returns GLSL text as
        // "SPIR-V" (`StubShaderCompiler::compile_to_spirv`), so this is not a
        // hypothetical input.
        const SPIRV_MAGIC: u32 = 0x0723_0203;
        let magic = u32::from_le_bytes([desc.code[0], desc.code[1], desc.code[2], desc.code[3]]);
        if magic != SPIRV_MAGIC {
            return Err(RhiError::ShaderCompilationError(format!(
                "the bytes are not SPIR-V: the magic number is {magic:#010x}, expected \
                 {SPIRV_MAGIC:#010x}"
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
        // SAFETY: device valid; SPIR-V bytecode in words is validated above (length multiple of 4);
        // p_code points to valid words vec that lives for duration of the call.
        let shader = unsafe { self.device.create_shader_module(&create_info, None) }
            .map_err(|e| vk_fail(e, "create shader module"))?;
        let mut out = ShaderModule::new(desc.clone());
        out.set_backend(GpuResource {
            handle: shader.as_raw(),
            memory: 0,
            mapped: 0,
        });
        self.track(TrackedKind::ShaderModule, shader.as_raw(), 0, 0);
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
        // SAFETY: device valid; create_info valid (bindings list is live for call, descriptors/types valid).
        let layout = unsafe { self.device.create_descriptor_set_layout(&create_info, None) }
            .map_err(|e| vk_fail(e, "create descriptor set layout"))?;
        let mut out = DescriptorSetLayout::new(desc.clone());
        out.set_backend(GpuResource {
            handle: layout.as_raw(),
            memory: 0,
            mapped: 0,
        });
        self.track(TrackedKind::DescriptorSetLayout, layout.as_raw(), 0, 0);
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
        // SAFETY: device valid; pool_info valid (sizes list is live for call, counts within limits).
        let pool = unsafe { self.device.create_descriptor_pool(&pool_info, None) }
            .map_err(|e| vk_fail(e, "create descriptor pool"))?;
        let alloc_info = vk::DescriptorSetAllocateInfo {
            descriptor_pool: pool,
            descriptor_set_count: 1,
            p_set_layouts: &vk_layout,
            ..Default::default()
        };
        // SAFETY: device and pool are valid; alloc_info valid (vk_layout is a live layout handle on this device).
        let vk_set = unsafe { self.device.allocate_descriptor_sets(&alloc_info) }
            .map_err(|e| vk_fail(e, "allocate descriptor set"))?
            .remove(0);

        let mut out = DescriptorSet::new(layout.clone());
        out.set_backend(GpuResource {
            handle: vk_set.as_raw(),
            memory: pool.as_raw(),
            mapped: 0,
        });
        // The pool owns the set: freeing the pool frees every set allocated from
        // it, so the set itself is tracked as zero-`object` rather than tracked
        // separately. Recording the pool is what actually stops the leak.
        self.track(TrackedKind::DescriptorPool, pool.as_raw(), 0, 0);
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
                        // A depth or stencil view is not an ordinary texture, and
                        // treating it as one is what the RHI could not do before.
                        //
                        // Two Vulkan rules meet here. The first is about the
                        // layout: an image with a depth format that is read by a
                        // shader lives in `DEPTH_STENCIL_READ_ONLY_OPTIMAL`,
                        // not in the layout every other sampled image uses. The
                        // second is about the sampler: a depth image may only be
                        // filtered by a comparison sampler, and a plain
                        // filtering sampler over it is
                        // `VUID-vkCmdDraw-None-08611`.
                        //
                        // The shadow map is exactly such an image - it is
                        // written as a depth attachment and read by the
                        // lighting shader - so without this the RHI had no
                        // way to express the one thing a shadow map is for.
                        let format = view.format();
                        let is_depth = format.is_depth() || format.is_stencil();
                        let image_layout = if is_depth {
                            vk::ImageLayout::DEPTH_STENCIL_READ_ONLY_OPTIMAL
                        } else {
                            vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL
                        };
                        if let Some(s) = sampler {
                            if is_depth && !s.is_comparison() && s.desc().mag_filter == crate::FilterMode::Linear
                            {
                                return Err(RhiError::BackendError(
                                    "a depth texture may only be sampled with a comparison sampler \
                                     or without filtering (VUID-vkCmdDraw-None-08611)"
                                        .into(),
                                ));
                            }
                        }
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
                            image_layout,
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

        // SAFETY: device and dst_set are valid; vk_writes entries point into buffer_infos/image_infos
        // vecs that live for the duration of the call; buffer/image handles in infos are live resources.
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
        // SAFETY: device valid; layout_info valid (set_layouts points to valid layout handle from this device).
        let pipeline_layout = unsafe { self.device.create_pipeline_layout(&layout_info, None) }
            .map_err(|e| {
                RhiError::PipelineCreationError(format!("create pipeline layout: {e:?}"))
            })?;
        let create_info = vk::ComputePipelineCreateInfo {
            stage,
            layout: pipeline_layout,
            ..Default::default()
        };
        // SAFETY: device valid; create_info valid (shader module and layout are live handles on this device).
        let mut pipeline = unsafe {
            self.device.create_compute_pipelines(
                vk::PipelineCache::null(),
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
        // Layout before pipeline, so reverse-order teardown destroys the pipeline first.
        self.track(
            TrackedKind::PipelineLayout,
            pipeline_layout.as_raw(),
            0,
            0,
        );
        self.track(
            TrackedKind::ComputePipeline,
            pipeline.as_raw(),
            0,
            0,
        );
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

        self.submit_one_shot(|cb| {
            // SAFETY: cb valid and in recording state (owned by this device); vk_pipeline/vk_layout/
            // vk_sets are live handles created on this device; barrier describes compute->compute/transfer hazard.
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
        let result = self.submit_one_shot(|cb| {
            // SAFETY: cb valid, in recording state; src and staging are valid buffers on this device;
            // barrier covers the exact [offset, offset+size) range being copied, which was bounds-checked above.
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
        // SAFETY: ptr is the live mapped pointer from create_staging_buffer, covering `size` bytes;
        // to_vec() copies out before staging buffer is destroyed on the next line.
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

        // Bug №299: the old layout came from the usage flags, which say what an
        // image *may* be used for, not where it is. A texture declared
        // `COLOR_ATTACHMENT` may equally be in `SHADER_READ_ONLY_OPTIMAL`, and a
        // barrier claiming `COLOR_ATTACHMENT_OPTIMAL` for an image in
        // `TRANSFER_SRC_OPTIMAL` is `VUID-vkCmdDraw-None-09600` — reported at a
        // draw, nowhere near the cause, with the readback returning nothing.
        //
        // So the layout is read from tracking. An untracked image means the tracker
        // has no opinion — a swapchain image, whose layout belongs to whoever owns
        // it — and that case now fails loudly instead of guessing. Guessing is what
        // made this wrong in the first place.
        let old_layout = self.image_layout(backend.handle).ok_or_else(|| {
            RhiError::BackendError(
                "cannot download a texture whose layout is not tracked: the backend \
                 does not know what layout this image is in, and guessing from its \
                 usage flags produces an invalid barrier. Images created through this \
                 device are tracked; images adopted from elsewhere (swapchain) are \
                 not, and must be transitioned by the caller first"
                    .into(),
            )
        })?;

        // The access masks follow from the *old* layout, not from usage. A layout
        // says who last touched the image, which is exactly what a barrier's source
        // access has to describe.
        let (src_access, src_stage) = match old_layout {
            l if l == vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL => (
                vk::AccessFlags::COLOR_ATTACHMENT_WRITE,
                vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
            ),
            l if l == vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL => (
                vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE,
                vk::PipelineStageFlags::LATE_FRAGMENT_TESTS,
            ),
            // A depth image that has been read by a shader - the shadow map
            // after the lighting pass has sampled it - sits in this layout, and
            // the fallback below claims `SHADER_WRITE` at the fragment stage.
            // That is the wrong access and the wrong stage: the depth was
            // written by the depth test, in the late fragment tests, and the
            // copy could therefore read the depth *before* the write landed.
            // The symptom is a shadow map that reads back as nothing but the
            // clear value, with no error anywhere: the barrier is legal, it
            // just does not wait for what it has to wait for.
            l if l == vk::ImageLayout::DEPTH_STENCIL_READ_ONLY_OPTIMAL => (
                vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE | vk::AccessFlags::SHADER_READ,
                vk::PipelineStageFlags::LATE_FRAGMENT_TESTS
                    | vk::PipelineStageFlags::FRAGMENT_SHADER,
            ),
            l if l == vk::ImageLayout::TRANSFER_DST_OPTIMAL => (
                vk::AccessFlags::TRANSFER_WRITE,
                vk::PipelineStageFlags::TRANSFER,
            ),
            l if l == vk::ImageLayout::TRANSFER_SRC_OPTIMAL => (
                // The image was already a transfer source — a previous readback
                // left it there, and the tracker now says so (Bug #644). Without
                // this arm the fallback claimed `SHADER_WRITE`, which is both the
                // wrong access and the wrong stage: the spec wants
                // `TRANSFER_READ` for a source that was read by a transfer.
                vk::AccessFlags::TRANSFER_READ,
                vk::PipelineStageFlags::TRANSFER,
            ),
            l if l == vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL => (
                vk::AccessFlags::SHADER_READ,
                vk::PipelineStageFlags::FRAGMENT_SHADER,
            ),
            _ => (vk::AccessFlags::SHADER_WRITE, vk::PipelineStageFlags::FRAGMENT_SHADER),
        };

        let result = self.submit_one_shot(|cb| {
            // SAFETY: cb valid, in recording state; image is a live image on this device;
            // barrier describes the exact mip/layer being downloaded and matches the layout
            // transition (old_layout -> TRANSFER_SRC_OPTIMAL) before the copy below.
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
                    // Bug №299: was `ALL_COMMANDS` with no reason. That is legal and
                    // it is also the weakest possible statement — it says nothing
                    // about which access is being waited for, so a later refactor can
                    // change `src_access_mask` and the stage mask stays as wide as
                    // everything, hiding the mistake. The stage now follows the
                    // layout the image is actually in.
                    src_stage,
                    vk::PipelineStageFlags::TRANSFER,
                    vk::DependencyFlags::empty(),
                    &[],
                    &[],
                    std::slice::from_ref(&barrier),
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

        // Bug #644: the readback moved the image into `TRANSFER_SRC_OPTIMAL`
        // and did not put it back, so the tracker had to be told or the next
        // barrier would name a layout the image is no longer in. The old
        // layout is not restored on purpose — the image really is in
        // `TRANSFER_SRC_OPTIMAL` now, and a tracker that claimed otherwise
        // would be the same class of lie this function was fixed for.
        self.note_image_layout(backend.handle, vk::ImageLayout::TRANSFER_SRC_OPTIMAL);

        // Same reason as the buffer readback: the device wrote this range, so the host
        // must not read it before the write is made visible.
        self.allocator.invalidate(staging_memory, total)?;
        // SAFETY: ptr is the live mapped pointer from create_staging_buffer, covering `total` bytes;
        // raw is used to copy out rows before staging buffer is destroyed at end of function.
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
        // SAFETY: device valid; create_info valid (attachments/subpasses/dependencies vectors live
        // for duration of the call, attachment/subpass references consistent).
        let render_pass = unsafe { self.device.create_render_pass(&create_info, None) }
            .map_err(|e| vk_fail(e, "create render pass"))?;

        let mut out = RenderPass::new(desc.clone());
        out.set_backend(GpuResource {
            handle: render_pass.as_raw(),
            memory: 0,
            mapped: 0,
        });
        self.track(TrackedKind::RenderPass, render_pass.as_raw(), 0, 0);
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
        // SAFETY: device valid; render_pass and all image views in views are live handles created
        // on this device; views vector lives for duration of the call.
        let framebuffer = unsafe { self.device.create_framebuffer(&create_info, None) }
            .map_err(|e| vk_fail(e, "create framebuffer"))?;

        let mut out = Framebuffer::new(desc.clone());
        out.set_backend(GpuResource {
            handle: framebuffer.as_raw(),
            memory: 0,
            mapped: 0,
        });
        self.track(TrackedKind::Framebuffer, framebuffer.as_raw(), 0, 0);
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

        // Bug №692: the colour blend state's attachment count has to match the
        // render pass's colour attachment count, and Vulkan does not check it
        // for us: `vkCreateGraphicsPipelines` returns the invalid pipeline as a
        // success. The pipeline then exists, is invalid, and draws nothing —
        // a dark frame with no message, while the light, the camera and the
        // uniforms are all correct.
        //
        // The caller knows how many colour attachments it has; the RHI knows
        // how many the render pass has; and the two have to agree. Checking it
        // here turns a silent wrong picture into an error that names the
        // mismatch.
        let render_pass_colour_attachments = render_pass
            .desc()
            .subpasses
            .first()
            .map(|subpass| subpass.color_attachments.len())
            .unwrap_or(0);
        let blend_attachments = desc
            .color_blend_state
            .as_ref()
            .map(|state| state.attachments.len())
            .unwrap_or(0);
        if blend_attachments != render_pass_colour_attachments {
            return Err(RhiError::PipelineCreationError(format!(
                "the pipeline declares {} colour attachment(s) and the render pass \
                 has {}",
                blend_attachments, render_pass_colour_attachments
            )));
        }

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
        // SAFETY: device valid; layout_info valid (vk_layouts vector is live for the call).
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

        // SAFETY: device valid; create_info references live shader stages, pipeline layout and
        // render pass (all handles from this device); every sub-struct pointer is a reference
        // that outlives the call.
        let mut pipelines = unsafe {
            self.device.create_graphics_pipelines(
                vk::PipelineCache::null(),
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
        // The layout is registered before the pipeline that uses it, so unwinding the
        // reverse order destroys the pipeline first. Registering it after would
        // leave the pipeline referring to a destroyed layout.
        self.track(
            TrackedKind::PipelineLayout,
            pipeline_layout.as_raw(),
            0,
            0,
        );
        self.track(TrackedKind::GraphicsPipeline, pipeline.as_raw(), 0, 0);
        Ok(out)
    }

    fn create_window_surface(
        &self,
        hinstance: u64,
        hwnd: u64,
    ) -> RhiResult<crate::present::WindowSurface> {
        let surface = crate::backend::vulkan::WindowSurface::new_win32(
            &self.instance_guard,
            hinstance as vk::HINSTANCE,
            hwnd as vk::HWND,
        )?;
        // Bug №663: the trait speaks the neutral presentation types; the Vulkan
        // object rides inside the enum and keeps its owner semantics there.
        Ok(crate::present::WindowSurface::vulkan(surface))
    }

    fn create_swapchain(
        &self,
        surface: &crate::present::WindowSurface,
        request: crate::present::SwapchainRequest,
        mut old: Option<&mut crate::present::Swapchain>,
    ) -> RhiResult<crate::present::Swapchain> {
        let Some(vulkan_surface) = surface.as_vulkan() else {
            return Err(RhiError::BackendError(
                "creating a Vulkan swapchain for a surface from another backend".into(),
            ));
        };
        let vulkan_request = crate::backend::vulkan::SwapchainRequest {
            width: request.width,
            height: request.height,
            present_mode: request.present_mode.as_vulkan(),
        };
        // A chain replaced through `oldSwapchain` must genuinely be this
        // backend's chain; one from another backend cannot be crossed over.
        let vulkan_old = old.as_mut().and_then(|chain| chain.as_vulkan_mut());
        // SAFETY: device valid; queue_family_index was validated at device creation (advertised family);
        // index 0 matches the single queue requested in DeviceQueueCreateInfo.
        let queue = unsafe { self.device.get_device_queue(self.queue_family_index, 0) };
        let swapchain = crate::backend::vulkan::Swapchain::create(
            &self.instance_guard,
            self.device.clone(),
            vulkan_surface,
            self.physical_device,
            queue,
            vulkan_request,
            vulkan_old,
        )?;
        Ok(crate::present::Swapchain::vulkan(swapchain))
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
        self.note_layout_transitions(commands);
        // Bug №282: a frame, not a one-shot. Waiting here would put the CPU back
        // in lockstep with the GPU and there would be no ring.
        self.submit_frame(|cb| record::record(&self.device, cb, commands))
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

        // Bug #643: a legacy direct draw is one-shot work. It has no
        // presentation of its own, so it must not take the frame's semaphores
        // — the same rule every other one-shot path follows.
        self.submit_one_shot(|cb| record::record(&self.device, cb, &commands))
    }
}

impl Drop for VulkanLogicalDeviceResource {
    fn drop(&mut self) {
        // SAFETY: every tracked object, fence, semaphore and command pool owned by this device;
        // device_wait_idle is called first so nothing is in GPU use during destruction.
        unsafe {
            // Idle first: every tracked object, the fence and the pool belong to
            // this device, and destroying any of them while the GPU still refers
            // to it is undefined.
            self.device.device_wait_idle().ok();
            // Reported, not ignored. A nonzero count means the caller created
            // something this backend has no kind recorded for, and that leak is
            // then the backend's fault rather than the caller's.
            let leaked = self.destroy_tracked();
            if leaked > 0 {
                eprintln!("[rhi/vulkan] released {leaked} GPU objects at device teardown");
            }
            for fence in &self.submit_fences {
            self.device.destroy_fence(*fence, None);
        }
        // The present semaphores live in a mutex because the pool grows when a
        // swapchain with more images is built.
        if let Ok(pool) = self.render_finished.lock() {
            for semaphore in pool.iter().chain(self.image_acquired.iter()) {
                self.device.destroy_semaphore(*semaphore, None);
            }
        }
              // Every per-slot fence and semaphore above is destroyed in the same
              // loop. `device_wait_idle` above has already drained the device, so
              // none of them can still be referenced by an in-flight present.
              self.device.destroy_command_pool(self.command_pool, None);
              self.device.destroy_device(None);
        }
    }
}

fn align_up(value: u64, align: u64) -> u64 {
    debug_assert!(align.is_power_of_two());
    (value + (align - 1)) & !(align - 1)
}

/// Take the submit lock, and say so if the mutex was poisoned. Bug №662.
///
/// A poisoned mutex means the previous holder panicked. The submit lock covers
/// the whole record-and-submit window, so that panic happened in the middle of
/// recording, and the device may be in a state nobody has checked.
///
/// Continuing is better than refusing to work: a renderer that stops drawing
/// is a renderer with a black screen, and the recording path is a place where
/// a panic is more likely than in most. But the fact must not be silent —
/// "everything is fine" and "we survived a panic" look identical from outside,
/// and only one of them is true.
///
/// The report happens once. On every frame after it, the same line would say
/// the same thing, and the line that matters would be lost among the copies.
///
/// Taken as a free function so it can be tested without a device: poisoning a
/// mutex and then taking it is exactly what a panicking recorder does, and the
/// test does it in a thread.
fn lock_submit_lock<'a>(
    lock: &'a std::sync::Mutex<()>,
    poison_reported: &std::sync::atomic::AtomicBool,
) -> std::sync::MutexGuard<'a, ()> {
    match lock.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            if !poison_reported.swap(true, std::sync::atomic::Ordering::AcqRel) {
                eprintln!(
                    "[rhi/vulkan] the submit lock was poisoned by an earlier panic; the \
                     device is continuing after an interrupted recording"
                );
            }
            poisoned.into_inner()
        }
    }
}

/// What kind of work a submit carries. Bug #643.
///
/// The two kinds differ in whether they take part in presentation at all, and
/// they used to differ in nothing else — which is how a one-shot upload ended
/// up waiting on the presentation engine's handover semaphore and raising the
/// semaphore the present waits on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum SubmitKind {
    /// A frame that will be presented to the swapchain.
    Frame,
    /// One-shot work: an upload, a copy, a readback, a legacy direct draw.
    OneShot,
}

/// The semaphores a submit waits on and signals. Bug #643.
///
/// A frame bound for the swapchain waits on the presentation engine's handover
/// and signals the semaphore the present waits on; both belong to the frame's
/// slot, and both are per-slot because one shared semaphore cannot serve two
/// frames (re-signalling it while the previous present is still waiting is
/// `VUID-vkQueueSubmit-pSignalSemaphores-00067`).
///
/// One-shot work has no presentation of its own, so it takes no semaphore and
/// signals none. It used to take the frame's: it swapped the arming flag and
/// used the *frame* slot's semaphores while submitting into the reserved
/// one-shot slot. A one-shot submit between the arm and the frame's submit
/// then consumed the handover signal, raised the present's semaphore, and
/// cleared the flag, so the frame itself waited on nothing and signalled
/// nothing.
///
/// `None` means "no semaphore"; the caller turns that into a null semaphore,
/// which is what `vkQueueSubmit` wants for "nothing to wait on, nothing to
/// signal".
fn submit_semaphores(
    kind: SubmitKind,
    slot: usize,
    image_acquired: &[vk::Semaphore],
    render_finished: Option<vk::Semaphore>,
) -> (Option<vk::Semaphore>, Option<vk::Semaphore>) {
    match kind {
        SubmitKind::Frame => (Some(image_acquired[slot]), render_finished),
        SubmitKind::OneShot => (None, None),
    }
}

// At the end of the file on purpose: clippy's `items_after_test_module` fires when
// anything follows a `mod tests`, and a warning here is a warning every future
// contributor learns to ignore.

#[cfg(test)]
mod teardown_coverage {
    use super::{ALL_TRACKED_KINDS, TrackedKind, TEARDOWN_ORDER};
    use crate::backend::common::ReleaseKind;
    #[test]
    fn teardown_order_covers_every_tracked_kind() {
        for kind in ALL_TRACKED_KINDS {
            assert!(
                TEARDOWN_ORDER.contains(&kind),
                "{:?} is tracked at creation but absent from TEARDOWN_ORDER, so it \
                 would never be destroyed and Vulkan would report it as leaked at \
                 vkDestroyDevice",
                kind
            );
        }
    }

    #[test]
    fn teardown_order_has_no_duplicates() {
        for (i, kind) in TEARDOWN_ORDER.iter().enumerate() {
            let first = TEARDOWN_ORDER.iter().position(|k| k == kind);
            assert_eq!(
                first,
                Some(i),
                "{:?} appears twice in TEARDOWN_ORDER; the object would be destroyed \
                 twice, which is undefined behaviour",
                kind
            );
        }
    }

    /// Bug №295: every `ReleaseKind` must mean something to this backend.
    ///
    /// `TrackedKind::from_release` is exhaustive, so a new variant cannot be added
    /// without deciding what it maps to. This test guards the other direction — that
    /// the mapping is total and never collapses two release kinds onto one tracked
    /// kind, which would release the wrong object without any compile error.
    #[test]
    fn every_release_kind_maps_to_a_distinct_tracked_kind() {
        let all = [
            ReleaseKind::Image,
            ReleaseKind::ImageView,
            ReleaseKind::Framebuffer,
            ReleaseKind::Buffer,
            ReleaseKind::Sampler,
            ReleaseKind::ShaderModule,
            ReleaseKind::DescriptorSetLayout,
            ReleaseKind::DescriptorPool,
            ReleaseKind::PipelineLayout,
            ReleaseKind::RenderPass,
            ReleaseKind::GraphicsPipeline,
            ReleaseKind::ComputePipeline,
        ];
        for kind in all {
            let mapped = TrackedKind::from_release(kind);
            assert!(
                ALL_TRACKED_KINDS.contains(&mapped),
                "{kind:?} maps to {mapped:?}, which is not a kind this backend tracks; \
                 releasing it would silently free the wrong object"
            );
            let duplicates = all
                .iter()
                .filter(|other| TrackedKind::from_release(**other) == mapped)
                .count();
            assert_eq!(
                duplicates,
                1,
                "{kind:?} and {duplicates} other release kinds all map to {mapped:?}"
            );
        }
    }
}

// At the end of the file on purpose: clippy's `items_after_test_module` fires when
// anything follows a `mod tests`, and a warning here is a warning every future
// contributor learns to ignore.

#[cfg(test)]
mod poisoned_submit_lock {
    use super::lock_submit_lock;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};

    /// A poisoned submit lock must not stop the device, and must be reported.
    ///
    /// Bug №662. The submit lock covers the whole record-and-submit window, so
    /// a poisoned one means a recorder panicked in the middle of recording.
    /// Two things have to hold at once, and they pull in opposite directions:
    /// the work must go on (a renderer that stops drawing shows a black
    /// screen), and the fact must not be silent (a panic that left the device
    /// in an unknown state looks exactly like a device that is fine).
    #[test]
    fn a_poisoned_lock_is_taken_anyway_and_reported_once() {
        let lock = Arc::new(Mutex::new(()));
        // Poison it the way a panicking holder would.
        {
            let clone = lock.clone();
            let _ = std::thread::spawn(move || {
                let _guard = clone.lock().unwrap();
                panic!("the recording panicked");
            })
            .join();
        }
        assert!(
            lock.is_poisoned(),
            "the lock must be poisoned for this test to mean anything"
        );

        let reported = AtomicBool::new(false);
        {
            let guard = lock_submit_lock(&lock, &reported);
            assert!(
                lock.try_lock().is_err(),
                "the lock must actually be held while the guard is alive: taking a \
                 poisoned mutex and then not holding it would let two recorders in"
            );
            drop(guard);
        }
        assert!(
            reported.load(Ordering::Acquire),
            "a poisoned lock must be reported: a panic that interrupted the \
             recording looks exactly like a device that is fine"
        );

        // The second acquisition must not report again: the fact has not
        // changed, and a log that repeats itself every frame drowns the line
        // that matters.
        {
            let _guard = lock_submit_lock(&lock, &reported);
        }
        assert!(
            reported.load(Ordering::Acquire),
            "the report flag stays set"
        );
    }

    /// A healthy lock is taken silently.
    ///
    /// The report is about a panic, and a mutex nobody poisoned is not a
    /// finding — reporting it would make the first real report the second
    /// line in the log.
    #[test]
    fn a_healthy_lock_is_taken_without_a_report() {
        let lock = Mutex::new(());
        let reported = AtomicBool::new(false);
        {
            let _guard = lock_submit_lock(&lock, &reported);
        }
        assert!(
            !reported.load(Ordering::Acquire),
            "a lock nobody poisoned must not be reported as poisoned"
        );
    }
}

#[cfg(test)]
mod presentation_semaphores {
    use super::{submit_semaphores, SubmitKind};
    // `from_raw` is a method of the `Handle` trait, not an inherent one.
    use ash::vk::{self, Handle};

    /// Bug #643: one-shot work must never take the frame's presentation
    /// semaphores.
    ///
    /// It used to: the one-shot path swapped the arming flag and used the
    /// *frame* slot's `image_acquired` and `render_finished`, while
    /// submitting into the reserved one-shot slot. A one-shot submit between
    /// the arm and the frame's submit therefore consumed the presentation
    /// engine's handover signal, raised the semaphore the present waits on,
    /// and cleared the flag — so the frame itself waited on nothing and
    /// signalled nothing, and could be shown half-drawn.
    #[test]
    fn one_shot_work_takes_no_presentation_semaphores() {
        let image_acquired = [
            vk::Semaphore::from_raw(0x1000),
            vk::Semaphore::from_raw(0x2000),
        ];
        // One-shot work signals nothing, so there is no present semaphore to
        // hand it — the call below does not even take one.

        let (wait, signal) =
            submit_semaphores(SubmitKind::OneShot, 0, &image_acquired, None);
        assert!(
            wait.is_none(),
            "one-shot work must not wait on the presentation engine's handover \
             semaphore: it would consume the signal the frame's submit is waiting for"
        );
        assert!(
            signal.is_none(),
            "one-shot work must not signal the semaphore the present waits on: \
             the frame would be shown on a signal from an upload rather than from \
             the rendering"
        );
    }

    /// The frame path does take them — otherwise the fix would be to switch
    /// presentation off rather than to separate the two kinds of submit.
    ///
    /// The acquire semaphore is per *slot*: the slot is what owns the command
    /// buffer and the fence, and the frame that used it before has finished by
    /// the time the slot comes round again.
    ///
    /// The present semaphore is per *image*: the present waits on it, and the
    /// swapchain holds it until the present completes. An image is handed out
    /// again only after that, so its semaphore is free — while a frame slot
    /// comes round long before the present of the frame that used it is over,
    /// and re-signalling then is VUID-vkQueueSubmit-pSignalSemaphores-00067.
    #[test]
    fn a_frame_waits_on_its_slot_and_signals_its_image() {
        let image_acquired = [
            vk::Semaphore::from_raw(0x1000),
            vk::Semaphore::from_raw(0x2000),
        ];
        // One semaphore per swapchain image, not per frame slot: the two
        // swapchain images below are not the two frame slots.
        let render_finished = [
            vk::Semaphore::from_raw(0x3000),
            vk::Semaphore::from_raw(0x4000),
            vk::Semaphore::from_raw(0x5000),
        ];

        for slot in 0..2 {
            for image in 0..render_finished.len() {
                let (wait, signal) = submit_semaphores(
                    SubmitKind::Frame,
                    slot,
                    &image_acquired,
                    Some(render_finished[image]),
                );
                assert_eq!(
                    wait,
                    Some(image_acquired[slot]),
                    "the frame must wait on its own slot's acquire semaphore"
                );
                assert_eq!(
                    signal,
                    Some(render_finished[image]),
                    "the frame must signal the semaphore of the image it renders into"
                );
            }
        }
    }
}
