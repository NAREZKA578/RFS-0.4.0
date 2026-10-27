//! UI Render Pass
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//!
//! The render pass the UI draws into, plus the screen-space projection its
//! shaders use.
//!
//! # Why this is not `Device::presentation_render_pass`
//!
//! That helper describes a pass which *starts* a frame: it clears the colour
//! attachment and declares `Undefined` as the initial layout, because nothing
//! has been drawn yet. UI is the opposite — it composites over a finished 3D
//! frame, so it must load what is already there and start from
//! `ColorAttachmentOptimal`.
//!
//! Getting this wrong is not a cosmetic problem. Clearing here wipes the scene
//! every time the UI pass runs, and a pipeline that declares
//! `ColorAttachmentOptimal` as the initial layout of an `Undefined` image
//! produces undefined contents, which on most drivers is a black frame.

use crate::command::pass::render::RenderPass;
use crate::types::Format;
use crate::Device;

use super::{UiError, UiResult};

/// Screen-space orthographic projection with the origin at the top left.
///
/// UI positions are pixels with Y growing downwards, which is the opposite of
/// Vulkan's clip space, so Y is negated here rather than in every caller.
/// Z is left at zero: the UI pipeline declares no depth state, so the value is
/// never tested.
///
/// A zero extent would divide by zero and put infinities in the matrix, which
/// silently draws nothing; `max(1)` keeps it finite and the geometry simply
/// ends up off-screen.
///
/// The result is column-major, matching what `std140 mat4x4<f32>` expects.
///
/// Free-standing so it can be checked without a device: the interesting
/// behaviour here is arithmetic, not a GPU round trip.
pub fn ui_ortho_projection(width: u32, height: u32) -> [f32; 16] {
    let w = width.max(1) as f32;
    let h = height.max(1) as f32;
        [
            2.0 / w, 0.0, 0.0, 0.0, // column 0
            0.0, -2.0 / h, 0.0, 0.0, // column 1
        0.0, 0.0, 0.0, 0.0, // column 2
        -1.0, 1.0, 0.0, 1.0, // column 3: translation
    ]
}

/// The description of the pass the UI overlays a finished 3D frame with (bug №647).
///
/// Free-standing — like [`ui_ortho_projection`] — because the shape is data,
/// and the weakest link of the bug is exactly a struct-literal constant: the
/// built pass used to copy the *frame-starting* description (`LoadOp::Clear`,
/// `initial_layout: Undefined`) from the presentation geometry pass, which is
/// the operator for an image whose contents may be discarded. An overlay is the
/// opposite operator, and both endpoints here are facts about the recorded
/// frame around the UI pass rather than choices of this code:
///
///  - `initial_layout`: the geometry pass that records before the UI — its own
///    presentation description — ends the swapchain image in `PresentSrc`, and
///    both passes go into the same recorded submission. `Undefined` was the
///    only legal-but-lying initial layout: the wildcard that discards previous
///    contents, and the frame the UI must sit on was the previous contents.
///    (`ColorAttachmentOptimal` would lie differently: with the image coming
///    from pass end in `PresentSrc`, naming it at pass begin is a conformance
///    error, not an operator.)
///  - `final_layout`: `PresentSrc` — what `vkQueuePresentKHR` needs, and what
///    the next frame's geometry pass then starts from again.
///
/// The subpass itself stays `ColorAttachmentOptimal` and the store stays
/// `Store`: the UI paints into it as into any colour attachment.
pub(crate) fn overlay_pass_desc(format: Format) -> crate::command::pass::render::RenderPassDesc {
    use crate::command::pass::render::{
        AttachmentDescription, AttachmentReference, LoadOp, PipelineBindPoint, StoreOp,
        SubpassDescription,
    };
    use crate::resource::TextureLayout;

    crate::command::pass::render::RenderPassDesc {
        attachments: vec![AttachmentDescription {
            format,
            samples: crate::types::SampleCount::X1,
            // Load, not Clear: the UI composites *over* what is already there.
            // Clear is right only for the offscreen variant, which has nothing
            // to preserve — see `new_offscreen`.
            // Load, not Clear: the UI composites *over* what is already there.
            // Clear is right only for the offscreen variant, which has nothing
            // to preserve — see `new_offscreen`.
            load_op: LoadOp::Load,
            store_op: StoreOp::Store,
            stencil_load_op: LoadOp::DontCare,
            stencil_store_op: StoreOp::DontCare,
            initial_layout: TextureLayout::PresentSrc,
            final_layout: TextureLayout::PresentSrc,
        }],
        subpasses: vec![SubpassDescription {
            pipeline_bind_point: PipelineBindPoint::Graphics,
            color_attachments: vec![AttachmentReference {
                attachment: 0,
                layout: TextureLayout::ColorAttachmentOptimal,
            }],
            depth_stencil_attachment: None,
            ..Default::default()
        }],
        dependencies: vec![],
    }
}

