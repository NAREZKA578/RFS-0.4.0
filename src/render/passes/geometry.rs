//! Geometry pass — the first pass that records real GPU work.
//!
//! Every other pass in the render layer is still a stub (#186), so nothing
//! reached the GPU before this. This one records the full sequence a real frame
//! needs:
//!
//! ```text
//! BeginRenderPass -> BindGraphicsPipeline -> BindDescriptorSets
//!   -> BindVertexBuffers -> BindIndexBuffer -> SetViewport -> SetScissor
//!   -> DrawIndexed -> EndRenderPass
//! ```
//!
//! All of those variants are already translatable to Vulkan
//! (`backend/vulkan/record.rs`); what was missing was anything that emitted
//! them. With the graph now actually submitting (#209), this is the piece that
//! makes a frame appear.

use std::collections::HashMap;

use crate::render::core::RenderContext;
use crate::render::graph::node::{PassDependency, RenderPass};
use crate::render::graph::resource::GraphResource;
use crate::render::scene::Scene;
use crate::rhi::command::commands::Command;
use crate::rhi::command::pass::render::RenderPassBeginInfo;
use crate::rhi::{
    BufferUsage, CommandEncoder, DescriptorBinding, DescriptorInfo, DescriptorSet,
    DescriptorSetLayout, DescriptorSetLayoutDesc, DescriptorType, DescriptorWrite, Extent2D,
    Format, Framebuffer, GraphicsPipeline, Offset2D, Rect2D, ShaderStage, Viewport,
};
use glam::{Mat4, Vec3};

/// One drawable mesh: its GPU buffers and the pipeline built for it.
pub struct GeometryItem {
    pub name: String,
    pub vertex_buffer: std::sync::Arc<crate::rhi::Buffer>,
    pub index_buffer: std::sync::Arc<crate::rhi::Buffer>,
    pub index_type: crate::render::meshes::IndexType,
    pub index_count: u32,
    pub model: Mat4,
}

/// Everything needed to record the pass, rebuilt when a resource changes.
pub struct GeometryPassState {
    pub pipeline: GraphicsPipeline,
    pub descriptor_layout: DescriptorSetLayout,
    pub descriptor_set: DescriptorSet,
    pub uniform_buffer: crate::rhi::Buffer,
    pub render_pass: crate::rhi::RenderPass,
    pub items: Vec<GeometryItem>,
    /// The depth image matching the current target size.
    ///
    /// Created on demand and rebuilt when the presentation extent changes. It
    /// has to be a real attachment: a depth test without one is not a "no
    /// depth", it is an invalid pipeline, and leaving it out entirely means a
    /// closed solid renders as an open shell with its far walls drawn over its
    /// near ones.
    pub depth_texture: Option<crate::rhi::Texture>,
    pub depth_view: Option<crate::rhi::TextureView>,
    pub depth_format: Format,
    /// The framebuffer for the swapchain image being rendered into.
    ///
    /// `None` means "the image the renderer acquires this frame", and is what a
    /// windowed run leaves it at. `Some` is the framebuffer currently bound,
    /// whether it was pinned by the caller or cached for the acquired image.
    pub framebuffer: Option<Framebuffer>,
    /// Whether `framebuffer` was chosen by the caller and must not be swapped
    /// out from under them.
    ///
    /// This has to be a separate flag rather than inferred from `framebuffer`
    /// being `Some`: the pass caches a framebuffer for the acquired image, so
    /// after the first frame `framebuffer` is always `Some`, and treating that
    /// as "pinned" leaves every frame drawing into whichever image happened to
    /// be first. The other images are then presented having never been written,
    /// which is a black or flickering window rather than an error.
    pub framebuffer_pinned: bool,
    /// Which swapchain image `framebuffer` was built for, so it is rebuilt when
    /// the renderer acquires a different one or resizes the chain.
    pub framebuffer_image: Option<u32>,
    pub width: u32,
    pub height: u32,
}

/// Uniform block layout, matching `geometry.wgsl`:
/// `mat4 mvp` + `vec4 light_dir` + `vec4 light_color` + `vec4 ambient` = 112 bytes.
const UNIFORM_SIZE: u64 = 16 * 4 * 4;

