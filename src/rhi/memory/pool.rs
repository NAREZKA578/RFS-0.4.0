//! Pool Memory Allocator
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use super::allocator::{Allocation, AllocationDesc, MemoryAllocator, MemoryStats};
use crate::error::{RhiError, RhiResult};

/// Pool block
#[derive(Debug, Clone)]
pub struct PoolBlock {
    pub offset: u64,
    pub size: u64,
    pub free_list: Vec<u64>,
}

/// Pool allocator
pub struct PoolAllocator {
    memory_type_index: u32,
    block_size: u64,
    blocks: Vec<PoolBlock>,
    stats: MemoryStats,
}

impl PoolAllocator {
    pub fn new(memory_type_index: u32, block_size: u64, initial_blocks: u32) -> Self {
        let mut blocks = Vec::new();
        for i in 0..initial_blocks {
            let offset = (i as u64) * block_size;
            blocks.push(PoolBlock {
                offset,
                size: block_size,
                free_list: vec![offset],
            });
        }
        Self {
            memory_type_index,
            block_size,
            blocks,
            stats: MemoryStats::default(),
        }
    }

    /// Returns the number of managed blocks.
    pub fn block_count(&self) -> usize {
        self.blocks.len()
    }

    fn add_block(&mut self) {
        let next_offset = self.blocks.iter().map(|b| b.offset + b.size).max().unwrap_or(0);
        self.blocks.push(PoolBlock {
            offset: next_offset,
            size: self.block_size,
            free_list: vec![next_offset],
        });
    }
}

impl MemoryAllocator for PoolAllocator {
    fn allocate(&mut self, desc: &AllocationDesc) -> RhiResult<Allocation> {
        if desc.size > self.block_size {
            return Err(RhiError::ValidationError(format!(
                "allocation of {} bytes exceeds pool block size {}",
                desc.size, self.block_size
            )));
        }

        let mut target: Option<usize> = None;
        for (i, block) in self.blocks.iter().enumerate() {
            if !block.free_list.is_empty() {
                target = Some(i);
                break;
            }
        }

        if target.is_none() {
            self.add_block();
            target = Some(self.blocks.len() - 1);
        }

        let index = target.unwrap();
        let offset = self.blocks[index].free_list.pop().unwrap();

        self.stats.total_allocated += desc.size;
        self.stats.current_usage += desc.size;
        self.stats.allocation_count += 1;
        if self.stats.current_usage > self.stats.peak_usage {
            self.stats.peak_usage = self.stats.current_usage;
        }

        Ok(Allocation {
            offset,
            size: desc.size,
            memory_type_index: self.memory_type_index,
        })
    }

    fn free(&mut self, allocation: Allocation) -> RhiResult<()> {
        let index = self
            .blocks
            .iter()
            .position(|b| allocation.offset >= b.offset && allocation.offset < b.offset + b.size)
            .ok_or_else(|| RhiError::InternalError("unknown allocation".into()))?;

        if self.blocks[index].free_list.contains(&allocation.offset) {
            return Err(RhiError::InternalError("allocation already freed".into()));
        }

        self.stats.current_usage -= allocation.size;
        self.stats.total_freed += allocation.size;
        self.stats.allocation_count -= 1;

        self.blocks[index].free_list.push(allocation.offset);
        Ok(())
    }

    fn get_memory_stats(&self) -> MemoryStats {
        self.stats.clone()
    }

    fn defragment(&mut self) -> RhiResult<()> {
        self.stats = MemoryStats::default();
        Ok(())
    }
}