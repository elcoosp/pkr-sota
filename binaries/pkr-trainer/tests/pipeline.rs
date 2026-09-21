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

    write_blueprint(tmp.path().to_str().unwrap(), trainer.get_table(), &keys);

    let reader = MmapReader::new(tmp.path()).unwrap();
    assert_eq!(reader.file_header().infoset_count as usize, keys.len());

    let handle = SolverHandle::new(reader);
    for &k in &keys {
        let advice = handle
            .get_advice_fast(k)
            .unwrap_or_else(|| panic!("expected hit for key {k:#x}"));
        let probs = &advice.cdf_probabilities[..advice.len as usize];
        for j in 1..probs.len() {
            assert!(
                probs[j] >= probs[j - 1],
                "CDF must be monotonic: {probs:?}"
            );
        }
        assert_eq!(*probs.last().unwrap(), 255, "CDF must end at 255");
    }

    assert!(
        handle.get_advice_fast(0xFFFF_FFFF_FFFF_FFFF).is_none(),
        "unsaved key must miss"
    );
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

    trainer.save_checkpoint(tmp.path().to_str().unwrap()).unwrap();

    let abstraction2: Arc<dyn AbstractionBuilder> = Arc::new(MockAbstraction);
    let evaluator2: Arc<dyn Evaluator> = Arc::new(MockEvaluator);
    let trainer2 = Trainer::with_capacity(abstraction2, evaluator2, 4096);
    trainer2.load_checkpoint(tmp.path().to_str().unwrap()).unwrap();

    assert_eq!(trainer2.get_table().len(), pre_len, "infoset count mismatch");
    assert_eq!(trainer2.iteration(), pre_iter, "iteration mismatch");

    let mut a = pre_keys;
    let mut b = trainer2.get_table().get_keys();
    a.sort_unstable();
    b.sort_unstable();
    assert_eq!(a, b, "key sets differ after checkpoint roundtrip");
}
