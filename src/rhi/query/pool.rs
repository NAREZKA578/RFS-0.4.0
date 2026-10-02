//! Query Pool
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use super::results::{PipelineStatistics, QueryResult};
use bitflags::bitflags;

/// Query type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum QueryType {
    #[default]
    Occlusion,
    BinaryOcclusion,
    Timestamp,
    PipelineStatistics,
    AccelerationStructureCompactionSize,
}

// Query pool flags
bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub struct QueryPoolFlags: u32 {
        const NONE = 0;
    }
}

/// Query pool description
#[derive(Debug, Clone, Default)]
pub struct QueryPoolDesc {
    pub ty: QueryType,
    pub count: u32,
    pub flags: QueryPoolFlags,
}

/// Query pool
pub struct QueryPool {
    desc: QueryPoolDesc,
    results: Vec<Option<QueryResult>>,
    available: Vec<bool>,
    /// Slots between `begin` and `end` — Bug №182.
    ///
    /// A Vulkan query is bracketed by begin/end, and a result may only appear
    /// for a slot that was actually opened. There was no notion of that state
    /// at all, so a result could be attached to a query that was never issued
    /// and a stale result could be attached to a finished one — both read back
    /// as authoritative.
    active: Vec<bool>,
    /// Counts writes rejected because the slot was not active (Bug №182), so a
    /// backend that fails to bracket its queries is visible rather than silent.
    rejected_writes: u64,
}

impl QueryPool {
    pub fn new(desc: QueryPoolDesc) -> Self {
        let count = desc.count as usize;
        Self {
            desc,
            results: vec![None; count],
            available: vec![false; count],
            active: vec![false; count],
            rejected_writes: 0,
        }
    }

    pub fn desc(&self) -> &QueryPoolDesc {
        &self.desc
    }

    /// Returns the number of slots in the pool.
    pub fn count(&self) -> usize {
        self.desc.count as usize
    }

    /// Returns `true` when the query at `index` has an available result.
    pub fn is_available(&self, index: u32) -> bool {
        self.available.get(index as usize).copied().unwrap_or(false)
    }

    /// Returns `true` when the query at `index` has been begun and not ended.
    pub fn is_active(&self, index: u32) -> bool {
        self.active.get(index as usize).copied().unwrap_or(false)
    }

    /// Opens the query at `index`.
    ///
    /// Returns `false` for an out-of-range index or a slot that is already open
    /// (Vulkan forbids re-beginning an active query).
    pub fn begin(&mut self, index: u32) -> bool {
        let Some(slot) = self.active.get_mut(index as usize) else {
            return false;
        };
        if *slot {
            return false;
        }
        *slot = true;
        true
    }

    /// Closes the query at `index`. A result must have been written by then.
    pub fn end(&mut self, index: u32) -> bool {
        let Some(slot) = self.active.get_mut(index as usize) else {
            return false;
        };
        if !*slot {
            return false;
        }
        *slot = false;
        true
    }

    /// Writes a result for the query at `index`.
    ///
    /// Bug №182: this used to be a silent no-op for an out-of-range index, and
    /// it accepted a write for a slot that was never begun. Both failures were
    /// invisible, so a query pool could report a plausible-looking result that
    /// no GPU work ever produced. It now returns `false` and counts the
    /// rejection.
    pub fn write(&mut self, index: u32, result: QueryResult) -> bool {
        let in_range = (index as usize) < self.results.len();
        if !in_range {
            self.rejected_writes += 1;
            return false;
        }
        if !self.active[index as usize] {
            self.rejected_writes += 1;
            return false;
        }
        self.results[index as usize] = Some(result);
        self.available[index as usize] = true;
        true
    }

    /// Writes a result for a slot regardless of its begin/end state.
    ///
    /// For a backend that already owns the bracketing (a real Vulkan command
    /// buffer, where the driver enforces the ordering) and for tests that
    /// exercise the result path without the command-buffer machinery.
    pub fn write_unvalidated(&mut self, index: u32, result: QueryResult) -> bool {
        let Some(slot) = self.results.get_mut(index as usize) else {
            self.rejected_writes += 1;
            return false;
        };
        *slot = Some(result);
        if let Some(avail) = self.available.get_mut(index as usize) {
            *avail = true;
        }
        true
    }

    /// Writes rejected because the slot was out of range or not active.
    pub fn rejected_writes(&self) -> u64 {
        self.rejected_writes
    }

    /// Returns the result for the query at `index`.
    pub fn get(&self, index: u32) -> Option<&QueryResult> {
        self.results.get(index as usize).and_then(|r| r.as_ref())
    }

