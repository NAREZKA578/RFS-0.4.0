//! Shadow Render Pass
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//!
//! The pass renders the scene from the light's point of view into a depth
//! image: the shadow map. It is the first half of shadows, and the half that
//! can exist on its own — the map is a real image with the scene's depth in
//! it, and the test reads that image back and checks the geometry is there.
//!
//! What this pass used to be: a set of CPU-side descriptions with no native
//! handle (`Texture::new`, `RenderPass::new`, `Framebuffer::new` — the
//! translator refuses all three), a pipeline that was `None` with the comment
//! "this would require a shadow shader" — the shader is `shadow.spv` and has
//! been there all along — and an `execute` that opened a render pass and
//! closed it again without drawing anything. It reported itself as having
//! recorded work, because recording an empty pass is recording.
//!
//! The geometry is the scene's, so the pass takes the same items the geometry
//! pass draws, and each item gets its own uniform block: the light's matrix
//! and the item's model matrix. One uniform for the whole pass would draw
//! every item with the last one's matrix, which is the mistake the geometry
//! pass already made once (№654).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use glam::{Mat4, Vec3, Vec4};

use crate::render::core::RenderContext;
use crate::render::graph::node::{PassDependency, PassOutcome, RenderPass};
use crate::render::graph::resource::GraphResource;
use crate::render::passes::base::BaseRenderPass;
use crate::render::passes::geometry::GeometryItem;
use crate::render::scene::Scene;
use crate::rhi::{
    AttachmentDescription, AttachmentReference, Command, CommandEncoder, DescriptorInfo,
    DescriptorSet, DescriptorSetLayout, DescriptorWrite, Framebuffer, FramebufferAttachment,
    FramebufferDesc, GraphicsPipeline, Offset2D, Rect2D, RenderPassDesc, SubpassDescription,
};

/// The shadow map's depth format.
///
/// `D32_SFLOAT`: a depth format the RHI can both write as an attachment and
/// read as a sampled image — the second half of that only became possible
/// when the RHI learned to describe a depth view with the depth layout and to
/// refuse a filtering sampler over one (№683).
pub const SHADOW_FORMAT: crate::rhi::Format = crate::rhi::Format::D32_SFLOAT;

/// The uniform block, matching `shadow.wgsl`:
/// `mat4 light_space_matrix` + `mat4 model` = 128 bytes.
const UNIFORM_SIZE: u64 = 16 * 4 * 2;

/// The name of the graph's resource holding the shadow map.
pub const SHADOW_MAP_RESOURCE: &str = "shadow_map";

/// Shadow map configuration.
#[derive(Debug, Clone)]
pub struct ShadowConfig {
    /// The size of the shadow map, in pixels.
    ///
    /// The square, because a shadow map is a projection of the light's view
    /// and a square one keeps the projection's aspect at 1 — the same texture
    /// sampled by pixels that are not square would need the aspect corrected
    /// twice, once in the projection and once in the lookup.
    pub resolution: u32,
    /// How many maps the light gets.
    ///
    /// One for a point light: its shadow is a perspective frustum, and the
    /// whole thing fits in one map.
    ///
    /// Several for a **directional** light: the shadow of a distant sun is
    /// parallel, and one map covering everything would spend its resolution
    /// on the far plane, where the shadow is a few pixels wide. So the
    /// camera's frustum is cut into slices by distance, and each slice gets
    /// its own map, its own orthographic projection and its own resolution.
    /// That is what a cascade is.
    pub cascade_count: usize,
    /// The distances at which the frustum is cut.
    ///
    /// `cascade_distances[i]` is where the slice ends, so the slice `i` runs
    /// from `cascade_distances[i - 1]` to `cascade_distances[i]`, and the
    /// first slice runs from the near plane. The distances must increase.
    pub cascade_distances: Vec<f32>,
    /// The depth bias, in the shadow map's depth units.
    ///
    /// The shadow of a surface is compared with the depth of the surface
    /// itself, and the two are the same number for a surface that shadows
    /// itself. Without a bias the surface shadows itself, and the whole
    /// scene is black.
    pub bias: f32,
    /// The normal bias: how far the lookup is pushed along the surface
    /// normal, in the same units as the depth.
    pub normal_bias: f32,
}

