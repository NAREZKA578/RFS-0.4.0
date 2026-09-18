//! Top-Level Acceleration Structure (TLAS)
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//! **TODO: Full implementation required**

use super::blas::{AccelerationStructureFlags, Blas};

/// TLAS description
#[derive(Debug, Clone)]
pub struct TlasDesc {
    pub instance_count: u32,
    pub flags: AccelerationStructureFlags,
    pub dynamic: bool,
}

/// TLAS instance
#[derive(Debug, Clone)]
pub struct TlasInstance {
    pub transform: [f32; 12],
    pub instance_custom_index: u32,
    pub mask: u32,
    pub instance_shader_binding_table_record_offset: u32,
    pub flags: InstanceFlags,
    pub acceleration_structure: Blas,
}

use bitflags::bitflags;

// Instance flags
bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct InstanceFlags: u32 {
        const OPAQUE = 1 << 0;
        const NO_OPAQUE = 1 << 1;
        const TRIANGLE_FACING_CULL_DISABLE = 1 << 2;
        const TRIANGLE_FLIP_FACING = 1 << 3;
    }
}

/// TLAS resource
#[derive(Debug, Clone)]
pub struct Tlas {
    desc: TlasDesc,
    instances: Vec<TlasInstance>,
}

impl Tlas {
    pub fn new(desc: TlasDesc, instances: Vec<TlasInstance>) -> Self {
        Self { desc, instances }
    }
}
