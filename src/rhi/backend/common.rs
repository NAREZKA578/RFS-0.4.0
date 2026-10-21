//! Common Backend Utilities
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use crate::descriptor::{DescriptorSet, DescriptorSetLayout, DescriptorSetLayoutDesc, DescriptorWrite};
use crate::error::RhiResult;
use crate::command::pass::render::{Framebuffer, FramebufferDesc, RenderPass, RenderPassDesc};
use crate::pipeline::compute::{ComputePipeline, ComputePipelineDesc};
use crate::pipeline::graphics::{GraphicsPipeline, GraphicsPipelineDesc};
use crate::resource::{Buffer, BufferDesc, Sampler, SamplerDesc, Texture, TextureDesc, TextureView, TextureViewDesc};
use crate::shader::module::{ShaderModule, ShaderModuleDesc};
use crate::types::{ClearValue, GraphicsApi, IndexType};

/// What kind of backend object is being released early. Bug №294.
///
/// Backend-neutral on purpose. The caller says "the framebuffer"; the backend
/// decides what that means for its own bookkeeping. Passing a raw discriminant
/// instead would tie this trait to one backend's internal enum ordering — a backend
/// that renumbered its variants would then release the wrong object kind, and the
/// compiler would not object.
///
/// Every variant must exist in every backend that tracks objects. A backend that
/// tracks nothing has no mapping for them, which is why
/// [`BackendResource::release`] is defaulted to a no-op.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleaseKind {
    Image,
    ImageView,
    Framebuffer,
    Buffer,
    Sampler,
    ShaderModule,
    DescriptorSetLayout,
    DescriptorPool,
    PipelineLayout,
    RenderPass,
    GraphicsPipeline,
    ComputePipeline,
}

/// Trait for backend resources
pub trait BackendResource: Send + Sync {
    fn as_raw(&self) -> *const ();
    fn backend_type(&self) -> GraphicsApi;
    fn wait_idle(&self) -> RhiResult<()> {
        Ok(())
    }

    /// Create a buffer on the backend. The default implementation returns a
    /// CPU-side buffer with no native handle.
    fn create_buffer(&self, desc: &BufferDesc) -> RhiResult<Buffer> {
        Ok(Buffer::from_desc(desc.clone()))
    }

    /// Create a texture on the backend. The default implementation returns a
    /// CPU-side texture with no native handle.
    fn create_texture(&self, desc: &TextureDesc) -> RhiResult<Texture> {
        Ok(Texture::from_desc(desc.clone()))
    }

    /// Create a texture view on the backend. The default implementation
    /// returns a CPU-side view with no native handle.
    fn create_texture_view(&self, texture: &Texture, desc: &TextureViewDesc) -> RhiResult<TextureView> {
        Ok(TextureView::from_parts(texture.clone(), desc.clone()))
    }

    /// Create a sampler on the backend. The default implementation returns a
    /// CPU-side sampler with no native handle.
    fn create_sampler(&self, desc: &SamplerDesc) -> RhiResult<Sampler> {
        Ok(Sampler::from_desc(desc.clone()))
    }

    /// Upload raw bytes into a host-visible buffer.
    fn upload_buffer(&self, _buffer: &Buffer, _data: &[u8]) -> RhiResult<()> {
        Err(crate::error::RhiError::NotSupported(
            "buffer uploads require a hardware backend".into(),
        ))
    }

    /// Upload raw bytes into a texture (through a staging transfer when the
    /// backend requires it).
    fn upload_texture(&self, _texture: &Texture, _data: &[u8]) -> RhiResult<()> {
        Err(crate::error::RhiError::NotSupported(
            "texture uploads require a hardware backend".into(),
        ))
    }

    /// Compile a shader module on the backend. The default implementation
    /// returns the module without a native handle.
    fn create_shader_module(&self, desc: &ShaderModuleDesc) -> RhiResult<ShaderModule> {
        Ok(ShaderModule::new(desc.clone()))
    }

    /// Create a descriptor set layout on the backend. The default
    /// implementation returns a CPU-side layout.
    fn create_descriptor_set_layout(
        &self,
        desc: &DescriptorSetLayoutDesc,
    ) -> RhiResult<DescriptorSetLayout> {
        Ok(DescriptorSetLayout::new(desc.clone()))
    }

    /// Allocate a descriptor set on the backend.
    fn create_descriptor_set(&self, _layout: &DescriptorSetLayout) -> RhiResult<DescriptorSet> {
        Err(crate::error::RhiError::NotSupported(
            "descriptor sets require a hardware backend".into(),
        ))
    }

