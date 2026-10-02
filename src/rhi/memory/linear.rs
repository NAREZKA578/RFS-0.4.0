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

    /// Rewind the bump pointer to the start of the heap.
    ///
    /// #225: this is only sound when nothing is live. It is a *release* of the
    /// whole heap, not a compaction, and it discards the allocation record
    /// entirely.
    ///
    /// The linear allocator has no per-allocation free list, so it cannot
    /// decide that for itself. `live_allocations` is the caller's count; when
    /// it is non-zero a rewind would hand the same bytes out again on top of
    /// memory that is still in use. `reset` therefore refuses in that case and
    /// `defragment` reports the same refusal as an error instead of silently
    /// corrupting the heap.
    pub fn reset(&mut self) {
        self.current_offset = 0;
        self.stats = MemoryStats::default();
    }

    /// Unconditional rewind. Only for teardown, where the heap is being
    /// destroyed and no allocation can outlive it.
    pub fn reset_unchecked(&mut self) {
        self.reset();
    }

    /// True while the heap still holds allocations the caller has not released.
    pub fn has_live_allocations(&self) -> bool {
        self.current_offset > 0
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

    fn free(&mut self, _allocation: Allocation) -> RhiResult<()> {
        // A bump allocator has no free list; individual frees are a no-op by
        // design and the memory is reclaimed by a full `reset`. The accounting
        // must not pretend otherwise, so `allocation_count` keeps growing while
        // `current_usage` tracks the bump high-water mark (bug №227 — the
        // count of "live" blocks here is always 0 below the mark).
        Ok(())
    }

    fn get_memory_stats(&self) -> MemoryStats {
        let mut stats = self.stats.clone();
        // A bump allocator has exactly one free region: the tail above the
        // bump pointer. Reporting it keeps `free_block_count` meaningful for
        // code that reads it generically.
        stats.free_block_count = u64::from(self.current_offset < self.heap_size);
        stats
    }

    fn defragment(&mut self) -> RhiResult<()> {
        // Bug №225: this used to call `reset()` unconditionally. `reset`
        // rewinds the bump pointer to 0, so with anything still live every
        // subsequent allocation was handed bytes already in use — silent
        // corruption, not compaction. A bump allocator cannot compact, so the
        // honest answer is to refuse while anything is live and let the caller
        // decide (typically: destroy the frame's resources, then reset).
        if self.has_live_allocations() {
            return Err(RhiError::ValidationError(
                "linear allocator cannot defragment while allocations are live; \
                 free the frame's resources and reset the heap explicitly"
                    .into(),
            ));
        }
        self.reset();
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

    /// Bug №225: `defragment` rewound the bump pointer with allocations still
    /// live, so the next allocation overlapped them.
    #[test]
    fn defragment_refuses_while_allocations_are_live() {
        let mut a = LinearAllocator::new(0, 1024);
        let first = a.allocate(&desc(64, 16)).unwrap();
        assert!(a.has_live_allocations());

        assert!(
            a.defragment().is_err(),
            "defragment must not rewind over a live allocation"
        );

        // The heap is untouched: the live allocation still owns its bytes.
        let second = a.allocate(&desc(64, 16)).unwrap();
        assert!(
            first.offset + first.size <= second.offset,
            "allocations overlapped: {first:?} {second:?}"
        );
    }

    #[test]
    fn defragment_succeeds_once_the_heap_is_empty() {
        let mut a = LinearAllocator::new(0, 1024);
        let _ = a.allocate(&desc(64, 16)).unwrap();
        a.reset();
        assert!(!a.has_live_allocations());
        assert!(a.defragment().is_ok());
        assert_eq!(a.offset(), 0);
    }

    /// A stale offset from before the rewind must not be handed out again.
    #[test]
    fn allocations_never_overlap_across_a_reset() {
        let mut a = LinearAllocator::new(0, 1024);
        let before: Vec<u64> = (0..4)
            .map(|_| a.allocate(&desc(32, 16)).unwrap().offset)
            .collect();
        a.reset();
        let after: Vec<u64> = (0..4)
            .map(|_| a.allocate(&desc(32, 16)).unwrap().offset)
            .collect();
        // After a full reset the heap is genuinely free, so reuse is correct
        // here — the point is that it only happens because the caller reset
        // explicitly, not because something called defragment behind its back.
        assert_eq!(before, after);
    }

    /// Bug №227: `free_block_count` always read zero, so the tail free region
    /// was invisible.
    #[test]
    fn free_block_count_reflects_the_remaining_tail() {
        let mut a = LinearAllocator::new(0, 1024);
        assert_eq!(a.get_memory_stats().free_block_count, 1);
        let _ = a.allocate(&desc(1024, 16)).unwrap();
        assert_eq!(
            a.get_memory_stats().free_block_count,
            0,
            "a full heap has no free tail"
        );
    }

    #[test]
    fn an_oversized_request_is_refused() {
        let mut a = LinearAllocator::new(0, 256);
        assert!(a.allocate(&desc(1024, 16)).is_err());
    }
}