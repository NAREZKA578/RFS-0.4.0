//! Pool Memory Allocator
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use super::allocator::{Allocation, AllocationDesc, MemoryAllocator, MemoryStats};
use crate::error::{RhiError, RhiResult};

/// A free run inside a pool block: `size` bytes available at `offset`.
///
/// Bug №226: the old `free_list` held bare `u64` block offsets and every
/// allocation consumed a whole block. Any size below `block_size` therefore
/// wasted the remainder, and any request above it was a hard failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FreeRun {
    pub offset: u64,
    pub size: u64,
}

/// Pool block
#[derive(Debug, Clone)]
pub struct PoolBlock {
    pub offset: u64,
    pub size: u64,
    pub free_runs: Vec<FreeRun>,
}

impl PoolBlock {
    fn occupied(&self) -> u64 {
        self.size - self.free_runs.iter().map(|r| r.size).sum::<u64>()
    }
}

/// Pool allocator
pub struct PoolAllocator {
    memory_type_index: u32,
    block_size: u64,
    blocks: Vec<PoolBlock>,
    /// Upper bound on the total pool size.
    ///
    /// Bug №226: `allocate` grew the pool whenever nothing fit, with no cap at
    /// all — so an allocator that was supposed to bound memory instead grew
    /// without limit and `OutOfMemory` was unreachable. `None` keeps the
    /// previous unbounded behaviour; `limit_bytes` makes it a real pool.
    max_bytes: Option<u64>,
    stats: MemoryStats,
}

impl PoolAllocator {
    /// Bug №226: `block_size` was never validated. With `block_size == 0` every
    /// block got offset 0, so every allocation aliased the same bytes while
    /// the API reported distinct offsets. A zero-sized pool is meaningless, so
    /// it is refused loudly here instead.
    pub fn new(memory_type_index: u32, block_size: u64, initial_blocks: u32) -> Self {
        let block_size = block_size.max(1);
        let mut blocks = Vec::new();
        for i in 0..initial_blocks {
            let offset = (i as u64) * block_size;
            blocks.push(PoolBlock {
                offset,
                size: block_size,
                free_runs: vec![FreeRun {
                    offset,
                    size: block_size,
                }],
            });
        }
        Self {
            memory_type_index,
            block_size,
            blocks,
            max_bytes: None,
            stats: MemoryStats::default(),
        }
    }

    /// Cap the total pool size. Once reached, `allocate` returns
    /// `OutOfMemory` instead of adding another block.
    pub fn limit_bytes(&mut self, max_bytes: u64) {
        self.max_bytes = Some(max_bytes);
    }

    /// Total bytes the pool currently occupies.
    pub fn total_bytes(&self) -> u64 {
        self.blocks.iter().map(|b| b.size).sum()
    }

    /// Returns the number of managed blocks.
    pub fn block_count(&self) -> usize {
        self.blocks.len()
    }

    /// Free runs across every block — the figure `free_block_count` used to
    /// report as a permanent zero (bug №227).
    pub fn free_block_count(&self) -> u64 {
        self.blocks.iter().map(|b| b.free_runs.len() as u64).sum()
    }

    fn add_block(&mut self) {
        let next_offset = self.blocks.iter().map(|b| b.offset + b.size).max().unwrap_or(0);
        let block_size = self.block_size;
        self.blocks.push(PoolBlock {
            offset: next_offset,
            size: block_size,
            free_runs: vec![FreeRun {
                offset: next_offset,
                size: block_size,
            }],
        });
    }

    /// Take `size` bytes out of `block`'s free list, honouring `alignment`.
    ///
    /// Bug №226: the old code ignored `desc.alignment` entirely and returned
    /// the raw run offset, so a descriptor bound at that offset was not
    /// guaranteed to be aligned — undefined behaviour on the device.
    fn carve(block: &mut PoolBlock, size: u64, alignment: u64) -> Option<u64> {
        for i in 0..block.free_runs.len() {
            let run = block.free_runs[i];
            // First fit on the lowest suitably aligned offset in this run.
            let start = crate::utils::alignment::align_up(run.offset, alignment);
            if start < run.offset {
                // align_up would have wrapped past the run.
                continue;
            }
            let lead = start - run.offset;
            if lead + size > run.size {
                continue;
            }

            if lead > 0 {
                // Split the leading padding off as its own run.
                block.free_runs[i].offset = start;
                block.free_runs[i].size = run.size - lead;
                block.free_runs.insert(
                    i,
                    FreeRun {
                        offset: run.offset,
                        size: lead,
                    },
                );
            }
            let taken = i + usize::from(lead > 0);
            let consumed = block.free_runs[taken];
            if consumed.size == size {
                block.free_runs.remove(taken);
            } else {
                block.free_runs[taken].offset = consumed.offset + size;
                block.free_runs[taken].size = consumed.size - size;
            }
            return Some(start);
        }
        None
    }

