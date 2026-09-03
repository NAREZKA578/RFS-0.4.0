//! Command buffer abstraction.

use super::buffer::Buffer;
use super::pipeline::Pipeline;
use super::texture::Texture;
use super::types::{Color, Rect};

/// Primitive topology.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PrimitiveTopology {
    /// Point list.
    PointList,
    /// Line list.
    LineList,
    /// Line strip.
    LineStrip,
    /// Triangle list.
    #[default]
    TriangleList,
    /// Triangle strip.
    TriangleStrip,
}

/// Command encoder for recording GPU commands.
pub trait CommandEncoder: Send + std::any::Any {
    /// Begins a render pass.
    fn begin_render_pass(
        &mut self,
        color_attachments: &[ColorAttachment],
        depth_attachment: Option<&DepthAttachment>,
    );

    /// Ends the current render pass.
    fn end_render_pass(&mut self);

    /// Binds a pipeline.
    fn bind_pipeline(&mut self, pipeline: &dyn Pipeline);

    /// Binds a vertex buffer.
    fn bind_vertex_buffer(&mut self, binding: u32, buffer: &dyn Buffer);

    /// Binds a per-instance buffer as four vec4 attributes starting at
    /// `first_location` (divisor = 1), e.g. for GPU instancing of model
    /// matrices. `stride` is the byte stride between instances.
    fn bind_instance_buffer(&mut self, first_location: u32, buffer: &dyn Buffer, stride: u32);

    /// Binds an index buffer.
    fn bind_index_buffer(&mut self, buffer: &dyn Buffer);

    /// Binds a uniform buffer.
    fn bind_uniform_buffer(&mut self, binding: u32, buffer: &dyn Buffer);

    /// Binds a storage buffer (read-only SSBO / storage buffer).
    ///
    /// Default implementation is a no-op; backends that support storage buffers
    /// (e.g. Vulkan) override it to wire the buffer into the descriptor set.
    fn bind_storage_buffer(&mut self, _binding: u32, _buffer: &dyn Buffer) {}

    /// Binds a texture.
    fn bind_texture(&mut self, binding: u32, texture: &dyn Texture);

    /// Sets a float uniform in the currently bound program.
    fn set_uniform_f32(&mut self, name: &str, x: f32);

    /// Sets a vec2 uniform in the currently bound program.
    fn set_uniform_vec2(&mut self, name: &str, x: f32, y: f32);

    /// Sets a vec3 uniform in the currently bound program.
    fn set_uniform_vec3(&mut self, name: &str, x: f32, y: f32, z: f32);

    /// Sets a vec4 uniform in the currently bound program.
    fn set_uniform_vec4(&mut self, name: &str, x: f32, y: f32, z: f32, w: f32);

    /// Sets an int uniform in the currently bound program.
    fn set_uniform_i32(&mut self, name: &str, x: i32);

    /// Sets a 3x3 matrix uniform in the currently bound program.
    fn set_uniform_mat3(&mut self, name: &str, value: &[f32; 9]);

    /// Sets a 4x4 matrix uniform in the currently bound program.
    fn set_uniform_mat4(&mut self, name: &str, value: &[f32; 16]);

    /// Binds a texture sampler uniform to a texture unit.
    fn set_uniform_texture(&mut self, name: &str, unit: u32);

    /// Sets the viewport.
    fn set_viewport(
        &mut self,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        min_depth: f32,
        max_depth: f32,
    );

    /// Sets the scissor rectangle.
    fn set_scissor(&mut self, rect: &Rect);

    /// Sets the primitive topology.
    fn set_primitive_topology(&mut self, topology: PrimitiveTopology);

    /// Draws non-indexed geometry.
    fn draw(
        &mut self,
        vertex_count: u32,
        instance_count: u32,
        first_vertex: u32,
        first_instance: u32,
    );

    /// Draws indexed geometry.
    fn draw_indexed(
        &mut self,
        index_count: u32,
        instance_count: u32,
        first_index: u32,
        vertex_offset: i32,
        first_instance: u32,
    );

    /// Updates a buffer.
    fn update_buffer(&mut self, buffer: &mut dyn Buffer, data: &[u8], offset: u64);

    /// Copies buffer to buffer.
    fn copy_buffer_to_buffer(
        &mut self,
        src: &dyn Buffer,
        dst: &dyn Buffer,
        src_offset: u64,
        dst_offset: u64,
        size: u64,
    );

    /// Copies texture to texture.
    fn copy_texture_to_texture(&mut self, src: &dyn Texture, dst: &dyn Texture);

    /// Copies buffer to texture.
    fn copy_buffer_to_texture(&mut self, src: &dyn Buffer, dst: &dyn Texture);

    /// Pushes a mat4 via push constants (Vulkan). No-op on other backends.
    fn push_mat4(&mut self, _value: &[f32; 16]) {}

    /// Pushes an i32 via push constants (Vulkan). No-op on other backends.
    fn push_i32(&mut self, _value: i32) {}

    /// Pushes raw bytes via push constants (Vulkan). No-op on other backends.
    fn push_bytes(&mut self, _bytes: &[u8]) {}

    /// Pushes a debug group.
    fn push_debug_group(&mut self, name: &str);

    /// Pops a debug group.
    fn pop_debug_group(&mut self);
}

/// Color attachment descriptor.
#[derive(Debug, Clone)]
pub struct ColorAttachment {
    /// The texture native handle to render to.
    pub texture_handle: u64,
    /// Load operation.
    pub load_op: LoadOp,
    /// Store operation.
    pub store_op: StoreOp,
    /// Clear color (used if load_op is Clear).
    pub clear_color: Color,
}

/// Depth/stencil attachment descriptor.
#[derive(Debug, Clone)]
pub struct DepthAttachment {
    /// The depth texture native handle to render to.
    pub texture_handle: u64,
    /// Depth load operation.
    pub depth_load_op: LoadOp,
    /// Depth store operation.
    pub depth_store_op: StoreOp,
    /// Clear depth value (used if load_op is Clear).
    pub clear_depth: f32,
}

/// Load operation for render pass attachments.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadOp {
    /// Load existing contents.
    Load,
    /// Clear to a specific value.
    Clear,
    /// Don't care (undefined contents).
    DontCare,
}

/// Store operation for render pass attachments.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreOp {
    /// Store results.
    Store,
    /// Discard results.
    DontCare,
}
