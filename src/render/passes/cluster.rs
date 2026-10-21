//! Cluster Pass
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//!
//! The light clustering of Forward+: the screen is divided into tiles, each
//! tile into depth slices, and each of those cells — a *cluster* — is filled
//! with the indices of the lights that reach into it. A pixel then asks only
//! its own cluster which lights it has to look at, instead of the whole list.
//!
//! It is a compute pass: the work is independent per light, and the GPU is the
//! right place for it. The RHI has compute pipelines and a dispatch, so
//! nothing had to be invented for it — only used.
//!
//! The cluster's shape in the world is the frustum slab under one tile
//! between two depths, and a light's sphere is tested against that slab's
//! box. The culling here tests every light against every cluster: that is
//! slower than the usual screen-space bound on the tiles a light can reach,
//! and it gives the same answer. The bound is an optimisation that comes
//! later, not the difference between right and wrong.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use glam::Mat4;

use crate::render::core::RenderContext;
use crate::render::graph::node::{PassDependency, PassOutcome, RenderPass};
use crate::render::graph::resource::GraphResource;
use crate::render::passes::base::BaseRenderPass;
use crate::render::scene::Scene;
use crate::rhi::{
    BufferUsage, Command, CommandEncoder, DescriptorInfo, DescriptorSet, DescriptorSetLayout,
    DescriptorWrite,
};

/// The uniform block, matching `cluster.wgsl`:
/// `mat4 view_projection` + `mat4 inverse_view_projection` + two `vec4` = 160 bytes.
const UNIFORM_SIZE: u64 = 16 * 4 * 2 + 4 * 4 * 2;

/// The maximum number of lights one cluster may hold.
///
/// The same number the shader uses. It is in two places because one of them is
/// the shader's and the other is the size of the buffer, and a buffer that is
/// smaller than the shader's bound is a shader reading past the end of it.
pub const MAX_LIGHTS_PER_CLUSTER: u32 = 32;

/// The light, in the layout the scene's light buffer uses.
///
/// `vec3` is 16-aligned inside a struct, so the stride is 48: the same buffer is
/// read by the lighting shader, and the two have to agree exactly.
pub const LIGHT_STRIDE: u64 = 48;

/// How the screen is divided.
#[derive(Debug, Clone, Copy)]
pub struct ClusterConfig {
    /// The tiles across the screen.
    pub tiles_x: u32,
    /// The tiles down the screen.
    pub tiles_y: u32,
    /// The depth slices a tile is divided into.
    pub depth_slices: u32,
}

impl Default for ClusterConfig {
    fn default() -> Self {
        Self {
            // 16x9 tiles of a 1920x1080 screen are 120x120 pixels, which is
            // about the size of a point light's influence at a normal
            // distance. The slices are the view space's depth, divided
            // linearly: the depth buffer is not linear, but a cluster's depth
            // range has to be, or the boxes would not nest.
            tiles_x: 16,
            tiles_y: 9,
            depth_slices: 24,
        }
    }
}

impl ClusterConfig {
    /// The number of clusters: one per tile and slice.
    pub fn cluster_count(&self) -> u32 {
        self.tiles_x * self.tiles_y * self.depth_slices
    }
}

/// Everything the pass needs to record, shared with the caller.
pub struct ClusterPassState {
    pub pipeline: crate::rhi::pipeline::compute::ComputePipeline,
    /// The pipeline that zeroes the counts.
    pub clear_pipeline: crate::rhi::pipeline::compute::ComputePipeline,
    pub descriptor_layout: DescriptorSetLayout,
    pub descriptor_set: Option<DescriptorSet>,
    /// The uniforms: the two matrices and the two vectors.
    pub uniform_buffer: crate::rhi::Buffer,
    /// The lights, in the scene's layout. The caller fills it.
    pub lights_buffer: crate::rhi::Buffer,
    /// The light indices, cluster after cluster.
    pub cluster_indices_buffer: crate::rhi::Buffer,
    /// How many lights each cluster holds.
    pub cluster_counts_buffer: crate::rhi::Buffer,
    pub config: ClusterConfig,
    /// How many lights are in the buffer.
    pub light_count: u32,
}

/// Shared handle to the pass state.
pub type SharedState = Arc<Mutex<ClusterPassState>>;

/// The light clustering pass.
pub struct ClusterPass {
    base: BaseRenderPass,
    state: Option<SharedState>,
}

impl ClusterPass {
    /// Build a pass with no GPU objects yet.
    pub fn new() -> Self {
        let mut base = BaseRenderPass::new("cluster");
        // The clusters are read by the forward pass, so the forward pass has to
        // come after this one.
        base.add_output("clusters");
        Self {
            base,
            state: None,
        }
    }

    /// Build a pass around a new shared state and return both.
    pub fn with_state(state: ClusterPassState) -> (Self, SharedState) {
        let mut base = BaseRenderPass::new("cluster");
        base.add_output("clusters");
        let shared: SharedState = Arc::new(Mutex::new(state));
        let pass = Self {
            base,
            state: Some(shared.clone()),
        };
        (pass, shared)
    }

