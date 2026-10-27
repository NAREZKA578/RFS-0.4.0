// Integration tests for the Vulkan resource layer.
//
// These tests require a real Vulkan driver and create actual GPU buffers,
// textures, image views and samplers through `VulkanBackend`.

use rhi::backend::vulkan::VulkanBackend;
use rhi::command::commands::Command;
use rhi::config::RhiConfig;
use rhi::core::device::{Device, DeviceDesc};
use rhi::resource::texture::{TextureDesc, TextureLayout};
use rhi::resource::{Sampler, SamplerDesc, Texture, TextureViewDesc, TextureViewType};
use rhi::shader::module::{ShaderFormat, ShaderModuleDesc};
use rhi::sync::barrier::TextureBarrier;
use rhi::types::{
    AccessFlags, ClearValue, Features, Format, PipelineStage, SampleCount, SharingMode,
    TextureDimensions, TextureUsage,
};
use rhi::{
    AddressMode, AttachmentDescription, AttachmentReference, Backend, BufferUsage, CompareOp,
    FilterMode, LoadOp, PipelineBindPoint, RenderPassDesc, StoreOp, SubpassDescription,
    TextureAspectFlags,
};

fn open_vulkan_device(features: Features) -> Device {
    open_vulkan_device_with_validation(features, false)
}

/// Same, but able to turn the validation layer on.
///
/// Separate from [`open_vulkan_device`] because validation is off in
/// `RhiConfig::default()`, and that is the correct default for shipped builds but
/// the wrong one for these tests. A test that claims "the validation layer is
/// quiet" while the layer was never enabled proves nothing at all — it cannot
/// distinguish a correct barrier from a broken one, because both produce silence.
/// Any test whose value depends on the validator reporting something must enable
/// it explicitly.
fn open_vulkan_device_with_validation(features: Features, validation: bool) -> Device {
    let config = RhiConfig {
        validation,
        ..Default::default()
    };
    let backend = VulkanBackend::new(&config).unwrap();
    let devices = backend.enumerate_physical_devices().unwrap();
    let desc = DeviceDesc {
        features,
        ..Default::default()
    };
    backend.create_device(&devices[0], &desc).unwrap()
}

#[test]
fn vulkan_buffer_host_upload_roundtrip() {
    let device = open_vulkan_device(Features::default());
    let mut buffer = device
        .create_buffer(
            64,
            BufferUsage::STORAGE | BufferUsage::TRANSFER_SRC | BufferUsage::TRANSFER_DST,
            true,
        )
        .expect("the host-visible storage buffer must be created");
    assert!(buffer.has_gpu_backing());
    assert!(buffer.is_storage());
    assert!(buffer.is_host_visible());

    device
        .upload_buffer(&buffer, &[1u32, 2, 3, 4, 5, 6, 7, 8])
        .expect("the first upload must succeed; without this a failed write is \
                 indistinguishable from a successful one, and the readback below \
                 would be asserting against memory nobody wrote");

    let data = device.buffer_data(&buffer).unwrap();
    assert_eq!(data.len(), 64);
    let words = unsafe { std::slice::from_raw_parts(data.as_ptr() as *const u32, 8) };
    assert_eq!(words, &[1, 2, 3, 4, 5, 6, 7, 8]);

    device
        .upload_buffer(&buffer, &[9u32, 9, 9, 9])
        .expect("the second upload must succeed");
    let data = device.buffer_data(&buffer).unwrap();
    let words = unsafe { std::slice::from_raw_parts(data.as_ptr() as *const u32, 4) };
    assert_eq!(words, &[9, 9, 9, 9]);

    buffer.set_device_address(0xABCD_DEAD);
    assert_eq!(buffer.device_address(), Some(0xABCD_DEAD));
}

#[test]
fn vulkan_device_local_buffer() {
    let device = open_vulkan_device(Features::default());
    let buffer = device
        .create_buffer(1 << 20, BufferUsage::VERTEX, false)
        .expect("the device-local vertex buffer must be created");
    assert!(buffer.has_gpu_backing());
    assert!(buffer.is_vertex());
    assert!(!buffer.is_host_visible());
    assert_eq!(buffer.size(), 1 << 20);
}

#[test]
fn vulkan_buffer_device_address() {
    let features = Features {
        buffer_device_address: true,
        ..Default::default()
    };
    let device = open_vulkan_device(features);
    let buffer = device
        .create_buffer(
            256,
            BufferUsage::STORAGE | BufferUsage::SHADER_DEVICE_ADDRESS,
            false,
        )
        .expect("the device-address buffer must be created");
    assert!(buffer.has_gpu_backing());
    assert!(buffer.device_address().is_some());
}

