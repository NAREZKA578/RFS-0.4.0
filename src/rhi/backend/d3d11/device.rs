//! Direct3D 11 Device and Backend Resources
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use crate::backend::common::BackendResource;
use crate::command::pass::render::{Framebuffer, FramebufferDesc, RenderPass, RenderPassDesc};
use crate::core::{DeviceDesc, MemoryProperties, PhysicalDevice};
use crate::descriptor::{DescriptorSet, DescriptorSetLayout, DescriptorSetLayoutDesc, DescriptorWrite};
use crate::error::*;
use crate::pipeline::compute::{ComputePipeline, ComputePipelineDesc};
use crate::pipeline::graphics::{GraphicsPipeline, GraphicsPipelineDesc};
use crate::resource::{Buffer, BufferDesc, GpuResource, Sampler, SamplerDesc, Texture, TextureDesc, TextureView, TextureViewDesc};
use crate::shader::module::{ShaderModule, ShaderModuleDesc};
use crate::types::{ClearValue, GraphicsApi, IndexType};
use crate::{SampleCount, TextureLayout};

/// D3D11 Device resource implementation
pub struct D3D11DeviceResource {
    // Mock device handle (would be ID3D11Device* in real implementation)
    device: u64,
    // Mock memory allocator
    allocator: D3D11MemoryAllocator,
    // Queue family index
    queue_family_index: u32,
    // Mock queue handle (would be ID3D11DeviceContext* in real implementation)
    context: u64,
    // Physical device properties
    physical_device: PhysicalDevice,
}

impl D3D11DeviceResource {
    pub fn new(
        physical: &PhysicalDevice,
        desc: &DeviceDesc,
        memory_properties: &MemoryProperties,
    ) -> RhiResult<Self> {
        // In a real implementation, this would:
        // 1. Create ID3D11Device from the adapter
        // 2. Get immediate context
        // 3. Initialize memory allocator
        
        let device = 1000u64;
        let context = 1001u64;
        let allocator = D3D11MemoryAllocator::new(memory_properties.clone());
        
        Ok(Self {
            device,
            allocator,
            queue_family_index: desc.queue_family_indices.first().copied().unwrap_or(0),
            context,
            physical_device: physical.clone(),
        })
    }
    
    /// Create a staging buffer for upload operations
    fn create_staging_buffer(&self, size: u64) -> RhiResult<(u64, u64, *mut u8)> {
        // For mock purposes, we allocate host memory directly
        let buffer_handle = 2000u64;
        let memory_handle = 2001u64;
        let ptr = unsafe { std::alloc::alloc(std::alloc::Layout::from_size_align(size as usize, 256).unwrap()) };
        
        unsafe {
            std::ptr::write_bytes(ptr, 0, size as usize);
        }
        
        Ok((buffer_handle, memory_handle, ptr))
    }
    
    /// Destroy a staging buffer
    fn destroy_staging_buffer(&self, _buffer: u64, _memory: u64, ptr: *mut u8, size: u64) {
        unsafe {
            std::alloc::dealloc(ptr, std::alloc::Layout::from_size_align(size as usize, 256).unwrap());
        }
    }
    
    /// Get bytes per texel for a format
    fn bytes_per_texel(format: crate::types::Format) -> Option<u8> {
        use crate::types::Format;
        match format {
            Format::R8_UNORM | Format::R8_SNORM | Format::R8_UINT | Format::R8_SINT => Some(1),
            Format::RG8_UNORM | Format::RG8_SNORM | Format::RG8_UINT | Format::RG8_SINT => Some(2),
            Format::RGBA8_UNORM | Format::RGBA8_SNORM | 
            Format::RGBA8_UINT | Format::RGBA8_SINT |
            Format::B8G8R8A8_UNORM | Format::B8G8R8A8_SRGB => Some(4),
            Format::R16_UNORM | Format::R16_SNORM | Format::R16_UINT | Format::R16_SINT | Format::R16_SFLOAT => Some(2),
            Format::RG16_UNORM | Format::RG16_SNORM | Format::RG16_UINT | Format::RG16_SINT | Format::RG16_SFLOAT => Some(4),
            Format::RGBA16_UNORM | Format::RGBA16_SFLOAT => Some(8),
            Format::R32_UINT | Format::R32_SINT | Format::R32_SFLOAT => Some(4),
            Format::R32G32_UINT | Format::R32G32_SINT | Format::R32G32_SFLOAT => Some(8),
            Format::R32G32B32_UINT | Format::R32G32B32_SINT | Format::R32G32B32_SFLOAT => Some(12),
            Format::RGBA32_UINT | Format::RGBA32_SINT | Format::RGBA32_SFLOAT => Some(16),
            Format::D16_UNORM | Format::D24_UNORM => Some(2),
            Format::D32_SFLOAT | Format::D24_UNORM_S8_UINT | Format::D32_SFLOAT_S8_UINT => Some(4),
            _ => None,
        }
    }
}

