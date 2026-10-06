//! UI Renderer
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//!
//! Turns [`UiCommand`]s and [`UiElement`]s into real GPU work: geometry
//! batching, buffer uploads, and recorded draw calls.
//!
//! # Scope of this stage
//!
//! Working: filled rectangles, outlines, lines, progress bars, circular
//! progress, panel elements, scissor regions and transform groups — everything
//! that is a solid vertex-coloured quad.
//!
//! Not yet drawn: text (needs a font atlas) and images (need a texture
//! binding). Those commands are counted in [`UiRenderStats::skipped`]
//! rather than silently ignored, so it is visible that they are missing
//! instead of the UI looking merely empty.
//!
//! # Backends
//!
//! Nothing here is Vulkan-specific: the renderer only uses `Device`, and the
//! target is described by plain handles, so the same path serves every backend
//! that can supply them.

use std::collections::HashMap;
use std::sync::Arc;

use crate::command::commands::Command;
use crate::command::encoder::CommandEncoder;
use crate::command::pass::render::RenderPassBeginInfo;
use crate::descriptor::{
    DescriptorBinding, DescriptorInfo, DescriptorSet, DescriptorSetLayout,
    DescriptorSetLayoutDesc, DescriptorType, DescriptorWrite,
};
use crate::pipeline::graphics::{
    BlendFactor, BlendOp, ColorBlendAttachment, ColorBlendState, CullMode, GraphicsPipelineDesc,
    InputAssemblyState, MultisampleState, PipelineShaderStage, PrimitiveTopology, RasterizerState,
    VertexAttribute, VertexBinding, VertexInputRate, VertexInputState,
};
use crate::pipeline::GraphicsPipeline;
use crate::resource::Buffer;
use crate::types::{BufferUsage, Extent2D, Format, IndexType, Offset2D, Rect2D, ShaderStage, Viewport};
use crate::Device;

use super::elements::{UiElement, UiElementId};
use super::pass::UiRenderPass;
use super::shaders::{UiShaderManager, UiShaderType};
use super::{UiCommand, UiConfig, UiError, UiResult};

/// Bytes of uniform data the UI shader reads: one column-major `mat4x4`.
const UI_UNIFORM_SIZE: u64 = 16 * 4;

/// Smallest vertex buffer to allocate, so the first few frames of a busy HUD
/// do not each trigger a reallocation.
const MIN_VERTEX_CAPACITY: usize = 4096;
const MIN_INDEX_CAPACITY: usize = 6144;

/// Where the UI should draw this frame.
///
/// The renderer has no window and no swapchain of its own: it is handed the
/// image the caller acquired, which is the only way to draw into the image that
/// is about to be presented.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UiRenderTarget {
    /// Swapchain image handle.
    pub image: u64,
    /// Image view handle for `image`.
    pub view: u64,
    /// Format the surface negotiated. The pipeline is built for this.
    pub format: Format,
    pub width: u32,
    pub height: u32,
}

/// Tunables for the renderer.
#[derive(Debug, Clone)]
pub struct UiRendererConfig {
    /// Vertices per buffer allocation, before growth.
    pub initial_vertex_capacity: usize,
    /// Indices per buffer allocation, before growth.
    pub initial_index_capacity: usize,
    /// Record a debug label around the UI pass.
    pub debug_label: bool,
}

impl Default for UiRendererConfig {
    fn default() -> Self {
        Self {
            initial_vertex_capacity: MIN_VERTEX_CAPACITY,
            initial_index_capacity: MIN_INDEX_CAPACITY,
            debug_label: false,
        }
    }
}

/// Per-frame counters.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct UiRenderStats {
    pub commands_processed: u32,
    pub elements_rendered: u32,
    pub vertices: u32,
    pub triangles: u32,
    pub batches: u32,
    pub draw_calls: u32,
    /// Commands that produced no geometry because the feature is not built yet.
    pub skipped: u32,
}

/// A vertex as the UI pipeline declares it: 32 bytes, `#[repr(C)]`.
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(C)]
pub struct UiVertex {
    pub position: [f32; 2],
    pub tex_coords: [f32; 2],
    pub color: [f32; 4],
}

impl Default for UiVertex {
    fn default() -> Self {
        Self {
            position: [0.0; 2],
            tex_coords: [0.0; 2],
            color: [1.0; 4],
        }
    }
}

/// Consecutive draws that share one scissor rectangle.
#[derive(Debug, Clone, Default)]
struct UiDrawBatch {
    vertices: Vec<UiVertex>,
    indices: Vec<u32>,
    scissor: Option<(i32, i32, u32, u32)>,
}

/// The two draw streams of a frame.
///
/// They are flattened into the same vertex and index buffers but drawn with
/// different pipelines, so they stay separate until the buffers are assembled.
#[derive(Debug, Clone, Default)]
struct UiDrawBatches {
    geometry: Vec<UiDrawBatch>,
    text: Vec<UiDrawBatch>,
}

/// What one pass over the input produced, before anything is uploaded.
#[derive(Debug, Clone, Copy, Default)]
struct UiBuildStats {
    commands: u32,
    elements: u32,
    vertices: u32,
    indices: u32,
    /// Commands that produced no geometry because the feature is not built yet.
    skipped: u32,
}

/// A run of draws that share one scissor rectangle, in the flattened buffers.
///
/// `(first_index, index_count, scissor)`; the scissor is `None` when the batch
/// is unclipped and the whole target applies.
type UiSpan = (u32, u32, Option<(i32, i32, u32, u32)>);

/// A run of text plus the font it is set in, resolved to glyph quads.
struct UiTextDraw<'a> {
    layout: &'a super::text::UiTextLayout,
    color: [f32; 4],
    atlas_width: f32,
    atlas_height: f32,
}

/// GPU resources owned by the renderer.
struct UiGpu {
    pipeline: GraphicsPipeline,
    descriptor_layout: DescriptorSetLayout,
    descriptor_set: DescriptorSet,
    /// Textured pipeline, for glyphs and images. `None` when no text SPIR-V was
    /// supplied, in which case text is reported as skipped instead of crashing.
    text_pipeline: Option<GraphicsPipeline>,
    text_descriptor_layout: Option<DescriptorSetLayout>,
    text_descriptor_set: Option<DescriptorSet>,
    uniform_buffer: Buffer,
    vertex_buffer: Buffer,
    index_buffer: Buffer,
    vertex_capacity: usize,
    index_capacity: usize,
}

/// Builder state for one frame.
struct UiBuilder {
    batches: Vec<UiDrawBatch>,
    current: UiDrawBatch,
    /// Glyph quads. Kept apart from `batches` because they are drawn with the
    /// textured pipeline and its own descriptor set; interleaving them into one
    /// buffer would mean rebinding the pipeline mid-stream for no benefit.
    text_batches: Vec<UiDrawBatch>,
    text_current: UiDrawBatch,
    transform: [f32; 6],
    transform_stack: Vec<[f32; 6]>,
    scissor: Option<(i32, i32, u32, u32)>,
    scissor_stack: Vec<Option<(i32, i32, u32, u32)>>,
    /// Vertices the current frame would need, tracked so buffers can grow once
    /// up front rather than being reallocated mid-batch.
    vertex_total: usize,
    index_total: usize,
    skipped: u32,
}

impl UiBuilder {
    fn new() -> Self {
        Self {
            batches: Vec::new(),
            current: UiDrawBatch::default(),
            text_batches: Vec::new(),
            text_current: UiDrawBatch::default(),
            transform: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            transform_stack: Vec::new(),
            scissor: None,
            scissor_stack: Vec::new(),
            vertex_total: 0,
            index_total: 0,
            skipped: 0,
        }
    }