#[test]
fn vulkan_texture_image_creation() {
    let device = open_vulkan_device(Features::default());
    let texture = device
        .create_texture(
            256,
            256,
            1,
            Format::RGBA8_UNORM,
            TextureUsage::SAMPLED,
            1,
        )
        .expect("the sampled texture must be created");
    assert!(texture.has_gpu_backing());
    assert!(texture.is_sampled());
    assert_eq!(texture.width(), 256);
    assert_eq!(texture.height(), 256);
    assert_eq!(texture.format(), Format::RGBA8_UNORM);
}

#[test]
fn vulkan_texture_view_and_sampler() {
    let features = Features {
        sampler_anisotropy: true,
        ..Default::default()
    };
    let device = open_vulkan_device(features);

    let texture = device
        .create_texture(
            128,
            128,
            1,
            Format::RGBA8_UNORM,
            TextureUsage::SAMPLED | TextureUsage::COLOR_ATTACHMENT,
            1,
        )
        .expect("the sampled texture must be created");
    assert!(texture.has_gpu_backing());

    let view_desc = TextureViewDesc {
        texture: texture.clone(),
        format: None,
        view_type: TextureViewType::D2,
        aspects: TextureAspectFlags::COLOR,
        base_mip_level: 0,
        mip_level_count: 1,
        base_array_layer: 0,
        array_layer_count: 1,
    };
    let view = device.create_texture_view(&texture, &view_desc).unwrap();
    assert!(view.has_gpu_backing());
    assert_eq!(view.view_type(), TextureViewType::D2);

    let sampler = device
        .create_sampler(AddressMode::ClampToEdge, FilterMode::Linear, 8.0)
        .expect("the anisotropic sampler must be created");
    assert!(sampler.has_gpu_backing());
    assert!(sampler.is_anisotropic());
    assert!(sampler.is_linear());
}

#[test]
fn vulkan_sampler_desc_helpers() {
    let device = open_vulkan_device(Features::default());
    let _ = device;
    let shadow = SamplerDesc::shadow_compare(CompareOp::Less);
    assert!(shadow.is_valid());
    assert!(shadow.compare_enable);
    assert!(SamplerDesc::linear_clamp().is_valid());
    let anisotropic = Sampler::new(SamplerDesc::anisotropic(4.0));
    assert!(anisotropic.is_anisotropic());
}

#[test]
fn vulkan_buffer_upload_through_backend_trait() {
    let device = open_vulkan_device(Features::default());
    let buffer = device
        .create_buffer(16, BufferUsage::UNIFORM | BufferUsage::TRANSFER_DST, true)
        .expect("the host-visible uniform buffer must be created");
    let backend = device.backend_resource().unwrap();
    backend
        .upload_buffer(&buffer, &[0x11, 0x22, 0x33, 0x44, 0x55, 0x66])
        .unwrap();
    let data = device.buffer_data(&buffer).unwrap();
    assert_eq!(&data[..6], &[0x11, 0x22, 0x33, 0x44, 0x55, 0x66]);
}

#[test]
fn vulkan_buffer_upload_to_device_local_buffer_via_staging() {
    let device = open_vulkan_device(Features::default());
    let buffer = device
        .create_buffer(32, BufferUsage::STORAGE | BufferUsage::TRANSFER_DST, false)
        .expect("the device-local storage buffer must be created");
    assert!(buffer.has_gpu_backing());
    assert!(!buffer.is_host_visible());
    assert!(buffer.supports_transfer());
    device
        .upload_buffer(&buffer, &[1, 2, 3, 4, 5, 6, 7, 8])
        .expect("a device-local buffer upload goes through staging; if that path \
                 failed the test would still pass, because nothing here reads the \
                 data back — it would only be asserting that creation succeeded");
}

#[test]
fn vulkan_texture_upload_with_mip_chain() {
    let device = open_vulkan_device(Features::default());
    let texture = device
        .create_texture(64, 64, 1, Format::RGBA8_UNORM, TextureUsage::SAMPLED, 4)
        .expect("the mip-chained texture must be created");
    assert!(texture.has_gpu_backing());

    let total = (64 * 64 + 32 * 32 + 16 * 16 + 8 * 8) * 4;
    assert_eq!(texture.mip_size(0), (64, 64, 1));
    assert_eq!(texture.mip_size(1), (32, 32, 1));
    assert_eq!(texture.mip_size(3), (8, 8, 1));
    let data: Vec<u8> = (0..total).map(|i| (i & 0xff) as u8).collect();

    device
        .upload_texture(&texture, &data)
        .expect("uploading a full mip chain must succeed; this test asserts nothing \
                 about the contents, so a discarded error here would leave it green \
                 while the upload did nothing at all");
    let view_desc = TextureViewDesc {
        texture: texture.clone(),
        format: None,
        view_type: TextureViewType::D2,
        aspects: TextureAspectFlags::COLOR,
        base_mip_level: 0,
        mip_level_count: texture.mip_levels(),
        base_array_layer: 0,
        array_layer_count: 1,
    };
    let view = device.create_texture_view(&texture, &view_desc).unwrap();
    assert!(view.has_gpu_backing());
}

