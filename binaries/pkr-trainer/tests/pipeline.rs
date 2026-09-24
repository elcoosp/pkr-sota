use pkr_cfr::Trainer;
use pkr_contracts::{AbstractionBuilder, Evaluator};
use pkr_export::writer::write_blueprint;
use pkr_runtime::{MmapReader, SolverHandle};
use std::sync::Arc;

/// Deterministic evaluator: same input -> same output, lower is better.
/// Not poker-correct; used only to exercise the full data path.
struct MockEvaluator;
impl Evaluator for MockEvaluator {
    fn evaluate_hand(&self, hole: &[u8], board: &[u8]) -> u32 {
        let mut h: u32 = 2166136261;
        for &c in hole.iter().chain(board) {
            h ^= c as u32;
            h = h.wrapping_mul(16777619);
        }
        h
    }
}

/// Trivial abstraction: hash cards + history directly. No EHS, no centroids.
/// Proves that Trainer -> write_blueprint -> MmapReader -> SolverHandle works
/// regardless of how the abstraction is built.
struct MockAbstraction;
impl AbstractionBuilder for MockAbstraction {
    fn get_infoset_hash(&self, hole: &[u8], board: &[u8], history: &[u8], street: u8) -> u64 {
        let mut h: u64 = 1469598103934665603;
        for &c in hole.iter().chain(board).chain(history) {
            h ^= c as u64;
            h = h.wrapping_mul(1099511628211);
        }
        h ^= street as u64;
        h
    }
}

#[test]
fn full_pipeline_trains_exports_loads_queries() {
    let tmp = tempfile::NamedTempFile::new().unwrap();

    let abstraction: Arc<dyn AbstractionBuilder> = Arc::new(MockAbstraction);
    let evaluator: Arc<dyn Evaluator> = Arc::new(MockEvaluator);
    let mut trainer = Trainer::with_capacity(abstraction, evaluator, 4096);

    for _ in 0..40 {
        trainer.run_iteration_parallel();
    }

    let mut keys = trainer.get_table().get_keys();
    assert!(
        !keys.is_empty(),
        "trainer produced zero infosets — pipeline is broken at the traversal level"
    );
    keys.sort_unstable();

    let fp = pkr_core::abstraction::AbstractionFingerprint::from_constants(4);
    let _ = write_blueprint(
        tmp.path().to_str().unwrap(),
        trainer.get_table(),
        &keys,
        &fp,
    );

    let reader = MmapReader::new(tmp.path()).unwrap();
    assert_eq!(reader.file_header().infoset_count as usize, keys.len());

    let handle = SolverHandle::new(reader);
    for &k in &keys {
        let advice = handle
            .get_advice_fast(k)
            .unwrap_or_else(|| panic!("expected hit for key {k:#x}"));
        let probs = &advice.cdf_probabilities[..advice.len as usize];
        for j in 1..probs.len() {
            assert!(probs[j] >= probs[j - 1], "CDF must be monotonic: {probs:?}");
        }
        assert_eq!(*probs.last().unwrap(), 255, "CDF must end at 255");
    }

    assert!(
        handle.get_advice_fast(0xFFFF_FFFF_FFFF_FFFF).is_none(),
        "unsaved key must miss"
    );
}

/// Verifies that a blueprint file produced by the real CLI is loadable.
/// Skipped unless the PKR_BLUEPRINT env var points at a blueprint.
/// Run via `PKR_BLUEPRINT=path cargo test -p pkr-trainer --test pipeline -- --ignored`.
#[test]
#[ignore]
fn load_external_blueprint() {
    let path = match std::env::var("PKR_BLUEPRINT") {
        Ok(p) => p,
        Err(_) => {
            eprintln!("PKR_BLUEPRINT not set; skipping");
            return;
        }
    };
    let reader = MmapReader::new(&path).unwrap_or_else(|e| panic!("open {path}: {e}"));
    let header = *reader.file_header();
    assert_eq!(&header.magic, b"PKRSOTA1", "bad magic");
    assert!(
        header.infoset_count > 0,
        "blueprint has zero infosets — trainer produced nothing"
    );
    assert_eq!(
        reader.keys_data().len() as u64,
        header.infoset_count * 8,
        "key table size mismatch"
    );
    assert_eq!(
        reader.cdf_data().len() as u64,
        header.infoset_count * header.max_actions_k as u64,
        "cdf table size mismatch"
    );

    let handle = SolverHandle::new(reader);
    let k = header.max_actions_k as usize;
    let num = header.infoset_count as usize;
    let mut hits = 0usize;
    for i in 0..num.min(64) {
        let key = u64::from_le_bytes(handle_slice_key(&handle, i));
        if let Some(a) = handle.get_advice_fast(key) {
            hits += 1;
            let probs = &a.cdf_probabilities[..a.len as usize];
            assert!(probs.len() <= k);
            for j in 1..probs.len() {
                assert!(probs[j] >= probs[j - 1], "non-monotonic CDF");
            }
            assert_eq!(*probs.last().unwrap(), 255, "CDF must end at 255");
        }
    }
    assert!(hits > 0, "no keys from the file could be queried");
    eprintln!("Loaded {num} infosets, queried {hits} successfully");
}