    /// Write descriptor bindings into a set.
    fn write_descriptors(&self, _set: &DescriptorSet, _writes: &[DescriptorWrite]) -> RhiResult<()> {
        Err(crate::error::RhiError::NotSupported(
            "descriptor writes require a hardware backend".into(),
        ))
    }

    /// Create a compute pipeline on the backend. The descriptor set layout
    /// defines the pipeline layout used at set 0.
    fn create_compute_pipeline(
        &self,
        _desc: &ComputePipelineDesc,
        _layout: &DescriptorSetLayout,
    ) -> RhiResult<ComputePipeline> {
        Err(crate::error::RhiError::NotSupported(
            "compute pipelines require a hardware backend".into(),
        ))
    }

    /// Dispatch a compute pipeline with the given descriptor sets bound.
    fn dispatch_compute(
        &self,
        _pipeline: &ComputePipeline,
        _sets: &[&DescriptorSet],
        _group_count_x: u32,
        _group_count_y: u32,
        _group_count_z: u32,
    ) -> RhiResult<()> {
        Err(crate::error::RhiError::NotSupported(
            "compute dispatch requires a hardware backend".into(),
        ))
    }

    /// Make device writes to a persistently mapped buffer visible to the host.
    ///
    /// Called by [`Device::buffer_data`](crate::core::Device::buffer_data) before
    /// it hands out a host slice over mapped memory. Backends whose host-visible
    /// memory may be non-coherent must override this; without it the caller can
    /// read the previous contents of the range. Backends with coherent-only host
    /// memory can rely on the default, which is why it succeeds silently.
    fn invalidate_mapped_buffer(&self, _buffer: &Buffer) -> RhiResult<()> {
        Ok(())
    }

    /// Read a range of a buffer back to the host.
    fn download_buffer(&self, _buffer: &Buffer, _offset: u64, _size: u64) -> RhiResult<Vec<u8>> {
        Err(crate::error::RhiError::NotSupported(
            "buffer downloads require a hardware backend".into(),
        ))
    }

    /// Read a texture mip/array slice back to the host as tightly packed,
    /// 4-byte aligned rows.
    fn download_texture(
        &self,
        _texture: &Texture,
        _mip_level: u32,
        _array_layer: u32,
    ) -> RhiResult<Vec<u8>> {
        Err(crate::error::RhiError::NotSupported(
            "texture downloads require a hardware backend".into(),
        ))
    }

    /// Create a render pass on the backend.
    fn create_render_pass(&self, _desc: &RenderPassDesc) -> RhiResult<RenderPass> {
        Err(crate::error::RhiError::NotSupported(
            "render passes require a hardware backend".into(),
        ))
    }

    /// Create a framebuffer on the backend.
    fn create_framebuffer(&self, _desc: &FramebufferDesc) -> RhiResult<Framebuffer> {
        Err(crate::error::RhiError::NotSupported(
            "framebuffers require a hardware backend".into(),
        ))
    }

    /// Create a graphics pipeline on the backend using the given render pass
    /// and descriptor set layouts for the pipeline layout.
    fn create_graphics_pipeline(
        &self,
        _desc: &GraphicsPipelineDesc,
        _render_pass: &RenderPass,
        _set_layouts: &[&DescriptorSetLayout],
    ) -> RhiResult<GraphicsPipeline> {
        Err(crate::error::RhiError::NotSupported(
            "graphics pipelines require a hardware backend".into(),
        ))
    }

    /// Create a window surface from raw Win32 handles.
    ///
    /// Takes `u64` rather than the typed Vulkan handles so the trait stays
    /// backend-agnostic and the crate-internal instance type does not leak into
    /// its signature.
    fn create_window_surface(
        &self,
        _hinstance: u64,
        _hwnd: u64,
    ) -> RhiResult<super::vulkan::WindowSurface> {
        Err(crate::error::RhiError::NotSupported(
            "this backend cannot present to a window".into(),
        ))
    }

    /// Create a swapchain on an already-created surface.
    ///
    /// `old` is the chain being replaced, if any. It must be handed to Vulkan
    /// rather than dropped first, or creating the replacement fails with
    /// `ERROR_NATIVE_WINDOW_IN_USE_KHR`.
    fn create_swapchain(
        &self,
        _surface: &super::vulkan::WindowSurface,
        _request: super::vulkan::SwapchainRequest,
        _old: Option<&mut super::vulkan::Swapchain>,
    ) -> RhiResult<super::vulkan::Swapchain> {
        Err(crate::error::RhiError::NotSupported(
            "this backend cannot create a swapchain".into(),
        ))
    }

