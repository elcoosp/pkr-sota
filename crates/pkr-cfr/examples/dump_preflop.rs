//! Dump the trained bot's preflop opening frequency for all 169 hand
//! classes. Loads a checkpoint, looks up each hand's open probability
//! from the preflop table, writes to CSV.
//!
//! Usage:
//!   cargo run --release -p pkr-cfr --example dump_preflop -- \
//!     <ckpt> <preflop_table> <centroids> <rank_table> [out.csv]

use pkr_abstraction::{load_centroids, KMeansAbstraction};
use pkr_cfr::table::CompactRegretTable;
use pkr_contracts::AbstractionBuilder;
use pkr_core::abstraction::AbstractionFingerprint;
use std::io::Write;
use std::sync::Arc;

const RANK_NAMES: &[u8; 13] = b"23456789TJQKA";

fn hand_name(r1: u8, r2: u8, suited: bool) -> String {
    let n1 = RANK_NAMES[r1 as usize] as char;
    let n2 = RANK_NAMES[r2 as usize] as char;
    if r1 == r2 {
        format!("{}{}", n1, n2)
    } else if suited {
        format!("{}{}s", n1, n2)
    } else {
        format!("{}{}o", n1, n2)
    }
}

/// Encoding: card = suit * 13 + rank, rank 0..12, suit 0..3.
fn to_card(rank: u8, suit: u8) -> u8 {
    suit * 13 + rank
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 5 {
        eprintln!("usage: {} <ckpt> <preflop_table> <centroids> <rank_table> [out.csv]", args[0]);
        std::process::exit(2);
    }
    let ckpt = &args[1];
    let preflop_table_path = &args[2];
    let centroids_path = &args[3];
    let rank_table_path = &args[4];
    let out_path = args.get(5).map(|s| s.as_str()).unwrap_or("preflop_dump.csv");

    // Build abstraction.
    let store = load_centroids(centroids_path)?;
    let abs = KMeansAbstraction::from_store(store, Arc::new(pkr_eval::NlheEvaluator));
    abs.init_table(0, preflop_table_path)?;
    let abs_arc: Arc<dyn AbstractionBuilder> = Arc::new(abs);

    // Load checkpoint.
    let table = CompactRegretTable::with_capacity(60_000_000);
    let fp = AbstractionFingerprint::from_constants(200);
    table.load_checkpoint(ckpt, &fp)?;

    // We need a fresh preflop GameState at the SB-to-act root.
    use pkr_core::state::GameState;
    let state = GameState::new(200.0, 1.0, 2.0);

    let mut sig_buf = [0u8; 8];
    let sig_len = state.infoset_signature_into(&mut sig_buf);
    let history = &sig_buf[..sig_len];
    let board: &[u8] = &[];
    let street = 0u8;

    let mut w = std::io::BufWriter::new(std::fs::File::create(out_path)?);
    writeln!(w, "hand,r1,r2,suited,fold,check,call,bet_small,bet_medium,bet_large,cluster")?;

    let mut rank_table_used = std::marker::PhantomData::<()>;
    let _ = rank_table_path;
    let _ = &mut rank_table_used;

    for r1 in (0u8..13).rev() {
        for r2 in (0u8..13).rev() {
            if r2 > r1 { continue; }
            let pair = r1 == r2;
            let variants: &[bool] = if pair { &[false] } else { &[true, false] };
            for &suited in variants {
                let hole = if pair {
                    [to_card(r1, 0), to_card(r1, 1)]
                } else if suited {
                    [to_card(r1, 0), to_card(r2, 0)]
                } else {
                    [to_card(r1, 0), to_card(r2, 1)]
                };

                let hash = abs_arc.get_infoset_hash(&hole, board, history, street);
                let mut strat = [0.0f32; 6];
                table.get_average_strategy_into(hash, &mut strat);

                // Look up the cluster id.
                let cluster = {
                    let idx = {
                        let (a, b) = if hole[0] > hole[1] { (hole[0], hole[1]) } else { (hole[1], hole[0]) };
                        let ca = pkr_eval::lookup::choose(a as u32, 2);
                        let cb = pkr_eval::lookup::choose(b as u32, 1);
                        ca + cb
                    } as usize;
                    let mut buf = [0u8; 1326];
                    let mut file = std::fs::File::open(preflop_table_path)?;
                    use std::io::Read;
                    file.read_exact(&mut buf)?;
                    buf[idx]
                };

                writeln!(
                    w,
                    "{},{},{},{},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{}",
                    hand_name(r1, r2, suited),
                    r1, r2, suited as u8,
                    strat[0], strat[1], strat[2], strat[3], strat[4], strat[5],
                    cluster,
                )?;
            }
        }
    }

    println!("wrote {} -> {}", args.len(), out_path);
    Ok(())
}