impl Default for ShadowConfig {
    fn default() -> Self {
        Self {
            resolution: 1024,
            cascade_count: 1,
            cascade_distances: vec![10.0, 50.0, 100.0, 200.0],
            bias: 0.001,
            normal_bias: 0.01,
        }
    }
}

/// The light's view-projection for a point light at `light_position`.
///
/// A point light has no direction, so its shadow map is a perspective frustum
/// from the light towards the target, and the shadow map is that frustum's
/// depth. A directional light has a direction and no position, and its map is
/// an orthographic projection of the same volume — that is the case the
/// cascades belong to, and it is not implemented here.
///
/// The frustum is 90 degrees wide, which is a cube: the map is square, and
/// the projection's aspect is 1, so the texture's texels stay square. A
/// frustum narrower than a cube would waste the map's corners, and wider
/// would push the resolution where it is needed least.
pub fn light_view_projection(light_position: Vec3, target: Vec3, distance: f32) -> Mat4 {
    let view = Mat4::look_at_rh(light_position, target, Vec3::Y);
    let projection = Mat4::perspective_rh(
        std::f32::consts::FRAC_PI_2,
        1.0,
        0.1,
        distance.max(1.0),
    );
    projection * view
}

/// The light's matrices, one per cascade.
///
/// A directional light's shadow is parallel, so one map for the whole scene
/// would spend its resolution on the far plane, where the shadow is a few
/// pixels wide. So the camera's frustum is cut into slices by distance, and
/// each slice gets its own orthographic map: the near slice covers a small
/// volume in great detail, the far one a large volume in less.
///
/// The slices are given by `splits`: the distances at which the frustum is
/// cut. The first slice runs from `near` to `splits[0]`, the second from
/// `splits[0]` to `splits[1]`, and the last to `far`.
///
/// The function is pure and does no GPU work, so the geometry of the slices
/// can be checked without a device.
pub fn cascade_matrices(
    view: Mat4,
    projection: Mat4,
    light_direction: Vec3,
    near: f32,
    far: f32,
    splits: &[f32],
) -> Vec<Mat4> {
    let view_projection = projection * view;
    let inverse = view_projection.inverse();
    let direction = light_direction.normalize();
    // The up vector has to be perpendicular to the light's direction, or the
    // light's view is degenerate. A light pointing straight up or down is the
    // case where the obvious up vector fails.
    let up = if direction.dot(Vec3::Y).abs() > 0.99 {
        Vec3::Z
    } else {
        Vec3::Y
    };

    // The depth of a point `d` units away, in the clip space's z.
    //
    // The projection is not linear in the depth, so a slice's ends have to be
    // found in the clip space and not in the view space. Getting this wrong
    // puts the slice's far end in the wrong place, and the shadows in the
    // slice do not line up with the geometry.
    let depth_in_clip = |d: f32| (far + near) / (far - near) - (2.0 * far * near) / ((far - near) * d);

    let mut matrices = Vec::new();
    let mut slice_near = near;
    for &slice_far in splits {
        if slice_far <= slice_near {
            // A slice that does not grow is a slice that is not there.
            slice_near = slice_far;
            continue;
        }
        // The slice's corners in the world: the frustum's, at the slice's two
        // depths.
        let mut minimum = Vec3::splat(f32::INFINITY);
        let mut maximum = Vec3::splat(f32::NEG_INFINITY);
        for depth in [slice_near, slice_far] {
            let z = depth_in_clip(depth);
            for x in [-1.0f32, 1.0] {
                for y in [-1.0f32, 1.0] {
                    let clip = inverse * Vec4::new(x, y, z, 1.0);
                    let point = Vec3::new(clip.x, clip.y, clip.z) / clip.w;
                    minimum = minimum.min(point);
                    maximum = maximum.max(point);
                }
            }
        }
        // The slice's bounding sphere: a box would rotate with the light and
        // shimmer as the light moves, while a sphere does not.
        let centre = (minimum + maximum) * 0.5;
        let radius = (maximum - minimum).length() * 0.5;

        // The light's view of that sphere, and the orthographic projection of
        // it. The light is placed far enough back that the whole sphere is in
        // front of it.
        let eye = centre - direction * (radius * 2.0 + 1.0);
        let light_view = Mat4::look_at_rh(eye, centre, up);
        let light_projection = Mat4::orthographic_rh(
            -radius,
            radius,
            -radius,
            radius,
            0.1,
            radius * 4.0 + 2.0,
        );
        matrices.push(light_projection * light_view);
        slice_near = slice_far;
    }
    matrices
}

