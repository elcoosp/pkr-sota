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
use rand::rngs::SmallRng;
use rand::SeedableRng;

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
        // Rebuild state: fresh deal, real hole, then replay the action
        // record AND advance the board when each street closes. Without
        // the street advance a "river" request resolved to a PREFLOP
        // decision (the board was discarded).
        let mut st = GameState::new(pkr_core::state::stack_chips(), 1.0, 2.0);
        let board_cards: Vec<u8> = v.get("board").and_then(|b| b.as_array())
            .map(|a| a.iter().filter_map(|x| x.as_u64()).map(|x| x as u8).collect())
            .unwrap_or_default();
        // The host passes the ACTING player's hole. It must be assigned to
        // the ACTING seat (st.actor), not always seat 0 - otherwise a
        // seat-1 decision hashes a placeholder instead of the real cards.
        let hero_hole: Option<[u8; 2]> = v.get("hole").and_then(|h| h.as_array())
            .and_then(|a| {
                let hv: Vec<u8> = a.iter().filter_map(|x| x.as_u64()).map(|x| x as u8).collect();
                if hv.len() == 2 { Some([hv[0], hv[1]]) } else { None }
            });
        st.set_hole_cards([40, 41], [40, 42]); // deterministic placeholders
        let mut dealt = 0usize;
        if let Some(arr) = v.get("actions").and_then(|a| a.as_array()) {
            for a in arr {
                if let Some(act) = action_from_json(a) {
                    st.apply_action_in_place(&act);
                    if st.is_street_complete() && st.street != Street::River {
                        let need = match st.street {
                            Street::Preflop => 3,
                            Street::Flop => 1,
                            Street::Turn => 1,
                            Street::River => 0,
                        };
                        if dealt + need <= board_cards.len() {
                            let chunk = board_cards[dealt..dealt + need].to_vec();
                            dealt += need;
                            st.advance_street_in_place(&chunk);
                        }
                    }
                }
            }
        }
        // If the record ended mid-street but the board says we are on a
        // later street, advance the remainder (host may omit auto-advance).
        while st.street != Street::River && st.is_street_complete() {
            let need = match st.street {
                Street::Preflop => 3,
                Street::Flop => 1,
                Street::Turn => 1,
                Street::River => 0,
            };
            if need == 0 || dealt + need > board_cards.len() { break; }
            let chunk = board_cards[dealt..dealt + need].to_vec();
            dealt += need;
            st.advance_street_in_place(&chunk);
        }
        // Assign the real hole to the ACTING seat now that the actor is known.
        if let Some(hh) = hero_hole {
            st.hole[st.actor] = hh;
        }
        // Decide: hash the acting player's infoset via the abstraction
        // loaded from the tables dir, then read the blueprint advice.
        if std::env::var("PKR_BOT_DEBUG").is_ok() {
            eprintln!("DEBUG actor={} hole={:?} board_len={} street={:?} face={}",
                st.actor, st.hole[st.actor], st.board_len, st.street, st.bet_to_call());
        }
        let hole = st.hole[st.actor];
        let mut sig = [0u8; 8];
        let sl = st.infoset_signature_into(&mut sig);
        let board = &st.board[..st.board_len as usize];
        let hash = abs.get_infoset_hash(&hole, board, &sig[..sl], st.street as u8);

        // CONCRETE DECISION: decode the blueprint CDF, map to legal
        // actions by bucket, sample one, emit the incr token.
        let mut buf = [Action { player: 0, kind: ActionKind::Fold }; 8];
        let n = st.legal_actions_into(&mut buf);
        let mut probs = [0.0f32; pkr_core::abstraction::NUM_ACTION_BUCKETS];
        if let Some(a) = solver.get_advice_fast(hash) {
            let mut prev = 0u16;
            let m = (a.len as usize).min(pkr_core::abstraction::NUM_ACTION_BUCKETS);
            for i in 0..m {
                let cc = a.cdf_probabilities[i] as u16;
                probs[i] = (cc.saturating_sub(prev)) as f32 / 255.0;
                prev = cc;
            }
        }
        let actor = st.actor;
        let mut b2a: [Option<usize>; pkr_core::abstraction::NUM_ACTION_BUCKETS] = [None; pkr_core::abstraction::NUM_ACTION_BUCKETS];
        for (i, act) in buf.iter().take(n).enumerate() {
            let b = pkr_core::abstraction::action_bucket(
                &act.kind, st.stacks[actor], st.street_bets[actor],
                st.street_bets[1 - actor], st.pot) as usize;
            if (b as usize) < pkr_core::abstraction::NUM_ACTION_BUCKETS && b2a[b].is_none() { b2a[b] = Some(i); }
        }
        let total: f32 = (0..pkr_core::abstraction::NUM_ACTION_BUCKETS).filter(|&b| b2a[b].is_some()).map(|b| probs[b]).sum();
        let chosen: Option<Action> = if n == 0 {
            None
        } else if total > 1e-9 {
            // Common random numbers: if the host passes a per-decision
            // "seed", use a deterministic RNG so paired runs draw the
            // same sample (A-vs-A must be exactly 0).
            let r: f32 = match v.get("seed").and_then(|s| s.as_u64()) {
                Some(sd) => SmallRng::seed_from_u64(sd).random_range(0.0..total),
                None => rand::rng().random_range(0.0..total),
            };
            let mut acc = 0.0f32;
            let mut pick = (0..pkr_core::abstraction::NUM_ACTION_BUCKETS).find(|&b| b2a[b].is_some()).unwrap_or(0);
            for b in 0..pkr_core::abstraction::NUM_ACTION_BUCKETS {
                if b2a[b].is_none() { continue; }
                acc += probs[b];
                if r <= acc { pick = b; break; }
            }
            b2a[pick].map(|i| buf[i])
        } else {
            (0..pkr_core::abstraction::NUM_ACTION_BUCKETS).find_map(|b| b2a[b].map(|i| buf[i]))
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