    /// Build a clustering pass with real GPU objects.
    pub fn create(
        device: &crate::rhi::Device,
        config: ClusterConfig,
        spirv: &[u8],
    ) -> crate::rhi::error::RhiResult<(Self, SharedState)> {
        use crate::rhi::error::RhiError;
        use crate::rhi::pipeline::compute::ComputePipelineDesc;
        use crate::rhi::shader::module::{ShaderFormat, ShaderModuleDesc};

        let clusters = config.cluster_count();
        let indices_size = u64::from(clusters) * u64::from(MAX_LIGHTS_PER_CLUSTER) * 4;
        let counts_size = u64::from(clusters) * 4;

        let uniform_buffer =
            device.create_buffer(UNIFORM_SIZE, BufferUsage::UNIFORM, true);
        // The lights' buffer is host visible, because the scene's lights are
        // written into it every frame.
        let lights_buffer = device.create_buffer(1024 * LIGHT_STRIDE, BufferUsage::STORAGE, true);
        let cluster_indices_buffer = device.create_buffer(
            indices_size,
            BufferUsage::STORAGE | BufferUsage::TRANSFER_SRC,
            false,
        );
        let cluster_counts_buffer =
            device.create_buffer(counts_size, BufferUsage::STORAGE | BufferUsage::TRANSFER_SRC, false);

        let descriptor_layout = device.create_descriptor_set_layout(
            &crate::rhi::descriptor::layout::DescriptorSetLayoutDesc {
                bindings: vec![
                    crate::rhi::descriptor::layout::DescriptorBinding {
                        binding: 0,
                        ty: crate::rhi::DescriptorType::UniformBuffer,
                        count: 1,
                        stages: crate::rhi::ShaderStage::COMPUTE,
                        immutable_samplers: None,
                    },
                    crate::rhi::descriptor::layout::DescriptorBinding {
                        binding: 1,
                        ty: crate::rhi::DescriptorType::StorageBuffer,
                        count: 1,
                        stages: crate::rhi::ShaderStage::COMPUTE,
                        immutable_samplers: None,
                    },
                    crate::rhi::descriptor::layout::DescriptorBinding {
                        binding: 2,
                        ty: crate::rhi::DescriptorType::StorageBuffer,
                        count: 1,
                        stages: crate::rhi::ShaderStage::COMPUTE,
                        immutable_samplers: None,
                    },
                    crate::rhi::descriptor::layout::DescriptorBinding {
                        binding: 3,
                        ty: crate::rhi::DescriptorType::StorageBuffer,
                        count: 1,
                        stages: crate::rhi::ShaderStage::COMPUTE,
                        immutable_samplers: None,
                    },
                ],
            },
        );
        let descriptor_set = device.create_descriptor_set(&descriptor_layout)?;
        device.write_descriptors(
            &descriptor_set,
            &[
                DescriptorWrite {
                    dst_set: descriptor_set.clone(),
                    dst_binding: 0,
                    dst_array_element: 0,
                    descriptors: vec![DescriptorInfo::Buffer(
                        uniform_buffer.clone(),
                        0,
                        UNIFORM_SIZE,
                    )],
                },
                DescriptorWrite {
                    dst_set: descriptor_set.clone(),
                    dst_binding: 1,
                    dst_array_element: 0,
                    descriptors: vec![DescriptorInfo::Buffer(
                        lights_buffer.clone(),
                        0,
                        1024 * LIGHT_STRIDE,
                    )],
                },
                DescriptorWrite {
                    dst_set: descriptor_set.clone(),
                    dst_binding: 2,
                    dst_array_element: 0,
                    descriptors: vec![DescriptorInfo::Buffer(
                        cluster_indices_buffer.clone(),
                        0,
                        indices_size,
                    )],
                },
                DescriptorWrite {
                    dst_set: descriptor_set.clone(),
                    dst_binding: 3,
                    dst_array_element: 0,
                    descriptors: vec![DescriptorInfo::Buffer(
                        cluster_counts_buffer.clone(),
                        0,
                        counts_size,
                    )],
                },
            ],
        )?;

        let module = device.create_shader_module(&ShaderModuleDesc {
            code: spirv.to_vec(),
            format: ShaderFormat::SpirV,
            entry_point: Some("cs_main".into()),
            name: Some("cluster".into()),
        });
        if !module.has_gpu_backing() {
            return Err(RhiError::BackendError(
                "the cluster shader module was not created on the device".into(),
            ));
        }
        // Two pipelines: one zeroes the counts, the other bins the lights.
        //
        // The zeroing is on the GPU on purpose. The host cannot do it: the
        // host's write would race with the previous frame's binning, which may
        // still be running, and the counts would be a mixture of two frames.
        let clear_pipeline = device.create_compute_pipeline(
            &ComputePipelineDesc {
                shader: crate::rhi::pipeline::graphics::PipelineShaderStage {
                    stage: crate::rhi::ShaderStage::COMPUTE,
                    module: module.clone(),
                    entry_point: "cs_clear".into(),
                },
            },
            &descriptor_layout,
        )?;
        let pipeline = device.create_compute_pipeline(
            &ComputePipelineDesc {
                shader: crate::rhi::pipeline::graphics::PipelineShaderStage {
                    stage: crate::rhi::ShaderStage::COMPUTE,
                    module,
                    entry_point: "cs_main".into(),
                },
            },
            &descriptor_layout,
        )?;
        if !pipeline.has_gpu_backing() {
            return Err(RhiError::BackendError(
                "the cluster compute pipeline was not created on the device".into(),
            ));
        }

        Ok(Self::with_state(ClusterPassState {
            pipeline,
            clear_pipeline,
            descriptor_layout,
            descriptor_set: Some(descriptor_set),
            uniform_buffer,
            lights_buffer,
            cluster_indices_buffer,
            cluster_counts_buffer,
            config,
            light_count: 0,
        }))
    }