    /// Build a render pass compatible with a presentation surface format.
    fn presentation_render_pass_desc(
        &self,
        _format: crate::types::Format,
    ) -> RhiResult<crate::command::pass::render::RenderPassDesc> {
        Err(crate::error::RhiError::NotSupported(
            "this backend cannot describe a presentation render pass".into(),
        ))
    }

    /// As `presentation_render_pass_desc`, plus a depth attachment.
    ///
    /// Split from the colour-only version rather than a parameter on it: a
    /// depth attachment changes the subpass layout, so a caller that has no
    /// depth to offer must not be able to pass one by accident, and the
    /// colour-only signature is already used by working code.
    fn presentation_render_pass_desc_with_depth(
        &self,
        _format: crate::types::Format,
        _depth_format: crate::types::Format,
    ) -> RhiResult<crate::command::pass::render::RenderPassDesc> {
        Err(crate::error::RhiError::NotSupported(
            "this backend cannot describe a presentation render pass with depth".into(),
        ))
    }

    /// Replay a recorded command list on the GPU.
    ///
    /// Bug №203/№248: this is the seam that makes the RHI command path real.
    /// `CommandBuffer` owns a `Vec<Command>` and this is what turns it into
    /// actual API calls, so the immediate helpers below and the explicit
    /// `CommandEncoder` path share one implementation instead of two.
    ///
    /// Defaults to unsupported: only a backend that has a translator can
    /// honour it, and silently ignoring the list would be worse than failing.
    fn submit_commands(&self, _commands: &[crate::command::commands::Command]) -> RhiResult<()> {
        Err(crate::error::RhiError::NotSupported(
            "this backend cannot replay a command list".into(),
        ))
    }

    /// Tell the backend the next submit is the one being presented.
    ///
    /// Bug №282: `vkQueuePresentKHR` waits on a semaphore that the frame's submit
    /// signals. Defaulting to "nothing to arm" is the safe answer for a backend
    /// with no presentation path — there is nothing to present to.
    fn arm_presentation_signal(&self) {}

    /// The semaphore presentation waits on, or null when there is none.
    ///
    /// Bug №282. Null here means `present` must not wait, which is what a backend
    /// without presentation wants; a Vulkan backend returns a real semaphore.
fn render_finished_semaphore(&self) -> crate::ash::vk::Semaphore {
             crate::ash::vk::Semaphore::null()
         }

    /// The swapchain image the frame being recorded will be presented into.
    ///
    /// The present waits on the semaphore of *that* image, so the backend has to
    /// know it. A backend with no presentation path has no images and ignores
    /// this.
    fn set_present_image(&self, _image_index: u32) {}

    /// Tell the backend how many swapchain images there are.
    ///
    /// A backend that keeps one present semaphore per image needs a semaphore
    /// for each of them. The count is known only after the chain is built, and
    /// a rebuilt chain may have a different one, so the backend is told rather
    /// than guessing.
    fn ensure_presentation_images(&self, _count: usize) {}

         /// The semaphore `vkAcquireNextImageKHR` should signal for the next frame.
         ///
         /// Bug №282: `acquire` used to block the CPU on a fence instead, which is
         /// precisely why the renderer could never have more than one frame in
         /// flight. Handing the semaphore to the acquire lets the submit wait for
         /// the handover on the GPU timeline rather than stalling the thread.
         fn image_acquired_semaphore(&self) -> crate::ash::vk::Semaphore {
             crate::ash::vk::Semaphore::null()
         }

          /// Give back a presentation frame that will not be submitted.
          ///
          /// Bug №282: `vkAcquireNextImageKHR` was given a semaphore to signal, and
          /// if no submit follows, nothing ever waits on it. The semaphore stays
          /// signalled forever, and the next `acquire` of that slot is then
          /// VUID-vkAcquireNextImageKHR-semaphore-01779 — "must not have any pending
          /// operations" — which is what happened on every frame that recorded no
          /// commands.
          ///
          /// A backend with per-frame semaphores drains the signal here. One without
          /// them has nothing pending and needs nothing.
          fn cancel_presentation_frame(&self) {}

    /// The layout the backend believes the image behind `image` is in. Bug #644.
    ///
    /// `None` means the backend does not track that image, which is the honest
    /// answer for an image it did not create — a swapchain image, whose layout
    /// belongs to whoever owns it. Guessing there is what made the readback
    /// wrong in the first place: a barrier that claims `UNDEFINED` for an image
    /// whose contents are still wanted declares them discarded.
    ///
    /// The default is `None`, which is the right answer for a backend with no
    /// layout tracking.
    fn image_layout(&self, _image: u64) -> Option<crate::resource::TextureLayout> {
        None
    }