impl BackendResource for D3D11DeviceResource {
    fn as_raw(&self) -> *const () {
        self.device as *const ()
    }
    
    fn backend_type(&self) -> GraphicsApi {
        GraphicsApi::Direct3D11
    }
    
    fn wait_idle(&self) -> RhiResult<()> {
        // In a real implementation, this would flush the context
        Ok(())
    }
    
    fn create_buffer(&self, desc: &BufferDesc) -> RhiResult<Buffer> {
        if desc.sharing_mode == crate::types::SharingMode::Concurrent {
            return Err(RhiError::NotSupported(
                "concurrent buffer sharing is not supported for buffer creation".into(),
            ));
        }
        
        let mut buffer = Buffer::from_desc(desc.clone());
        
        // Allocate memory
        let (handle, memory, mapped) = if desc.is_host_mappable() {
            let size = desc.size;
            let (buf, mem, ptr) = self.create_staging_buffer(size)?;
            (buf, mem, ptr as usize)
        } else {
            // Device-local buffer
            let handle = 3000u64;
            let memory = 3001u64;
            (handle, memory, 0)
        };
        
        buffer.set_backend(GpuResource {
            handle,
            memory,
            mapped,
        });
        
        // Set device address if requested
        if desc.usage.contains(crate::types::BufferUsage::SHADER_DEVICE_ADDRESS) {
            buffer.set_device_address(handle + 0x1000);
        }
        
        Ok(buffer)
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
            // Host-visible buffer: copy directly
            unsafe {
                std::ptr::copy_nonoverlapping(data.as_ptr(), gpu.mapped as *mut u8, data.len());
            }
            return Ok(());
        }
        
        // Device-local buffer: upload through staging
        if !buffer.supports_transfer() {
            return Err(RhiError::BackendError(
                "buffer memory is not host mapped and the buffer lacks transfer usage".into(),
            ));
        }
        
        let (staging, staging_memory, ptr) = self.create_staging_buffer(data.len() as u64)?;
        unsafe {
            std::ptr::copy_nonoverlapping(data.as_ptr(), ptr, data.len());
        }
        
