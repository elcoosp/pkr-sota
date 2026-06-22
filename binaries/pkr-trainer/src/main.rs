use clap::Parser;
use pkr_abstraction::KMeansAbstraction;
use pkr_cfr::Trainer;
use pkr_core::rules::NlheRuleset;
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
}

fn main() {
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();

    let rules = Box::new(NlheRuleset);
    let eval = Box::new(NlheEvaluator);

    // Provide some dummy centroids so it runs out of the box
    let centroids = vec![(0.2, 0.04), (0.5, 0.25), (0.8, 0.64)];
    let abstraction = Box::new(KMeansAbstraction::new(centroids, eval));

    let capacity = 1024; // Capacity is just a hint for HashMaps now
    let mut trainer = Trainer::new(rules, abstraction, Box::new(NlheEvaluator), capacity);

    let mut rng = rand::rng();
    let mut deck: Vec<u8> = (0..52).collect();

    for i in 0..cli.iterations {
        if i % 100 == 0 {
            tracing::info!("Iteration {}/{}", i, cli.iterations);
        }
        deck.shuffle(&mut rng);
        let hole = deck[..2].to_vec();
        trainer.run_iteration(&hole, &mut rng);
    }

    let output_path = cli.output.to_str().expect("invalid output path");
    write_blueprint(output_path, trainer.get_table());
    tracing::info!("Blueprint written to {}", output_path);
}
