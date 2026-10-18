//! Device and PhysicalDevice
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use crate::backend::common::{BackendResource, ReleaseKind};
use crate::backend::vulkan::{Swapchain, SwapchainRequest, WindowSurface};
use crate::command::pass::render::{
    Framebuffer, FramebufferAttachment, FramebufferDesc, RenderPass, RenderPassDesc,
};
use crate::error::RhiResult;
use crate::pipeline::graphics::{GraphicsPipeline, GraphicsPipelineDesc};
use crate::resource::{
    AddressMode, Buffer, BufferDesc, FilterMode, Sampler, SamplerDesc, Texture, TextureDesc,
    TextureView, TextureViewDesc,
};
use crate::types::*;
use serde::{Deserialize, Serialize};

/// Physical device type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PhysicalDeviceType {
    Discrete,
    Integrated,
    Virtual,
    Cpu,
}

/// Memory properties
#[derive(Debug, Clone, Default)]
pub struct MemoryProperties {
    pub memory_types: Vec<MemoryType>,
    pub memory_heaps: Vec<MemoryHeap>,
}

/// Memory type
#[derive(Debug, Clone)]
pub struct MemoryType {
    pub flags: MemoryPropertyFlags,
    pub heap_index: u32,
}

/// Memory heap
#[derive(Debug, Clone)]
pub struct MemoryHeap {
    pub size: u64,
    pub flags: MemoryHeapFlags,
}

/// Queue family
#[derive(Debug, Clone)]
pub struct QueueFamily {
    pub index: u32,
    pub flags: QueueFlags,
    pub queue_count: u32,
    pub timestamp_valid_bits: u32,
    pub min_image_transfer_granularity: Extent3D,
}

/// Physical GPU device
#[derive(Debug, Clone)]
pub struct PhysicalDevice {
    pub name: String,
    pub vendor_id: u32,
    pub device_id: u32,
    pub device_type: PhysicalDeviceType,
    pub features: Features,
    pub limits: Limits,
    pub memory_properties: MemoryProperties,
    pub queue_families: Vec<QueueFamily>,
}

impl PhysicalDevice {
    pub fn get_queue_family(&self, flags: QueueFlags) -> Option<&QueueFamily> {
        self.queue_families.iter().find(|q| q.flags.contains(flags))
    }
}

/// Queue
#[derive(Debug, Clone)]
pub struct Queue {
    pub family_index: u32,
    pub index: u32,
    pub flags: QueueFlags,
}

impl Queue {
    pub fn supports_graphics(&self) -> bool {
        self.flags.contains(QueueFlags::GRAPHICS)
    }
    pub fn supports_compute(&self) -> bool {
        self.flags.contains(QueueFlags::COMPUTE)
    }
    pub fn supports_transfer(&self) -> bool {
        self.flags.contains(QueueFlags::TRANSFER)
    }
}

/// Device creation description
#[derive(Debug, Clone, Default)]
pub struct DeviceDesc {
    pub features: Features,
    pub queue_family_indices: Vec<u32>,
    pub validation: bool,
}

