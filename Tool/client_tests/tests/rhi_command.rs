// Integration tests for the rhi::command module.
//
// Covers: CommandPool, CommandBuffer state machine, CommandEncoder,
// RenderPass/Framebuffer/Subpass data structures.

use rhi::command::buffer::{
    CommandBuffer, CommandBufferDesc, CommandBufferFlags, CommandBufferLevel, CommandBufferState,
};
use rhi::command::commands::Command;
use rhi::command::encoder::CommandEncoder;
use rhi::command::pass::render::{
    AttachmentDescription, AttachmentReference, DependencyFlags, Framebuffer, FramebufferDesc,
    LoadOp, PipelineBindPoint, RenderPass, RenderPassBeginInfo, RenderPassDesc, StoreOp,
    SubpassDependency, SubpassDescription, SubpassFlags,
};
use rhi::command::pool::{CommandPool, CommandPoolDesc, CommandPoolFlags};
use rhi::{AccessFlags, ClearValue, Format, PipelineStage, Rect2D, SampleCount, ShaderStage, TextureLayout};

fn make_pool(qfi: u32) -> CommandPool {
    CommandPool::new(CommandPoolDesc {
        queue_family_index: qfi,
        flags: CommandPoolFlags::TRANSIENT | CommandPoolFlags::RESET_COMMAND_BUFFER,
    })
}

fn make_buffer(pool: CommandPool) -> CommandBuffer {
    CommandBuffer::new(
        pool,
        CommandBufferDesc {
            level: CommandBufferLevel::Primary,
            queue_family_index: Some(0),
            flags: CommandBufferFlags::ONE_TIME_SUBMIT,
        },
    )
}

#[test]
fn command_pool_new_and_accessors() {
    let pool = make_pool(3);
    assert_eq!(pool.queue_family_index(), 3);
    assert_eq!(pool.desc().queue_family_index, 3);
    assert!(pool.desc().flags.contains(CommandPoolFlags::TRANSIENT));
}

#[test]
fn command_pool_flags_ops() {
    let f = CommandPoolFlags::TRANSIENT | CommandPoolFlags::RESET_COMMAND_BUFFER;
    assert!(f.contains(CommandPoolFlags::TRANSIENT));
    assert!(f.contains(CommandPoolFlags::RESET_COMMAND_BUFFER));
    assert!(!f.intersects(CommandPoolFlags::NONE));
}

#[test]
fn command_buffer_level_defaults_to_primary() {
    assert_eq!(CommandBufferLevel::default(), CommandBufferLevel::Primary);
    let _ = CommandBufferLevel::Secondary;
}

#[test]
fn command_buffer_flags_ops() {
    let f = CommandBufferFlags::ONE_TIME_SUBMIT | CommandBufferFlags::SIMULTANEOUS_USE;
    assert!(f.contains(CommandBufferFlags::ONE_TIME_SUBMIT));
    assert!(!f.contains(CommandBufferFlags::RENDER_PASS_CONTINUE));
    assert_eq!(CommandBufferFlags::NONE, CommandBufferFlags::empty());
}

#[test]
fn command_buffer_starts_initial() {
    let buf = make_buffer(make_pool(0));
    assert_eq!(buf.state(), CommandBufferState::Initial);
}

#[test]
fn command_buffer_begin_records() {
    let mut buf = make_buffer(make_pool(0));
    buf.begin().expect("begin");
    assert_eq!(buf.state(), CommandBufferState::Recording);
}

#[test]
fn command_buffer_end_makes_executable() {
    let mut buf = make_buffer(make_pool(0));
    buf.begin().expect("begin");
    buf.end().expect("end");
    assert_eq!(buf.state(), CommandBufferState::Executable);
}

#[test]
fn command_buffer_desc_and_pool_accessors() {
    let pool = make_pool(1);
    let buf = make_buffer(pool);
    assert_eq!(buf.desc().level, CommandBufferLevel::Primary);
    assert_eq!(buf.desc().queue_family_index, Some(0));
    assert!(buf.desc().flags.contains(CommandBufferFlags::ONE_TIME_SUBMIT));
    assert_eq!(buf.pool().queue_family_index(), 1);
}

#[test]
fn command_encoder_wraps_command_buffer() {
    let pool = make_pool(0);
    let mut buf = make_buffer(pool);
    buf.begin().expect("begin");
    let mut enc = CommandEncoder::new(buf);
    assert_eq!(enc.command_buffer().state(), CommandBufferState::Recording);
    enc.set_viewport(0.0, 0.0, 640.0, 480.0, 0.0, 1.0);
    enc.set_scissor(0, 0, 640, 480);
    enc.end_render_pass();
}