// ---------------------------------------------------------------------------
// Bug №289: early release of tracked objects.
//
// The defect was that a texture displaced by a resize was dropped from the Rust
// cache and nothing else: its Vulkan image stayed in the teardown list until the
// device died. One leak per resize step, which no single-frame test can see and a
// long session pays for.
//
// These tests assert on the count *while the program is running*, which is the
// only level at which this symptom is observable. The validation layer's report
// at `vkDestroyDevice` shows a number at the end and says nothing about whether
// it grew.
// ---------------------------------------------------------------------------

/// A texture that is created and released must leave the tracked list as it
/// found it.
#[test]
fn vulkan_released_texture_leaves_the_teardown_list() {
    let device = open_vulkan_device_with_validation(Features::default(), true);

    let before = device.tracked_object_count();
    let texture = device
        .create_texture(
            256,
            256,
            1,
            Format::RGBA8_UNORM,
            TextureUsage::COLOR_ATTACHMENT,
            1,
        )
        .expect("the tracked texture must be created");
    assert_eq!(
        device.tracked_object_count(),
        before + 1,
        "creating a texture must add exactly one tracked object (the image)"
    );

    device.release_texture(&texture);
    assert_eq!(
        device.tracked_object_count(),
        before,
        "release_texture must remove the image from the teardown list, otherwise \
         every resize leaves another texture resident until the device dies"
    );
}

/// The regression test proper: repeated create-and-release, the pattern a resize
/// produces, must not grow the list.
#[test]
fn vulkan_texture_churn_does_not_grow_the_teardown_list() {
    let device = open_vulkan_device_with_validation(Features::default(), true);

    // Settle first so the baseline cannot include one-off allocations from device
    // creation that land later on their own.
    {
        let warmup = device
            .create_texture(
                64,
                64,
                1,
                Format::RGBA8_UNORM,
                TextureUsage::COLOR_ATTACHMENT,
                1,
            )
            .expect("the warmup texture must be created");
        device.release_texture(&warmup);
    }
    let baseline = device.tracked_object_count();

    for step in 1..=32 {
        let texture = device
            .create_texture(
                64 + step,
                64,
                1,
                Format::RGBA8_UNORM,
                TextureUsage::COLOR_ATTACHMENT,
                1,
            )
            .expect("the churn texture must be created");
        device.release_texture(&texture);
        assert_eq!(
            device.tracked_object_count(),
            baseline,
            "step {step}: create-and-release changed the tracked count, so each \
             resize is leaking a texture"
        );
    }
}

/// Releasing twice must be a no-op, not a double free.
///
/// `geometry.rs` takes the old texture out of its `Option` before releasing, so
/// the second release cannot reach the backend from there. This pins that the
/// backend itself is not the thing that would turn a future caller mistake into
/// undefined behaviour: the entry is gone from the list after the first call, so
/// the second finds nothing to destroy.
#[test]
fn vulkan_double_release_is_harmless() {
    let device = open_vulkan_device_with_validation(Features::default(), true);

    let texture = device
        .create_texture(
            32,
            32,
            1,
            Format::RGBA8_UNORM,
            TextureUsage::COLOR_ATTACHMENT,
            1,
        )
        .expect("the release-test texture must be created");

    device.release_texture(&texture);
    let after_first = device.tracked_object_count();
    device.release_texture(&texture);

    assert_eq!(
        device.tracked_object_count(),
        after_first,
        "a second release must not touch the list again — the entry was already \
         removed, and destroying it twice is undefined behaviour"
    );
}

// ---------------------------------------------------------------------------
// Bug №290: the aspect mask on translated texture barriers.
//
// The translator used to hardcode COLOR|DEPTH|STENCIL for every texture barrier.
// Naming an aspect the image does not have is
// `VUID-VkImageMemoryBarrier-aspectMask`.
//
// This is the first test anywhere that actually *executes* the barrier
// translator: `rhi_sync.rs` only builds the barrier structs, and no other test
// ever submitted a `PipelineBarrier`. That gap is the whole reason the defect
// survived — a translation bug in code no test reaches cannot be caught by a
// test that never runs it.
// ---------------------------------------------------------------------------

