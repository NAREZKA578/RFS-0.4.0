//! Acceleration Structure Build Operations
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//! **TODO: Full implementation required**

use super::blas::Blas;
use super::tlas::Tlas;
use crate::resource::buffer::Buffer;

/// Build mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuildMode {
    Build,
    Update,
}

/// Acceleration structure build info
#[derive(Debug, Clone)]
pub struct AccelerationStructureBuildInfo {
    pub ty: AccelerationStructureType,
    pub mode: BuildMode,
    pub dst: AccelerationStructure,
    pub scratch: Buffer,
}

/// Acceleration structure type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AccelerationStructureType {
    Blas,
    Tlas,
}

/// Acceleration structure
#[derive(Debug, Clone)]
pub enum AccelerationStructure {
    Blas(Blas),
    Tlas(Tlas),
}

/// Acceleration structure sizes
#[derive(Debug, Clone, Default)]
pub struct AccelerationStructureSizes {
    pub result_data_size: u64,
    pub scratch_data_size: u64,
    pub build_scratch_data_size: u64,
    pub update_scratch_data_size: u64,
}
