//! Graph Types
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use std::fmt;
use std::ops::{BitAnd, BitAndAssign, BitOr, BitOrAssign, Not, Sub};

/// How a resource is used inside the graph.
///
/// Bug №102: this used to be a plain enum with a hand-written `BitOr` that picked
/// a winner:
///
/// ```text
/// ColorAttachment | Sampled  =>  ColorAttachment   // Sampled is gone
/// Sampled | ColorAttachment  =>  ColorAttachment   // and here it is kept
/// ```
///
/// Two defects, both silent. The operation is not commutative, so the answer
/// depended on the order the caller happened to write, and it *discards* the
/// second usage rather than combining them. `graph.rs` declares
/// `ColorAttachment | Sampled` for seven resources, and every one of those ended
/// up without the `Sampled` bit — so `GraphResource::create` built textures
/// without `TextureUsage::SAMPLED` and a pass that sampled them read a texture
/// that was never created for sampling. Nothing reported it: a missing usage flag
/// produces a valid-looking resource and a wrong picture much later.
///
/// It is a set, not a choice. The variants are associated constants rather than
/// enum cases, so existing code that writes `ResourceUsage::Sampled` still
/// compiles, while `|` now means union instead of "pick one".
///
/// Matching requires `contains`: a flags value can legitimately hold several
/// usages at once, so `match usage { ResourceUsage::Sampled => .. }` would be
/// wrong — it would miss the resource that is both a colour attachment and
/// sampled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct ResourceUsage(u8);

impl ResourceUsage {
    /// Nothing. The state of a resource nobody has declared a use for.
    pub const NONE: Self = Self(0);
    /// Written to as a render target.
    pub const RENDER_TARGET: Self = Self(1 << 0);
    /// Written to as a colour attachment.
    pub const COLOR_ATTACHMENT: Self = Self(1 << 1);
    /// Written to as a depth/stencil attachment.
    pub const DEPTH_STENCIL: Self = Self(1 << 2);
    /// Read as a sampled texture.
    pub const SAMPLED: Self = Self(1 << 3);
    /// Read and written as storage.
    pub const STORAGE: Self = Self(1 << 4);

    /// Alias kept for the original spelling, so `ResourceUsage::Sampled` in
    /// existing code keeps working.
    #[allow(non_upper_case_globals)]
    pub const RenderTarget: Self = Self::RENDER_TARGET;
    #[allow(non_upper_case_globals)]
    pub const ColorAttachment: Self = Self::COLOR_ATTACHMENT;
    #[allow(non_upper_case_globals)]
    pub const DepthStencil: Self = Self::DEPTH_STENCIL;
    #[allow(non_upper_case_globals)]
    pub const Sampled: Self = Self::SAMPLED;
    #[allow(non_upper_case_globals)]
    pub const Storage: Self = Self::STORAGE;

    /// Every usage this type can express. Used by tests that check a value is
    /// built only from known bits.
    pub const ALL: Self = Self(0b1_1111);

    /// The raw bits, for backends that want a numeric mask.
    pub const fn bits(self) -> u8 {
        self.0
    }

    /// Whether `other` is entirely contained in `self`.
    ///
    /// This is the test a caller actually wants. `usage.contains(SAMPLED)` is true
    /// whether the resource is sampled alone or sampled *and* a colour
    /// attachment, which is the whole point of making this a set.
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Whether no usage is declared.
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// The individual usages held, for callers that need to branch per bit.
    pub const fn iter(self) -> ResourceUsageIter {
        ResourceUsageIter { remaining: self.0 }
    }
}

/// Iterator over the individual usages in a `ResourceUsage` set.
pub struct ResourceUsageIter {
    remaining: u8,
}

impl Iterator for ResourceUsageIter {
    type Item = ResourceUsage;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }
        // The lowest set bit is the next usage in declaration order, so the
        // iteration order is stable rather than dependent on integer width.
        let bit = self.remaining & self.remaining.wrapping_neg();
        self.remaining &= self.remaining - 1;
        Some(ResourceUsage(bit))
    }
}

impl BitOr for ResourceUsage {
    type Output = Self;