    /// Append one glyph quad.
    ///
    /// `atlas_rect` arrives as texels from the layout; the caller converts to
    /// UVs because only it knows the atlas dimensions. A zero-area glyph —
    /// a space, or a combining mark — emits nothing rather than a degenerate
    /// triangle that still costs a draw.
    fn glyph(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        uv: [f32; 4],
        color: [f32; 4],
    ) {
        if !(w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0) {
            return;
        }
        if !(x.is_finite() && y.is_finite()) {
            return;
        }
        if !(color.iter().all(|c| c.is_finite())) {
            return;
        }

        if self.text_current.scissor != self.scissor {
            self.flush_text();
            self.text_current.scissor = self.scissor;
        }

        let base = self.text_current.vertices.len() as u32;
        for (i, (cx, cy)) in [(x, y), (x + w, y), (x + w, y + h), (x, y + h)]
            .into_iter()
            .enumerate()
        {
            let p = self.apply(cx, cy);
            // Corners run TL, TR, BR, BL and the rect is likewise TL, TR, BR,
            // BL, so the UV corner index matches without a lookup table.
            let uv_corner = match i {
                0 => [uv[0], uv[1]],
                1 => [uv[2], uv[1]],
                2 => [uv[2], uv[3]],
                _ => [uv[0], uv[3]],
            };
            self.text_current.vertices.push(UiVertex {
                position: p,
                tex_coords: uv_corner,
                color,
            });
        }
        self.text_current
            .indices
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);

        self.vertex_total += 4;
        self.index_total += 6;
    }

    fn flush_text(&mut self) {
        if !self.text_current.indices.is_empty() {
            self.text_batches
                .push(std::mem::take(&mut self.text_current));
        }
        self.text_current.scissor = self.scissor;
    }

    fn flush_all(&mut self) {
        self.flush();
        self.flush_text();
    }

    /// Emit every glyph of `draw` as a textured quad.
    ///
    /// The layout carries texel rectangles; they become UVs here, where the
    /// atlas dimensions are finally known.
    fn text(&mut self, draw: &UiTextDraw<'_>) {
        let inv_w = if draw.atlas_width.is_finite() && draw.atlas_width > 0.0 {
            1.0 / draw.atlas_width
        } else {
            // A zero-sized atlas would divide by zero and put infinities into
            // every UV, sampling garbage. Dropping the text is the honest
            // outcome: there is nothing to sample.
            return;
        };
        let inv_h = if draw.atlas_height.is_finite() && draw.atlas_height > 0.0 {
            1.0 / draw.atlas_height
        } else {
            return;
        };

        for glyph in draw.layout.glyphs() {
            let r = glyph.glyph.atlas_rect;
            if !(r.iter().all(|v| v.is_finite())) {
                continue;
            }
            let uv = [r[0] * inv_w, r[1] * inv_h, r[2] * inv_w, r[3] * inv_h];
            self.glyph(
                glyph.position[0],
                glyph.position[1],
                glyph.glyph.size[0],
                glyph.glyph.size[1],
                uv,
                draw.color,
            );
        }
    }

    fn apply(&self, x: f32, y: f32) -> [f32; 2] {
        let m = &self.transform;
        [m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5]]
    }

    /// Append an axis-aligned quad. Winding is irrelevant because the pipeline
    /// culls nothing, but the order is kept consistent anyway.
    fn quad(&mut self, x: f32, y: f32, w: f32, h: f32, color: [f32; 4]) {
        // A non-finite component here is not merely cosmetic: these values go
        // straight into the uploaded vertex buffer, and a NaN position makes the
        // whole primitive discard on some drivers while a NaN colour can
        // propagate through blending for every primitive that follows.
        //
        // Every check is a positive test on purpose. Written as `!(w > 0.0)` the
        // NaN case is handled too, but only implicitly, and the intent is not
        // visible to a reader.
        let finite = |v: f32| v.is_finite();
        let positive = |v: f32| v.is_finite() && v > 0.0;

        if !finite(x) || !finite(y) || !positive(w) || !positive(h) {
            return;
        }
        if !(color.iter().all(|c| c.is_finite())) {
            return;
        }

        // A change of scissor starts a new batch, so that recording can emit
        // `SetScissor` only where it actually differs.
        if self.current.scissor != self.scissor {
            self.flush();
            self.current.scissor = self.scissor;
        }

        let base = self.current.vertices.len() as u32;
        let corners = [
            (x, y),
            (x + w, y),
            (x + w, y + h),
            (x, y + h),
        ];
        for (cx, cy) in corners {
            let p = self.apply(cx, cy);
            self.current.vertices.push(UiVertex {
                position: p,
                tex_coords: [0.0, 0.0],
                color,
            });
        }
        self.current
            .indices
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);

        self.vertex_total += 4;
        self.index_total += 6;
    }

    /// Axis-aligned outline drawn as four thin quads.
    fn outline(&mut self, x: f32, y: f32, w: f32, h: f32, t: f32, color: [f32; 4]) {
        if !(w.is_finite() && h.is_finite() && t.is_finite()) {
            return;
        }
        if !(w > 0.0 && h > 0.0 && t > 0.0) {
            return;
        }
        // A border thicker than the shape it outlines would draw outside it.
        let t = t.min(w).min(h);
        self.quad(x, y, w, t, color);
        self.quad(x, y + h - t, w, t, color);
        self.quad(x, y + t, t, h - 2.0 * t, color);
        self.quad(x + w - t, y + t, t, h - 2.0 * t, color);
    }

    /// A line of finite thickness, as a quad around the segment.
    fn line(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, t: f32, color: [f32; 4]) {
        if !(t.is_finite() && t > 0.0) {
            return;
        }
        if !(x1.is_finite() && y1.is_finite() && x2.is_finite() && y2.is_finite()) {
            return;
        }
        let dx = x2 - x1;
        let dy = y2 - y1;
        let len = (dx * dx + dy * dy).sqrt();
        if !(len.is_finite() && len > 0.0) {
            return;
        }
        // Normal to the segment, halved so the quad straddles the line.
        let (nx, ny) = (-dy / len * t * 0.5, dx / len * t * 0.5);
        let base = self.current.vertices.len() as u32;
        for (px, py) in [
            (x1 + nx, y1 + ny),
            (x2 + nx, y2 + ny),
            (x2 - nx, y2 - ny),
            (x1 - nx, y1 - ny),
        ] {
            let p = self.apply(px, py);
            self.current.vertices.push(UiVertex {
                position: p,
                tex_coords: [0.0, 0.0],
                color,
            });
        }
        self.current
            .indices
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        self.vertex_total += 4;
        self.index_total += 6;
    }

    /// Clamp a scissor to the target and convert to the integer form the
    /// command takes. `None` when nothing would be visible.
    fn clamp_scissor(
        rect: (f32, f32, f32, f32),
        width: u32,
        height: u32,
    ) -> Option<(i32, i32, u32, u32)> {
        let (x, y, w, h) = rect;
        if !(w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0) {
            return None;
        }
        if !(x.is_finite() && y.is_finite()) {
            return None;
        }
        let x0 = x.max(0.0).floor().min(width as f32) as i32;
        let y0 = y.max(0.0).floor().min(height as f32) as i32;
        let x1 = (x + w).max(0.0).ceil().min(width as f32) as i32;
        let y1 = (y + h).max(0.0).ceil().min(height as f32) as i32;
        if x1 <= x0 || y1 <= y0 {
            return None;
        }
        Some((x0, y0, (x1 - x0) as u32, (y1 - y0) as u32))
    }

    fn flush(&mut self) {
        if !self.current.indices.is_empty() {
            self.batches.push(std::mem::take(&mut self.current));
        }
        // `take` resets the batch, including its scissor, so restore it.
        self.current.scissor = self.scissor;
    }
}

/// The UI renderer.
pub struct UiRenderer {
    device: Arc<Device>,
    config: UiRendererConfig,
    ui_config: UiConfig,
    shaders: UiShaderManager,
    pass: Option<UiRenderPass>,
    gpu: Option<UiGpu>,
    command_queue: Vec<UiCommand>,
    elements: HashMap<UiElementId, UiElement>,
    texture_cache: HashMap<u64, Arc<crate::resource::Texture>>,
    /// Fonts available to `UiCommand::Text`, by name.
    fonts: HashMap<String, Box<dyn super::text::UiFont>>,
    /// Name of the font used when a command does not name one.
    default_font: Option<String>,
    /// The glyph atlas, with its size in texels.
    atlas: Option<(Arc<crate::resource::Texture>, u32, u32)>,
    /// Text laid out ahead of recording, waiting to become geometry.
    pending_text: Vec<(super::text::UiTextLayout, [f32; 4])>,
    stats: UiRenderStats,
    frame_number: u64,
    initialized: bool,
}

