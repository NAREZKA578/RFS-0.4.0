//! Vulkan Pipeline implementation.
//!
//! Pipelines are built against dynamic rendering (core in Vulkan 1.3), so no
//! `VkRenderPass` is required and attachments can change per frame. Vertex
//! input is derived from the RHI `VertexLayout`, color blend from `BlendMode`,
//! and the pipeline layout contains a texture descriptor set (binding 0) plus a
//! 128-byte push-constant block used for named uniforms.

use crate::rhi::command::PrimitiveTopology;
use crate::rhi::pipeline::{
    BlendMode, CompareFunc, CullMode, FrontFace, Pipeline as PipelineTrait, PipelineDescriptor,
    PolygonMode,
};
use crate::rhi::types::VertexFormat;
use ash::vk;
use ash::vk::Handle;
use std::sync::Arc;

use super::command::{PUSH_CONSTANT_SIZE, SSBO_BASE_BINDING, UBO_BASE_BINDING};
use super::registry;

/// Maps an RHI vertex format to a `VkFormat`.
fn to_vk_vertex_format(format: VertexFormat) -> vk::Format {
    match format {
        VertexFormat::Float1 => vk::Format::R32_SFLOAT,
        VertexFormat::Float2 => vk::Format::R32G32_SFLOAT,
        VertexFormat::Float3 => vk::Format::R32G32B32_SFLOAT,
        VertexFormat::Float4 => vk::Format::R32G32B32A32_SFLOAT,
        VertexFormat::Uint2 => vk::Format::R16G16_UINT,
        VertexFormat::Uint4 => vk::Format::R8G8B8A8_UINT,
        VertexFormat::Snorm4 => vk::Format::R8G8B8A8_SNORM,
    }
}

fn to_vk_topology(topology: PrimitiveTopology) -> vk::PrimitiveTopology {
    match topology {
        PrimitiveTopology::PointList => vk::PrimitiveTopology::POINT_LIST,
        PrimitiveTopology::LineList => vk::PrimitiveTopology::LINE_LIST,
        PrimitiveTopology::LineStrip => vk::PrimitiveTopology::LINE_STRIP,
        PrimitiveTopology::TriangleList => vk::PrimitiveTopology::TRIANGLE_LIST,
        PrimitiveTopology::TriangleStrip => vk::PrimitiveTopology::TRIANGLE_STRIP,
    }
}

/// Vulkan graphics pipeline.
pub struct VulkanPipeline {
    device: Arc<ash::Device>,
    pipeline: vk::Pipeline,
    layout: vk::PipelineLayout,
    descriptor_set_layout: vk::DescriptorSetLayout,
    vertex_layout: crate::rhi::types::VertexLayout,
    blend_mode: BlendMode,
    cull_mode: CullMode,
}

impl VulkanPipeline {
    /// Creates a new Vulkan pipeline.
    pub fn new(
        device: &Arc<ash::Device>,
        descriptor: &PipelineDescriptor,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        // Descriptor set layout:
        //   bindings 0..8   : COMBINED_IMAGE_SAMPLER (textures)
        //   bindings 8..16  : UNIFORM_BUFFER (UBOs)
        //   bindings 16..24 : STORAGE_BUFFER (SSBOs)
        let bindings: Vec<vk::DescriptorSetLayoutBinding> = (0..super::command::TOTAL_BINDINGS)
            .map(|i| {
                let ty = if i < UBO_BASE_BINDING {
                    vk::DescriptorType::COMBINED_IMAGE_SAMPLER
                } else if i < SSBO_BASE_BINDING {
                    vk::DescriptorType::UNIFORM_BUFFER
                } else {
                    vk::DescriptorType::STORAGE_BUFFER
                };
                vk::DescriptorSetLayoutBinding::default()
                    .binding(i)
                    .descriptor_type(ty)
                    .descriptor_count(1)
                    .stage_flags(vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT)
            })
            .collect();
        let set_layout_info = vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings);
        let descriptor_set_layout =
            unsafe { device.create_descriptor_set_layout(&set_layout_info, None)? };

        // Push-constant range for named uniforms.
        let push_range = vk::PushConstantRange::default()
            .stage_flags(vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT)
            .offset(0)
            .size(PUSH_CONSTANT_SIZE);

        // Pipeline layout.
        let layout_create_info = vk::PipelineLayoutCreateInfo::default()
            .set_layouts(std::slice::from_ref(&descriptor_set_layout))
            .push_constant_ranges(std::slice::from_ref(&push_range));
        let layout = unsafe { device.create_pipeline_layout(&layout_create_info, None)? };