/// A backend object that can be destroyed ahead of teardown. Bug №294.
///
/// A thin enum so one batch can mix kinds without exposing raw handles to the
/// render layer. See [`Device::release_many`].
pub enum Releasable<'a> {
    Texture(&'a Texture),
    TextureView(&'a TextureView),
    Framebuffer(&'a Framebuffer),
}

/// Logical device
pub struct Device {
    physical_device: PhysicalDevice,
    queues: Vec<Queue>,
    backend_resource: Option<Box<dyn BackendResource>>,
}

impl Device {
    pub fn from_parts(
        physical_device: PhysicalDevice,
        queues: Vec<Queue>,
        backend_resource: Box<dyn BackendResource>,
    ) -> Self {
        Self {
            physical_device,
            queues,
            backend_resource: Some(backend_resource),
        }
    }

    pub fn physical_device(&self) -> &PhysicalDevice {
        &self.physical_device
    }
    pub fn queues(&self) -> &[Queue] {
        &self.queues
    }
    pub fn graphics_queue(&self) -> Option<&Queue> {
        self.queues.iter().find(|q| q.supports_graphics())
    }
    pub fn backend_resource(&self) -> Option<&dyn BackendResource> {
        self.backend_resource.as_deref()
    }
    pub fn wait_idle(&self) -> RhiResult<()> {
        if let Some(backend_resource) = &self.backend_resource {
            backend_resource.wait_idle()
        } else {
            Ok(())
        }
    }

    /// Create a graphics pipeline from a description.
    ///
    /// Takes the render pass and the descriptor set layouts because the backend
    /// genuinely cannot build a pipeline without them: the subpass index and the
    /// attachment compatibility come from the pass, and the layout from the
    /// layouts. Bug №314 — this method used to accept only `desc` and return
    /// `Ok(GraphicsPipeline::new(desc.clone()))` unconditionally, never calling
    /// the backend. The pipeline it produced had `backend: None`, could not be
    /// drawn with, and reported success, which is the most convincing kind of
    /// broken: a caller would bind it and draw nothing.
    ///
    /// There is no fallback here on purpose. A descriptor-only pipeline is
    /// exactly the defect, so returning one again would reintroduce it with the
    /// signature change as cover.
    pub fn create_graphics_pipeline(
        &self,
        desc: &GraphicsPipelineDesc,
        render_pass: &RenderPass,
        set_layouts: &[&crate::descriptor::DescriptorSetLayout],
    ) -> RhiResult<GraphicsPipeline> {
        if let Some(backend) = &self.backend_resource {
            return backend.create_graphics_pipeline(desc, render_pass, set_layouts);
        }
        Err(crate::error::RhiError::BackendError(
            "active backend cannot create graphics pipelines".into(),
        ))
    }

    /// Create a render pass from a description.
    ///
    /// Bug №314, the same defect as `create_graphics_pipeline`: this returned
    /// `RenderPass::new(desc)` without ever calling the backend, so the pass had
    /// no native handle. It was worse there, because it did not even return a
    /// `Result` — there was nowhere for a failure to be reported and nothing to
    /// distinguish a real pass from a descriptor.
    ///
    /// Found while fixing №314 in the pipeline path: fixing pipelines alone
    /// would have left every pipeline unable to reach a usable pass.
    pub fn create_render_pass(
        &self,
        desc: &RenderPassDesc,
    ) -> crate::error::RhiResult<RenderPass> {
        if let Some(backend) = &self.backend_resource {
            return backend.create_render_pass(desc);
        }
        Err(crate::error::RhiError::BackendError(
            "active backend cannot create render passes".into(),
        ))
    }

    /// Create a framebuffer from the given render pass and texture views.
    pub fn create_framebuffer(
        &self,
        render_pass: &RenderPass,
        attachments: Vec<TextureView>,
        width: u32,
        height: u32,
    ) -> Framebuffer {
        let attachments = attachments
            .into_iter()
            .map(|texture_view| FramebufferAttachment {
                texture_view,
                layer: 0,
                mip_level: 0,
            })
            .collect();
        Framebuffer::new(FramebufferDesc {
            render_pass: render_pass.clone(),
            attachments,
            width,
            height,
            layers: 1,
        })
    }

    /// Create a sampler with uniform wrap/filter modes.
    pub fn create_sampler(&self, wrap_mode: AddressMode, filter_mode: FilterMode, anisotropy: f32) -> Sampler {
        let desc = SamplerDesc {
            mag_filter: filter_mode,
            min_filter: filter_mode,
            mipmap_mode: filter_mode,
            address_mode_u: wrap_mode,
            address_mode_v: wrap_mode,
            address_mode_w: wrap_mode,
            max_anisotropy: anisotropy,
            ..Default::default()
        };
        if let Some(backend) = &self.backend_resource {
            if let Ok(sampler) = backend.create_sampler(&desc) {
                return sampler;
            }
        }
        Sampler::from_desc(desc)
    }

    /// Create a 2D/3D texture (convenience used by the legacy client render code).
    pub fn create_texture(
        &self,
        width: u32,
        height: u32,
        depth: u32,
        format: Format,
        usage: TextureUsage,
        mip_levels: u32,
    ) -> Texture {
        let desc = TextureDesc {
            width,
            height,
            depth,
            mip_levels,
            array_layers: 1,
            format,
            usage,
            sample_count: SampleCount::X1,
            dimensions: TextureDimensions::D2,
            sharing_mode: SharingMode::Exclusive,
            queue_family_indices: Vec::new(),
        };
        if let Some(backend) = &self.backend_resource {
            if let Ok(texture) = backend.create_texture(&desc) {
                return texture;
            }
        }
        Texture::from_desc(desc)
    }

    /// Create a texture from a full description.
    pub fn create_texture_from_desc(&self, desc: &TextureDesc) -> RhiResult<Texture> {
        if let Some(backend) = &self.backend_resource {
            if let Ok(texture) = backend.create_texture(desc) {
                return Ok(texture);
            }
        }
        Ok(Texture::from_desc(desc.clone()))
    }

    /// Create a texture view on the active backend.
    pub fn create_texture_view(&self, texture: &Texture, desc: &TextureViewDesc) -> RhiResult<TextureView> {
        if let Some(backend) = &self.backend_resource {
            if let Ok(view) = backend.create_texture_view(texture, desc) {
                return Ok(view);
            }
        }
        Ok(TextureView::from_parts(texture.clone(), desc.clone()))
    }

    /// Create a buffer (convenience used by the legacy client render code).
    pub fn create_buffer(&self, size: u64, usage: BufferUsage, cpu_visible: bool) -> Buffer {
        let memory_flags = if cpu_visible {
            MemoryPropertyFlags::HOST_VISIBLE | MemoryPropertyFlags::HOST_COHERENT
        } else {
            MemoryPropertyFlags::DEVICE_LOCAL
        };
        let desc = BufferDesc {
            size,
            usage,
            memory_flags,
            sharing_mode: SharingMode::Exclusive,
            queue_family_indices: Vec::new(),
        };
        if let Some(backend) = &self.backend_resource {
            if let Ok(buffer) = backend.create_buffer(&desc) {
                return buffer;
            }
        }
        Buffer::from_desc(desc)
    }

    /// Create a buffer from a full description.
    pub fn create_buffer_from_desc(&self, desc: &BufferDesc) -> RhiResult<Buffer> {
        if let Some(backend) = &self.backend_resource {
            if let Ok(buffer) = backend.create_buffer(desc) {
                return Ok(buffer);
            }
        }
        Ok(Buffer::from_desc(desc.clone()))
    }

    /// Upload data to a buffer. Delegates to the active backend when the
    /// buffer has GPU backing, otherwise acts as a CPU-side no-op.
    ///
    /// Returns the backend's result. It used to be discarded here, which made a
    /// failed upload indistinguishable from a successful one: the caller went on
    /// to bind a buffer that held nothing, and the draw produced nothing with
    /// no error anywhere.
    pub fn upload_buffer<T>(&self, buffer: &Buffer, data: &[T]) -> RhiResult<()> {
        let Some(backend) = &self.backend_resource else {
            return Ok(());
        };
        let bytes = unsafe {
            std::slice::from_raw_parts(
                data.as_ptr() as *const u8,
                data.len().saturating_mul(std::mem::size_of::<T>()),
            )
        };
        backend.upload_buffer(buffer, bytes)
    }

    /// Returns a view into the host-mapped memory of a host-visible buffer,
    /// if the buffer has GPU backing and its memory is mapped.
    ///
    /// The backend is asked to make pending device writes visible first: for
    /// non-coherent host memory the slice would otherwise show the previous
    /// contents of the range.
    pub fn buffer_data(&self, buffer: &Buffer) -> Option<&[u8]> {
        let gpu = buffer.backend()?;
        if gpu.mapped == 0 {
            return None;
        }
        if let Some(backend) = &self.backend_resource {
            backend.invalidate_mapped_buffer(buffer).ok();
        }
        Some(unsafe { std::slice::from_raw_parts(gpu.mapped as *const u8, buffer.size() as usize) })
    }

    /// Upload data to a texture (delegated to the backend when available).
    ///
    /// Returns the backend's result rather than discarding it, for the same
    /// reason as [`Self::upload_buffer`].
    pub fn upload_texture(&self, texture: &Texture, data: &[u8]) -> RhiResult<()> {
        let Some(backend) = &self.backend_resource else {
            return Ok(());
        };
        backend.upload_texture(texture, data)
    }

    /// Compile a shader module on the active backend.
    pub fn create_shader_module(&self, desc: &crate::shader::module::ShaderModuleDesc) -> crate::shader::module::ShaderModule {
        if let Some(backend) = &self.backend_resource {
            if let Ok(module) = backend.create_shader_module(desc) {
                return module;
            }
        }
        crate::shader::module::ShaderModule::new(desc.clone())
    }

    /// Create a descriptor set layout on the active backend.
    pub fn create_descriptor_set_layout(
        &self,
        desc: &crate::descriptor::DescriptorSetLayoutDesc,
    ) -> crate::descriptor::DescriptorSetLayout {
        if let Some(backend) = &self.backend_resource {
            if let Ok(layout) = backend.create_descriptor_set_layout(desc) {
                return layout;
            }
        }
        crate::descriptor::DescriptorSetLayout::new(desc.clone())
    }

    /// Allocate a descriptor set on the active backend.
    pub fn create_descriptor_set(
        &self,
        layout: &crate::descriptor::DescriptorSetLayout,
    ) -> RhiResult<crate::descriptor::DescriptorSet> {
        if let Some(backend) = &self.backend_resource {
            if let Ok(set) = backend.create_descriptor_set(layout) {
                return Ok(set);
            }
        }
        Err(crate::error::RhiError::BackendError(
            "active backend cannot allocate descriptor sets".into(),
        ))
    }

    /// Write descriptors into a set on the active backend.
    pub fn write_descriptors(
        &self,
        set: &crate::descriptor::DescriptorSet,
        writes: &[crate::descriptor::DescriptorWrite],
    ) -> RhiResult<()> {
        if let Some(backend) = &self.backend_resource {
            return backend.write_descriptors(set, writes);
        }
        Ok(())
    }

    /// Create a compute pipeline on the active backend.
    pub fn create_compute_pipeline(
        &self,
        desc: &crate::pipeline::compute::ComputePipelineDesc,
        layout: &crate::descriptor::DescriptorSetLayout,
    ) -> RhiResult<crate::pipeline::compute::ComputePipeline> {
        if let Some(backend) = &self.backend_resource {
            return backend.create_compute_pipeline(desc, layout);
        }
        Err(crate::error::RhiError::BackendError(
            "active backend cannot create compute pipelines".into(),
        ))
    }

    /// Dispatch a compute pipeline on the active backend.
    pub fn dispatch_compute(
        &self,
        pipeline: &crate::pipeline::compute::ComputePipeline,
        sets: &[&crate::descriptor::DescriptorSet],
        group_count_x: u32,
        group_count_y: u32,
        group_count_z: u32,
    ) {
        if let Some(backend) = &self.backend_resource {
            let _ = backend.dispatch_compute(pipeline, sets, group_count_x, group_count_y, group_count_z);
        }
    }

    /// Read a range of a buffer back to the host on the active backend.
    pub fn download_buffer(&self, buffer: &Buffer, offset: u64, size: u64) -> RhiResult<Vec<u8>> {
        if let Some(backend) = &self.backend_resource {
            return backend.download_buffer(buffer, offset, size);
        }
        Err(crate::error::RhiError::BackendError(
            "active backend cannot download buffers".into(),
        ))
    }

    /// Create a render pass on the active backend.
    pub fn create_gpu_render_pass(&self, desc: &RenderPassDesc) -> RhiResult<RenderPass> {
        if let Some(backend) = &self.backend_resource {
            return backend.create_render_pass(desc);
        }
        Err(crate::error::RhiError::BackendError(
            "active backend cannot create render passes".into(),
        ))
    }

    /// Create a framebuffer on the active backend.
    pub fn create_gpu_framebuffer(&self, desc: &FramebufferDesc) -> RhiResult<Framebuffer> {
        if let Some(backend) = &self.backend_resource {
            return backend.create_framebuffer(desc);
        }
        Err(crate::error::RhiError::BackendError(
            "active backend cannot create framebuffers".into(),
        ))
    }

    /// Create a graphics pipeline on the active backend.
    pub fn create_gpu_graphics_pipeline(
        &self,
        desc: &GraphicsPipelineDesc,
        render_pass: &RenderPass,
        set_layouts: &[&crate::descriptor::DescriptorSetLayout],
    ) -> RhiResult<GraphicsPipeline> {
        if let Some(backend) = &self.backend_resource {
            return backend.create_graphics_pipeline(desc, render_pass, set_layouts);
        }
        Err(crate::error::RhiError::BackendError(
            "active backend cannot create graphics pipelines".into(),
        ))
    }

    /// Replay a recorded command list on the GPU.
    ///
    /// The consumer side of the RHI command path: `CommandEncoder` records
    /// `Command`s, `finish()` moves them into the buffer, and this hands them
    /// to the backend to translate. An empty list is refused rather than
    /// submitted, because "the recording produced nothing" is a real and common
    /// state that should be visible.
    pub fn submit_recorded(&self, commands: &[crate::command::commands::Command]) -> RhiResult<()> {
        let Some(backend) = &self.backend_resource else {
            return Err(crate::error::RhiError::BackendError(
                "no active backend to submit commands to".into(),
            ));
        };
        backend.submit_commands(commands)
    }

    /// Give back a presentation frame whose submit will not happen. Bug №282.
    ///
    /// See [`BackendResource::cancel_presentation_frame`](crate::backend::BackendResource::cancel_presentation_frame).
    pub fn cancel_presentation_frame(&self) {
        if let Some(backend) = &self.backend_resource {
            backend.cancel_presentation_frame();
        }
    }

    /// Destroy a texture now instead of at device teardown.
///
/// Bug №289: objects go into a teardown list that is only walked at
/// `vkDestroyDevice`. A texture replaced on every resize therefore stayed
/// resident in VRAM for the whole session — one leak per resize step, which is
/// not visible in any test and very visible in a long play session.
///
/// Waits for the device to be idle first, because Vulkan forbids destroying an
/// object the GPU may still be reading. That is affordable here: callers release
/// on resize, not per frame.
pub fn release_texture(&self, texture: &crate::resource::Texture) {
    let Some(backend) = &self.backend_resource else {
        return;
    };
    backend.release_texture(texture);
}

/// Destroy a framebuffer now instead of at device teardown.
///
/// Same reasoning as [`release_texture`](Self::release_texture); see bug №289.
pub fn release_framebuffer(&self, framebuffer: &crate::command::pass::render::Framebuffer) {
    let Some(backend) = &self.backend_resource else {
        return;
    };
    backend.release_framebuffer(framebuffer);
}

    /// Destroy a texture view now instead of at device teardown.
    ///
    /// Bug №289. Must be called before the texture the view was created from, or
    /// the view is left referencing freed memory — which the driver does not report
    /// at the mistake but later, at whatever command used it.
    pub fn release_texture_view(&self, view: &crate::resource::TextureView) {
        let Some(backend) = &self.backend_resource else {
            return;
        };
        backend.release_texture_view(view);
    }

    /// How many backend objects are still alive and waiting for teardown.
    ///
    /// Bug №289. Zero means nothing is tracked, which is the honest answer for a
    /// backend that keeps no list.
    pub fn tracked_object_count(&self) -> usize {
        match &self.backend_resource {
            Some(backend) => backend.tracked_object_count(),
            None => 0,
        }
    }

    /// Wrap an image this device did not create, without taking ownership.
    ///
    /// Bug №298. For swapchain images and nothing else: they are owned by
    /// `Swapchain`, which destroys them, and this handle is deliberately *not*
    /// tracked, so nothing here will destroy it a second time. Adopting a handle that
    /// this crate already tracks would produce exactly that.
    ///
    /// The reason it exists is asymmetry: `create_texture` returns a typed `Texture`
    /// that any pass can build a framebuffer from, but swapchain images were
    /// reachable only as raw `u64` handles, so a pass could target the window and
    /// could not target an offscreen texture — which made the window path impossible
    /// to verify by reading pixels back.
    pub fn adopt_texture(&self, desc: crate::resource::TextureDesc, image_handle: u64) -> Texture {
        crate::resource::Texture::adopt(desc, image_handle)
    }

    /// Which backend objects are still alive, broken down by kind. Bug №289.
    pub fn tracked_object_summary(&self) -> String {
        match &self.backend_resource {
            Some(backend) => backend.tracked_object_summary(),
            None => String::new(),
        }
    }

    /// Destroy several tracked objects with a single device wait. Bug №294.
    ///
    /// Takes resource references rather than raw handles on purpose: `backend()` is
    /// private to this crate, which is exactly why the single-object helpers exist
    /// at all. Exposing handles would undo that, and the caller has no way to be
    /// right about them.
    ///
    /// Order is the caller's responsibility and is not checked: the backend destroys
    /// in the order given, and Vulkan requires dependents before dependencies. A
    /// wrong order is a use-after-free, so it is left visible rather than silently
    /// sorted.
    pub fn release_many(&self, objects: &[Releasable<'_>]) {
        let Some(backend) = &self.backend_resource else {
            return;
        };
        let raw: Vec<(ReleaseKind, u64)> = objects
            .iter()
            .filter_map(|item| match item {
                Releasable::Texture(texture) => {
                    texture.backend().map(|gpu| (ReleaseKind::Image, gpu.handle))
                }
                Releasable::TextureView(view) => {
                    view.backend().map(|gpu| (ReleaseKind::ImageView, gpu.handle))
                }
                Releasable::Framebuffer(framebuffer) => framebuffer
                    .backend()
                    .map(|gpu| (ReleaseKind::Framebuffer, gpu.handle)),
            })
            .collect();
        backend.release_many(&raw);
    }

    /// Bug №282: `vkQueuePresentKHR` has to wait on a semaphore that the frame's
    /// submit signals, and the submit happens deep inside the render graph, which
    /// has no idea whether its output reaches a swapchain. The renderer arms this
    /// before running the graph and reads the semaphore back afterwards.
    ///
    /// A flag rather than a parameter, because threading a "is this presented"
    /// argument through `RenderGraph::execute` would put a presentation concern
    /// into the graph for the sake of one caller.
    pub fn arm_presentation_signal(&self) {
        if let Some(backend) = &self.backend_resource {
            backend.arm_presentation_signal();
        }
    }

    /// The semaphore [`present`](Self::arm_presentation_signal) waits on.
    ///
    /// Null when the active backend is not Vulkan, which is exactly what `present`
    /// needs to skip the wait rather than wait on nothing.
    pub fn render_finished_semaphore(&self) -> crate::ash::vk::Semaphore {
        self.backend_resource
            .as_ref()
            .map(|b| b.render_finished_semaphore())
            .unwrap_or_else(crate::ash::vk::Semaphore::null)
    }

    /// Create a window surface from raw Win32 handles (`HINSTANCE`, `HWND`),
    /// as obtained from `raw-window-handle`.
    ///
    /// Step 2 of the render plan. Until this existed there was no way to reach
    /// a screen: `Renderer::new` took a window pointer and ignored it, and the
    /// `SwapChain` type was a CPU-only stub.
    pub fn create_window_surface(&self, hinstance: u64, hwnd: u64) -> RhiResult<WindowSurface> {
        let Some(backend) = &self.backend_resource else {
            return Err(crate::error::RhiError::BackendError(
                "no active backend for window presentation".into(),
            ));
        };
        backend.create_window_surface(hinstance, hwnd)
    }

    /// Create a swapchain for an existing surface.
    ///
    /// Pass `old` when replacing a chain. It has to reach Vulkan as
    /// `oldSwapchain` — creating a second chain for the same window and dropping
    /// the first afterwards fails with `ERROR_NATIVE_WINDOW_IN_USE_KHR`.
    pub fn create_swapchain(
        &self,
        surface: &WindowSurface,
        request: SwapchainRequest,
        old: Option<&mut Swapchain>,
    ) -> RhiResult<Swapchain> {
        let Some(backend) = &self.backend_resource else {
            return Err(crate::error::RhiError::BackendError(
                "no active backend for swapchain creation".into(),
            ));
        };
        backend.create_swapchain(surface, request, old)
    }

    /// A render pass matching a presentation surface's negotiated format.
    ///
    /// The swapchain picks the format, not the caller, so a pass has to be built
    /// for whatever it reports — hardcoding `B8G8R8A8_SRGB` produces a pass that
    /// is incompatible with the surface on any driver that chose differently.
    pub fn presentation_render_pass(&self, format: Format) -> RhiResult<RenderPass> {
        let Some(backend) = &self.backend_resource else {
            return Err(crate::error::RhiError::BackendError(
                "no active backend for a presentation render pass".into(),
            ));
        };
        let desc = backend.presentation_render_pass_desc(format)?;
        self.create_gpu_render_pass(&desc)
    }

    /// A presentation render pass that also has a depth attachment.
    ///
    /// Without a depth attachment and a depth test, a closed solid renders as
    /// an open shell: the far walls pass the depth-less rasteriser and are drawn
    /// over the near ones, so any volumetric object looks inside out. The
    /// caller must then supply a depth image of the same size and format.
    pub fn presentation_render_pass_with_depth(
        &self,
        format: crate::types::Format,
        depth_format: crate::types::Format,
    ) -> RhiResult<RenderPass> {
        let Some(backend) = &self.backend_resource else {
            return Err(crate::error::RhiError::BackendError(
                "no active backend for a presentation render pass".into(),
            ));
        };
        let desc = backend.presentation_render_pass_desc_with_depth(format, depth_format)?;
        self.create_gpu_render_pass(&desc)
    }

    /// A framebuffer rendering into one image of a swapchain, plus a depth image.
    ///
    /// The attachment order has to match the render pass: the colour view is
    /// attachment 0 and the depth view is attachment 1, which is what
    /// `presentation_render_pass_with_depth` sets up.
    pub fn swapchain_image_framebuffer_with_depth(
        &self,
        render_pass: &RenderPass,
        image: u64,
        image_view: u64,
        depth_view: crate::resource::TextureView,
        format: crate::types::Format,
        width: u32,
        height: u32,
    ) -> RhiResult<Framebuffer> {
        let (_texture, view) =
            Texture::adopt_backend_image(width, height, format, image, image_view);
        self.create_gpu_framebuffer(&FramebufferDesc {
            render_pass: render_pass.clone(),
            attachments: vec![
                FramebufferAttachment { texture_view: view, layer: 0, mip_level: 0 },
                FramebufferAttachment { texture_view: depth_view, layer: 0, mip_level: 0 },
            ],
            width,
            height,
            layers: 1,
        })
    }
    ///
    /// The image belongs to the swapchain, so the framebuffer is only valid for
    /// as long as that chain does. Rebuilt per frame is correct but wasteful;
    /// once several frames are in flight it should be cached per image index.
    pub fn swapchain_image_framebuffer(
        &self,
        render_pass: &RenderPass,
        image: u64,
        image_view: u64,
        format: Format,
        width: u32,
        height: u32,
    ) -> RhiResult<Framebuffer> {
        let (_texture, view) = Texture::adopt_backend_image(width, height, format, image, image_view);
        self.create_gpu_framebuffer(&FramebufferDesc {
            render_pass: render_pass.clone(),
            attachments: vec![FramebufferAttachment { texture_view: view, layer: 0, mip_level: 0 }],
            width,
            height,
            layers: 1,
        })
    }

    /// Draw into a framebuffer with a single pipeline and submit the commands.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_framebuffer(
        &self,
        framebuffer: &Framebuffer,
        pipeline: &GraphicsPipeline,
        sets: &[&crate::descriptor::DescriptorSet],
        vertex_buffer: Option<(&Buffer, u32)>,
        vertex_count: u32,
        first_vertex: u32,
        clear_values: &[ClearValue],
    ) -> RhiResult<()> {
        if let Some(backend) = &self.backend_resource {
            return backend.draw_framebuffer(
                framebuffer,
                pipeline,
                sets,
                vertex_buffer,
                vertex_count,
                first_vertex,
                clear_values,
            );
        }
        Err(crate::error::RhiError::BackendError(
            "active backend cannot draw into framebuffers".into(),
        ))
    }

    /// Draw into a framebuffer with a single pipeline and an optional index
    /// buffer and submit the commands.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_framebuffer_indexed(
        &self,
        framebuffer: &Framebuffer,
        pipeline: &GraphicsPipeline,
        sets: &[&crate::descriptor::DescriptorSet],
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
        if let Some(backend) = &self.backend_resource {
            return backend.draw_framebuffer_indexed(
                framebuffer,
                pipeline,
                sets,
                vertex_buffer,
                index_buffer,
                index_type,
                vertex_count,
                first_vertex,
                index_count,
                first_index,
                vertex_offset,
                clear_values,
            );
        }
        Err(crate::error::RhiError::BackendError(
            "active backend cannot draw into framebuffers".into(),
        ))
    }

    /// Read a texture mip/array slice back to the host on the active backend.
    pub fn download_texture(
        &self,
        texture: &Texture,
        mip_level: u32,
        array_layer: u32,
    ) -> RhiResult<Vec<u8>> {
        if let Some(backend) = &self.backend_resource {
            return backend.download_texture(texture, mip_level, array_layer);
        }
        Err(crate::error::RhiError::BackendError(
            "active backend cannot download textures".into(),
        ))
    }
}
