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

            Command::BindComputeDescriptorSets { pipeline, first_set, sets } => {
                let mut raw = Vec::with_capacity(sets.len());
                for set in sets {
                    raw.push(ash::vk::DescriptorSet::from_raw(Translated::handle(
                        "descriptor set",
                        set.backend(),
                    )?));
                }
                // The compute pipeline's layout is in the same `memory` field
                // as the graphics pipeline's: the backend stores the pipeline
                // layout next to the pipeline when it creates it. Without the
                // layout Vulkan would need a null one, which is a crash.
                let Some(pipeline_gpu) = pipeline.backend() else {
                    return Err(RhiError::BackendError(
                        "compute pipeline has no GPU backing".into(),
                    ));
                };
                if pipeline_gpu.memory == 0 {
                    return Err(RhiError::BackendError(
                        "compute pipeline carries no pipeline layout; \
                         descriptor sets cannot be bound"
                            .into(),
                    ));
                }
                device.cmd_bind_descriptor_sets(
                    cb,
                    ash::vk::PipelineBindPoint::COMPUTE,
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

            Command::Dispatch { x, y, z } => {
                device.cmd_dispatch(cb, *x, *y, *z);
            }

            Command::ClearColor { texture, value, rects } => {
                // A clear needs the image itself, not a view of it: Vulkan's
                // clear commands take a VkImage and a subresource range, and the
                // rects are refused below rather than through a view. The view is
                // still the only handle the command carries, so the image is
                // reached through it.
                refuse_partial_clear("ClearColor", rects)?;
                let image = ash::vk::Image::from_raw(Translated::handle(
                    "color clear target",
                    texture.texture().backend(),
                )?);
                let aspect = clear_aspect_for(texture.texture().desc().format);
                let ranges = subresource_ranges(texture.mip_level_count(), aspect);
                let raw = match value {
                    crate::types::ClearValue::Color { r, g, b, a } => {
                        ash::vk::ClearColorValue { float32: [*r, *g, *b, *a] }
                    }
                    // A depth value reaching a colour clear is a caller mistake,
                    // and Vulkan has no sensible way to interpret it. Passing the
                    // clear through silently would write depth into colour, which
                    // is the kind of corruption that only shows up as a wrong
                    // pixel much later.
                    crate::types::ClearValue::DepthStencil { .. } => {
                        return Err(RhiError::BackendError(
                            "ClearColor was given a depth/stencil value".into(),
                        ));
                    }
                };
                transition_whole_image_to_transfer_dst(device, cb, image, aspect);
                device.cmd_clear_color_image(
                    cb,
                    image,
                    ash::vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    &raw,
                    &ranges,
                );
            }

            Command::ClearDepthStencil { texture, depth, stencil, rects } => {
                refuse_partial_clear("ClearDepthStencil", rects)?;
                let image = ash::vk::Image::from_raw(Translated::handle(
                    "depth clear target",
                    texture.texture().backend(),
                )?);
                let aspect = clear_aspect_for(texture.texture().desc().format);
                let ranges = subresource_ranges(texture.mip_level_count(), aspect);
                transition_whole_image_to_transfer_dst(device, cb, image, aspect);
                device.cmd_clear_depth_stencil_image(
                    cb,
                    image,
                    // Clearing a depth/stencil image accepts only
                    // TRANSFER_DST_OPTIMAL or GENERAL
                    // (`VUID-vkCmdClearDepthStencilImage-imageLayout-00012`).
                    // DEPTH_STENCIL_ATTACHMENT_OPTIMAL reads as the natural
                    // choice — it is where a depth image usually lives — and is
                    // exactly the value the spec refuses.
                    ash::vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    &ash::vk::ClearDepthStencilValue {
                        depth: *depth,
                        stencil: *stencil,
                    },
                    &ranges,
                );
            }

            Command::CopyBuffer { src, dst, regions } => {
                let src_raw = ash::vk::Buffer::from_raw(Translated::handle(
                    "copy source buffer",
                    src.backend(),
                )?);
                let dst_raw = ash::vk::Buffer::from_raw(Translated::handle(
                    "copy destination buffer",
                    dst.backend(),
                )?);
                let raw: Vec<ash::vk::BufferCopy> = regions
                    .iter()
                    .map(|r| ash::vk::BufferCopy {
                        src_offset: r.src_offset,
                        dst_offset: r.dst_offset,
                        size: r.size,
                    })
                    .collect();
                device.cmd_copy_buffer(cb, src_raw, dst_raw, &raw);
            }

            Command::CopyBufferToTexture { src, dst, regions } => {
                let src_raw = ash::vk::Buffer::from_raw(Translated::handle(
                    "upload source buffer",
                    src.backend(),
                )?);
                let image = ash::vk::Image::from_raw(Translated::handle(
                    "upload destination texture",
                    dst.backend(),
                )?);
                // 1D images are legal Vulkan copy targets, so this is a note
                // rather than a refusal; the extent below is already 2D.
                let is_1d = dst.desc().dimensions == crate::types::TextureDimensions::D1;
                let raw: Vec<ash::vk::BufferImageCopy> = regions
                    .iter()
                    .map(|r| ash::vk::BufferImageCopy {
                        buffer_offset: r.buffer_offset,
                        buffer_row_length: 0,
                        buffer_image_height: 0,
                        image_subresource: ash::vk::ImageSubresourceLayers {
                            aspect_mask: ash::vk::ImageAspectFlags::COLOR,
                            mip_level: 0,
                            base_array_layer: 0,
                            layer_count: 1,
                        },
                        image_offset: ash::vk::Offset3D { x: 0, y: 0, z: 0 },
                        image_extent: ash::vk::Extent3D {
                            width: dst.desc().width,
                            height: dst.desc().height,
                            depth: dst.desc().depth.max(1),
                        },
                    })
                    .collect();
                if is_1d {
                    // `rhi` has no `crate::warn!`, unlike `render`: logging here
                    // would mean reaching for a backend-neutral facility that does
                    // not exist, so this stays an explicit note.
                    eprintln!(
                        "[rhi/record] CopyBufferToTexture used with a 1D image; \
                         the copy extent is still 2D"
                    );
                }
                // The barrier's aspect comes from the format like every other
                // image barrier here; the copy itself declares COLOR because it
                // was written for colour uploads, and a depth-format upload is a
                // separate pre-existing mismatch this does not worsen.
                transition_whole_image_to_transfer_dst(
                    device,
                    cb,
                    image,
                    clear_aspect_for(dst.desc().format),
                );
                device.cmd_copy_buffer_to_image(
                    cb,
                    src_raw,
                    image,
                    ash::vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    &raw,
                );
            }

            Command::PipelineBarrier {
                src_stage,
                dst_stage,
                memory_barriers,
                buffer_barriers,
                texture_barriers,
            } => {
                let src = convert::pipeline_stage(*src_stage);
                let dst = convert::pipeline_stage(*dst_stage);
                let mut mem: Vec<ash::vk::MemoryBarrier> = memory_barriers
                    .iter()
                    .map(|b| ash::vk::MemoryBarrier {
                        src_access_mask: convert::access_flags(b.src_access),
                        dst_access_mask: convert::access_flags(b.dst_access),
                        ..Default::default()
                    })
                    .collect();
                let mut buf: Vec<ash::vk::BufferMemoryBarrier> = Vec::new();
                for b in buffer_barriers {
                    buf.push(ash::vk::BufferMemoryBarrier {
                        src_access_mask: convert::access_flags(b.src_access),
                        dst_access_mask: convert::access_flags(b.dst_access),
                        src_queue_family_index: ash::vk::QUEUE_FAMILY_IGNORED,
                        dst_queue_family_index: ash::vk::QUEUE_FAMILY_IGNORED,
                        buffer: ash::vk::Buffer::from_raw(Translated::handle(
                            "barrier buffer",
                            b.buffer.backend(),
                        )?),
                        // `VK_WHOLE_SIZE` rather than the barrier's own size: a
                        // zero size means "the whole buffer", and a non-zero one
                        // that disagrees with the caller's intent is not
                        // something the RHI can second-guess here. The offset is
                        // passed through because a partial range is a real use.
                        offset: b.offset,
                        size: ash::vk::WHOLE_SIZE,
                        ..Default::default()
                    });
                }
                let mut tex: Vec<ash::vk::ImageMemoryBarrier> = Vec::new();
                for b in texture_barriers {
                    let image = ash::vk::Image::from_raw(Translated::handle(
                        "barrier texture",
                        b.texture.backend(),
                    )?);
                // The aspect mask must name exactly the aspects the image has, and
                // the *format* is what decides that, not the usage. Bug №290: this
                // used to always pass COLOR|DEPTH|STENCIL, which names aspects the
                // image does not have — `VUID-VkImageMemoryBarrier-aspectMask`.
                //
                // Keying off the usage bit instead looks equivalent and is not: a
                // texture created with `DEPTH_STENCIL_ATTACHMENT` and format
                // `D32_SFLOAT` has depth only, so `DEPTH|STENCIL` is itself a
                // violation —
                // `VUID-VkImageMemoryBarrier-subresourceRange-09601`. That was
                // caught by running this under the validator, not by reading it.
                //
                // Depth-only formats take DEPTH alone, dual-purpose ones take both,
                // and anything with neither is a colour image.
                let format = b.texture.desc().format;
                let aspect_mask = match (format.is_depth(), format.is_stencil()) {
                    (true, true) => {
                        ash::vk::ImageAspectFlags::DEPTH | ash::vk::ImageAspectFlags::STENCIL
                    }
                    (true, false) => ash::vk::ImageAspectFlags::DEPTH,
                    (false, true) => ash::vk::ImageAspectFlags::STENCIL,
                    (false, false) => ash::vk::ImageAspectFlags::COLOR,
                };
                // Each layout change is also a barrier, so the full subresource
                // range is covered: leaving a mip level behind means it keeps the old
                // layout while its siblings move on, which is undefined behaviour
                // rather than a slow path.
                tex.push(ash::vk::ImageMemoryBarrier {
                    src_access_mask: convert::access_flags(b.src_access),
                    dst_access_mask: convert::access_flags(b.dst_access),
                    old_layout: convert::texture_layout(b.old_layout),
                    new_layout: convert::texture_layout(b.new_layout),
                    src_queue_family_index: ash::vk::QUEUE_FAMILY_IGNORED,
                    dst_queue_family_index: ash::vk::QUEUE_FAMILY_IGNORED,
                    image,
                    subresource_range: ash::vk::ImageSubresourceRange {
                        aspect_mask,
                        base_mip_level: 0,
                        level_count: ash::vk::REMAINING_MIP_LEVELS,
                        base_array_layer: 0,
                        layer_count: ash::vk::REMAINING_ARRAY_LAYERS,
                    },
                        ..Default::default()
                    });
                }
                device.cmd_pipeline_barrier(
                    cb,
                    src,
                    dst,
                    ash::vk::DependencyFlags::empty(),
                    &mem,
                    &buf,
                    &tex,
                );
                // Kept alive for the duration of the call above; ash borrows
                // these slices, so they must outlive it.
                let _ = (&mem, &buf, &tex);
                mem.clear();
            }

            Command::NextSubpass => {
                device.cmd_next_subpass2(
                    cb,
                    &ash::vk::SubpassBeginInfo {
                        contents: ash::vk::SubpassContents::INLINE,
                        ..Default::default()
                    },
                    &ash::vk::SubpassEndInfo::default(),
                );
            }

            Command::BeginDebugLabel { label, .. } => {
                // Reported rather than dropped. The command entry points live on
                // the `VK_EXT_debug_utils` device extension loader, not on
                // `ash::Device`, and this translator is handed a plain
                // `ash::Device` — so there is nothing to call, and silently
                // ignoring the marker would make a capture tool show unnamed
                // regions with no hint that markers were ever requested.
                return Err(RhiError::NotSupported(format!(
                    "debug label {label:?} cannot be recorded: the translator has no \
                     VK_EXT_debug_utils device loader, so markers would be dropped"
                )));
            }

            Command::EndDebugLabel => {
                return Err(RhiError::NotSupported(
                    "EndDebugLabel cannot be recorded for the same reason as BeginDebugLabel"
                        .into(),
                ));
            }

            Command::DispatchRays { width, height, depth } => {
                return Err(RhiError::NotSupported(format!(
                    "ray tracing dispatch ({width}x{height}x{depth}) has no Vulkan path: \
                     the backend never enabled VK_KHR_ray_tracing_pipeline"
                )));
            }

            Command::BuildAccelerationStructure => {
                return Err(RhiError::NotSupported(
                    "BuildAccelerationStructure has no Vulkan path: \
                     BLAS/TLAS construction is not implemented in the backend"
                        .into(),
                ));
            }
        }
    }
    Ok(())
}

