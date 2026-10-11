//! Save -> load roundtrip for `CompactRegretTable`. Exercises the
//! streaming loader (M-15, 2026-10-10): a mismatch in any field
//! (magic, fingerprint, K, iteration, key map, regret cells, strategy
//! sums) fails here before it can corrupt a resumption.

use pkr_cfr::table::CompactRegretTable;
use pkr_core::abstraction::{AbstractionFingerprint, NUM_ACTION_BUCKETS};

/// Seed each hash with a per-hand, per-action strategy mass through the
/// public API (`add_strategy_sum`). Uses `get_or_create` semantics via
/// the public `add_strategy_sum` wrapper.
fn seed(table: &CompactRegretTable, hashes: &[u64]) {
    for (i, &h) in hashes.iter().enumerate() {
        for a in 0..NUM_ACTION_BUCKETS {
            table.add_strategy_sum(h, a, ((i + 1) * (a + 1)) as f32 * 0.01);
        }
    }
}

#[test]
fn save_load_roundtrip_preserves_cells() {
    let fp = AbstractionFingerprint::from_constants(200);
    let src = CompactRegretTable::with_capacity(4096);
    let hashes: Vec<u64> = (0u64..100)
        .map(|i| 0xDEAD_0000_0000_0000u64.wrapping_add(i.wrapping_mul(0x0101_0101_0101_0101)))
        .collect();
    seed(&src, &hashes);

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ckpt.bin");
    src.save_checkpoint(path.to_str().unwrap(), 12_345, &fp).unwrap();

    let dst = CompactRegretTable::with_capacity(4096);
    let iter = dst.load_checkpoint(path.to_str().unwrap(), &fp).unwrap();
    assert_eq!(iter, 12_345, "iteration roundtrips");

    for &h in &hashes {
        let mut a = [0.0f32; NUM_ACTION_BUCKETS];
        let mut b = [0.0f32; NUM_ACTION_BUCKETS];
        src.get_average_strategy_into(h, &mut a);
        dst.get_average_strategy_into(h, &mut b);
        for k in 0..NUM_ACTION_BUCKETS {
            assert!(
                (a[k] - b[k]).abs() < 1e-6,
                "hash {h:#x} action {k}: src={} dst={}",
                a[k], b[k]
            );
        }
    }
}

#[test]
fn load_rejects_fingerprint_mismatch() {
    let fp = AbstractionFingerprint::from_constants(200);
    let src = CompactRegretTable::with_capacity(4096);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ckpt.bin");
    src.save_checkpoint(path.to_str().unwrap(), 1, &fp).unwrap();

    let fp2 = AbstractionFingerprint::from_constants(300);
    let dst = CompactRegretTable::with_capacity(4096);
    let err = dst.load_checkpoint(path.to_str().unwrap(), &fp2).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
}

#[test]
fn load_rejects_truncated_file() {
    let fp = AbstractionFingerprint::from_constants(200);
    let src = CompactRegretTable::with_capacity(4096);
    let hashes: Vec<u64> = (0u64..50).map(|i| 0xBEEF_0000u64.wrapping_add(i)).collect();
    seed(&src, &hashes);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ckpt.bin");
    src.save_checkpoint(path.to_str().unwrap(), 7, &fp).unwrap();

    let bytes = std::fs::read(&path).unwrap();
    std::fs::write(&path, &bytes[..bytes.len() / 2]).unwrap();

    let dst = CompactRegretTable::with_capacity(4096);
    let err = dst.load_checkpoint(path.to_str().unwrap(), &fp).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::UnexpectedEof);
}
