//! The scene, as the mirror camera sees it.
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//!
//! The water pass takes the reflection and the refraction as **inputs**: it
//! draws the surface, it does not render what is reflected in it. Something
//! has to render the scene from the mirrored camera, and that something is this
//! module.
//!
//! # The mirror camera
//!
//! A mirror in the plane `y = level` reflects the world about that plane. The
//! camera's reflection is the same reflection applied to the camera: it looks
//! at the world from under the water, and the picture it makes is the
//! reflection the water shows.
//!
//! The reflection is **not** a rotation — it reverses handedness, so the
//! triangles that faced the camera now face away from it. The forward pass
//! draws with `CullMode::None`, and the winding therefore does not matter.
//!
//! The projection is not mirrored. The mirror camera looks at the world
//! through the same lens, and the picture comes out in the same orientation as
//! the main view's — which is what lets the water sample it by the fragment's
//! screen position.
//!
//! # What the mirror camera is not
//!
//! It is not a second scene. It is the same geometry, the same lights and the
//! same shadow maps, drawn with a reflected view matrix. The shadow maps are
//! in the light's space and do not depend on the camera, so they are the very
//! same maps the main pass reads.
//!
//! The lighting is computed at the geometry's **real** positions, because the
//! fragment's world position comes from the model matrix, which the reflection
//! does not touch. A mirror shows the object's own lighting, and that is what
//! this gives.

use glam::{Mat4, Vec3};

use crate::rhi::sync::barrier::MemoryBarrier;
use crate::rhi::{
    AccessFlags, Command, CommandEncoder, Framebuffer, FramebufferAttachment, FramebufferDesc,
    PipelineStage,
};

use super::cluster::{ClusterConfig, ClusterPass};
use super::cluster::SharedState as ClusterSharedState;
use super::forward::{ForwardPass};
use super::forward::SharedState as ForwardSharedState;
use super::geometry::GeometryItem;

/// The height of the water's surface.
///
/// The mirror is the plane `y = level`, and the world is reflected about it.
pub const DEFAULT_LEVEL: f32 = 0.0;

/// The reflection about the plane `y = level`.
///
/// It negates `y` and then moves the plane back to where it was: the
/// reflection about `y = level` is a translation of the plane to the origin,
/// the reflection about `y = 0`, and the translation back.
fn reflection_matrix(level: f32) -> Mat4 {
    Mat4::from_translation(Vec3::new(0.0, 2.0 * level, 0.0))
        * Mat4::from_scale(Vec3::new(1.0, -1.0, 1.0))
}

/// The camera's view matrix, reflected about the plane `y = level`.
///
/// This is the mirror camera's view matrix. It is what the scene is drawn
/// with, and the picture it makes is the reflection.
///
/// The reflection is applied **on the right**: the world is reflected first and
/// the camera looks at it. The camera's own basis is not the reflected one
/// then — the camera stays where it is, and the world it sees is the
/// reflected world. The picture is the same either way, and the geometry keeps
/// its real positions, so the lighting is the object's own.
pub fn mirror_view_matrix(view: Mat4, level: f32) -> Mat4 {
    view * reflection_matrix(level)
}

/// The configuration of the mirror camera.
#[derive(Debug, Clone, Copy)]
pub struct MirrorConfig {
    /// The height of the water's surface: the plane the world is reflected
    /// about.
    pub level: f32,
    /// The width of the reflection's image.
    pub width: u32,
    /// The height of the reflection's image.
    pub height: u32,
}

impl Default for MirrorConfig {
    fn default() -> Self {
        Self {
            level: DEFAULT_LEVEL,
            width: 256,
            height: 256,
        }
    }
}

/// Everything the mirror pass needs to record, shared with the caller.
pub struct MirrorPassState {
    /// The image the mirror camera made: the reflection.
    pub reflection_texture: Option<crate::rhi::Texture>,
    pub reflection_view: Option<crate::rhi::TextureView>,
    /// The depth the mirror camera wrote.
    pub depth_texture: Option<crate::rhi::Texture>,
    pub depth_view: Option<crate::rhi::TextureView>,
    /// The framebuffer the mirror camera draws into.
    pub framebuffer: Option<Framebuffer>,
    /// The forward pass that draws the scene, with the mirror camera's view.
    pub forward: ForwardSharedState,
    /// The clustering the forward pass reads its lights from.
    pub cluster: ClusterSharedState,
    /// The lights, in the scene's layout.
    pub lights_buffer: crate::rhi::Buffer,
    /// The configuration the pass was built with.
    pub config: MirrorConfig,
}

