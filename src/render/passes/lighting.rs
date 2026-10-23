//! Lighting Render Pass
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//!
//! The second pass of the deferred chain: it reads the GBuffer the geometry
//! pass filled and resolves it into a lit image.
//!
//! It is a fullscreen pass — one triangle over the whole target, no vertex
//! buffer of geometry — and it reads four textures (position, normal, albedo,
//! material) plus the scene's lights out of a read-only storage buffer.
//!
//! Two things about the shader are worth knowing before changing it:
//!
//! - **The light model is positional.** A light is a point with a colour, an
//!   intensity and a radius, and the falloff is `1 - distance / radius`. A
//!   directional light has no position, so it has to be expressed as a point
//!   far enough away that the falloff across the scene is negligible. The
//!   shader has no light-type field, and that is a limitation of the shader
//!   rather than of the scene's light model.
//! - **The GBuffer's row 0 is the top of the image.** Vulkan's framebuffer
//!   origin is top-left, so sampling it with an unflipped Y reads every row
//!   from the wrong end. The picture that comes out is not upside down; it is
//!   simply not the right picture, which is worse.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::render::core::RenderContext;
use crate::render::graph::node::{PassDependency, PassOutcome, RenderPass};
use crate::render::graph::resource::GraphResource;
use crate::render::graph::types::ResourceUsage;
use crate::render::lighting::light_buffer;
use crate::render::passes::base::BaseRenderPass;
use crate::render::scene::Scene;
use crate::rhi::{
    AccessFlags, AttachmentDescription, AttachmentReference, Buffer, BufferDesc, BufferUsage, Command,
    CommandEncoder, DescriptorInfo, DescriptorSet, DescriptorSetLayout, DescriptorType,
    DescriptorWrite, Framebuffer, FramebufferAttachment, FramebufferDesc, GraphicsPipeline,
    MemoryPropertyFlags, Offset2D, Rect2D, RenderPassDesc, ShaderStage, SharingMode,
    SubpassDescription, Texture, TextureLayout, TextureView, VertexAttribute, VertexBinding,
    VertexInputRate, Viewport,
};
use crate::rhi::sync::barrier::TextureBarrier;

/// Lighting configuration
#[derive(Debug, Clone)]
pub struct LightingConfig {
    pub max_direction_lights: usize,
    pub max_point_lights: usize,
    pub max_spot_lights: usize,
    pub use_ibl: bool,
    pub use_ssao: bool,
    pub use_ssr: bool,
    pub ssao_radius: f32,
    pub ssao_bias: f32,
}

impl Default for LightingConfig {
    fn default() -> Self {
        Self {
            max_direction_lights: 4,
            max_point_lights: 16,
            max_spot_lights: 16,
            use_ibl: true,
            use_ssao: true,
            use_ssr: true,
            ssao_radius: 0.5,
            ssao_bias: 0.05,
        }
    }
}

/// The name the lighting result has in the graph's resource map.
pub const TARGET_LIGHTING: &str = "lighting";

/// The GBuffer targets the lighting pass reads, in binding order.
pub const SOURCE_POSITION: &str = "gbuffer_position";
pub const SOURCE_NORMAL: &str = "gbuffer_normal";
pub const SOURCE_ALBEDO: &str = "gbuffer_albedo";
pub const SOURCE_MATERIAL: &str = "gbuffer_material";

/// The uniform block: `view_proj`, `camera_position`, `light_count`, `ambient`.
///
/// WGSL's uniform layout puts `camera_position` and `ambient` at 16-byte
/// boundaries, so the block is 96 bytes and not 92. The padding is in the
/// data, not in the size: the block is 24 floats, and the three unused ones
/// are the padding before `ambient`.
const UNIFORM_SIZE: u64 = 24 * 4;

