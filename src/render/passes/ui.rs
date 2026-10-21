//! The UI pass, driven by `rhi::ui::UiRenderer`.
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//!
//! This used to be a self-contained pass with a hand-written pipeline, and it
//! never touched `rhi::ui` at all: two independent implementations of the same
//! idea, only one of which was exercised. It now wraps `UiRenderer` and hands it
//! the acquired swapchain image, so the drawing code that ships is the same code
//! the unit tests cover.
//!
//! The size rule is the one the geometry pass arrived at (Bug №272), and it is
//! load-bearing rather than cosmetic:
//!
//!  - The window is created with a *logical* size while the swapchain holds
//!    *physical* pixels, so at 125% scaling a 1280x720 window yields a 1600x900
//!    swapchain. A target sized from `context.resolution` covers a corner.
//!  - `context.resolution` is the wrong primary source. It is also not always
//!    populated: a headless context has neither it nor a presentation extent,
//!    and a pass that trusted it drew nothing at all.
//!  - So the physical presentation extent leads, and resolution is only the last
//!    resort for an offscreen target.
//!
//! The projection is built from the same extent, so the viewport, the scissor
//! and the NDC mapping agree by construction rather than by coincidence.

use crate::render::core::RenderContext;
use crate::render::graph::node::{PassDependency, PassOutcome, RenderPass};
use crate::render::graph::resource::GraphResource;
use crate::render::graph::types::ResourceUsage;
use crate::render::passes::base::BaseRenderPass;
use crate::render::scene::Scene;
use crate::rhi::command::encoder::CommandEncoder;
use crate::rhi::ui::{UiCommand, UiRenderStats, UiRenderer, UiRendererConfig};
use std::collections::HashMap;

/// Compiled from `assets/shaders/ui_geom.wgsl`. The RHI takes SPIR-V as bytes
/// rather than reading a path, so it does not need to know where assets live.
///
/// The path reaches into `Tool/client_tests`, which is the convention
/// `examples/cube.rs` already uses and the only place the compiled `.spv`
/// currently exist. It couples the render crate to another workspace and will
/// break if the layout moves, so it is worth moving the compiled output next to
/// the source rather than spreading a third path convention.
const UI_GEOM_SPV: &[u8] =
    include_bytes!("../../../Tool/client_tests/assets/shaders/ui_geom.spv");

/// The textured pipeline, used for glyphs and images. Optional: if it is missing
/// the renderer still draws solid geometry and reports text as skipped, which
/// beats refusing to start.
const UI_TEX_SPV: &[u8] =
    include_bytes!("../../../Tool/client_tests/assets/shaders/ui_tex.spv");

/// UI configuration.
#[derive(Debug, Clone)]
pub struct UIConfig {
    pub resolution_scale: f32,
    pub enable_hud: bool,
    pub enable_menu: bool,
    pub enable_minimap: bool,
    /// Draw a single green rectangle in the corner. Deliberately crude: the
    /// point of it is to answer "does any UI pixel reach the screen at all",
    /// which nothing else in the stack can answer.
    pub debug_marker: bool,
}

impl Default for UIConfig {
    fn default() -> Self {
        Self {
            resolution_scale: 1.0,
            enable_hud: true,
            enable_menu: true,
            enable_minimap: true,
            debug_marker: true,
        }
    }
}

/// The UI pass.
pub struct UIPass {
    base: BaseRenderPass,
    /// `BaseRenderPass::name` is private, and `RenderPass::name` has to hand out
    /// a borrow that outlives the call, so the name is kept here too.
    name: String,
    config: UIConfig,
    /// Built once the surface format is known, so it cannot be built in `new`.
    ui: Option<UiRenderer>,
    /// The extent the renderer was initialized for, so a resize is noticed.
    built_for: Option<(u32, u32)>,
        /// Reported once per change rather than every frame, which would drown the
        /// log at 60 Hz.
        reported_init: bool,
        reported_resize: bool,
    /// Frames that actually recorded a draw. Drives the heartbeat log.
    frames: u64,
}

impl UIPass {
    pub fn new(config: UIConfig) -> Self {
        let mut base = BaseRenderPass::new("ui");
        // The UI samples what the 3D passes produced, so it runs after them.
        base.add_dependency("water", "water", ResourceUsage::Sampled);
        base.add_dependency("transparent", "final", ResourceUsage::Sampled);
        // Load, not clear: the UI is an overlay, and clearing here would erase
        // the 3D scene underneath it.
        base.add_output("swapchain");

        Self {
            base,
            name: "ui".to_string(),
            config,
            ui: None,
        built_for: None,
        reported_init: false,
            reported_resize: false,
            frames: 0,
        }
    }

    pub fn config(&self) -> &UIConfig {
        &self.config
    }