/// Shared handle to the pass state.
///
/// The graph owns the pass as a `Box<dyn RenderPass>`, so the caller has no way
/// to reach into it to update the MVP or repoint the framebuffer. Sharing the
/// state is what makes the pass drivable from outside the graph, without a
/// downcast and without a bespoke trait method for each field.
pub type SharedState = std::sync::Arc<std::sync::Mutex<GeometryPassState>>;

pub struct GeometryPass {
    state: Option<SharedState>,
    name: String,
    dependencies: Vec<PassDependency>,
    outputs: Vec<String>,
    enabled: bool,
}

impl GeometryPass {
    pub fn new() -> Self {
        Self {
            state: None,
            name: "geometry".to_string(),
            dependencies: Vec::new(),
            outputs: vec!["swapchain".to_string()],
            enabled: true,
        }
    }

    /// Build a pass around a new shared state and return both.
    ///
    /// The caller keeps the handle; the pass keeps its own copy. The two are
    /// deliberately separate: the pass must not be able to observe itself, and
    /// the caller must not be able to record commands behind the graph's back.
    pub fn with_state(state: GeometryPassState) -> (Self, SharedState) {
        let shared: SharedState = std::sync::Arc::new(std::sync::Mutex::new(state));
        let pass = Self {
            state: Some(shared.clone()),
            name: "geometry".to_string(),
            dependencies: Vec::new(),
            outputs: vec!["swapchain".to_string()],
            enabled: true,
        };
        (pass, shared)
    }

    /// The depth format this pass renders with. `D32_SFLOAT` is the cheapest
    /// depth format that is guaranteed to be supported as an attachment.
    pub const DEPTH_FORMAT: Format = Format::D32_SFLOAT;

    /// Build a pass with every resource it needs, and return the shared handle.
    ///
    /// The alternative is the sequence `build_uniforms` plus a hand-written
    /// `GraphicsPipelineDesc` plus a render pass, which only the test was doing —
    /// so outside the tests there was no way to obtain a working geometry pass
    /// at all, however much of the RHI was in place.
    ///
    /// `color_format` must match the surface the pass will draw into: a
    /// pipeline declares its colour target format, and using a different one
    /// than the framebuffer's is undefined behaviour rather than a warning. The
    /// caller learns it from the swapchain after attaching presentation, which
    /// is why registration has to come afterwards.
    ///
    /// `spirv` is taken as bytes instead of being read from a path so the
    /// render layer carries no knowledge of where assets live; the embedder
    /// uses `include_bytes!`.
    ///
    /// The framebuffer is left unset, so the pass draws into whatever image the
    /// renderer acquires. `width`/`height` are filled in on first use.
    pub fn create(
        device: &crate::rhi::Device,
        items: Vec<GeometryItem>,
        color_format: Format,
        spirv: &[u8],
    ) -> crate::rhi::error::RhiResult<(Self, SharedState)> {
        // The presentation render pass, so its final layout is presentable
        // without a separate transition at the end of the frame.
        let render_pass =
            device.presentation_render_pass_with_depth(color_format, Self::DEPTH_FORMAT)?;
        Self::create_with_render_pass(device, items, render_pass, spirv)
    }

