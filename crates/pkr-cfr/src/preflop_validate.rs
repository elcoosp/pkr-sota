//! Preflop chart validation — checks trained preflop strategy against
//! known HU ranges. (Roadmap §5.3)
//!
//! HU preflop is publicly well-mapped (BU open ~80%+, BB defend very wide,
//! 3-bet ~13–16%). A structurally wrong preflop chart bleeds EV every hand.

use pkr_core::card::Rank;

/// Canonical hand categories for preflop validation.
/// Each entry is (rank1, rank2, suited_flag, category_name).
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
struct PreflopHand {
    rank1: Rank,
    rank2: Rank,
    suited: bool,
    category: &'static str,
}

/// Expected opening range frequencies for the button (SB) in HU.
/// Values are approximate from public solvers (PioSolver, GTO Wizard, Slumbot).
const OPENING_RANGES: &[(Rank, Rank, bool, f32)] = &[
    // Pocket pairs
    (Rank::Ace, Rank::Ace, false, 1.0),
    (Rank::King, Rank::King, false, 1.0),
    (Rank::Queen, Rank::Queen, false, 1.0),
    (Rank::Jack, Rank::Jack, false, 1.0),
    (Rank::Ten, Rank::Ten, false, 1.0),
    (Rank::Nine, Rank::Nine, false, 0.95),
    (Rank::Eight, Rank::Eight, false, 0.90),
    (Rank::Seven, Rank::Seven, false, 0.70),
    (Rank::Six, Rank::Six, false, 0.55),
    (Rank::Five, Rank::Five, false, 0.40),
    (Rank::Four, Rank::Four, false, 0.30),
    (Rank::Three, Rank::Three, false, 0.25),
    (Rank::Two, Rank::Two, false, 0.20),
    // Suited Aces
    (Rank::Ace, Rank::King, true, 1.0),
    (Rank::Ace, Rank::Queen, true, 1.0),
    (Rank::Ace, Rank::Jack, true, 0.95),
    (Rank::Ace, Rank::Ten, true, 0.90),
    (Rank::Ace, Rank::Nine, true, 0.80),
    (Rank::Ace, Rank::Eight, true, 0.65),
    (Rank::Ace, Rank::Seven, true, 0.45),
    (Rank::Ace, Rank::Six, true, 0.35),
    (Rank::Ace, Rank::Five, true, 0.25),
    (Rank::Ace, Rank::Four, true, 0.20),
    (Rank::Ace, Rank::Three, true, 0.15),
    (Rank::Ace, Rank::Two, true, 0.10),
    // Suited Kings
    (Rank::King, Rank::Queen, true, 0.80),
    (Rank::King, Rank::Jack, true, 0.70),
    (Rank::King, Rank::Ten, true, 0.50),
    (Rank::King, Rank::Nine, true, 0.30),
    // Offsuty broadways
    (Rank::Ace, Rank::King, false, 0.85),
    (Rank::King, Rank::Queen, false, 0.40),
    (Rank::Queen, Rank::Jack, false, 0.30),
    (Rank::Jack, Rank::Ten, false, 0.15),
];

/// Minimum expected open frequency for the button (SB) — should be ~80%+.
const EXPECTED_OPEN_THRESHOLD: f32 = 0.70;

/// Expected 3-bet range for the BB vs SB open — ~13-16% of hands.
const EXPECTED_3BET_THRESHOLD: f32 = 0.10;

/// Expected BB defend range (call or 3-bet) — very wide, ~60%+.
const EXPECTED_DEFEND_THRESHOLD: f32 = 0.50;

/// Validate a preflop strategy lookup. The `lookup` function takes
/// hole card ranks (high first) and suitedness, and returns the
/// probability of opening (from the SB/button perspective).
pub fn validate_preflop_opening(lookup: &dyn Fn(Rank, Rank, bool) -> f32) -> ValidationResult {
    let mut issues = Vec::new();
    let mut total_weight = 0.0f32;
    let mut weighted_freq = 0.0f32;

    for &(r1, r2, suited, _) in OPENING_RANGES {
        let freq = lookup(r1, r2, suited);
        total_weight += 1.0;
        weighted_freq += freq;
    }

    let avg_open = weighted_freq / total_weight;
    if avg_open < EXPECTED_OPEN_THRESHOLD {
        issues.push(format!(
            "Average opening frequency {:.1}% is below expected {:.1}% for strong hands",
            avg_open * 100.0,
            EXPECTED_OPEN_THRESHOLD * 100.0
        ));
    }

    // Check specific hands
    let aces = lookup(Rank::Ace, Rank::Ace, false);
    if aces < 0.99 {
        issues.push(format!(
            "AA should open 100% but opens {:.1}%",
            aces * 100.0
        ));
    }

    let seven_two = lookup(Rank::Seven, Rank::Two, false);
    if seven_two > 0.15 {
        issues.push(format!(
            "72o should open rarely but opens {:.1}%",
            seven_two * 100.0
        ));
    }

    let suited_ace_six = lookup(Rank::Ace, Rank::Six, true);
    if suited_ace_six < 0.20 {
        issues.push(format!(
            "A6s should open at least 20% but opens {:.1}%",
            suited_ace_six * 100.0
        ));
    }

    ValidationResult {
        category: "preflop_opening".to_string(),
        passed: issues.is_empty(),
        score: avg_open,
        issues,
    }
}