/// Both aspects in one submit: a colour texture and a depth-stencil texture
/// transitioned side by side.
///
/// The two must not share a mask. Passing the colour mask to the depth image, or
/// the depth mask to the colour image, is exactly the error being guarded, and
/// only a real submit makes the validator check it.
#[test]
fn vulkan_texture_barrier_uses_the_aspect_the_image_actually_has() {
    let device = open_vulkan_device_with_validation(Features::default(), true);

    let color = device
        .create_texture(
            64,
            64,
            1,
            Format::RGBA8_UNORM,
            TextureUsage::COLOR_ATTACHMENT,
            1,
        )
        .expect("the colour texture must be created");
    let depth = device
        .create_texture(
            64,
            64,
            1,
            Format::D32_SFLOAT,
            TextureUsage::DEPTH_STENCIL_ATTACHMENT,
            1,
        )
        .expect("the depth texture must be created");
    let before_errors = rhi::validation_error_count();

    // Sanity check on the predicate the translator keys off, so a failure below
    // can only mean the barrier, not the fixture.
    assert!(!color.is_depth_stencil());
    assert!(depth.is_depth_stencil());

    let commands = [Command::PipelineBarrier {
        src_stage: PipelineStage::TOP_OF_PIPE,
        dst_stage: PipelineStage::COLOR_ATTACHMENT_OUTPUT
            | PipelineStage::EARLY_FRAGMENT_TESTS,
        memory_barriers: vec![],
        buffer_barriers: vec![],
        texture_barriers: vec![
            TextureBarrier {
                texture: color,
                src_access: AccessFlags::empty(),
                dst_access: AccessFlags::COLOR_ATTACHMENT_WRITE,
                old_layout: TextureLayout::Undefined,
                new_layout: TextureLayout::ColorAttachmentOptimal,
            },
            TextureBarrier {
                texture: depth,
                src_access: AccessFlags::empty(),
                dst_access: AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE,
                old_layout: TextureLayout::Undefined,
                new_layout: TextureLayout::DepthStencilAttachmentOptimal,
            },
        ],
    }];

    device
        .submit_recorded(&commands)
        .expect("barrier submit should succeed");

    // Assert on the validator, not on the return value: a Vulkan call made with an
    // invalid parameter still returns `vk::Success` and reports the misuse
    // alongside it, so checking `is_ok()` alone cannot tell a correct barrier from
    // a broken one. See bug №292.
    assert_eq!(
        rhi::validation_error_count(),
        before_errors,
        "the validation layer reported an error for this barrier"
    );
}

/// A depth-stencil image transitioned through a read-only layout on its own.
///
/// Separate from the test above because this is the transition that would break
/// first in practice: a depth texture read by a later pass is the ordinary case,
/// and `DepthStencilReadOnlyOptimal` still carries both aspects, so a translator
/// that narrowed to DEPTH alone would pass the colour test and fail here.
#[test]
fn vulkan_depth_barrier_transitions_to_read_only_layout() {
    let device = open_vulkan_device_with_validation(Features::default(), true);

    let depth = device
        .create_texture(
            128,
            128,
            1,
            Format::D32_SFLOAT,
            TextureUsage::DEPTH_STENCIL_ATTACHMENT,
            1,
        )
        .expect("the read-only depth texture must be created");
    let before_errors = rhi::validation_error_count();

    let commands = [Command::PipelineBarrier {
        src_stage: PipelineStage::TOP_OF_PIPE,
        dst_stage: PipelineStage::FRAGMENT_SHADER,
        memory_barriers: vec![],
        buffer_barriers: vec![],
        texture_barriers: vec![TextureBarrier {
            texture: depth,
            src_access: AccessFlags::empty(),
            dst_access: AccessFlags::SHADER_READ,
            old_layout: TextureLayout::Undefined,
            new_layout: TextureLayout::DepthStencilReadOnlyOptimal,
        }],
    }];

    device
        .submit_recorded(&commands)
        .expect("depth-stencil barrier submit should succeed");

    assert_eq!(
        rhi::validation_error_count(),
        before_errors,
        "the validation layer reported an error for this depth-stencil barrier"
    );
}

