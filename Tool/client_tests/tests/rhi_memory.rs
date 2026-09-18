// Integration tests for the rhi::memory module.
//
// Covers: LinearAllocator, PoolAllocator, BuddyAllocator (construction and
// stateless operations only; allocate/free of pool/buddy are unimplemented).

use rhi::memory::allocator::{Allocation, AllocationDesc, MemoryAllocator, MemoryStats};
use rhi::memory::buddy::{BuddyAllocator, BuddyBlock};
use rhi::memory::budget::{MemoryBudget, MemoryBudgetManager};
use rhi::memory::linear::LinearAllocator;
use rhi::memory::pool::{PoolAllocator, PoolBlock};

#[test]
fn allocation_desc_default() {
    let desc = AllocationDesc::default();
    assert_eq!(desc.size, 0);
    assert_eq!(desc.alignment, 0);
    assert!(desc.name.is_none());
}

#[test]
fn allocation_desc_creation() {
    let desc = AllocationDesc {
        size: 4096,
        alignment: 256,
        memory_type: rhi::MemoryPropertyFlags::DEVICE_LOCAL,
        preferred_flags: rhi::MemoryPropertyFlags::DEVICE_LOCAL,
        required_flags: rhi::MemoryPropertyFlags::empty(),
        name: Some("ubo".to_string()),
    };
    assert_eq!(desc.size, 4096);
    assert_eq!(desc.name.as_deref(), Some("ubo"));
}

#[test]
fn allocation_struct_fields() {
    let alloc = Allocation {
        offset: 128,
        size: 512,
        memory_type_index: 1,
    };
    assert_eq!(alloc.offset, 128);
    assert_eq!(alloc.size, 512);
    assert_eq!(alloc.memory_type_index, 1);
}

#[test]
fn memory_stats_default() {
    let stats = MemoryStats::default();
    assert_eq!(stats.total_allocated, 0);
    assert_eq!(stats.current_usage, 0);
    assert_eq!(stats.allocation_count, 0);
}

#[test]
fn linear_allocator_new_and_reset() {
    let mut alloc = LinearAllocator::new(0, 1 << 20);
    alloc.reset();
    assert_eq!(alloc.get_memory_stats().current_usage, 0);
}

#[test]
fn linear_allocator_free_is_ok() {
    let mut alloc = LinearAllocator::new(0, 1 << 20);
    let result = alloc.free(Allocation {
        offset: 0,
        size: 64,
        memory_type_index: 0,
    });
    assert!(result.is_ok(), "linear free must be a no-op Ok");
}

#[test]
fn linear_allocator_defragment_resets() {
    let mut alloc = LinearAllocator::new(2, 8192);
    assert!(alloc.defragment().is_ok());
}

#[test]
fn pool_allocator_new_stats() {
    let alloc = PoolAllocator::new(0, 1 << 20, 4);
    assert_eq!(alloc.get_memory_stats().total_allocated, 0);
}

#[test]
fn pool_block_struct() {
    let block = PoolBlock {
        offset: 0,
        size: 1024,
        free_list: vec![0],
    };
    assert_eq!(block.size, 1024);
    assert_eq!(block.free_list.len(), 1);
}

#[test]
fn buddy_allocator_new_stats() {
    let alloc = BuddyAllocator::new(0, 1 << 20, 256);
    assert_eq!(alloc.get_memory_stats().current_usage, 0);
}

#[test]
fn buddy_block_struct() {
    let block = BuddyBlock {
        offset: 0,
        size: 1024,
        is_free: true,
    };
    assert!(block.is_free);
}

#[test]
fn memory_budget_default() {
    let budget = MemoryBudget::default();
    assert_eq!(budget.total_memory, 0);
    assert_eq!(budget.free_memory, 0);
}

#[test]
fn memory_budget_manager_new() {
    let manager = MemoryBudgetManager::new();
    assert_eq!(manager.budget().used_memory, 0);
}

#[test]
fn allocator_trait_is_object_safe() {
    // The trait must be usable as a &dyn MemoryAllocator.
    let mut linear = LinearAllocator::new(0, 1024);
    let alloc: &mut dyn MemoryAllocator = &mut linear;
    let stats = alloc.get_memory_stats();
    assert_eq!(stats.allocation_count, 0);
}

