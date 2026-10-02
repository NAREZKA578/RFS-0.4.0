// Integration tests for the rhi::descriptor module.
//
// Covers: DescriptorSetLayout, DescriptorSet, DescriptorPool,
// DescriptorAllocator, Bindless descriptor sets.

use rhi::descriptor::allocator::{
    DescriptorAllocator, DescriptorPool, DescriptorPoolDesc, DescriptorPoolSize,
};
use rhi::descriptor::binding::{BindlessDescriptorSet, BindlessDescriptorSetDesc};
use rhi::descriptor::layout::{
    DescriptorBinding, DescriptorSetLayout, DescriptorSetLayoutDesc, DescriptorSetLayoutFlags,
    DescriptorType,
};
use rhi::descriptor::set::DescriptorSet;
use rhi::ShaderStage;

#[test]
fn descriptor_type_variants() {
    let _ = DescriptorType::UniformBuffer;
    let _ = DescriptorType::DynamicUniformBuffer;
    let _ = DescriptorType::StorageBuffer;
    let _ = DescriptorType::DynamicStorageBuffer;
    let _ = DescriptorType::SampledTexture;
    let _ = DescriptorType::StorageTexture;
    let _ = DescriptorType::UniformTexelBuffer;
    let _ = DescriptorType::StorageTexelBuffer;
    let _ = DescriptorType::CombinedTextureSampler;
    let _ = DescriptorType::InputAttachment;
    let _ = DescriptorType::InlineUniformBlock;
    let _ = DescriptorType::AccelerationStructure;
}

#[test]
fn descriptor_set_layout_desc_creation() {
    let desc = DescriptorSetLayoutDesc {
        bindings: vec![DescriptorBinding {
            binding: 0,
            ty: DescriptorType::UniformBuffer,
            count: 1,
            stages: ShaderStage::VERTEX | ShaderStage::FRAGMENT,
            immutable_samplers: None,
        }],
    };
    assert_eq!(desc.bindings.len(), 1);
    assert!(desc.bindings[0].stages.contains(ShaderStage::VERTEX));
}

#[test]
fn descriptor_set_layout_new_and_desc() {
    let desc = DescriptorSetLayoutDesc::default();
    let layout = DescriptorSetLayout::new(desc);
    assert!(layout.desc().bindings.is_empty());
}

#[test]
fn descriptor_set_layout_flags() {
    let f = DescriptorSetLayoutFlags::PUSH_DESCRIPTOR | DescriptorSetLayoutFlags::UPDATE_AFTER_BIND;
    assert!(f.contains(DescriptorSetLayoutFlags::PUSH_DESCRIPTOR));
    assert!(f.contains(DescriptorSetLayoutFlags::UPDATE_AFTER_BIND));
    let combined = f | DescriptorSetLayoutFlags::DESCRIPTOR_BUFFER;
    assert!(combined.contains(DescriptorSetLayoutFlags::DESCRIPTOR_BUFFER));
}

#[test]
fn descriptor_set_new_and_layout() {
    let layout = DescriptorSetLayout::new(DescriptorSetLayoutDesc::default());
    let set = DescriptorSet::new(layout.clone());
    let _ = set.layout();
    let _ = layout;
}

#[test]
fn descriptor_pool_new() {
    let pool = DescriptorPool::new(DescriptorPoolDesc {
        max_sets: 128,
        pool_sizes: vec![DescriptorPoolSize {
            ty: DescriptorType::UniformBuffer,
            count: 512,
        }],
    });
    let _ = pool;
}

#[test]
fn descriptor_allocator_new() {
    let alloc = DescriptorAllocator::new();
    let _ = alloc;
    let _ = DescriptorPoolSize {
        ty: DescriptorType::SampledTexture,
        count: 64,
    };
}

#[test]
fn bindless_descriptor_set_desc_default() {
    let desc = BindlessDescriptorSetDesc {
        max_uniform_buffers: 1024,
        max_storage_buffers: 1024,
        max_sampled_textures: 4096,
        max_storage_textures: 1024,
        max_samplers: 1024,
        max_acceleration_structures: 128,
    };
    let _ = BindlessDescriptorSet::new(desc);
}

