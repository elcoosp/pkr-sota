# W1-T1: Card Primitives

## Objective
Implement basic card representations and the NLHE ruleset in `pkr-core`.

## Dependencies
- `pkr-contracts` (W0-T1)

## Exclusive File Paths
- `crates/pkr-core/Cargo.toml`
- `crates/pkr-core/src/card.rs`
- `crates/pkr-core/src/deck.rs`
- `crates/pkr-core/src/rules.rs`
- `crates/pkr-core/src/lib.rs`

## TDD Instructions
1. **Red**: In `card.rs`, write tests verifying that `Card::new(Suit::Spade, Rank::Ace)` returns a valid card and that 52 unique cards can be generated. In `rules.rs`, write a test verifying `NlheRuleset` implements `GameRules` with `max_actions_per_node() == 4`, `deck_size() == 52`, `hand_size() == 2`.
2. **Green**: Implement `Suit` (enum), `Rank` (enum), `Card` (struct). Implement `Deck` with `new()` and `deal()`. Implement `NlheRuleset`.
3. **Refactor**: Use `#[repr(u8)]` for enums to minimize memory footprint. Derive `Copy`, `Clone`, `PartialEq`, `Eq`, `Debug` for all primitives.

## Acceptance Criteria
- `cargo test -p pkr-core` passes.
- `cargo clippy -p pkr-core -- -D warnings` passes.
- `NlheRuleset` correctly implements `pkr_contracts::GameRules`.