/// The mirror pass.
pub struct MirrorPass {
    base: super::base::BaseRenderPass,
    /// The clustering the mirror camera's lights are binned with.
    cluster_pass: Option<ClusterPass>,
    /// The forward pass the mirror camera drives.
    forward_pass: Option<ForwardPass>,
}

impl MirrorPass {
    /// Build a pass with no GPU objects yet.
    ///
    /// The configuration comes with the GPU objects, in `create`: a pass
    /// without them has nothing to draw into.
    pub fn new() -> Self {
        let mut base = super::base::BaseRenderPass::new("mirror");
        // The shadow maps are produced by the shadow pass, and the pass has to
        // come after it: the map is the whole point of the shadow pass.
        base.add_dependency(
            "shadow",
            "shadow_map",
            crate::render::graph::types::ResourceUsage::Sampled,
        );
        // The clusters are produced by the clustering pass, and the pass reads
        // the light's index out of them.
        base.add_dependency(
            "cluster",
            "clusters",
            crate::render::graph::types::ResourceUsage::Sampled,
        );
        base.add_output("reflection");
        Self {
            base,
            cluster_pass: None,
            forward_pass: None,
        }
    }

    /// Build a mirror pass with real GPU objects.
    ///
    /// `items` are the scene's opaque meshes: the same ones the main forward
    /// pass draws, because the reflection is the same scene.
    #[allow(clippy::too_many_arguments)]
    pub fn create(
        device: &crate::rhi::Device,
        items: Vec<GeometryItem>,
        config: MirrorConfig,
        color_format: crate::rhi::Format,
        cluster_spirv: &[u8],
        forward_spirv: &[u8],
    ) -> crate::rhi::error::RhiResult<(
        Self,
        std::sync::Arc<std::sync::Mutex<MirrorPassState>>,
    )> {
        use crate::rhi::error::RhiError;

        let width = config.width.max(1);
        let height = config.height.max(1);

        // The image the mirror camera draws into, and the depth it writes.
        let reflection_texture = device.create_texture(
            width,
            height,
            1,
            color_format,
            crate::rhi::TextureUsage::COLOR_ATTACHMENT
                | crate::rhi::TextureUsage::SAMPLED
                | crate::rhi::TextureUsage::TRANSFER_SRC,
            1,
        );
        if !reflection_texture.has_gpu_backing() {
            return Err(RhiError::BackendError(
                "the reflection's image was not created on the device".into(),
            ));
        }
        let reflection_view = device
            .create_texture_view(
                &reflection_texture,
                &crate::rhi::TextureViewDesc {
                    texture: reflection_texture.clone(),
                    format: None,
                    view_type: crate::rhi::TextureViewType::D2,
                    aspects: crate::rhi::TextureAspectFlags::COLOR,
                    base_mip_level: 0,
                    mip_level_count: 1,
                    base_array_layer: 0,
                    array_layer_count: 1,
                },
            )
            .map_err(|e| RhiError::BackendError(format!("the reflection's view: {e}")))?;

        let depth_texture = device.create_texture(
            width,
            height,
            1,
            crate::rhi::Format::D32_SFLOAT,
            crate::rhi::TextureUsage::DEPTH_STENCIL_ATTACHMENT,
            1,
        );
        let depth_view = device
            .create_texture_view(
                &depth_texture,
                &crate::rhi::TextureViewDesc {
                    texture: depth_texture.clone(),
                    format: None,
                    view_type: crate::rhi::TextureViewType::D2,
                    aspects: crate::rhi::TextureAspectFlags::DEPTH,
                    base_mip_level: 0,
                    mip_level_count: 1,
                    base_array_layer: 0,
                    array_layer_count: 1,
                },
            )
            .map_err(|e| RhiError::BackendError(format!("the reflection's depth view: {e}")))?;

        // The forward pass the mirror camera drives. Its render pass is the
        // one the framebuffer is built from: the colour is cleared, the depth
        // is cleared, and the formats are the ones the reflection's own
        // images have.
        let (forward_pass, forward_state) =
            ForwardPass::create(device, items, color_format, forward_spirv)?;
        let render_pass = forward_state
            .lock()
            .map(|state| state.render_pass.clone())
            .unwrap_or_default();

        let framebuffer = device
            .create_gpu_framebuffer(&FramebufferDesc {
                render_pass,
                attachments: vec![
                    FramebufferAttachment {
                        texture_view: reflection_view.clone(),
                        layer: 0,
                        mip_level: 0,
                    },
                    FramebufferAttachment {
                        texture_view: depth_view.clone(),
                        layer: 0,
                        mip_level: 0,
                    },
                ],
                width,
                height,
                layers: 1,
            })
            .map_err(|e| RhiError::BackendError(format!("the mirror framebuffer: {e}")))?;

        // The clustering the mirror camera's lights are binned with. It is a
        // pass of its own: the clusters are cut with the view the forward pass
        // draws with, and the mirror camera's view is not the main one.
        let (cluster_pass, cluster_state) =
            ClusterPass::create(device, ClusterConfig::default(), cluster_spirv)?;
        let lights_buffer = cluster_state
            .lock()
            .map(|state| state.lights_buffer.clone())
            .unwrap_or_else(|_| {
                device.create_buffer(
                    1024 * crate::render::lighting::light_buffer::LIGHT_STRIDE_BYTES,
                    crate::rhi::BufferUsage::STORAGE,
                    true,
                )
            });

        // The forward pass reads the clusters the clustering pass wrote, and
        // the lights from the same buffer the clustering binned from.
        let indices = cluster_state
            .lock()
            .map(|state| state.cluster_indices_buffer.clone())
            .unwrap_or_default();
        ForwardPass::set_cluster_buffers(
            &forward_state,
            lights_buffer.clone(),
            indices,
            ClusterConfig::default(),
        );

        // The mirror camera draws into the framebuffer above.
        ForwardPass::set_framebuffer(&forward_state, Some(framebuffer.clone()));

        let state = MirrorPassState {
            reflection_texture: Some(reflection_texture),
            reflection_view: Some(reflection_view),
            depth_texture: Some(depth_texture),
            depth_view: Some(depth_view),
            framebuffer: Some(framebuffer),
            forward: forward_state,
            cluster: cluster_state,
            lights_buffer,
            config,
        };
        let shared = std::sync::Arc::new(std::sync::Mutex::new(state));

        let mut base = super::base::BaseRenderPass::new("mirror");
        base.add_dependency(
            "shadow",
            "shadow_map",
            crate::render::graph::types::ResourceUsage::Sampled,
        );
        base.add_dependency(
            "cluster",
            "clusters",
            crate::render::graph::types::ResourceUsage::Sampled,
        );
        base.add_output("reflection");

        Ok((
            Self {
                base,
                cluster_pass: Some(cluster_pass),
                forward_pass: Some(forward_pass),
            },
            shared,
        ))
    }

