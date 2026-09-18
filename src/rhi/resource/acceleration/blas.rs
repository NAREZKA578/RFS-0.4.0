//! Bottom-Level Acceleration Structure (BLAS)
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//! **TODO: Full implementation required**

use crate::resource::buffer::Buffer;
use crate::types::*;

/// BLAS description
#[derive(Debug, Clone)]
pub struct BlasDesc {
    pub geometries: Vec<AccelerationStructureGeometry>,
    pub flags: AccelerationStructureFlags,
}

use bitflags::bitflags;

// Acceleration structure flags
bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct AccelerationStructureFlags: u32 {
        const ALLOW_UPDATE = 1 << 0;
        const ALLOW_COMPACTION = 1 << 1;
        const PREFER_FAST_TRACE = 1 << 2;
        const PREFER_FAST_BUILD = 1 << 3;
    }
}

/// Geometry type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GeometryType {
    Triangles,
    AABBs,
    Instances,
}

// Geometry flags
bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct GeometryFlags: u32 {
        const OPAQUE = 1 << 0;
        const NO_DUPLICATE_ANY_HIT_INVOCATION = 1 << 1;
    }
}

/// Acceleration structure geometry
#[derive(Debug, Clone)]
pub struct AccelerationStructureGeometry {
    pub geometry_type: GeometryType,
    pub geometry: GeometryData,
    pub flags: GeometryFlags,
}

/// Geometry data
#[derive(Debug, Clone)]
pub enum GeometryData {
    Triangles(TriangleGeometry),
    AABBs(AabbGeometry),
    Instances(InstanceGeometry),
}

/// Triangle geometry
#[derive(Debug, Clone)]
pub struct TriangleGeometry {
    pub vertex_format: Format,
    pub vertex_data: Buffer,
    pub vertex_offset: u64,
    pub vertex_stride: u64,
    pub vertex_count: u32,
    pub max_vertex: u32,
    pub index_type: IndexType,
    pub index_data: Buffer,
    pub index_offset: u64,
    pub transform_data: Option<Buffer>,
}

/// AABB geometry
#[derive(Debug, Clone)]
pub struct AabbGeometry {
    pub data: Buffer,
    pub offset: u64,
    pub stride: u64,
    pub count: u32,
}

/// Instance geometry
#[derive(Debug, Clone)]
pub struct InstanceGeometry {
    pub data: Buffer,
    pub offset: u64,
    pub count: u32,
}

/// BLAS resource
#[derive(Debug, Clone)]
pub struct Blas {
    desc: BlasDesc,
}

impl Blas {
    pub fn new(desc: BlasDesc) -> Self {
        Self { desc }
    }
}
