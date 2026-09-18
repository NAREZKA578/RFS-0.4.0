//! Query Pool
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//! **TODO: Full implementation required**

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
}

impl QueryPool {
    pub fn new(desc: QueryPoolDesc) -> Self {
        Self { desc }
    }

    pub fn desc(&self) -> &QueryPoolDesc {
        &self.desc
    }
}