    /// The shadow maps the mirror camera reads.
    ///
    /// The same maps the main pass reads: they are in the light's space, and
    /// the light does not move when the camera is mirrored.
    pub fn set_shadow_maps(
        state: &std::sync::Arc<std::sync::Mutex<MirrorPassState>>,
        views: Vec<crate::rhi::TextureView>,
    ) {
        if let Ok(state) = state.lock() {
            ForwardPass::set_shadow_maps(&state.forward, views);
        }
    }

    /// The camera's matrices, reflected about the water's surface.
    ///
    /// The view matrix the mirror camera draws with is the reflection of the
    /// camera's own. The projection is the camera's: the mirror camera looks
    /// through the same lens.
    #[allow(clippy::too_many_arguments)]
    pub fn update_uniforms(
        state: &std::sync::Arc<std::sync::Mutex<MirrorPassState>>,
        device: &crate::rhi::Device,
        view: Mat4,
        projection: Mat4,
        light_space: &[Mat4],
        splits: &[f32],
        cascade_count: usize,
        shadow_light_index: u32,
        light_count: u32,
        near: f32,
        far: f32,
        ambient: Vec3,
    ) -> bool {
        let Ok(state) = state.lock() else {
            return false;
        };
        let level = state.config.level;
        let mirror_view = mirror_view_matrix(view, level);
        // The clustering pass cuts its clusters with the same view the forward
        // pass draws with, or the light's index would be looked up in the
        // wrong place.
        let cluster_ok = ClusterPass::update_uniforms(
            &state.cluster,
            device,
            mirror_view,
            projection,
            light_count,
            near,
            far,
            state.config.width.max(1),
            state.config.height.max(1),
        );
        let forward_ok = ForwardPass::update_uniforms(
            &state.forward,
            device,
            mirror_view,
            projection,
            light_space,
            splits,
            cascade_count,
            shadow_light_index,
            near,
            far,
            ambient,
        );
        cluster_ok && forward_ok
    }