#[test]
fn linear_allocator_allocates_with_alignment() {
    let mut alloc = LinearAllocator::new(0, 1 << 20);
    let a = alloc
        .allocate(&AllocationDesc {
            size: 64,
            alignment: 256,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(a.offset % 256, 0);
    assert_eq!(a.memory_type_index, 0);

    let b = alloc
        .allocate(&AllocationDesc {
            size: 100,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(b.offset % 256, 0);
    assert!(b.offset > a.offset + a.size);

    let stats = alloc.get_memory_stats();
    assert_eq!(stats.allocation_count, 2);
    assert_eq!(stats.current_usage, 164);
    assert_eq!(stats.peak_usage, 164);
    assert!(stats.free_block_count == 0);
}

#[test]
fn linear_allocator_out_of_memory() {
    let mut alloc = LinearAllocator::new(0, 1024);
    let r = alloc.allocate(&AllocationDesc {
        size: 2048,
        ..Default::default()
    });
    assert!(matches!(r, Err(rhi::RhiError::OutOfMemory)));
}

#[test]
fn linear_allocator_free_asserts() {
    let mut alloc = LinearAllocator::new(1, 4096);
    let a = alloc
        .allocate(&AllocationDesc {
            size: 512,
            ..Default::default()
        })
        .unwrap();
    alloc.free(a).unwrap();
    let stats = alloc.get_memory_stats();
    assert_eq!(stats.current_usage, 0);
    assert_eq!(stats.total_freed, 512);
    assert_eq!(stats.allocation_count, 0);
}

#[test]
fn buddy_allocator_allocates_and_merges() {
    let mut alloc = BuddyAllocator::new(0, 1 << 20, 64);
    let a = alloc
        .allocate(&AllocationDesc {
            size: 128,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(a.size, 128);
    assert_eq!(a.offset, 0);

    let b = alloc
        .allocate(&AllocationDesc {
            size: 128,
            ..Default::default()
        })
        .unwrap();
    assert!(b.offset >= 128);

    let stats = alloc.get_memory_stats();
    assert_eq!(stats.allocation_count, 2);
    assert_eq!(stats.current_usage, 256);

    alloc.free(a).unwrap();
    alloc.free(b).unwrap();
    let stats = alloc.get_memory_stats();
    assert_eq!(stats.allocation_count, 0);
    assert_eq!(stats.current_usage, 0);
    // After merging, a single free root block should remain.
    assert_eq!(alloc.block_count(), 1);
}

#[test]
fn buddy_allocator_split_records_sizes() {
    let mut alloc = BuddyAllocator::new(0, 4096, 16);
    let mut blocks = Vec::new();
    for _ in 0..5 {
        blocks.push(
            alloc
                .allocate(&AllocationDesc {
                    size: 64,
                    ..Default::default()
                })
                .unwrap(),
        );
    }
    let mut offsets = blocks.iter().map(|b| b.offset).collect::<Vec<_>>();
    offsets.sort();
    assert_eq!(offsets.len(), 5);
    for w in offsets.windows(2) {
        assert!(w[1] > w[0]);
    }
}

#[test]
fn pool_allocator_roundtrip() {
    let mut alloc = PoolAllocator::new(2, 1024, 2);
    let a = alloc
        .allocate(&AllocationDesc {
            size: 256,
            ..Default::default()
        })
        .unwrap();
    let b = alloc
        .allocate(&AllocationDesc {
            size: 256,
            ..Default::default()
        })
        .unwrap();
    assert_ne!(a.offset, b.offset);
    assert_eq!(alloc.block_count(), 2);

    alloc.free(a).unwrap();
    alloc.free(b).unwrap();
    let stats = alloc.get_memory_stats();
    assert_eq!(stats.allocation_count, 0);
    assert_eq!(stats.current_usage, 0);
}

#[test]
fn pool_allocator_grows_when_exhausted() {
    let mut alloc = PoolAllocator::new(0, 512, 1);
    let _ = alloc
        .allocate(&AllocationDesc {
            size: 32,
            ..Default::default()
        })
        .unwrap();
    let _ = alloc
        .allocate(&AllocationDesc {
            size: 32,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(alloc.block_count(), 2);
}

#[test]
fn pool_allocator_rejects_oversized_allocation() {
    let mut alloc = PoolAllocator::new(0, 256, 1);
    let r = alloc.allocate(&AllocationDesc {
        size: 1024,
        ..Default::default()
    });
    assert!(r.is_err());
}