    /// The buffers the clusters are in, so a caller can read them back.
    pub fn cluster_buffers(
        state: &SharedState,
    ) -> Option<(crate::rhi::Buffer, crate::rhi::Buffer)> {
        let state = state.lock().ok()?;
        Some((
            state.cluster_indices_buffer.clone(),
            state.cluster_counts_buffer.clone(),
        ))
    }

    /// The camera matrices and how many lights there are.
    pub fn update_uniforms(
        state: &SharedState,
        device: &crate::rhi::Device,
        view: Mat4,
        projection: Mat4,
        light_count: u32,
        near: f32,
        far: f32,
        screen_width: u32,
        screen_height: u32,
    ) -> bool {
        let Ok(mut state) = state.lock() else {
            return false;
        };
        state.light_count = light_count;
        let view_projection = projection * view;
        let mut data = [0f32; 40];
        data[..16].copy_from_slice(&view_projection.to_cols_array());
        // The inverse is what turns a cluster's screen position and depth back
        // into the world, and a cluster's box is in the world. Without it the
        // shader could only cull in the screen's space, which is not what a
        // light's sphere is.
        let inverse = view_projection.inverse();
        data[16..32].copy_from_slice(&inverse.to_cols_array());
        data[32] = screen_width as f32;
        data[33] = screen_height as f32;
        data[34] = state.config.tiles_x as f32;
        data[35] = state.config.tiles_y as f32;
        data[36] = state.config.depth_slices as f32;
        data[37] = near;
        data[38] = far;
        data[39] = light_count as f32;
        device
            .upload_buffer(&state.uniform_buffer, &data)
            .is_ok()
    }
}

impl RenderPass for ClusterPass {
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
        // Everything the pass needs was built with the device in `create`.
    }

    fn execute(
        &mut self,
        encoder: &mut CommandEncoder,
        _context: &mut RenderContext,
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
        if state.light_count == 0 {
            return PassOutcome::skipped("no lights to cluster");
        }
        let Some(descriptor_set) = state.descriptor_set.clone() else {
            return PassOutcome::skipped("no descriptor set");
        };
        let pipeline = state.pipeline.clone();
        let clear_pipeline = state.clear_pipeline.clone();
        let groups = state.light_count.div_ceil(64);
        let clear_groups = state.config.cluster_count().div_ceil(64);
        let descriptor_set = descriptor_set.clone();

        let mut commands: Vec<Command> = Vec::with_capacity(6);
        // Zero the counts, then bin. The barrier between them is what makes
        // the zeroing visible to the binning: two dispatches in a row are
        // ordered in *execution*, not in memory.
        commands.push(Command::BindComputePipeline(clear_pipeline.clone()));
        commands.push(Command::BindComputeDescriptorSets {
            pipeline: clear_pipeline,
            first_set: 0,
            sets: vec![descriptor_set.clone()],
        });
        commands.push(Command::Dispatch {
            x: clear_groups,
            y: 1,
            z: 1,
        });
        commands.push(Command::PipelineBarrier {
            src_stage: crate::rhi::PipelineStage::COMPUTE_SHADER,
            dst_stage: crate::rhi::PipelineStage::COMPUTE_SHADER,
            memory_barriers: vec![crate::rhi::sync::barrier::MemoryBarrier {
                src_access: crate::rhi::AccessFlags::SHADER_WRITE,
                dst_access: crate::rhi::AccessFlags::SHADER_READ
                    | crate::rhi::AccessFlags::SHADER_WRITE,
            }],
            buffer_barriers: vec![],
            texture_barriers: vec![],
        });
        commands.push(Command::BindComputePipeline(pipeline.clone()));
        commands.push(Command::BindComputeDescriptorSets {
            pipeline,
            first_set: 0,
            sets: vec![descriptor_set],
        });
        commands.push(Command::Dispatch {
            x: groups,
            y: 1,
            z: 1,
        });

        for command in commands {
            if !encoder.record(command) {
                crate::error!("cluster", "a command was rejected: the encoder is not recording");
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
