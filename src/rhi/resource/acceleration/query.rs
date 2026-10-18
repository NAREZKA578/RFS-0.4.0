//! Acceleration Structure Query Operations
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

/// Acceleration structure query type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AccelerationStructureQueryType {
    CompactionSize,
    SerializationSize,
    CurrentSize,
}
