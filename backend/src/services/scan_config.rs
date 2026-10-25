//! Canonical source of truth for the scan configuration checksum, which encodes every
//! input that affects hull generation and support detection. If a new input influences
//! either (an algorithm version, a mesh tolerance, a raft parameter), add it to [`compute`]
//! so cached results are invalidated on the next scan. Do not add staleness checks
//! elsewhere.

use crate::geometry::hull::CURRENT_HULL_GENERATION_VERSION;
use crate::geometry::support_separation::CURRENT_SUPPORT_DETECTION_VERSION;

/// Canonical string identifying the inputs used to produce a model's hulls and support
/// detection. Stored on the model; a mismatch with the current value means both are stale.
pub fn compute(raft_height_mm: f32) -> String {
    format!(
        "hull:{CURRENT_HULL_GENERATION_VERSION}|supports:{CURRENT_SUPPORT_DETECTION_VERSION}|raft:{raft_height_mm:.4}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contains_current_hull_generation_version() {
        assert!(compute(2.0).contains(&format!("hull:{CURRENT_HULL_GENERATION_VERSION}")));
    }

    #[test]
    fn produces_expected_format() {
        assert_eq!(
            compute(2.0),
            format!("hull:{CURRENT_HULL_GENERATION_VERSION}|supports:{CURRENT_SUPPORT_DETECTION_VERSION}|raft:2.0000")
        );
    }

    #[test]
    fn formats_raft_height_to_four_decimal_places() {
        for (raft, expected) in [(0.0, "0.0000"), (1.0, "1.0000"), (1.5, "1.5000"), (2.25, "2.2500")] {
            assert!(compute(raft).contains(&format!("raft:{expected}")));
        }
    }

    #[test]
    fn is_deterministic_and_sensitive_to_raft_height() {
        assert_eq!(compute(3.0), compute(3.0));
        assert_ne!(compute(1.0), compute(2.0));
    }
}
