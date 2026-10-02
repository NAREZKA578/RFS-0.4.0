//! `Command` -> Vulkan translation.
//!
//! Bug №203/№248: `CommandBuffer` recorded a list of `Command`s that nothing
//! ever consumed, so the whole RHI command path was decorative. This module is
//! the consumer. It replays a recorded list into a `vk::CommandBuffer`.
//!
//! Deliberately narrow: it covers the commands a frame needs. Anything else is
//! reported as unsupported rather than skipped, because a silently dropped
//! command draws a wrong picture and looks like a driver bug.

use crate::backend::vulkan::convert;
use crate::command::commands::Command;
use crate::error::{RhiError, RhiResult};
use ash::vk::Handle;

/// A handle plus the device needed to emit calls, resolved once up front.
struct Translated<'a> {
    cb: ash::vk::CommandBuffer,
    device: &'a ash::Device,
}

impl Translated<'_> {
    /// Resolve a `GpuResource` to a raw handle, or say which thing lacked one.
    fn handle(what: &str, res: Option<crate::resource::GpuResource>) -> RhiResult<u64> {
        res.map(|g| g.handle)
            .ok_or_else(|| RhiError::BackendError(format!("{what} has no GPU backing")))
    }
}

/// Replay `commands` into `cb`.
///
/// The caller owns recording: this only emits the calls between `cmd_begin_*`
/// and `cmd_end_*`, never the begin/end of the command buffer itself.
pub fn record(
    device: &ash::Device,
    cb: ash::vk::CommandBuffer,
    commands: &[Command],
) -> RhiResult<()> {
    let t = Translated { cb, device };
    for command in commands {
        replay(&t, command)?;
    }
    Ok(())
}