/// Everything the pass needs to record, shared with the caller.
pub struct ShadowPassState {
    pub pipeline: GraphicsPipeline,
    pub descriptor_layout: DescriptorSetLayout,
    /// One descriptor set per item, in the same order as `items`.
    pub descriptor_sets: Vec<DescriptorSet>,
    /// One uniform buffer per item, in the same order as `items`.
    pub uniform_buffers: Vec<crate::rhi::Buffer>,
    pub items: Vec<GeometryItem>,
    pub render_pass: crate::rhi::RenderPass,
    /// The shadow maps, one per cascade.
    ///
    /// A point light gets one; a directional light gets as many as the
    /// frustum is cut into.
    pub shadow_maps: Vec<crate::rhi::Texture>,
    /// The views of the shadow maps, which the framebuffers are built on.
    pub shadow_views: Vec<crate::rhi::TextureView>,
    /// One framebuffer per cascade.
    pub framebuffers: Vec<Framebuffer>,
    pub config: ShadowConfig,
    /// The light's matrix per cascade, kept so the per-item uniforms can be
    /// refreshed without the caller passing them again.
    pub light_view_projections: Vec<Mat4>,
}

/// Shared handle to the pass state.
pub type SharedState = Arc<Mutex<ShadowPassState>>;

/// Shadow pass for a single light.
pub struct ShadowPass {
    base: BaseRenderPass,
    state: Option<SharedState>,
}

impl ShadowPass {
    /// Build a pass with no GPU objects yet.
    ///
    /// The GPU objects need a device, and the device is not always available
    /// when the pass is built; [`ShadowPass::create`] is the form that has
    /// one.
    pub fn new() -> Self {
        let mut base = BaseRenderPass::new("shadow");
        base.add_output(SHADOW_MAP_RESOURCE);
        Self {
            base,
            state: None,
        }
    }

    /// Build a pass around a new shared state and return both.
    pub fn with_state(state: ShadowPassState) -> (Self, SharedState) {
        let mut base = BaseRenderPass::new("shadow");
        base.add_output(SHADOW_MAP_RESOURCE);
        let shared: SharedState = Arc::new(Mutex::new(state));
        let pass = Self {
            base,
            state: Some(shared.clone()),
        };
        (pass, shared)
    }

