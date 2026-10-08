//! Cross-process V4-vs-V5 H2H: two `pkr-bot-pipe` subprocesses (one per
//! blueprint, different sig_version so they cannot share a process), with
//! this driver tracking the game via `GameState` and shelling decisions.
//!
//! Usage: v4v5_h2h <bp_a> <sig_a> <bp_b> <sig_b> <tables> <deals> <seed>
use pkr_core::state::{Action, ActionKind, GameState, Street};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

struct Bot {
    _child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl Bot {
    fn launch(bp: &str, sig: u8, tables: &str) -> Self {
        let mut c = Command::new("target/release/pkr-bot-pipe")
            .arg(bp).arg(tables).arg("--sig").arg(sig.to_string())
            .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null())
            .spawn().expect("spawn bot");
        let stdin = c.stdin.take().unwrap();
        let stdout = BufReader::new(c.stdout.take().unwrap());
        Bot { _child: c, stdin, stdout }
    }
    fn decide(&mut self, hole: [u8; 2], board: &[u8], actions: &[Action], actor: usize, seed: u64) -> String {
        let acts: Vec<_> = actions.iter().map(|a| {
            let (kind, amt) = match a.kind {
                ActionKind::Fold => ("fold", 0.0),
                ActionKind::Check => ("check", 0.0),
                ActionKind::Call => ("call", 0.0),
                ActionKind::Bet(t) => ("bet", t as f64),
            };
            serde_json::json!({"player": a.player, "kind": kind, "amt": amt})
        }).collect();
        let msg = serde_json::json!({
            "hole": [hole[0], hole[1]], "board": board,
            "actor": actor, "street": 0, "actions": acts, "seed": seed
        }).to_string();
        self.stdin.write_all(msg.as_bytes()).unwrap();
        self.stdin.write_all(b"\n").unwrap();
        self.stdin.flush().unwrap();
        let mut line = String::new();
        self.stdout.read_line(&mut line).unwrap();
        line.trim().to_string()
    }
}

fn token_to_kind(tok: &str) -> ActionKind {
    match tok.chars().next() {
        Some('f') => ActionKind::Fold,
        Some('k') => ActionKind::Check,
        Some('c') => ActionKind::Call,
        Some('b') => ActionKind::Bet(tok[1..].parse::<f32>().unwrap_or(0.0)),
        _ => ActionKind::Fold,
    }
}

fn deal(seed: u64) -> ([u8; 2], [u8; 2], [u8; 5]) {
    // splitmix64 decorrelation (same as tournament).
    let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut next = || {
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        (z ^ (z >> 31)) as u32
    };
    let mut deck: [u8; 52] = core::array::from_fn(|i| i as u8);
    for i in 0..9 {
        let j = i + (next() as usize) % (52 - i);
        deck.swap(i, j);
    }
    ([deck[0], deck[1]], [deck[2], deck[3]], [deck[4], deck[5], deck[6], deck[7], deck[8]])
}

fn mix(x: u64) -> u64 {
    let mut z = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

fn play(
    a: &mut Bot, b: &mut Bot,
    hero_hole: [u8; 2], vill_hole: [u8; 2], runout: &[u8; 5],
    hero_seat: usize, base_seed: u64,
) -> f32 {
    let mut st = GameState::new(pkr_core::state::stack_chips(), 1.0, 2.0);
    let (h0, h1) = if hero_seat == 0 { (hero_hole, vill_hole) } else { (vill_hole, hero_hole) };
    st.set_hole_cards(h0, h1);
    let mut actions: Vec<Action> = Vec::new();
    let mut board: Vec<u8> = Vec::new();
    let mut deck_idx = 0usize;
    let mut steps = 0;
    let mut decision_idx: u64 = 0;
    while !st.is_terminal() && steps < 80 {
        steps += 1;
        let actor = st.actor;
        // Common random numbers: decision K gets the same seed in BOTH
        // passes, so A-vs-A cancels exactly.
        let dseed = mix(base_seed.wrapping_add(decision_idx.wrapping_mul(0x9E37_79B9_7F4A_7C15)));
        decision_idx += 1;
        let bot = if (actor == 0) == (hero_seat == 0) { &mut *a } else { &mut *b };
        let tok = bot.decide(st.hole[actor], &board, &actions, actor, dseed);
        let act = Action { player: actor, kind: token_to_kind(&tok) };
        actions.push(act);
        st.apply_action_in_place(&act);
        if st.is_street_complete() && st.street != Street::River {
            let need = match st.street {
                Street::Preflop => 3, Street::Flop => 1, Street::Turn => 1, Street::River => 0,
            };
            if deck_idx + need <= runout.len() {
                let chunk = runout[deck_idx..deck_idx + need].to_vec();
                deck_idx += need;
                board.extend_from_slice(&chunk);
                st.advance_street_in_place(&chunk);
            }
        }
    }
    let p0 = if st.is_terminal() { st.terminal_payoff(0, &pkr_eval::NlheEvaluator) } else { 0.0 };
    if hero_seat == 0 { p0 } else { -p0 }
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (bp_a, sig_a, bp_b, sig_b, tables) = (&a[1], a[2].parse::<u8>().unwrap(), &a[3], a[4].parse::<u8>().unwrap(), &a[5]);
    let deals: u64 = a.get(6).and_then(|s| s.parse().ok()).unwrap_or(2000);
    let seed: u64 = a.get(7).and_then(|s| s.parse().ok()).unwrap_or(42);
    let mut bot_a = Bot::launch(bp_a, sig_a, tables);
    let mut bot_b = Bot::launch(bp_b, sig_b, tables);
    let mut diffs: Vec<f64> = Vec::new();
    for i in 0..deals {
        let (h, v, runout) = deal(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(i));
        let a0 = play(&mut bot_a, &mut bot_b, h, v, &runout, 0, seed.wrapping_add(i));
        let b0 = play(&mut bot_b, &mut bot_a, h, v, &runout, 0, seed.wrapping_add(i));
        diffs.push((a0 - b0) as f64);
    }
    let n = diffs.len() as f64;
    let mean = diffs.iter().sum::<f64>() / n;
    let var = diffs.iter().map(|d| (d - mean).powi(2)).sum::<f64>() / (n - 1.0);
    let se = (var / n).sqrt();
    println!("V4-vs-V5 H2H: deals={deals} seed={seed} diff(A-B)={mean:+.4} chips/deal SE={se:.4} t={:+.2}", mean / se);
}