/// Refuse a clear that names 2D rects, loudly, instead of clearing everything.
///
/// `vkCmdClearColorImage` and `vkCmdClearDepthStencilImage` take a subresource
/// range and have no per-rect form. Honouring a partial clear needs a render
/// pass whose attachment is cleared with `vkCmdClearAttachments` and
/// `VkClearRect` — and the render pass and framebuffer it needs must outlive
/// the submission that recorded them, which means a frame-lifetime retirement
/// queue this backend does not have.
///
/// Refusing beats the previous behaviour, which ignored the rects and cleared
/// the whole image: a caller that asked for a quarter of a texture and got all
/// of it, with the command succeeding, is a corruption that surfaces as a wrong
/// pixel much later and cannot be traced back here.
fn refuse_partial_clear(
    command: &str,
    rects: &[crate::types::Rect2D],
) -> RhiResult<()> {
    if rects.is_empty() {
        return Ok(());
    }
    Err(RhiError::BackendError(format!(
        "{command} was given {} rect(s): a per-rect clear needs a render pass with \
         ClearAttachments, which this backend does not build yet; refusing rather \
         than clearing the whole image silently",
        rects.len()
    )))
}

/// Put an image into `TRANSFER_DST_OPTIMAL` for a writer that overwrites it whole.
///
/// `ClearColor`, `ClearDepthStencil` and `CopyBufferToTexture` name
/// `TRANSFER_DST_OPTIMAL` in the command itself, so the image must be in that
/// layout when the command executes
/// (`VUID-vkCmdClearColorImage-imageLayout-00012`,
/// `VUID-vkCmdClearDepthStencilImage-imageLayout-00012`, and the copy's
/// counterpart). Nothing used to put it there: the only transition in the system
/// was an explicit `PipelineBarrier` command, and a clear that never goes
/// through one failed validation before writing a single pixel — the second of
/// the two `VUID`s behind `a_registered_image_is_actually_sampled_and_drawn`.
///
/// `oldLayout` is `UNDEFINED` deliberately. All three commands discard the
/// image's previous contents from end to end, so there is nothing to preserve
/// and no availability to arrange for the content itself — and
/// `UNDEFINED -> TRANSFER_DST_OPTIMAL` is a legal transition from *any* real
/// layout, which makes it the one statement about the past that cannot be
/// stated wrongly. Execution order against earlier accesses in the same command
/// buffer is still owed, so the barrier waits on everything before it
/// (`ALL_COMMANDS` + `MEMORY_READ | MEMORY_WRITE`) instead of `TOP_OF_PIPE`:
/// a texture copied, cleared and cleared again in one buffer otherwise risks
/// finishing in the wrong order, and these commands are rare enough that the
/// wider wait costs nothing. The other half of the contract — sampling a
/// `TRANSFER_DST` image — is the UI's barrier before the render pass, kept on
/// its side by AGENT-SBF2; neither side must add the other's, or the second
/// barrier would name a stale `oldLayout`.
fn transition_whole_image_to_transfer_dst(
    device: &ash::Device,
    cb: ash::vk::CommandBuffer,
    image: ash::vk::Image,
    aspect_mask: ash::vk::ImageAspectFlags,
) {
    let barriers = [ash::vk::ImageMemoryBarrier {
        src_access_mask: ash::vk::AccessFlags::MEMORY_READ
            | ash::vk::AccessFlags::MEMORY_WRITE,
        dst_access_mask: ash::vk::AccessFlags::TRANSFER_WRITE,
        old_layout: ash::vk::ImageLayout::UNDEFINED,
        new_layout: ash::vk::ImageLayout::TRANSFER_DST_OPTIMAL,
        src_queue_family_index: ash::vk::QUEUE_FAMILY_IGNORED,
        dst_queue_family_index: ash::vk::QUEUE_FAMILY_IGNORED,
        image,
        subresource_range: ash::vk::ImageSubresourceRange {
            aspect_mask,
            base_mip_level: 0,
            level_count: ash::vk::REMAINING_MIP_LEVELS,
            base_array_layer: 0,
            layer_count: ash::vk::REMAINING_ARRAY_LAYERS,
        },
        ..Default::default()
    }];
    unsafe {
        // SAFETY: `barriers` is a local array living across the call ash
        // borrows it for; the image handle was resolved by the caller from a
        // live resource; the stage/access pair matches the writer that follows.
        device.cmd_pipeline_barrier(
            cb,
            ash::vk::PipelineStageFlags::ALL_COMMANDS,
            ash::vk::PipelineStageFlags::TRANSFER,
            ash::vk::DependencyFlags::empty(),
            &[],
            &[],
            &barriers,
        );
    }
}

