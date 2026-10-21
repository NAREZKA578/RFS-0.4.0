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

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::command::commands::Command;
use crate::command::encoder::CommandEncoder;
use crate::command::pass::render::{
    Framebuffer, FramebufferAttachment, FramebufferDesc, RenderPass, RenderPassBeginInfo,
};
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
use crate::resource::{Buffer, Texture, TextureLayout, TextureView};
use crate::sync::barrier::TextureBarrier;
use crate::types::{
    AccessFlags, BufferUsage, ColorComponentFlags, Extent2D, Format, IndexType, Offset2D,
    PipelineStage, Rect2D, ShaderStage, Viewport,
};
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
    /// Image quads, grouped by the texture they sample. One group per texture
    /// rather than one per image, because the textured pipeline binds exactly
    /// one texture and switching it costs a descriptor write and a bind - but
    /// several images from the same atlas region are free to share.
    images: Vec<(u64, UiDrawBatch)>,
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
    /// Full-colour image pipeline. `None` until an image SPIR-V is supplied
    /// through [`UiRenderer::load_image_shader`]; images then fall back to the
    /// text (coverage) pipeline, which is what the renderer did before images
    /// had a pipeline of their own.
    image_pipeline: Option<GraphicsPipeline>,
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
    /// Image quads as they arrive, with their texture id and the scissor in
    /// force at the time. Kept flat rather than pre-grouped: images for one
    /// texture may be interleaved with others, and grouping on arrival would
    /// start a new draw every time the id changed. Grouped once, in
    /// first-appearance order, when the build is assembled.
    images: Vec<(u64, Option<(i32, i32, u32, u32)>, [UiVertex; 4])>,
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
images: Vec::new(),
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

    /// An image quad, sampled from `uv_min` to `uv_max` of `texture_id`.
    ///
    /// The same finite/positive guards as [`Self::quad`], for the same reason:
    /// a NaN position discards the primitive on some drivers and a NaN colour
    /// propagates through blending into every primitive after it. The UV rect is
    /// checked too, because an inverted or non-finite UV is not clamped away by
    /// the sampler and would sample across the whole texture.
    fn image_quad(
        &mut self,
        texture_id: u64,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        uv_min: [f32; 2],
        uv_max: [f32; 2],
        color: [f32; 4],
    ) {
        let positive = |v: f32| v.is_finite() && v > 0.0;
        if !x.is_finite() || !y.is_finite() || !positive(w) || !positive(h) {
            return;
        }
        if !color.iter().all(|c| c.is_finite()) {
            return;
        }
        if !uv_min.iter().all(|c| c.is_finite()) || !uv_max.iter().all(|c| c.is_finite()) {
            return;
        }
        // An image that samples nothing is dropped, not drawn transparent: the
        // caller asked for a picture and there is no picture, and a zero-area
        // primitive still costs a draw.
        if !positive(uv_max[0] - uv_min[0]) || !positive(uv_max[1] - uv_min[1]) {
            return;
        }

        let corners = [
            (x, y, uv_min),
            (x + w, y, [uv_max[0], uv_min[1]]),
            (x + w, y + h, uv_max),
            (x, y + h, [uv_min[0], uv_max[1]]),
        ];
        let mut quad = [UiVertex::default(); 4];
        for (slot, (cx, cy, uv)) in quad.iter_mut().zip(corners) {
            *slot = UiVertex {
                position: self.apply(cx, cy),
                tex_coords: uv,
                color,
            };
        }

        self.images.push((texture_id, self.scissor, quad));
        self.vertex_total += 4;
        self.index_total += 6;
    }

    /// Groups the queued image quads into contiguous runs and hands back the draw
    /// batches in command order.
    ///
    /// Deliberately *runs*, not "every quad of this texture". Merging non-adjacent
    /// images of one texture into a single group would draw them together and
    /// push whatever was between them later, which silently changes the stacking
    /// of overlapping images from different textures. A few extra descriptor
    /// binds is a fair price for drawing in the order the caller asked for.
    fn flush_images(&mut self) -> Vec<(u64, UiDrawBatch)> {
        let pending = std::mem::take(&mut self.images);
        let mut groups: Vec<(u64, UiDrawBatch)> = Vec::new();
        for (texture_id, scissor, quad) in pending {
            let extends_last = groups
                .last()
                .is_some_and(|(id, _)| *id == texture_id)
                && groups.last().is_some_and(|(_, b)| b.scissor == scissor);
            if !extends_last {
                groups.push((
                    texture_id,
                    UiDrawBatch {
                        vertices: Vec::new(),
                        indices: Vec::new(),
                        scissor,
                    },
                ));
            }
            let (_, batch) = groups.last_mut().expect("a group was just ensured");
            let base = batch.vertices.len() as u32;
            batch.vertices.extend_from_slice(&quad);
            batch
                .indices
                .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        }
        groups
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
    /// One framebuffer per swapchain image view, keyed by the view's handle.
    ///
    /// Not one framebuffer reused for all of them: a swapchain cycles through
    /// several images, so a single cached framebuffer would send every frame
    /// into whichever image happened to be acquired first, and the rest would
    /// be presented never having been written.
    presentation_framebuffers: HashMap<u64, Framebuffer>,
    command_queue: Vec<UiCommand>,
    elements: HashMap<UiElementId, UiElement>,
    texture_cache: HashMap<u64, Arc<crate::resource::Texture>>,
    /// One descriptor set per sampled texture, created on first use.
    image_descriptor_sets: HashMap<u64, DescriptorSet>,
    /// Textures already moved from the transfer layout to the sampled layout.
    ///
    /// `Texture` does not track its own current layout, so the renderer remembers
    /// which registered images it has already transitioned. The sampler
    /// descriptor always declares `ShaderReadOnlyOptimal`, while an uploaded or
    /// cleared image arrives in `TransferDstOptimal`; the first sample moves it
    /// across. Doing that more than once would name `TransferDstOptimal` as the
    /// old layout of an image already sitting in `ShaderReadOnlyOptimal`, which is
    /// a validation error rather than a harmless repeat.
    transitioned_images: HashSet<u64>,
    /// Fonts available to `UiCommand::Text`, by name.
    fonts: HashMap<String, Box<dyn super::text::UiFont>>,
    /// Name of the font used when a command does not name one.
    default_font: Option<String>,
    /// The glyph atlas, with its size in texels.
    atlas: Option<(Arc<crate::resource::Texture>, u32, u32)>,
    /// Text laid out ahead of recording, waiting to become geometry.
    pending_text: Vec<(super::text::UiTextLayout, [f32; 4])>,
    /// Cursor and press state, resolved once per frame from the queued events.
    ///
    /// This field is the whole missing edge in the input chain. `input::update`
    /// had been written, unit-tested and correct since the UI was first laid
    /// out; `UiRenderer` drew `element.hovered` and `element.pressed` when
    /// painting a button, but nothing ever set them, because the renderer held
    /// no input state and no caller built a `UiInputState`. So a button could
    /// never show a hover, an input field could never show a caret, and task #9
    /// "input" was reported done while being a subsystem with no entrance.
    input: super::input::UiInputState,
    /// Fallback colours for elements that do not specify their own.
    ///
    /// Was not wired at all: `theme.rs` had a complete palette and the renderer
    /// painted hard-coded literals instead - the same mistake as filling an open
    /// sea with asteroids, at a smaller scale. A theme is also the only thing
    /// that can answer "what does a disabled button look like" in one place
    /// instead of per call site.
    theme: super::theme::UiTheme,
    /// Named themes the renderer can switch between.
    ///
    /// Held next to the active theme so `select_theme` has somewhere to look:
    /// a caller that wants "light mode" should not have to construct a whole
    /// `UiTheme` to get one, and the palette is the part most likely to be
    /// changed by whoever asks for it.
    themes: super::theme::ThemeManager,
    /// Events waiting to be applied on the next record.
    pending_input: Vec<super::input::UiInputEvent>,
    /// What this frame's input resolved to, for a caller that wants clicks and
    /// focus changes rather than just the painted result.
    interaction: super::input::UiInteraction,
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
            presentation_framebuffers: HashMap::new(),
            command_queue: Vec::new(),
            elements: HashMap::new(),
            texture_cache: HashMap::new(),
    image_descriptor_sets: HashMap::new(),
            transitioned_images: HashSet::new(),
            fonts: HashMap::new(),
            default_font: None,
            atlas: None,
            pending_text: Vec::new(),
        input: Default::default(),
        theme: Default::default(),
        themes: Default::default(),
        pending_input: Vec::new(),
        interaction: Default::default(),
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
        let pass = UiRenderPass::new(&self.device, format, width, height)?;
        self.initialize_with_pass(pass, format, width, height, geometry_spirv, text_spirv)
    }

    /// Like [`Self::initialize`], but the pass ends in a layout the pixels can
    /// be read back from instead of `PresentSrc`.
    ///
    /// This exists so "did the UI actually draw" can be answered by reading the
    /// image, rather than inferred from a recorded draw call and a quiet
    /// validation layer. A draw that records perfectly and rasterises nothing
    /// looks identical from the outside to one that worked.
    pub fn initialize_offscreen(
        &mut self,
        format: Format,
        width: u32,
        height: u32,
        geometry_spirv: &[u8],
        text_spirv: Option<&[u8]>,
    ) -> UiResult<()> {
        let pass = UiRenderPass::new_offscreen(&self.device, format, width, height)?;
        self.initialize_with_pass(pass, format, width, height, geometry_spirv, text_spirv)
    }

    /// Build the full-colour image pipeline from `spirv`.
    ///
    /// Kept separate from [`Self::initialize_offscreen`] so callers that only
    /// draw panels, glyphs and the atlas are unaffected: until this is called,
    /// images are drawn with the text (coverage) pipeline. The glyph pipeline
    /// samples only the red channel as coverage, so an image whose red is zero
    /// is invisible through it — that is the whole reason images need their own
    /// fragment stage.
    ///
    /// The pipeline reuses the textured descriptor layout the glyph pipeline was
    /// built with, so the per-image descriptor sets (allocated from that same
    /// layout) are compatible with it and need no rebuilding.
    pub fn load_image_shader(&mut self, spirv: &[u8]) -> UiResult<()> {
        self.shaders.load_image(spirv)?;
        let layout = self
            .gpu
            .as_ref()
            .and_then(|g| g.text_descriptor_layout.clone())
            .ok_or_else(|| {
                UiError::BackendError(
                    "the image pipeline needs the textured descriptor layout; supply a text \
                     shader to `initialize_offscreen` first"
                        .into(),
                )
            })?;
        let pass = self.pass.as_ref().ok_or(UiError::RendererNotInitialized)?;
        let pipeline = self.build_textured_pipeline(UiShaderType::Image, &layout, pass)?;
        if let Some(gpu) = self.gpu.as_mut() {
            gpu.image_pipeline = Some(pipeline);
        }
        Ok(())
    }

    fn initialize_with_pass(
        &mut self,
        pass: UiRenderPass,
        _format: Format,
        width: u32,
        height: u32,
        geometry_spirv: &[u8],
        text_spirv: Option<&[u8]>,
    ) -> UiResult<()> {
        self.shaders.load_geometry(geometry_spirv)?;

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
                let pipeline = self.build_textured_pipeline(UiShaderType::Text, &layout, &pass)?;
                text_descriptor_layout = Some(layout);
                text_descriptor_set = Some(set);
                text_pipeline = Some(pipeline);
            }
        }

        let vertex_capacity = self.config.initial_vertex_capacity.max(MIN_VERTEX_CAPACITY);
        let index_capacity = self.config.initial_index_capacity.max(MIN_INDEX_CAPACITY);
        let (vertex_buffer, index_buffer) = self.build_buffers(vertex_capacity, index_capacity)?;

        self.pass = Some(pass);
        // A framebuffer is built against one render pass. Keeping them across a
        // rebuild would leave framebuffers pointing at the previous pass, which
        // the driver rejects when the pass is begun with a mismatched one.
        self.presentation_framebuffers.clear();
        self.gpu = Some(UiGpu {
            pipeline,
            descriptor_layout,
            descriptor_set,
            text_pipeline,
            text_descriptor_layout,
            text_descriptor_set,
            image_pipeline: None,
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
                    // Must be explicit. `ColorComponentFlags` is a `bitflags`
                    // type that derives `Default`, and for bitflags `Default` is
                    // the *empty* set - so `..Default::default()` here silently
                    // gave this attachment a write mask of zero. The draw then
                    // blended correctly into nothing: the framebuffer kept the
                    // clear colour, no validation layer complains (a zero write
                    // mask is legal), and any shader produced the same empty
                    // frame.
                    color_write_mask: ColorComponentFlags::R
                        | ColorComponentFlags::G
                        | ColorComponentFlags::B
                        | ColorComponentFlags::A,
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

    /// Build a pipeline that samples binding 1 with binding 2's sampler.
    ///
    /// Glyphs and images share the vertex layout and the descriptor layout; only
    /// the fragment stage differs, so the two pipelines are built from the same
    /// code with a different `shader_type`.
    fn build_textured_pipeline(
        &self,
        shader_type: UiShaderType,
        layout: &DescriptorSetLayout,
        pass: &UiRenderPass,
    ) -> UiResult<GraphicsPipeline> {
        let module = self.shaders.module(shader_type)?;
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
        let name = shader_type.source_filename();
        let pipeline = self
            .device
            .create_gpu_graphics_pipeline(&desc, pass.render_pass(), &[layout])
            .map_err(|e| UiError::BackendError(format!("UI {name} pipeline: {e}")))?;
        if !pipeline.has_gpu_backing() {
            return Err(UiError::BackendError(format!(
                "the UI {name} graphics pipeline was not created on the device"
            )));
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
        // A newly registered image has not been moved to the sampled layout yet,
        // even if an earlier texture carried the same id.
        self.transitioned_images.remove(&id);
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

    /// Rasterises `font`'s atlas to a texture and registers it, as `name`.
    ///
    /// Why this is one call: `register_font` and `set_atlas` are two halves of
    /// one fact, and in practice only the first was ever reachable. A renderer
    /// with a font and no atlas reports every string as skipped, and a renderer
    /// with an atlas and no font never looks at it — both states are silent, and
    /// nothing in the tree put them together. The bitmap font owns its own atlas
    /// data, so the pairing is not optional here; it is the point.
    ///
    /// Text drawn before this call is still counted as skipped, and `push_text`
    /// starts working after it. Replacing this with a TTF font means passing
    /// that font's atlas instead, and nothing above changes.
    ///
    /// `R8` rather than a colour format: glyph coverage is single-channel, and
    /// the fragment shader samples it as alpha, so three quarters of the atlas
    /// would be replicated bytes.
    pub fn install_bitmap_font(&mut self, name: &str) -> UiResult<()> {
        let font = super::bitmap_font::UiBitmapFont::new();
        let (w, h) = (font.atlas_width(), font.atlas_height());

        let texture = self.device.create_texture(
            w,
            h,
            1,
            crate::types::Format::R8_UNORM,
            crate::types::TextureUsage::SAMPLED | crate::types::TextureUsage::TRANSFER_DST,
            1,
        );
        self.device
            .upload_texture(&texture, font.atlas_pixels())
            .map_err(|e| UiError::BackendError(format!("UI atlas upload: {e}")))?;

        self.register_font(name, Box::new(font));
        self.set_atlas(Arc::new(texture), w, h);
        Ok(())
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
                &self.texture_cache,
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
        // Text carried by retained elements. Collected here and resolved below,
        // after the element loop, through the same `UiBuilder::text` the
        // immediate path uses - one code path means one set of bugs rather than
        // two that agree today and drift tomorrow.
        let mut retained_text: Vec<(super::text::UiTextLayout, [f32; 4])> = Vec::new();
        let rects = super::layout::resolve_rects(&self.elements, (0.0, 0.0, width as f32, height as f32));
        for id in ids {
            let Some(element) = self.elements.get(&id) else {
                continue;
            };
            if !element.style.visible || !element.enabled {
                continue;
            }
            Self::emit_element(
                &mut b,
                element,
                &mut elements_rendered,
                &self.fonts,
                self.default_font.as_deref(),
                &self.texture_cache,
                &self.theme,
                &rects,
                &mut retained_text,
            );
        }

        // Retained text is resolved after the immediate text, in draw order, so
        // the two streams merge into one set of glyph batches.
        if can_draw_text && !retained_text.is_empty() {
            let (_, aw, ah) = self.atlas.as_ref().expect("checked above").clone();
            for (layout, color) in &retained_text {
                b.text(&UiTextDraw {
                    layout,
                    color: *color,
                    atlas_width: aw as f32,
                    atlas_height: ah as f32,
                });
            }
        } else if !can_draw_text {
            b.skipped += retained_text.len() as u32;
        }

        b.flush_all();
        let stats = UiBuildStats {
            commands: command_count,
            elements: elements_rendered,
            vertices: b.vertex_total as u32,
            indices: b.index_total as u32,
            skipped: b.skipped,
        };
// Collected before the other fields are moved out of the builder.
        let images = b.flush_images();
        (
            UiDrawBatches {
                geometry: b.batches,
                text: b.text_batches,
                images,
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
        textures: &HashMap<u64, Arc<crate::resource::Texture>>,
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
            UiCommand::Image {
                texture_id,
                x,
                y,
                width,
                height,
                uv_min,
                uv_max,
                color,
            } => {
                if textures.contains_key(texture_id) {
                    b.image_quad(*texture_id, *x, *y, *width, *height, *uv_min, *uv_max, *color);
                } else {
                    // Counted, not drawn. An unknown id is a caller mistake, and
                    // drawing an untextured quad in its place would show a solid
                    // rectangle that looks like a loaded image that is merely the
                    // wrong one.
                    b.skipped += 1;
                }
            }
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
        fonts: &HashMap<String, Box<dyn super::text::UiFont>>,
        default_font: Option<&str>,
        textures: &HashMap<u64, Arc<crate::resource::Texture>>,
        theme: &super::theme::UiTheme,
        rects: &HashMap<super::elements::UiElementId, super::layout::UiRect>,
        out_text: &mut Vec<(super::text::UiTextLayout, [f32; 4])>,
    ) {
        use crate::ui::elements::{UiElementData, UiElementType};
        // Anchor and pivot are resolved once per frame in `build`, from the
        // same map the hit test uses. Reading `position` directly here is what
        // made both of those fields do nothing.
        let (x, y, w, h) = rects
            .get(&element.id)
            .copied()
            .unwrap_or((element.position.0, element.position.1, element.size.0, element.size.1));
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
                    let bg = element
                        .style
                        .background_color
                        .unwrap_or_else(|| theme.surface());
                    let fill = element
                        .style
                        .foreground_color
                        .unwrap_or_else(|| theme.get_progress_color(*progress));
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
                    let track = element
                        .style
                        .background_color
                        .unwrap_or_else(|| theme.surface());
                    let fill = element
                        .style
                        .foreground_color
                        .unwrap_or_else(|| theme.get_progress_color(t));
                    b.quad(x, y, w, h, track);
                    b.quad(x, y, w * t, h, fill);
                }
            }
            UiElementType::Checkbox => {
                if let UiElementData::Checkbox { checked, .. } = &element.data {
                    let off = element
                        .style
                        .background_color
                        .unwrap_or_else(|| theme.get_button_color(super::theme::ButtonState::Normal));
                    let on = element
                        .style
                        .foreground_color
                        .unwrap_or_else(|| theme.get_button_color(super::theme::ButtonState::Hover));
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
                            .unwrap_or_else(|| theme.accent());
                        b.outline(x, y, w, h, 1.0, highlight);
                    }
                }
            }
            UiElementType::InputField => {
                if let UiElementData::InputField { .. } = &element.data {
                    let style_for_input = theme.get_input_style(if element.focused {
                        super::theme::InputState::Focused
                    } else {
                        super::theme::InputState::Normal
                    });
                    let bg = element
                        .style
                        .background_color
                        .unwrap_or(style_for_input.background);
                    b.quad(x, y, w, h, bg);
                    // Focus is shown as an accent border, so the caret — and the
                    // text — are not the only cues.
                    if element.focused {
                        let accent = element
                            .style
                            .border_color
                            .unwrap_or_else(|| theme.accent());
                        b.outline(x, y, w, h, 2.0, accent);
                    }
                }
            }
            UiElementType::RadioButton => {
                if let UiElementData::RadioButton { checked, .. } = &element.data {
                    // A radio button is a checkbox with a mutual-exclusion contract,
                    // so it gets the checkbox's two-state fill. Drawn as a square
                    // rather than a circle: this pipeline has no round primitive,
                    // and a square that fills on selection is unambiguous next to a
                    // checkbox in the same list.
                    let off = element
                        .style
                        .background_color
                        .unwrap_or_else(|| theme.get_button_color(super::theme::ButtonState::Normal));
                    let on = element
                        .style
                        .foreground_color
                        .unwrap_or_else(|| theme.accent());
                    b.quad(x, y, w, h, if *checked { on } else { off });
                    if let Some(border) = element.style.border_color {
                        b.outline(x, y, w, h, element.style.border_width.max(1.0), border);
                    }
                }
            }
            UiElementType::ScrollContainer => {
                if let UiElementData::ScrollContainer {
                    scroll_x,
                    scroll_y,
                    scroll_width,
                    scroll_height,
                } = &element.data
                {
                    // The viewport, then a thumb sized by the visible fraction and
                    // positioned by the scroll offset. Both are computed from the
                    // declared content size rather than from children, because the
                    // children have already been appended as siblings and are not
                    // available here.
                    b.quad(
                        x,
                        y,
                        w,
                        h,
                        element
                            .style
                            .background_color
.unwrap_or_else(|| theme.muted()),
                      );
                      let thumb = element
                          .style
                          .foreground_color
                          .unwrap_or_else(|| theme.border());
                    let fraction = |visible: f32, content: f32| -> f32 {
                        if content > 0.0 && content.is_finite() {
                            (visible / content).clamp(0.0, 1.0)
                        } else {
                            1.0
                        }
                    };
                    // Vertical bar.
                    if *scroll_height > h && scroll_height.is_finite() && *scroll_height > 0.0 {
                        let frac = fraction(h, *scroll_height);
                        let travel = h * (1.0 - frac);
                        let t = if (scroll_height - h).abs() > f32::EPSILON {
                            (scroll_y / (scroll_height - h)).clamp(0.0, 1.0)
                        } else {
                            0.0
                        };
                        b.quad(x + w, y + travel * t, 4.0, (h * frac).max(4.0), thumb);
                    }
                    // Horizontal bar, only when there is something to scroll.
                    if *scroll_width > w && scroll_width.is_finite() && *scroll_width > 0.0 {
                        let frac = fraction(w, *scroll_width);
                        let travel = w * (1.0 - frac);
                        let t = if (scroll_width - w).abs() > f32::EPSILON {
                            (scroll_x / (scroll_width - w)).clamp(0.0, 1.0)
                        } else {
                            0.0
                        };
                        b.quad(x + travel * t, y + h, (w * frac).max(4.0), 4.0, thumb);
                    }
                }
            }
            UiElementType::Dropdown => {
                if let UiElementData::Dropdown { selected_index, items, is_open } = &element.data
                {
                    let bg = element
                        .style
                        .background_color
                        .unwrap_or_else(|| theme.surface());
                    let accent = element
                        .style
                        .foreground_color
                        .unwrap_or_else(|| theme.accent());
                    b.quad(x, y, w, h, bg);
                    // The closed field shows the selection as a filled bar. The row
                    // labels themselves need a font atlas and are drawn by the text
                    // pass, so this is the selection, not the text.
                    if let Some(index) = selected_index {
                        if *index < items.len() {
                            b.quad(x, y + h * 0.5, w, h * 0.5, accent);
                        }
                    }
                    // An open dropdown lists one row per item beneath the field.
                    // Rows are drawn as separators; the labels are the text pass's
                    // job, so the row geometry is what is available here.
                    if *is_open {
                        let row = h * 0.75;
                        for (i, _) in items.iter().enumerate() {
                            let top = y + h + row * i as f32;
                            b.quad(x, top, w, row, bg);
                            b.line(x, top + row, x + w, top + row, 1.0, accent);
                            if selected_index == &Some(i) {
                                b.quad(x, top, 3.0, row, accent);
                            }
                        }
                    }
                }
            }
            UiElementType::ListView => {
                if let UiElementData::ListView { items, selected_indices } = &element.data {
                    let bg = element
                        .style
                        .background_color
                        .unwrap_or_else(|| theme.muted());
                    let accent = element
                        .style
                        .foreground_color
                        .unwrap_or_else(|| theme.accent());
                    b.quad(x, y, w, h, bg);
                    // Even split: a list that has not been measured is better drawn
                    // than not drawn, and the row count comes from the data.
                    let count = items.len().max(1);
                    let row = h / count as f32;
                    for (i, _) in items.iter().enumerate() {
                        let top = y + row * i as f32;
                        if selected_indices.contains(&i) {
                            b.quad(x, top, w, row, accent);
                        } else if i > 0 {
                            b.line(x, top, x + w, top, 1.0, [0.3, 0.3, 0.35, 1.0]);
                        }
                    }
                }
            }
            UiElementType::GridView => {
                if let UiElementData::GridView { columns, items, selected_indices } = &element.data {
                    let bg = element
                        .style
                        .background_color
                        .unwrap_or_else(|| theme.muted());
                    let accent = element
                        .style
                        .foreground_color
                        .unwrap_or_else(|| theme.accent());
                    b.quad(x, y, w, h, bg);
                    // Zero columns would divide by zero; one column is the safe
                    // reading of a grid whose column count was never set.
                    let cols = (*columns).max(1);
                    let cell_w = w / cols as f32;
                    for (i, _) in items.iter().enumerate() {
                        let (cx, cy) = (i % cols, i / cols);
                        let left = x + cell_w * cx as f32;
                        let top = y + cell_w * cy as f32;
                        if selected_indices.contains(&i) {
                            b.quad(left, top, cell_w, cell_w, accent);
                        } else {
                            b.line(left, top, left + cell_w, top, 1.0, [0.3, 0.3, 0.35, 1.0]);
                            b.line(left, top, left, top + cell_w, 1.0, [0.3, 0.3, 0.35, 1.0]);
                        }
                    }
                }
            }
            UiElementType::Custom { .. } => {
                // `UiElementData::Custom` holds the *name* of a render function,
                // not the function, and this pipeline has no way to call one. So
                // there is nothing honest to draw here: no placeholder box, because
                // a box would be mistaken for the element's own output, and no
                // `skipped` bump, because that counter means "needs a resource
                // this stage lacks" and would send a caller looking for a missing
                // atlas instead of a missing callback contract.
            }
            UiElementType::Text => {
                // Was counted as skipped because no font and no atlas existed
                // anywhere in the tree. Both do now - `install_bitmap_font`
                // rasterises the atlas - so the reason has gone and counting it
                // would hide a real failure behind a stale excuse.
                if let UiElementData::Text { content } = &element.data {
                    let name = default_font;
                    match name.and_then(|n| fonts.get(n)) {
                        Some(font) => {
                            let color = element
                                .style
                                .foreground_color
                                .or(element.style.background_color)
                                .unwrap_or([1.0, 1.0, 1.0, 1.0]);
                            let style = super::text::UiTextStyle {
                                size: element.style.font_size.max(1.0),
                                color,
                                ..Default::default()
                            };
                            let layout = super::text::UiTextLayout::layout(
                                content,
                                font.as_ref(),
                                &style,
                                [x, y],
                            );
                            out_text.push((layout, color));
                        }
                        // No font registered: skipped, because this *is* the
                        // resource this stage lacks, unlike the old claim.
                        None => b.skipped += 1,
                    }
                }
            }
            UiElementType::Image => {
                // The textured pipeline binds one texture per descriptor set, and
                // that machinery exists now - `image_descriptor_set` and
                // `write_image_descriptors`. A retained image takes the same path
                // as `UiCommand::Image`, so the two cannot drift apart.
                if let UiElementData::Image { texture_id, uv_min, uv_max } = &element.data {
                    if textures.contains_key(texture_id) {
                        let color = element.style.foreground_color.unwrap_or([1.0, 1.0, 1.0, 1.0]);
                        b.image_quad(*texture_id, x, y, w, h, *uv_min, *uv_max, color);
                    } else {
                        b.skipped += 1;
                    }
                }
            }
        }
    }

    /// The fallback theme.
    pub fn theme(&self) -> &super::theme::UiTheme {
        &self.theme
    }

    /// Replaces the fallback theme.
    pub fn set_theme(&mut self, theme: super::theme::UiTheme) {
        self.theme = theme;
    }

    /// The names of the themes that can be selected.
    pub fn theme_names(&self) -> Vec<&String> {
        self.themes.theme_names()
    }

    /// Adds or replaces a named theme.
    pub fn add_theme(&mut self, name: String, theme: super::theme::UiTheme) {
        self.themes.add_theme(name, theme);
    }

    /// Selects a named theme and makes it the one elements are painted with.
    ///
    /// Reports whether the name was known, so a caller can fall back rather
    /// than silently keep the current theme and call it a settings change.
    pub fn select_theme(&mut self, name: &str) -> bool {
        if self.themes.set_theme(name).is_none() {
            return false;
        }
        self.theme = self.themes.current().clone();
        true
    }

    /// Queues an input event for the next frame.
    ///
    /// Applied at the start of [`Self::record`] rather than immediately, so a
    /// press and a release inside one frame resolve against one set of element
    /// flags - doing it on arrival is how an element ends up stuck pressed.
    pub fn push_input_event(&mut self, event: super::input::UiInputEvent) {
        self.pending_input.push(event);
    }

    /// The retained element tree.
    ///
    /// Read-only on purpose. `submit_element` and `remove_element` are the way
    /// in: letting a caller hold `&mut` to an element would let it change
    /// `hovered` or `pressed`, which are resolved once per frame from the input
    /// state and would then disagree with what was painted.
    pub fn elements(&self) -> &std::collections::HashMap<UiElementId, UiElement> {
        &self.elements
    }

    /// The interaction this frame's input produced: what was clicked, whether
    /// focus moved, what was typed.
    ///
    /// Empty until the first frame is recorded.
    pub fn interaction(&self) -> super::input::UiInteraction {
        self.interaction.clone()
    }

    /// The render pass this renderer records against.
    ///
    /// A caller that builds its own framebuffer — the offscreen verification
    /// path does — must build it against **this** pass. A framebuffer made from a
    /// different, even identically-described, render pass is not compatible, and
    /// pairing the two is a mistake that does not necessarily announce itself.
    pub fn render_pass(&self) -> Option<RenderPass> {
        self.pass.as_ref().map(|p| p.render_pass().clone())
    }

    /// TEMPORARY DIAGNOSTIC: the bytes the GPU would actually read for the
    /// projection and the vertex buffer.
    ///
    /// Added because the offscreen harness reports a correct recording
    /// (4 vertices, 2 triangles, 1 draw call, `skipped = 0`) and still an image
    /// with zero lit pixels. That is only possible if either the data never
    /// reached the GPU or it reached and did not rasterise, and reading those are
    /// the two possibilities — so read them instead of choosing between them by
    /// argument.
    pub fn debug_visible_bytes(&self) -> Option<(Vec<u8>, Vec<u8>)> {
        let gpu = self.gpu.as_ref()?;
        let uniform = self.device.buffer_data(&gpu.uniform_buffer)?.to_vec();
        let vertices = self.device.buffer_data(&gpu.vertex_buffer)?.to_vec();
        Some((uniform, vertices))
    }

    /// Record the frame's UI into `encoder`.
    ///
    /// Returns whether any draw was recorded.
    ///
    /// # Why this cannot honour a target yet
    ///
    /// `target` identifies the image by raw `u64` handles, and a framebuffer
    /// cannot be built from those: `TextureView` owns the `Texture` it came
    /// from, so the RHI keeps no way to rebuild one from a handle. This used to
    /// accept the target, ignore it, and return `Ok(true)` - so the pass printed
    /// "first frame recorded" every launch while recording **no render pass at
    /// all**, and the validator duly complained twice per frame. A caller was
    /// told the UI drew when nothing was submitted.
    ///
    /// So the two outcomes are now kept apart instead of being one: no target
    /// means nothing was drawn and says so, and a target that cannot be
    /// honoured is an error naming what is missing. Carrying the typed
    /// `TextureView` through `RenderContext` is what turns the second into the
    /// first - see `record_into_framebuffer` for the working shape.
    pub fn record(
        &mut self,
        _encoder: &mut CommandEncoder,
        target: Option<&UiRenderTarget>,
    ) -> UiResult<bool> {
        let Some(target) = target else {
            // Nothing to draw into: headless, or the chain was rebuilt this
            // frame. Recording against a guessed-at image is worse than drawing
            // nothing.
            return Ok(false);
        };

        Err(UiError::BackendError(format!(
            "UI cannot record into swapchain image {} (view {}, {}x{}): a framebuffer needs a \
             typed TextureView, and RenderContext passes raw handles. Pass the typed view \
             through and build the framebuffer as `record_into_framebuffer` does.",
            target.image, target.view, target.width, target.height
        )))
    }

    /// Record this frame's UI into a swapchain image, given its typed view.
    ///
    /// This is the working presentation path. [`Self::record`] cannot honour a
    /// target because a framebuffer needs the typed view, and an offscreen target
    /// is the only kind whose pixels can be read back, so a caller that holds the
    /// typed view passes it here instead of a raw handle.
    ///
    /// The framebuffer is cached per image view, and one is built the first time
    /// each image is drawn into. Reusing a single framebuffer across the chain
    /// would send every frame into whichever image was acquired first; rebuilding
    /// one every frame would allocate a Vulkan object per frame for nothing.
    pub fn record_into_view(
        &mut self,
        encoder: &mut CommandEncoder,
        view: &TextureView,
        width: u32,
        height: u32,
    ) -> UiResult<bool> {
        let key = view.backend().map(|b| b.handle).unwrap_or(0);
        if !self.presentation_framebuffers.contains_key(&key) {
            let render_pass = self
                .render_pass()
                .ok_or(UiError::RendererNotInitialized)?;
            let framebuffer = self
                .device
                .create_gpu_framebuffer(&FramebufferDesc {
                    render_pass,
                    attachments: vec![FramebufferAttachment {
                        texture_view: view.clone(),
                        layer: 0,
                        mip_level: 0,
                    }],
                    width,
                    height,
                    layers: 1,
                })
                .map_err(|e| UiError::BackendError(format!("UI framebuffer: {e}")))?;
            self.presentation_framebuffers.insert(key, framebuffer);
        }

        // Taken out and put back rather than borrowed: `record_into_framebuffer`
        // needs `&mut self`, and the framebuffer lives in `self`.
        let framebuffer = self
            .presentation_framebuffers
            .remove(&key)
            .ok_or(UiError::RendererNotInitialized)?;
        let recorded = self.record_into_framebuffer(encoder, &framebuffer, width, height);
        self.presentation_framebuffers.insert(key, framebuffer);
        recorded
    }

    /// Record the frame's UI into a caller-built framebuffer.
    ///
    /// # Why this exists
    ///
    /// [`Self::record`] identifies its target by raw image/view handles, which
    /// only a swapchain can supply — the RHI keeps no handle accessor on
    /// `Texture` or `TextureView`. An offscreen target therefore cannot be
    /// described to `record` at all, and offscreen is the only way to *verify*
    /// that the UI drew anything: a presentation pass ends the frame in
    /// `PresentSrc`, and `download_texture` cannot read that layout.
    ///
    /// Without this method, "did the UI render" can only be inferred from a
    /// recorded draw call and a silent validation layer — which is exactly the
    /// evidence that let a correctly-recorded, never-rasterised draw pass for a
    /// working one. Mirroring `GeometryPass::set_framebuffer`, which already
    /// serves that purpose for geometry, keeps the two passes honest in the same
    /// way.
    ///
    /// The framebuffer must have been built against this renderer's render pass,
    /// which [`Self::initialize_offscreen`] arranges.
    pub fn record_into_framebuffer(
        &mut self,
        encoder: &mut CommandEncoder,
        framebuffer: &Framebuffer,
        width: u32,
        height: u32,
    ) -> UiResult<bool> {
        self.record_internal(encoder, Some(framebuffer), Some((width, height)))
    }

    fn record_internal(
        &mut self,
        encoder: &mut CommandEncoder,
        framebuffer: Option<&Framebuffer>,
        extent: Option<(u32, u32)>,
    ) -> UiResult<bool> {
        let (width, height) = extent.unwrap_or((
            self.ui_config.screen_width,
            self.ui_config.screen_height,
        ));
        let target: Option<&UiRenderTarget> = None;

        if !self.initialized {
            return Err(UiError::RendererNotInitialized);
        }

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

        // Input first, so the flags the pass paints from describe this frame's input.
        // Applying it here rather than on arrival is the documented contract of
        // `input::update`: a press and a release inside one frame must not leave
        // an element stuck in a state the final cursor position contradicts.
        let events = std::mem::take(&mut self.pending_input);
        self.interaction = super::input::update(
            &mut self.elements,
            &mut self.input,
            &events,
            (0.0, 0.0, width as f32, height as f32),
        );

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

        // Images come last, one group per texture, each with its own descriptor
        // set. The texture id travels with the span because the draw has to bind
        // the set that samples it.
        let mut image_spans: Vec<(u64, UiSpan)> = Vec::with_capacity(batches.images.len());
        for (texture_id, batch) in &batches.images {
            if !self.texture_cache.contains_key(texture_id) {
                continue;
            }
            let first = indices.len() as u32;
            let base = vertices.len() as u32;
            for &i in &batch.indices {
                indices.push(base + i);
            }
            vertices.extend_from_slice(&batch.vertices);
            image_spans.push((
                *texture_id,
                (first, batch.indices.len() as u32, batch.scissor),
            ));
        }
        image_spans.retain(|(_, span)| span.1 > 0);

        // The sampler descriptors declare `ShaderReadOnlyOptimal`, but a texture
        // is left in the transfer layout its upload wrote through. Move each one
        // the first time it is sampled, and record that it has been moved so a
        // later frame does not name `TransferDstOptimal` as the old layout of an
        // image that is already readable. Collected here, before the render pass
        // begins and before `gpu` is borrowed: a sampled texture is not the pass
        // attachment, so its layout change belongs outside the pass.
        let mut sample_transitions: Vec<Texture> = Vec::new();
        {
            let mut already_named: HashSet<u64> = HashSet::new();
            for &(texture_id, _) in &image_spans {
                if !already_named.insert(texture_id)
                    || self.transitioned_images.contains(&texture_id)
                {
                    continue;
                }
                if let Some(texture) = self.texture_cache.get(&texture_id) {
                    // A CPU-only texture has no image to move.
                    if texture.backend().is_some() {
                        sample_transitions.push((**texture).clone());
                        self.transitioned_images.insert(texture_id);
                    }
                }
            }
        }

        // Resolve every group's descriptor set here, before `gpu` is borrowed for
        // the recording below. Doing it later would need `&mut self` while the
        // buffers are borrowed immutably by the closures that emit the binds.
        let mut image_groups: Vec<(DescriptorSet, Vec<UiSpan>)> = Vec::new();
        let mut last_image_id: Option<u64> = None;
        for (texture_id, span) in image_spans.iter().copied() {
            let set = self.image_descriptor_set(texture_id)?;
            self.write_image_descriptors(&set, texture_id)?;
            match image_groups.last_mut() {
                Some((_, spans)) if last_image_id == Some(texture_id) => {
                    spans.push(span);
                }
                _ => {
                    last_image_id = Some(texture_id);
                    image_groups.push((set, vec![span]));
                }
            }
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

        // One-shot dump of what is actually uploaded. A correct pass, viewport,
        // scissor and draw that rasterise nothing are the signature of degenerate
        // or zeroed vertex/index data, and that is only visible in the bytes.
        if std::env::var_os("UI_SPIKE_DUMP").is_some()
            && DUMP_DONE.swap(true, std::sync::atomic::Ordering::Relaxed) == false
        {
            eprintln!("[ui] upload: {} verts, {} indices", vertices.len(), indices.len());
            for (i, v) in vertices.iter().take(4).enumerate() {
                eprintln!(
                    "  v[{i}] pos=({:.3},{:.3}) uv=({:.3},{:.3}) color=({:.2},{:.2},{:.2},{:.2})",
                    v.position[0], v.position[1], v.tex_coords[0], v.tex_coords[1],
                    v.color[0], v.color[1], v.color[2], v.color[3]
                );
            }
            eprintln!("  i[..] = {:?}", &indices[..indices.len().min(6)]);
            // Which buffers are actually bound, and what reached the mapped
            // memory. If the upload never lands, every vertex is (0,0), the quad
            // collapses to a point and nothing rasterises - with no validation
            // error, because nothing Vulkan sees is actually invalid.
            if let Some(gpu) = self.gpu.as_ref() {
                if let Some(bytes) = self.device.buffer_data(&gpu.vertex_buffer) {
                    let head: Vec<String> =
                        bytes[..16.min(bytes.len())].iter().map(|b| b.to_string()).collect();
                    eprintln!("  vbuf[0..16] bytes = [{}]", head.join(", "));
                }
                if let Some(bytes) = self.device.buffer_data(&gpu.index_buffer) {
                    let idx: Vec<u32> = bytes[..8.min(bytes.len())]
                        .chunks_exact(4)
                        .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                        .collect();
                    eprintln!("  ibuf[..2] indices = {idx:?}");
                }
            }
            // Positions are in pixels; the vertex shader expects clip space. A
            // wrong projection puts every vertex outside the frustum, which
            // rasterises nothing and raises no validation error at all.
            eprintln!(
                "  projection = [{}]",
                projection
                    .iter()
                    .map(|v| format!("{v:.5}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }

        let mut commands: Vec<Command> = Vec::with_capacity(20 + geometry_spans.len() + text_spans.len());        if self.config.debug_label {
            commands.push(Command::BeginDebugLabel {
                label: "ui".into(),
                color: [0.0, 0.85, 0.35, 1.0],
            });
        }

        // Move the newly sampled images to `ShaderReadOnlyOptimal` before the
        // pass opens. `TRANSFER` is where the upload left them and where the
        // transfer write happened; `FRAGMENT_SHADER` is where the sampler reads
        // them. Emitted outside the render pass on purpose: a sampled image is
        // not an attachment, so it can change layout here without a subpass
        // dependency.
        if !sample_transitions.is_empty() {
            commands.push(Command::PipelineBarrier {
                src_stage: PipelineStage::TRANSFER,
                dst_stage: PipelineStage::FRAGMENT_SHADER,
                memory_barriers: Vec::new(),
                buffer_barriers: Vec::new(),
                texture_barriers: sample_transitions
                    .iter()
                    .map(|texture| TextureBarrier {
                        texture: texture.clone(),
                        src_access: AccessFlags::TRANSFER_WRITE,
                        dst_access: AccessFlags::SHADER_READ,
                        old_layout: TextureLayout::TransferDstOptimal,
                        new_layout: TextureLayout::ShaderReadOnlyOptimal,
                    })
                    .collect(),
            });
        }

        // Without a framebuffer there is no image to draw into. The state
        // bindings are still recorded so the caller can tell the work was
        // produced.
        //
        // Two ways in, because two kinds of target exist and only one of them
        // can be described by handles:
        //
        //  - a swapchain image, which arrives as raw handles and whose
        //    framebuffer the RHI can build from them, and
        //  - an offscreen texture, which has no handle accessor in the RHI at
        //    all, so its framebuffer has to be built by the caller.
        //
        // Routing the second case through the first would mean either inventing
        // a second framebuffer or giving `Texture`/`TextureView` handle
        // accessors just to satisfy this one call.
        let framebuffer = match (framebuffer, target) {
            (Some(provided), _) => Some(provided.clone()),
            (None, Some(t)) => Some(
                self.device
                    .swapchain_image_framebuffer(
                        &render_pass,
                        t.image,
                        t.view,
                        t.format,
                        t.width,
                        t.height,
                    )
                    .map_err(|e| UiError::BackendError(format!("UI framebuffer: {e}")))?,
            ),
            (None, None) => None,
        };

        if let Some(framebuffer) = &framebuffer {
            commands.push(Command::BeginRenderPass(RenderPassBeginInfo {
                render_pass,
                framebuffer: framebuffer.clone(),
                render_area: Rect2D {
                    offset: Offset2D { x: 0, y: 0 },
                    extent: Extent2D { width, height },
                },
                // One clear value, one attachment.
                //
                // This used to be an empty list, which is invalid whenever the
                // pass declares `LoadOp::Clear` — VUID-VkRenderPassBeginInfo-
                // clearValueCount-00902 — and leaves the attachment's contents
                // undefined rather than merely unwritten. Both the presentation
                // and the offscreen pass clear, so both were affected.
                //
                // The colour is fully transparent black: it must not leave a
                // visible wash where the UI has nothing to draw. The real fix for
                // an overlay is `LoadOp::Load` with an acquire barrier, so the 3D
                // frame survives — that is still open, and is tracked separately.
                clear_values: vec![crate::primitives::ClearValue::color(0.0, 0.0, 0.0, 0.0)],
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

        // Images last: they are the topmost UI layer, and each texture group gets
        // its own descriptor set because the textured pipeline binds exactly one
        // texture. Groups are already contiguous, so consecutive images of the
        // same texture rebind nothing.
        if !image_groups.is_empty() {
            // Prefer the full-colour image pipeline. Fall back to the text
            // (coverage) pipeline for callers that never loaded an image shader,
            // which is what every caller did before images had their own stage.
            let pipeline = gpu.image_pipeline.as_ref().or(gpu.text_pipeline.as_ref());
            if let Some(pipeline) = pipeline {
                for (set, spans) in &image_groups {
                    commands.push(Command::BindGraphicsPipeline(pipeline.clone()));
                    commands.push(Command::BindDescriptorSets {
                        pipeline: pipeline.clone(),
                        first_set: 0,
                        sets: vec![set.clone()],
                    });
                    bind_buffers(&mut commands);
                    // Each group rebinds the pipeline, so the scissor has to be
                    // restated rather than assumed to carry over.
                    previous = None;
                    draw_spans(&mut commands, spans, &mut previous, &mut draw_calls);
                }
            } else {
                // Same reasoning as the text arm above: image geometry without a
                // textured pipeline has nowhere to go, and drawing it with the
                // untextured one would render every image as a solid block.
                eprintln!("[ui] image geometry without a textured pipeline; dropped");
            }
        }

        // Keyed on whether a render pass was actually *opened*, not on whether a
        // swapchain target was supplied.
        //
        // Refactoring this body to accept a caller-supplied framebuffer left
        // `target` permanently `None`, so this test went false and
        // `EndRenderPass` was never emitted. The render pass stayed open, which
        // makes `vkEndCommandBuffer` invalid (VUID-00060), so the driver
        // discarded the entire command buffer — every draw in the frame vanished,
        // with no error anywhere unless validation was running. It was not.
        if framebuffer.is_some() {
            commands.push(Command::EndRenderPass);
        }
        if self.config.debug_label {
            commands.push(Command::EndDebugLabel);
        }

        // One-shot command dump, gated so it never spams a running frame loop.
        // Validation reported `VUID-vkCmdDrawIndexed-renderpass` in the windowed
        // path — the draw landed outside an active render pass — and the recorded
        // order is the only way to tell a misplaced Begin/End from a missing one.
        if std::env::var_os("UI_SPIKE_DUMP").is_some()
            && DUMP_ORDER_DONE.swap(true, std::sync::atomic::Ordering::Relaxed) == false
        {
            let names: Vec<&str> = commands.iter().map(command_name).collect();
            eprintln!("[ui] recorded order: {names:?}");
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

        // Zero, not 1.0.
        //
        // The Vulkan backend decides `anisotropyEnable` from
        // `max_anisotropy > 0.0`, and the device does not enable the
        // `samplerAnisotropy` feature. So any non-zero value - including 1.0,
        // which is one sample and therefore *no* anisotropy - produces
        // `VUID-VkSamplerCreateInfo-anisotropyEnable-01070` and a sampler the
        // validator rejects. Zero leaves anisotropy off, which is what was
        // wanted anyway. The backend predicate is the real bug and is in
        // `src/rhi/backend/vulkan/device.rs`; it should be `> 1.0`.
        let sampler = self
            .device
            .create_sampler(AddressMode::ClampToEdge, FilterMode::Linear, 0.0);
        let view = self.gpu_texture_view(atlas)?;

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

    /// A GPU-backed view of `texture`, for binding as a sampled image.
///
/// Not `Texture::create_view`: that returns a CPU-side handle with no GPU
/// backing, and the failure surfaces much later as "descriptor texture view has
/// no GPU backing" when the write is issued — pointing at the descriptor rather
/// than at the view that was built the wrong way. Going through the device is
/// what actually allocates the view.
fn gpu_texture_view(
    &self,
    texture: &crate::resource::Texture,
) -> UiResult<crate::resource::TextureView> {
    self.device
        .create_texture_view(texture, texture.create_default_view().desc())
        .map_err(|e| UiError::BackendError(format!("UI texture view: {e}")))
}

/// The descriptor set that samples the texture registered under `texture_id`.
    ///
    /// One set per texture, cached, because the layout is the text layout
    /// (uniform, texture, sampler) and only the texture differs between images.
    /// Rebuilding it every frame would allocate a new set per frame and grow
    /// without bound.
    fn image_descriptor_set(&mut self, texture_id: u64) -> UiResult<DescriptorSet> {
        if let Some(set) = self.image_descriptor_sets.get(&texture_id) {
            return Ok(set.clone());
        }
        let layout = self
            .gpu
            .as_ref()
            .and_then(|g| g.text_descriptor_layout.clone())
            .ok_or(UiError::RendererNotInitialized)?;
        let set = self
            .device
            .create_descriptor_set(&layout)
            .map_err(|e| UiError::BackendError(format!("UI image descriptor set: {e}")))?;
        self.image_descriptor_sets.insert(texture_id, set.clone());
        Ok(set)
    }

    /// Points `set` at the texture registered under `texture_id`.
    ///
    /// The same three bindings the atlas uses - uniform at 0, sampled texture at
    /// 1, sampler at 2 - so the textured pipeline serves both glyphs and images
    /// without a second layout.
    fn write_image_descriptors(&self, set: &DescriptorSet, texture_id: u64) -> UiResult<()> {
        use crate::resource::sampler::{AddressMode, FilterMode};

        let texture = self
            .texture_cache
            .get(&texture_id)
            .ok_or_else(|| UiError::BackendError(format!("unknown UI texture {texture_id}")))?;
        // See `write_text_descriptors` for why the anisotropy argument is 0.
        let sampler = self
            .device
            .create_sampler(AddressMode::ClampToEdge, FilterMode::Linear, 0.0);
        let view = self.gpu_texture_view(texture)?;
        let uniform = self
            .gpu
            .as_ref()
            .ok_or(UiError::RendererNotInitialized)?
            .uniform_buffer
            .clone();

        self.device
            .write_descriptors(
                set,
                &[
                    DescriptorWrite {
                        dst_set: set.clone(),
                        dst_binding: 0,
                        dst_array_element: 0,
                        descriptors: vec![DescriptorInfo::Buffer(uniform, 0, UI_UNIFORM_SIZE)],
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
            .map_err(|e| UiError::BackendError(format!("UI image descriptor write: {e}")))
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

    /// Builds a retained element of `element_type` with `data`, sized so the
    /// element's own geometry is non-degenerate.
    fn retained(
        element_type: crate::ui::elements::UiElementType,
        data: crate::ui::elements::UiElementData,
    ) -> UiElement {
        let mut element = UiElement::new(element_type, 1u64);
        element.position = (10.0, 10.0);
        element.size = (120.0, 60.0);
        element.data = data;
        element
    }

    /// Runs one element through `emit_element` and reports what came out:
    /// drawable batches, vertex count, and how many commands were skipped.
    ///
    /// No fonts and no textures are supplied, which is the case that matters for
    /// the geometry-like elements: a text or image element under it is skipped,
    /// because a missing font *is* a resource this stage lacks. Text that does
    /// draw is covered by `ui_spike_offscreen` reading real pixels.
    fn emitted(element: &UiElement) -> (usize, usize, u32) {
        let mut b = builder();
        let mut rendered = 0u32;
        let fonts = HashMap::new();
        let textures: HashMap<u64, Arc<crate::resource::Texture>> = HashMap::new();
        let theme = crate::ui::theme::UiTheme::default();
        let mut text = Vec::new();
        UiRenderer::emit_element(
            &mut b,
            element,
            &mut rendered,
            &fonts,
            None,
            &textures,
            &theme,
            &std::collections::HashMap::new(),
            &mut text,
        );
        b.flush();
        (b.batches.len(), b.vertex_total as usize, b.skipped)
    }

    /// Every retained type that has no resource dependency must produce
    /// geometry. An element type that quietly produces nothing is
    /// indistinguishable from one that was never implemented.
    #[test]
    fn retained_elements_without_resources_draw_something() {
        use crate::ui::elements::{UiElementData, UiElementType};

        let cases: Vec<(&str, UiElementType, UiElementData)> = vec![
            ("radio unchecked", UiElementType::RadioButton, UiElementData::RadioButton {
                checked: false, label: "a".into(), group: "g".into(),
            }),
            ("radio checked", UiElementType::RadioButton, UiElementData::RadioButton {
                checked: true, label: "a".into(), group: "g".into(),
            }),
            ("scroll", UiElementType::ScrollContainer, UiElementData::ScrollContainer {
                scroll_x: 0.0, scroll_y: 20.0, scroll_width: 100.0, scroll_height: 400.0,
            }),
            ("dropdown closed", UiElementType::Dropdown, UiElementData::Dropdown {
                selected_index: Some(1), items: vec!["a".into(), "b".into()], is_open: false,
            }),
            ("dropdown open", UiElementType::Dropdown, UiElementData::Dropdown {
                selected_index: Some(0), items: vec!["a".into(), "b".into()], is_open: true,
            }),
            ("list", UiElementType::ListView, UiElementData::ListView {
                items: vec!["a".into(), "b".into(), "c".into()], selected_indices: vec![1],
            }),
            ("grid", UiElementType::GridView, UiElementData::GridView {
                columns: 3,
                items: vec!["a".into(), "b".into(), "c".into(), "d".into()],
                selected_indices: vec![2],
            }),
        ];

        for (name, element_type, data) in cases {
            let (batches, vertices, skipped) = emitted(&retained(element_type, data));
            assert!(batches > 0, "{name} produced no drawable geometry");
            assert!(vertices > 0, "{name} produced no vertices");
            assert_eq!(skipped, 0, "{name} was counted as skipped");
        }
    }

    /// Retained text and images skip **only** because the resource is missing.
    ///
    /// This used to read "they need an atlas and a sampled-texture binding that
    /// this stage does not have" — true when it was written, false since
    /// `install_bitmap_font` and the per-texture descriptor sets landed. The
    /// count is still the right thing to assert, but for a different reason: with
    /// no font registered and no texture uploaded, a skipped element is the
    /// honest answer, and it is what tells a caller the difference between "this
    /// element cannot be drawn yet" and "this element drew nothing on purpose".
    ///
    /// What they do *with* those resources is proved by pixels, not here:
    /// `ui_spike_offscreen` renders a string through the real pipeline and reads
    /// the atlas out of the frame.
    #[test]
    fn text_and_image_skip_only_when_their_resource_is_missing() {
        use crate::ui::elements::{UiElementData, UiElementType};

        let text = retained(
            UiElementType::Text,
            UiElementData::Text { content: "hi".into() },
        );
        assert_eq!(
            emitted(&text).2,
            1,
            "text with no font registered must report as skipped"
        );

        let image = retained(
            UiElementType::Image,
            UiElementData::Image {
                texture_id: 1,
                uv_min: [0.0, 0.0],
                uv_max: [1.0, 1.0],
            },
        );
        assert_eq!(
            emitted(&image).2,
            1,
            "an image whose texture was never uploaded must report as skipped"
        );
    }

    /// A `Custom` element must not be counted as skipped. `skipped` means "needs
    /// a resource this stage lacks", and a caller reading that would go looking
    /// for a missing atlas when the real gap is that there is no callback
    /// contract to call at all.
    #[test]
    fn a_custom_element_neither_draws_nor_claims_a_missing_resource() {
        use crate::ui::elements::{UiElementData, UiElementType};

        let element = retained(
            UiElementType::Custom,
            UiElementData::Custom { render_function: Some("draw_hud".into()) },
        );
        let (batches, vertices, skipped) = emitted(&element);
        assert_eq!((batches, vertices), (0, 0), "a custom element drew a placeholder");
        assert_eq!(skipped, 0, "a custom element reported a missing resource");
    }

    /// A scroll container whose content fits must not draw a scrollbar: the
    /// thumb would cover the whole track and read as a permanent border.
    #[test]
    fn a_scroll_container_that_fits_draws_no_bar() {
        use crate::ui::elements::{UiElementData, UiElementType};

        let element = retained(
            UiElementType::ScrollContainer,
            UiElementData::ScrollContainer {
                scroll_x: 0.0,
                scroll_y: 0.0,
                // Content no larger than the 120x60 viewport.
                scroll_width: 100.0,
                scroll_height: 50.0,
            },
        );
        // The background only: one quad, four vertices, no thumb.
        let (batches, vertices, _) = emitted(&element);
        assert_eq!((batches, vertices), (1, 4));
    }

    /// A grid with zero columns would divide by zero when splitting the width.
    #[test]
    fn a_grid_with_no_columns_does_not_divide_by_zero() {
        use crate::ui::elements::{UiElementData, UiElementType};

        let element = retained(
            UiElementType::GridView,
            UiElementData::GridView {
                columns: 0,
                items: vec!["a".into(), "b".into()],
                selected_indices: vec![],
            },
        );
        let (_, vertices, _) = emitted(&element);
        assert!(vertices > 0, "a zero-column grid drew nothing");
    }

    /// Image quads must carry the UV rect they were asked for.
    ///
    /// The vertex layout has always had texture coordinates, but until now
    /// nothing wrote anything but `[0, 0]`, so a sampled draw would have shown
    /// one texel stretched over the quad. This asserts the mapping corner by
    /// corner, which is where an inverted axis shows up.
    #[test]
    fn an_image_quad_carries_the_requested_uv_rect() {
        let mut b = builder();
        b.image_quad(7, 10.0, 20.0, 100.0, 50.0, [0.25, 0.5], [0.75, 1.0], RED);
        let groups = b.flush_images();
        assert_eq!(groups.len(), 1, "one texture must produce one group");
        assert_eq!(groups[0].0, 7, "the group lost its texture id");
        let v = &groups[0].1.vertices;
        assert_eq!(v.len(), 4);
        assert_eq!(v[0].tex_coords, [0.25, 0.5]);
        assert_eq!(v[1].tex_coords, [0.75, 0.5]);
        assert_eq!(v[2].tex_coords, [0.75, 1.0]);
        assert_eq!(v[3].tex_coords, [0.25, 1.0]);
        // And the rectangle itself, so a UV fix cannot quietly move the quad.
        assert_eq!(v[0].position, [10.0, 20.0]);
        assert_eq!(v[2].position, [110.0, 70.0]);
        assert!(v.iter().all(|x| x.color == RED));
    }

    /// Several images of one texture share a single draw; a new texture starts a
    /// new group. This is the batching the descriptor-set-per-texture design buys,
    /// and losing it would mean a bind per image.
    #[test]
    fn images_of_one_texture_share_a_group() {
        let mut b = builder();
        for i in 0..4 {
            b.image_quad(1, i as f32 * 10.0, 0.0, 8.0, 8.0, [0.0, 0.0], [1.0, 1.0], RED);
        }
        b.image_quad(2, 0.0, 20.0, 8.0, 8.0, [0.0, 0.0], [1.0, 1.0], RED);
        let groups = b.flush_images();
        assert_eq!(groups.len(), 2, "expected one group per texture");
        assert_eq!(groups[0].0, 1);
        assert_eq!(groups[0].1.indices.len(), 4 * 6);
        assert_eq!(groups[1].0, 2);
        assert_eq!(groups[1].1.indices.len(), 6);
    }

    /// Interleaved images of the same texture still group together. Grouping on
    /// arrival would start a new draw every time the id changed, which is the
    /// case this design exists to avoid.
    #[test]
    fn interleaved_images_of_one_texture_still_group() {
        let mut b = builder();
        b.image_quad(1, 0.0, 0.0, 8.0, 8.0, [0.0, 0.0], [1.0, 1.0], RED);
        b.image_quad(2, 8.0, 0.0, 8.0, 8.0, [0.0, 0.0], [1.0, 1.0], RED);
        b.image_quad(1, 16.0, 0.0, 8.0, 8.0, [0.0, 0.0], [1.0, 1.0], RED);
        let groups = b.flush_images();
        // Two textures, but three runs: the second image of texture 1 cannot
        // share the descriptor set bind with its earlier one, because texture 2's
        // bind intervened. Ordering is preserved, so the middle group is what
        // split.
        assert_eq!(groups.len(), 3);
        assert_eq!(groups.iter().map(|g| g.0).collect::<Vec<_>>(), vec![1, 2, 1]);
    }

    /// A zero-area UV rect is dropped, not drawn. A degenerate primitive still
    /// costs a draw call, and it samples the whole texture's edge.
    #[test]
    fn an_image_with_a_degenerate_uv_rect_is_dropped() {
        let mut b = builder();
        b.image_quad(1, 0.0, 0.0, 8.0, 8.0, [0.5, 0.5], [0.5, 0.5], RED);
        b.image_quad(1, 0.0, 0.0, 8.0, 8.0, [0.5, 0.5], [0.25, 0.5], RED);
        let groups = b.flush_images();
        assert!(groups.is_empty(), "a zero-area UV rect produced geometry");
    }

    /// Non-finite positions, sizes, colours and UVs are all dropped, for the same
    /// reason `quad` drops them: they reach the vertex buffer unchanged.
    #[test]
    fn an_image_with_non_finite_values_is_dropped() {
        let nan = f32::NAN;
        let mut b = builder();
        b.image_quad(1, nan, 0.0, 8.0, 8.0, [0.0, 0.0], [1.0, 1.0], RED);
        b.image_quad(1, 0.0, 0.0, nan, 8.0, [0.0, 0.0], [1.0, 1.0], RED);
        b.image_quad(1, 0.0, 0.0, 8.0, 8.0, [0.0, 0.0], [nan, 1.0], RED);
        b.image_quad(1, 0.0, 0.0, 8.0, 8.0, [0.0, 0.0], [1.0, 1.0], [nan, 0.0, 0.0, 1.0]);
        let groups = b.flush_images();
        assert!(groups.is_empty(), "a non-finite image produced geometry");
    }
}

/// One-shot gate for the `UI_SPIKE_DUMP` command-order dump.
static DUMP_DONE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// One-shot gate for the `UI_SPIKE_DUMP` command-order dump.
static DUMP_ORDER_DONE: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// A short label for a recorded command, used only by the debug dump.
fn command_name(command: &Command) -> &'static str {
    match command {
        Command::BeginRenderPass(info) => {
            // The backend honours `render_area` whenever it is non-zero, and
            // rasterisation outside it is discarded outright - so a wrong rect is
            // a silent full-frame wipe, not an error.
            eprintln!(
                "[ui] BeginRenderPass area=({},{} {}x{}) clears={:?}",
                info.render_area.offset.x,
                info.render_area.offset.y,
                info.render_area.extent.width,
                info.render_area.extent.height,
                info.clear_values.len()
            );
            "BeginRenderPass"
        }
        Command::EndRenderPass => "EndRenderPass",
        Command::NextSubpass => "NextSubpass",
        Command::BindGraphicsPipeline(_) => "BindGraphicsPipeline",
        Command::BindDescriptorSets { .. } => "BindDescriptorSets",
        Command::BindVertexBuffers { .. } => "BindVertexBuffers",
        Command::BindIndexBuffer { .. } => "BindIndexBuffer",
        Command::SetViewport(_) => "SetViewport",
        Command::SetScissor(_) => "SetScissor",
        Command::DrawIndexed { .. } => "DrawIndexed",
        Command::Draw { .. } => "Draw",
        Command::PipelineBarrier { .. } => "PipelineBarrier",
        Command::BeginDebugLabel { .. } => "BeginDebugLabel",
        Command::EndDebugLabel => "EndDebugLabel",
        _ => "other",
    }
}