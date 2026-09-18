use rhi::pipeline::compute::{ComputePipeline, ComputePipelineDesc};
use rhi::pipeline::graphics::{
    format_size, ColorBlendState, CullMode, DepthStencilState, FrontFace, GraphicsPipeline,
    GraphicsPipelineDesc, PipelineShaderStage, PolygonMode, PrimitiveTopology, RasterizerState,
    VertexInputRate, VertexInputState,
};
use rhi::pipeline::state::{PipelineLayout, PushConstantRange};
use rhi::shader::module::{ShaderFormat, ShaderModule, ShaderModuleDesc};
use rhi::types::{Format, ShaderStage};

fn vertex_stage() -> PipelineShaderStage {
    PipelineShaderStage {
        stage: ShaderStage::VERTEX,
        module: ShaderModule::new(ShaderModuleDesc {
            code: vec![0x03, 0x02, 0x23, 0x07],
            format: ShaderFormat::SpirV,
            entry_point: Some("main".into()),
            name: Some("vs".into()),
        }),
        entry_point: "main".into(),
    }
}

fn fragment_stage() -> PipelineShaderStage {
    PipelineShaderStage {
        stage: ShaderStage::FRAGMENT,
        module: ShaderModule::new(ShaderModuleDesc {
            code: vec![0x03, 0x02, 0x23, 0x07],
            format: ShaderFormat::SpirV,
            entry_point: Some("main".into()),
            name: Some("fs".into()),
        }),
        entry_point: "main".into(),
    }
}

#[test]
fn format_sizes() {
    assert_eq!(format_size(Format::R8_UNORM), 1);
    assert_eq!(format_size(Format::RG8_UNORM), 2);
    assert_eq!(format_size(Format::R32_SFLOAT), 4);
    assert_eq!(format_size(Format::RGBA8_UNORM), 4);
    assert_eq!(format_size(Format::RG16_SFLOAT), 4);
    assert_eq!(format_size(Format::RGBA32_SFLOAT), 16);
}

#[test]
fn vertex_input_from_formats() {
    let state = VertexInputState::from_formats(&[
        (0, Format::R32G32B32_SFLOAT),
        (1, Format::R32G32_SFLOAT),
        (2, Format::RGBA8_UNORM),
    ]);

    assert_eq!(state.bindings.len(), 1);
    assert_eq!(state.bindings[0].stride, 12 + 8 + 4);
    assert_eq!(state.bindings[0].input_rate, VertexInputRate::Vertex);

    assert_eq!(state.attributes.len(), 3);
    assert_eq!(state.attributes[0].offset, 0);
    assert_eq!(state.attributes[1].offset, 12);
    assert_eq!(state.attributes[2].offset, 20);
    assert_eq!(state.binding_stride(0), 24);
    assert_eq!(state.binding_stride(3), 0);
}

#[test]
fn rasterizer_presets() {
    let solid = RasterizerState::solid();
    assert_eq!(solid.polygon_mode, PolygonMode::Fill);
    assert_eq!(solid.cull_mode, CullMode::Back);
    assert_eq!(solid.front_face, FrontFace::CounterClockwise);
    assert_eq!(solid.line_width, 1.0);

    let wire = RasterizerState::wireframe();
    assert_eq!(wire.polygon_mode, PolygonMode::Line);
    assert_eq!(wire.cull_mode, CullMode::None);

    let points = RasterizerState::points();
    assert_eq!(points.polygon_mode, PolygonMode::Point);

    let double_sided = RasterizerState::double_sided();
    assert_eq!(double_sided.cull_mode, CullMode::None);
}

#[test]
fn depth_stencil_presets() {
    let enabled = DepthStencilState::enabled();
    assert!(enabled.depth_test_enable);
    assert!(enabled.depth_write_enable);

    let le = DepthStencilState::less_equal();
    assert!(le.depth_test_enable);
    assert!(le.depth_write_enable);

    let ro = DepthStencilState::read_only();
    assert!(ro.depth_test_enable);
    assert!(!ro.depth_write_enable);

    let blank = DepthStencilState::default();
    assert!(!blank.depth_test_enable);
}