    /// Per-frame counters. The caller needs this because `UiRenderer` reports
    /// anything it refused to draw through `stats().skipped` rather than by
    /// failing — silence would otherwise look like success.
    pub fn stats(&self) -> UiRenderStats {
        self.ui.as_ref().map_or_default(UiRenderer::stats)
    }

    pub fn is_initialized(&self) -> bool {
        self.ui.as_ref().is_some_and(UiRenderer::is_initialized)
    }

    /// Builds (or rebuilds) the renderer for `width` x `height`.
    ///
    /// A resize invalidates the projection, so the renderer is rebuilt rather
    /// than only told about the new size: `UiRenderPass` bakes the extent into
    /// the viewport it records.
    fn ensure_built(
        &mut self,
        device: &std::sync::Arc<crate::rhi::core::Device>,
        format: crate::rhi::Format,
        width: u32,
        height: u32,
    ) {
        let needs_build = self
            .ui
            .as_ref()
            .map_or(true, |ui| !ui.is_initialized() || self.built_for != Some((width, height)));

        if !needs_build {
            return;
        }

        let mut ui = UiRenderer::new(
            std::sync::Arc::clone(device),
            UiRendererConfig {
                // Enough for a HUD on the first frame; the buffers grow.
                initial_vertex_capacity: 4096,
                initial_index_capacity: 6144,
                debug_label: false,
            },
        );

        match ui.initialize(format, width, height, UI_GEOM_SPV, Some(UI_TEX_SPV)) {
            Ok(()) => {
                // A font and its atlas are one fact, and neither half was ever
                // wired: with no atlas the textured pipeline cannot run, so every
                // string was counted as skipped and nothing said why.
                //
                // Failure is reported once and is not fatal — geometry is
                // independent of glyphs, and a renderer that cannot rasterise a
                // font should still draw its panels.
                if let Err(e) = ui.install_bitmap_font("bitmap8") {
                    eprintln!("[ui] font install failed, text will be skipped: {e}");
                }
                if !self.reported_init {
                    eprintln!(
                        "[ui] renderer ready for {width}x{height} as {format:?} \
                         (projection follows this extent)"
                    );
                    self.reported_init = true;
                }
                self.reported_resize = false;
                self.built_for = Some((width, height));
                self.ui = Some(ui);
            }
            Err(e) => {
                // Reported once: a per-frame message here would spam the log and
                // hide the one fact that matters, which is that nothing drew.
                if !self.reported_init {
                    eprintln!("[ui] renderer failed to initialize: {e}");
                    self.reported_init = true;
                }
                self.ui = None;
            }
        }
    }
}

impl RenderPass for UIPass {
    fn name(&self) -> &str {
        &self.name
    }

    fn dependencies(&self) -> &[PassDependency] {
        self.base.dependencies()
    }

    fn outputs(&self) -> &[String] {
        self.base.outputs()
    }

    fn initialize(
        &mut self,
        _device: &crate::rhi::core::Device,
        _context: &RenderContext,
        _resources: &mut HashMap<String, GraphResource>,
    ) {
        // Nothing is built here on purpose. The surface format is only known once
        // presentation is attached, and `UiRenderer` wants an `Arc<Device>` while
        // this hook is handed a `&Device` — so the renderer is built on the first
        // drawable frame in `execute`, which already has the `Arc`.
    }