/// Helper: read the i-th 8-byte key from the handle's key region.
/// Uses unsafe pointer arithmetic on the mmap'd data to avoid exposing
/// the raw MmapReader again after it has been moved into SolverHandle.
fn handle_slice_key(handle: &SolverHandle, i: usize) -> [u8; 8] {
    let data = handle.debug_keys();
    let start = i * 8;
    let mut out = [0u8; 8];
    out.copy_from_slice(&data[start..start + 8]);
    out
}

#[test]
fn checkpoint_roundtrip_preserves_table() {
    let tmp = tempfile::NamedTempFile::new().unwrap();

    let abstraction: Arc<dyn AbstractionBuilder> = Arc::new(MockAbstraction);
    let evaluator: Arc<dyn Evaluator> = Arc::new(MockEvaluator);
    let mut trainer = Trainer::with_capacity(abstraction, evaluator, 4096);

    for _ in 0..20 {
        trainer.run_iteration_parallel();
    }
    let pre_keys = trainer.get_table().get_keys();
    let pre_len = trainer.get_table().len();
    let pre_iter = trainer.iteration();

    let fp = pkr_core::abstraction::AbstractionFingerprint::from_constants(4);
    trainer
        .save_checkpoint(tmp.path().to_str().unwrap(), &fp)
        .unwrap();

    let abstraction2: Arc<dyn AbstractionBuilder> = Arc::new(MockAbstraction);
    let evaluator2: Arc<dyn Evaluator> = Arc::new(MockEvaluator);
    let trainer2 = Trainer::with_capacity(abstraction2, evaluator2, 4096);
    let fp2 = pkr_core::abstraction::AbstractionFingerprint::from_constants(4);
    trainer2
        .load_checkpoint(tmp.path().to_str().unwrap(), &fp2)
        .unwrap();

    assert_eq!(
        trainer2.get_table().len(),
        pre_len,
        "infoset count mismatch"
    );
    assert_eq!(trainer2.iteration(), pre_iter, "iteration mismatch");

    let mut a = pre_keys;
    let mut b = trainer2.get_table().get_keys();
    a.sort_unstable();
    b.sort_unstable();
    assert_eq!(a, b, "key sets differ after checkpoint roundtrip");
}

/// F2b: a checkpoint written under one fingerprint must refuse to load
/// under a different one.
#[test]
fn fingerprint_mismatch_aborts_load() {
    let tmp = tempfile::NamedTempFile::new().unwrap();

    let abstraction: Arc<dyn AbstractionBuilder> = Arc::new(MockAbstraction);
    let evaluator: Arc<dyn Evaluator> = Arc::new(MockEvaluator);
    let mut trainer = Trainer::with_capacity(abstraction, evaluator, 4096);
    for _ in 0..10 {
        trainer.run_iteration_parallel();
    }

    // Write with k=4.
    let fp_written = pkr_core::abstraction::AbstractionFingerprint::from_constants(4);
    trainer
        .save_checkpoint(tmp.path().to_str().unwrap(), &fp_written)
        .unwrap();

    // Try to load under a different k.
    let abstraction2: Arc<dyn AbstractionBuilder> = Arc::new(MockAbstraction);
    let evaluator2: Arc<dyn Evaluator> = Arc::new(MockEvaluator);
    let trainer2 = Trainer::with_capacity(abstraction2, evaluator2, 4096);
    let fp_loaded = pkr_core::abstraction::AbstractionFingerprint::from_constants(8);
    let err = trainer2
        .load_checkpoint(tmp.path().to_str().unwrap(), &fp_loaded)
        .unwrap_err();
    let msg = format!("{}", err);
    assert!(
        msg.contains("abstraction mismatch"),
        "expected mismatch error, got: {msg}"
    );
    assert!(
        msg.contains("preflop_k"),
        "error must name the diff, got: {msg}"
    );
}

/// F2c: a v4 blueprint stores the AbstractionFingerprint and the
/// reader exposes it (and can enforce a match).
#[test]
fn blueprint_v4_roundtrip_carries_fingerprint() {
    let tmp = tempfile::NamedTempFile::new().unwrap();

    let abstraction: Arc<dyn AbstractionBuilder> = Arc::new(MockAbstraction);
    let evaluator: Arc<dyn Evaluator> = Arc::new(MockEvaluator);
    let mut trainer = Trainer::with_capacity(abstraction, evaluator, 4096);
    for _ in 0..20 {
        trainer.run_iteration_parallel();
    }
    let mut keys = trainer.get_table().get_keys();
    keys.sort_unstable();

    let fp = pkr_core::abstraction::AbstractionFingerprint::from_constants(4);
    let _ = write_blueprint(
        tmp.path().to_str().unwrap(),
        trainer.get_table(),
        &keys,
        &fp,
    );

    let reader = MmapReader::new(tmp.path()).unwrap();
    assert_eq!(reader.file_header().version, 4, "writer must emit v4");

    let stored = reader.fingerprint().expect("v4 must carry a fingerprint");
    assert_eq!(stored, fp, "stored fingerprint must match written");

    // Matching current fingerprint: OK.
    reader
        .check_fingerprint(&fp)
        .expect("matching fingerprint must pass check");

    // Mismatched current fingerprint: error.
    let fp_mismatch = pkr_core::abstraction::AbstractionFingerprint::from_constants(8);
    let err = reader.check_fingerprint(&fp_mismatch).unwrap_err();
    let msg = format!("{}", err);
    assert!(
        msg.contains("abstraction mismatch"),
        "expected mismatch error, got: {msg}"
    );
    assert!(msg.contains("preflop_k"), "error must name the diff: {msg}");
}