        // Shader stages — convert native handles back to vk::ShaderModule
        let vertex_module =
            unsafe { std::mem::transmute::<u64, vk::ShaderModule>(descriptor.vertex_shader) };
        let fragment_module =
            unsafe { std::mem::transmute::<u64, vk::ShaderModule>(descriptor.fragment_shader) };

        let stages = [
            vk::PipelineShaderStageCreateInfo::default()
                .stage(vk::ShaderStageFlags::VERTEX)
                .module(vertex_module)
                .name(c"main"),
            vk::PipelineShaderStageCreateInfo::default()
                .stage(vk::ShaderStageFlags::FRAGMENT)
                .module(fragment_module)
                .name(c"main"),
        ];

        // Vertex input from VertexLayout.
        let vertex_binding = vk::VertexInputBindingDescription::default()
            .binding(0)
            .stride(descriptor.vertex_layout.stride)
            .input_rate(vk::VertexInputRate::VERTEX);
        let attributes: Vec<vk::VertexInputAttributeDescription> = descriptor
            .vertex_layout
            .attributes
            .iter()
            .map(|a| {
                vk::VertexInputAttributeDescription::default()
                    .location(a.location)
                    .binding(0)
                    .format(to_vk_vertex_format(a.format))
                    .offset(a.offset)
            })
            .collect();
        let vertex_input_info = vk::PipelineVertexInputStateCreateInfo::default()
            .vertex_binding_descriptions(std::slice::from_ref(&vertex_binding))
            .vertex_attribute_descriptions(&attributes);

        // Input assembly.
        let input_assembly = vk::PipelineInputAssemblyStateCreateInfo::default()
            .topology(to_vk_topology(descriptor.topology));

        // Viewport and scissor (dynamic state).
        let viewport_state = vk::PipelineViewportStateCreateInfo::default()
            .viewport_count(1)
            .scissor_count(1);

        // Rasterizer.
        let rasterizer = vk::PipelineRasterizationStateCreateInfo::default()
            .polygon_mode(match descriptor.polygon_mode {
                PolygonMode::Fill => vk::PolygonMode::FILL,
                PolygonMode::Line => vk::PolygonMode::LINE,
                PolygonMode::Point => vk::PolygonMode::POINT,
            })
            .cull_mode(match descriptor.cull_mode {
                CullMode::None => vk::CullModeFlags::NONE,
                CullMode::Front => vk::CullModeFlags::FRONT,
                CullMode::Back => vk::CullModeFlags::BACK,
                CullMode::FrontAndBack => vk::CullModeFlags::FRONT_AND_BACK,
            })
            .front_face(match descriptor.front_face {
                FrontFace::Ccw => vk::FrontFace::COUNTER_CLOCKWISE,
                FrontFace::Cw => vk::FrontFace::CLOCKWISE,
            })
            .line_width(1.0);

        // Multisampling.
        let msaa_samples = descriptor.samples.clamp(1, 64).next_power_of_two();
        let samples = vk::SampleCountFlags::from_raw(msaa_samples.min(8));
        let multisampling =
            vk::PipelineMultisampleStateCreateInfo::default().rasterization_samples(samples);

        // Depth stencil.
        let depth_stencil = vk::PipelineDepthStencilStateCreateInfo::default()
            .depth_test_enable(descriptor.depth_state.enabled)
            .depth_write_enable(descriptor.depth_state.write_enabled)
            .depth_compare_op(match descriptor.depth_state.compare_func {
                CompareFunc::Never => vk::CompareOp::NEVER,
                CompareFunc::Less => vk::CompareOp::LESS,
                CompareFunc::Equal => vk::CompareOp::EQUAL,
                CompareFunc::LessEqual => vk::CompareOp::LESS_OR_EQUAL,
                CompareFunc::Greater => vk::CompareOp::GREATER,
                CompareFunc::NotEqual => vk::CompareOp::NOT_EQUAL,
                CompareFunc::GreaterEqual => vk::CompareOp::GREATER_OR_EQUAL,
                CompareFunc::Always => vk::CompareOp::ALWAYS,
            });

