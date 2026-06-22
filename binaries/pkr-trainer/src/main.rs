use clap::Parser;
use pkr_abstraction::KMeansAbstraction;
use pkr_cfr::Trainer;
use pkr_core::state::{GameState, Street};
use pkr_eval::NlheEvaluator;
use pkr_export::writer::write_blueprint;
use rand::Rng;
use rand::seq::SliceRandom;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "pkr-trainer")]
struct Cli {
    #[arg(long, default_value_t = 1000)]
    iterations: u32,

    #[arg(long, default_value = "blueprint.bin")]
    output: PathBuf,

    #[arg(long, default_value_t = 200.0)]
    stack: f32,

    #[arg(long, default_value_t = 1.0)]
    sb: f32,

    #[arg(long, default_value_t = 2.0)]
    bb: f32,
}

fn main() {
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();

    // Dummy centroids for abstraction
    let centroids = vec![(0.2, 0.04), (0.5, 0.25), (0.8, 0.64)];
    let eval = Box::new(NlheEvaluator);
    let abstraction = Box::new(KMeansAbstraction::new(centroids, eval));

    // Number of abstract actions: we use 4 (fold, check/call, bet, all-in)
    let num_actions = 4;
    let mut trainer = Trainer::new(abstraction, Box::new(NlheEvaluator), num_actions);

    let mut rng = rand::rng();
    let mut deck: Vec<u8> = (0..52).collect();

    for i in 0..cli.iterations {
        if i % 100 == 0 {
            tracing::info!("Iteration {}/{}", i, cli.iterations);
        }
        deck.shuffle(&mut rng);
        let hole_hero = [deck[0], deck[1]];
        let hole_villain = [deck[2], deck[3]];
        let flop = vec![deck[4], deck[5], deck[6]];
        let turn = vec![deck[7]];
        let river = vec![deck[8]];

        // Build initial state
        let mut state = GameState::new(cli.stack, cli.sb, cli.bb);
        state.set_hole_cards(hole_hero, hole_villain);

        // Advance through streets dealing cards
        // Preflop done automatically; then flop
        if state.street == Street::Preflop {
            state.advance_street(&flop);
        }
        if state.street == Street::Flop {
            state.advance_street(&turn);
        }
        if state.street == Street::Turn {
            state.advance_street(&river);
        }
        // Now state is at River with full board
        // For simplicity, we skip pre-river betting and start at the flop?
        // Actually our traversal assumes the game starts from the state given.
        // We'll just start at flop with some pot and betting history empty.
        // Reset history to empty and reinitialize: we set state to postflop with a pot.
        // For now, we'll just build a state at flop with initial pot = sb+bb.
        // Better: we'll define a utility to set up a random flop scenario.
        // Quick hack: we create a state, deal flop, and set both players as having checked preflop.
        // This is not correct, but we need a functioning trainer demo. We'll do it properly later.

        // Quick fix: start from flop state with 0 bets, pot = sb+bb, dealer=0 (hero SB).
        let mut state = GameState::new(cli.stack, cli.sb, cli.bb);
        state.set_hole_cards(hole_hero, hole_villain);
        // override: go directly to flop
        state.board = flop;
        state.street = Street::Flop;
        state.street_bets = [0.0, 0.0];
        state.history = Vec::new();
        state.actor = 1; // non-dealer first on flop (villain)
        // pot remains sb+bb

        trainer.run_iteration(&state, &mut rng);
    }

    let output_path = cli.output.to_str().expect("invalid output path");
    write_blueprint(output_path, trainer.get_table());
    tracing::info!("Blueprint written to {}", output_path);
}
