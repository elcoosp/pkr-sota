use clap::Parser;
use pkr_abstraction::{KMeansAbstraction, load_centroids};
use pkr_cfr::Trainer;
use pkr_core::state::{GameState, Street};
use pkr_eval::NlheEvaluator;
use pkr_export::writer::write_blueprint;
use rand::seq::SliceRandom;
use std::path::PathBuf;
use std::sync::Arc;

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

    #[arg(long, default_value = "centroids.bin")]
    centroids: PathBuf,

    #[arg(long)]
    threads: Option<usize>,
}

fn main() {
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();

    let num_threads = cli.threads
        .unwrap_or_else(|| std::thread::available_parallelism().map(|p| p.get()).unwrap_or(4));

    let _store = load_centroids(cli.centroids.to_str().unwrap())
        .expect("Failed to load centroids");
    let evaluator = Arc::new(NlheEvaluator);
    let abstraction = Arc::new(KMeansAbstraction::new());
    let num_actions = 4;
    let mut trainer = Trainer::new(abstraction, evaluator.clone(), num_actions);

    let mut rng = rand::rng();
    let mut deck: Vec<u8> = (0..52).collect();

    let iters_per_thread = cli.iterations / num_threads as u32;
    let remainder = cli.iterations % num_threads as u32;

    deck.shuffle(&mut rng);
    let hero = [deck[0], deck[1]];
    let villain = [deck[2], deck[3]];
    let flop = vec![deck[4], deck[5], deck[6]];
    let turn = vec![deck[7]];
    let river = vec![deck[8]];

    let mut state = GameState::new(cli.stack, cli.sb, cli.bb);
    state.set_hole_cards(hero, villain);
    state.board = flop;
    state.street = Street::Flop;
    state.street_bets = [0.0, 0.0];
    state.history = Vec::new();
    state.actor = 1;

    let chance_cards: [Vec<u8>; 3] = [vec![], turn, river];

    trainer.run_iterations_parallel(&state, &chance_cards, iters_per_thread, num_threads);
    for _ in 0..remainder {
        trainer.run_iterations_parallel(&state, &chance_cards, 1, 1);
    }

    let output_path = cli.output.to_str().expect("invalid output path");
    write_blueprint(output_path, trainer.get_table());
    tracing::info!("Blueprint written to {}", output_path);
}
