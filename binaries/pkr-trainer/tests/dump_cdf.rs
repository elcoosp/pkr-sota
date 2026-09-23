// Diagnostic: dump CDFs for canonical facing-all-in infosets.
// Run:
//   PKR_BLUEPRINT=outputs/v12/blueprint_10000000.bin \
//   PKR_ABS_DIR=outputs/v9 \
//   cargo test --release -p pkr-trainer --test dump_cdf -- --ignored --nocapture
//
// Bucket layout (post-T0.2):
//   0=fold  1=call  2=0.4x  3=0.8x  4=1.6x  5=all-in

use pkr_abstraction::{load_centroids, KMeansAbstraction};
use pkr_contracts::AbstractionBuilder;
use pkr_eval::TableEvaluator;
use pkr_runtime::{MmapReader, SolverHandle};
use std::sync::Arc;

#[test]
#[ignore]
fn dump_cdf() {
    // cargo test's CWD is the package dir; resolve relative paths
    // against the workspace root (same pattern as eval_harness.rs).
    let workspace_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("pkr-trainer has two parents")
        .to_path_buf();
    let resolve = |p: String| -> String {
        let pb = std::path::PathBuf::from(&p);
        if pb.is_absolute() {
            pb.to_string_lossy().into_owned()
        } else {
            workspace_root.join(pb).to_string_lossy().into_owned()
        }
    };

    let bp = resolve(std::env::var("PKR_BLUEPRINT").expect("PKR_BLUEPRINT"));
    let abs_dir = resolve(std::env::var("PKR_ABS_DIR").expect("PKR_ABS_DIR"));

    let evaluator =
        Arc::new(TableEvaluator::new(format!("{}/hand_ranks.bin", abs_dir)).expect("hand_ranks"));
    let store = load_centroids(&format!("{}/centroids.bin", abs_dir)).expect("centroids");
    let abstraction = KMeansAbstraction::from_store(store, evaluator.clone());
    abstraction
        .init_table(0, &format!("{}/preflop_abstraction.bin", abs_dir))
        .ok();
    abstraction
        .init_table(1, &format!("{}/abstraction.bin", abs_dir))
        .ok();
    abstraction
        .init_table(2, &format!("{}/turn_abstraction.bin", abs_dir))
        .ok();
    abstraction
        .init_table(3, &format!("{}/river_buckets.bin", abs_dir))
        .ok();

    let reader = MmapReader::new(&bp).expect("open blueprint");
    let handle = SolverHandle::new(reader);

    // Card: rank = c >> 2, suit = c & 3. A♠=48, K♠=44, Q♠=40, ... 2♠=0.
    let cases: &[(&str, [u8; 2])] = &[
        ("AA", [48, 49]),
        ("KK", [44, 45]),
        ("QQ", [40, 41]),
        ("JJ", [36, 37]),
        ("TT", [32, 33]),
        ("99", [28, 29]),
        ("88", [24, 25]),
        ("77", [20, 21]),
        ("66", [16, 17]),
        ("55", [12, 13]),
        ("44", [8, 9]),
        ("33", [4, 5]),
        ("22", [0, 1]),
        ("AKs", [48, 44]),
        ("AQo", [48, 42]),
        ("KQo", [44, 40]),
        ("T9s", [32, 28]),
        ("54s", [12, 8]),
        ("72o", [20, 2]),
    ];

    // History signatures (u32 = actions | raises<<8 | last_bet<<16):
    //   limp → jam:  actions=2, raises=1, last_bet=1  →  0x00010102
    //   raise → jam: actions=2, raises=2, last_bet=1  →  0x00010202
    let scenarios: &[(&str, u32)] = &[
        (
            "preflop limp→jam (facing 200bb jam)",
            2 | (1 << 8) | (1 << 16),
        ),
        (
            "preflop raise→jam (facing 3bet jam)",
            2 | (2 << 8) | (1 << 16),
        ),
    ];

    println!("### blueprint: {}", bp);
    for (name, sig) in scenarios {
        println!("");
        println!("### {}   sig=0x{:08x}", name, sig);
        let hist = sig.to_le_bytes();
        println!(
            "  {:<5} {:>7} {:>7} {:>7} {:>7} {:>7} {:>7}   argmax",
            "hand", "fold", "call", "0.4x", "0.8x", "1.6x", "jam"
        );
        for (hname, hole) in cases {
            let hash = abstraction.get_infoset_hash(hole, &[], &hist, 0);
            match handle.get_advice_fast(hash) {
                Some(advice) => {
                    let n = advice.len as usize;
                    let mut probs = [0.0f32; 6];
                    let mut prev = 0u16;
                    for i in 0..n.min(6) {
                        let c = advice.cdf_probabilities[i] as u16;
                        probs[i] = (c.saturating_sub(prev)) as f32 / 255.0;
                        prev = c;
                    }
                    let (am, _) = probs
                        .iter()
                        .enumerate()
                        .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
                        .unwrap();
                    let cells: Vec<String> = probs.iter().map(|p| format!("{:>7.3}", p)).collect();
                    println!("  {:<5} {}   {}", hname, cells.join(" "), am);
                }
                None => println!("  {:<5} MISS", hname),
            }
        }
    }
}