impl UiRenderer {
    pub fn new(device: Arc<Device>, config: UiRendererConfig) -> Self {
        Self {
            shaders: UiShaderManager::new(device.clone()),
            device,
            config,
            ui_config: UiConfig::default(),
            pass: None,
            gpu: None,
            command_queue: Vec::new(),
            elements: HashMap::new(),
            texture_cache: HashMap::new(),
            fonts: HashMap::new(),
            default_font: None,
            atlas: None,
            pending_text: Vec::new(),
            stats: UiRenderStats::default(),
            frame_number: 0,
            initialized: false,
        }
    }

    /// Build every resource the UI needs.
    ///
    /// `geometry_spirv` is passed in rather than read from disk: `rhi` is a
    /// separate crate and must not know where assets live. The caller embeds
    /// the `.spv` with `include_bytes!`.
    pub fn initialize(
        &mut self,
        format: Format,
        width: u32,
        height: u32,
        geometry_spirv: &[u8],
        text_spirv: Option<&[u8]>,
    ) -> UiResult<()> {
        self.shaders.load_geometry(geometry_spirv)?;

        let pass = UiRenderPass::new(&self.device, format, width, height)?;
        let (uniform_buffer, descriptor_layout, descriptor_set) = self.build_uniforms()?;
        let pipeline = self.build_pipeline(&descriptor_layout, &pass)?;

        // Text is optional. A missing or broken text shader must not stop panels
        // and buttons from drawing, so a failure here leaves the pipeline unset
        // and text shows up in `skipped`.
        let mut text_pipeline = None;
        let mut text_descriptor_layout = None;
        let mut text_descriptor_set = None;
        if let Some(spirv) = text_spirv {
            match self.shaders.load_text(spirv) {
                Ok(()) => {}
                Err(e) => eprintln!("[ui] text shader unavailable: {e}"),
            }
            if self.shaders.is_loaded(UiShaderType::Text) {
                let layout = self.build_text_layout()?;
                let set = self.device
                    .create_descriptor_set(&layout)
                    .map_err(|e| UiError::BackendError(format!("UI text descriptor set: {e}")))?;
                let pipeline = self.build_text_pipeline(&layout, &pass)?;
                text_descriptor_layout = Some(layout);
                text_descriptor_set = Some(set);
                text_pipeline = Some(pipeline);
            }
        }

        let vertex_capacity = self.config.initial_vertex_capacity.max(MIN_VERTEX_CAPACITY);
        let index_capacity = self.config.initial_index_capacity.max(MIN_INDEX_CAPACITY);
        let (vertex_buffer, index_buffer) = self.build_buffers(vertex_capacity, index_capacity)?;

        self.pass = Some(pass);
        self.gpu = Some(UiGpu {
            pipeline,
            descriptor_layout,
            descriptor_set,
            text_pipeline,
            text_descriptor_layout,
            text_descriptor_set,
            uniform_buffer,
            vertex_buffer,
            index_buffer,
            vertex_capacity,
            index_capacity,
        });
        self.ui_config.screen_width = width;
        self.ui_config.screen_height = height;
        self.initialized = true;
        Ok(())
    }

    fn build_uniforms(&self) -> UiResult<(Buffer, DescriptorSetLayout, DescriptorSet)> {
        let uniform = self
            .device
            .create_buffer(UI_UNIFORM_SIZE, BufferUsage::UNIFORM, true);
        if !uniform.has_gpu_backing() {
            // Reported as an allocation failure, not `RendererNotInitialized`.
            // That variant is what `record` returns when the renderer was never
            // built, so using it here pointed the reader at the wrong thing
            // entirely: the message said "not initialized" while the actual
            // cause was a buffer that got no GPU memory.
            return Err(UiError::BackendError(format!(
                "UI uniform buffer ({UI_UNIFORM_SIZE} bytes) got no GPU backing"
            )));
        }

        let layout = self.device.create_descriptor_set_layout(&DescriptorSetLayoutDesc {
            bindings: vec![DescriptorBinding {
                binding: 0,
                ty: DescriptorType::UniformBuffer,
                count: 1,
                stages: ShaderStage::VERTEX,
                immutable_samplers: None,
            }],
        });

        let set = self
            .device
            .create_descriptor_set(&layout)
            .map_err(|e| UiError::BackendError(format!("UI descriptor set: {e}")))?;

        self.device
            .write_descriptors(
                &set,
                &[DescriptorWrite {
                    dst_set: set.clone(),
                    dst_binding: 0,
                    dst_array_element: 0,
                    descriptors: vec![DescriptorInfo::Buffer(uniform.clone(), 0, UI_UNIFORM_SIZE)],
                }],
            )
            .map_err(|e| UiError::BackendError(format!("UI descriptor write: {e}")))?;

        Ok((uniform, layout, set))
    }

    fn build_pipeline(
        &self,
        layout: &DescriptorSetLayout,
        pass: &UiRenderPass,
    ) -> UiResult<GraphicsPipeline> {
        let module = self.shaders.module(UiShaderType::Geometry)?;
        let desc = self.pipeline_desc(
            module.clone(),
            UiShaderType::VERTEX_ENTRY,
            UiShaderType::FRAGMENT_ENTRY,
        );

        let pipeline = self
            .device
            .create_gpu_graphics_pipeline(&desc, pass.render_pass(), &[layout])
            .map_err(|e| UiError::BackendError(format!("UI pipeline: {e}")))?;

        if !pipeline.has_gpu_backing() {
            return Err(UiError::BackendError(
                "the UI graphics pipeline was not created on the device".into(),
            ));
        }
        Ok(pipeline)
    }