    /// Build a shadow pass with real GPU objects.
    ///
    /// `items` are the scene's meshes — the same ones the geometry pass
    /// draws, cloned — because the shadow map has to contain the geometry,
    /// and the geometry is the only thing that knows where it is.
    pub fn create(
        device: &crate::rhi::Device,
        items: Vec<GeometryItem>,
        config: ShadowConfig,
        spirv: &[u8],
    ) -> crate::rhi::error::RhiResult<(Self, SharedState)> {
        use crate::rhi::error::RhiError;
        use crate::rhi::pipeline::graphics::PrimitiveTopology;
        use crate::rhi::{
            DepthStencilState, GraphicsPipelineDesc, InputAssemblyState, MultisampleState,
            PipelineShaderStage, RasterizerState, ShaderFormat, ShaderModuleDesc,
        };

        let resolution = config.resolution.max(1);
        let mut config = config;
        config.resolution = resolution;

        // The shadow maps: depth images that are written as attachments and
        // read as textures. `TRANSFER_SRC` so a test can read them back — the
        // only honest way to check that the pass drew anything is to look at
        // what it drew.
        //
        // One per cascade. A point light gets one; a directional light gets
        // as many as the frustum is cut into.
        let cascade_count = config.cascade_count.max(1);
        config.cascade_count = cascade_count;
        let mut shadow_maps = Vec::with_capacity(cascade_count);
        let mut shadow_views = Vec::with_capacity(cascade_count);
        for _ in 0..cascade_count {
            let shadow_map = device.create_texture(
                resolution,
                resolution,
                1,
                SHADOW_FORMAT,
                crate::rhi::TextureUsage::DEPTH_STENCIL_ATTACHMENT
                    | crate::rhi::TextureUsage::SAMPLED
                    | crate::rhi::TextureUsage::TRANSFER_SRC,
                1,
            );
            if !shadow_map.has_gpu_backing() {
                return Err(RhiError::BackendError(
                    "the shadow map texture was not created on the device".into(),
                ));
            }
            let shadow_view = device
                .create_texture_view(
                    &shadow_map,
                    &crate::rhi::TextureViewDesc {
                        texture: shadow_map.clone(),
                        format: None,
                        view_type: crate::rhi::TextureViewType::D2,
                        aspects: crate::rhi::TextureAspectFlags::DEPTH,
                        base_mip_level: 0,
                        mip_level_count: 1,
                        base_array_layer: 0,
                        array_layer_count: 1,
                    },
                )
                .map_err(|e| RhiError::BackendError(format!("the shadow map view: {e}")))?;
            shadow_maps.push(shadow_map);
            shadow_views.push(shadow_view);
        }

        // The render pass: one depth attachment and no colour at all. A pass
        // that wrote colour as well would need a colour target, and the
        // shadow map is not a colour image.
        let render_pass = device
            .create_gpu_render_pass(&RenderPassDesc {
                attachments: vec![AttachmentDescription {
                    format: SHADOW_FORMAT,
                    samples: crate::rhi::SampleCount::X1,
                    load_op: crate::rhi::LoadOp::Clear,
                    store_op: crate::rhi::StoreOp::Store,
                    stencil_load_op: crate::rhi::LoadOp::DontCare,
                    stencil_store_op: crate::rhi::StoreOp::DontCare,
                    initial_layout: crate::rhi::TextureLayout::Undefined,
                    // Read-only from here on: the lighting shader samples the
                    // map, and the layout it is sampled in is the one the
                    // descriptor names.
                    final_layout: crate::rhi::TextureLayout::DepthStencilReadOnlyOptimal,
                }],
                subpasses: vec![SubpassDescription {
                    pipeline_bind_point: crate::rhi::PipelineBindPoint::Graphics,
                    color_attachments: vec![],
                    depth_stencil_attachment: Some(AttachmentReference {
                        attachment: 0,
                        layout: crate::rhi::TextureLayout::DepthStencilAttachmentOptimal,
                    }),
                    ..Default::default()
                }],
                dependencies: vec![],
            })
            .map_err(|e| RhiError::BackendError(format!("the shadow render pass: {e}")))?;

        // One framebuffer per cascade: each map is its own attachment, and a
        // framebuffer names exactly one view per attachment.
        let mut framebuffers = Vec::with_capacity(cascade_count);
        for view in shadow_views.iter() {
            let framebuffer = device
                .create_gpu_framebuffer(&FramebufferDesc {
                    render_pass: render_pass.clone(),
                    attachments: vec![FramebufferAttachment {
                        texture_view: view.clone(),
                        layer: 0,
                        mip_level: 0,
                    }],
                    width: resolution,
                    height: resolution,
                    layers: 1,
                })
                .map_err(|e| RhiError::BackendError(format!("the shadow framebuffer: {e}")))?;
            framebuffers.push(framebuffer);
        }

        // One uniform buffer and one descriptor set per item: the uniform
        // holds the item's model matrix, and one buffer for the whole pass
        // would make every item use the last one's matrix (§654).
        let descriptor_layout = device.create_descriptor_set_layout(
            &crate::rhi::descriptor::layout::DescriptorSetLayoutDesc {
                bindings: vec![crate::rhi::descriptor::layout::DescriptorBinding {
                    binding: 0,
                    ty: crate::rhi::DescriptorType::UniformBuffer,
                    count: 1,
                    stages: crate::rhi::ShaderStage::VERTEX | crate::rhi::ShaderStage::FRAGMENT,
                    immutable_samplers: None,
                }],
            },
        );
        let mut uniform_buffers = Vec::with_capacity(items.len());
        let mut descriptor_sets = Vec::with_capacity(items.len());
        for _ in 0..items.len() {
            let uniform = device.create_buffer(UNIFORM_SIZE, crate::rhi::BufferUsage::UNIFORM, true);
            let set = device.create_descriptor_set(&descriptor_layout)?;
            device.write_descriptors(
                &set,
                &[DescriptorWrite {
                    dst_set: set.clone(),
                    dst_binding: 0,
                    dst_array_element: 0,
                    descriptors: vec![DescriptorInfo::Buffer(uniform.clone(), 0, UNIFORM_SIZE)],
                }],
            )?;
            uniform_buffers.push(uniform);
            descriptor_sets.push(set);
        }

        let module = device.create_shader_module(&ShaderModuleDesc {
            code: spirv.to_vec(),
            format: ShaderFormat::SpirV,
            entry_point: Some("vs_main".into()),
            name: Some("shadow".into()),
        });
        if !module.has_gpu_backing() {
            return Err(RhiError::BackendError(
                "the shadow shader module was not created on the device".into(),
            ));
        }

        let mut desc = GraphicsPipelineDesc {
            shader_stages: vec![
                PipelineShaderStage {
                    stage: crate::rhi::ShaderStage::VERTEX,
                    module: module.clone(),
                    entry_point: "vs_main".into(),
                },
                PipelineShaderStage {
                    stage: crate::rhi::ShaderStage::FRAGMENT,
                    module,
                    entry_point: "fs_main".into(),
                },
            ],
            // The mesh's own layout: the shadow shader reads the same five
            // attributes the geometry shader does.
            vertex_input_state: Some(crate::render::passes::geometry::vertex_input_state()),
            input_assembly_state: InputAssemblyState {
                topology: PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            rasterizer_state: Some(RasterizerState::double_sided()),
            multisample_state: MultisampleState {
                sample_count: crate::rhi::SampleCount::X1,
                ..Default::default()
            },
            // No colour attachments, so no blending either. A blend state with
            // attachments would make the pipeline disagree with the render
            // pass, and Vulkan counts the attachments.
            color_blend_state: None,
            ..Default::default()
        };
        // The depth test and the depth write: the shadow map is the depth.
        desc.depth_stencil_state = Some(DepthStencilState::enabled());

        let pipeline = device.create_gpu_graphics_pipeline(&desc, &render_pass, &[&descriptor_layout])?;
        if !pipeline.has_gpu_backing() {
            return Err(RhiError::BackendError(
                "the shadow graphics pipeline was not created on the device".into(),
            ));
        }

        Ok(Self::with_state(ShadowPassState {
            pipeline,
            descriptor_layout,
            descriptor_sets,
            uniform_buffers,
            items,
            render_pass,
            shadow_maps,
            shadow_views,
            framebuffers,
            config: config.clone(),
            light_view_projections: vec![Mat4::IDENTITY; cascade_count],
        }))
    }

    /// The shadow maps, so a consumer can sample them.
    ///
    /// One per cascade. A consumer picks the cascade the fragment falls into.
    pub fn shadow_maps(state: &SharedState) -> Vec<crate::rhi::Texture> {
        state.lock().ok().map(|s| s.shadow_maps.clone()).unwrap_or_default()
    }

    /// The first cascade's map, for a consumer that has only one shadow.
    pub fn shadow_map(state: &SharedState) -> Option<crate::rhi::Texture> {
        state.lock().ok()?.shadow_maps.first().cloned()
    }

    /// The light's matrices, one per cascade, and the per-item uniforms.
    ///
    /// The matrix is the same for every item; the model matrix is not. Both
    /// live in the item's own uniform block, so the block has to be rewritten
    /// whenever either changes.
    ///
    /// The blocks are rewritten per cascade while the pass records: the scene is
    /// drawn once into each cascade's map, and each draw needs that cascade's
    /// matrix in the uniform.
    pub fn update_light(
        state: &SharedState,
        device: &crate::rhi::Device,
        view_projections: &[Mat4],
    ) -> bool {
        let Ok(mut state) = state.lock() else {
            return false;
        };
        if view_projections.is_empty() {
            return false;
        }
        state.light_view_projections = view_projections.to_vec();
        let mut ok = true;
        for (index, item) in state.items.iter().enumerate() {
            let Some(buffer) = state.uniform_buffers.get(index) else {
                continue;
            };
            let mut data = [0f32; 32];
            data[..16].copy_from_slice(&view_projections[0].to_cols_array());
            data[16..].copy_from_slice(&item.model.to_cols_array());
            if device.upload_buffer(buffer, &data).is_err() {
                ok = false;
            }
        }
        ok
    }

    /// Upload the per-item uniforms for one cascade.
    ///
    /// The scene is drawn once into each cascade's map, and each draw needs
    /// that cascade's matrix in the item's uniform block. So the blocks are
    /// rewritten between the cascades, while the frame is being recorded.
    fn upload_cascade_uniforms(
        state: &ShadowPassState,
        device: &crate::rhi::Device,
        cascade: usize,
    ) -> bool {
        let Some(matrix) = state.light_view_projections.get(cascade) else {
            return false;
        };
        let mut ok = true;
        for (index, item) in state.items.iter().enumerate() {
            let Some(buffer) = state.uniform_buffers.get(index) else {
                continue;
            };
            let mut data = [0f32; 32];
            data[..16].copy_from_slice(&matrix.to_cols_array());
            data[16..].copy_from_slice(&item.model.to_cols_array());
            if device.upload_buffer(buffer, &data).is_err() {
                ok = false;
            }
        }
        ok
    }

    /// Refresh the item matrices from the scene, and rewrite the uniforms.
    ///
    /// The same bridge the geometry pass uses: the items know which entity
    /// they came from, and the entity's transform is the model matrix.
    pub fn update_models(
        state: &SharedState,
        device: &crate::rhi::Device,
        scene: &Scene,
    ) -> usize {
        let Ok(mut state) = state.lock() else {
            return 0;
        };
        let mut updated = 0;
        for item in state.items.iter_mut() {
            let Some(entity_id) = item.entity_id else {
                continue;
            };
            let Some(entity) = scene.get_entity(entity_id) else {
                continue;
            };
            let Some(transform) = entity.transform() else {
                continue;
            };
            item.model = transform.matrix();
            updated += 1;
        }
        let view_projection = state
            .light_view_projections
            .first()
            .copied()
            .unwrap_or(Mat4::IDENTITY);
        for (index, item) in state.items.iter().enumerate() {
            let Some(buffer) = state.uniform_buffers.get(index) else {
                continue;
            };
            let mut data = [0f32; 32];
            data[..16].copy_from_slice(&view_projection.to_cols_array());
            data[16..].copy_from_slice(&item.model.to_cols_array());
            let _ = device.upload_buffer(buffer, &data);
        }
        updated
    }
}

impl RenderPass for ShadowPass {
    fn name(&self) -> &str {
        self.base.name()
    }

