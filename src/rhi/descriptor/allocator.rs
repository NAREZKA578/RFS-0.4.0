//! Descriptor Allocator
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use super::layout::{DescriptorSetLayout, DescriptorType};
use super::set::DescriptorSet;

/// Descriptor pool description
#[derive(Debug, Clone, Default)]
pub struct DescriptorPoolDesc {
    pub max_sets: u32,
    pub pool_sizes: Vec<DescriptorPoolSize>,
}

impl DescriptorPoolDesc {
    /// Returns the total number of descriptors across all pool sizes.
    pub fn total_descriptors(&self) -> u32 {
        self.pool_sizes.iter().map(|p| p.count).sum()
    }
}

/// Descriptor pool size
#[derive(Debug, Clone)]
pub struct DescriptorPoolSize {
    pub ty: DescriptorType,
    pub count: u32,
}

/// Descriptor pool
pub struct DescriptorPool {
    desc: DescriptorPoolDesc,
    allocated_sets: u32,
    allocated_descriptors: u32,
}

impl DescriptorPool {
    pub fn new(desc: DescriptorPoolDesc) -> Self {
        Self {
            desc,
            allocated_sets: 0,
            allocated_descriptors: 0,
        }
    }

    pub fn desc(&self) -> &DescriptorPoolDesc {
        &self.desc
    }

    /// Returns the number of sets allocated from this pool so far.
    pub fn allocated_sets(&self) -> u32 {
        self.allocated_sets
    }

    /// Returns `true` if the pool has room for the given amount of descriptors.
    pub fn can_allocate(&self, descriptors: u32) -> bool {
        self.allocated_sets < self.desc.max_sets
            && self.allocated_descriptors + descriptors <= self.desc.total_descriptors()
    }

    /// Allocates a descriptor set for the given layout, or `None` when the
    /// pool is exhausted.
    pub fn allocate(&mut self, layout: &DescriptorSetLayout) -> Option<DescriptorSet> {
        let needed = layout.total_descriptors();
        if !self.can_allocate(needed) {
            return None;
        }
        self.allocated_sets += 1;
        self.allocated_descriptors += needed;
        Some(DescriptorSet::new(layout.clone()))
    }

    /// Releases all allocations back to the pool.
    pub fn reset(&mut self) {
        self.allocated_sets = 0;
        self.allocated_descriptors = 0;
    }
}

/// Descriptor allocator
pub struct DescriptorAllocator {
    pools: Vec<DescriptorPool>,
}

impl DescriptorAllocator {
    pub fn new() -> Self {
        Self { pools: Vec::new() }
    }

    /// Returns the number of managed pools.
    pub fn pool_count(&self) -> usize {
        self.pools.len()
    }

    /// Adds a pool to the allocator.
    pub fn add_pool(&mut self, pool: DescriptorPool) {
        self.pools.push(pool);
    }

    /// Allocates a descriptor set, creating a default pool when no existing
    /// pool has room.
    pub fn allocate(&mut self, layout: &DescriptorSetLayout) -> DescriptorSet {
        for pool in &mut self.pools {
            if let Some(set) = pool.allocate(layout) {
                return set;
            }
        }

        let mut pool = DescriptorPool::new(DescriptorPoolDesc {
            max_sets: 64,
            pool_sizes: vec![DescriptorPoolSize {
                ty: DescriptorType::UniformBuffer,
                count: 1024,
            }],
        });
        let set = pool.allocate(layout).unwrap_or_else(|| DescriptorSet::new(layout.clone()));
        self.pools.push(pool);
        set
    }

    /// Resets every managed pool.
    pub fn reset_all(&mut self) {
        for pool in &mut self.pools {
            pool.reset();
        }
    }
}

impl Default for DescriptorAllocator {
    fn default() -> Self {
        Self::new()
    }
}