    /// The pipeline state shared by the untextured and textured UI pipelines.
    ///
    /// Only the fragment stage differs between them, so everything else is
    /// built once here rather than restated per shader.
    fn pipeline_desc(
        &self,
        module: crate::shader::module::ShaderModule,
        vertex_entry: &str,
        fragment_entry: &str,
    ) -> GraphicsPipelineDesc {
        GraphicsPipelineDesc {
            shader_stages: vec![
                PipelineShaderStage {
                    stage: ShaderStage::VERTEX,
                    module: module.clone(),
                    entry_point: vertex_entry.to_string(),
                },
                PipelineShaderStage {
                    stage: ShaderStage::FRAGMENT,
                    module,
                    entry_point: fragment_entry.to_string(),
                },
            ],
            // Stride and offsets come from the real struct, so the layout cannot
            // drift away from what is actually uploaded.
            vertex_input_state: Some(Self::vertex_input_state()),
            input_assembly_state: InputAssemblyState {
                topology: PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            rasterizer_state: Some(RasterizerState {
                // UI quads are two-sided by construction and their winding
                // depends on which corner order the caller produced, so culling
                // would drop geometry that is meant to be visible.
                cull_mode: CullMode::None,
                ..RasterizerState::solid()
            }),
            // No depth: the UI draws over a finished frame and must never be
            // rejected by the scene's depth buffer.
            depth_stencil_state: None,
            color_blend_state: Some(ColorBlendState {
                attachments: vec![ColorBlendAttachment {
                    blend_enable: true,
                    src_color_blend_factor: BlendFactor::SrcAlpha,
                    dst_color_blend_factor: BlendFactor::OneMinusSrcAlpha,
                    color_blend_op: BlendOp::Add,
                    src_alpha_blend_factor: BlendFactor::One,
                    dst_alpha_blend_factor: BlendFactor::OneMinusSrcAlpha,
                    alpha_blend_op: BlendOp::Add,
                    ..Default::default()
                }],
                ..Default::default()
            }),
            multisample_state: MultisampleState {
                sample_count: crate::types::SampleCount::X1,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    /// Descriptor layout for the textured pipeline: uniform, atlas, sampler.
    fn build_text_layout(&self) -> UiResult<DescriptorSetLayout> {
        Ok(self.device.create_descriptor_set_layout(&DescriptorSetLayoutDesc {
            bindings: vec![
                DescriptorBinding {
                    binding: 0,
                    ty: DescriptorType::UniformBuffer,
                    count: 1,
                    stages: ShaderStage::VERTEX,
                    immutable_samplers: None,
                },
                DescriptorBinding {
                    binding: 1,
                    ty: DescriptorType::SampledTexture,
                    count: 1,
                    stages: ShaderStage::FRAGMENT,
                    immutable_samplers: None,
                },
                DescriptorBinding {
                    binding: 2,
                    ty: DescriptorType::Sampler,
                    count: 1,
                    stages: ShaderStage::FRAGMENT,
                    immutable_samplers: None,
                },
            ],
        }))
    }

    fn build_text_pipeline(
        &self,
        layout: &DescriptorSetLayout,
        pass: &UiRenderPass,
    ) -> UiResult<GraphicsPipeline> {
        let module = self.shaders.module(UiShaderType::Text)?;
        let mut desc = self.pipeline_desc(
            module.clone(),
            UiShaderType::VERTEX_ENTRY,
            UiShaderType::FRAGMENT_ENTRY,
        );
        // The atlas is sampled with a repeating sampler; glyph UVs are inside
        // [0,1] so repeat never actually wraps, but clamp would be the wrong
        // default if an atlas ever had padding computed modulo one.
        desc.multisample_state = MultisampleState {
            sample_count: crate::types::SampleCount::X1,
            ..Default::default()
        };
        let pipeline = self
            .device
            .create_gpu_graphics_pipeline(&desc, pass.render_pass(), &[layout])
            .map_err(|e| UiError::BackendError(format!("UI text pipeline: {e}")))?;
        if !pipeline.has_gpu_backing() {
            return Err(UiError::BackendError(
                "the UI text graphics pipeline was not created on the device".into(),
            ));
        }
        Ok(pipeline)
    }

    fn vertex_input_state() -> VertexInputState {
        use std::mem::{offset_of, size_of};
        VertexInputState {
            bindings: vec![VertexBinding {
                binding: 0,
                stride: size_of::<UiVertex>() as u32,
                input_rate: VertexInputRate::Vertex,
            }],
            attributes: vec![
                VertexAttribute {
                    binding: 0,
                    location: 0,
                    format: Format::R32G32_SFLOAT,
                    offset: offset_of!(UiVertex, position) as u32,
                },
                VertexAttribute {
                    binding: 0,
                    location: 1,
                    format: Format::R32G32_SFLOAT,
                    offset: offset_of!(UiVertex, tex_coords) as u32,
                },
                VertexAttribute {
                    binding: 0,
                    location: 2,
                    format: Format::RGBA32_SFLOAT,
                    offset: offset_of!(UiVertex, color) as u32,
                },
            ],
        }
    }

    fn build_buffers(
        &self,
        vertex_capacity: usize,
        index_capacity: usize,
    ) -> UiResult<(Buffer, Buffer)> {
        let vertex = self.device.create_buffer(
            (vertex_capacity * std::mem::size_of::<UiVertex>()) as u64,
            BufferUsage::VERTEX,
            true,
        );
        let index = self.device.create_buffer(
            (index_capacity * std::mem::size_of::<u32>()) as u64,
            BufferUsage::INDEX,
            true,
        );
        if !vertex.has_gpu_backing() || !index.has_gpu_backing() {
            return Err(UiError::BackendError(
                "UI vertex/index buffers were not created on the device".into(),
            ));
        }
        Ok((vertex, index))
    }

    pub fn is_initialized(&self) -> bool {
        self.initialized
    }

    pub fn ui_config(&self) -> &UiConfig {
        &self.ui_config
    }

    pub fn stats(&self) -> UiRenderStats {
        self.stats
    }

    pub fn reset_stats(&mut self) {
        self.stats = UiRenderStats::default();
    }

    pub fn frame_number(&self) -> u64 {
        self.frame_number
    }

    pub fn ui_config_mut(&mut self) -> &mut UiConfig {
        &mut self.ui_config
    }

    /// Update the target size after a resize.
    ///
    /// The pipeline is unaffected because it declares the surface format, not
    /// an extent. Buffers are kept: a resize does not imply different geometry.
    pub fn resize(&mut self, width: u32, height: u32) {
        if let Some(pass) = self.pass.as_mut() {
            pass.resize(width, height);
        }
        self.ui_config.screen_width = width;
        self.ui_config.screen_height = height;
    }

    /// Queue commands for the next [`Self::record`].
    pub fn submit_commands(&mut self, commands: Vec<UiCommand>) -> UiResult<()> {
        if !self.initialized {
            return Err(UiError::RendererNotInitialized);
        }
        self.command_queue.extend(commands);
        Ok(())
    }

    pub fn submit_element(&mut self, element: UiElement) -> UiResult<()> {
        if !self.initialized {
            return Err(UiError::RendererNotInitialized);
        }
        self.elements.insert(element.id, element);
        Ok(())
    }

    pub fn remove_element(&mut self, id: UiElementId) -> Option<UiElement> {
        self.elements.remove(&id)
    }

    pub fn clear_elements(&mut self) {
        self.elements.clear();
    }

    pub fn add_texture(&mut self, id: u64, texture: Arc<crate::resource::Texture>) {
        self.texture_cache.insert(id, texture);
    }

    /// Register a font under `name`, replacing any font already registered
    /// under it.
    ///
    /// The name is what [`UiCommand::Text`] refers to; `None` means the default
    /// font, which is whichever was registered first if several are.
    pub fn register_font(&mut self, name: &str, font: Box<dyn super::text::UiFont>) {
        self.fonts.insert(name.to_string(), font);
        if self.default_font.is_none() {
            self.default_font = Some(name.to_string());
        }
    }

    /// Attach the glyph atlas the fonts were rasterised into.
    ///
    /// Dimensions are in texels and are required: glyph rectangles from the
    /// layout are texel rectangles and cannot be turned into UVs without them.
    pub fn set_atlas(&mut self, texture: Arc<crate::resource::Texture>, width: u32, height: u32) {
        self.atlas = Some((texture, width, height));
    }

    /// Remove the atlas. Subsequent text is reported as skipped rather than
    /// sampling a texture that is no longer bound.
    pub fn clear_atlas(&mut self) {
        self.atlas = None;
    }

    pub fn has_atlas(&self) -> bool {
        self.atlas.is_some()
    }

    /// Lay `text` out with the named font and queue the resulting glyphs.
    ///
    /// Returns the layout so a caller can measure it, or `None` when the text
    /// cannot be drawn — no font by that name, or no atlas.
    pub fn push_text(
        &mut self,
        text: &str,
        font: Option<&str>,
        style: &super::text::UiTextStyle,
        origin: [f32; 2],
    ) -> Option<super::text::UiTextLayout> {
        let name = font.or(self.default_font.as_deref())?;
        let found = self.fonts.get(name)?;
        let layout = super::text::UiTextLayout::layout(text, found.as_ref(), style, origin);
        // Queued by value for drawing, and a copy handed back so the caller can
        // measure it. Cloning a layout is a Vec of small Copy structs, cheaper
        // than laying the same string out twice.
        self.pending_text.push((layout.clone(), style.color));
        Some(layout)
    }

    pub fn remove_texture(&mut self, id: u64) -> Option<Arc<crate::resource::Texture>> {
        self.texture_cache.remove(&id)
    }

    /// Turn the queued commands and elements into draw batches.
    ///
    /// Consumes the command queue: commands describe one frame, so retaining
    /// them would draw the same UI again next frame.
    fn build(&mut self, width: u32, height: u32) -> (UiDrawBatches, UiBuildStats) {
        let mut b = UiBuilder::new();

        let commands = std::mem::take(&mut self.command_queue);
        let command_count = commands.len() as u32;
        // Borrowed rather than cloned: laying text out needs the font metrics,
        // and the queue is the only thing that could be invalidated meanwhile.
        let pending = std::mem::take(&mut self.pending_text);
        let default_font = self.default_font.clone();
        // Laid-out text collected while walking the commands, joined with what
        // `push_text` queued earlier.
        let mut collected: Vec<(super::text::UiTextLayout, [f32; 4])> = Vec::new();
        for command in &commands {
            Self::emit_command(
                &mut b,
                command,
                width,
                height,
                &self.fonts,
                default_font.as_deref(),
                &mut collected,
            );
        }
        let mut pending = pending;
        pending.append(&mut collected);

        // Text queued by `push_text` or by `UiCommand::Text` is resolved now,
        // once the atlas is known.
        //
        // Without a pipeline there is nothing to draw it with, and without an
        // atlas there is nothing to sample: both are counted as skipped so the
        // reason the text is missing is visible.
        let can_draw_text = self.atlas.is_some()
            && self
                .gpu
                .as_ref()
                .and_then(|g| g.text_pipeline.as_ref())
                .is_some();
        if can_draw_text {
            let (_, aw, ah) = self.atlas.as_ref().expect("checked above").clone();
            for (layout, color) in &pending {
                b.text(&UiTextDraw {
                    layout,
                    color: *color,
                    atlas_width: aw as f32,
                    atlas_height: ah as f32,
                });
            }
        } else {
            b.skipped += pending.len() as u32;
        }

        // Draw order is the single definition of "what is on top", shared with hit
        // testing. Sorting here by id alone used to disagree with the click
        // order, so `z_index` was silently ignored and a click could land on a
        // panel that was visibly underneath.
        let ids: Vec<UiElementId> = super::input::draw_order(&self.elements);
        let mut elements_rendered = 0u32;
        for id in ids {
            let Some(element) = self.elements.get(&id) else {
                continue;
            };
            if !element.style.visible || !element.enabled {
                continue;
            }
            Self::emit_element(&mut b, element, &mut elements_rendered);
        }

        b.flush_all();
        let stats = UiBuildStats {
            commands: command_count,
            elements: elements_rendered,
            vertices: b.vertex_total as u32,
            indices: b.index_total as u32,
            skipped: b.skipped,
        };
        (
            UiDrawBatches {
                geometry: b.batches,
                text: b.text_batches,
            },
            stats,
        )
    }

    fn emit_command(
        b: &mut UiBuilder,
        command: &UiCommand,
        width: u32,
        height: u32,
        fonts: &HashMap<String, Box<dyn super::text::UiFont>>,
        default_font: Option<&str>,
        out_text: &mut Vec<(super::text::UiTextLayout, [f32; 4])>,
    ) {
        match command {
            UiCommand::Rectangle { x, y, width: w, height: h, color, corner_radius } => {
                if *corner_radius > 0.0 {
                    // Rounded corners need a signed-distance shader; a square
                    // quad is closer to the intent than silently ignoring the
                    // radius, and it keeps the panel readable.
                    b.skipped += 1;
                }
                b.quad(*x, *y, *w, *h, *color);
            }
            UiCommand::RectangleOutline {
                x,
                y,
                width: w,
                height: h,
                color,
                thickness,
                corner_radius: _,
            } => b.outline(*x, *y, *w, *h, *thickness, *color),
            UiCommand::Line { x1, y1, x2, y2, color, thickness } => {
                b.line(*x1, *y1, *x2, *y2, *thickness, *color)
            }
            UiCommand::ProgressBar {
                x,
                y,
                width: w,
                height: h,
                progress,
                background_color,
                fill_color,
                corner_radius: _,
            } => {
                b.quad(*x, *y, *w, *h, *background_color);
                let filled = w * progress.clamp(0.0, 1.0);
                b.quad(*x, *y, filled, *h, *fill_color);
            }
            UiCommand::CircularProgress {
                center_x,
                center_y,
                radius,
                progress,
                color,
                background_color,
                thickness,
            } => {
                // The track first, then the filled arc on top of it.
                let segments = 48usize;
                let full = std::f32::consts::TAU;
                for i in 0..segments {
                    let a0 = i as f32 / segments as f32 * full;
                    let a1 = (i + 1) as f32 / segments as f32 * full;
                    b.line(
                        center_x + a0.cos() * radius,
                        center_y + a0.sin() * radius,
                        center_x + a1.cos() * radius,
                        center_y + a1.sin() * radius,
                        *thickness,
                        *background_color,
                    );
                }
                let filled = (progress.clamp(0.0, 1.0) * segments as f32).round() as usize;
                for i in 0..filled {
                    let a0 = i as f32 / segments as f32 * full;
                    let a1 = (i + 1) as f32 / segments as f32 * full;
                    b.line(
                        center_x + a0.cos() * radius,
                        center_y + a0.sin() * radius,
                        center_x + a1.cos() * radius,
                        center_y + a1.sin() * radius,
                        *thickness,
                        *color,
                    );
                }
            }
            UiCommand::ScissorBegin { x, y, width: w, height: h } => {
                b.scissor_stack.push(b.scissor);
                b.scissor = UiBuilder::clamp_scissor((*x, *y, *w, *h), width, height);
            }
            UiCommand::ScissorEnd => {
                b.scissor = b.scissor_stack.pop().flatten();
            }
            UiCommand::GroupBegin { transform } => {
                b.transform_stack.push(b.transform);
                b.transform = *transform;
            }
            UiCommand::GroupEnd => {
                if let Some(prev) = b.transform_stack.pop() {
                    b.transform = prev;
                }
            }
            UiCommand::Text {
                content,
                x,
                y,
                size,
                color,
                font,
            } => {
                // Laid out here rather than in `push_text` so the immediate-mode
                // command path works the same way. A missing font is not an
                // error: it means the caller has not registered one yet, and the
                // text is counted as skipped instead of failing the whole frame.
                let name = font.as_deref().or(default_font);
                match name.and_then(|n| fonts.get(n)) {
                    Some(found) => {
                        let style = super::text::UiTextStyle {
                            size: *size,
                            color: *color,
                            ..Default::default()
                        };
                        let layout = super::text::UiTextLayout::layout(
                            content,
                            found.as_ref(),
                            &style,
                            [*x, *y],
                        );
                        out_text.push((layout, *color));
                    }
                    None => b.skipped += 1,
                }
            }
            UiCommand::Image { .. } => b.skipped += 1,
            // Cursor bookkeeping is the context's concern; by the time commands
            // reach the renderer the positions are already resolved.
            UiCommand::SetCursor { .. }
            | UiCommand::PushCursor
            | UiCommand::PopCursor => {}
        }
    }

    fn emit_element(
        b: &mut UiBuilder,
        element: &UiElement,
        elements_rendered: &mut u32,
    ) {
        use super::elements::{UiElementData, UiElementType};
        let (x, y) = element.position;
        let (w, h) = element.size;
        *elements_rendered += 1;

        match element.element_type {
            UiElementType::Panel | UiElementType::Container => {
                if let Some(color) = element.style.background_color {
                    let mut c = color;
                    c[3] *= element.style.opacity;
                    b.quad(x, y, w, h, c);
                }
                if let (Some(border), true) = (element.style.border_color, element.style.border_width > 0.0)
                {
                    b.outline(x, y, w, h, element.style.border_width, border);
                }
            }
            UiElementType::ProgressBar => {
                if let UiElementData::ProgressBar { progress, .. } = &element.data {
                    let bg = element.style.background_color.unwrap_or([0.15, 0.15, 0.15, 1.0]);
                    let fill = element.style.foreground_color.unwrap_or([0.0, 0.85, 0.35, 1.0]);
                    b.quad(x, y, w, h, bg);
                    b.quad(x, y, w * progress.clamp(0.0, 1.0), h, fill);
                }
            }
            UiElementType::Slider => {
                if let UiElementData::Slider { min, max, value, .. } = &element.data {
                    let span = max - min;
                    let t = if span.abs() > f32::EPSILON {
                        ((value - min) / span).clamp(0.0, 1.0)
                    } else {
                        0.0
                    };
                    let track = element.style.background_color.unwrap_or([0.15, 0.15, 0.15, 1.0]);
                    let fill = element.style.foreground_color.unwrap_or([0.0, 0.85, 0.35, 1.0]);
                    b.quad(x, y, w, h, track);
                    b.quad(x, y, w * t, h, fill);
                }
            }
            UiElementType::Checkbox => {
                if let UiElementData::Checkbox { checked, .. } = &element.data {
                    let off = element.style.background_color.unwrap_or([0.25, 0.25, 0.25, 1.0]);
                    let on = element.style.foreground_color.unwrap_or([0.0, 0.85, 0.35, 1.0]);
                    b.quad(x, y, w, h, if *checked { on } else { off });
                }
            }
            UiElementType::Button => {
                if let UiElementData::Button { text: _, normal_color, hover_color, pressed_color, .. } =
                    &element.data
                {
                    let base = if element.pressed {
                        *pressed_color
                    } else if element.hovered {
                        *hover_color
                    } else {
                        *normal_color
                    };
                    let mut c = base;
                    c[3] *= element.style.opacity;
                    b.quad(x, y, w, h, c);
                    if element.hovered || element.focused {
                        // A highlight is the only feedback a pointer-only build can
                        // give; keyboard focus reuses it via the same border.
                        let highlight = element
                            .style
                            .border_color
                            .unwrap_or([0.0, 0.85, 0.35, 1.0]);
                        b.outline(x, y, w, h, 1.0, highlight);
                    }
                }
            }
            UiElementType::InputField => {
                if let UiElementData::InputField { .. } = &element.data {
                    let bg = element.style.background_color.unwrap_or([0.12, 0.12, 0.12, 1.0]);
                    b.quad(x, y, w, h, bg);
                    // Focus is shown as an accent border, so the caret — and the
                    // text — are not the only cues.
                    if element.focused {
                        let accent = element
                            .style
                            .border_color
                            .unwrap_or([0.0, 0.85, 0.35, 1.0]);
                        b.outline(x, y, w, h, 2.0, accent);
                    }
                }
            }
            // Text and images need resources this stage does not have yet.
            UiElementType::Text | UiElementType::Image => b.skipped += 1,
            _ => {}
        }
    }

    /// Record the frame's UI into `encoder`.
    ///
    /// Returns whether any draw was recorded. `target` is the acquired swapchain
    /// image; `None` means there is nothing to draw into (headless, or the
    /// chain was rebuilt this frame) and nothing is recorded rather than a
    /// guessed-at image being used.
    pub fn record(
        &mut self,
        encoder: &mut CommandEncoder,
        target: Option<&UiRenderTarget>,
    ) -> UiResult<bool> {
        if !self.initialized {
            return Err(UiError::RendererNotInitialized);
        }

        let width = target.map_or(self.ui_config.screen_width, |t| t.width).max(1);
        let height = target.map_or(self.ui_config.screen_height, |t| t.height).max(1);

        // The projection must describe the same extent as the viewport, or the
        // UI is stretched or letterboxed.
        //
        // The swapchain extent is in physical pixels while the size a caller
        // passes to `initialize` or `resize` is often the logical window size,
        // and the two differ whenever the display scale is not 100%. The target
        // is what is actually being drawn into, so it wins, every frame. Doing
        // it here rather than trusting the caller is what keeps the two in step
        // across a DPI change, which no explicit resize call has to know about.
        if target.is_some() {
            let stale = self
                .pass
                .as_ref()
                .is_none_or(|p| p.width() != width || p.height() != height);
            if stale {
                if let Some(p) = self.pass.as_mut() {
                    p.resize(width, height);
                }
            }
        }

        let (batches, build) = self.build(width, height);

        // Flatten both streams into one pair of buffers. Scissors are re-emitted
        // at the batch boundaries below, so the split is what carries them, not
        // the upload.
        let mut vertices: Vec<UiVertex> = Vec::new();
        let mut indices: Vec<u32> = Vec::new();
        let mut geometry_spans: Vec<UiSpan> = Vec::with_capacity(batches.geometry.len());
        let mut text_spans: Vec<UiSpan> = Vec::with_capacity(batches.text.len());

        for batch in &batches.geometry {
            let first = indices.len() as u32;
            let base = vertices.len() as u32;
            for &i in &batch.indices {
                indices.push(base + i);
            }
            vertices.extend_from_slice(&batch.vertices);
            geometry_spans.push((first, batch.indices.len() as u32, batch.scissor));
        }
        // Text starts after the geometry so its indices need no rebasing against
        // a second vertex array.
        for batch in &batches.text {
            let first = indices.len() as u32;
            let base = vertices.len() as u32;
            for &i in &batch.indices {
                indices.push(base + i);
            }
            vertices.extend_from_slice(&batch.vertices);
            text_spans.push((first, batch.indices.len() as u32, batch.scissor));
        }

        let vertex_count = vertices.len();
        let index_count = indices.len();

        if index_count == 0 {
            self.stats = UiRenderStats {
                commands_processed: build.commands,
                elements_rendered: build.elements,
                skipped: build.skipped,
                ..Default::default()
            };
            // Nothing to draw is not a failure: a frame with an empty HUD is
            // legitimate, and there is nothing to submit.
            return Ok(false);
        }

        let Some(pass) = self.pass.as_ref() else {
            return Err(UiError::RendererNotInitialized);
        };
        if self.gpu.is_none() {
            return Err(UiError::RendererNotInitialized);
        }

        // Taken out before the buffer growth below, which needs `&mut self`
        // while a borrow of `self.pass` would still be alive.
        let projection = pass.projection();
        let render_pass = pass.render_pass().clone();

        // Grow before uploading: the bound buffers must cover the whole frame.
        self.upload_geometry(vertex_count, index_count)?;

        let gpu = self
            .gpu
            .as_ref()
            .ok_or(UiError::RendererNotInitialized)?;
        self.device
            .upload_buffer(&gpu.uniform_buffer, &projection)
            .map_err(|e| UiError::BackendError(format!("UI uniform upload: {e}")))?;
        self.device
            .upload_buffer(&gpu.vertex_buffer, &vertices)
            .map_err(|e| UiError::BackendError(format!("UI vertex upload: {e}")))?;
        self.device
            .upload_buffer(&gpu.index_buffer, &indices)
            .map_err(|e| UiError::BackendError(format!("UI index upload: {e}")))?;

        let mut commands: Vec<Command> = Vec::with_capacity(20 + geometry_spans.len() + text_spans.len());
        if self.config.debug_label {
            commands.push(Command::BeginDebugLabel {
                label: "ui".into(),
                color: [0.0, 0.85, 0.35, 1.0],
            });
        }

        // Without a target there is no image to draw into. The state bindings
        // are still recorded so the caller can tell the work was produced.
        if let Some(target) = target {
            let framebuffer = self
                .device
                .swapchain_image_framebuffer(
                    &render_pass,
                    target.image,
                    target.view,
                    target.format,
                    target.width,
                    target.height,
                )
                .map_err(|e| UiError::BackendError(format!("UI framebuffer: {e}")))?;

            commands.push(Command::BeginRenderPass(RenderPassBeginInfo {
                render_pass,
                framebuffer,
                render_area: Rect2D {
                    offset: Offset2D { x: 0, y: 0 },
                    extent: Extent2D { width, height },
                },
                // No clear values: the pass loads what the 3D frame left, and a
                // clear here would wipe it.
                clear_values: vec![],
            }));
        }

        commands.push(Command::SetViewport(Viewport {
            x: 0.0,
            y: 0.0,
            width: width as f32,
            height: height as f32,
            min_depth: 0.0,
            max_depth: 1.0,
        }));

        // The buffers are shared by both streams; only the pipeline and its
        // descriptor set change between them.
        let bind_buffers = |commands: &mut Vec<Command>| {
            commands.push(Command::BindVertexBuffers {
                first_binding: 0,
                buffers: vec![(gpu.vertex_buffer.clone(), 0)],
            });
            commands.push(Command::BindIndexBuffer {
                buffer: gpu.index_buffer.clone(),
                offset: 0,
                index_type: IndexType::U32,
            });
        };

        let mut draw_calls = 0u32;
        let draw_spans = |commands: &mut Vec<Command>,
                              spans: &[UiSpan],
                              previous: &mut Option<Option<(i32, i32, u32, u32)>>,
                              draw_calls: &mut u32| {
            for (first_index, count, scissor) in spans.iter().copied() {
                let scissor = scissor.unwrap_or((0, 0, width, height));
                // Only emit when it differs: a redundant `SetScissor` between
                // draws with the same rect is pure driver work.
                if *previous != Some(Some(scissor)) {
                    commands.push(Command::SetScissor(crate::types::Scissor {
                        offset: Offset2D {
                            x: scissor.0,
                            y: scissor.1,
                        },
                        extent: Extent2D {
                            width: scissor.2,
                            height: scissor.3,
                        },
                    }));
                    *previous = Some(Some(scissor));
                }
                commands.push(Command::DrawIndexed {
                    index_count: count,
                    instance_count: 1,
                    first_index,
                    vertex_offset: 0,
                    first_instance: 0,
                });
                *draw_calls += 1;
            }
        };

        // Geometry first, then text over it, so labels sit on top of panels.
        let mut previous: Option<Option<(i32, i32, u32, u32)>> = None;
        commands.push(Command::BindGraphicsPipeline(gpu.pipeline.clone()));
        commands.push(Command::BindDescriptorSets {
            pipeline: gpu.pipeline.clone(),
            first_set: 0,
            sets: vec![gpu.descriptor_set.clone()],
        });
        bind_buffers(&mut commands);
        draw_spans(&mut commands, &geometry_spans, &mut previous, &mut draw_calls);

        if !text_spans.is_empty() {
            match (
                gpu.text_pipeline.as_ref(),
                gpu.text_descriptor_set.as_ref(),
                self.atlas.as_ref(),
            ) {
                (Some(pipeline), Some(set), Some((texture, _, _))) => {
                    self.write_text_descriptors(pipeline, set, texture)?;
                    commands.push(Command::BindGraphicsPipeline(pipeline.clone()));
                    commands.push(Command::BindDescriptorSets {
                        pipeline: pipeline.clone(),
                        first_set: 0,
                        sets: vec![set.clone()],
                    });
                    bind_buffers(&mut commands);
                    // Geometry and text are separate draw runs, so the scissor
                    // must be restated rather than assumed to carry over.
                    previous = None;
                    draw_spans(&mut commands, &text_spans, &mut previous, &mut draw_calls);
                }
                _ => {
                    // Unreachable in practice: `build` only emits text batches
                    // when an atlas and a text pipeline both exist, and that is
                    // exactly what this arm tests for. Kept as a guard rather
                    // than an `unreachable!`, because the consequence of being
                    // wrong is dropped text rather than a crash.
                    //
                    // Dropped rather than drawn with the untextured pipeline,
                    // which would show every glyph as a solid block.
                    eprintln!("[ui] text geometry without a pipeline or atlas; dropped");
                }
            }
        }

        if target.is_some() {
            commands.push(Command::EndRenderPass);
        }
        if self.config.debug_label {
            commands.push(Command::EndDebugLabel);
        }

        for command in commands {
            if !encoder.record(command) {
                return Err(UiError::BackendError(
                    "the command encoder rejected a UI command: it is not recording".into(),
                ));
            }
        }

        self.stats = UiRenderStats {
            commands_processed: build.commands,
            elements_rendered: build.elements,
            vertices: vertex_count as u32,
            triangles: (index_count / 3) as u32,
            batches: (geometry_spans.len() + text_spans.len()) as u32,
            draw_calls,
            skipped: build.skipped,
        };
        self.frame_number += 1;
        Ok(true)
    }

    /// Point the text descriptor set at the current atlas and sampler.
    fn write_text_descriptors(
        &self,
        _pipeline: &GraphicsPipeline,
        set: &DescriptorSet,
        atlas: &Arc<crate::resource::Texture>,
    ) -> UiResult<()> {
        use crate::resource::sampler::{AddressMode, FilterMode};

        let sampler = self
            .device
            .create_sampler(AddressMode::ClampToEdge, FilterMode::Linear, 1.0);
        let view = atlas.create_view(crate::resource::TextureViewDesc::default());

        self.device
            .write_descriptors(
                set,
                &[
                    DescriptorWrite {
                        dst_set: set.clone(),
                        dst_binding: 0,
                        dst_array_element: 0,
                        descriptors: vec![DescriptorInfo::Buffer(
                            self.gpu
                                .as_ref()
                                .ok_or(UiError::RendererNotInitialized)?
                                .uniform_buffer
                                .clone(),
                            0,
                            UI_UNIFORM_SIZE,
                        )],
                    },
                    DescriptorWrite {
                        dst_set: set.clone(),
                        dst_binding: 1,
                        dst_array_element: 0,
                        descriptors: vec![DescriptorInfo::Texture(view, None)],
                    },
                    DescriptorWrite {
                        dst_set: set.clone(),
                        dst_binding: 2,
                        dst_array_element: 0,
                        descriptors: vec![DescriptorInfo::Sampler(sampler)],
                    },
                ],
            )
            .map_err(|e| UiError::BackendError(format!("UI text descriptor write: {e}")))?;
        Ok(())
    }

    /// Grow the vertex and index buffers if this frame does not fit.
    ///
    /// Growth is checked once, before uploading, and doubles until the frame
    /// fits. Reallocating mid-record would leave the bound buffers sized for
    /// part of the frame.
    fn upload_geometry(&mut self, vertex_count: usize, index_count: usize) -> UiResult<()> {
        let needs_vertex = vertex_count > self.gpu.as_ref().map_or(0, |g| g.vertex_capacity);
        let needs_index = index_count > self.gpu.as_ref().map_or(0, |g| g.index_capacity);
        if !needs_vertex && !needs_index {
            return Ok(());
        }

        let old = self.gpu.take().ok_or(UiError::RendererNotInitialized)?;
        let vertex_capacity = grow(old.vertex_capacity, vertex_count, MIN_VERTEX_CAPACITY);
        let index_capacity = grow(old.index_capacity, index_count, MIN_INDEX_CAPACITY);

        let (vertex_buffer, index_buffer) = self.build_buffers(vertex_capacity, index_capacity)?;
        self.gpu = Some(UiGpu {
            vertex_buffer,
            index_buffer,
            vertex_capacity,
            index_capacity,
            ..old
        });
        Ok(())
    }
}

fn grow(current: usize, needed: usize, minimum: usize) -> usize {
    let mut capacity = current.max(minimum);
    while capacity < needed {
        capacity = capacity.saturating_mul(2);
    }
    capacity
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: [f32; 4] = [1.0, 0.0, 0.0, 1.0];

    fn builder() -> UiBuilder {
        UiBuilder::new()
    }

    #[test]
    fn a_rectangle_is_two_triangles_over_four_vertices() {
        let mut b = builder();
        b.quad(10.0, 20.0, 100.0, 50.0, RED);
        b.flush();
        assert_eq!(b.batches.len(), 1);
        let batch = &b.batches[0];
        assert_eq!(batch.vertices.len(), 4);
        assert_eq!(batch.indices.len(), 6);
        assert_eq!(b.vertex_total, 4);
        assert_eq!(b.index_total, 6);
    }

    #[test]
    fn quad_corners_land_where_they_were_asked_to() {
        let mut b = builder();
        b.quad(10.0, 20.0, 100.0, 50.0, RED);
        b.flush();
        let v = &b.batches[0].vertices;
        assert_eq!(v[0].position, [10.0, 20.0]);
        assert_eq!(v[1].position, [110.0, 20.0]);
        assert_eq!(v[2].position, [110.0, 70.0]);
        assert_eq!(v[3].position, [10.0, 70.0]);
        // Colour must reach every vertex or the quad fades towards black.
        assert!(v.iter().all(|x| x.color == RED));
    }

    #[test]
    fn indices_stay_inside_the_vertex_array() {
        // A base offset that forgot to be applied reads another quad's
        // vertices, which shows up as geometry in the wrong place rather than
        // as an error.
        let mut b = builder();
        for i in 0..4 {
            b.quad(i as f32 * 10.0, 0.0, 5.0, 5.0, RED);
        }
        b.flush();
        for &i in &b.batches[0].indices {
            assert!(
                (i as usize) < b.batches[0].vertices.len(),
                "index {i} is out of range"
            );
        }
    }

    #[test]
    fn degenerate_shapes_produce_no_geometry() {
        // Zero and negative extents would otherwise emit a quad that covers no
        // pixels but still costs a draw call.
        let mut b = builder();
        b.quad(0.0, 0.0, 0.0, 10.0, RED);
        b.quad(0.0, 0.0, 10.0, 0.0, RED);
        b.quad(0.0, 0.0, -5.0, -5.0, RED);
        b.flush();
        assert!(b.batches.is_empty(), "degenerate quads were emitted");
        assert_eq!(b.vertex_total, 0);
    }

    #[test]
    fn non_finite_input_is_rejected_rather_than_poisoning_the_buffer() {
        let mut b = builder();
        b.quad(f32::NAN, 0.0, 10.0, 10.0, RED);
        b.quad(0.0, 0.0, f32::INFINITY, 10.0, RED);
        b.quad(0.0, 0.0, 10.0, 10.0, [f32::NAN, 0.0, 0.0, 1.0]);
        b.flush();
        assert!(b.batches.is_empty(), "non-finite geometry was emitted");
    }

    #[test]
    fn a_transform_group_is_applied_to_the_vertices_it_contains() {
        let mut b = builder();
        // Translate by (100, 50) and double in X.
        b.transform_stack.push(b.transform);
        b.transform = [2.0, 0.0, 0.0, 1.0, 100.0, 50.0];
        b.quad(0.0, 0.0, 10.0, 10.0, RED);
        b.transform = b.transform_stack.pop().unwrap();
        b.quad(0.0, 0.0, 10.0, 10.0, RED);
        b.flush();
        let batch = &b.batches[0];
        assert_eq!(batch.vertices.len(), 8);
        // Inside the group: x' = 2*0 + 100, y' = 50.
        assert_eq!(batch.vertices[0].position, [100.0, 50.0]);
        assert_eq!(batch.vertices[1].position, [120.0, 50.0]);
        // After the group was popped the next quad is untransformed.
        assert_eq!(batch.vertices[4].position, [0.0, 0.0]);
    }

    #[test]
    fn a_scissor_change_splits_the_batches() {
        // The split is what carries the rectangle to recording: two scissors in
        // one batch would silently apply only the last one.
        let mut b = builder();
        b.scissor = UiBuilder::clamp_scissor((0.0, 0.0, 100.0, 100.0), 800, 600);
        b.quad(0.0, 0.0, 10.0, 10.0, RED);
        b.scissor = UiBuilder::clamp_scissor((200.0, 200.0, 100.0, 100.0), 800, 600);
        b.quad(0.0, 0.0, 10.0, 10.0, RED);
        b.flush();
        assert_eq!(b.batches.len(), 2, "the scissors were not separated");
        assert_eq!(b.batches[0].scissor, Some((0, 0, 100, 100)));
        assert_eq!(b.batches[1].scissor, Some((200, 200, 100, 100)));
    }

    #[test]
    fn consecutive_draws_under_one_scissor_stay_in_a_single_batch() {
        let mut b = builder();
        let s = UiBuilder::clamp_scissor((0.0, 0.0, 50.0, 50.0), 800, 600);
        b.scissor = s;
        for _ in 0..3 {
            b.quad(0.0, 0.0, 10.0, 10.0, RED);
        }
        b.flush();
        assert_eq!(b.batches.len(), 1, "batching split unnecessarily");
        assert_eq!(b.batches[0].indices.len(), 18);
    }

    #[test]
    fn a_scissor_is_clamped_to_the_target() {
        // Partially offscreen scissors must not wrap or go negative, which would
        // make the driver reject the whole command list.
        let c = UiBuilder::clamp_scissor((-50.0, -50.0, 100.0, 100.0), 800, 600).unwrap();
        assert_eq!(c, (0, 0, 50, 50));
        let c = UiBuilder::clamp_scissor((700.0, 500.0, 500.0, 500.0), 800, 600).unwrap();
        assert_eq!(c, (700, 500, 100, 100));
    }

    #[test]
    fn a_scissor_that_covers_nothing_is_reported_as_absent() {
        // `Some` with a zero size would clamp every draw away, which looks like
        // a renderer that stopped drawing at all.
        assert!(UiBuilder::clamp_scissor((10.0, 10.0, 0.0, 10.0), 800, 600).is_none());
        assert!(UiBuilder::clamp_scissor((-100.0, -100.0, 10.0, 10.0), 800, 600).is_none());
    }

    #[test]
    fn an_outline_is_four_quads_and_stays_inside_the_shape() {
        let mut b = builder();
        b.outline(0.0, 0.0, 100.0, 50.0, 2.0, RED);
        b.flush();
        // Two horizontal bars plus two vertical ones between them.
        assert_eq!(b.batches[0].vertices.len(), 16);
        for v in &b.batches[0].vertices {
            assert!(
                v.position[0] >= 0.0 && v.position[0] <= 100.0,
                "x {} escaped the shape",
                v.position[0]
            );
        }
    }

    #[test]
    fn an_outline_thicker_than_the_shape_is_clamped() {
        // Left unclamped, the vertical bars would be drawn at x + w - t, which is
        // left of the shape once t exceeds w.
        let mut b = builder();
        b.outline(0.0, 0.0, 4.0, 4.0, 100.0, RED);
        b.flush();
        for v in &b.batches[0].vertices {
            assert!(v.position[0].is_finite() && v.position[1].is_finite());
        }
    }

    #[test]
    fn a_zero_length_line_produces_nothing() {
        let mut b = builder();
        b.line(5.0, 5.0, 5.0, 5.0, 2.0, RED);
        b.line(0.0, 0.0, 10.0, 10.0, 0.0, RED);
        b.flush();
        assert!(b.batches.is_empty(), "a degenerate line was emitted");
    }

    #[test]
    fn a_line_produces_a_quad_straddling_the_segment() {
        let mut b = builder();
        b.line(0.0, 0.0, 10.0, 0.0, 2.0, RED);
        b.flush();
        let batch = &b.batches[0];
        assert_eq!(batch.vertices.len(), 4);
        // Half the thickness on each side of y = 0.
        let ys: Vec<f32> = batch.vertices.iter().map(|v| v.position[1]).collect();
        assert!(ys.iter().all(|y| y.abs() - 1.0 < 1e-6), "ys were {ys:?}");
    }

    #[test]
    fn buffer_growth_doubles_until_the_frame_fits() {
        assert_eq!(grow(0, 1, 16), 16);
        assert_eq!(grow(16, 16, 16), 16, "an exact fit should not grow");
        assert_eq!(grow(16, 17, 16), 32);
        assert_eq!(grow(16, 1024, 16), 1024);
        // A runaway request must not overflow into a smaller capacity.
        assert!(grow(16, usize::MAX, 16) >= usize::MAX / 2);
    }

    #[test]
    fn the_vertex_stride_matches_the_declared_layout() {
        // The pipeline derives its stride and offsets from this struct, so a
        // layout change here is picked up automatically. The size is asserted
        // because the buffer upload copies `size_of` bytes.
        assert_eq!(std::mem::size_of::<UiVertex>(), 32);
        let state = UiRenderer::vertex_input_state();
        assert_eq!(state.bindings[0].stride, 32);
        assert_eq!(state.attributes.len(), 3);
        assert_eq!(
            state.attributes[0].offset,
            std::mem::offset_of!(UiVertex, position) as u32
        );
    }
}