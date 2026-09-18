//! Graphics Pipeline
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//! **TODO: Full implementation required**

use crate::resource::sampler::CompareOp;
use crate::shader::ShaderModule;
use crate::types::*;

/// Vertex attribute
#[derive(Debug, Clone)]
pub struct VertexAttribute {
    pub location: u32,
    pub binding: u32,
    pub format: Format,
    pub offset: u32,
}

/// Vertex binding
#[derive(Debug, Clone)]
pub struct VertexBinding {
    pub binding: u32,
    pub stride: u32,
    pub input_rate: VertexInputRate,
}

/// Vertex input rate
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum VertexInputRate {
    #[default]
    Vertex,
    Instance,
}

/// Vertex input state
#[derive(Debug, Clone, Default)]
pub struct VertexInputState {
    pub bindings: Vec<VertexBinding>,
    pub attributes: Vec<VertexAttribute>,
}

impl VertexInputState {
    /// Creates a vertex input state with a single binding consisting of the
    /// given formats laid out sequentially from offset 0.
    pub fn from_formats(attributes: &[(u32, Format)]) -> Self {
        let stride: u32 = attributes.iter().map(|(_, f)| format_size(*f)).sum();

        Self {
            bindings: vec![VertexBinding {
                binding: 0,
                stride,
                input_rate: VertexInputRate::Vertex,
            }],
            attributes: attributes
                .iter()
                .scan(0u32, |offset, (location, format)| {
                    let o = *offset;
                    *offset += format_size(*format);
                    Some(VertexAttribute {
                        location: *location,
                        binding: 0,
                        format: *format,
                        offset: o,
                    })
                })
                .collect(),
        }
    }

    /// Returns the total stride for the given binding, or the attribute
    /// offsets recomputed when the binding has no explicit stride.
    pub fn binding_stride(&self, binding: u32) -> u32 {
        self.bindings
            .iter()
            .find(|b| b.binding == binding)
            .map(|b| b.stride)
            .unwrap_or(0)
    }
}

/// Returns the size in bytes of a single texel of the given format.
pub fn format_size(format: Format) -> u32 {
    use Format::*;
    match format {
        R8_UNORM | R8_SNORM | R8_UINT | R8_SINT | S8_UINT | A8_UNORM => 1,
        R16_UNORM | R16_SNORM | R16_UINT | R16_SINT | R16_SFLOAT | RG8_UNORM | RG8_SNORM
        | RG8_UINT | RG8_SINT => 2,
        R32_UINT | R32_SINT | R32_SFLOAT | RG16_UNORM | RG16_SNORM | RG16_UINT | RG16_SINT
        | RG16_SFLOAT | RGBA8_UNORM | RGBA8_SNORM | RGBA8_UINT | RGBA8_SINT
        | B10G11R11_UFLOAT | E5B9G9R9_UFLOAT => 4,
        R32G32_UINT | R32G32_SINT | R32G32_SFLOAT | RGBA16_UNORM | RGBA16_SFLOAT
        | D16_UNORM | D24_UNORM_S8_UINT => 8,
        R32G32B32_UINT | R32G32B32_SINT | R32G32B32_SFLOAT | D32_SFLOAT
        | D32_SFLOAT_S8_UINT => 12,
        RGBA32_UINT | RGBA32_SINT | RGBA32_SFLOAT => 16,
        _ => 0,
    }
}

/// Primitive topology
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PrimitiveTopology {
    #[default]
    PointList,
    LineList,
    LineStrip,
    TriangleList,
    TriangleStrip,
    TriangleFan,
}

/// Input assembly state
#[derive(Debug, Clone, Default)]
pub struct InputAssemblyState {
    pub topology: PrimitiveTopology,
    pub primitive_restart_enable: bool,
}

/// Polygon mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PolygonMode {
    #[default]
    Fill,
    Line,
    Point,
}

/// Cull mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum CullMode {
    #[default]
    None,
    Front,
    Back,
    FrontAndBack,
}