// ---------------------------------------------------------------------------
// Bug №318: the aspect mask on translated clear commands.
//
// `subresource_ranges` hardcoded COLOR|DEPTH|STENCIL for every clear, the same
// mistake the barrier translator made (bug №290, tested above). A colour clear
// naming DEPTH or STENCIL is `VUID-vkCmdClearColorImage-aspectMask-02498`, and a
// D32 clear naming COLOR is `VUID-vkCmdClearDepthStencilImage-aspectMask-00011`.
// Both are rejected before any pixel is written, so the whole colour-clear path —
// which the UI capture test depends on — was unusable.
//
// The barrier tests above could not have caught this: they exercise
// `PipelineBarrier`, a different match arm that was already fixed. A defect in
// code no test reaches is invisible to a test that never runs it — the same gap,
// moved one arm down.
// ---------------------------------------------------------------------------

/// A colour clear and a depth clear in one submit.
///
/// The two formats do not share an aspect, so a translator that emits a single
/// mask for both is caught by whichever one the mask is wrong for. Only a real
/// submit makes the validator check the mask at all.
#[test]
fn vulkan_clear_names_only_the_aspect_the_format_has() {
    let device = open_vulkan_device_with_validation(Features::default(), true);

    // TRANSFER_DST is required to clear (`VUID-vkCmdClearColorImage-image-00002`).
    let color = device
        .create_texture(
            32,
            32,
            1,
            Format::RGBA8_UNORM,
            TextureUsage::COLOR_ATTACHMENT | TextureUsage::TRANSFER_DST,
            1,
        )
        .expect("the cleared colour texture must be created");
    let depth = device
        .create_texture(
            32,
            32,
            1,
            Format::D32_SFLOAT,
            TextureUsage::DEPTH_STENCIL_ATTACHMENT | TextureUsage::TRANSFER_DST,
            1,
        )
        .expect("the cleared depth texture must be created");
    let color_view = device
        .create_texture_view(&color, color.create_default_view().desc())
        .expect("colour view");
    let depth_view = device
        .create_texture_view(&depth, depth.create_default_view().desc())
        .expect("depth view");

    // Sanity check on the predicate the translator keys off, so a failure below
    // can only be the clear, not the fixture.
    assert!(!color.is_depth_stencil());
    assert!(depth.is_depth_stencil());

    let before_errors = rhi::validation_error_count();

    let commands = [
        // No transition is commanded here, on purpose. The clears name
        // `TRANSFER_DST_OPTIMAL`, and putting the image into that layout is the
        // translator's own job now — the writer half of the contract with
        // AGENT-SBF2 on `VUID-vkCmdDraw-None-09600`. If it ever stops doing
        // that, the validator reports `...imageLayout-00012` on this submit and
        // this test fails, which is the proof that the self-transition exists;
        // the aspect check below then runs on the same commands.
        Command::ClearColor {
            texture: color_view,
            value: ClearValue::color(0.0, 0.0, 1.0, 1.0),
            rects: vec![],
        },
        Command::ClearDepthStencil {
            texture: depth_view,
            depth: 1.0,
            stencil: 0,
            rects: vec![],
        },
    ];

    device
        .submit_recorded(&commands)
        .expect("clear submit should succeed");

    assert_eq!(
        rhi::validation_error_count(),
        before_errors,
        "the validation layer rejected the clear's aspect mask"
    );
}

/// A clear that names a 2D rect is refused instead of clearing everything.
///
/// `vkCmdClearColorImage` has no per-rect form, and the translator used to ignore
/// the rects and clear the whole image anyway: a caller that asked for a quarter
/// of a texture got all of it, with the command succeeding, which is a
/// corruption that surfaces as a wrong pixel much later and cannot be traced
/// back to the clear.
///
/// The refusal is the contract until a render-pass path with
/// `vkCmdClearAttachments` and frame-lifetime objects exists. When that lands,
/// this test has to be rewritten into the partial-clear proof it can then be —
/// it exists to make sure the silent full clear cannot come back unnoticed.
#[test]
fn vulkan_refuses_a_partial_clear_instead_of_clearing_everything() {
    let device = open_vulkan_device_with_validation(Features::default(), false);

    let color = device
        .create_texture(
            32,
            32,
            1,
            Format::RGBA8_UNORM,
            TextureUsage::COLOR_ATTACHMENT | TextureUsage::TRANSFER_DST,
            1,
        )
        .expect("the texture the clear is refused on must be created");
    let color_view = device
        .create_texture_view(&color, color.create_default_view().desc())
        .expect("colour view");

    let commands = vec![Command::ClearColor {
        texture: color_view,
        value: ClearValue::color(1.0, 0.0, 0.0, 1.0),
        rects: vec![rhi::types::Rect2D {
            offset: rhi::types::Offset2D { x: 0, y: 0 },
            extent: rhi::types::Extent2D {
                width: 16,
                height: 16,
            },
        }],
    }];

    let error = device
        .submit_recorded(&commands)
        .expect_err("a partial clear must be refused, not silently applied in full");
    let text = format!("{error}");
    assert!(
        text.contains("per-rect clear"),
        "the refusal should say why a rect clear is impossible here, got: {text}"
    );
}

