//! Runtime action translation.
//!
//! When the host app observes an off-tree opponent bet size (one that
//! doesn't match any concrete bet size the trainer used), we map it onto
//! the two bracketing trained anchors using the pseudo-harmonic formula
//! (Ganzfried & Sandholm 2013). The blueprint stores the anchors in its
//! v3 layout, so the runtime does not need to know the trainer's sizing
//! policy at compile time.
//!
//! The training-side `pkr_export::translate::compute_translation` is the
//! full formula with opponent reach in the denominator. At inference
//! time the runtime has no reach estimator; we use the equal-reach form,
//! which is the standard fallback and matches the conservative precompute.

use crate::mmap::MmapReader;

/// Bracket an off-tree pot fraction onto the two nearest trained anchors
/// for the given street.
///
/// Returns `(lower_anchor, upper_anchor, p_lower, p_upper)` where the two
/// probabilities sum to 1.0. If `fraction` is outside the anchor range,
/// both anchors collapse to the nearest one with weight 1.0.
pub fn bracket_bet(
    reader: &MmapReader,
    street: u8,
    fraction: f32,
) -> (f32, f32, f32, f32) {
    let street_idx = (street as usize).min(3);
    let row = reader.anchors()[street_idx];

    // Sort anchors ascending (writer emits them in order but be defensive).
    let mut a = [row[0], row[1], row[2]];
    a.sort_by(|x, y| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal));

    // Preflop: no partial sizes. All mass to the all-in anchor.
    if a[2] == 0.0 {
        return (1.0, 1.0, 1.0, 0.0);
    }

    if fraction <= a[0] {
        return (a[0], a[0], 1.0, 0.0);
    }
    if fraction >= a[2] {
        return (a[2], a[2], 1.0, 0.0);
    }

    let (lower, upper) = if fraction <= a[1] { (a[0], a[1]) } else { (a[1], a[2]) };

    // Equal-reach pseudo-harmonic: linear interpolation of mass.
    let p_lower = ((upper - fraction) / (upper - lower)).clamp(0.0, 1.0);
    (lower, upper, p_lower, 1.0 - p_lower)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fake MmapReader is not constructible; this is a signature-only
    /// compile check that the module builds. Integration coverage is in
    /// crates/pkr-runtime/tests/roundtrip.rs.
    #[test]
    fn signature_is_stable() {
        fn _sig(_r: &MmapReader, _street: u8, _frac: f32) -> (f32, f32, f32, f32) {
            unreachable!()
        }
    }
}