/// Front face
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum FrontFace {
    #[default]
    CounterClockwise,
    Clockwise,
}

/// Rasterizer state
#[derive(Debug, Clone, Default)]
pub struct RasterizerState {
    pub polygon_mode: PolygonMode,
    pub cull_mode: CullMode,
    pub front_face: FrontFace,
    pub depth_clamp_enable: bool,
    pub rasterizer_discard_enable: bool,
    pub depth_bias_enable: bool,
    pub depth_bias_constant: f32,
    pub depth_bias_clamp: f32,
    pub depth_bias_slope: f32,
    pub line_width: f32,
}

impl RasterizerState {
    /// Solid fill with back-face culling (the common 3D default).
    pub fn solid() -> Self {
        Self {
            polygon_mode: PolygonMode::Fill,
            cull_mode: CullMode::Back,
            front_face: FrontFace::CounterClockwise,
            line_width: 1.0,
            ..Self::default()
        }
    }

    /// Wireframe fill with no culling.
    pub fn wireframe() -> Self {
        Self {
            polygon_mode: PolygonMode::Line,
            line_width: 1.0,
            ..Self::default()
        }
    }

    /// Point fill with no culling (debug helper).
    pub fn points() -> Self {
        Self {
            polygon_mode: PolygonMode::Point,
            ..Self::default()
        }
    }

    /// Solid fill with no culling (used for debug / double-sided rendering).
    pub fn double_sided() -> Self {
        Self {
            polygon_mode: PolygonMode::Fill,
            line_width: 1.0,
            ..Self::default()
        }
    }
}

/// Stencil operation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum StencilOp {
    #[default]
    Keep,
    Zero,
    Replace,
    IncrementAndClamp,
    DecrementAndClamp,
    Invert,
    IncrementAndWrap,
    DecrementAndWrap,
}

/// Stencil operation state
#[derive(Debug, Clone, Default)]
pub struct StencilOpState {
    pub fail_op: StencilOp,
    pub pass_op: StencilOp,
    pub depth_fail_op: StencilOp,
    pub compare_op: CompareOp,
    pub compare_mask: u32,
    pub write_mask: u32,
    pub reference: u32,
}

/// Depth stencil state
#[derive(Debug, Clone, Default)]
pub struct DepthStencilState {
    pub depth_test_enable: bool,
    pub depth_write_enable: bool,
    pub depth_compare_op: CompareOp,
    pub depth_bounds_test_enable: bool,
    pub min_depth_bounds: f32,
    pub max_depth_bounds: f32,
    pub stencil_test_enable: bool,
    pub front: StencilOpState,
    pub back: StencilOpState,
}

impl DepthStencilState {
    /// Depth testing + writing with `Less` comparison (the common default).
    pub fn enabled() -> Self {
        Self {
            depth_test_enable: true,
            depth_write_enable: true,
            depth_compare_op: CompareOp::Less,
            ..Self::default()
        }
    }

    /// Depth testing + writing with `LessOrEqual` comparison.
    pub fn less_equal() -> Self {
        Self {
            depth_test_enable: true,
            depth_write_enable: true,
            depth_compare_op: CompareOp::LessOrEqual,
            ..Self::default()
        }
    }

    /// Depth testing only (no writes), useful for depth-prepass consumers.
    pub fn read_only() -> Self {
        Self {
            depth_test_enable: true,
            depth_write_enable: false,
            depth_compare_op: CompareOp::LessOrEqual,
            ..Self::default()
        }
    }

    /// Stencil testing with full 8-bit write/compare masks on both faces.
    pub fn stencil(front: StencilOpState, back: StencilOpState) -> Self {
        Self {
            depth_test_enable: true,
            depth_write_enable: true,
            depth_compare_op: CompareOp::Less,
            stencil_test_enable: true,
            front,
            back,
            ..Self::default()
        }
    }
}