// ---------------------------------------------------------------------------
// Bug #642: a refused creation is an error, not a CPU stand-in.
//
// The `Device` creation methods used to swallow the backend's refusal and hand
// back an object with no native handle, reported as a success. That is the
// class of defect #314 was filed about — a pipeline that looked created and
// could not be drawn with — one level down, on the resource path.
//
// A stand-in is worse than an error because the failure shows up later and
// somewhere else: the caller binds handle 0, the draw produces nothing, and
// the report is about the draw.
// ---------------------------------------------------------------------------

/// The Vulkan backend refuses concurrent sharing for textures. The refusal has
/// to reach the caller.
#[test]
fn vulkan_a_refused_texture_is_an_error_and_not_a_cpu_stand_in() {
    let device = open_vulkan_device(Features::default());
    let desc = TextureDesc {
        width: 8,
        height: 8,
        depth: 1,
        mip_levels: 1,
        array_layers: 1,
        format: Format::RGBA8_UNORM,
        usage: TextureUsage::SAMPLED,
        sample_count: SampleCount::X1,
        dimensions: TextureDimensions::D2,
        sharing_mode: SharingMode::Concurrent,
        queue_family_indices: Vec::new(),
    };
    let result = device.create_texture_from_desc(&desc);
    assert!(
        result.is_err(),
        "the backend refuses concurrent sharing, so this has to be an error; a CPU \
         stand-in would be a texture with no native handle, reported as a success"
    );
}

/// A view of a texture that has no native image cannot be created, and the
/// caller has to be told.
#[test]
fn vulkan_a_view_of_a_texture_without_an_image_is_an_error() {
    let device = open_vulkan_device(Features::default());
    let cpu_only = Texture::new(TextureDesc {
        width: 8,
        height: 8,
        depth: 1,
        mip_levels: 1,
        array_layers: 1,
        format: Format::RGBA8_UNORM,
        usage: TextureUsage::SAMPLED,
        sample_count: SampleCount::X1,
        dimensions: TextureDimensions::D2,
        sharing_mode: SharingMode::Exclusive,
        queue_family_indices: Vec::new(),
    });
    let view_desc = TextureViewDesc {
        texture: cpu_only.clone(),
        format: None,
        view_type: TextureViewType::D2,
        aspects: TextureAspectFlags::COLOR,
        base_mip_level: 0,
        mip_level_count: 1,
        base_array_layer: 0,
        array_layer_count: 1,
    };
    let result = device.create_texture_view(&cpu_only, &view_desc);
    assert!(
        result.is_err(),
        "there is no image to make a view of, so this has to be an error; a CPU \
         stand-in view cannot be bound to anything"
    );
}