/// Render pass and projection for UI drawn over a finished frame.
pub struct UiRenderPass {
    render_pass: RenderPass,
    /// Column-major 4x4, laid out for `std140` `mat4x4<f32>`.
    projection: [f32; 16],
    format: Format,
    width: u32,
    height: u32,
}

impl UiRenderPass {
    /// Build the UI pass for a surface format and size.
    ///
    /// See [`Self::new_offscreen`] for the variant that can be read back.
    pub fn new(device: &Device, format: Format, width: u32, height: u32) -> UiResult<Self> {
        let render_pass = device
            .create_gpu_render_pass(&overlay_pass_desc(format))
            .map_err(|e| UiError::BackendError(format!("UI presentation render pass: {e}")))?;

        Self::from_render_pass(render_pass, format, width, height)
    }

    /// Build the UI pass for an offscreen texture that can be read back.
    ///
    /// # Why this exists separately
    ///
    /// A presentation pass ends the frame in `PresentSrc`. That is correct for a
    /// swapchain image and useless for verification: `download_texture` copies
    /// out of `TransferSrcOptimal` (or `General`), and an image parked in
    /// `PresentSrc` is neither readable nor copyable, so a UI pass that can only
    /// present cannot be used to check whether it drew anything at all.
    ///
    /// That is the whole problem this exists to solve. Every check so far has
    /// been indirect — "a draw call was recorded", "validation stayed quiet" —
    /// and none of them can tell a correct draw from one that rasterised nothing.
    /// Reading the pixels answers it.
    ///
    /// `Clear` rather than `Load`: an offscreen texture has no 3D frame to
    /// preserve, and starting from `Undefined` makes the untouched part of the
    /// image a known value rather than a guess.
    pub fn new_offscreen(device: &Device, format: Format, width: u32, height: u32) -> UiResult<Self> {
        use crate::command::pass::render::{
            AttachmentDescription, AttachmentReference, LoadOp, PipelineBindPoint,
            RenderPassDesc, StoreOp, SubpassDescription,
        };
        use crate::resource::TextureLayout;

        let render_pass = device
            .create_gpu_render_pass(&RenderPassDesc {
                attachments: vec![AttachmentDescription {
                    format,
                    samples: crate::types::SampleCount::X1,
                    load_op: LoadOp::Clear,
                    store_op: StoreOp::Store,
                    stencil_load_op: LoadOp::DontCare,
                    stencil_store_op: StoreOp::DontCare,
                    initial_layout: TextureLayout::Undefined,
                    // The one difference that matters: not `PresentSrc`.
                    // `ColorAttachmentOptimal`, not `TransferSrcOptimal`.
            // `Device::download_texture` derives its barrier's `oldLayout` from
            // the texture's *usage flags* - COLOR_ATTACHMENT_OPTIMAL for a
            // colour attachment - so a pass that already leaves the image in
            // TRANSFER_SRC_OPTIMAL makes that barrier lie about the old layout.
            // Validation rejected it as `VUID-vkCmdDraw-None-09600` and the
            // readback came back empty. Geometry already ends here, which is why
            // the same readback is clean for it.
            final_layout: TextureLayout::ColorAttachmentOptimal,
                }],
                subpasses: vec![SubpassDescription {
                    pipeline_bind_point: PipelineBindPoint::Graphics,
                    color_attachments: vec![AttachmentReference {
                        attachment: 0,
                        layout: TextureLayout::ColorAttachmentOptimal,
                    }],
                    depth_stencil_attachment: None,
                    ..Default::default()
                }],
                dependencies: vec![],
            })
            .map_err(|e| UiError::BackendError(format!("UI offscreen render pass: {e}")))?;

        Self::from_render_pass(render_pass, format, width, height)
    }

    fn from_render_pass(
        render_pass: RenderPass,
        format: Format,
        width: u32,
        height: u32,
    ) -> UiResult<Self> {
        let mut this = Self {
            render_pass,
            projection: [0.0; 16],
            format,
            width: width.max(1),
            height: height.max(1),
        };
        this.rebuild_projection();
        Ok(this)
    }

    /// Screen-space orthographic projection; see [`ui_ortho_projection`].
    fn rebuild_projection(&mut self) {
        self.projection = ui_ortho_projection(self.width, self.height);
    }

    pub fn render_pass(&self) -> &RenderPass {
        &self.render_pass
    }

    /// The matrix to upload into the UI uniform buffer, column-major.
    pub fn projection(&self) -> [f32; 16] {
        self.projection
    }

    pub fn format(&self) -> Format {
        self.format
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    /// Update the target size after a window resize.
    ///
    /// The render pass itself does not change — its attachments are described
    /// by format and sample count, not by extent — so only the projection is
    /// rebuilt. The framebuffer, which does depend on extent, is rebuilt by the
    /// renderer against the new swapchain.
    pub fn resize(&mut self, width: u32, height: u32) {
        self.width = width.max(1);
        self.height = height.max(1);
        self.rebuild_projection();
    }
}

#[cfg(test)]
mod tests {
    use super::ui_ortho_projection;