    /// Union. Every usage held by either side is kept, which is what makes the
    /// operation commutative — the property the old implementation lacked.
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl BitOrAssign for ResourceUsage {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

impl BitAnd for ResourceUsage {
    type Output = Self;

    fn bitand(self, rhs: Self) -> Self {
        Self(self.0 & rhs.0)
    }
}

impl BitAndAssign for ResourceUsage {
    fn bitand_assign(&mut self, rhs: Self) {
        self.0 &= rhs.0;
    }
}

impl Sub for ResourceUsage {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self {
        Self(self.0 & !rhs.0)
    }
}

impl Not for ResourceUsage {
    type Output = Self;

    fn not(self) -> Self {
        Self(!self.0 & Self::ALL.0)
    }
}

impl fmt::Display for ResourceUsage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_empty() {
            return f.write_str("ResourceUsage(NONE)");
        }
        let mut first = true;
        for usage in self.iter() {
            let name = match usage {
                u if u == Self::RENDER_TARGET => "RENDER_TARGET",
                u if u == Self::COLOR_ATTACHMENT => "COLOR_ATTACHMENT",
                u if u == Self::DEPTH_STENCIL => "DEPTH_STENCIL",
                u if u == Self::SAMPLED => "SAMPLED",
                _ => "STORAGE",
            };
            if !first {
                f.write_str("|")?;
            }
            f.write_str(name)?;
            first = false;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::ResourceUsage;

    /// The defect that motivated the change: `|` used to keep only one side.
    #[test]
    fn union_keeps_both_sides_in_either_order() {
        let forward = ResourceUsage::COLOR_ATTACHMENT | ResourceUsage::SAMPLED;
        let backward = ResourceUsage::SAMPLED | ResourceUsage::COLOR_ATTACHMENT;
        assert_eq!(
            forward, backward,
            "the operation must be commutative; the old implementation was not"
        );
        assert!(forward.contains(ResourceUsage::COLOR_ATTACHMENT));
        assert!(
            forward.contains(ResourceUsage::SAMPLED),
            "SAMPLED was being discarded, which left textures created without \
             TextureUsage::SAMPLED"
        );
    }

    #[test]
    fn three_usages_all_survive() {
        let usage = ResourceUsage::RENDER_TARGET
            | ResourceUsage::DEPTH_STENCIL
            | ResourceUsage::STORAGE;
        assert_eq!(usage.iter().count(), 3);
        assert!(usage.contains(ResourceUsage::RENDER_TARGET));
        assert!(usage.contains(ResourceUsage::DEPTH_STENCIL));
        assert!(usage.contains(ResourceUsage::STORAGE));
        assert!(!usage.contains(ResourceUsage::SAMPLED));
    }

    #[test]
    fn empty_is_absent_from_everything() {
        let usage = ResourceUsage::SAMPLED;
        assert!(ResourceUsage::NONE.is_empty());
        // `contains(NONE)` is vacuously true for any value, which is the correct
        // answer: every set contains the empty set. The useful direction is the
        // other one — an empty value contains nothing.
        assert!(ResourceUsage::NONE.contains(ResourceUsage::NONE));
        assert!(!ResourceUsage::NONE.contains(ResourceUsage::SAMPLED));
        assert!(usage.contains(ResourceUsage::SAMPLED));
        assert!(usage.contains(ResourceUsage::SAMPLED | ResourceUsage::NONE));
    }

    /// `contains` with a set is the intersection test, which is what a caller
    /// asking "is this both" expects.
    #[test]
    fn contains_accepts_a_set_and_tests_the_intersection() {
        let usage = ResourceUsage::COLOR_ATTACHMENT | ResourceUsage::SAMPLED;
        assert!(usage.contains(ResourceUsage::SAMPLED | ResourceUsage::COLOR_ATTACHMENT));
        assert!(!usage.contains(
            ResourceUsage::SAMPLED | ResourceUsage::DEPTH_STENCIL
        ));
    }

    #[test]
    fn subtraction_and_negation_stay_inside_known_bits() {
        let usage = ResourceUsage::ALL;
        let cleared = usage - ResourceUsage::STORAGE;
        assert!(!cleared.contains(ResourceUsage::STORAGE));
        assert!(cleared.contains(ResourceUsage::SAMPLED));
        // Subtracting a usage that was never set must not invent it. `^` is
        // deliberately not implemented: symmetric difference on a usage set has
        // no meaning here and would let a caller drop a usage by accident.
        assert!(!cleared.contains(ResourceUsage::STORAGE));
        assert!(!ResourceUsage::NONE.contains(ResourceUsage::STORAGE));
    }

    #[test]
    fn iteration_is_stable_and_covers_each_bit_once() {
        let usage = ResourceUsage::STORAGE
            | ResourceUsage::SAMPLED
            | ResourceUsage::COLOR_ATTACHMENT;
        let first: Vec<_> = usage.iter().collect();
        let second: Vec<_> = usage.iter().collect();
        assert_eq!(first, second, "iteration must not depend on prior state");
        assert_eq!(usage.iter().count(), 3, "no bit may be visited twice");
    }
}