#[test]
fn bindless_descriptor_set_desc_default_impl() {
    let desc = BindlessDescriptorSetDesc::default();
    assert_eq!(desc.max_samplers, 0);
}

fn descriptor_layout(binding: u32, count: u32) -> DescriptorSetLayout {
    DescriptorSetLayout::new(DescriptorSetLayoutDesc {
        bindings: vec![DescriptorBinding {
            binding,
            ty: DescriptorType::UniformBuffer,
            count,
            stages: ShaderStage::VERTEX,
            immutable_samplers: None,
        }],
    })
}

#[test]
fn descriptor_set_layout_helpers() {
    let layout = descriptor_layout(0, 1);
    assert_eq!(layout.binding_count(), 1);
    assert_eq!(layout.total_descriptors(), 1);
    assert!(layout.binding(0).is_some());
    let b = layout.binding(0).unwrap();
    assert_eq!(b.ty, DescriptorType::UniformBuffer);
    assert_eq!(layout.bindings().len(), 1);
    assert!(layout.binding(3).is_none());
}

#[test]
fn descriptor_set_layout_total_descriptors_multi() {
    let layout = DescriptorSetLayout::new(DescriptorSetLayoutDesc {
        bindings: vec![
            DescriptorBinding {
                binding: 0,
                ty: DescriptorType::UniformBuffer,
                count: 2,
                stages: ShaderStage::VERTEX,
                immutable_samplers: None,
            },
            DescriptorBinding {
                binding: 1,
                ty: DescriptorType::SampledTexture,
                count: 4,
                stages: ShaderStage::FRAGMENT,
                immutable_samplers: None,
            },
        ],
    });
    assert_eq!(layout.total_descriptors(), 6);
    assert_eq!(layout.binding_count(), 2);
}

#[test]
fn descriptor_pool_allocates_until_exhausted() {
    let layout = descriptor_layout(0, 2);
    let mut pool = DescriptorPool::new(DescriptorPoolDesc {
        max_sets: 2,
        pool_sizes: vec![DescriptorPoolSize {
            ty: DescriptorType::UniformBuffer,
            count: 8,
        }],
    });
    assert!(pool.can_allocate(2));
    assert!(pool.allocate(&layout).is_some());
    assert!(pool.allocate(&layout).is_some());
    assert!(pool.allocate(&layout).is_none());
    assert_eq!(pool.allocated_sets(), 2);

    pool.reset();
    assert_eq!(pool.allocated_sets(), 0);
    assert!(pool.allocate(&layout).is_some());
}

#[test]
fn descriptor_pool_respects_descriptor_budget() {
    let layout = descriptor_layout(0, 9);
    let mut pool = DescriptorPool::new(DescriptorPoolDesc {
        max_sets: 16,
        pool_sizes: vec![DescriptorPoolSize {
            ty: DescriptorType::UniformBuffer,
            count: 8,
        }],
    });
    assert!(!pool.can_allocate(9));
    assert!(pool.allocate(&layout).is_none());
}

#[test]
fn descriptor_allocator_auto_creates_pools() {
    let layout = descriptor_layout(0, 1);
    let mut alloc = DescriptorAllocator::new();
    assert_eq!(alloc.pool_count(), 0);

    let set = alloc.allocate(&layout);
    assert_eq!(set.layout().binding_count(), 1);
    assert_eq!(alloc.pool_count(), 1);

    for _ in 0..200 {
        alloc.allocate(&layout);
    }
    assert!(alloc.pool_count() > 1);
}

#[test]
fn descriptor_allocator_reset_all() {
    let layout = descriptor_layout(0, 1);
    let mut alloc = DescriptorAllocator::new();
    for _ in 0..10 {
        alloc.allocate(&layout);
    }
    alloc.reset_all();
    // After reset, allocations can continue from existing pools.
    let set = alloc.allocate(&layout);
    assert_eq!(set.layout().binding_count(), 1);
}