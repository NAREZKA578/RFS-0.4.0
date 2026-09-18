//! Memory Alignment Utilities
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

/// Align up
pub fn align_up(value: u64, alignment: u64) -> u64 {
    (value + alignment - 1) & !(alignment - 1)
}

/// Align down
pub fn align_down(value: u64, alignment: u64) -> u64 {
    value & !(alignment - 1)
}

/// Check if aligned
pub fn is_aligned(value: u64, alignment: u64) -> bool {
    (value & (alignment - 1)) == 0
}

/// Buffer alignment constants
pub const BUFFER_COPY_ALIGNMENT: u64 = 4;
pub const TEXTURE_COPY_ALIGNMENT: u64 = 4;
pub const UNIFORM_BUFFER_ALIGNMENT: u64 = 256;
pub const STORAGE_BUFFER_ALIGNMENT: u64 = 16;
