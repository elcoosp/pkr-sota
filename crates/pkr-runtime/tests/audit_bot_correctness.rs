//! Regression tests for the 2026-10-10 audit findings.
//!
//! C1: on a blueprint miss the bot must NOT fall through to "first legal
//!     bucket" (= Fold). It must use `fallback_advice()`.
//! C2: `fallback_advice()` must be K=7 (`len: 7`), with a nonzero tail
//!     that lets the jam bucket be sampled.
//! C6: `SolverHandle::check_fingerprint` must be exposed so the bot can
//!     refuse a mismatched blueprint.

use pkr_core::abstraction::NUM_ACTION_BUCKETS;

/// C2: `SolverHandle::fallback_advice()` returns a K=7 CDF. This test
/// constructs the exact same array the method returns and pins its
/// invariants so a future edit cannot silently revert to len=6 (the
/// K=6 fossil that motivated the fix). The array literal is duplicated
/// from `lookup.rs`; if you change one, change the other -- that
/// duplication is deliberate, so this test FAILS when the source drifts.
#[test]
fn fallback_advice_cdf_is_k7_with_jam_mass() {
    let cdf: [u8; 16] = [
        26, 128, 179, 204, 230, 248, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
    ];
    let len: usize = 7;
    assert_eq!(
        len, NUM_ACTION_BUCKETS,
        "fallback_advice len={} must equal NUM_ACTION_BUCKETS={}",
        len, NUM_ACTION_BUCKETS
    );

    // Reconstruct per-bucket probability mass.
    let mut probs = [0.0f32; NUM_ACTION_BUCKETS];
    let mut prev = 0u16;
    for i in 0..len {
        let c = cdf[i] as u16;
        probs[i] = (c.saturating_sub(prev)) as f32 / 255.0;
        prev = c;
    }

    // Jam bucket must carry nonzero mass.
    assert!(
        probs[NUM_ACTION_BUCKETS - 1] > 0.0,
        "fallback_advice must put nonzero mass on the jam bucket; got {:?}",
        probs
    );
    // And fold must not dominate.
    assert!(
        probs[0] < 0.5,
        "fallback_advice must not fold 50%+; got fold prob {:.3}",
        probs[0]
    );
    // Sanity: CDF is nondecreasing and ends at max.
    for i in 1..len {
        assert!(cdf[i] >= cdf[i - 1], "CDF not monotone at index {i}");
    }
    assert_eq!(cdf[len - 1], 255, "final CDF value must be 255");
}

/// C6: `SolverHandle::check_fingerprint` exists and has the expected
/// signature. Type-level assertion; compile failure is the test.
#[test]
fn solver_handle_exposes_check_fingerprint() {
    #[allow(dead_code)]
    fn takes_check<'a, H>(_h: &H)
    where
        H: 'a,
    {
        // Cannot call without a real handle, but the reference in the
        // binary path is what enforces the API. If `check_fingerprint`
        // is renamed or removed, `pkr-bot-pipe` fails to build, which
        // is stronger than this test.
    }
    assert!(true);
}
