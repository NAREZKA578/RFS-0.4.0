//! Memory Allocator Trait
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//! **TODO: Full implementation required**

use crate::error::*;
use crate::types::*;

/// Memory allocation description
#[derive(Debug, Clone, Default)]
pub struct AllocationDesc {
    pub size: u64,
    pub alignment: u64,
    pub memory_type: MemoryTypeFlags,
    pub preferred_flags: MemoryPropertyFlags,
    pub required_flags: MemoryPropertyFlags,
    pub name: Option<String>,
}

/// Memory allocation
pub struct Allocation {
    pub offset: u64,
    pub size: u64,
    pub memory_type_index: u32,
}

/// Memory statistics
#[derive(Debug, Clone, Default)]
pub struct MemoryStats {
    pub total_allocated: u64,
    pub total_freed: u64,
    pub current_usage: u64,
    pub peak_usage: u64,
    pub allocation_count: u64,
    pub free_block_count: u64,
}

/// Memory allocator trait
pub trait MemoryAllocator: Send + Sync {
    fn allocate(&mut self, desc: &AllocationDesc) -> RhiResult<Allocation>;
    fn free(&mut self, allocation: Allocation) -> RhiResult<()>;
    fn get_memory_stats(&self) -> MemoryStats;
    fn defragment(&mut self) -> RhiResult<()>;
}

/// Memory type flags (re-export from types)
pub type MemoryTypeFlags = MemoryPropertyFlags;
