use clap::Parser;
use pkr_abstraction::{KMeansAbstraction, load_centroids};
use pkr_cfr::Trainer;
use pkr_core::state::GameState;
use pkr_eval::NlheEvaluator;
use pkr_export::writer::write_blueprint;
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

    #[arg(long, default_value = "centroids.bin")]
    centroids: PathBuf,
}

fn main() {
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();

    let store = load_centroids(cli.centroids.to_str().unwrap())
        .expect("Failed to load centroids");
    let abstraction = Box::new(KMeansAbstraction::from_store(store));
    let num_actions = 4;
    let mut trainer = Trainer::new(abstraction, Box::new(NlheEvaluator), num_actions);

    let mut rng = rand::rng();
    let mut deck: Vec<u8> = (0..52).collect();

    for i in 0..cli.iterations {
        if i % 100 == 0 {
            tracing::info!("Iteration {}/{}", i, cli.iterations);
        }
        deck.shuffle(&mut rng);
        let hero = [deck[0], deck[1]];
        let villain = [deck[2], deck[3]];
        let flop = vec![deck[4], deck[5], deck[6]];
        let turn = vec![deck[7]];
        let river = vec![deck[8]];

        let mut state = GameState::new(cli.stack, cli.sb, cli.bb);
        state.set_hole_cards(hero, villain);
        let chance_cards: [Vec<u8>; 3] = [flop, turn, river];
        trainer.run_iteration(&state, &chance_cards, &mut rng);
    }

    let output_path = cli.output.to_str().expect("invalid output path");
    write_blueprint(output_path, trainer.get_table());
    tracing::info!("Blueprint written to {}", output_path);
}