fn replay(t: &Translated<'_>, command: &Command) -> RhiResult<()> {
    let device = t.device;
    let cb = t.cb;
    unsafe {
        match command {
            Command::BeginRenderPass(info) => {
                let Some(rp) = info.render_pass.backend() else {
                    return Err(RhiError::BackendError("render pass has no GPU backing".into()));
                };
                let Some(fb) = info.framebuffer.backend() else {
                    return Err(RhiError::BackendError("framebuffer has no GPU backing".into()));
                };
                let clears: Vec<ash::vk::ClearValue> = info
                    .clear_values
                    .iter()
                    .map(clear_value)
                    .collect();
                let width = info.framebuffer.desc().width.max(1);
                let height = info.framebuffer.desc().height.max(1);
                // The recorded render area is honoured only when it was set;
                // a default Rect2D is all zeroes, which Vulkan would reject.
                let area = if info.render_area.extent.width == 0 || info.render_area.extent.height == 0 {
                    ash::vk::Rect2D {
                        offset: ash::vk::Offset2D { x: 0, y: 0 },
                        extent: ash::vk::Extent2D { width, height },
                    }
                } else {
                    ash::vk::Rect2D {
                        offset: ash::vk::Offset2D {
                            x: info.render_area.offset.x,
                            y: info.render_area.offset.y,
                        },
                        extent: ash::vk::Extent2D {
                            width: info.render_area.extent.width,
                            height: info.render_area.extent.height,
                        },
                    }
                };
                let begin = ash::vk::RenderPassBeginInfo {
                    render_pass: ash::vk::RenderPass::from_raw(rp.handle),
                    framebuffer: ash::vk::Framebuffer::from_raw(fb.handle),
                    render_area: area,
                    clear_value_count: clears.len() as u32,
                    p_clear_values: clears.as_ptr(),
                    ..Default::default()
                };
                device.cmd_begin_render_pass(cb, &begin, ash::vk::SubpassContents::INLINE);
            }

            Command::EndRenderPass => {
                device.cmd_end_render_pass(cb);
            }

            Command::BindGraphicsPipeline(pipeline) => {
                let handle = Translated::handle("graphics pipeline", pipeline.backend())?;
                device.cmd_bind_pipeline(cb, ash::vk::PipelineBindPoint::GRAPHICS, ash::vk::Pipeline::from_raw(handle));
            }

            Command::BindComputePipeline(pipeline) => {
                let handle = Translated::handle("compute pipeline", pipeline.backend())?;
                device.cmd_bind_pipeline(cb, ash::vk::PipelineBindPoint::COMPUTE, ash::vk::Pipeline::from_raw(handle));
            }

            Command::SetViewport(vp) => {
                let viewport = ash::vk::Viewport {
                    x: vp.x,
                    y: vp.y,
                    width: vp.width,
                    height: vp.height,
                    min_depth: vp.min_depth,
                    max_depth: vp.max_depth,
                };
                device.cmd_set_viewport(cb, 0, &[viewport]);
            }

            Command::SetScissor(sc) => {
                let rect = ash::vk::Rect2D {
                    offset: ash::vk::Offset2D { x: sc.offset.x, y: sc.offset.y },
                    extent: ash::vk::Extent2D {
                        width: sc.extent.width,
                        height: sc.extent.height,
                    },
                };
                device.cmd_set_scissor(cb, 0, &[rect]);
            }

            Command::BindDescriptorSets { pipeline, first_set, sets } => {
                let mut raw = Vec::with_capacity(sets.len());
                for set in sets {
                    raw.push(ash::vk::DescriptorSet::from_raw(Translated::handle(
                        "descriptor set",
                        set.backend(),
                    )?));
                }
                // The `PipelineLayout` is stored in the pipeline's `memory`
                // field, alongside the `Pipeline` in `handle`. Without it
                // Vulkan would need a null layout, which is a crash.
                let Some(pipeline_gpu) = pipeline.backend() else {
                    return Err(RhiError::BackendError(
                        "graphics pipeline has no GPU backing".into(),
                    ));
                };
                if pipeline_gpu.memory == 0 {
                    return Err(RhiError::BackendError(
                        "graphics pipeline carries no pipeline layout; \
                         descriptor sets cannot be bound"
                            .into(),
                    ));
                }
                device.cmd_bind_descriptor_sets(
                    cb,
                    ash::vk::PipelineBindPoint::GRAPHICS,
                    ash::vk::PipelineLayout::from_raw(pipeline_gpu.memory),
                    *first_set,
                    &raw,
                    &[],
                );
            }

            Command::BindVertexBuffers { first_binding, buffers } => {
                if buffers.is_empty() {
                    return Ok(());
                }
                let mut raw = Vec::with_capacity(buffers.len());
                let mut offsets = Vec::with_capacity(buffers.len());
                for (buffer, offset) in buffers {
                    raw.push(ash::vk::Buffer::from_raw(Translated::handle(
                        "vertex buffer",
                        buffer.backend(),
                    )?));
                    offsets.push(*offset);
                }
                device.cmd_bind_vertex_buffers(cb, *first_binding, &raw, &offsets);
            }

            Command::BindIndexBuffer { buffer, offset, index_type } => {
                let raw = ash::vk::Buffer::from_raw(Translated::handle(
                    "index buffer",
                    buffer.backend(),
                )?);
                device.cmd_bind_index_buffer(cb, raw, *offset, convert::index_type(*index_type));
            }

            Command::Draw { vertex_count, instance_count, first_vertex, first_instance } => {
                device.cmd_draw(cb, *vertex_count, *instance_count, *first_vertex, *first_instance);
            }

            Command::DrawIndexed {
                index_count,
                instance_count,
                first_index,
                vertex_offset,
                first_instance,
            } => {
                device.cmd_draw_indexed(
                    cb,
                    *index_count,
                    *instance_count,
                    *first_index,
                    *vertex_offset,
                    *first_instance,
                );
            }

            other => {
                return Err(RhiError::NotSupported(format!(
                    "command not yet translatable to Vulkan: {other:?}"
                )));
            }
        }
    }
    Ok(())
}

fn clear_value(value: &crate::types::ClearValue) -> ash::vk::ClearValue {
    match value {
        crate::types::ClearValue::Color { r, g, b, a } => ash::vk::ClearValue {
            color: ash::vk::ClearColorValue { float32: [*r, *g, *b, *a] },
        },
        crate::types::ClearValue::DepthStencil { depth, stencil } => ash::vk::ClearValue {
            depth_stencil: ash::vk::ClearDepthStencilValue {
                depth: *depth,
                stencil: *stencil,
            },
        },
    }
}
