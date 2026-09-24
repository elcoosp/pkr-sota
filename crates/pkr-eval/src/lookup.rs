/// Binomial coefficient C(n,k), safe for 0≤k≤7. Uses u64 for intermediates.
/// Binomial coefficient `C(n, k)`, computed at compile time.
const fn choose_const(n: u32, k: u32) -> u32 {
    if k > n {
        return 0;
    }
    let k = if k > n - k { n - k } else { k };
    let mut result: u64 = 1;
    let mut i: u32 = 0;
    while i < k {
        result = result * (n - i) as u64 / (i + 1) as u64;
        i += 1;
    }
    result as u32
}

/// Precomputed binomial coefficients: `CHOOSE[n][k] == C(n, k)` for
/// n in 0..=51, k in 0..=5. 1.25 KB; one L1 load per (n, k).
/// Bit-identical to the multiply-based implementation.
static CHOOSE: [[u32; 8]; 52] = {
    let mut t = [[0u32; 8]; 52];
    let mut n: usize = 0;
    while n < 52 {
        let mut k: usize = 0;
        while k < 8 {
            t[n][k] = choose_const(n as u32, k as u32);
            k += 1;
        }
        n += 1;
    }
    t
};

/// Binomial coefficient `C(n, k)` as u32. Table lookup for the hot
/// range (n < 52, k < 6); pure-math fallback otherwise.
#[inline(always)]
pub fn choose(n: u32, k: u32) -> u32 {
    if n < 52 && k < 8 {
        CHOOSE[n as usize][k as usize]
    } else {
        choose_const(n, k)
    }
}

/// Combinadic rank of a 5-card combination sorted descending.
pub fn combinadic_rank(cards: &[u8; 5]) -> u32 {
    let c0 = cards[0] as u32;
    let c1 = cards[1] as u32;
    let c2 = cards[2] as u32;
    let c3 = cards[3] as u32;
    let c4 = cards[4] as u32;
    choose(c0, 5) + choose(c1, 4) + choose(c2, 3) + choose(c3, 2) + choose(c4, 1)
}

/// Combinadic rank of a 4-card combination sorted descending.
pub fn combinadic_rank_4(cards: &[u8; 4]) -> u32 {
    let c0 = cards[0] as u32;
    let c1 = cards[1] as u32;
    let c2 = cards[2] as u32;
    let c3 = cards[3] as u32;
    choose(c0, 4) + choose(c1, 3) + choose(c2, 2) + choose(c3, 1)
}