/// One subresource range per mip level of a clear.
///
/// Vulkan clears per `ImageSubresourceRange`, and a clear that only covered mip 0
/// would leave the rest of a mipped texture uninitialised, so the clear is emitted
/// once per level — and each of those ranges has to name **its own** level.
///
/// Bug #639: the ranges were all the same range with `base_mip_level: 0` and
/// `level_count: 1`, repeated `mip_levels` times. So the clear covered mip 0
/// `mip_levels` times and mips 1..N-1 not at all: the levels above the first
/// kept whatever they had — uninitialised memory for a fresh texture — and the
/// repetitions were redundant work inside the render pass.
///
/// The aspects a clear's subresource range is allowed to name, derived from the
/// **format** rather than from the usage.
///
/// A colour image has colour aspect only, so `cmd_clear_color_image` rejects a
/// range naming DEPTH or STENCIL
/// (`VUID-vkCmdClearColorImage-aspectMask-02498`), and a depth-only format
/// rejects STENCIL for the same reason. Emitting `COLOR|DEPTH|STENCIL` because
/// "it's the image's aspects, broadly speaking" is not a superset that happens
/// to work — it is a claim about aspects the format does not have, and the call
/// is refused.
///
/// Same reasoning as the barrier path (Bug #290), where the identical mistake
/// was already corrected: the format decides, and keying off the usage bit
/// instead of it is itself the violation.
fn clear_aspect_for(format: crate::types::Format) -> ash::vk::ImageAspectFlags {
    match (format.is_depth(), format.is_stencil()) {
        (true, true) => ash::vk::ImageAspectFlags::DEPTH | ash::vk::ImageAspectFlags::STENCIL,
        (true, false) => ash::vk::ImageAspectFlags::DEPTH,
        (false, true) => ash::vk::ImageAspectFlags::STENCIL,
        (false, false) => ash::vk::ImageAspectFlags::COLOR,
    }
}