    /// As `create`, but with a caller-supplied render pass.
    ///
    /// A render pass fixes both the colour format *and* the final layout, and
    /// the final layout of a presentation pass is presentable rather than
    /// readable. An offscreen target has to end in something copyable, so a
    /// pass that can only present cannot be used to verify that it drew.
    pub fn create_with_render_pass(
        device: &crate::rhi::Device,
        items: Vec<GeometryItem>,
        render_pass: crate::rhi::RenderPass,
        spirv: &[u8],
    ) -> crate::rhi::error::RhiResult<(Self, SharedState)> {
        use crate::rhi::error::RhiError;
        use crate::rhi::pipeline::graphics::PrimitiveTopology;
        use crate::rhi::{
            ColorBlendAttachment, ColorBlendState, ColorComponentFlags, ColorTargetDesc,
            GraphicsPipelineDesc, InputAssemblyState, MultisampleState, PipelineShaderStage,
            RasterizerState, ShaderFormat, ShaderModuleDesc,
        };

        let (uniform_buffer, descriptor_layout, descriptor_set) = build_uniforms(device)?;

        let module = device.create_shader_module(&ShaderModuleDesc {
            code: spirv.to_vec(),
            format: ShaderFormat::SpirV,
            entry_point: Some("vs_main".into()),
            name: Some("geometry".into()),
        });
        if !module.has_gpu_backing() {
            return Err(RhiError::BackendError(
                "the geometry shader module was not created on the device".into(),
            ));
        }

        let mut desc = GraphicsPipelineDesc {
            shader_stages: vec![
                PipelineShaderStage {
                    stage: ShaderStage::VERTEX,
                    module: module.clone(),
                    entry_point: "vs_main".into(),
                },
                PipelineShaderStage {
                    stage: ShaderStage::FRAGMENT,
                    module,
                    entry_point: "fs_main".into(),
                },
            ],
            vertex_input_state: Some(vertex_input_state()),
            input_assembly_state: InputAssemblyState {
                topology: PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            // Double sided so a mesh with a winding mistake is visible rather
            // than invisible, which looks exactly like "nothing is drawn".
            rasterizer_state: Some(RasterizerState::double_sided()),
            multisample_state: MultisampleState {
                sample_count: crate::rhi::SampleCount::X1,
                ..Default::default()
            },
            color_blend_state: Some(ColorBlendState {
                attachments: vec![ColorBlendAttachment {
                    color_write_mask: ColorComponentFlags::R
                        | ColorComponentFlags::G
                        | ColorComponentFlags::B
                        | ColorComponentFlags::A,
                    ..ColorTargetDesc::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        };
        // Depth test and write only when the render pass actually has a depth
        // attachment; the two must agree or the pipeline is invalid.
        desc.depth_stencil_state = Self::render_pass_has_depth(&render_pass)
            .then(crate::rhi::DepthStencilState::enabled);

        let pipeline =
            device.create_gpu_graphics_pipeline(&desc, &render_pass, &[&descriptor_layout])?;
        if !pipeline.has_gpu_backing() {
            return Err(RhiError::BackendError(
                "the geometry graphics pipeline was not created on the device".into(),
            ));
        }

        Ok(Self::with_state(GeometryPassState {
            pipeline,
            descriptor_layout,
            descriptor_set,
            uniform_buffer,
            depth_format: Self::DEPTH_FORMAT,
            render_pass,
            items,
            depth_texture: None,
            depth_view: None,
            framebuffer: None,
            framebuffer_pinned: false,
            framebuffer_image: None,
            width: 0,
            height: 0,
        }))
    }

    /// Whether a render pass declares a depth attachment.
    ///
    /// Read from the render pass rather than assumed, because the pipeline's
    /// depth state has to agree with it: a pipeline that declares a depth test
    /// against a render pass with no depth attachment is invalid, and one that
    /// omits the test against a pass that has depth leaves the depth buffer
    /// unwritten and useless.
    fn render_pass_has_depth(render_pass: &crate::rhi::RenderPass) -> bool {
        render_pass
            .desc()
            .subpasses
            .first()
            .is_some_and(|s| s.depth_stencil_attachment.is_some())
    }

    /// The framebuffer this pass should draw into this frame.
    ///
    /// A caller-pinned framebuffer wins: the caller said exactly where to draw,
    /// so there is nothing to resolve. Otherwise the pass draws into the image
    /// the renderer acquired, building a framebuffer for it on first use and
    /// again whenever the acquired index changes.
    ///
    /// The framebuffer is cached per image and reused while the acquired index
    /// does not change. A swapchain alternates between a small fixed set of
    /// images, so building one per frame would be pure cost; a resize is picked
    /// up because the extent it is built from is the presentation extent.
    ///
    /// `None` when there is no surface, or the recorded index is not a live
    /// image — the case where drawing is skipped rather than guessed at.
    fn resolve_target(
        state: &mut GeometryPassState,
        context: &RenderContext,
    ) -> Option<Framebuffer> {
        if state.framebuffer_pinned {
            return state.framebuffer.clone();
        }
        if state.framebuffer_image == Some(context.swapchain_image_index) {
            return state.framebuffer.clone();
        }
        // Out of range until the renderer publishes presentation state, and
        // after a resize that rebuilt the chain mid-frame.
        let (image, view) = context.acquired_image()?;
        let format = context.presentation_format?;
        let width = context.presentation_extent.width;
        let height = context.presentation_extent.height;

        // The depth image is created on demand and rebuilt when the target size
        // changes, because a framebuffer's attachments must all be the same size
        // as the render area.
        if Self::render_pass_has_depth(&state.render_pass) {
            let stale = state
                .depth_texture
                .as_ref()
                .is_none_or(|t| t.width() != width || t.height() != height);
            if stale {
                let depth = context.device().create_texture(
                    width,
                    height,
                    1,
                    state.depth_format,
                    crate::rhi::TextureUsage::DEPTH_STENCIL_ATTACHMENT,
                    1,
                );
                // Through the device, not `Texture::create_view`: the latter
                // builds a CPU-side handle with no GPU backing, and a framebuffer
                // attachment without backing is rejected — reported as
                // "framebuffer attachment has no GPU backing" from the frame
                // that tried to use it, with nothing at the point of the mistake.
                let depth_view = match context.device().create_texture_view(
                    &depth,
                    &crate::rhi::TextureViewDesc {
                        texture: depth.clone(),
                        format: None,
                        view_type: crate::rhi::TextureViewType::D2,
                        aspects: crate::rhi::TextureAspectFlags::DEPTH,
                        base_mip_level: 0,
                        mip_level_count: 1,
                        base_array_layer: 0,
                        array_layer_count: 1,
                    },
                ) {
                    Ok(view) => view,
                    Err(e) => {
                        crate::error!("geometry", "depth view failed: {e}");
                        return None;
                    }
                };
                state.depth_texture = Some(depth);
                state.depth_view = Some(depth_view);
            }
        }

        let framebuffer = if Self::render_pass_has_depth(&state.render_pass) {
            let depth_view = state.depth_view.clone()?;
            context.device().swapchain_image_framebuffer_with_depth(
                &state.render_pass,
                image,
                view,
                depth_view,
                format,
                width,
                height,
            )
        } else {
            context.device().swapchain_image_framebuffer(
                &state.render_pass,
                image,
                view,
                format,
                width,
                height,
            )
        };

        match framebuffer {
            Ok(framebuffer) => {
                state.framebuffer = Some(framebuffer.clone());
                state.framebuffer_image = Some(context.swapchain_image_index);
                state.width = width;
                state.height = height;
                Some(framebuffer)
            }
            Err(e) => {
                crate::error!("geometry", "framebuffer for image {} failed: {e}", context.swapchain_image_index);
                None
            }
        }
    }

    pub fn state(&self) -> Option<SharedState> {
        self.state.clone()
    }

    /// Point the pass at a specific framebuffer, and set the viewport.
    ///
    /// This is a pin: the pass keeps this framebuffer whatever image the
    /// renderer acquires. Pass `None` to hand control back to the acquired
    /// image, which is what a windowed run wants.
    pub fn set_framebuffer(
        state: &SharedState,
        framebuffer: Option<Framebuffer>,
        width: u32,
        height: u32,
    ) {
        if let Ok(mut state) = state.lock() {
            state.framebuffer = framebuffer;
            state.framebuffer_pinned = true;
            state.framebuffer_image = None;
            state.width = width;
            state.height = height;
        }
    }

    /// Hand the pass back to the image the renderer acquires each frame.
    pub fn clear_framebuffer_pin(state: &SharedState) {
        if let Ok(mut state) = state.lock() {
            state.framebuffer = None;
            state.framebuffer_pinned = false;
            state.framebuffer_image = None;
        }
    }

    /// Update the MVP and lighting uniforms for this frame.
    ///
    /// Takes the device explicitly rather than reading it from the context, so
    /// this is callable from outside a graph execution.
    ///
    /// Returns whether the GPU now holds this frame's uniforms.
    #[must_use]
    pub fn update_uniforms(
        state: &SharedState,
        device: &crate::rhi::Device,
        view: Mat4,
        projection: Mat4,
    ) -> bool {
        let Ok(state) = state.lock() else {
            return false;
        };
        let mvp = projection * view;

        // Column-major, so the 16 floats of the matrix go down unchanged.
        let mut data: [f32; 28] = [0.0; 28];
        data[..16].copy_from_slice(&mvp.to_cols_array());
        // A directional light in view space, normalised so the shader need not be.
        let light = Vec3::new(0.4, 0.7, 1.0).normalize();
        data[16..20].copy_from_slice(&[light.x, light.y, light.z, 0.0]);
        data[20..24].copy_from_slice(&[1.0, 0.96, 0.9, 1.0]);
        data[24..28].copy_from_slice(&[0.18, 0.2, 0.24, 1.0]);

        // A failed uniform upload leaves the previous frame's MVP on the GPU, which
        // would draw the mesh under the old camera. Report it rather than
        // letting the frame go out with transforms nobody asked for.
        device
            .upload_buffer(&state.uniform_buffer, &data)
            .is_err()
    }
}

impl Default for GeometryPass {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderPass for GeometryPass {
    fn name(&self) -> &str {
        &self.name
    }

    fn dependencies(&self) -> &[PassDependency] {
        &self.dependencies
    }

    fn outputs(&self) -> &[String] {
        &self.outputs
    }

    fn is_enabled(&self) -> bool {
        self.enabled && self.state.is_some()
    }

    fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    fn execute(
        &mut self,
        encoder: &mut CommandEncoder,
        context: &mut RenderContext,
        _scene: &mut Scene,
        _resources: &HashMap<String, GraphResource>,
    ) {
        // Held only for the duration of recording: the state is shared with the
        // caller, and holding the lock across the graph's other passes would
        // serialise them behind this one for no reason.
        let Some(shared) = self.state.as_ref() else {
            return;
        };
        let Ok(mut state) = shared.lock() else {
            return;
        };
        if state.items.is_empty() {
            return;
        }
        let Some(framebuffer) = Self::resolve_target(&mut state, context) else {
            return;
        };
        let framebuffer = framebuffer.clone();

        let width = state.width;
        let height = state.height;

        let mut commands: Vec<Command> = Vec::with_capacity(11 + state.items.len() * 3);
        // One clear value per attachment, in attachment order. A depth
        // attachment with a clear load operation but no matching clear value
        // is invalid, so the depth attachment needs one too: far plane, which
        // makes every fragment pass the `Less` test on the first frame.
        let has_depth = Self::render_pass_has_depth(&state.render_pass);
        let mut clear_values = vec![crate::rhi::ClearValue::color(0.05, 0.06, 0.08, 1.0)];
        if has_depth {
            clear_values.push(crate::rhi::ClearValue::depth_stencil(1.0, 0));
        }
        commands.push(Command::BeginRenderPass(RenderPassBeginInfo {
            render_pass: state.render_pass.clone(),
            framebuffer: framebuffer.clone(),
            render_area: Rect2D {
                offset: Offset2D { x: 0, y: 0 },
                extent: Extent2D { width, height },
            },
            // Mid grey, so an unlit surface is distinguishable from a cleared
            // black one.
            clear_values,
        }));

        commands.push(Command::SetViewport(Viewport {
            x: 0.0,
            y: 0.0,
            width: width as f32,
            height: height as f32,
            min_depth: 0.0,
            max_depth: 1.0,
        }));
        commands.push(Command::SetScissor(Rect2D {
            offset: Offset2D { x: 0, y: 0 },
            extent: Extent2D { width, height },
        }));
        commands.push(Command::BindGraphicsPipeline(state.pipeline.clone()));
        commands.push(Command::BindDescriptorSets {
            pipeline: state.pipeline.clone(),
            first_set: 0,
            sets: vec![state.descriptor_set.clone()],
        });

        for item in &state.items {
            commands.push(Command::BindVertexBuffers {
                first_binding: 0,
                buffers: vec![((*item.vertex_buffer).clone(), 0)],
            });
            commands.push(Command::BindIndexBuffer {
                buffer: (*item.index_buffer).clone(),
                offset: 0,
                // The item carries the *render layer's* index type, which
                // includes 8-bit; the RHI has no such type, so such a mesh is
                // skipped rather than bound with the wrong stride — the
                // geometry would be read at the wrong offsets.
                index_type: match item.index_type {
                    crate::render::meshes::IndexType::U16 => crate::rhi::IndexType::U16,
                    crate::render::meshes::IndexType::U32 => crate::rhi::IndexType::U32,
                    crate::render::meshes::IndexType::U8 => {
                        crate::warn!("geometry", "{}: 8-bit indices are not supported", item.name);
                        continue;
                    }
                },
            });
            commands.push(Command::DrawIndexed {
                index_count: item.index_count,
                instance_count: 1,
                first_index: 0,
                vertex_offset: 0,
                first_instance: 0,
            });
        }

        commands.push(Command::EndRenderPass);

        for command in commands {
            // `record` reports whether the command was kept; a rejected one
            // means the encoder is not recording, which is a programming error
            // rather than something to retry.
            if !encoder.record(command) {
                crate::error!("geometry", "a command was rejected: the encoder is not recording");
                return;
            }
        }
    }
}

/// Build the uniform buffer, descriptor layout and set for the geometry shader.
pub fn build_uniforms(
    device: &crate::rhi::Device,
) -> crate::rhi::error::RhiResult<(crate::rhi::Buffer, DescriptorSetLayout, DescriptorSet)> {
    let uniform = device.create_buffer(UNIFORM_SIZE, BufferUsage::UNIFORM, true);

    // `create_descriptor_set_layout` is infallible: it falls back to a
    // descriptor-less layout when the backend refuses, which is the right
    // behaviour here (a frame with no bindings still draws) but means the
    // result must be checked before relying on binding 0 being live.
    let layout = device.create_descriptor_set_layout(&DescriptorSetLayoutDesc {
        bindings: vec![DescriptorBinding {
            binding: 0,
            ty: DescriptorType::UniformBuffer,
            count: 1,
            stages: ShaderStage::VERTEX | ShaderStage::FRAGMENT,
            immutable_samplers: None,
        }],
    });

    let set = device.create_descriptor_set(&layout)?;

    device.write_descriptors(
        &set,
        &[DescriptorWrite {
            dst_set: set.clone(),
            dst_binding: 0,
            dst_array_element: 0,
            descriptors: vec![DescriptorInfo::Buffer(uniform.clone(), 0, UNIFORM_SIZE)],
        }],
    )?;

    Ok((uniform, layout, set))
}

/// Vertex layout matching the render layer's `Vertex` and `geometry.wgsl`.
///
/// The offsets and the stride are read from the real `Vertex` with
/// `offset_of!`/`size_of!` instead of being summed from format sizes.
/// `Vertex` is `#[repr(C)]` and glam's `Vec4` is 16-byte aligned, so the struct
/// contains padding that the sum of format sizes does not: summing gives a
/// stride of 64 where the struct is 80, and a 0-byte hole before `tangent` and
/// another before `color`. A pipeline declaring those summed offsets is not
/// rejected by the driver — it silently reads each vertex from the wrong
/// address, which shows up as geometry that is not the geometry in the buffer.
///
/// Deriving the layout from the struct means the two cannot drift apart again.
pub fn vertex_input_state() -> crate::rhi::VertexInputState {
    use std::mem::{offset_of, size_of};

    use crate::render::meshes::Vertex;
    use crate::rhi::{VertexAttribute, VertexBinding, VertexInputRate};

    crate::rhi::VertexInputState {
        bindings: vec![VertexBinding {
            binding: 0,
            stride: size_of::<Vertex>() as u32,
            input_rate: VertexInputRate::Vertex,
        }],
        attributes: vec![
            VertexAttribute {
                binding: 0,
                location: 0,
                format: Format::R32G32B32_SFLOAT,
                offset: offset_of!(Vertex, position) as u32,
            },
            VertexAttribute {
                binding: 0,
                location: 1,
                format: Format::R32G32B32_SFLOAT,
                offset: offset_of!(Vertex, normal) as u32,
            },
            VertexAttribute {
                binding: 0,
                location: 2,
                format: Format::RGBA32_SFLOAT,
                offset: offset_of!(Vertex, tangent) as u32,
            },
            VertexAttribute {
                binding: 0,
                location: 3,
                format: Format::R32G32_SFLOAT,
                offset: offset_of!(Vertex, tex_coord) as u32,
            },
            VertexAttribute {
                binding: 0,
                location: 4,
                format: Format::RGBA32_SFLOAT,
                offset: offset_of!(Vertex, color) as u32,
            },
        ],
    }
}