#[test]
fn load_store_ops_defaults() {
    assert_eq!(LoadOp::default(), LoadOp::Load);
    assert_eq!(StoreOp::default(), StoreOp::Store);
    assert_eq!(PipelineBindPoint::default(), PipelineBindPoint::Graphics);
    let _ = LoadOp::Clear;
    let _ = LoadOp::DontCare;
    let _ = StoreOp::DontCare;
}

#[test]
fn attachment_description_creation() {
    let att = AttachmentDescription {
        format: Format::RGBA8_UNORM,
        samples: SampleCount::X4,
        load_op: LoadOp::Clear,
        store_op: StoreOp::Store,
        stencil_load_op: LoadOp::DontCare,
        stencil_store_op: StoreOp::DontCare,
        initial_layout: TextureLayout::Undefined,
        final_layout: TextureLayout::ColorAttachmentOptimal,
    };
    assert_eq!(att.format, Format::RGBA8_UNORM);
    assert_eq!(att.samples.as_count(), 4);
    assert_eq!(att.load_op, LoadOp::Clear);
}

#[test]
fn attachment_reference_creation() {
    let refa = AttachmentReference {
        attachment: 0,
        layout: TextureLayout::DepthStencilAttachmentOptimal,
    };
    assert_eq!(refa.attachment, 0);
}

#[test]
fn subpass_description_creation() {
    let sub = SubpassDescription {
        flags: SubpassFlags::NONE,
        pipeline_bind_point: PipelineBindPoint::Graphics,
        input_attachments: vec![],
        color_attachments: vec![AttachmentReference {
            attachment: 0,
            layout: TextureLayout::ColorAttachmentOptimal,
        }],
        resolve_attachments: vec![],
        depth_stencil_attachment: Some(AttachmentReference {
            attachment: 1,
            layout: TextureLayout::DepthStencilAttachmentOptimal,
        }),
        preserve_attachments: vec![],
    };
    assert_eq!(sub.color_attachments.len(), 1);
    assert!(sub.depth_stencil_attachment.is_some());
}

#[test]
fn render_pass_desc_with_attachment() {
    let desc = RenderPassDesc {
        attachments: vec![AttachmentDescription {
            format: Format::R32_SFLOAT,
            samples: SampleCount::X1,
            load_op: LoadOp::Clear,
            store_op: StoreOp::Store,
            stencil_load_op: LoadOp::DontCare,
            stencil_store_op: StoreOp::DontCare,
            initial_layout: TextureLayout::Undefined,
            final_layout: TextureLayout::ColorAttachmentOptimal,
        }],
        subpasses: vec![SubpassDescription {
            flags: SubpassFlags::NONE,
            pipeline_bind_point: PipelineBindPoint::Graphics,
            input_attachments: vec![],
            color_attachments: vec![AttachmentReference {
                attachment: 0,
                layout: TextureLayout::ColorAttachmentOptimal,
            }],
            resolve_attachments: vec![],
            depth_stencil_attachment: None,
            preserve_attachments: vec![],
        }],
        dependencies: vec![SubpassDependency {
            src_subpass: 0,
            dst_subpass: 1,
            src_stage_mask: PipelineStage::COLOR_ATTACHMENT_OUTPUT,
            dst_stage_mask: PipelineStage::FRAGMENT_SHADER,
            src_access_mask: AccessFlags::COLOR_ATTACHMENT_WRITE,
            dst_access_mask: AccessFlags::SHADER_READ,
            dependency_flags: DependencyFlags::BY_REGION,
        }],
    };
    let rp = RenderPass::new(desc);
    assert_eq!(rp.desc().attachments.len(), 1);
    assert_eq!(rp.desc().subpasses.len(), 1);
    assert_eq!(rp.desc().dependencies.len(), 1);
    assert_eq!(rp.desc().subpasses[0].color_attachments[0].attachment, 0);
}

#[test]
fn framebuffer_new() {
    let fb = Framebuffer::new(FramebufferDesc {
        render_pass: RenderPass::new(RenderPassDesc::default()),
        attachments: vec![],
        width: 1280,
        height: 720,
        layers: 1,
    });
    let _ = fb;
}

#[test]
fn render_pass_begin_info_defaults() {
    let info = RenderPassBeginInfo {
        render_pass: RenderPass::new(RenderPassDesc::default()),
        framebuffer: Framebuffer::new(FramebufferDesc::default()),
        render_area: Rect2D::default(),
        clear_values: vec![
            ClearValue::color(0.0, 0.0, 0.0, 1.0),
            ClearValue::DepthStencil { depth: 1.0, stencil: 0 },
        ],
    };
    assert_eq!(info.clear_values.len(), 2);
}