    /// The lights, in the scene's layout.
    ///
    /// The caller fills it: the same lights the main pass is lit by, because
    /// the reflection is the same scene.
    pub fn lights_buffer(
        state: &std::sync::Arc<std::sync::Mutex<MirrorPassState>>,
    ) -> Option<crate::rhi::Buffer> {
        state.lock().ok().map(|state| state.lights_buffer.clone())
    }

    /// The reflection: the scene as the mirror camera saw it.
    pub fn reflection_view(
        state: &std::sync::Arc<std::sync::Mutex<MirrorPassState>>,
    ) -> Option<crate::rhi::TextureView> {
        state
            .lock()
            .ok()
            .and_then(|state| state.reflection_view.clone())
    }
}

impl crate::render::graph::node::RenderPass for MirrorPass {
    fn name(&self) -> &str {
        self.base.name()
    }

    fn dependencies(&self) -> &[crate::render::graph::node::PassDependency] {
        self.base.dependencies()
    }

    fn outputs(&self) -> &[String] {
        self.base.outputs()
    }

    fn initialize(
        &mut self,
        _device: &crate::rhi::Device,
        _context: &crate::render::core::RenderContext,
        _resources: &mut std::collections::HashMap<
            String,
            crate::render::graph::resource::GraphResource,
        >,
    ) {
    }

    fn execute(
        &mut self,
        encoder: &mut CommandEncoder,
        context: &mut crate::render::core::RenderContext,
        scene: &mut crate::render::scene::Scene,
        resources: &std::collections::HashMap<
            String,
            crate::render::graph::resource::GraphResource,
        >,
    ) -> crate::render::graph::node::PassOutcome {
        use crate::render::graph::node::PassOutcome;

        if !self.base.is_enabled() {
            return PassOutcome::skipped("disabled");
        }
        let Some(cluster_pass) = self.cluster_pass.as_mut() else {
            return PassOutcome::skipped("the pass has no clustering pass");
        };
        let Some(forward_pass) = self.forward_pass.as_mut() else {
            return PassOutcome::skipped("the pass has no forward pass");
        };

        // The lights are binned into the clusters.
        let cluster_outcome = cluster_pass.execute(encoder, context, scene, resources);
        if !cluster_outcome.recorded {
            return cluster_outcome;
        }

        // The clusters are written by the compute pass and read by the
        // forward pass, and the order of the passes is not a memory
        // dependency: without this the forward pass would read the clusters
        // while the compute pass is still writing them.
        if !encoder.record(Command::PipelineBarrier {
            src_stage: PipelineStage::COMPUTE_SHADER,
            dst_stage: PipelineStage::FRAGMENT_SHADER,
            memory_barriers: vec![MemoryBarrier {
                src_access: AccessFlags::SHADER_WRITE,
                dst_access: AccessFlags::SHADER_READ,
            }],
            buffer_barriers: vec![],
            texture_barriers: vec![],
        }) {
            return PassOutcome::skipped("the encoder rejected a command");
        }

        forward_pass.execute(encoder, context, scene, resources)
    }

    fn is_enabled(&self) -> bool {
        self.base.is_enabled()
    }

    fn set_enabled(&mut self, enabled: bool) {
        self.base.set_enabled(enabled);
    }
}