/// Bug #644: the RHI has to know where the upload left the image.
///
/// The layout tracker did not learn about the upload. The upload's barriers are
/// written straight into the command buffer rather than through a
/// `PipelineBarrier` command, so the tracker still said the image was in
/// `UNDEFINED` — and `download_texture` takes the old layout from the tracker,
/// where a barrier with `oldLayout = UNDEFINED` declares the contents
/// discarded.
///
/// The bytes alone do not catch it. "Discarded" is a contract-level statement:
/// a driver is free to leave the bits alone, so the readback came back with
/// the right bytes *and* the barrier said the wrong thing. On this machine the
/// readback is right for the wrong reason, which is worse than a wrong answer
/// because it looks like proof.
///
/// So the test asks the tracker directly, through the accessor that exists so
/// this state can be seen at all. `Undefined` after an upload means the
/// tracker never heard about it; `ShaderReadOnlyOptimal` means it did, and
/// that is the layout the upload left a sampled image in.
#[test]
fn vulkan_the_tracker_learns_where_an_upload_left_the_image() {
    let device = open_vulkan_device_with_validation(Features::default(), true);
    assert!(
        rhi::validation_layer_active(),
        "the validation layer has to be up for this test to mean anything: a clean \
         readback with no layer is a readback nobody checked"
    );
    let texture = device
        .create_texture(
            64,
            64,
            1,
            Format::RGBA8_UNORM,
            TextureUsage::SAMPLED | TextureUsage::TRANSFER_DST | TextureUsage::TRANSFER_SRC,
            1,
        )
        .expect("the upload-tracker texture must be created");
    assert!(texture.has_gpu_backing());

    // A freshly created image has no contents yet, and the tracker says so.
    assert_eq!(
        device.texture_layout(&texture),
        Some(TextureLayout::Undefined),
        "a freshly created image has not been written to"
    );

    let data: Vec<u8> = (0..64 * 64 * 4).map(|i| (i & 0xff) as u8).collect();
    device
        .upload_texture(&texture, &data)
        .expect("the upload must succeed; a discarded error here would leave the \
                 readback below asserting against memory nobody wrote");

    assert_eq!(
        device.texture_layout(&texture),
        Some(TextureLayout::ShaderReadOnlyOptimal),
        "the upload ended with the image in SHADER_READ_ONLY_OPTIMAL, and the \
         tracker has to know that: a barrier built from a stale oldLayout \
         declares the contents discarded"
    );

    let errors_before_readback = rhi::validation_error_count();
    let read_back = device
        .download_texture(&texture, 0, 0)
        .expect("the readback must succeed");
    assert_eq!(read_back.len(), data.len(), "the readback is a different size");
    assert_eq!(
        read_back, data,
        "the bytes that were uploaded must come back; a readback of zeros is a \
         successful-looking wrong answer, which is what a stale oldLayout produces"
    );
    assert_eq!(
        rhi::validation_error_count(),
        errors_before_readback,
        "the readback named the wrong old layout for the image"
    );

    // The readback moved the image into TRANSFER_SRC_OPTIMAL and did not put
    // it back, so the tracker has to know that too — otherwise the next
    // readback names a layout the image is no longer in.
    assert_eq!(
        device.texture_layout(&texture),
        Some(TextureLayout::TransferSrcOptimal),
        "the readback left the image in TRANSFER_SRC_OPTIMAL, and the tracker has \
         to know that"
    );
}

/// Bug #646: a framebuffer has to be a real one.
///
/// `Device::create_framebuffer` used to build a CPU-side description and never
/// call the backend, so the framebuffer it returned had no native handle. The
/// two passes that used it — `transparent` and `water` — bound it and then
/// recorded a `BeginRenderPass` against it, which the translator refuses with
/// "framebuffer has no GPU backing". So those two passes could not record a
/// frame at all, and the error pointed at the pass rather than at the method
/// that produced an object with nothing in it.
#[test]
fn vulkan_a_created_framebuffer_has_a_native_handle() {
    let device = open_vulkan_device(Features::default());
    let color = device
        .create_texture(
            64,
            64,
            1,
            Format::RGBA8_UNORM,
            TextureUsage::COLOR_ATTACHMENT,
            1,
        )
        .expect("the framebuffer test's colour target must be created");
    let view = device
        .create_texture_view(&color, color.create_default_view().desc())
        .expect("colour attachment view");
    let render_pass = device
        .create_gpu_render_pass(&RenderPassDesc {
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
            subpasses: vec![SubpassDescription {
                pipeline_bind_point: PipelineBindPoint::Graphics,
                color_attachments: vec![AttachmentReference {
                    attachment: 0,
                    layout: TextureLayout::ColorAttachmentOptimal,
                }],
                ..Default::default()
            }],
            dependencies: vec![],
        })
        .expect("render pass");

    let framebuffer = device
        .create_framebuffer(&render_pass, vec![view], 64, 64)
        .expect("the framebuffer must reach the backend: a description with no \
                 native handle cannot be bound to, and the pass fails at the \
                 render pass rather than here");
    assert!(
        framebuffer.has_gpu_backing(),
        "a framebuffer with no native handle cannot be bound to anything, and the \
         error that says so arrives at the render pass, far from the cause"
    );
    assert_eq!(framebuffer.desc().width, 64);
    assert_eq!(framebuffer.desc().height, 64);
    assert_eq!(framebuffer.desc().attachments.len(), 1);
}

/// Bug #649: a frame that was not submitted has no semaphore to wait on.
///
/// The present asked for the render-finished semaphore of the last submit
/// *ever*, which is the wrong answer whenever the current frame submitted
/// nothing — the graph recorded no commands, or the submit failed. The present
/// then waited on a semaphore the previous present had already consumed, or on
/// one nothing had ever signalled, and the chain stalled.
#[test]
fn vulkan_a_frame_that_was_not_submitted_has_no_semaphore_to_wait_on() {
    let device = open_vulkan_device(Features::default());
    let backend = device
        .backend_resource()
        .expect("a Vulkan device has a backend resource");

    // Nothing has been submitted yet, so there is nothing to wait on.
    assert_eq!(
        backend.render_finished_semaphore(),
        rhi::ash::vk::Semaphore::null(),
        "no frame has been submitted, so the present must not wait on a \
         render-finished semaphore"
    );

    // The renderer arms a frame before executing the graph. If the graph then
    // records nothing, the arm is still standing and nothing was submitted —
    // and the present must not be handed the previous frame's semaphore, which
    // the previous present already consumed.
    backend.arm_presentation_signal();
    assert_eq!(
        backend.render_finished_semaphore(),
        rhi::ash::vk::Semaphore::null(),
        "an armed frame that was not submitted must not hand the present a \
         semaphore to wait on: it belongs to a frame that was already presented"
    );
}