        self.destroy_staging_buffer(staging, staging_memory, ptr, data.len() as u64);
        Ok(())
    }
    
    fn create_texture(&self, desc: &TextureDesc) -> RhiResult<Texture> {
        let mut texture = Texture::from_desc(desc.clone());
        let handle = 4000u64;
        let memory = 4001u64;
        
        texture.set_backend(GpuResource {
            handle,
            memory,
            mapped: 0,
        });
        
        Ok(texture)
    }
    
    fn create_texture_view(&self, _texture: &Texture, _desc: &TextureViewDesc) -> RhiResult<TextureView> {
        let mut view = TextureView::from_parts(_texture.clone(), _desc.clone());
        view.set_backend(GpuResource {
            handle: 5000u64,
            memory: 0,
            mapped: 0,
        });
        
        Ok(view)
    }
    
    fn create_sampler(&self, desc: &SamplerDesc) -> RhiResult<Sampler> {
        let mut sampler = Sampler::from_desc(desc.clone());
        sampler.set_backend(GpuResource {
            handle: 6000u64,
            memory: 0,
            mapped: 0,
        });
        
        Ok(sampler)
    }
    
    fn upload_texture(&self, texture: &Texture, data: &[u8]) -> RhiResult<()> {
        let Some(_gpu) = texture.backend() else {
            return Err(RhiError::BackendError("texture has no GPU backing".into()));
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
        
        let total = (texture.width() as u64) * (texture.height() as u64) * (texture.depth() as u64) * (bpp as u64);
        if total > data.len() as u64 {
            return Err(RhiError::BackendError(format!(
                "texture upload data too small: {} bytes, {} needed",
                data.len(),
                total
            )));
        }
        
        Err(RhiError::NotSupported("D3D11 texture upload not yet fully implemented".into()))
    }
    
    fn create_shader_module(&self, desc: &ShaderModuleDesc) -> RhiResult<ShaderModule> {
        // D3D11 uses GLSL (via ANGLE) or SpirV (via DirectXShaderCompiler)
        // For mock purposes, we accept any format
        let mut module = ShaderModule::new(desc.clone());
        module.set_backend(GpuResource {
            handle: 7000u64,
            memory: 0,
            mapped: 0,
        });
        
        Ok(module)
    }
    
    fn create_descriptor_set_layout(&self, desc: &DescriptorSetLayoutDesc) -> RhiResult<DescriptorSetLayout> {
        let mut layout = DescriptorSetLayout::new(desc.clone());
        layout.set_backend(GpuResource {
            handle: 8000u64,
            memory: 0,
            mapped: 0,
        });
        
        Ok(layout)
    }
    
    fn create_descriptor_set(&self, layout: &DescriptorSetLayout) -> RhiResult<DescriptorSet> {
        let Some(layout_backend) = layout.backend() else {
            return Err(RhiError::BackendError("descriptor set layout has no GPU backing".into()));
        };
        
        let mut set = DescriptorSet::new(layout.clone());
        set.set_backend(GpuResource {
            handle: 9000u64,
            memory: layout_backend.handle + 0x1000,
            mapped: 0,
        });
        
        Ok(set)
    }
    
    fn write_descriptors(&self, set: &DescriptorSet, writes: &[DescriptorWrite]) -> RhiResult<()> {
        let _ = set;
        
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
            
            if write.descriptors.len() as u32 > binding.count {
                return Err(RhiError::BackendError(format!(
                    "writing {} descriptors to binding {} which only accepts {}",
                    write.descriptors.len(),
                    write.dst_binding,
                    binding.count
                )));
            }
        }
        
        Ok(())
    }
    
    fn create_compute_pipeline(
        &self,
        desc: &ComputePipelineDesc,
        layout: &DescriptorSetLayout,
    ) -> RhiResult<ComputePipeline> {
        let Some(_shader_backend) = desc.shader_module().backend() else {
            return Err(RhiError::BackendError(
                "compute shader module has no GPU backing".into(),
            ));
        };
        
        let Some(layout_backend) = layout.backend() else {
            return Err(RhiError::BackendError(
                "descriptor set layout has no GPU backing".into(),
            ));
        };
        
        let mut pipeline = ComputePipeline::new(desc.clone());
        pipeline.set_backend(GpuResource {
            handle: 10000u64,
            memory: layout_backend.handle,
            mapped: 0,
        });
        
        Ok(pipeline)
    }
    
    fn dispatch_compute(
        &self,
        _pipeline: &ComputePipeline,
        _sets: &[&DescriptorSet],
        _group_count_x: u32,
        _group_count_y: u32,
        _group_count_z: u32,
    ) -> RhiResult<()> {
        Err(RhiError::NotSupported("D3D11 compute dispatch not yet fully implemented".into()))
    }
    
    fn invalidate_mapped_buffer(&self, _buffer: &Buffer) -> RhiResult<()> {
        Ok(())
    }
    
    fn download_buffer(&self, _buffer: &Buffer, _offset: u64, _size: u64) -> RhiResult<Vec<u8>> {
        Err(RhiError::NotSupported("D3D11 buffer download not yet fully implemented".into()))
    }
    
    fn download_texture(
        &self,
        _texture: &Texture,
        _mip_level: u32,
        _array_layer: u32,
    ) -> RhiResult<Vec<u8>> {
        Err(RhiError::NotSupported("D3D11 texture download not yet fully implemented".into()))
    }
    
    fn create_render_pass(&self, desc: &RenderPassDesc) -> RhiResult<RenderPass> {
        let mut render_pass = RenderPass::new(desc.clone());
        render_pass.set_backend(GpuResource {
            handle: 11000u64,
            memory: 0,
            mapped: 0,
        });
        
        Ok(render_pass)
    }
    
    fn create_framebuffer(&self, desc: &FramebufferDesc) -> RhiResult<Framebuffer> {
        let mut framebuffer = Framebuffer::new(desc.clone());
        framebuffer.set_backend(GpuResource {
            handle: 12000u64,
            memory: 0,
            mapped: 0,
        });
        
        Ok(framebuffer)
    }
    
    fn create_graphics_pipeline(
        &self,
        desc: &GraphicsPipelineDesc,
        _render_pass: &RenderPass,
        _set_layouts: &[&DescriptorSetLayout],
    ) -> RhiResult<GraphicsPipeline> {
        let mut pipeline = GraphicsPipeline::new(desc.clone());
        pipeline.set_backend(GpuResource {
            handle: 13000u64,
            memory: 0,
            mapped: 0,
        });
        
        Ok(pipeline)
    }
    
    fn submit_commands(&self, _commands: &[crate::command::commands::Command]) -> RhiResult<()> {
        Err(RhiError::NotSupported("D3D11 command submission not yet fully implemented".into()))
    }
    
    fn create_window_surface(&self, _hinstance: u64, _hwnd: u64) -> RhiResult<crate::backend::vulkan::WindowSurface> {
        Err(RhiError::NotSupported("D3D11 uses DXGI swap chains, not Vulkan surfaces".into()))
    }
    
    fn create_swapchain(
        &self,
        _surface: &crate::backend::vulkan::WindowSurface,
        _request: crate::backend::vulkan::SwapchainRequest,
        _old: Option<&mut crate::backend::vulkan::Swapchain>,
    ) -> RhiResult<crate::backend::vulkan::Swapchain> {
        Err(RhiError::NotSupported("D3D11 uses DXGI swap chains".into()))
    }
    
    fn presentation_render_pass_desc(&self, format: crate::types::Format) -> RhiResult<RenderPassDesc> {
        Ok(RenderPassDesc {
            attachments: vec![crate::command::pass::render::AttachmentDescription {
                format,
                samples: SampleCount::X1,
                load_op: crate::command::pass::render::LoadOp::Clear,
                store_op: crate::command::pass::render::StoreOp::Store,
                stencil_load_op: crate::command::pass::render::LoadOp::DontCare,
                stencil_store_op: crate::command::pass::render::StoreOp::DontCare,
                initial_layout: TextureLayout::Undefined,
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
    ) -> RhiResult<RenderPassDesc> {
        let mut desc = self.presentation_render_pass_desc(format)?;
        desc.attachments.push(crate::command::pass::render::AttachmentDescription {
            format: depth_format,
            samples: SampleCount::X1,
            load_op: crate::command::pass::render::LoadOp::Clear,
            store_op: crate::command::pass::render::StoreOp::Store,
            stencil_load_op: crate::command::pass::render::LoadOp::DontCare,
            stencil_store_op: crate::command::pass::render::StoreOp::DontCare,
            initial_layout: TextureLayout::Undefined,
            final_layout: TextureLayout::DepthStencilAttachmentOptimal,
        });
        desc.subpasses[0].depth_stencil_attachment = Some(crate::command::pass::render::AttachmentReference {
            attachment: 1,
            layout: TextureLayout::DepthStencilAttachmentOptimal,
        });
        Ok(desc)
    }
    
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
        Err(RhiError::NotSupported("D3D11 draw_framebuffer_indexed not yet fully implemented".into()))
    }
}

/// D3D11 Memory Allocator (mock implementation)
pub struct D3D11MemoryAllocator {
    memory_properties: MemoryProperties,
}

impl D3D11MemoryAllocator {
    pub fn new(memory_properties: MemoryProperties) -> Self {
        Self { memory_properties }
    }
    
    pub fn find_memory_type(&self, _allowed_bits: u32, required: crate::types::MemoryPropertyFlags) -> Option<u32> {
        for (index, mem_type) in self.memory_properties.memory_types.iter().enumerate() {
            if mem_type.flags.contains(required) {
                return Some(index as u32);
            }
        }
        None
    }
    
    pub fn allocate(&self, _size: u64, _allowed_bits: u32, required: crate::types::MemoryPropertyFlags) -> RhiResult<(u64, u32)> {
        let index = self.find_memory_type(0, required).ok_or_else(|| {
            RhiError::BackendError(
                "no memory type satisfies the requested properties on this device".into(),
            )
        })?;
        
        Ok((14000u64, index))
    }
}


