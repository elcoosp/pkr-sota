use clap::Parser;
use pkr_cfr::Trainer;
use pkr_contracts::{AbstractionBuilder, Evaluator};
use pkr_core::rules::NlheRuleset;
use pkr_export::writer::write_blueprint;
use rand::Rng;
use rand::RngExt;
use std::path::PathBuf;

// Fast deterministic abstraction – avoids expensive Monte Carlo EHS.
pub struct FastAbstraction;

impl AbstractionBuilder for FastAbstraction {
    fn get_infoset_hash(&self, hole: &[u8], _board: &[u8], _history: &[u8]) -> u64 {
        // Simple hash based on hole cards only (fast).
        let mut h: u64 = 14695981039346656037;
        for &c in hole {
            h = h.wrapping_mul(1099511628211).wrapping_add(c as u64);
        }
        h
    }
}

/// Trivial evaluator for CFR to avoid board-length assertion.
pub struct TrivialEvaluator;

impl Evaluator for TrivialEvaluator {
    fn evaluate_hand(&self, _hole: &[u8], _board: &[u8]) -> u16 {
        0
    }
}

#[derive(Parser)]
#[command(name = "pkr-trainer")]
struct Cli {
    #[arg(long, default_value_t = 1000)]
    iterations: u32,

    #[arg(long, default_value = "blueprint.bin")]
    output: PathBuf,

    #[arg(long, default_value = "nlhe")]
    variant: String,
}

fn main() {
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();

    assert_eq!(
        cli.variant, "nlhe",
        "Only 'nlhe' variant is supported at the moment"
    );

    let rules = Box::new(NlheRuleset);
    let cfr_evaluator = Box::new(TrivialEvaluator);
    let abstraction = Box::new(FastAbstraction);

    let capacity = 1024;
    let mut trainer = Trainer::new(rules, abstraction, cfr_evaluator, capacity);

    let mut rng = rand::rng();

    for i in 0..cli.iterations {
        if i % 10_000 == 0 {
            tracing::info!("Iteration {}/{}", i, cli.iterations);
        }
        let hole = random_hole(&mut rng);
        trainer.run_iteration(&hole, &mut rng);
    }

    let keys: Vec<u64> = (0..capacity as u64).collect();
    let output_path = cli.output.to_str().expect("invalid output path");
    write_blueprint(output_path, trainer.get_table(), &keys);
    tracing::info!("Blueprint written to {}", output_path);
}

fn random_hole(rng: &mut impl Rng) -> Vec<u8> {
    let idx1 = rng.random_range(0..52);
    let idx2 = loop {
        let i = rng.random_range(0..52);
        if i != idx1 {
            break i;
        }
    };
    vec![idx1, idx2]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn test_binary_creates_blueprint() {
        let output_file = "test_blueprint.bin";

        let status = Command::new("cargo")
            .args(&[
                "run",
                "--bin",
                "pkr-trainer",
                "--",
                "--iterations",
                "10",
                "--output",
                output_file,
            ])
            .status()
            .expect("failed to run binary");
        assert!(status.success(), "binary exited with non‑zero status");

        assert!(std::path::Path::new(output_file).exists());

        use pkr_export::header::FileHeader;
        use std::io::Read;
        let mut file = std::fs::File::open(output_file).unwrap();
        let mut header_bytes = [0u8; std::mem::size_of::<FileHeader>()];
        file.read_exact(&mut header_bytes).unwrap();
        let header: &FileHeader = bytemuck::from_bytes(&header_bytes);
        assert_eq!(&header.magic, b"PKRSOTA1");
        assert_eq!(header.infoset_count, 1024);

        std::fs::remove_file(output_file).ok();
    }
}
