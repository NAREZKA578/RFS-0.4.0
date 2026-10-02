//! Command Encoder
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use super::buffer::CommandBuffer;
use super::commands::Command;
use crate::command::pass::render::{Framebuffer, RenderPass, RenderPassBeginInfo};
use crate::descriptor::set::DescriptorSet;
use crate::pipeline::GraphicsPipeline;
use crate::error::RhiResult;
use crate::resource::{Buffer, TextureView};
use crate::types::*;

/// Command encoder
pub struct CommandEncoder {
    command_buffer: CommandBuffer,
    commands: Vec<Command>,
}

impl CommandEncoder {
    pub fn new(command_buffer: CommandBuffer) -> Self {
        Self {
            command_buffer,
            commands: Vec::new(),
        }
    }

    pub fn command_buffer(&self) -> &CommandBuffer {
        &self.command_buffer
    }

    /// Returns the number of recorded commands.
    pub fn command_count(&self) -> usize {
        self.commands.len()
    }

    /// Returns `true` if the encoder is currently recording.
    pub fn is_recording(&self) -> bool {
        self.command_buffer.is_recording()
    }

    /// Starts recording into the wrapped command buffer.
    ///
    /// Bug №181: this used to call `begin()` and then `commands.clear()`
    /// unconditionally. `begin()` was a silent no-op when already recording, so
    /// a stray second `begin()` wiped the commands recorded so far and the
    /// caller had no way to know. `begin` now refuses to re-enter.
    pub fn begin(&mut self) -> RhiResult<()> {
        self.command_buffer.begin()?;
        self.commands.clear();
        Ok(())
    }

    /// Stops recording and returns the finished command buffer.
    ///
    /// Bug №181: `finish` used to panic via the encoder's `end()`. It now
    /// reports the misuse.
    ///
    /// Bug №203: the recorded `Command`s used to be dropped on the floor here
    /// — they were held by the encoder and nothing was ever moved into the
    /// buffer, so a finished buffer always reported zero commands and no
    /// backend could replay the frame. They are now moved into the buffer, and
    /// `submit_commands` on the active backend consumes them.
    pub fn finish(mut self) -> RhiResult<CommandBuffer> {
        self.command_buffer.end()?;
        for command in self.commands.drain(..) {
            self.command_buffer.push_command(command);
        }
        Ok(self.command_buffer)
    }

    /// Records a command.
    ///
    /// Bug №181: this pushed unconditionally, so commands recorded outside a
    /// `begin`/`end` pair were silently accumulated and submitted as part of
    /// the next recording. It now returns `false` and drops the command when
    /// the buffer is not recording.
    pub fn record(&mut self, command: Command) -> bool {
        if !self.command_buffer.is_recording() {
            eprintln!(
                "[RHI] command dropped: the buffer is not recording (state={:?})",
                self.command_buffer.state()
            );
            return false;
        }
        self.commands.push(command);
        true
    }

    /// Begin a render pass.
    pub fn begin_render_pass(
        &mut self,
        render_pass: &RenderPass,
        framebuffer: &Framebuffer,
        render_area: Rect2D,
        clear_values: &[ClearValue],
        _clear_depth: f32,
        _clear_stencil: u32,
    ) {
        self.record(Command::BeginRenderPass(RenderPassBeginInfo {
            render_pass: render_pass.clone(),
            framebuffer: framebuffer.clone(),
            render_area,
            clear_values: clear_values.to_vec(),
        }));
    }

    /// End the current render pass.
    pub fn end_render_pass(&mut self) {
        self.record(Command::EndRenderPass);
    }

    /// Set a viewport.
    pub fn set_viewport(
        &mut self,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        min_depth: f32,
        max_depth: f32,
    ) {
        self.record(Command::SetViewport(Viewport {
            x,
            y,
            width,
            height,
            min_depth,
            max_depth,
        }));
    }

    /// Set a scissor rectangle.
    pub fn set_scissor(&mut self, x: i32, y: i32, width: u32, height: u32) {
        self.record(Command::SetScissor(Scissor {
            offset: Offset2D { x, y },
            extent: Extent2D { width, height },
        }));
    }

