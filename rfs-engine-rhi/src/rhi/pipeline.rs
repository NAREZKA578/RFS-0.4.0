//! Pipeline state object abstraction.

use super::types::VertexLayout;

/// Polygon mode for rasterization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PolygonMode {
    /// Fill polygons.
    #[default]
    Fill,
    /// Draw polygon outlines.
    Line,
    /// Draw polygon vertices.
    Point,
}

/// Cull mode for backface culling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CullMode {
    /// Don't cull any faces.
    None,
    /// Cull front faces.
    Front,
    /// Cull back faces.
    #[default]
    Back,
    /// Cull both front and back faces.
    FrontAndBack,
}

/// Front face winding order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FrontFace {
    /// Counter-clockwise is front.
    #[default]
    Ccw,
    /// Clockwise is front.
    Cw,
}

/// Blend mode for color blending.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BlendMode {
    /// No blending.
    #[default]
    None,
    /// Alpha blending.
    Alpha,
    /// Additive blending.
    Additive,
    /// Premultiplied alpha.
    Premultiplied,
}

/// Depth state.
#[derive(Debug, Clone)]
pub struct DepthState {
    /// Whether depth testing is enabled.
    pub enabled: bool,
    /// Whether depth writing is enabled.
    pub write_enabled: bool,
    /// Depth comparison function.
    pub compare_func: CompareFunc,
}

impl Default for DepthState {
    fn default() -> Self {
        Self {
            enabled: true,
            write_enabled: true,
            compare_func: CompareFunc::Less,
        }
    }
}

/// Comparison function for depth/stencil tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompareFunc {
    Never,
    Less,
    Equal,
    LessEqual,
    Greater,
    NotEqual,
    GreaterEqual,
    Always,
}

/// Stencil state.
#[derive(Debug, Clone, Default)]
pub struct StencilState {
    /// Whether stencil testing is enabled.
    pub enabled: bool,
    /// Stencil read mask.
    pub read_mask: u8,
    /// Stencil write mask.
    pub write_mask: u8,
}

/// Describes a single uniform field in a push-constant block.
#[derive(Debug, Clone)]
pub struct UniformEntry {
    /// Field name (for debugging / reflection).
    pub name: String,
    /// Byte offset within the push-constant block.
    pub offset: u32,
    /// Byte size of the field.
    pub size: u32,
}

/// Pipeline descriptor.
#[derive(Debug, Clone)]
pub struct PipelineDescriptor {
    /// Vertex shader native handle.
    pub vertex_shader: u64,
    /// Fragment shader native handle.
    pub fragment_shader: u64,
    /// Vertex layout.
    pub vertex_layout: VertexLayout,
    /// Primitive topology.
    pub topology: super::command::PrimitiveTopology,
    /// Polygon mode.
    pub polygon_mode: PolygonMode,
    /// Cull mode.
    pub cull_mode: CullMode,
    /// Front face winding.
    pub front_face: FrontFace,
    /// Blend mode.
    pub blend_mode: BlendMode,
    /// Depth state.
    pub depth_state: DepthState,
    /// Stencil state.
    pub stencil_state: StencilState,
    /// Number of samples (MSAA).
    pub samples: u32,
    /// Optional color attachment formats for Vulkan dynamic rendering.
    pub color_formats: Option<Vec<super::texture::TextureFormat>>,
    /// Optional depth attachment format for Vulkan dynamic rendering.
    pub depth_format: Option<super::texture::TextureFormat>,
    /// Optional push-constant layout metadata for Vulkan (field name, offset, size).
    pub uniform_layout: Option<Vec<UniformEntry>>,
}

impl Default for PipelineDescriptor {
    fn default() -> Self {
        Self {
            vertex_shader: 0,
            fragment_shader: 0,
            vertex_layout: VertexLayout::default(),
            topology: super::command::PrimitiveTopology::TriangleList,
            polygon_mode: PolygonMode::Fill,
            cull_mode: CullMode::Back,
            front_face: FrontFace::Ccw,
            blend_mode: BlendMode::None,
            depth_state: DepthState::default(),
            stencil_state: StencilState::default(),
            samples: 1,
            color_formats: None,
            depth_format: None,
            uniform_layout: None,
        }
    }
}

/// Pipeline trait — implemented by each backend.
pub trait Pipeline: Send + Sync {
    /// Returns the vertex layout.
    fn vertex_layout(&self) -> &VertexLayout;

    /// Returns the blend mode.
    fn blend_mode(&self) -> BlendMode;

    /// Returns the cull mode.
    fn cull_mode(&self) -> CullMode;

    /// Binds the pipeline for rendering.
    fn bind(&self) -> Result<(), Box<dyn std::error::Error>>;

    /// Returns the backend-specific handle.
    fn native_handle(&self) -> u64;
}
