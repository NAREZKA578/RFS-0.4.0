// Integration tests for the Vulkan compute pipeline.
//
// These tests compile a WGSL shader to SPIR-V with naga, build a real Vulkan
// compute pipeline, dispatch it and read the result back from the GPU, which
// exercises shader modules, descriptor sets, pipelines, queues, command
// buffers, staging transfers and synchronization in one go.

use rhi::backend::vulkan::VulkanBackend;
use rhi::config::RhiConfig;
use rhi::core::device::{Device, DeviceDesc};
use rhi::descriptor::{
    DescriptorBinding, DescriptorInfo, DescriptorSetLayoutDesc, DescriptorType, DescriptorWrite,
};
use rhi::pipeline::{ComputePipelineDesc, PipelineShaderStage};
use rhi::shader::module::{ShaderFormat, ShaderModuleDesc};
use rhi::types::{Features, ShaderStage};
use rhi::{Backend, BufferUsage};

const SHADER: &str = r#"
@group(0) @binding(0) var<storage, read> src: array<u32>;
@group(0) @binding(1) var<storage, read_write> dst: array<u32>;

@compute @workgroup_size(8)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i < arrayLength(&src)) {
        dst[i] = src[i] * 2u + 1u;
    }
}
"#;

fn open_vulkan_device(features: Features) -> Device {
    let backend = VulkanBackend::new(&RhiConfig::default()).unwrap();
    let devices = backend.enumerate_physical_devices().unwrap();
    let desc = DeviceDesc {
        features,
        ..Default::default()
    };
    backend.create_device(&devices[0], &desc).unwrap()
}

fn wgsl_to_spirv(source: &str) -> Vec<u8> {
    let module = naga::front::wgsl::parse_str(source).expect("WGSL parse");
    let mut validator = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    );
    let info = validator.validate(&module).expect("WGSL validation");
    let options = naga::back::spv::Options::default();
    let words = naga::back::spv::write_vec(&module, &info, &options, None).expect("SPIR-V emit");
    let mut bytes = Vec::with_capacity(words.len() * 4);
    for word in words {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    bytes
}

#[test]
fn vulkan_compute_shader_and_descriptor_layout() {
    let device = open_vulkan_device(Features::default());

    let module = device.create_shader_module(&ShaderModuleDesc {
        code: wgsl_to_spirv(SHADER),
        format: ShaderFormat::SpirV,
        entry_point: Some("main".into()),
        name: Some("compute test".into()),
    });
    assert!(module.has_gpu_backing());
    assert!(module.has_code());

    let layout = device.create_descriptor_set_layout(&DescriptorSetLayoutDesc {
        bindings: vec![
            DescriptorBinding {
                binding: 0,
                ty: DescriptorType::StorageBuffer,
                count: 1,
                stages: ShaderStage::COMPUTE,
                immutable_samplers: None,
            },
            DescriptorBinding {
                binding: 1,
                ty: DescriptorType::StorageBuffer,
                count: 1,
                stages: ShaderStage::COMPUTE,
                immutable_samplers: None,
            },
        ],
    });
    assert!(layout.has_gpu_backing());
    assert_eq!(layout.binding_count(), 2);
    assert_eq!(layout.total_descriptors(), 2);

    let set = device.create_descriptor_set(&layout).unwrap();
    assert!(set.has_gpu_backing());
}

#[test]
fn vulkan_compute_roundtrip() {
    let device = open_vulkan_device(Features::default());
    let count = 8u32;
    let byte_size = (count * 4) as u64;
    let source: Vec<u32> = (1..=count).collect();
    let source_bytes: Vec<u8> = source.iter().flat_map(|v| v.to_le_bytes()).collect();

    let src = device.create_buffer(byte_size, BufferUsage::STORAGE | BufferUsage::TRANSFER_DST, false);
    let dst = device.create_buffer(byte_size, BufferUsage::STORAGE | BufferUsage::TRANSFER_SRC, false);
    device.upload_buffer(&src, &source_bytes);

    let layout = device.create_descriptor_set_layout(&DescriptorSetLayoutDesc {
        bindings: vec![
            DescriptorBinding {
                binding: 0,
                ty: DescriptorType::StorageBuffer,
                count: 1,
                stages: ShaderStage::COMPUTE,
                immutable_samplers: None,
            },
            DescriptorBinding {
                binding: 1,
                ty: DescriptorType::StorageBuffer,
                count: 1,
                stages: ShaderStage::COMPUTE,
                immutable_samplers: None,
            },
        ],
    });

    let module = device.create_shader_module(&ShaderModuleDesc {
        code: wgsl_to_spirv(SHADER),
        format: ShaderFormat::SpirV,
        entry_point: Some("main".into()),
        name: Some("compute test".into()),
    });
    assert!(module.has_gpu_backing());

    let pipeline = device
        .create_compute_pipeline(
            &ComputePipelineDesc {
                shader: PipelineShaderStage {
                    stage: ShaderStage::COMPUTE,
                    module,
                    entry_point: "main".into(),
                },
            },
            &layout,
        )
        .expect("compute pipeline");
    assert!(pipeline.has_gpu_backing());

    let set = device.create_descriptor_set(&layout).unwrap();
    device
        .write_descriptors(
            &set,
            &[
                DescriptorWrite {
                    dst_set: set.clone(),
                    dst_binding: 0,
                    dst_array_element: 0,
                    descriptors: vec![DescriptorInfo::Buffer(src.clone(), 0, byte_size)],
                },
                DescriptorWrite {
                    dst_set: set.clone(),
                    dst_binding: 1,
                    dst_array_element: 0,
                    descriptors: vec![DescriptorInfo::Buffer(dst.clone(), 0, byte_size)],
                },
            ],
        )
        .unwrap();

    device.dispatch_compute(&pipeline, &[&set], 1, 1, 1);

    let output = device.download_buffer(&dst, 0, byte_size).unwrap();
    let output_values: Vec<u32> = output
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| u32::from_le_bytes(*c))
        .collect();
    let expected: Vec<u32> = source.iter().map(|v| v * 2 + 1).collect();
    assert_eq!(output_values, expected);
}