    use super::overlay_pass_desc;
    use crate::command::pass::render::LoadOp;
    use crate::resource::TextureLayout;

    /// Bug №647 negative control: the overlay's shape is data here, so the
    /// old behaviour — `LoadOp::Clear` plus an `Undefined` initial layout, the
    /// operator for an image whose contents may be discarded — reds this test
    /// on every assert. The old builder shipped exactly that while its own
    /// comment named the cost: "wipes the 3D frame the UI is meant to sit on".
    #[test]
    fn the_overlay_preserves_the_frame_instead_of_clearing() {
        let desc = overlay_pass_desc(crate::types::Format::B8G8R8A8_UNORM);
        let attachment = &desc.attachments[0];
        assert!(matches!(attachment.load_op, LoadOp::Load));
        assert!(matches!(attachment.initial_layout, TextureLayout::PresentSrc));
        assert!(matches!(attachment.final_layout, TextureLayout::PresentSrc));
        // The subpass paints into the attachment as into any colour target.
        assert!(matches!(
            desc.subpasses[0]
                .color_attachments
                .first()
                .expect("the overlay describes exactly one colour attachment"),
            crate::command::pass::render::AttachmentReference {
                layout: TextureLayout::ColorAttachmentOptimal,
                ..
            }
        ));
    }

    /// Applies the column-major matrix to a point the way the shader does.
    fn project(m: &[f32; 16], x: f32, y: f32) -> [f32; 4] {
        [
            m[0] * x + m[4] * y + m[12],
            m[1] * x + m[5] * y + m[13],
            m[2] * x + m[6] * y + m[14],
            m[3] * x + m[7] * y + m[15],
        ]
    }

    #[test]
    fn the_top_left_corner_maps_to_clip_minus_one_plus_one() {
        // Vulkan's clip +1 is the top of the screen, which is where UI y = 0 is.
        let m = ui_ortho_projection(1920, 1080);
        let p = project(&m, 0.0, 0.0);
        assert!((p[0] + 1.0).abs() < 1e-6, "x was {}", p[0]);
        assert!((p[1] - 1.0).abs() < 1e-6, "y was {}", p[1]);
    }

    #[test]
    fn the_bottom_right_corner_maps_to_clip_plus_one_minus_one() {
        // UI Y grows downwards, so the bottom of the screen must be clip -1.
        let m = ui_ortho_projection(1920, 1080);
        let p = project(&m, 1920.0, 1080.0);
        assert!((p[0] - 1.0).abs() < 1e-6, "x was {}", p[0]);
        assert!((p[1] + 1.0).abs() < 1e-6, "y was {}", p[1]);
    }

    #[test]
    fn the_centre_maps_to_the_clip_origin() {
        let m = ui_ortho_projection(800, 600);
        let p = project(&m, 400.0, 300.0);
        assert!(p[0].abs() < 1e-6, "x was {}", p[0]);
        assert!(p[1].abs() < 1e-6, "y was {}", p[1]);
    }

    #[test]
    fn a_zero_extent_stays_finite_instead_of_producing_infinities() {
        // A window that has not been sized yet must not poison the matrix:
        // NaN there draws nothing and reports nothing.
        for (w, h) in [(0u32, 0u32), (0, 1080), (1920, 0)] {
            let m = ui_ortho_projection(w, h);
            assert!(
                m.iter().all(|v| v.is_finite()),
                "{w}x{h} produced a non-finite matrix: {m:?}"
            );
        }
    }

    #[test]
    fn depth_is_flat_because_the_pipeline_has_no_depth_state() {
        let m = ui_ortho_projection(640, 480);
        let p = project(&m, 10.0, 20.0);
        assert_eq!(p[2], 0.0);
        assert_eq!(p[3], 1.0);
    }

    #[test]
    fn the_projection_follows_a_physical_extent_that_differs_from_the_window() {
        // A window can be logical while the swapchain is physical: 1280x720 at
        // 125% is a 1600x900 image. The projection has to describe the extent
        // actually being drawn into, or the UI is scaled to the wrong size.
        let logical = ui_ortho_projection(1280, 720);
        let physical = ui_ortho_projection(1600, 900);

        // The logical projection maps the logical width to the right edge...
        let p = project(&logical, 1280.0, 720.0);
        assert!((p[0] - 1.0).abs() < 1e-6);

        // ...while the physical one maps the physical width, so a UI laid out
        // in physical pixels still lands on the right edge.
        let p = project(&physical, 1600.0, 900.0);
        assert!((p[0] - 1.0).abs() < 1e-6);
        assert!((p[1] + 1.0).abs() < 1e-6);

        // The two differ, which is the whole reason the target extent has to win.
        assert_ne!(logical[0], physical[0]);
        assert_ne!(logical[5], physical[5]);
    }
}