//! `pkr-bot-pipe` (Task 1.3): line-protocol bot for Slumbot / arena.
//!
//! Reads one JSON object per line on stdin, writes one incr token per
//! line on stdout. Uses `RuntimeSession` (blueprint + shadow + tracker)
//! so Slumbot, arena, tournament and LBR can share this decision path.
//!
//! Input line:  {"hole":[c,c],"board":[..],"actor":0|1,
//!               "street":0..3,"actions":[{"player":p,"kind":"check"|"call"|"fold"|"bet","amt":f}]}
//! Output line: incr token ("k" | "c" | "f" | "b<total_chips>")
//!
//! Usage: pkr-bot-pipe <blueprint.bin> <tables_dir> [--sig N]

use pkr_runtime::{MmapReader, SolverHandle};
use pkr_contracts::AbstractionBuilder;
use pkr_core::state::{Action, ActionKind, GameState, Street};
use std::io::{BufRead, Write};
use rand::RngExt;

fn action_from_json(v: &serde_json::Value) -> Option<Action> {
    let player = v.get("player")?.as_u64()? as usize;
    let kind = match v.get("kind")?.as_str()? {
        "fold" => ActionKind::Fold,
        "check" => ActionKind::Check,
        "call" => ActionKind::Call,
        "bet" => ActionKind::Bet(v.get("amt")?.as_f64()? as f32),
        _ => return None,
    };
    Some(Action { player, kind })
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: {} <blueprint.bin> <tables_dir> [--sig N]", args[0]);
        std::process::exit(2);
    }
    let bp = &args[1];
    // tables dir + sig read but the solver path is wired in the host app;
    // this bin establishes the protocol and the shared decision entry.
    let _tables = &args[2];
    let sig: u8 = args.iter().position(|a| a == "--sig")
        .and_then(|i| args.get(i + 1)).and_then(|s| s.parse().ok()).unwrap_or(3);
    // Apply BEFORE any signature / fingerprint call (OnceLocks).
    // Before this fix the flag was parsed into `_sig` and discarded: a
    // user passing --sig 4 silently hashed with the default (V3) and
    // saw all-zero blueprint hits. The flag is now load-bearing.
    match sig {
        5 => {
            std::env::set_var("PKR_SIG_V5", "1");
            std::env::set_var("PKR_SIG_V4", "0");
            std::env::set_var("PKR_SIG_V3", "1");
        }
        4 => {
            std::env::set_var("PKR_SIG_V4", "1");
            std::env::set_var("PKR_SIG_V5", "0");
            std::env::set_var("PKR_SIG_V3", "1");
        }
        3 | 1 => {
            std::env::set_var("PKR_SIG_V3", "1");
            std::env::set_var("PKR_SIG_V4", "0");
            std::env::set_var("PKR_SIG_V5", "0");
        }
        _ => {
            std::env::set_var("PKR_SIG_V3", "0");
            std::env::set_var("PKR_SIG_V4", "0");
            std::env::set_var("PKR_SIG_V5", "0");
        }
    }

    let reader = match MmapReader::new(bp) {
        Ok(r) => r,
        Err(e) => { eprintln!("load blueprint: {e:?}"); std::process::exit(1); }
    };
    let solver = SolverHandle::new(reader);
    // Load the abstraction tables (needed to hash infosets).
    let store = pkr_abstraction::load_centroids(
        &format!("{}/centroids.bin", args[2])).unwrap_or_else(|e| {
            eprintln!("centroids: {e}"); std::process::exit(1); });
    let abs = pkr_abstraction::KMeansAbstraction::from_store(
        store, std::sync::Arc::new(pkr_eval::NlheEvaluator));
    for (i, f) in [(0u8, "preflop_abstraction.bin"), (1, "abstraction.bin"),
                   (2, "turn_abstraction.bin"), (3, "river_buckets.bin")] {
        abs.init_table(i, &format!("{}/{}", args[2], f)).unwrap_or_else(|e| {
            eprintln!("init table {f}: {e:?}"); std::process::exit(1); });
    }
    eprintln!("pkr-bot-pipe ready: {bp} (tables {})", args[2]);

    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = match line { Ok(l) => l, Err(_) => break };
        if line.trim().is_empty() { continue; }
        let v: serde_json::Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => { eprintln!("bad json: {e}"); continue; }
        };
        // Rebuild state.
        let mut st = GameState::new(pkr_core::state::stack_chips(), 1.0, 2.0);
        if let (Some(h), Some(b)) = (v.get("hole"), v.get("board")) {
            let hole: Vec<u8> = h.as_array().unwrap().iter()
                .map(|x| x.as_u64().unwrap() as u8).collect();
            let board: Vec<u8> = b.as_array().unwrap().iter()
                .map(|x| x.as_u64().unwrap() as u8).collect();
            if hole.len() == 2 {
                // set_hole_cards needs both seats; leave villain 0 for now,
                // the host supplies real holes via the fuller protocol.
                st.set_hole_cards([hole[0], hole[1]], [0, 0]);
            }
            let _ = board;
        }
        // Apply the action record.
        if let Some(arr) = v.get("actions").and_then(|a| a.as_array()) {
            for a in arr {
                if let Some(act) = action_from_json(a) {
                    st.apply_action_in_place(&act);
                }
            }
        }
        // Decide: hash the acting player's infoset via the abstraction
        // loaded from the tables dir, then read the blueprint advice.
        let hole = st.hole[st.actor];
        let mut sig = [0u8; 8];
        let sl = st.infoset_signature_into(&mut sig);
        let board = &st.board[..st.board_len as usize];
        let hash = abs.get_infoset_hash(&hole, board, &sig[..sl], st.street as u8);

        // CONCRETE DECISION: decode the blueprint CDF, map to legal
        // actions by bucket, sample one, emit the incr token.
        let mut buf = [Action { player: 0, kind: ActionKind::Fold }; 8];
        let n = st.legal_actions_into(&mut buf);
        let mut probs = [0.0f32; 6];
        if let Some(a) = solver.get_advice_fast(hash) {
            let mut prev = 0u16;
            let m = (a.len as usize).min(6);
            for i in 0..m {
                let cc = a.cdf_probabilities[i] as u16;
                probs[i] = (cc.saturating_sub(prev)) as f32 / 255.0;
                prev = cc;
            }
        }
        let actor = st.actor;
        let mut b2a: [Option<usize>; 6] = [None; 6];
        for (i, act) in buf.iter().take(n).enumerate() {
            let b = pkr_core::abstraction::action_bucket(
                &act.kind, st.stacks[actor], st.street_bets[actor],
                st.street_bets[1 - actor], st.pot) as usize;
            if b < 6 && b2a[b].is_none() { b2a[b] = Some(i); }
        }
        let total: f32 = (0..6).filter(|&b| b2a[b].is_some()).map(|b| probs[b]).sum();
        let chosen: Option<Action> = if n == 0 {
            None
        } else if total > 1e-9 {
            let r: f32 = rand::rng().random_range(0.0..total);
            let mut acc = 0.0f32;
            let mut pick = (0..6).find(|&b| b2a[b].is_some()).unwrap_or(0);
            for b in 0..6 {
                if b2a[b].is_none() { continue; }
                acc += probs[b];
                if r <= acc { pick = b; break; }
            }
            b2a[pick].map(|i| buf[i])
        } else {
            (0..6).find_map(|b| b2a[b].map(|i| buf[i]))
        };
        let token = match chosen.map(|a| a.kind) {
            Some(ActionKind::Fold) => "f".to_string(),
            Some(ActionKind::Check) => "k".to_string(),
            Some(ActionKind::Call) => "c".to_string(),
            Some(ActionKind::Bet(t)) => format!("b{}", t as i64),
            None => "f".to_string(),
        };
        let _ = writeln!(out, "{token}");
        let _ = out.flush();
    }
}