/// One light in the shader's layout, and the size of one in the buffer.
///
/// The layout is written down once, in
/// [`crate::render::lighting::light_buffer`], and every writer and every
/// reader uses it. It used to be written down in three places, and two of them
/// disagreed with the third by four bytes — which is enough for the shader to
/// read a light's intensity out of the padding and find a zero there. A light
/// with an intensity of zero is a light that is present in the file and
/// contributes nothing to the picture, and no message says so.
///
/// The layout is the one WGSL gives the struct
///
/// ```wgsl
/// struct Light { position: vec3<f32>, color: vec3<f32>, intensity: f32, radius: f32 };
/// ```
///
/// and a `vec3` in a struct is **padded to 16 bytes**, because the next
/// member's alignment has to be respected: `position` is at 0..12, `color` has
/// to start at 16, `intensity` at 28 and `radius` at 32. The struct is 36 bytes,
/// rounded up to its 16-byte alignment to 40, and the array's stride is that
/// rounded up again — 48.
///
/// Packing the fields tightly, as `position, color, intensity, radius` in
/// consecutive floats, is the mistake this comment exists to prevent.
pub use crate::render::lighting::light_buffer::LIGHT_STRIDE_BYTES as LIGHT_SIZE;

/// How many lights the storage buffer holds.
///
/// The shader loops `light_count` times over the buffer, so the buffer has to
/// be at least that long; the extra room is what lets the count change per
/// frame without reallocating.
const MAX_LIGHTS: usize = 32;

/// The format of the lit image. HDR, because the lighting sums contributions.
pub const LIGHTING_FORMAT: crate::rhi::Format = crate::rhi::Format::RGBA16_SFLOAT;

/// Everything the pass needs to record, shared with the caller.
pub struct LightingPassState {
    pub pipeline: GraphicsPipeline,
    pub descriptor_layout: DescriptorSetLayout,
    pub descriptor_set: Option<DescriptorSet>,
    pub uniform_buffer: Buffer,
    pub lights_buffer: Buffer,
    /// The fullscreen triangle: three vertices in clip space.
    pub triangle_buffer: Buffer,
    pub render_pass: crate::rhi::RenderPass,
    pub framebuffer: Option<Framebuffer>,
    /// The ambient term, applied to every pixel before the lights.
    pub ambient: [f32; 3],
    /// How many lights the pass writes, at most.
    ///
    /// From the configuration, and clamped to what the storage buffer holds:
    /// the shader loops `light_count` times over the buffer, so a count larger
    /// than the buffer would read past the end of it.
    pub max_lights: usize,
}

/// The handle the caller keeps.
pub type SharedState = Arc<Mutex<LightingPassState>>;

/// Lighting pass
pub struct LightingPass {
    base: BaseRenderPass,
    state: Option<SharedState>,
}

impl LightingPass {
    /// The pass's base, with what it reads and what it produces.
    ///
    /// Both constructors use it. `with_state` used to build a fresh base with
    /// no dependencies and no outputs, so the graph created nothing for the
    /// pass and nothing ordered it against the GBuffer — the same mistake the
    /// GBuffer pass had, and the same symptom: a pass that looked wired and
    /// drew nothing.
    fn base(config: &LightingConfig) -> BaseRenderPass {
        let mut base = BaseRenderPass::new("lighting");

        // Lighting pass depends on GBuffer
        base.add_dependency("gbuffer", SOURCE_POSITION, ResourceUsage::Sampled);
        base.add_dependency("gbuffer", SOURCE_NORMAL, ResourceUsage::Sampled);
        base.add_dependency("gbuffer", SOURCE_ALBEDO, ResourceUsage::Sampled);
        base.add_dependency("gbuffer", SOURCE_MATERIAL, ResourceUsage::Sampled);

        // Lighting pass outputs lighting texture
        base.add_output(TARGET_LIGHTING);

        // SSAO and SSR are not implemented, so they are not declared: a
        // resource nobody writes is a resource the graph would create for
        // nothing.
        let _ = config;
        base
    }

    pub fn new(config: LightingConfig) -> Self {
        let _ = config;
        Self {
            base: Self::base(&LightingConfig::default()),
            state: None,
        }
    }

