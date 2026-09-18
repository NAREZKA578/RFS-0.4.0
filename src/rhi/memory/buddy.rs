//! Buddy Memory Allocator
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use super::allocator::{Allocation, AllocationDesc, MemoryAllocator, MemoryStats};
use crate::error::{RhiError, RhiResult};

/// Buddy block
#[derive(Debug, Clone)]
pub struct BuddyBlock {
    pub offset: u64,
    pub size: u64,
    pub is_free: bool,
}

/// Buddy allocator
pub struct BuddyAllocator {
    memory_type_index: u32,
    heap_size: u64,
    min_allocation_size: u64,
    blocks: Vec<BuddyBlock>,
    stats: MemoryStats,
}

impl BuddyAllocator {
    pub fn new(memory_type_index: u32, heap_size: u64, min_allocation_size: u64) -> Self {
        let mut blocks = Vec::new();
        if heap_size > 0 {
            blocks.push(BuddyBlock {
                offset: 0,
                size: heap_size,
                is_free: true,
            });
        }
        Self {
            memory_type_index,
            heap_size,
            min_allocation_size: min_allocation_size.max(1),
            blocks,
            stats: MemoryStats::default(),
        }
    }

    /// Returns the number of tracked blocks (free and allocated).
    pub fn block_count(&self) -> usize {
        self.blocks.len()
    }

    fn find_best_free(&self, size: u64) -> Option<usize> {
        let mut best: Option<usize> = None;
        for (i, block) in self.blocks.iter().enumerate() {
            if block.is_free && block.size >= size {
                match best {
                    None => best = Some(i),
                    Some(b) if block.size < self.blocks[b].size => best = Some(i),
                    _ => {}
                }
            }
        }
        best
    }

    fn split_block(&mut self, index: usize) {
        let block = self.blocks[index].clone();
        if !block.is_free || block.size < 2 * self.min_allocation_size {
            return;
        }
        let half = block.size / 2;
        if half < self.min_allocation_size {
            return;
        }
        self.blocks[index].size = half;
        self.blocks.insert(
            index + 1,
            BuddyBlock {
                offset: block.offset + half,
                size: half,
                is_free: true,
            },
        );
    }

    fn merge_buddies(&mut self) {
        loop {
            let mut merged = false;
            let len = self.blocks.len();
            let mut i = 0;
            while i < len {
                let a = self.blocks[i].clone();
                if a.is_free {
                    let buddy_offset = a.offset ^ a.size;
                    if let Some(pos) = self
                        .blocks
                        .iter()
                        .position(|b| b.offset == buddy_offset && b.size == a.size && b.is_free)
                    {
                        let (small, large) = if pos < i { (pos, i) } else { (i, pos) };
                        let merged_offset = self.blocks[small].offset.min(self.blocks[large].offset);
                        self.blocks[small].offset = merged_offset;
                        self.blocks[small].size *= 2;
                        self.blocks.remove(large);
                        merged = true;
                        break;
                    }
                }
                i += 1;
            }
            if !merged {
                break;
            }
        }
    }
}

impl MemoryAllocator for BuddyAllocator {
    fn allocate(&mut self, desc: &AllocationDesc) -> RhiResult<Allocation> {
        let size = desc.size.max(self.min_allocation_size);
        if size > self.heap_size {
            return Err(RhiError::OutOfMemory);
        }

        let mut index = match self.find_best_free(size) {
            Some(i) => i,
            None => return Err(RhiError::OutOfMemory),
        };

        while self.blocks[index].size / 2 >= size.max(self.min_allocation_size) {
            self.split_block(index);
            index = self.blocks
                .iter()
                .position(|b| b.offset == self.blocks[index].offset && b.is_free)
                .unwrap_or(index);
        }

        self.blocks[index].is_free = false;
        let block = self.blocks[index].clone();
        self.blocks[index].size = block.size;

        self.stats.total_allocated += block.size;
        self.stats.current_usage += block.size;
        self.stats.allocation_count += 1;
        if self.stats.current_usage > self.stats.peak_usage {
            self.stats.peak_usage = self.stats.current_usage;
        }

        Ok(Allocation {
            offset: block.offset,
            size: block.size,
            memory_type_index: self.memory_type_index,
        })
    }

    fn free(&mut self, allocation: Allocation) -> RhiResult<()> {
        let index = self
            .blocks
            .iter()
            .position(|b| b.offset == allocation.offset && !b.is_free)
            .ok_or_else(|| RhiError::InternalError("unknown allocation".into()))?;

        self.stats.current_usage -= self.blocks[index].size;
        self.stats.total_freed += allocation.size;
        self.stats.allocation_count -= 1;

        self.blocks[index].is_free = true;
        self.merge_buddies();
        Ok(())
    }

    fn get_memory_stats(&self) -> MemoryStats {
        self.stats.clone()
    }

    fn defragment(&mut self) -> RhiResult<()> {
        self.merge_buddies();
        Ok(())
    }
}