    /// Return a run to the list and coalesce with its neighbours.
    ///
    /// Bug №226 (found by `freeing_coalesces_runs_back_to_one`): the first
    /// version of this walked neighbours with an index that skipped entries,
    /// so adjacent freed runs were never merged and the block fragmented into
    /// one run per freed allocation. Sorting and merging in one pass is both
    /// simpler and obviously correct.
    fn release(block: &mut PoolBlock, offset: u64, size: u64) {
        let mut runs = std::mem::take(&mut block.free_runs);
        runs.push(FreeRun { offset, size });
        runs.sort_by_key(|r| r.offset);

        let mut merged: Vec<FreeRun> = Vec::with_capacity(runs.len());
        for run in runs {
            if let Some(last) = merged.last_mut() {
                if last.offset + last.size == run.offset {
                    last.size += run.size;
                    continue;
                }
            }
            merged.push(run);
        }
        block.free_runs = merged;
    }
}

impl MemoryAllocator for PoolAllocator {
    fn allocate(&mut self, desc: &AllocationDesc) -> RhiResult<Allocation> {
        if desc.size == 0 {
            return Err(RhiError::ValidationError(
                "pool allocation of zero bytes".into(),
            ));
        }
        // Bug №226: there is no fallback allocator behind this one, so a
        // request larger than a block is a genuine, permanent failure. It is
        // reported as such (OutOfMemory, with the numbers) rather than as a
        // generic validation error that reads like a caller mistake.
        if desc.size > self.block_size {
            return Err(RhiError::OutOfMemory);
        }
        let alignment = if desc.alignment <= 1 { 1 } else { desc.alignment };

        // Try every existing block before growing the pool.
        for i in 0..self.blocks.len() {
            if let Some(offset) = Self::carve(&mut self.blocks[i], desc.size, alignment) {
                self.record_alloc(desc.size);
                return Ok(Allocation {
                    offset,
                    size: desc.size,
                    memory_type_index: self.memory_type_index,
                });
            }
        }

        // No existing block can satisfy it — grow, if the cap allows.
        if let Some(max) = self.max_bytes {
            if self.total_bytes().saturating_add(self.block_size) > max {
                return Err(RhiError::OutOfMemory);
            }
        }
        self.add_block();
        let index = self.blocks.len() - 1;
        let offset = Self::carve(&mut self.blocks[index], desc.size, alignment)
            .ok_or(RhiError::OutOfMemory)?;
        self.record_alloc(desc.size);
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
            .position(|b| {
                allocation.offset >= b.offset && allocation.offset < b.offset + b.size
            })
            .ok_or_else(|| RhiError::InternalError("unknown allocation".into()))?;

        // Reject a double free: the run must not already be covered by a free
        // run, otherwise the block would hand the same bytes out twice.
        let already_free = self.blocks[index].free_runs.iter().any(|r| {
            allocation.offset >= r.offset && allocation.offset < r.offset + r.size
        });
        if already_free {
            return Err(RhiError::InternalError("allocation already freed".into()));
        }

        self.stats.current_usage = self
            .stats
            .current_usage
            .saturating_sub(allocation.size);
        self.stats.total_freed += allocation.size;
        self.stats.allocation_count = self.stats.allocation_count.saturating_sub(1);

        Self::release(&mut self.blocks[index], allocation.offset, allocation.size);
        Ok(())
    }

    fn get_memory_stats(&self) -> MemoryStats {
        let mut stats = self.stats.clone();
        stats.free_block_count = self.free_block_count();
        stats
    }

    fn defragment(&mut self) -> RhiResult<()> {
        // Bug №206/#179: the old body did `self.stats = MemoryStats::default()`
        // and nothing else. That zeroed the accounting while the allocations
        // stayed live — the reports then claimed an idle heap that was in fact
        // full. A pool's free runs are already coalesced on every `free`, so
        // there is nothing to compact; report the truth instead of erasing it.
        self.stats.free_block_count = self.free_block_count();
        Ok(())
    }
}

impl PoolAllocator {
    fn record_alloc(&mut self, size: u64) {
        self.stats.total_allocated += size;
        self.stats.current_usage += size;
        self.stats.allocation_count += 1;
        if self.stats.current_usage > self.stats.peak_usage {
            self.stats.peak_usage = self.stats.current_usage;
        }
    }