    /// Bind a graphics pipeline.
    pub fn bind_pipeline(&mut self, pipeline: &GraphicsPipeline) {
        self.record(Command::BindGraphicsPipeline(pipeline.clone()));
    }

    /// Bind a descriptor set.
    ///
    /// The pipeline must be passed too: descriptor binding needs the bound
    /// pipeline's `PipelineLayout`, and the recorded command has no other way
    /// to reach it.
    pub fn bind_descriptor_set(
        &mut self,
        pipeline: &GraphicsPipeline,
        set: &DescriptorSet,
        index: u32,
    ) {
        self.record(Command::BindDescriptorSets {
            pipeline: pipeline.clone(),
            first_set: index,
            sets: vec![set.clone()],
        });
    }

    /// Bind a vertex buffer at offset 0.
    pub fn bind_vertex_buffer(&mut self, buffer: &Buffer) {
        self.bind_vertex_buffer_at(buffer, 0);
    }

    /// Bind a vertex buffer at a specific offset.
    pub fn bind_vertex_buffer_at(&mut self, buffer: &Buffer, offset: u64) {
        self.record(Command::BindVertexBuffers {
            first_binding: 0,
            buffers: vec![(buffer.clone(), offset)],
        });
    }

    /// Bind an index buffer.
    pub fn bind_index_buffer(&mut self, buffer: &Buffer, index_type: IndexType) {
        self.record(Command::BindIndexBuffer {
            buffer: buffer.clone(),
            offset: 0,
            index_type,
        });
    }

    /// Bind a uniform buffer to a binding slot.
    pub fn bind_uniform_buffer(&mut self, _buffer: &Buffer, _binding: u32) {
        // Uniform buffers are exposed through descriptor sets in the stub
        // encoder; the binding is tracked by descriptor set layout instead.
    }

    /// Bind a texture view to a binding slot.
    pub fn bind_texture(&mut self, _view: &TextureView, _binding: u32) {
        // Texture bindings are exposed through descriptor sets.
    }

    /// Draw instanced indexed geometry.
    pub fn draw_indexed_instanced(
        &mut self,
        index_count: u32,
        instance_count: u32,
        first_index: u32,
        vertex_offset: i32,
        first_instance: u32,
    ) {
        self.record(Command::DrawIndexed {
            index_count,
            instance_count,
            first_index,
            vertex_offset,
            first_instance,
        });
    }

    /// Copy a texture view into another.
    pub fn copy_texture(&mut self, _source: &TextureView, _destination: &TextureView) {
        // Placeholder for a GPU-side copy; a CPU stub has nothing to move.
    }

    /// Submitted commands in recording order.
    pub fn commands(&self) -> &[Command] {
        &self.commands
    }
}

/// Render encoder
pub struct RenderEncoder {
    encoder: CommandEncoder,
}

impl RenderEncoder {
    pub fn new(encoder: CommandEncoder) -> Self {
        Self { encoder }
    }

    pub fn inner(&self) -> &CommandEncoder {
        &self.encoder
    }
    pub fn inner_mut(&mut self) -> &mut CommandEncoder {
        &mut self.encoder
    }
}

/// Compute encoder
pub struct ComputeEncoder {
    encoder: CommandEncoder,
}

impl ComputeEncoder {
    pub fn new(encoder: CommandEncoder) -> Self {
        Self { encoder }
    }

    pub fn inner(&self) -> &CommandEncoder {
        &self.encoder
    }
    pub fn inner_mut(&mut self) -> &mut CommandEncoder {
        &mut self.encoder
    }
}

/// Ray tracing encoder
pub struct RayTracingEncoder {
    encoder: CommandEncoder,
}

impl RayTracingEncoder {
    pub fn new(encoder: CommandEncoder) -> Self {
        Self { encoder }
    }

    pub fn inner(&self) -> &CommandEncoder {
        &self.encoder
    }
    pub fn inner_mut(&mut self) -> &mut CommandEncoder {
        &mut self.encoder
    }
}