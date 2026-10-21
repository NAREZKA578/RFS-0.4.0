//! Guards for non-finite values entering the damage model.
//!
//! # Why this exists
//!
//! Rust's `f32::min`, `f32::max` and `f32::clamp` have one behaviour that is
//! easy to forget and expensive in a simulation: **given a NaN they return the
//! other operand** rather than propagating it. `f32::clamp` additionally
//! returns NaN unchanged, because both `NaN < min` and `NaN > max` are false.
//!
//! So a single NaN reaching an `amount.min(space)` does not stay a NaN. It
//! becomes the *extreme* value — a compartment fills to capacity in one call, or
//! a fire spreads to every neighbour at full intensity — and nothing reports an
//! error. The simulation looks like it is working.
//!
//! Guards written as `amount <= 0.0` do not help: every comparison against NaN is
//! false, so the guard passes it through.
//!
//! These helpers are the single place that behaviour is handled, so a new call
//! site does not have to remember it.

/// Whether `v` is usable as a positive amount.
///
/// This is the predicate to use before any `min`/`max`/`clamp` on an amount:
/// `is_positive_finite` is false for NaN, which `v <= 0.0` is not.
#[inline]
pub fn is_positive_finite(v: f32) -> bool {
    v.is_finite() && v > 0.0
}

/// A non-negative amount, with anything unusable mapped to zero.
///
/// Zero means "nothing happened", which is the safe reading for a caller that
/// passed something unusable: no water moved, no damage applied.
#[inline]
pub fn positive_or_zero(v: f32) -> f32 {
    if v.is_finite() {
        v.max(0.0)
    } else {
        0.0
    }
}

/// An intensity in `0.0..=1.0`, with anything unusable mapped to zero.
///
/// Unusable input becomes **zero**, not one. For fire, `clamp` turning a NaN
/// into 1.0 is the difference between a compartment quietly not burning and
/// every compartment on the ship being alight.
#[inline]
pub fn unit_or_zero(v: f32) -> f32 {
    if v.is_finite() {
        v.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// `v` clamped to `lo..=hi`, tolerant of unusable bounds and values.
///
/// An inverted range (`lo > hi`) comes from bad data and must not panic:
/// `f32::clamp` asserts on its bounds. Returns `v` unchanged there, because
/// there is no valid interval to clamp into.
#[inline]
pub fn safe_clamp(v: f32, lo: f32, hi: f32) -> f32 {
    if !v.is_finite() {
        return 0.0;
    }
    if !lo.is_finite() || !hi.is_finite() || lo > hi {
        return v;
    }
    v.clamp(lo, hi)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    fn nan_is_not_a_positive_amount() {
        // The whole point: the naive guard `v <= 0.0` lets NaN through,
        // because every comparison against NaN is false. The lint objecting to
        // `!(nan <= 0.0)` is the very confusion this test documents, so the
        // negated comparison is the assertion, not an oversight.
        let nan = f32::NAN;
        assert!(!is_positive_finite(nan));
        assert!(!(nan <= 0.0), "the naive `<= 0.0` guard is unsound");
    }

    #[test]
    fn infinities_are_not_positive_amounts() {
        assert!(!is_positive_finite(f32::INFINITY));
        assert!(!is_positive_finite(f32::NEG_INFINITY));
    }

    #[test]
    fn positive_amounts_pass() {
        assert!(is_positive_finite(1.0));
        assert!(is_positive_finite(0.001));
    }

    #[test]
    fn positive_or_zero_maps_every_unusable_value_to_zero() {
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert_eq!(positive_or_zero(bad), 0.0, "{bad} was not rejected");
        }
        assert_eq!(positive_or_zero(-5.0), 0.0, "a negative amount survived");
        assert_eq!(positive_or_zero(3.5), 3.5);
    }

    #[test]
    fn unit_or_zero_maps_nan_to_zero_not_to_one() {
        // `f32::clamp` hands NaN straight back, which is what let fire spread at
        // full intensity. Zero is the safe reading of "unusable".
        assert_eq!(unit_or_zero(f32::NAN), 0.0);
        let clamped = f32::NAN.clamp(0.0, 1.0);
        assert!(clamped.is_nan(), "clamp propagated the NaN");
    }

    #[test]
    fn unit_or_zero_clamps_and_rejects() {
        assert_eq!(unit_or_zero(2.0), 1.0);
        assert_eq!(unit_or_zero(-1.0), 0.0);
        assert_eq!(unit_or_zero(0.25), 0.25);
        assert_eq!(unit_or_zero(f32::INFINITY), 0.0);
    }

    #[test]
    fn safe_clamp_does_not_panic_on_an_inverted_range() {
        // `f32::clamp` asserts `min <= max`, so bad data would abort the process
        // rather than merely misbehave.
        assert_eq!(safe_clamp(5.0, 10.0, 0.0), 5.0);
    }

    #[test]
    fn safe_clamp_does_not_panic_on_non_finite_bounds() {
        assert_eq!(safe_clamp(5.0, f32::NAN, 10.0), 5.0);
        assert_eq!(safe_clamp(5.0, 0.0, f32::NAN), 5.0);
    }

    #[test]
    fn safe_clamp_rejects_a_non_finite_value() {
        assert_eq!(safe_clamp(f32::NAN, 0.0, 10.0), 0.0);
        assert_eq!(safe_clamp(f32::INFINITY, 0.0, 10.0), 0.0);
    }

    #[test]
    fn safe_clamp_clamps_a_usable_value() {
        assert_eq!(safe_clamp(-1.0, 0.0, 10.0), 0.0);
        assert_eq!(safe_clamp(11.0, 0.0, 10.0), 10.0);
        assert_eq!(safe_clamp(5.0, 0.0, 10.0), 5.0);
    }
}
