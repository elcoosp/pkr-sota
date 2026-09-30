//! Regression tests for the fingerprint guards added by F4 and F6.
//!
//! These catch a class of bug the audit called out: loading a
//! checkpoint against abstraction tables from a different feature
//! space or legal-action tree, which silently produces nonsense
//! infosets. The runtime `load_checkpoint` path uses
//! `describe_mismatch`, so a mismatch there produces the same message
//! these tests assert on.

use pkr_core::abstraction::AbstractionFingerprint;

#[test]
fn action_legal_v_mismatch_is_named() {
    let a = AbstractionFingerprint::from_constants(200);
    let mut b = a;
    b.action_legal_v = a.action_legal_v.wrapping_add(1);
    let msg = a.describe_mismatch(&b);
    assert!(
        msg.contains("action_legal_v"),
        "message should name action_legal_v, got: {msg}"
    );
}

#[test]
fn centroid_feature_v_mismatch_is_named() {
    let a = AbstractionFingerprint::from_constants(200);
    let mut b = a;
    b.centroid_feature_v = a.centroid_feature_v.wrapping_add(1);
    let msg = a.describe_mismatch(&b);
    assert!(
        msg.contains("centroid_feature_v"),
        "message should name centroid_feature_v, got: {msg}"
    );
}

#[test]
fn matching_fields_compare_equal() {
    let a = AbstractionFingerprint::from_constants(200);
    let b = AbstractionFingerprint::from_constants(200);
    assert_eq!(a.action_legal_v, b.action_legal_v);
    assert_eq!(a.centroid_feature_v, b.centroid_feature_v);
}

#[test]
fn struct_size_is_still_40_bytes() {
    assert_eq!(
        std::mem::size_of::<AbstractionFingerprint>(),
        40,
        "fingerprint must stay 40 bytes for the checkpoint format"
    );
}
