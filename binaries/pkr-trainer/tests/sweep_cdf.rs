// Sweep call-vs-jam probability for trash hands across blueprints.
use pkr_abstraction::{load_centroids, KMeansAbstraction};
use pkr_contracts::AbstractionBuilder;
use pkr_eval::TableEvaluator;
use pkr_runtime::{MmapReader, SolverHandle};
use std::sync::Arc;

fn sweep(bp: &str, abs_dir: &str, sig: u32, label: &str) -> (f32, f32, f32) {
    let evaluator = Arc::new(
        TableEvaluator::new(format!("{}/hand_ranks.bin", abs_dir)).expect("hand_ranks"),
    );
    let store = load_centroids(&format!("{}/centroids.bin", abs_dir)).expect("centroids");
    let abstraction = KMeansAbstraction::from_store(store, evaluator.clone());
    abstraction.init_table(0, &format!("{}/preflop_abstraction.bin", abs_dir)).ok();

    let reader = MmapReader::new(bp).expect("open blueprint");
    let handle = SolverHandle::new(reader);

    let hist = sig.to_le_bytes();

    // Trash hands: 72o, 82o, 92o, 73o, 83o, 62o, T2o.
    let trash: &[[u8; 2]] = &[
        [20, 2], [24, 2], [28, 2], [20, 6], [24, 6], [16, 2], [32, 2],
    ];

    // Premium hands: AA, KK, QQ, JJ, TT, AKs.
    let premium: &[[u8; 2]] = &[
        [48, 49], [44, 45], [40, 41], [36, 37], [32, 33], [48, 44],
    ];

    let avg_call = |set: &[[u8; 2]]| -> f32 {
        let mut sum = 0.0f32;
        let mut n = 0.0f32;
        for hole in set {
            let hash = abstraction.get_infoset_hash(hole, &[], &hist, 0);
            if let Some(advice) = handle.get_advice_fast(hash) {
                let n_act = advice.len as usize;
                if n_act >= 2 {
                    let fold_byte = advice.cdf_probabilities[0] as f32 / 255.0;
                    let call_byte = advice.cdf_probabilities[1] as f32 / 255.0;
                    let call_prob = (call_byte - fold_byte).max(0.0);
                    sum += call_prob;
                    n += 1.0;
                }
            }
        }
        if n > 0.0 { sum / n } else { 0.0 }
    };

    let trash_call = avg_call(trash);
    let premium_call = avg_call(premium);
    let edge = premium_call - trash_call;

    println!(
        "  {:<14} trash_call={:.3}  premium_call={:.3}  edge={:+.3}",
        label, trash_call, premium_call, edge
    );
    (trash_call, premium_call, edge)
}

#[test]
#[ignore]
fn sweep_all_chunks() {
    let abs_dir = std::env::var("PKR_ABS_DIR").expect("PKR_ABS_DIR");
    let workspace_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent().and_then(|p| p.parent()).expect("two parents").to_path_buf();
    let abs_dir = workspace_root.join(abs_dir).to_string_lossy().into_owned();

    let sig: u32 = 2 | (1 << 8) | (1 << 16);  // limp→jam
    println!("scenario: preflop limp→jam  sig=0x{:08x}", sig);

    for chunk in [5_000_000u32, 10_000_000, 15_000_000, 20_000_000,
                  25_000_000, 30_000_000, 35_000_000, 40_000_000] {
        let bp = workspace_root
            .join(format!("outputs/v12/blueprint_{}.bin", chunk))
            .to_string_lossy().into_owned();
        if !std::path::Path::new(&bp).exists() {
            println!("  chunk {:>12}  MISSING", chunk);
            continue;
        }
        sweep(&bp, &abs_dir, sig, &format!("v12 @ {:>10}", chunk));
    }

    println!("");
    println!("v13 (sizings reverted, thresholds fixed):");
    for chunk in [5_000_000u32, 10_000_000, 15_000_000, 20_000_000] {
        let bp = workspace_root
            .join(format!("outputs/v13/blueprint_{}.bin", chunk))
            .to_string_lossy().into_owned();
        if !std::path::Path::new(&bp).exists() { continue; }
        sweep(&bp, &abs_dir, sig, &format!("v13 @ {:>10}", chunk));
    }

    println!("");
    println!("v9 baseline for comparison:");
    for chunk in [5_000_000u32, 10_000_000, 15_000_000, 20_000_000] {
        let bp = workspace_root
            .join(format!("outputs/v9/blueprint_{}.bin", chunk))
            .to_string_lossy().into_owned();
        if !std::path::Path::new(&bp).exists() {
            continue;
        }
        sweep(&bp, &abs_dir, sig, &format!("v9  @ {:>10}", chunk));
    }
}