        // Color blend.
        let (blend_enable, src_color, dst_color, src_alpha, dst_alpha) = match descriptor.blend_mode
        {
            BlendMode::None => (
                false,
                vk::BlendFactor::ONE,
                vk::BlendFactor::ZERO,
                vk::BlendFactor::ONE,
                vk::BlendFactor::ZERO,
            ),
            BlendMode::Alpha => (
                true,
                vk::BlendFactor::SRC_ALPHA,
                vk::BlendFactor::ONE_MINUS_SRC_ALPHA,
                vk::BlendFactor::ONE,
                vk::BlendFactor::ONE_MINUS_SRC_ALPHA,
            ),
            BlendMode::Premultiplied => (
                true,
                vk::BlendFactor::ONE,
                vk::BlendFactor::ONE_MINUS_SRC_ALPHA,
                vk::BlendFactor::ONE,
                vk::BlendFactor::ONE_MINUS_SRC_ALPHA,
            ),
            BlendMode::Additive => (
                true,
                vk::BlendFactor::SRC_ALPHA,
                vk::BlendFactor::ONE,
                vk::BlendFactor::ONE,
                vk::BlendFactor::ONE,
            ),
        };
        let color_blend_attachment = vk::PipelineColorBlendAttachmentState::default()
            .color_write_mask(vk::ColorComponentFlags::RGBA)
            .blend_enable(blend_enable)
            .src_color_blend_factor(src_color)
            .dst_color_blend_factor(dst_color)
            .color_blend_op(vk::BlendOp::ADD)
            .src_alpha_blend_factor(src_alpha)
            .dst_alpha_blend_factor(dst_alpha)
            .alpha_blend_op(vk::BlendOp::ADD);

        let color_blend = vk::PipelineColorBlendStateCreateInfo::default()
            .logic_op_enable(false)
            .attachments(std::slice::from_ref(&color_blend_attachment));

        // Dynamic state.
        let dynamic_states = [vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];
        let dynamic_state =
            vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dynamic_states);

        // Dynamic rendering format list — use formats from descriptor if provided
        let default_color_formats = [vk::Format::B8G8R8A8_UNORM, vk::Format::R8G8B8A8_UNORM];
        let color_formats: Vec<vk::Format> = descriptor
            .color_formats
            .as_ref()
            .map(|f| {
                f.iter()
                    .map(|fmt| super::texture::to_vk_format(*fmt))
                    .collect()
            })
            .unwrap_or_else(|| default_color_formats.to_vec());
        let depth_format_vk = descriptor
            .depth_format
            .map(|f| super::texture::to_vk_format(f))
            .unwrap_or(vk::Format::D32_SFLOAT);
        let mut rendering_info = vk::PipelineRenderingCreateInfo::default()
            .color_attachment_formats(&color_formats)
            .depth_attachment_format(depth_format_vk);

        // Create pipeline.
        let pipeline_create_info = vk::GraphicsPipelineCreateInfo::default()
            .stages(&stages)
            .vertex_input_state(&vertex_input_info)
            .input_assembly_state(&input_assembly)
            .viewport_state(&viewport_state)
            .rasterization_state(&rasterizer)
            .multisample_state(&multisampling)
            .depth_stencil_state(&depth_stencil)
            .color_blend_state(&color_blend)
            .dynamic_state(&dynamic_state)
            .layout(layout)
            .push_next(&mut rendering_info);

        let pipeline = unsafe {
            let result = device.create_graphics_pipelines(
                vk::PipelineCache::null(),
                std::slice::from_ref(&pipeline_create_info),
                None,
            );
            match result {
                Ok(pipelines) => pipelines.into_iter().next().ok_or("No pipeline created")?,
                Err((pipelines, err)) => {
                    for p in pipelines {
                        device.destroy_pipeline(p, None);
                    }
                    return Err(Box::new(err));
                }
            }
        };

        registry::register_pipeline(
            pipeline,
            layout,
            descriptor_set_layout,
            super::command::MAX_TEXTURE_BINDINGS,
        );

        Ok(Self {
            device: device.clone(),
            pipeline,
            layout,
            descriptor_set_layout,
            vertex_layout: descriptor.vertex_layout.clone(),
            blend_mode: descriptor.blend_mode,
            cull_mode: descriptor.cull_mode,
        })
    }

    /// Returns the Vulkan pipeline handle.
    pub fn vk_pipeline(&self) -> vk::Pipeline {
        self.pipeline
    }

    /// Returns the Vulkan pipeline layout handle.
    pub fn vk_layout(&self) -> vk::PipelineLayout {
        self.layout
    }
}

impl Drop for VulkanPipeline {
    fn drop(&mut self) {
        registry::unregister_pipeline(self.pipeline);
        unsafe {
            self.device.destroy_pipeline(self.pipeline, None);
            self.device.destroy_pipeline_layout(self.layout, None);
            self.device
                .destroy_descriptor_set_layout(self.descriptor_set_layout, None);
        }
    }
}

impl PipelineTrait for VulkanPipeline {
    fn vertex_layout(&self) -> &crate::rhi::types::VertexLayout {
        &self.vertex_layout
    }

    fn blend_mode(&self) -> BlendMode {
        self.blend_mode
    }

    fn cull_mode(&self) -> CullMode {
        self.cull_mode
    }

    fn bind(&self) -> Result<(), Box<dyn std::error::Error>> {
        // Binding happens in command buffer recording.
        Ok(())
    }

    fn native_handle(&self) -> u64 {
        self.pipeline.as_raw()
    }
}
