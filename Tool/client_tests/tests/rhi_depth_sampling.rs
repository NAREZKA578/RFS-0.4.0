// Can the RHI bind a depth texture?
//
// The shadow map is written as a depth attachment and read by the lighting
// shader, and until now the RHI had no way to express that: a depth view was
// described to Vulkan as an ordinary sampled image, in the layout every other
// sampled image uses, and a filtering sampler over it is
// `VUID-vkCmdDraw-None-08611`.
//
// The tests here are about the *descriptor* path, which is where the RHI made
// the wrong choice. The end-to-end proof - a shadow map actually sampled by a
// lighting shader - belongs to the shadow pass, and it is the shadow pass's
// test to own.

use rhi::backend::vulkan::VulkanBackend;
use rhi::config::RhiConfig;
use rhi::core::device::{Device, DeviceDesc};
use rhi::descriptor::layout::{DescriptorBinding, DescriptorSetLayoutDesc};
use rhi::resource::sampler::SamplerDesc;
use rhi::resource::texture::TextureViewDesc;
use rhi::resource::{Sampler, TextureViewType};
use rhi::types::{Features, Format, TextureAspectFlags, TextureUsage};
use rhi::{AddressMode, Backend, CompareOp, DescriptorInfo, DescriptorType, DescriptorWrite, FilterMode, ShaderStage};

fn open_vulkan_device() -> Option<Device> {
    let backend = VulkanBackend::new(&RhiConfig {
        validation: true,
        ..Default::default()
    })
    .ok()?;
    let devices = backend.enumerate_physical_devices().ok()?;
    backend
        .create_device(
            devices.first()?,
            &DeviceDesc {
                features: Features::default(),
                ..Default::default()
            },
        )
        .ok()
}

/// A depth image, as a shadow map is: a depth attachment that is also read.
fn shadow_map(device: &Device) -> (rhi::Texture, rhi::TextureView) {
    let texture = device
        .create_texture(
            256,
            256,
            1,
            Format::D32_SFLOAT,
            TextureUsage::DEPTH_STENCIL_ATTACHMENT | TextureUsage::SAMPLED,
            1,
        )
        .expect("the shadow map's depth texture must be created");
    assert!(
        texture.has_gpu_backing(),
        "the shadow map must be a real image: a stand-in has no depth to read"
    );
    let view = device
        .create_texture_view(
            &texture,
            &TextureViewDesc {
                texture: texture.clone(),
                format: None,
                view_type: TextureViewType::D2,
                aspects: TextureAspectFlags::DEPTH,
                base_mip_level: 0,
                mip_level_count: 1,
                base_array_layer: 0,
                array_layer_count: 1,
            },
        )
        .expect("a depth view");
    (texture, view)
}

fn descriptor_layout(device: &Device) -> rhi::DescriptorSetLayout {
    device
        .create_descriptor_set_layout(&DescriptorSetLayoutDesc {
            bindings: vec![DescriptorBinding {
                binding: 0,
                ty: DescriptorType::SampledTexture,
                count: 1,
                stages: ShaderStage::FRAGMENT,
                immutable_samplers: None,
            }],
        })
        .expect("the shadow map's descriptor layout must be created")
}

/// A depth view must be bindable: the shadow map is the reason this exists.
#[test]
fn a_depth_view_can_be_bound_as_a_sampled_texture() {
    // Bug №187: graceful skip instead of a panic — a machine without a Vulkan
    // loader/device falls out of the test as a skip, not as a red unit.
    let Some(device) = open_vulkan_device() else {
        eprintln!("skipped: no Vulkan backend/device on this machine");
        return;
    };
    let (_texture, view) = shadow_map(&device);
    let layout = descriptor_layout(&device);
    let set = device
        .create_descriptor_set(&layout)
        .expect("a descriptor set for the shadow map");

    // No sampler: the shader reads the depth with `textureLoad`, so no
    // filtering is involved and the depth-sampler rule does not apply.
    device
        .write_descriptors(
            &set,
            &[DescriptorWrite {
                dst_set: set.clone(),
                dst_binding: 0,
                dst_array_element: 0,
                descriptors: vec![DescriptorInfo::Texture(view, None)],
            }],
        )
        .expect("a depth texture must be bindable");

    // The write is only a statement about the descriptor; the layout it
    // claims has to be the one the image is in. A depth image that is read by
    // a shader lives in a read-only depth layout, and describing it as an
    // ordinary sampled image is what the RHI used to do.
    assert_eq!(
        rhi::validation_error_count(),
        0,
        "the validator reported an error while binding a depth texture: the \
         descriptor's layout is wrong for a depth image"
    );
}

/// A filtering sampler over a depth image is illegal, and the RHI has to say
/// so itself rather than leave it to the validator.
#[test]
fn a_filtering_sampler_over_a_depth_texture_is_refused() {
    // Bug №187: graceful skip instead of a panic — a machine without a Vulkan
    // loader/device falls out of the test as a skip, not as a red unit.
    let Some(device) = open_vulkan_device() else {
        eprintln!("skipped: no Vulkan backend/device on this machine");
        return;
    };
    let (_texture, view) = shadow_map(&device);
    let layout = descriptor_layout(&device);
    let set = device
        .create_descriptor_set(&layout)
        .expect("a descriptor set for the shadow map");
    let sampler = Sampler::from_desc(SamplerDesc {
        mag_filter: FilterMode::Linear,
        min_filter: FilterMode::Linear,
        address_mode_u: AddressMode::ClampToEdge,
        address_mode_v: AddressMode::ClampToEdge,
        ..Default::default()
    });

    let result = device.write_descriptors(
        &set,
        &[DescriptorWrite {
            dst_set: set.clone(),
            dst_binding: 0,
            dst_array_element: 0,
            descriptors: vec![DescriptorInfo::Texture(view, Some(sampler))],
        }],
    );

    assert!(
        result.is_err(),
        "a depth image may only be filtered by a comparison sampler \
         (VUID-vkCmdDraw-None-08611); the RHI must refuse it, not hand it to \
         the driver"
    );
    let message = result.err().expect("an error").to_string();
    assert!(
        message.contains("comparison sampler"),
        "the refusal must name the reason, got: {message}"
    );
}

/// The legal way to filter a depth image: a comparison sampler.
#[test]
fn a_comparison_sampler_over_a_depth_texture_is_accepted() {
    // Bug №187: graceful skip instead of a panic — a machine without a Vulkan
    // loader/device falls out of the test as a skip, not as a red unit.
    let Some(device) = open_vulkan_device() else {
        eprintln!("skipped: no Vulkan backend/device on this machine");
        return;
    };
    let (_texture, view) = shadow_map(&device);
    let layout = descriptor_layout(&device);
    let set = device
        .create_descriptor_set(&layout)
        .expect("a descriptor set for the shadow map");
    let sampler = device
        .create_sampler_from_desc(&SamplerDesc {
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            address_mode_u: AddressMode::ClampToEdge,
            address_mode_v: AddressMode::ClampToEdge,
            compare_enable: true,
            compare_op: CompareOp::LessOrEqual,
            ..Default::default()
        })
        .expect("a comparison sampler");

    device
        .write_descriptors(
            &set,
            &[DescriptorWrite {
                dst_set: set.clone(),
                dst_binding: 0,
                dst_array_element: 0,
            descriptors: vec![DescriptorInfo::Texture(view, Some(sampler.clone()))],
            }],
        )
        .expect("a comparison sampler over a depth texture is legal");

    assert_eq!(
        rhi::validation_error_count(),
        0,
        "the validator reported an error while binding a depth texture with a \
         comparison sampler"
    );
}
