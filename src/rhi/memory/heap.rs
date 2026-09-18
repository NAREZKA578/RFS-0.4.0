//! Memory Heap
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//! **TODO: Full implementation required**

use crate::types::*;

/// Memory heap
#[derive(Debug, Clone)]
pub struct MemoryHeap {
    pub index: u32,
    pub size: u64,
    pub flags: MemoryHeapFlags,
}