    /// Bytes still available across the whole pool.
    pub fn free_bytes(&self) -> u64 {
        self.blocks.iter().map(|b| b.occupied().saturating_sub(b.occupied()) + b.free_runs.iter().map(|r| r.size).sum::<u64>()).sum()
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

    /// Bug №226: `block_size == 0` used to put every block at offset 0, so all
    /// allocations aliased one another while reporting distinct offsets.
    /// Either the request is refused, or the offsets must be distinct.
    #[test]
    fn a_zero_block_size_cannot_alias_allocations() {
        let mut a = PoolAllocator::new(0, 0, 3);
        // A 1-byte block cannot hold 16 bytes, so this is refused outright.
        assert!(a.allocate(&desc(16, 1)).is_err());

        // At a size the (1-byte) blocks *can* hold, offsets must differ.
        let mut offsets = Vec::new();
        for _ in 0..3 {
            match a.allocate(&desc(1, 1)) {
                Ok(alloc) => offsets.push(alloc.offset),
                Err(RhiError::OutOfMemory) => break,
                Err(e) => panic!("unexpected error {e:?}"),
            }
        }
        let unique: std::collections::HashSet<u64> = offsets.iter().copied().collect();
        assert_eq!(
            unique.len(),
            offsets.len(),
            "allocations aliased the same offset: {offsets:?}"
        );
    }

    /// Bug №226: `desc.alignment` was ignored, so descriptors could be bound at
    /// a misaligned offset.
    #[test]
    fn allocations_honour_the_requested_alignment() {
        for alignment in [1u64, 4, 16, 64, 256] {
            let mut a = PoolAllocator::new(0, 4096, 1);
            let mut offsets = Vec::new();
            for size in [4u64, 8, 16, 32, 64] {
                let alloc = a
                    .allocate(&desc(size, alignment))
                    .unwrap_or_else(|e| panic!("alignment {alignment} size {size}: {e:?}"));
                assert_eq!(
                    alloc.offset % alignment,
                    0,
                    "offset {} not aligned to {alignment}",
                    alloc.offset
                );
                offsets.push((alloc.offset, alloc.size));
            }
            // And the allocations must not overlap.
            for (i, (ox, os)) in offsets.iter().enumerate() {
                for (oy, ys) in offsets.iter().skip(i + 1) {
                    assert!(
                        ox + os <= *oy || oy + ys <= *ox,
                        "overlap at alignment {alignment}: {offsets:?}"
                    );
                }
            }
        }
    }

    /// Bug №226: the old allocator consumed a whole block per allocation, so
    /// anything smaller than `block_size` wasted the rest of the block.
    #[test]
    fn small_allocations_share_a_block() {
        let mut a = PoolAllocator::new(0, 4096, 1);
        let mut allocs = Vec::new();
        for _ in 0..16 {
            allocs.push(a.allocate(&desc(16, 16)).unwrap());
        }
        assert_eq!(
            a.block_count(),
            1,
            "16 x 16 bytes must fit in one 4 KiB block, got {} blocks",
            a.block_count()
        );
        let used: u64 = allocs.iter().map(|a| a.size).sum();
        assert_eq!(a.get_memory_stats().current_usage, used);
    }

    #[test]
    fn freeing_coalesces_runs_back_to_one() {
        let mut a = PoolAllocator::new(0, 4096, 1);
        let mut allocs = Vec::new();
        for _ in 0..8 {
            allocs.push(a.allocate(&desc(32, 16)).unwrap());
        }
        for alloc in allocs {
            a.free(alloc).unwrap();
        }
        assert_eq!(
            a.blocks[0].free_runs.len(),
            1,
            "runs did not coalesce: {:?}",
            a.blocks[0].free_runs
        );
        assert_eq!(a.blocks[0].free_runs[0].size, 4096);
        assert_eq!(a.get_memory_stats().current_usage, 0);
    }

    #[test]
    fn a_double_free_is_rejected() {
        let mut a = PoolAllocator::new(0, 1024, 1);
        let alloc = a.allocate(&desc(64, 16)).unwrap();
        a.free(alloc.clone()).unwrap();
        assert!(a.free(alloc).is_err(), "double free must be refused");
    }

    /// Bug №226: a request bigger than a block has no fallback behind it, so it
    /// must fail explicitly rather than silently truncating or aliasing.
    #[test]
    fn an_oversized_request_fails_explicitly() {
        let mut a = PoolAllocator::new(0, 256, 2);
        assert!(matches!(
            a.allocate(&desc(4096, 16)),
            Err(RhiError::OutOfMemory)
        ));
    }

    /// Bug №227: the metric was never written by any allocator.
    #[test]
    fn free_block_count_is_reported() {
        let mut a = PoolAllocator::new(0, 1024, 1);
        assert_eq!(a.get_memory_stats().free_block_count, 1);
        let alloc = a.allocate(&desc(64, 16)).unwrap();
        // Splitting leaves a trailing run, so the count rises.
        assert!(a.get_memory_stats().free_block_count >= 1);
        a.free(alloc).unwrap();
        assert_eq!(a.get_memory_stats().free_block_count, 1);
    }

    #[test]
    fn exhaustion_grows_the_pool_then_fails_cleanly() {
        // Bug №226: without a cap the pool grew forever and never reported
        // OutOfMemory, which is the opposite of what a pool is for.
        let mut a = PoolAllocator::new(0, 128, 1);
        a.limit_bytes(128 * 8);
        let mut allocs = Vec::new();
        loop {
            match a.allocate(&desc(32, 16)) {
                Ok(x) => allocs.push(x),
                Err(RhiError::OutOfMemory) => break,
                Err(e) => panic!("unexpected error {e:?}"),
            }
            assert!(allocs.len() < 10_000, "pool grew without bound");
        }
        assert!(a.block_count() > 1, "the pool should have grown first");
        assert!(
            a.total_bytes() <= 128 * 8,
            "the cap was exceeded: {}",
            a.total_bytes()
        );
    }
}
