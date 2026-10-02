//! Memory Alignment Utilities
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

/// Round `value` up to the next multiple of `alignment`.
///
/// Bug №223: the old body was `(value + alignment - 1) & !(alignment - 1)`.
/// That has two defects:
///
/// 1. `alignment == 0` evaluates `value + u64::MAX`, which panics on overflow in
///    a debug build and wraps to nonsense in a release build.
/// 2. The bitmask form is only valid when `alignment` is a power of two. For
///    3, 5 or 100 the mask `!(alignment - 1)` is not a bit mask at all and the
///    result is garbage — silently, with no panic to warn you.
///
/// This version works for any non-zero alignment and treats 0 as "no
/// alignment required", which is the only meaningful reading of a zero
/// alignment request.
pub fn align_up(value: u64, alignment: u64) -> u64 {
    if alignment <= 1 {
        return value;
    }
    let remainder = value % alignment;
    if remainder == 0 {
        value
    } else {
        // Checked so a huge alignment cannot wrap back to a small offset.
        value
            .checked_add(alignment - remainder)
            .expect("align_up overflow: value plus alignment exceeds u64")
    }
}

/// Round `value` down to a multiple of `alignment`.
///
/// Bug №223: same power-of-two-only assumption as the old `align_down`; now
/// correct for any alignment, and `alignment <= 1` is a no-op.
pub fn align_down(value: u64, alignment: u64) -> u64 {
    if alignment <= 1 {
        return value;
    }
    value - (value % alignment)
}

/// Is `value` a multiple of `alignment`?
///
/// Bug №223: the old bitmask form reported nonsense for non-power-of-two
/// alignments; now it is a plain remainder test.
pub fn is_aligned(value: u64, alignment: u64) -> bool {
    if alignment <= 1 {
        return true;
    }
    value.is_multiple_of(alignment)
}

/// Buffer alignment constants
pub const BUFFER_COPY_ALIGNMENT: u64 = 4;
pub const TEXTURE_COPY_ALIGNMENT: u64 = 4;
pub const UNIFORM_BUFFER_ALIGNMENT: u64 = 256;
pub const STORAGE_BUFFER_ALIGNMENT: u64 = 16;