    /// Build a pass around a new shared state and return both.
    pub fn with_state(state: LightingPassState) -> (Self, SharedState) {
        let shared: SharedState = Arc::new(Mutex::new(state));
        let pass = Self {
            base: Self::base(&LightingConfig::default()),
            state: Some(shared.clone()),
        };
        (pass, shared)
    }

    /// Build a lighting pass.
    ///
    /// `spirv` is taken as bytes so the render layer carries no knowledge of
    /// where assets live; the embedder uses `include_bytes!`.
    pub fn create(
        device: &crate::rhi::Device,
        config: LightingConfig,
        spirv: &[u8],
    ) -> crate::rhi::error::RhiResult<(Self, SharedState)> {
        use crate::rhi::error::RhiError;
        use crate::rhi::pipeline::graphics::PrimitiveTopology;
        use crate::rhi::{
            ColorBlendAttachment, ColorBlendState, ColorComponentFlags, ColorTargetDesc,
            GraphicsPipelineDesc, InputAssemblyState, MultisampleState, PipelineShaderStage,
            RasterizerState, ShaderFormat, ShaderModuleDesc, VertexInputState,
        };

        let render_pass = device.create_gpu_render_pass(&render_pass_desc())?;

        // How many lights the configuration allows, and how many the storage
        // buffer holds. The smaller wins: the shader loops `light_count` times
        // over the buffer, so a count larger than the buffer would read past
        // the end of it.
        let configured_lights =
            config.max_direction_lights + config.max_point_lights + config.max_spot_lights;

        let descriptor_layout =
            device.create_descriptor_set_layout(&descriptor_set_layout_desc());

        // The uniform and the lights are written every frame, so they are
        // host-visible: a device-local buffer would need a staging copy per
        // frame, and the upload would fail outright with "buffer memory is not
        // host mapped and the buffer lacks transfer usage".
        let uniform_buffer = device.create_buffer_from_desc(&BufferDesc {
            size: UNIFORM_SIZE,
            usage: BufferUsage::UNIFORM,
            memory_flags: MemoryPropertyFlags::HOST_VISIBLE | MemoryPropertyFlags::HOST_COHERENT,
            sharing_mode: SharingMode::Exclusive,
            queue_family_indices: Vec::new(),
        })?;
        let lights_buffer = device.create_buffer_from_desc(&BufferDesc {
            size: MAX_LIGHTS as u64 * LIGHT_SIZE,
            usage: BufferUsage::STORAGE,
            memory_flags: MemoryPropertyFlags::HOST_VISIBLE | MemoryPropertyFlags::HOST_COHERENT,
            sharing_mode: SharingMode::Exclusive,
            queue_family_indices: Vec::new(),
        })?;

        // The fullscreen triangle: one triangle that covers the whole clip
        // space, with the third vertex outside it so no clipping is needed.
        let triangle_buffer = device.create_buffer_from_desc(&BufferDesc {
            size: 3 * 3 * 4,
            usage: BufferUsage::VERTEX,
            memory_flags: MemoryPropertyFlags::HOST_VISIBLE | MemoryPropertyFlags::HOST_COHERENT,
            sharing_mode: SharingMode::Exclusive,
            queue_family_indices: Vec::new(),
        })?;
        device.upload_buffer(
            &triangle_buffer,
            &[-1.0f32, -1.0, 0.0, 3.0, -1.0, 0.0, -1.0, 3.0, 0.0],
        )?;

        let module = device.create_shader_module(&ShaderModuleDesc {
            code: spirv.to_vec(),
            format: ShaderFormat::SpirV,
            entry_point: Some("vs_main".into()),
            name: Some("lighting".into()),
        });
        if !module.has_gpu_backing() {
            return Err(RhiError::BackendError(
                "the lighting shader module was not created on the device".into(),
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
            // One attribute: the clip-space position of the fullscreen
            // triangle. This is not the mesh layout the geometry pass uses —
            // that one is five attributes for a position, a normal, a tangent,
            // a texture coordinate and a colour.
            vertex_input_state: Some(VertexInputState {
                bindings: vec![VertexBinding {
                    binding: 0,
                    stride: 3 * 4,
                    input_rate: VertexInputRate::Vertex,
                }],
                attributes: vec![VertexAttribute {
                    binding: 0,
                    location: 0,
                    format: crate::rhi::Format::R32G32B32_SFLOAT,
                    offset: 0,
                }],
            }),
            input_assembly_state: InputAssemblyState {
                topology: PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            rasterizer_state: Some(RasterizerState::double_sided()),
            multisample_state: MultisampleState {
                sample_count: crate::rhi::SampleCount::X1,
                ..Default::default()
            },
            color_blend_state: Some(ColorBlendState {
                // The write mask has to be set explicitly. `ColorComponentFlags`
                // derives `Default`, and for a bitflags type that default is
                // the *empty* set: a pipeline with an empty write mask blends
                // into nowhere, which is legal, silent, and looks exactly like
                // a shader that does nothing.
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
        // The render pass has no depth attachment, so the pipeline must not
        // declare a depth test: a pipeline that declares one against a pass
        // with no depth attachment is invalid.
        desc.depth_stencil_state = None;

        let pipeline =
            device.create_gpu_graphics_pipeline(&desc, &render_pass, &[&descriptor_layout])?;
        if !pipeline.has_gpu_backing() {
            return Err(RhiError::BackendError(
                "the lighting graphics pipeline was not created on the device".into(),
            ));
        }

        Ok(Self::with_state(LightingPassState {
            pipeline,
            descriptor_layout,
            descriptor_set: None,
            uniform_buffer,
            lights_buffer,
            triangle_buffer,
            render_pass,
            framebuffer: None,
            ambient: [0.1, 0.1, 0.12],
            max_lights: configured_lights.min(MAX_LIGHTS),
        }))
    }

    /// Fill the lights buffer from the scene's light components.
    ///
    /// The position comes from the light entity's transform, because the
    /// scene's light model has no position of its own: a light is a component
    /// on an entity, and the entity is what is placed in the world.
    ///
    /// Returns how many lights were written.
    pub fn update_lights(
        state: &SharedState,
        device: &crate::rhi::Device,
        scene: &Scene,
    ) -> usize {
        let Ok(state) = state.lock() else {
            return 0;
        };
        let mut data = vec![0.0f32; MAX_LIGHTS * light_buffer::LIGHT_STRIDE_FLOATS];
        let mut count = 0usize;
        let limit = state.max_lights.min(MAX_LIGHTS);

        for entity in scene.entities() {
            if count >= limit {
                break;
            }
            let Some(light_component) = entity.light() else {
                continue;
            };
            let light = light_component.get_light();
            if !light.enabled {
                continue;
            }
            let position = entity
                .transform()
                .map(|transform| transform.position())
                .unwrap_or_default();
            let base = count * light_buffer::LIGHT_STRIDE_FLOATS;
            data[base + light_buffer::offsets::POSITION] = position.x;
            data[base + light_buffer::offsets::POSITION + 1] = position.y;
            data[base + light_buffer::offsets::POSITION + 2] = position.z;
            data[base + light_buffer::offsets::COLOR] = light.color.x;
            data[base + light_buffer::offsets::COLOR + 1] = light.color.y;
            data[base + light_buffer::offsets::COLOR + 2] = light.color.z;
            data[base + light_buffer::offsets::INTENSITY] = light.intensity;
            data[base + light_buffer::offsets::RADIUS] = light.radius;
            count += 1;
        }

        if let Err(e) = device.upload_buffer(&state.lights_buffer, &data) {
            eprintln!("[lighting] the lights could not be uploaded: {e}");
            return 0;
        }
        count
    }

    /// Write this frame's uniforms.
    ///
    /// Returns whether the GPU now holds them.
    pub fn update_uniforms(
        state: &SharedState,
        device: &crate::rhi::Device,
        view_proj: glam::Mat4,
        camera_position: glam::Vec3,
        light_count: u32,
    ) -> bool {
        let Ok(state) = state.lock() else {
            return false;
        };
        let mut data = [0.0f32; 24];
        data[..16].copy_from_slice(&view_proj.to_cols_array());
        data[16..19].copy_from_slice(&[camera_position.x, camera_position.y, camera_position.z]);
        // `light_count` is a `u32` in the shader, and this buffer is written as
        // floats. The bits have to be the integer's, not the float's: writing
        // `1.0f32` puts 0x3F800000 there, the shader reads 1065353216, and it
        // then loops that many times over the lights buffer — reading far past
        // its end, which is `ERROR_DEVICE_LOST` and nothing else.
        data[19] = f32::from_bits(light_count);
        data[20..23].copy_from_slice(&state.ambient);
        // The 24th float is the padding before `ambient`, and stays zero.
        match device.upload_buffer(&state.uniform_buffer, &data) {
            Ok(()) => true,
            Err(e) => {
                eprintln!("[lighting] the uniform upload failed: {e}");
                false
            }
        }
    }
}

/// The render pass: one colour attachment, the lit image.
fn render_pass_desc() -> RenderPassDesc {
    RenderPassDesc {
        attachments: vec![AttachmentDescription {
            format: LIGHTING_FORMAT,
            samples: crate::rhi::SampleCount::X1,
            load_op: crate::rhi::LoadOp::Clear,
            store_op: crate::rhi::StoreOp::Store,
            stencil_load_op: crate::rhi::LoadOp::DontCare,
            stencil_store_op: crate::rhi::StoreOp::DontCare,
            initial_layout: crate::rhi::TextureLayout::Undefined,
            // Readable out: the next pass in the chain samples it.
            final_layout: crate::rhi::TextureLayout::ShaderReadOnlyOptimal,
        }],
        subpasses: vec![SubpassDescription {
            pipeline_bind_point: crate::rhi::PipelineBindPoint::Graphics,
            color_attachments: vec![AttachmentReference {
                attachment: 0,
                layout: crate::rhi::TextureLayout::ColorAttachmentOptimal,
            }],
            ..Default::default()
        }],
        dependencies: vec![],
    }
}

/// The descriptor layout: a uniform, four GBuffer textures and the lights.
fn descriptor_set_layout_desc() -> crate::rhi::descriptor::layout::DescriptorSetLayoutDesc {
    use crate::rhi::descriptor::layout::DescriptorBinding;
    let texture = |binding: u32| DescriptorBinding {
        binding,
        ty: DescriptorType::SampledTexture,
        count: 1,
        stages: ShaderStage::FRAGMENT,
        immutable_samplers: None,
    };
    crate::rhi::descriptor::layout::DescriptorSetLayoutDesc {
        bindings: vec![
            DescriptorBinding {
                binding: 0,
                ty: DescriptorType::UniformBuffer,
                count: 1,
                stages: ShaderStage::FRAGMENT,
                immutable_samplers: None,
            },
            texture(1),
            texture(2),
            texture(3),
            texture(4),
            DescriptorBinding {
                binding: 5,
                ty: DescriptorType::StorageBuffer,
                count: 1,
                stages: ShaderStage::FRAGMENT,
                immutable_samplers: None,
            },
        ],
    }
}

/// Take a texture and its view out of the graph's resource map.
fn target<'a>(
    resources: &'a HashMap<String, GraphResource>,
    name: &str,
) -> Option<(&'a Texture, &'a TextureView)> {
    let resource = resources.get(name)?;
    let texture = resource.texture()?;
    let view = resource.view()?;
    Some((texture, view))
}

impl RenderPass for LightingPass {
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
        device: &crate::rhi::Device,
        _context: &RenderContext,
        resources: &mut HashMap<String, GraphResource>,
    ) {
        // The GBuffer's four targets and the lit image all belong to the
        // graph. The pass asks for them by name; the graph created them
        // because both ends of every dependency are in this frame.
        //
        // Only the lit image is the framebuffer's *attachment*. The GBuffer's
        // four are sampled, and a framebuffer's attachment count has to match
        // its render pass's exactly — `VUID-VkFramebufferCreateInfo-
        // attachmentCount-00876` — so putting them in the framebuffer as well
        // would make the framebuffer unusable.
        let mut missing = Vec::new();
        let mut source_views: Vec<TextureView> = Vec::with_capacity(4);
        let mut target_view: Option<TextureView> = None;
        let mut width = 0u32;
        let mut height = 0u32;
        for name in [
            SOURCE_POSITION,
            SOURCE_NORMAL,
            SOURCE_ALBEDO,
            SOURCE_MATERIAL,
        ] {
            match target(resources, name) {
                Some((texture, view)) => {
                    if width == 0 {
                        width = texture.width();
                        height = texture.height();
                    }
                    source_views.push(view.clone());
                }
                None => missing.push(name),
            }
        }
        match target(resources, TARGET_LIGHTING) {
            Some((texture, view)) => {
                if width == 0 {
                    width = texture.width();
                    height = texture.height();
                }
                target_view = Some(view.clone());
            }
            None => missing.push(TARGET_LIGHTING),
        }
        if !missing.is_empty() {
            crate::error!(
                "lighting",
                "the graph has no target(s) {missing:?}; the pass will not draw"
            );
            if let Ok(mut state) = self.state.as_ref().expect("the state exists").lock() {
                state.framebuffer = None;
                state.descriptor_set = None;
            }
            return;
        }

        let Some(state) = self.state.as_ref() else {
            return;
        };
        let Ok(mut state) = state.lock() else {
            return;
        };

        // The descriptor set names the four GBuffer textures; the lit image is
        // the framebuffer's attachment, not a binding.
        let set = match device.create_descriptor_set(&state.descriptor_layout) {
            Ok(set) => set,
            Err(e) => {
                crate::error!("lighting", "the descriptor set could not be created: {e}");
                state.framebuffer = None;
                return;
            }
        };
        let mut writes = Vec::with_capacity(6);
        // Binding 0 is the uniform. It has to be written like the rest: a
        // descriptor the shader reads and nobody updated is
        // `VUID-vkCmdDrawIndexed-None-08114`, and the draw reads it.
        writes.push(DescriptorWrite {
            dst_set: set.clone(),
            dst_binding: 0,
            dst_array_element: 0,
            descriptors: vec![DescriptorInfo::Buffer(
                state.uniform_buffer.clone(),
                0,
                UNIFORM_SIZE,
            )],
        });
        for (index, view) in source_views.iter().enumerate() {
            writes.push(DescriptorWrite {
                dst_set: set.clone(),
                dst_binding: index as u32 + 1,
                dst_array_element: 0,
                descriptors: vec![DescriptorInfo::Texture(view.clone(), None)],
            });
        }
        writes.push(DescriptorWrite {
            dst_set: set.clone(),
            dst_binding: 5,
            dst_array_element: 0,
            descriptors: vec![DescriptorInfo::Buffer(
                state.lights_buffer.clone(),
                0,
                MAX_LIGHTS as u64 * LIGHT_SIZE,
            )],
        });
        if let Err(e) = device.write_descriptors(&set, &writes) {
            crate::error!("lighting", "the descriptors could not be written: {e}");
            state.framebuffer = None;
            return;
        }
        state.descriptor_set = Some(set);

        let render_pass = state.render_pass.clone();
        match device.create_gpu_framebuffer(&FramebufferDesc {
            render_pass,
            attachments: target_view
                .into_iter()
                .map(|texture_view| FramebufferAttachment {
                    texture_view,
                    layer: 0,
                    mip_level: 0,
                })
                .collect(),
            width,
            height,
            layers: 1,
        }) {
            Ok(framebuffer) => state.framebuffer = Some(framebuffer),
            Err(e) => {
                crate::error!("lighting", "the framebuffer could not be created: {e}");
                state.framebuffer = None;
            }
        }
    }

    fn execute(
        &mut self,
        encoder: &mut CommandEncoder,
        context: &mut RenderContext,
        _scene: &mut Scene,
        _resources: &HashMap<String, GraphResource>,
    ) -> PassOutcome {
        // Bug №653: the pass says what it did, so the graph can tell "was
        // called" from "contributed to the frame".
        if !self.base.is_enabled() {
            return PassOutcome::skipped("disabled");
        }
        let Some(state) = self.state.as_ref() else {
            return PassOutcome::skipped("the pass has no state");
        };
        let Ok(state) = state.lock() else {
            return PassOutcome::skipped("the state lock is poisoned");
        };
        let Some(framebuffer) = state.framebuffer.as_ref() else {
            return PassOutcome::skipped("no framebuffer");
        };
        let Some(descriptor_set) = state.descriptor_set.as_ref() else {
            return PassOutcome::skipped("no descriptor set");
        };

        let (width, height) = context.drawable_size();
        let mut commands: Vec<Command> = Vec::with_capacity(14);

        // The barrier the deferred chain needs, and the reason the graph does
        // not give it for free.
        //
        // The graph orders the passes, but an order is not a memory
        // dependency: a render pass boundary does **not** make the writes of
        // the first pass visible to the sampled reads of the second. Without
        // this barrier the lighting pass reads the GBuffer while the GBuffer
        // is still being written, and the result is a lit image that changes
        // from run to run — the dark region moves.
        //
        // The layouts are not transitioned here: the GBuffer's render pass ends
        // with `final_layout: ShaderReadOnlyOptimal`, so the images are
        // already where the sampled reads need them. What is missing is only
        // the dependency, and a barrier that keeps a layout says exactly that.
        let mut texture_barriers = Vec::with_capacity(4);
        for name in [
            SOURCE_POSITION,
            SOURCE_NORMAL,
            SOURCE_ALBEDO,
            SOURCE_MATERIAL,
        ] {
            if let Some((texture, _)) = target(_resources, name) {
                texture_barriers.push(TextureBarrier {
                    texture: texture.clone(),
                    src_access: AccessFlags::COLOR_ATTACHMENT_WRITE,
                    dst_access: AccessFlags::SHADER_READ,
                    old_layout: TextureLayout::ShaderReadOnlyOptimal,
                    new_layout: TextureLayout::ShaderReadOnlyOptimal,
                });
            }
        }
        if !texture_barriers.is_empty() {
            commands.push(Command::PipelineBarrier {
                src_stage: crate::rhi::PipelineStage::COLOR_ATTACHMENT_OUTPUT,
                dst_stage: crate::rhi::PipelineStage::FRAGMENT_SHADER,
                memory_barriers: vec![],
                buffer_barriers: vec![],
                texture_barriers,
            });
        }

        commands.push(Command::BeginRenderPass(
            crate::rhi::command::pass::render::RenderPassBeginInfo {
                render_pass: state.render_pass.clone(),
                framebuffer: framebuffer.clone(),
                render_area: Rect2D {
                    offset: Offset2D { x: 0, y: 0 },
                    extent: crate::rhi::Extent2D { width, height },
                },
                clear_values: vec![crate::rhi::ClearValue::color(0.0, 0.0, 0.0, 1.0)],
            },
        ));
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
            extent: crate::rhi::Extent2D { width, height },
        }));
        commands.push(Command::BindGraphicsPipeline(state.pipeline.clone()));
        commands.push(Command::BindDescriptorSets {
            pipeline: state.pipeline.clone(),
            first_set: 0,
            sets: vec![descriptor_set.clone()],
        });
        commands.push(Command::BindVertexBuffers {
            first_binding: 0,
            buffers: vec![(state.triangle_buffer.clone(), 0)],
        });
        commands.push(Command::DrawIndexed {
            index_count: 3,
            instance_count: 1,
            first_index: 0,
            vertex_offset: 0,
            first_instance: 0,
        });
        commands.push(Command::EndRenderPass);

        for command in commands {
            if !encoder.record(command) {
                crate::error!("lighting", "a command was rejected: the encoder is not recording");
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