/// Blend factor
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum BlendFactor {
    #[default]
    Zero,
    One,
    SrcColor,
    DstColor,
    SrcAlpha,
    DstAlpha,
    ConstantColor,
    ConstantAlpha,
    SrcAlphaSaturate,
    Src1Color,
    Src1Alpha,
    OneMinusSrcColor,
    OneMinusDstColor,
    OneMinusSrcAlpha,
    OneMinusDstAlpha,
    OneMinusConstantColor,
    OneMinusConstantAlpha,
    OneMinusSrc1Color,
    OneMinusSrc1Alpha,
}

/// Blend operation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum BlendOp {
    #[default]
    Add,
    Subtract,
    ReverseSubtract,
    Min,
    Max,
}

/// Color blend attachment
#[derive(Debug, Clone, Default)]
pub struct ColorBlendAttachment {
    pub blend_enable: bool,
    pub src_color_blend_factor: BlendFactor,
    pub dst_color_blend_factor: BlendFactor,
    pub color_blend_op: BlendOp,
    pub src_alpha_blend_factor: BlendFactor,
    pub dst_alpha_blend_factor: BlendFactor,
    pub alpha_blend_op: BlendOp,
    pub color_write_mask: ColorComponentFlags,
}

/// Logic operation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum LogicOp {
    #[default]
    Clear,
    And,
    AndReverse,
    Copy,
    AndInverted,
    NoOp,
    Xor,
    Or,
    Nor,
    Equivalent,
    Invert,
    OrReverse,
    CopyInverted,
    OrInverted,
    Nand,
    Set,
}

/// Color blend state
#[derive(Debug, Clone, Default)]
pub struct ColorBlendState {
    pub logic_op_enable: bool,
    pub logic_op: LogicOp,
    pub attachments: Vec<ColorBlendAttachment>,
    pub blend_constants: [f32; 4],
}

impl ColorBlendState {
    /// Constructor for an opaque (disabled) blend state.
    pub fn disabled() -> Self {
        Self::default()
    }

    /// Constructor for standard alpha blending.
    pub fn alpha() -> Self {
        Self {
            attachments: vec![ColorBlendAttachment {
                blend_enable: true,
                src_color_blend_factor: BlendFactor::SrcAlpha,
                dst_color_blend_factor: BlendFactor::OneMinusSrcAlpha,
                color_blend_op: BlendOp::Add,
                src_alpha_blend_factor: BlendFactor::One,
                dst_alpha_blend_factor: BlendFactor::OneMinusSrcAlpha,
                alpha_blend_op: BlendOp::Add,
                color_write_mask: Self::all_channels(),
            }],
            ..Default::default()
        }
    }

    /// Constructor for additive blending.
    pub fn additive() -> Self {
        Self {
            attachments: vec![ColorBlendAttachment {
                blend_enable: true,
                src_color_blend_factor: BlendFactor::One,
                dst_color_blend_factor: BlendFactor::One,
                color_blend_op: BlendOp::Add,
                src_alpha_blend_factor: BlendFactor::One,
                dst_alpha_blend_factor: BlendFactor::One,
                alpha_blend_op: BlendOp::Add,
                color_write_mask: Self::all_channels(),
            }],
            ..Default::default()
        }
    }

    /// Constructor for multiplicative blending.
    pub fn multiplicative() -> Self {
        Self {
            attachments: vec![ColorBlendAttachment {
                blend_enable: true,
                src_color_blend_factor: BlendFactor::DstColor,
                dst_color_blend_factor: BlendFactor::Zero,
                color_blend_op: BlendOp::Add,
                src_alpha_blend_factor: BlendFactor::DstAlpha,
                dst_alpha_blend_factor: BlendFactor::Zero,
                alpha_blend_op: BlendOp::Add,
                color_write_mask: Self::all_channels(),
            }],
            ..Default::default()
        }
    }