fn subresource_ranges(
    mip_levels: u32,
    aspect_mask: ash::vk::ImageAspectFlags,
) -> Vec<ash::vk::ImageSubresourceRange> {
    (0..mip_levels.max(1))
        .map(|mip| ash::vk::ImageSubresourceRange {
            aspect_mask,
            base_mip_level: mip,
            level_count: 1,
            base_array_layer: 0,
            layer_count: 1,
        })
        .collect()
}

fn clear_value(value: &crate::types::ClearValue) -> ash::vk::ClearValue {
    match value {
        crate::types::ClearValue::Color { r, g, b, a } => ash::vk::ClearValue {
            color: ash::vk::ClearColorValue {
                float32: [*r, *g, *b, *a],
            },
        },
        crate::types::ClearValue::DepthStencil { depth, stencil } => ash::vk::ClearValue {
            depth_stencil: ash::vk::ClearDepthStencilValue {
                depth: *depth,
                stencil: *stencil,
            },
        },
    }
}

// At the end of the file on purpose: clippy's `items_after_test_module` fires when
// anything follows a `mod tests`, and a warning here is a warning every future
// contributor learns to ignore.

#[cfg(test)]
mod clear_subresources {
    use super::subresource_ranges;
    use ash::vk;

    /// Bug #639: a clear has to name every mip level it covers, and each range
    /// has to name **its own** level.
    ///
    /// The ranges used to be the same range repeated: `base_mip_level: 0` and
    /// `level_count: 1`, once per level. So the clear covered mip 0 N times
    /// and mips 1..N-1 not at all — the levels above the first kept
    /// uninitialised memory, and the repetitions were redundant work inside the
    /// render pass.
    #[test]
    fn every_mip_level_gets_its_own_range() {
        let ranges = subresource_ranges(4, vk::ImageAspectFlags::COLOR);
        assert_eq!(ranges.len(), 4, "one range per mip level");
        for (mip, range) in ranges.iter().enumerate() {
            let mip = mip as u32;
            assert_eq!(
                range.base_mip_level, mip,
                "range {mip} clears mip {} instead of mip {mip}",
                range.base_mip_level
            );
            assert_eq!(
                range.level_count, 1,
                "range {mip} covers {} levels; a clear covers one level at a time",
                range.level_count
            );
            assert_eq!(range.aspect_mask, vk::ImageAspectFlags::COLOR);
        }
    }

    /// A texture with no mips still has to be cleared, so the range list is
    /// never empty. An empty list would make `vkCmdClearColorImage` a no-op
    /// at best and a validation error at worst.
    #[test]
    fn a_texture_without_mips_still_gets_one_range() {
        let ranges = subresource_ranges(0, vk::ImageAspectFlags::COLOR);
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].base_mip_level, 0);
        assert_eq!(ranges[0].level_count, 1);
    }

    /// The aspect comes from the format, not from the usage. A colour image
    /// has colour aspect only, and naming DEPTH or STENCIL for it is refused
    /// by the driver (`VUID-vkCmdClearColorImage-aspectMask-02498`).
    ///
    /// This is a property of the caller's choice of aspect, checked here so
    /// that the range builder stays a pure function of (levels, aspect) and
    /// nobody widens it by accident.
    #[test]
    fn the_aspect_is_carried_through_unchanged() {
        let ranges = subresource_ranges(2, vk::ImageAspectFlags::DEPTH);
        for range in &ranges {
            assert_eq!(range.aspect_mask, vk::ImageAspectFlags::DEPTH);
        }
    }
}