    /// Takes and removes the result for the query at `index`.
    pub fn take(&mut self, index: u32) -> Option<QueryResult> {
        let result = self.results.get_mut(index as usize)?.take();
        if let Some(avail) = self.available.get_mut(index as usize) {
            *avail = false;
        }
        result
    }

    /// Clears all stored results and ends every open query.
    ///
    /// Bug №182: `reset` used to clear results and availability but left the
    /// (then-nonexistent) activity state alone. A slot left "open" across a
    /// reset would accept a result that belonged to work from the previous run.
    pub fn reset(&mut self) {
        self.results = vec![None; self.desc.count as usize];
        self.available = vec![false; self.desc.count as usize];
        self.active = vec![false; self.desc.count as usize];
    }
}

impl QueryResult {
    /// Returns a default result appropriate for the given query type.
    pub fn default_for(ty: QueryType) -> Self {
        match ty {
            QueryType::Occlusion | QueryType::BinaryOcclusion => QueryResult::Occlusion(false),
            QueryType::Timestamp => QueryResult::Timestamp(0),
            QueryType::PipelineStatistics => {
                QueryResult::PipelineStatistics(PipelineStatistics::default())
            }
            QueryType::AccelerationStructureCompactionSize => {
                QueryResult::CompactionSize(0)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pool(count: u32) -> QueryPool {
        QueryPool::new(QueryPoolDesc {
            ty: QueryType::Timestamp,
            count,
            flags: QueryPoolFlags::NONE,
        })
    }

    /// Bug №182: writing to a slot that was never begun used to succeed, so the
    /// pool could report a result that no GPU work ever produced.
    #[test]
    fn writing_to_an_inactive_slot_is_refused() {
        let mut p = pool(4);
        assert!(!p.is_active(0));
        assert!(!p.write(0, QueryResult::Timestamp(1)), "must be refused");
        assert!(!p.is_available(0));
        assert_eq!(p.get(0), None);
        assert_eq!(p.rejected_writes(), 1);
    }

    /// Bug №182: an out-of-range index used to be a silent no-op.
    #[test]
    fn writing_out_of_range_is_refused_and_counted() {
        let mut p = pool(2);
        assert!(!p.write(99, QueryResult::Timestamp(1)));
        assert_eq!(p.rejected_writes(), 1);
        assert!(!p.write_unvalidated(99, QueryResult::Timestamp(1)));
        assert_eq!(p.rejected_writes(), 2);
    }

    #[test]
    fn a_bracketed_query_accepts_exactly_one_result() {
        let mut p = pool(2);
        assert!(p.begin(0));
        assert!(p.is_active(0));
        assert!(!p.begin(0), "a query cannot be begun twice");
        assert!(p.write(0, QueryResult::Timestamp(12_345)));
        assert!(p.is_available(0));
        assert!(p.end(0));
        assert!(!p.is_active(0));
        assert!(!p.end(0), "a query cannot be ended twice");

        match p.get(0) {
            Some(QueryResult::Timestamp(v)) => assert_eq!(*v, 12_345),
            other => panic!("expected a timestamp, got {other:?}"),
        }
    }

    /// After a query is closed its result must no longer be writable, or a
    /// later run could overwrite this frame's answer.
    #[test]
    fn a_finished_query_no_longer_accepts_writes() {
        let mut p = pool(1);
        p.begin(0);
        p.write(0, QueryResult::Timestamp(1));
        p.end(0);
        assert!(!p.write(0, QueryResult::Timestamp(2)));
        match p.get(0) {
            Some(QueryResult::Timestamp(v)) => assert_eq!(*v, 1, "the value must stand"),
            other => panic!("expected a timestamp, got {other:?}"),
        }
    }

    /// Bug №182: a reset that left a slot open would accept a result from the
    /// previous run.
    #[test]
    fn reset_ends_open_queries() {
        let mut p = pool(1);
        p.begin(0);
        p.write(0, QueryResult::Timestamp(1));
        p.reset();
        assert!(!p.is_active(0));
        assert!(!p.is_available(0));
        assert!(!p.write(0, QueryResult::Timestamp(2)), "a reset pool has no active query");
    }

    #[test]
    fn begin_and_end_reject_out_of_range_slots() {
        let mut p = pool(1);
        assert!(!p.begin(5));
        assert!(!p.end(5));
    }

    /// The escape hatch stays available for backends that own the bracketing.
    #[test]
    fn the_unvalidated_path_still_works_and_still_bounds_checks() {
        let mut p = pool(1);
        assert!(p.write_unvalidated(0, QueryResult::Timestamp(7)));
        assert!(p.is_available(0));
        assert!(!p.write_unvalidated(3, QueryResult::Timestamp(7)));
    }
}