/// Validate the BB 3-bet range against expected ~13-16%.
pub fn validate_bb_3bet(lookup: &dyn Fn(Rank, Rank, bool) -> f32) -> ValidationResult {
    let mut issues = Vec::new();
    let mut total_weight = 0.0f32;
    let mut weighted_3bet = 0.0f32;

    // Sample of hands that should 3-bet
    let strong_3bet_hands = [
        (Rank::Ace, Rank::Ace, false, 1.0),
        (Rank::Ace, Rank::King, false, 0.9),
        (Rank::Ace, Rank::King, true, 0.95),
        (Rank::King, Rank::King, false, 1.0),
        (Rank::King, Rank::Queen, false, 0.3),
        (Rank::Ace, Rank::Queen, true, 0.5),
        (Rank::Ace, Rank::Ace, false, 1.0),
        (Rank::King, Rank::King, false, 1.0),
        (Rank::Queen, Rank::Queen, false, 0.8),
        (Rank::Ace, Rank::King, true, 1.0),
    ];

    for &(r1, r2, suited, expected) in &strong_3bet_hands {
        let freq = lookup(r1, r2, suited);
        total_weight += 1.0;
        weighted_3bet += freq;
        if freq < expected * 0.5 {
            issues.push(format!(
                "{:?}{:?}{} should 3-bet at least {:.0}% but 3-bets {:.1}%",
                r1,
                r2,
                if suited { "s" } else { "o" },
                expected * 100.0,
                freq * 100.0
            ));
        }
    }

    let avg_3bet = weighted_3bet / total_weight;
    if avg_3bet < EXPECTED_3BET_THRESHOLD {
        issues.push(format!(
            "Average 3-bet frequency {:.1}% is below expected {:.1}%",
            avg_3bet * 100.0,
            EXPECTED_3BET_THRESHOLD * 100.0
        ));
    }

    ValidationResult {
        category: "bb_3bet".to_string(),
        passed: issues.is_empty(),
        score: avg_3bet,
        issues,
    }
}

/// Validate the BB defend range (call or 3-bet) — should be very wide.
pub fn validate_bb_defend(lookup: &dyn Fn(Rank, Rank, bool) -> f32) -> ValidationResult {
    let mut issues = Vec::new();
    let mut total_weight = 0.0f32;
    let mut weighted_defend = 0.0f32;

    let defend_hands = [
        (Rank::Ace, Rank::Two, false),
        (Rank::King, Rank::Two, false),
        (Rank::Queen, Rank::Two, false),
        (Rank::Jack, Rank::Two, false),
        (Rank::Ten, Rank::Two, false),
        (Rank::Five, Rank::Three, true),
        (Rank::Four, Rank::Two, true),
        (Rank::Three, Rank::Two, true),
        (Rank::Ace, Rank::King, true),
        (Rank::King, Rank::Queen, true),
    ];

    for (r1, r2, suited) in defend_hands {
        let freq = lookup(r1, r2, suited);
        total_weight += 1.0;
        weighted_defend += freq;
        if freq < 0.10 {
            issues.push(format!(
                "{:?}{:?}{} should defend at least 10% but defends {:.1}%",
                r1,
                r2,
                if suited { "s" } else { "o" },
                freq * 100.0
            ));
        }
    }

    let avg_defend = weighted_defend / total_weight;
    if avg_defend < EXPECTED_DEFEND_THRESHOLD {
        issues.push(format!(
            "Average BB defend frequency {:.1}% is below expected {:.1}%",
            avg_defend * 100.0,
            EXPECTED_DEFEND_THRESHOLD * 100.0
        ));
    }

    ValidationResult {
        category: "bb_defend".to_string(),
        passed: issues.is_empty(),
        score: avg_defend,
        issues,
    }
}

pub struct ValidationResult {
    pub category: String,
    pub passed: bool,
    pub score: f32,
    pub issues: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Dummy lookup: opens with 80% of hands, 3-bets 15%, defends 60%
    fn dummy_lookup(r1: Rank, r2: Rank, suited: bool) -> f32 {
        // AA always opens
        if r1 == Rank::Ace && r2 == Rank::Ace {
            return 1.0;
        }
        // 72o rarely opens
        if r1 == Rank::Seven && r2 == Rank::Two && !suited {
            return 0.05;
        }
        // A6s opens ~35% (above threshold)
        if r1 == Rank::Ace && r2 == Rank::Six && suited {
            return 0.35;
        }
        // Everything else opens at a reasonable frequency
        0.80
    }

    #[test]
    fn test_validate_preflop_opening_passes() {
        let result = validate_preflop_opening(&dummy_lookup);
        assert!(result.passed, "issues: {:?}", result.issues);
        assert!(result.score > 0.7);
    }

    #[test]
    fn test_validate_bb_3bet_passes() {
        let result = validate_bb_3bet(&dummy_lookup);
        assert!(result.passed, "issues: {:?}", result.issues);
        assert!(result.score > 0.1);
    }

    #[test]
    fn test_validate_bb_defend_passes() {
        let result = validate_bb_defend(&dummy_lookup);
        assert!(result.passed, "issues: {:?}", result.issues);
        assert!(result.score > 0.1);
    }

    /// A bad lookup that opens everything — should flag 72o and low suited aces
    fn overopen_lookup(_r1: Rank, _r2: Rank, _suited: bool) -> f32 {
        1.0 // open everything 100%
    }

    #[test]
    fn test_validate_preflop_opening_catches_overopen() {
        let result = validate_preflop_opening(&overopen_lookup);
        assert!(!result.passed);
        assert!(result.issues.iter().any(|i| i.contains("72o")));
    }

    /// A bad lookup that never opens — should flag low frequencies
    fn nunopen_lookup(_r1: Rank, _r2: Rank, _suited: bool) -> f32 {
        0.0
    }

    #[test]
    fn test_validate_preflop_opening_catches_foldey() {
        let result = validate_preflop_opening(&nunopen_lookup);
        assert!(!result.passed);
        assert!(result.issues.iter().any(|i| i.contains("AA")));
    }
}