#[test]
fn blend_presets_exist() {
    let disabled = ColorBlendState::disabled();
    assert!(disabled.attachments.is_empty());

    let alpha = ColorBlendState::alpha();
    assert!(!alpha.attachments.is_empty());
    assert!(alpha.attachments[0].blend_enable);

    let additive = ColorBlendState::additive();
    assert!(additive.attachments[0].blend_enable);

    let mult = ColorBlendState::multiplicative();
    assert!(mult.attachments[0].blend_enable);

    let screen = ColorBlendState::screen();
    assert!(screen.attachments[0].blend_enable);
}

#[test]
fn graphics_pipeline_stages_and_validity() {
    let pipeline = GraphicsPipeline::new(GraphicsPipelineDesc {
        shader_stages: vec![vertex_stage(), fragment_stage()],
        ..Default::default()
    });

    assert!(pipeline.has_stage(ShaderStage::VERTEX));
    assert!(pipeline.has_stage(ShaderStage::FRAGMENT));
    assert!(pipeline.has_vertex_stage());
    assert!(!pipeline.has_stage(ShaderStage::COMPUTE));
    assert!(pipeline.is_valid());
}

#[test]
fn graphics_pipeline_invalid_cases() {
    let empty = GraphicsPipeline::new(GraphicsPipelineDesc::default());
    assert!(!empty.is_valid());

    let point_list = GraphicsPipeline::new(GraphicsPipelineDesc {
        input_assembly_state: rhi::pipeline::graphics::InputAssemblyState {
            topology: PrimitiveTopology::PointList,
            primitive_restart_enable: false,
        },
        ..Default::default()
    });
    assert!(!point_list.is_valid(), "no shader stages at all");
}

#[test]
fn compute_pipeline_helpers() {
    let stage = PipelineShaderStage {
        stage: ShaderStage::COMPUTE,
        module: ShaderModule::new(ShaderModuleDesc {
            code: vec![0x01, 0x02, 0x03],
            format: ShaderFormat::SpirV,
            ..Default::default()
        }),
        entry_point: "main".into(),
    };

    let pipeline = ComputePipeline::new(ComputePipelineDesc { shader: stage.clone() });
    assert!(pipeline.desc().is_valid());
    assert_eq!(pipeline.shader_stage().stage, ShaderStage::COMPUTE);

    let vertex_only = ComputePipelineDesc {
        shader: vertex_stage(),
    };
    assert!(!vertex_only.is_valid());
}

#[test]
fn pipeline_layout_helpers() {
    let layout = PipelineLayout {
        descriptor_set_layouts: Vec::new(),
        push_constant_ranges: vec![
            PushConstantRange {
                stage_flags: ShaderStage::VERTEX,
                offset: 0,
                size: 64,
            },
            PushConstantRange {
                stage_flags: ShaderStage::FRAGMENT,
                offset: 64,
                size: 32,
            },
        ],
    };
    assert_eq!(layout.push_constant_total_size(), 96);
    assert_eq!(layout.descriptor_set_count(), 0);
    assert!(!layout.is_empty());

    let empty = PipelineLayout::default();
    assert!(empty.is_empty());
    assert_eq!(empty.push_constant_total_size(), 0);
}

#[test]
fn shader_module_accessors() {
    let module = ShaderModule::new(ShaderModuleDesc {
        code: vec![0x03, 0x02, 0x23, 0x07],
        format: ShaderFormat::Glsl,
        entry_point: Some("main".into()),
        name: Some("test".into()),
    });

    assert_eq!(module.format(), ShaderFormat::Glsl);
    assert_eq!(module.name(), Some("test"));
    assert_eq!(module.entry_point(), Some("main"));
    assert_eq!(module.code_size(), 4);
    assert!(module.has_code());

    let empty = ShaderModule::default();
    assert!(!empty.has_code());
    assert_eq!(empty.code_size(), 0);
}