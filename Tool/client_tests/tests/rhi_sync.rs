// Integration tests for the rhi::sync module.
//
// Covers: Fence, Semaphore, TimelineSemaphore, barriers and copy regions.

use rhi::resource::texture::{Texture, TextureDesc};
use rhi::resource::buffer::{Buffer, BufferDesc};
use rhi::sync::barrier::{
    BufferBarrier, BufferCopy, BufferTextureCopy, MemoryBarrier, PipelineBarrier, TextureBarrier,
};
use rhi::sync::fence::{Fence, FenceDesc};
use rhi::sync::semaphore::{Semaphore, SemaphoreDesc, SemaphoreFlags, TimelineSemaphore};
use rhi::{AccessFlags, BufferUsage, PipelineStage, TextureLayout, TextureUsage};

#[test]
fn fence_new_and_desc() {
    let fence = Fence::new(FenceDesc { signaled: true });
    assert!(fence.desc().signaled);
    let fence = Fence::new(FenceDesc { signaled: false });
    assert!(!fence.desc().signaled);
}

#[test]
fn fence_desc_default() {
    let d = FenceDesc::default();
    assert!(!d.signaled);
}

#[test]
fn semaphore_new_and_desc() {
    let sem = Semaphore::new(SemaphoreDesc { flags: SemaphoreFlags::NONE });
    assert_eq!(sem.desc().flags, SemaphoreFlags::NONE);
}

#[test]
fn semaphore_desc_default() {
    let d = SemaphoreDesc::default();
    assert!(d.flags.is_empty());
}

#[test]
fn timeline_semaphore_new() {
    let ts = TimelineSemaphore::new(42);
    let _ = ts;
}

#[test]
fn memory_barrier_creation() {
    let barrier = MemoryBarrier {
        src_access: AccessFlags::HOST_WRITE,
        dst_access: AccessFlags::SHADER_READ,
    };
    assert!(!barrier.src_access.is_empty());
    assert!(!barrier.dst_access.is_empty());
}

#[test]
fn buffer_barrier_creation() {
    let buffer = Buffer::new(BufferDesc {
        size: 1024,
        usage: BufferUsage::UNIFORM,
        ..Default::default()
    });
    let barrier = BufferBarrier {
        buffer,
        src_access: AccessFlags::HOST_WRITE,
        dst_access: AccessFlags::UNIFORM_READ,
        offset: 0,
        size: 1024,
    };
    assert_eq!(barrier.size, 1024);
}

#[test]
fn texture_barrier_creation() {
    let texture = Texture::new(TextureDesc {
        width: 256,
        height: 256,
        usage: TextureUsage::SAMPLED,
        ..Default::default()
    });
    let barrier = TextureBarrier {
        texture,
        src_access: AccessFlags::COLOR_ATTACHMENT_WRITE,
        dst_access: AccessFlags::SHADER_READ,
        old_layout: TextureLayout::ColorAttachmentOptimal,
        new_layout: TextureLayout::ShaderReadOnlyOptimal,
    };
    assert_eq!(barrier.old_layout, TextureLayout::ColorAttachmentOptimal);
}

#[test]
fn pipeline_barrier_creation() {
    let pbar = PipelineBarrier {
        src_stage: PipelineStage::COLOR_ATTACHMENT_OUTPUT,
        dst_stage: PipelineStage::FRAGMENT_SHADER,
        memory_barriers: vec![MemoryBarrier::default()],
        buffer_barriers: vec![],
        texture_barriers: vec![],
    };
    assert_eq!(pbar.memory_barriers.len(), 1);
}

#[test]
fn buffer_copy_regions() {
    let copy = BufferCopy {
        src_offset: 0,
        dst_offset: 16,
        size: 256,
    };
    assert_eq!(copy.dst_offset, 16);

    let tex_copy = BufferTextureCopy {
        buffer_offset: 0,
        size: 4096,
    };
    assert_eq!(tex_copy.size, 4096);
}

#[test]
    fn fence_state_machine() {
        let fence = Fence::new(FenceDesc { signaled: false });
        assert!(!fence.get_status().unwrap());
        // Bug №181: `wait` used to ignore its timeout and answer instantly, so
        // these polls now pass an explicit `Some(0)` to say "check now" — a
        // `None` would mean the one-second default budget.
        assert!(!fence.wait(Some(0)).unwrap());

    fence.signal().unwrap();
    assert!(fence.get_status().unwrap());
    assert!(fence.wait(None).unwrap());

    fence.reset().unwrap();
    assert!(!fence.get_status().unwrap());
    assert!(!fence.wait(Some(0)).unwrap());
}

#[test]
fn fence_signal_constructor() {
    let signaled = Fence::from_desc(FenceDesc { signaled: true });
    assert!(signaled.wait(None).unwrap());
}

#[test]
fn semaphore_signaling() {
    let sem = Semaphore::new(SemaphoreDesc::default());
    assert!(!sem.signaled());
    sem.signal();
    assert!(sem.signaled());
    sem.reset();
    assert!(!sem.signaled());
}

#[test]
fn timeline_semaphore_value_progression() {
    let ts = TimelineSemaphore::new(5);
    assert_eq!(ts.initial_value(), 5);
    assert_eq!(ts.get_value().unwrap(), 5);

    // Bug №182: `wait` used to ignore its timeout and answer instantly. An
    // explicit `Some(0)` is a non-blocking poll; `None` is a one-second budget.
    assert!(!ts.wait(10, Some(0)).unwrap());

    ts.signal(10).unwrap();
    assert!(ts.wait(10, None).unwrap());
    assert!(ts.wait(8, None).unwrap());
    assert!(!ts.wait(11, Some(0)).unwrap());

    // Bug №182: a timeline semaphore may not go backwards.
    assert!(ts.signal(9).is_err());
    assert_eq!(ts.get_value().unwrap(), 10, "the value must not move back");
}

#[test]
fn timeline_semaphore_actually_blocks_until_signalled() {
    use std::sync::Arc;
    let ts = Arc::new(TimelineSemaphore::new(0));
    let writer = ts.clone();
    let handle = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(30));
        writer.signal(1).unwrap();
    });

    let start = std::time::Instant::now();
    assert!(ts.wait(1, Some(5_000)).unwrap());
    assert!(
        start.elapsed() >= std::time::Duration::from_millis(25),
        "wait returned before the signal arrived"
    );
    handle.join().unwrap();
}