    /// Constructor for screen (inverse multiply) blending.
    pub fn screen() -> Self {
        Self {
            attachments: vec![ColorBlendAttachment {
                blend_enable: true,
                src_color_blend_factor: BlendFactor::One,
                dst_color_blend_factor: BlendFactor::OneMinusSrcColor,
                color_blend_op: BlendOp::Add,
                src_alpha_blend_factor: BlendFactor::One,
                dst_alpha_blend_factor: BlendFactor::OneMinusSrcAlpha,
                alpha_blend_op: BlendOp::Add,
                color_write_mask: Self::all_channels(),
            }],
            ..Default::default()
        }
    }

    fn all_channels() -> ColorComponentFlags {
        ColorComponentFlags::R | ColorComponentFlags::G | ColorComponentFlags::B | ColorComponentFlags::A
    }
}

/// Viewport state
#[derive(Debug, Clone, Default)]
pub struct ViewportState {
    pub viewports: Vec<Viewport>,
    pub scissors: Vec<Scissor>,
}

/// Multisample state
#[derive(Debug, Clone, Default)]
pub struct MultisampleState {
    pub sample_count: SampleCount,
    pub sample_mask: Option<Vec<u32>>,
    pub alpha_to_coverage_enable: bool,
    pub alpha_to_one_enable: bool,
}

/// Pipeline shader stage
#[derive(Debug, Clone, Default)]
pub struct PipelineShaderStage {
    pub stage: ShaderStage,
    pub module: ShaderModule,
    pub entry_point: String,
}

/// Graphics pipeline description
#[derive(Debug, Clone, Default)]
pub struct GraphicsPipelineDesc {
    pub shader_stages: Vec<PipelineShaderStage>,
    pub vertex_input_state: Option<VertexInputState>,
    pub input_assembly_state: InputAssemblyState,
    pub rasterizer_state: Option<RasterizerState>,
    pub depth_stencil_state: Option<DepthStencilState>,
    pub color_blend_state: Option<ColorBlendState>,
    pub viewport_state: ViewportState,
    pub multisample_state: MultisampleState,
}

/// Graphics pipeline
#[derive(Debug, Clone)]
pub struct GraphicsPipeline {
    desc: GraphicsPipelineDesc,
}

impl GraphicsPipeline {
    pub fn new(desc: GraphicsPipelineDesc) -> Self {
        Self { desc }
    }

    pub fn desc(&self) -> &GraphicsPipelineDesc {
        &self.desc
    }

    /// Returns `true` if the pipeline has a shader stage of the given type.
    pub fn has_stage(&self, stage: ShaderStage) -> bool {
        self.desc.shader_stages.iter().any(|s| s.stage == stage)
    }

    /// Returns `true` if the pipeline has at least a vertex stage.
    pub fn has_vertex_stage(&self) -> bool {
        self.has_stage(ShaderStage::VERTEX)
    }

    /// Returns the vertex input bindings, if any.
    pub fn vertex_input(&self) -> Option<&VertexInputState> {
        self.desc.vertex_input_state.as_ref()
    }

    /// Returns `true` when the pipeline state is self-consistent enough to be
    /// submitted for backend creation.
    pub fn is_valid(&self) -> bool {
        !self.desc.shader_stages.is_empty()
            && self.desc.shader_stages.len() <= 5
            && (self.has_stage(ShaderStage::VERTEX)
                || self.desc.input_assembly_state.topology == PrimitiveTopology::PointList)
    }
}

/// Render-layer alias for `VertexInputState`.
pub type VertexInputDesc = VertexInputState;

/// Render-layer alias for `RasterizerState`.
pub type RasterizerDesc = RasterizerState;

/// Render-layer alias for `DepthStencilState`.
pub type DepthStencilDesc = DepthStencilState;

/// Render-layer alias for `ColorBlendState` (provides `BlendDesc::disabled()`, etc.).
pub type BlendDesc = ColorBlendState;

/// Render-layer alias for `ColorBlendAttachment`.
pub type ColorTargetDesc = ColorBlendAttachment;