    fn execute(
        &mut self,
        encoder: &mut CommandEncoder,
        context: &mut RenderContext,
        _scene: &mut Scene,
        _resources: &HashMap<String, GraphResource>,
    ) -> PassOutcome {
        // Bug №653: the pass reports what it did, so the graph can tell "was
        // called" from "contributed to the frame".
        let Some((width, height)) = Self::drawable_extent(context) else {
            // A minimised window reports a zero-sized swapchain; drawing into it
            // is not a valid operation, so the frame is skipped rather than
            // recorded against a zero-sized viewport.
            return PassOutcome::skipped("no drawable extent");
        };
        if width == 0 || height == 0 {
            return PassOutcome::skipped("zero-sized extent");
        }

        let Some(format) = context.presentation_format else {
            return PassOutcome::skipped("no presentation format");
        };

        self.ensure_built(context.device(), format, width, height);

        let Some(ui) = self.ui.as_mut() else {
            return PassOutcome::skipped("the UI renderer was not built");
        };

        // The acquired image's *typed* view, not a raw handle and not a pinned
        // image. Two separate reasons, and both are load-bearing:
        //
        //  - Pinning leaves every frame drawing into whichever image happened to
        //    be acquired first, and the other swapchain images are then presented
        //    having never been written.
        //  - A raw handle cannot become a framebuffer: `TextureView` owns the
        //    `Texture` it came from, so the RHI keeps no way to rebuild one from
        //    a handle. `UiRenderer::record` takes raw handles and therefore
        //    cannot record a render pass at all — it says so instead of returning
        //    success (see its `# Why this cannot honour a target yet`). The index
        //    is the same `swapchain_image_index` the presenter will present, so
        //    the view and the presented image agree by construction.
        let Some(view) = context
            .presentation_texture_views
            .get(context.swapchain_image_index as usize)
        else {
            return PassOutcome::skipped("no typed view for the acquired image");
        };

        // What is actually drawn this frame. Kept minimal on purpose — see
        // `UIConfig::debug_marker`.
        if self.config.debug_marker {
            // One rectangle at a fixed pixel offset. A single known-shape marker
            // is what tells "the UI drew nothing" apart from "the UI drew in the
            // wrong place": if this appears at the top-left corner at the right
            // size, the pixel mapping and the blend state are both correct, and
            // anything else wrong on screen is not the coordinate transform.
            //
            // It is deliberately not full-screen. An earlier version filled the
            // frame with blue as well, on the theory that an unmistakable fill
            // would be easier to spot. It was not: the fill covered up exactly
            // the thing being measured, and a marker that hides the background
            // cannot show whether it blended against it correctly.
            let corner = UiCommand::Rectangle {
                x: 32.0,
                y: 32.0,
                width: 160.0,
                height: 90.0,
                color: [0.76, 0.95, 0.13, 1.0],
                corner_radius: 0.0,
            };
            if let Err(e) = ui.submit_commands(vec![corner]) {
                eprintln!("[ui] submit_commands failed: {e}");
            }
        }

        // The presentation path, using the typed view. `UiRenderer::record`
        // cannot be used here: it identifies its target by raw `u64` handles and
        // records no render pass for them, which is exactly the
        // `VUID-vkCmdDrawIndexed-renderpass` and `VUID-VkPresentInfoKHR-
        // pImageIndices-01430` pair this pass used to emit. `record_into_view`
        // builds (and caches, per image view) the framebuffer and records
        // Begin/Draw/End.
        match ui.record_into_view(encoder, view, width, height) {
            // `false` means there was nothing to draw, which is normal on a frame
            // where every UI element was culled.
            Ok(true) => {
                let stats = ui.stats();
                if !self.reported_resize {
                    self.reported_resize = true;
                    eprintln!(
                        "[ui] first frame recorded at {width}x{height}: {} verts, \
                         {} batches, {} draw calls, {} skipped",
                        stats.vertices, stats.batches, stats.draw_calls, stats.skipped
                    );
                }
                // A heartbeat, because "the rectangle appears and then goes away"
                // is a claim about time and a single frame cannot refute it. This
                // is what shows whether recording keeps happening, whether the
                // acquired image index keeps cycling, and whether the two ever
                // disagree — the three things that could make a correct draw
                // invisible after a while.
                self.frames += 1;
                if self.frames % 120 == 0 {
                    eprintln!(
                        "[ui:hb] frame {} images={} image_index={} draw_calls={} skipped={}",
                        self.frames,
                        context.presentation_images.len(),
                        context.swapchain_image_index,
                        stats.draw_calls,
                        stats.skipped
                    );
                }
                PassOutcome::recorded()
            }
            Ok(false) => PassOutcome::skipped("nothing to draw"),
            Err(e) => {
                eprintln!("[ui] record failed: {e}");
                PassOutcome::skipped(format!("record failed: {e}"))
            }
        }
    }

    fn resize(&mut self, width: u32, height: u32) {
        if let Some(ui) = self.ui.as_mut() {
            ui.resize(width, height);
        }
        // Force a rebuild: the baked extent inside `UiRenderPass` is stale now.
        //
        // Bug №655: this used to set `built_for = Some((width, height))` — that
        // is, it recorded the *new* size as the size the pass was built for.
        // `needs_build` compares `built_for` with the size it is being asked
        // for, so after a resize the two were equal and `needs_build` was
        // false: the rebuild the comment demands was exactly the thing the
        // code prevented.
        //
        // The correct invalidation is to forget what the pass was built for.
        // `None` is not a size, so it never equals the size being asked for,
        // and the next `execute` rebuilds.
        self.built_for = None;
    }

    fn is_enabled(&self) -> bool {
        true
    }
}

impl UIPass {
    /// The size to draw at, in physical pixels.
    ///
    /// Delegates to [`RenderContext::drawable_extent`], which is where the rule
    /// lives now. It used to be written out here as well, which is how it ended
    /// up written out wrongly in five other places (Bug №651): a rule that is
    /// copied is a rule that drifts.
    fn drawable_extent(context: &RenderContext) -> Option<(u32, u32)> {
        context.drawable_extent()
    }
}