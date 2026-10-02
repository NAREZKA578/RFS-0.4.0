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