    /// Release a texture early instead of at device teardown. Bug №289.
    ///
    /// Default is a no-op, which is the honest answer for a backend with no
    /// tracking: nothing to untrack means nothing to leak.
    fn release_texture(&self, _texture: &crate::resource::Texture) {}

    /// Release a framebuffer early instead of at device teardown. Bug №289.
    fn release_framebuffer(&self, _framebuffer: &crate::command::pass::render::Framebuffer) {}

    /// Release a texture view early instead of at device teardown. Bug №289.
    ///
    /// Needed in its own right, not as a convenience: a view outlives nothing, but
    /// it is destroyed only when its image is, and `release_texture` frees the
    /// image. Releasing the image underneath a live view leaves the view pointing at
    /// freed memory, which is the failure mode bug №289's first attempt produced.
    fn release_texture_view(&self, _view: &crate::resource::TextureView) {}

    /// Destroy one tracked object now rather than at device teardown. Bug №289.
    ///
    /// Provided rather than required because most backends track nothing and have
    /// nothing to release.
    fn release(&self, _kind: ReleaseKind, _object: u64) {}

    /// Release several objects with a single wait. Bug №294.
    ///
    /// [`release`](Self::release) waits for the device to go idle before destroying,
    /// which it must: Vulkan forbids destroying an object the GPU may still be
    /// reading. Doing that per object is correct and slow — a resize replaces three
    /// objects at once, and three consecutive waits stall the GPU three times for
    /// work that only needed to be finished once. On this machine that showed up as
    /// visible stutter on every resize step.
    ///
    /// So the wait belongs to the batch, not to the object. Same guarantee, one
    /// stall.
    ///
    /// Order is the caller's responsibility and is not checked: the backend destroys
    /// in the order given, and Vulkan requires dependents before dependencies. A
    /// wrong order here is a use-after-free, so it is left visible rather than
    /// silently sorted — sorting would require the backend to model the dependency
    /// graph, which is exactly the knowledge it does not have.
    fn release_many(&self, _objects: &[(ReleaseKind, u64)]) {}

    /// How many backend objects are still waiting to be destroyed.
    ///
    /// Diagnostics, added with bug №289. Leaks used to be hunted for by reading the
    /// validation layer's report at `vkDestroyDevice`, which tells you *that*
    /// something leaked but not *which object* or whether the count grew while the
    /// program ran. That distinction is the whole point for objects owned by a cache
    /// that is replaced on resize: the symptom is unbounded growth over time, and it
    /// is invisible in a single-frame report.
    ///
    /// Zero for a backend that tracks nothing, so callers need no per-backend
    /// branching and no `Option`.
    fn tracked_object_count(&self) -> usize {
        0
    }

    /// Which backend objects are outstanding, broken down by kind.
    ///
    /// Bug №289. A bare count localises nothing: "+2" is equally consistent with two
    /// images, two views, or a pair that grows every frame. See
    /// [`tracked_object_count`] for the underlying reason this exists.
    fn tracked_object_summary(&self) -> String {
        String::new()
    }

    /// Draw into a framebuffer with a single pipeline, an optional index buffer
    /// and submit the commands.
    #[allow(clippy::too_many_arguments)]
    fn draw_framebuffer_indexed(
        &self,
        _framebuffer: &Framebuffer,
        _pipeline: &GraphicsPipeline,
        _sets: &[&DescriptorSet],
        _vertex_buffer: Option<(&Buffer, u32)>,
        _index_buffer: Option<(&Buffer, u32)>,
        _index_type: IndexType,
        _vertex_count: u32,
        _first_vertex: u32,
        _index_count: u32,
        _first_index: u32,
        _vertex_offset: i32,
        _clear_values: &[ClearValue],
    ) -> RhiResult<()> {
        Err(crate::error::RhiError::NotSupported(
            "indexed framebuffer draws require a hardware backend".into(),
        ))
    }

    /// Draw into a framebuffer with a single pipeline and submit the commands.
    #[allow(clippy::too_many_arguments)]
    fn draw_framebuffer(
        &self,
        framebuffer: &Framebuffer,
        pipeline: &GraphicsPipeline,
        sets: &[&DescriptorSet],
        vertex_buffer: Option<(&Buffer, u32)>,
        vertex_count: u32,
        first_vertex: u32,
        clear_values: &[ClearValue],
    ) -> RhiResult<()> {
        self.draw_framebuffer_indexed(
            framebuffer,
            pipeline,
            sets,
            vertex_buffer,
            None,
            IndexType::U32,
            vertex_count,
            first_vertex,
            0,
            0,
            0,
            clear_values,
        )
    }
}