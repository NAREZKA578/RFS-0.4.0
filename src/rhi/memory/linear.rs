//! Linear Memory Allocator
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use super::allocator::{Allocation, AllocationDesc, MemoryAllocator, MemoryStats};
use crate::error::{RhiError, RhiResult};

/// Linear allocator
pub struct LinearAllocator {
    memory_type_index: u32,
    heap_size: u64,
    current_offset: u64,
    stats: MemoryStats,
}

impl LinearAllocator {
    pub fn new(memory_type_index: u32, heap_size: u64) -> Self {
        Self {
            memory_type_index,
            heap_size,
            current_offset: 0,
            stats: MemoryStats::default(),
        }
    }

    pub fn reset(&mut self) {
        self.current_offset = 0;
        self.stats = MemoryStats::default();
    }

    /// Returns the current bump offset.
    pub fn offset(&self) -> u64 {
        self.current_offset
    }
}

fn align_up(offset: u64, alignment: u64) -> u64 {
    if alignment <= 1 {
        return offset;
    }
    let remainder = offset % alignment;
    if remainder == 0 {
        offset
    } else {
        offset + (alignment - remainder)
    }
}

impl MemoryAllocator for LinearAllocator {
    fn allocate(&mut self, desc: &AllocationDesc) -> RhiResult<Allocation> {
        let alignment = if desc.alignment == 0 { 1 } else { desc.alignment };
        let aligned = align_up(self.current_offset, alignment);
        let end = aligned.checked_add(desc.size).ok_or(RhiError::OutOfMemory)?;
        if end > self.heap_size {
            return Err(RhiError::OutOfMemory);
        }
        self.current_offset = end;

        self.stats.total_allocated += desc.size;
        self.stats.current_usage += desc.size;
        self.stats.allocation_count += 1;
        if self.stats.current_usage > self.stats.peak_usage {
            self.stats.peak_usage = self.stats.current_usage;
        }

        Ok(Allocation {
            offset: aligned,
            size: desc.size,
            memory_type_index: self.memory_type_index,
        })
    }

    fn free(&mut self, allocation: Allocation) -> RhiResult<()> {
        if allocation.size > self.stats.current_usage {
            return Err(RhiError::InternalError("invalid allocation".into()));
        }
        self.stats.current_usage -= allocation.size;
        self.stats.total_freed += allocation.size;
        self.stats.allocation_count -= 1;
        Ok(())
    }

    fn get_memory_stats(&self) -> MemoryStats {
        self.stats.clone()
    }

    fn defragment(&mut self) -> RhiResult<()> {
        self.reset();
        Ok(())
    }
}