/// A sampler that asks for anisotropy on a device without the feature has to
/// be legal.
///
/// Anisotropy means more than one sample, so the predicate is `> 1.0` — it was
/// `> 0.0`, which meant a sampler asking for one sample still had anisotropy
/// switched on, and `max_anisotropy = 0.0` with anisotropy on is not a legal
/// combination at all. On top of that, `samplerAnisotropy` is off unless the
/// caller asked for it, and a sampler with anisotropy on a device without the
/// feature is `VUID-VkSamplerCreateInfo-anisotropyEnable-01070`.
///
/// So the request is clamped to what the device can do: the sampler is legal,
/// it just samples once. Enabling the feature is the caller's decision;
/// producing an invalid sampler is nobody's.
#[test]
fn vulkan_a_sampler_asking_for_anisotropy_is_legal_without_the_feature() {
    let device = open_vulkan_device_with_validation(Features::default(), true);
    assert!(
        !Features::default().sampler_anisotropy,
        "this test is about the device *without* the feature; if the default \
         changed, it is testing nothing"
    );
    let before = rhi::validation_error_count();
    let sampler = device
        .create_sampler(AddressMode::ClampToEdge, FilterMode::Linear, 8.0)
        .expect("the sampler must still be created; the request is clamped, not refused");
    assert!(
        sampler.has_gpu_backing(),
        "the sampler must still be created; the request is clamped, not refused"
    );
    assert_eq!(
        rhi::validation_error_count(),
        before,
        "a sampler with anisotropy on a device without the feature is \
         VUID-VkSamplerCreateInfo-anisotropyEnable-01070"
    );
}

/// The same, with the feature on: the sampler must be legal there too, and the
/// anisotropy request must not be silently dropped.
#[test]
fn vulkan_a_sampler_asking_for_anisotropy_is_legal_with_the_feature() {
    let features = Features {
        sampler_anisotropy: true,
        ..Default::default()
    };
    let device = open_vulkan_device_with_validation(features, true);
    let before = rhi::validation_error_count();
    let sampler = device
        .create_sampler(AddressMode::ClampToEdge, FilterMode::Linear, 8.0)
        .expect("the anisotropic sampler must be created");
    assert!(sampler.has_gpu_backing());
    assert_eq!(
        rhi::validation_error_count(),
        before,
        "a sampler with anisotropy on a device that has the feature must be \
         accepted"
    );
}

/// Bytes that are not SPIR-V must be refused at the door.
///
/// The check that existed was "the length is a multiple of four", which is not
/// evidence of anything: a GLSL source file whose length happens to be a
/// multiple of four passed it and was handed to the driver, which rejected it
/// much later and with a message about the shader rather than about the bytes.
/// The repository even has a compiler that returns GLSL text as "SPIR-V"
/// (`StubShaderCompiler::compile_to_spirv`), so the input is not hypothetical.
#[test]
fn vulkan_bytes_that_are_not_spirv_are_refused() {
    let device = open_vulkan_device(Features::default());
    // Sixteen bytes: a multiple of four, and not SPIR-V.
    let desc = ShaderModuleDesc {
        code: b"not spirv at all".to_vec(),
        format: ShaderFormat::SpirV,
        entry_point: None,
        name: Some("not-spirv".into()),
    };
    // The check is asked of the backend directly, because
    // `Device::create_shader_module` is one of the methods that still turns a
    // refusal into a CPU stand-in and reports it as a success (Bug #642, second
    // half — it needs SBF2's files to change and is still open). Going through
    // the backend is what the other half of that entry will do.
    let backend = device
        .backend_resource()
        .expect("a Vulkan device has a backend resource");
    let result = backend.create_shader_module(&desc);
    assert!(
        result.is_err(),
        "bytes that are not SPIR-V must be refused here, not by the driver much \
         later and with a message about the shader"
    );
    let text = format!("{}", result.err().expect("the refusal"));
    assert!(
        text.contains("SPIR-V"),
        "the refusal should say what is wrong with the bytes, got: {text}"
    );
}