use pkr_core::state::{Action, ActionKind, GameState};

fn main() {
    let s = GameState::new(200.0, 1.0, 2.0);
    println!("=== preflop root ===");
    println!("  actor:        {}", s.actor);
    println!("  street:       {:?}", s.street);
    println!("  stacks:       {:?}", s.stacks);
    println!("  street_bets:  {:?}", s.street_bets);
    println!("  pot:          {}", s.pot);
    println!();
    let mut buf = [Action { player: 0, kind: ActionKind::Fold }; 8];
    let n = s.legal_actions_into(&mut buf);
    println!("  {} legal actions:", n);
    for i in 0..n {
        let a = &buf[i];
        let b = pkr_core::abstraction::action_bucket(
            &a.kind, s.stacks[s.actor],
            s.street_bets[s.actor], s.street_bets[1 - s.actor], s.pot);
        println!("    [{}] {:?}  -> bucket {}", i, a.kind, b);
    }

    // Cluster-level strategy dump: for each cluster, print strategy at
    // the SB preflop root. We iterate 0..200 (the cluster count) and
    // construct the hash via get_infoset_hash with a synthetic hole whose
    // preflop table index maps to that cluster.
    println!();
    println!("=== cluster -> bucket strategy (SB preflop root) ===");
    use pkr_abstraction::{load_centroids, KMeansAbstraction, save_centroids};
    use pkr_contracts::AbstractionBuilder;
    use std::sync::Arc;
    let store = load_centroids("outputs/v34long/centroids.bin").expect("centroids");
    let abs = KMeansAbstraction::from_store(store, Arc::new(pkr_eval::NlheEvaluator));
    abs.init_table(0, "outputs/v34long/preflop_abstraction.bin").expect("table");

    // We just want the signature hash for the SB preflop root.
    let mut sig = [0u8; 8];
    let sl = s.infoset_signature_into(&mut sig);
    let hist = &sig[..sl];
    let board: &[u8] = &[];
    let street = 0u8;

    // Load the checkpoint via a fresh trainer.
    let abs_arc: Arc<dyn AbstractionBuilder> = Arc::new(abs);
    let trainer = pkr_cfr::Trainer::with_capacity(abs_arc.clone(), Arc::new(pkr_eval::NlheEvaluator), 60_000_000);
    let fp = pkr_core::abstraction::AbstractionFingerprint::from_constants(200);
    trainer.load_checkpoint("outputs/v34long/train.ckpt", &fp).expect("ckpt");

    let table_bytes = std::fs::read("outputs/v34long/preflop_abstraction.bin").unwrap();

    // For each cluster id 0..200, find one hand that lands there.
    let mut cluster_example: [Option<[u8; 2]>; 256] = [None; 256];
    for c0 in 0u8..52 {
        for c1 in (c0 + 1)..52 {
            let hole = [c0, c1];
            let mut hh = [hole[0], hole[1]];
            if hh[0] < hh[1] { hh.swap(0, 1); }
            use pkr_eval::lookup::choose;
            let idx = (choose(hh[0] as u32, 2) + choose(hh[1] as u32, 1)) as usize;
            let cluster = table_bytes[idx] as usize;
            if cluster_example[cluster].is_none() {
                cluster_example[cluster] = Some(hole);
            }
        }
    }

    // Print fold/call/raise for each cluster.
    println!("{:>8}  {:>8}  {:>8}  {:>8}  {}", "cluster", "fold", "call", "raise", "example");
    for cid in 0..200usize {
        if let Some(hole) = cluster_example[cid] {
            let hash = abs_arc.get_infoset_hash(&hole, board, hist, street);
            let mut strat = [0.0f32; 6];
            trainer.get_table().get_average_strategy_into(hash, &mut strat);
            // bucket semantics: 0=Fold, 1=Call, 2..5=raises/all-in
            let fold = strat[0];
            let call = strat[1];
            let raise: f32 = strat[2..].iter().sum();
            // Only print clusters that would show interesting behavior
            if fold > 0.5 || (call > 0.5 && cid == 46) || cid < 5 {
                println!("{:>8}  {:>8.4}  {:>8.4}  {:>8.4}  {:?}", cid, fold, call, raise, hole);
            }
        }
    }
    println!();
    println!("  (only printing clusters with fold>0.5, plus cid 46)");
}