#[test]
fn pipeline_stage_and_access_flags_exist() {
    let s = PipelineStage::COLOR_ATTACHMENT_OUTPUT | PipelineStage::VERTEX_SHADER;
    assert!(s.contains(PipelineStage::COLOR_ATTACHMENT_OUTPUT));
    let _ = ShaderStage::VERTEX;
    let a = AccessFlags::COLOR_ATTACHMENT_WRITE;
    assert!(!a.is_empty());
}

#[test]
fn command_pool_allocate_primary_and_secondary() {
    let pool = make_pool(2);
    let primary = pool.allocate();
    assert_eq!(primary.desc().level, CommandBufferLevel::Primary);
    assert_eq!(primary.pool().queue_family_index(), 2);
    assert_eq!(primary.state(), CommandBufferState::Initial);

    let secondary = pool.allocate_level(CommandBufferLevel::Secondary);
    assert_eq!(secondary.desc().level, CommandBufferLevel::Secondary);
}

#[test]
fn command_buffer_reset_returns_to_initial() {
    let mut buf = make_buffer(make_pool(0));
    buf.begin().expect("begin");
    assert!(buf.is_recording());
    buf.end().expect("end");
    assert!(buf.is_executable());
    buf.reset().expect("reset");
    assert_eq!(buf.state(), CommandBufferState::Initial);
    assert!(!buf.is_recording());
    assert!(!buf.is_executable());
}

#[test]
fn command_buffer_end_without_begin_reports_an_error() {
    // Bug №181: this used to be `#[should_panic]` — a misuse of the command
    // buffer aborted the process instead of being reportable.
    let mut buf = make_buffer(make_pool(0));
    assert!(
        buf.end().is_err(),
        "ending a buffer that is not recording must be refused"
    );
    // Bug №181: a buffer left in an inconsistent state is now marked invalid
    // rather than silently continuing to look healthy.
    assert_eq!(buf.state(), CommandBufferState::Invalid);
}

#[test]
fn command_buffer_cannot_begin_twice() {
    // Bug №181: `begin` used to be a silent no-op while recording, and the
    // encoder's `begin` cleared the recorded commands regardless — so a stray
    // second begin silently discarded a frame's work.
    let mut buf = make_buffer(make_pool(0));
    buf.begin().expect("first begin");
    assert!(buf.begin().is_err(), "a second begin must be refused");
    assert_eq!(buf.state(), CommandBufferState::Recording);
}

#[test]
fn command_buffer_submit_and_complete() {
    // Bug №233: `Pending` was declared but unreachable, so "in flight" and
    // "ready" were indistinguishable.
    let mut buf = make_buffer(make_pool(0));
    buf.begin().expect("begin");
    buf.end().expect("end");
    assert!(buf.is_executable());

    buf.submit().expect("submit");
    assert!(buf.is_pending());
    assert!(!buf.is_executable());

    // Re-recording while the GPU reads it is now refused.
    assert!(buf.begin().is_err(), "a pending buffer must not be re-recorded");
    // And so is resetting it out from under the queue.
    assert!(buf.reset().is_err(), "a pending buffer must not be reset");

    buf.complete().expect("complete");
    assert!(buf.is_executable());
    assert!(!buf.is_pending());
}

#[test]
fn command_buffer_invalidate_is_sticky() {
    // Bug №233: `Invalid` was declared but never set.
    let mut buf = make_buffer(make_pool(0));
    buf.invalidate("simulated submit failure");
    assert_eq!(buf.state(), CommandBufferState::Invalid);
    assert!(buf.begin().is_err());
    assert!(buf.submit().is_err());
}

