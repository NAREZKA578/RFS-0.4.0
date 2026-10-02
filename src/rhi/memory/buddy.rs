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
    /// Smallest power of two that is >= `value` (and >= 1).
    fn round_up_pow2(value: u64) -> u64 {
        if value <= 1 {
            return 1;
        }
        value.next_power_of_two()
    }

    /// Bug №224: the buddy is found with `offset ^ size`, which is only correct
    /// when `size` is a power of two *and* `offset` is aligned to it. The old
    /// constructor pushed a single block of the raw `heap_size`, so a heap of
    /// 1000 produced 1000/500/250/... — sizes that are not powers of two — and
    /// every buddy computation after that pointed at an unrelated block. The
    /// heap is now rounded up to a power of two, and the allocation is capped
    /// at that rounded size so no request can address the padding.
    pub fn new(memory_type_index: u32, heap_size: u64, min_allocation_size: u64) -> Self {
        let min_allocation_size = min_allocation_size.max(1);
        let usable = Self::round_up_pow2(heap_size);

        let mut blocks = Vec::new();
        if usable > 0 {
            blocks.push(BuddyBlock {
                offset: 0,
                size: usable,
                is_free: true,
            });
        }
        Self {
            memory_type_index,
            heap_size: usable,
            min_allocation_size: Self::round_up_pow2(min_allocation_size),
            blocks,
            stats: MemoryStats::default(),
        }
    }

    /// Returns the number of tracked blocks (free and allocated).
    pub fn block_count(&self) -> usize {
        self.blocks.len()
    }

    /// Number of free blocks — the figure `MemoryStats.free_block_count` was
    /// silently reporting as zero (bug №227).
    pub fn free_block_count(&self) -> u64 {
        self.blocks.iter().filter(|b| b.is_free).count() as u64
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

    fn split_block(&mut self, index: usize) -> bool {
        let block = self.blocks[index].clone();
        if !block.is_free || block.size < 2 * self.min_allocation_size {
            return false;
        }
        let half = block.size / 2;
        if half < self.min_allocation_size {
            return false;
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
        true
    }

    fn merge_buddies(&mut self) {
        loop {
            let mut merged = false;
            let len = self.blocks.len();
            let mut i = 0;
            while i < len {
                let a = self.blocks[i].clone();
                if a.is_free {
                    // Only meaningful for power-of-two sizes aligned to the
                    // size, which `new` now guarantees.
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
        // Honour the requested alignment: a descriptor bound to a sub-range of
        // this block needs its offset to satisfy `desc.alignment` (bug №224).
        let alignment = if desc.alignment <= 1 { 1 } else { desc.alignment };
        let size = desc
            .size
            .max(self.min_allocation_size)
            .checked_next_multiple_of(alignment)
            .ok_or(RhiError::OutOfMemory)?;
        if size > self.heap_size {
            return Err(RhiError::OutOfMemory);
        }

        let index = match self.find_best_free(size) {
            Some(i) => i,
            None => return Err(RhiError::OutOfMemory),
        };

        // Bug №224: the old loop re-found the block by offset after every
        // split. Because `split_block` leaves the *lower* half at `index` and
        // inserts the upper half after it, the search could land on the upper
        // half while the lower one stayed free and was later handed out again —
        // two live allocations overlapping. The split is reported now, so the
        // index stays correct by construction.
        while self.blocks[index].size / 2 >= size.max(self.min_allocation_size) {
            if !self.split_block(index) {
                break;
            }
        }

        let block = self.blocks[index].clone();
        self.blocks[index].is_free = false;

        self.stats.total_allocated += block.size;
        self.stats.current_usage += block.size;
        self.stats.allocation_count += 1;
        if self.stats.current_usage > self.stats.peak_usage {
            self.stats.peak_usage = self.stats.current_usage;
        }
        self.stats.free_block_count = self.free_block_count();

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

        // Bug №224: trust the block's own size, not the size the caller echoes
        // back. `allocation.size` is whatever the caller felt like passing, and
        // using it made the usage accounting drift (and could underflow).
        self.stats.current_usage = self
            .stats
            .current_usage
            .saturating_sub(self.blocks[index].size);
        self.stats.total_freed += self.blocks[index].size;
        self.stats.allocation_count = self.stats.allocation_count.saturating_sub(1);

        self.blocks[index].is_free = true;
        self.merge_buddies();
        self.stats.free_block_count = self.free_block_count();
        Ok(())
    }

    fn get_memory_stats(&self) -> MemoryStats {
        let mut stats = self.stats.clone();
        // Never report a stale figure if the struct was built before this
        // bookkeeping existed.
        stats.free_block_count = self.free_block_count();
        stats
    }

    fn defragment(&mut self) -> RhiResult<()> {
        self.merge_buddies();
        self.stats.free_block_count = self.free_block_count();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::MemoryPropertyFlags;
    use crate::MemoryTypeFlags;

    fn desc(size: u64, alignment: u64) -> AllocationDesc {
        AllocationDesc {
            size,
            alignment,
            memory_type: MemoryTypeFlags::DEVICE_LOCAL,
            preferred_flags: MemoryPropertyFlags::empty(),
            required_flags: MemoryPropertyFlags::empty(),
            name: None,
        }
    }

    /// Bug №224: a non-power-of-two heap produced non-power-of-two blocks, and
    /// the `offset ^ size` buddy lookup then pointed at unrelated memory.
    #[test]
    fn a_ragged_heap_is_rounded_up_to_a_power_of_two() {
        let a = BuddyAllocator::new(0, 1000, 16);
        assert_eq!(a.heap_size, 1024);
        assert_eq!(a.blocks.len(), 1);
        assert!(a.blocks[0].size.is_power_of_two());

        // Every block that ever exists must stay a power of two, otherwise the
        // buddy formula is meaningless.
        let mut a = a;
        let mut live = Vec::new();
        for size in [16, 32, 64, 100, 250, 300] {
            if let Ok(alloc) = a.allocate(&desc(size, 16)) {
                live.push(alloc);
            }
        }
        for block in &a.blocks {
            assert!(
                block.size.is_power_of_two(),
                "block size {} is not a power of two",
                block.size
            );
        }
        for alloc in live {
            let _ = a.free(alloc);
        }
    }

    /// Bug №224: the old split loop re-found the block by offset and could hand
    /// out both halves of one block, i.e. two live allocations overlapping.
    #[test]
    fn splits_never_hand_out_overlapping_blocks() {
        let mut a = BuddyAllocator::new(0, 1024, 16);
        let mut allocs = Vec::new();
        for _ in 0..8 {
            allocs.push(a.allocate(&desc(64, 16)).expect("64 bytes must fit 8 times in 1 KiB"));
        }

        // No two live allocations may overlap.
        for (i, x) in allocs.iter().enumerate() {
            for y in allocs.iter().skip(i + 1) {
                let disjoint =
                    x.offset + x.size <= y.offset || y.offset + y.size <= x.offset;
                assert!(disjoint, "allocations overlap: {x:?} and {y:?}");
            }
        }
        // And their total must be within the heap.
        let total: u64 = allocs.iter().map(|a| a.size).sum();
        assert!(total <= 1024, "allocated {total} from a 1024 byte heap");
    }

    /// Bug №226: the offset handed back has to satisfy the requested alignment,
    /// otherwise a descriptor bound at that offset is undefined behaviour.
    #[test]
    fn allocations_honour_the_requested_alignment() {
        for alignment in [1u64, 4, 16, 64, 256] {
            let mut a = BuddyAllocator::new(0, 4096, 16);
            for _ in 0..8 {
                let alloc = a.allocate(&desc(32, alignment)).expect("must fit");
                assert_eq!(
                    alloc.offset % alignment,
                    0,
                    "offset {} is not aligned to {alignment}",
                    alloc.offset
                );
            }
        }
    }

    #[test]
    fn freeing_everything_restores_the_heap() {
        let mut a = BuddyAllocator::new(0, 1024, 16);
        let mut allocs = Vec::new();
        for _ in 0..10 {
            allocs.push(a.allocate(&desc(64, 16)).unwrap());
        }
        for alloc in allocs {
            a.free(alloc).unwrap();
        }
        // Fully merged back into one free block.
        assert_eq!(a.block_count(), 1, "heap did not coalesce: {:?}", a.blocks);
        assert!(a.blocks[0].is_free);
        assert_eq!(a.blocks[0].size, 1024);
        assert_eq!(a.get_memory_stats().current_usage, 0);
    }

    /// Bug №227: `free_block_count` was declared but never written by any
    /// allocator, so the metric always read zero.
    #[test]
    fn free_block_count_tracks_reality() {
        let mut a = BuddyAllocator::new(0, 1024, 16);
        assert_eq!(a.get_memory_stats().free_block_count, 1);

        let x = a.allocate(&desc(64, 16)).unwrap();
        let after_alloc = a.get_memory_stats().free_block_count;
        assert!(after_alloc > 1, "splitting must raise the free count, got {after_alloc}");

        a.free(x).unwrap();
        assert_eq!(a.get_memory_stats().free_block_count, 1);
    }

    /// Bug №224: `free` used the size the caller echoed back, so a wrong size
    /// corrupted the usage accounting.
    #[test]
    fn free_ignores_a_bogus_size_from_the_caller() {
        let mut a = BuddyAllocator::new(0, 1024, 16);
        let mut alloc = a.allocate(&desc(64, 16)).unwrap();
        let real_size = alloc.size;
        alloc.size = 999_999;

        a.free(alloc).unwrap();
        let stats = a.get_memory_stats();
        assert!(stats.current_usage < real_size, "usage should be back near zero");
        assert_eq!(stats.current_usage, 0);
    }

    #[test]
    fn an_impossible_request_is_refused_not_served() {
        let mut a = BuddyAllocator::new(0, 256, 16);
        assert!(a.allocate(&desc(1000, 16)).is_err());
    }
}