    fn dependencies(&self) -> &[PassDependency] {
        self.base.dependencies()
    }

    fn outputs(&self) -> &[String] {
        self.base.outputs()
    }

    fn initialize(
        &mut self,
        _device: &crate::rhi::Device,
        _context: &RenderContext,
        _resources: &mut HashMap<String, GraphResource>,
    ) {
        // Everything the pass needs was built with the device in `create`:
        // the map, the view, the framebuffer, the pipeline and the per-item
        // uniforms. There is nothing to build here, and nothing to look up —
        // the shadow map is not a graph resource, because its size is the
        // light's, not the screen's.
    }

    fn execute(
        &mut self,
        encoder: &mut CommandEncoder,
        context: &mut RenderContext,
        _scene: &mut Scene,
        _resources: &HashMap<String, GraphResource>,
    ) -> PassOutcome {
        if !self.base.is_enabled() {
            return PassOutcome::skipped("disabled");
        }
        let Some(state) = self.state.as_ref() else {
            return PassOutcome::skipped("the pass has no state");
        };
        let Ok(state) = state.lock() else {
            return PassOutcome::skipped("the state lock is poisoned");
        };
        if state.framebuffers.is_empty() {
            return PassOutcome::skipped("no framebuffer");
        }

        let resolution = state.config.resolution;
        let device = context.device().clone();
        let mut commands: Vec<Command> =
            Vec::with_capacity(state.framebuffers.len() * (8 + state.items.len() * 4));

        // The scene is drawn once into each cascade's map.
        //
        // The cascade's matrix goes into the items' uniform blocks before its
        // own draw, so the blocks are rewritten between the cascades. The
        // rewrite is a host write and the draw that follows it is recorded
        // after it, so the order is the right one.
        for (cascade, framebuffer) in state.framebuffers.iter().enumerate() {
            Self::upload_cascade_uniforms(&state, device.as_ref(), cascade);

        commands.push(Command::BeginRenderPass(
            crate::rhi::command::pass::render::RenderPassBeginInfo {
                render_pass: state.render_pass.clone(),
                    framebuffer: framebuffer.clone(),
                render_area: Rect2D {
                    offset: Offset2D { x: 0, y: 0 },
                    extent: crate::rhi::Extent2D {
                        width: resolution,
                        height: resolution,
                    },
                },
                // The far plane: a fragment that was not drawn has to be
                // "nothing is in front of it", or the whole map is a wall.
                clear_values: vec![crate::rhi::ClearValue::depth_stencil(1.0, 0)],
            },
        ));
        commands.push(Command::SetViewport(crate::rhi::Viewport {
            x: 0.0,
            y: 0.0,
            width: resolution as f32,
            height: resolution as f32,
            min_depth: 0.0,
            max_depth: 1.0,
        }));
        commands.push(Command::SetScissor(Rect2D {
            offset: Offset2D { x: 0, y: 0 },
            extent: crate::rhi::Extent2D {
                width: resolution,
                height: resolution,
            },
        }));
        commands.push(Command::BindGraphicsPipeline(state.pipeline.clone()));

        for (index, item) in state.items.iter().enumerate() {
            let Some(descriptor_set) = state.descriptor_sets.get(index) else {
                crate::error!(
                    "shadow",
                    "no descriptor set for item {:?}: the pass has {} sets for {} items",
                    item.name,
                    state.descriptor_sets.len(),
                    state.items.len()
                );
                continue;
            };
            commands.push(Command::BindDescriptorSets {
                pipeline: state.pipeline.clone(),
                first_set: 0,
                sets: vec![descriptor_set.clone()],
            });
            commands.push(Command::BindVertexBuffers {
                first_binding: 0,
                buffers: vec![((*item.vertex_buffer).clone(), 0)],
            });
            commands.push(Command::BindIndexBuffer {
                buffer: (*item.index_buffer).clone(),
                offset: 0,
                index_type: match item.index_type {
                    crate::render::meshes::IndexType::U16 => crate::rhi::IndexType::U16,
                    crate::render::meshes::IndexType::U32 => crate::rhi::IndexType::U32,
                    crate::render::meshes::IndexType::U8 => {
                        crate::warn!("shadow", "{}: 8-bit indices are not supported", item.name);
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
        }

        for command in commands {
            if !encoder.record(command) {
                crate::error!("shadow", "a command was rejected: the encoder is not recording");
                return PassOutcome::skipped("the encoder rejected a command");
            }
        }
        PassOutcome::recorded()
    }

    fn is_enabled(&self) -> bool {
        self.base.is_enabled()
    }

    fn set_enabled(&mut self, enabled: bool) {
        self.base.set_enabled(enabled);
    }
}