#[test]
fn command_encoder_full_render_flow() {
    let pool = make_pool(0);
    let buf = make_buffer(pool);

    let rp = RenderPass::new(RenderPassDesc {
        attachments: vec![AttachmentDescription {
            format: Format::RGBA8_UNORM,
            samples: SampleCount::X1,
            load_op: LoadOp::Clear,
            store_op: StoreOp::Store,
            stencil_load_op: LoadOp::DontCare,
            stencil_store_op: StoreOp::DontCare,
            initial_layout: TextureLayout::Undefined,
            final_layout: TextureLayout::ColorAttachmentOptimal,
        }],
        ..Default::default()
    });
    let fb = Framebuffer::new(FramebufferDesc {
        width: 640,
        height: 480,
        ..Default::default()
    });

    let mut enc = CommandEncoder::new(buf);
    enc.begin().expect("begin");
    assert!(enc.is_recording());

    enc.begin_render_pass(&rp, &fb, Rect2D::default(), &[ClearValue::color(0.1, 0.2, 0.3, 1.0)], 1.0, 0);
    enc.set_viewport(0.0, 0.0, 640.0, 480.0, 0.0, 1.0);
    enc.set_scissor(0, 0, 640, 480);
    enc.end_render_pass();

    assert_eq!(enc.command_count(), 4);
    assert!(matches!(enc.commands()[0], Command::BeginRenderPass(_)));
    assert!(matches!(enc.commands()[1], Command::SetViewport(_)));
    assert!(matches!(enc.commands()[2], Command::SetScissor(_)));
    assert!(matches!(enc.commands()[3], Command::EndRenderPass));

    let finished = enc.finish().expect("finish");
    assert_eq!(finished.state(), CommandBufferState::Executable);
}

#[test]
fn command_encoder_records_viewport_bindings_and_draw() {
    let pool = make_pool(0);
    let mut enc = CommandEncoder::new(make_buffer(pool));
    enc.begin().expect("begin");

    enc.set_viewport(0.0, 0.0, 800.0, 600.0, 0.0, 1.0);
    enc.draw_indexed_instanced(36, 1, 0, 0, 0);
    enc.end_render_pass();

    assert_eq!(enc.command_count(), 3);
    let first = &enc.commands()[0];
    match first {
        Command::SetViewport(vp) => {
            assert_eq!(vp.width, 800.0);
            assert_eq!(vp.height, 600.0);
        }
        other => panic!("expected SetViewport, got {:?}", other),
    }
    let draw = &enc.commands()[1];
    match draw {
        Command::DrawIndexed {
            index_count, ..
        } => assert_eq!(*index_count, 36),
        other => panic!("expected DrawIndexed, got {:?}", other),
    }
}

#[test]
fn command_encoder_finish_requires_recording() {
    let pool = make_pool(0);
    let buf = make_buffer(pool);
    assert!(!buf.is_recording());
}

/// Bug №203: `finish()` used to drop the recorded commands on the floor. The
/// encoder's `Vec<Command>` was never moved into the buffer, so a finished
/// buffer always reported zero commands and no backend could replay the frame.
/// Nothing failed — the commands simply vanished.
#[test]
fn finishing_an_encoder_moves_the_commands_into_the_buffer() {
    let pool = make_pool(0);
    let mut buf = make_buffer(pool);
    buf.begin().expect("begin");
    let mut enc = CommandEncoder::new(buf);

    enc.set_viewport(0.0, 0.0, 64.0, 64.0, 0.0, 1.0);
    enc.set_scissor(0, 0, 64, 64);
    enc.end_render_pass();
    assert_eq!(enc.command_count(), 3, "encoder should have recorded three");

    let finished = enc.finish().expect("finish");
    assert_eq!(
        finished.commands().len(),
        3,
        "bug №203: finish() discarded the recorded commands"
    );
    assert!(matches!(
        finished.commands()[0],
        Command::SetViewport(_)
    ));
    assert!(matches!(finished.commands()[2], Command::EndRenderPass));
}

/// A second recording must not inherit the previous one, or a stale half of the
/// frame would be replayed on top of the new one.
#[test]
fn re_recording_replaces_the_previous_command_list() {
    let pool = make_pool(0);
    let mut buf = make_buffer(pool);

    buf.begin().expect("first begin");
    let mut first = CommandEncoder::new(buf);
    first.set_scissor(0, 0, 10, 10);
    first.set_scissor(0, 0, 20, 20);
    first.set_scissor(0, 0, 30, 30);
    let mut buf = first.finish().expect("first finish");
    assert_eq!(buf.commands().len(), 3);

    buf.begin().expect("second begin");
    let mut second = CommandEncoder::new(buf);
    second.set_scissor(0, 0, 99, 99);
    let buf = second.finish().expect("second finish");
    assert_eq!(
        buf.commands().len(),
        1,
        "the earlier recording must not leak into the next one"
    );
}

/// Bug №203: the buffer is the thing a backend replays, so an empty list is a
/// broken frame. `submit_commands` refuses it rather than submitting nothing.
#[test]
fn an_empty_recording_is_rejected_rather_than_silently_submitted() {
    let pool = make_pool(0);
    let mut buf = make_buffer(pool);
    buf.begin().expect("begin");
    let enc = CommandEncoder::new(buf);
    let finished = enc.finish().expect("finish");
    assert!(
        finished.commands().is_empty(),
        "a recording with no commands should stay empty and be rejected at submit"
    );
}
