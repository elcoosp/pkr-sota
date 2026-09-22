
/// Binomial coefficient C(n,k), safe for 0≤k≤7. Uses u64 for intermediates.
pub fn choose(n: u32, k: u32) -> u32 {
    if k > n { return 0; }
    let n = n as u64;
    let result: u64 = match k {
        0 => 1,
        1 => n,
        2 => n * (n - 1) / 2,
        3 => n * (n - 1) * (n - 2) / 6,
        4 => n * (n - 1) * (n - 2) * (n - 3) / 24,
        5 => n * (n - 1) * (n - 2) * (n - 3) * (n - 4) / 120,
        6 => n * (n - 1) * (n - 2) * (n - 3) * (n - 4) * (n - 5) / 720,
        7 => n * (n - 1) * (n - 2) * (n - 3) * (n - 4) * (n - 5) * (n - 6) / 5040,
        _ => panic!("k>7 unsupported"),
    };
    result as u32
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
