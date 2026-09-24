//! Audit F3 cross-check: shipped hand_ranks.bin must agree with the
//! slow (corrected) evaluator on two-pair ordering.
//!
//! Ignored by default. Run with:
//!   PKR_RANK_TABLE=outputs/v17/hand_ranks.bin \
//!     cargo test -p pkr-eval --test f3_table_crosscheck -- --ignored --nocapture

use pkr_contracts::Evaluator;

#[inline]
fn card(rank: u8, suit: u8) -> u8 {
    suit * 13 + rank
}

fn table_path() -> Option<String> {
    std::env::var("PKR_RANK_TABLE").ok()
}

#[test]
#[ignore = "requires PKR_RANK_TABLE"]
fn f3_kk_queens_ordering_in_shipped_table() {
    let path = match table_path() { Some(p) => p, None => { eprintln!("PKR_RANK_TABLE not set"); return; } };
    let table = pkr_eval::TableEvaluator::new(&path).expect("load table");
    let slow = pkr_eval::NlheEvaluator;

    let kk447: [u8; 5] = [card(11,0), card(11,1), card(2,0), card(2,1), card(5,0)];
    let qq229: [u8; 5] = [card(10,0), card(10,1), card(0,0), card(0,1), card(7,0)];

    let t_kk = table.evaluate_hand(&kk447, &[]);
    let t_qq = table.evaluate_hand(&qq229, &[]);
    let s_kk = slow.evaluate_hand(&kk447, &[]);
    let s_qq = slow.evaluate_hand(&qq229, &[]);

    assert!(t_kk < t_qq, "table: KK447 ({t_kk}) must rank better than QQ229 ({t_qq})");
    assert_eq!(t_kk, s_kk, "disagreement on KK447: table={t_kk} slow={s_kk}");
    assert_eq!(t_qq, s_qq, "disagreement on QQ229: table={t_qq} slow={s_qq}");
    eprintln!("PASS: KK447 t={t_kk} s={s_kk} | QQ229 t={t_qq} s={s_qq}");
}

#[test]
#[ignore = "requires PKR_RANK_TABLE"]
fn f3_10k_random_hands_agree() {
    let path = match table_path() { Some(p) => p, None => { eprintln!("PKR_RANK_TABLE not set"); return; } };
    let table = pkr_eval::TableEvaluator::new(&path).expect("load table");
    let slow = pkr_eval::NlheEvaluator;

    let mut seed: u64 = 0xF3F3_F3F3_F3F3_F3F3;
    let mut next = || -> u8 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((seed >> 33) & 0xFF) as u8
    };

    let mut mismatches = 0u32;
    for i in 0..10_000 {
        let mut used = [false; 52];
        let mut cards = [0u8; 5];
        let mut n = 0usize;
        while n < 5 {
            let c = (next() % 52) as usize;
            if !used[c] { used[c] = true; cards[n] = c as u8; n += 1; }
        }
        let t = table.evaluate_hand(&cards, &[]);
        let s = slow.evaluate_hand(&cards, &[]);
        if t != s {
            mismatches += 1;
            if mismatches <= 3 { eprintln!("mismatch {i}: {cards:?} t={t} s={s}"); }
        }
    }
    assert_eq!(mismatches, 0, "{mismatches} mismatches / 10k");
    eprintln!("PASS: 10k random hands agree");
}
