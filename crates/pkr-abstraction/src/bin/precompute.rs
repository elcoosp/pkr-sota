use pkr_abstraction::{calculate_ehs, save_centroids, CentroidStore};
use pkr_eval::NlheEvaluator;
use rand::prelude::IndexedRandom;
use rand::seq::SliceRandom;
use rayon::prelude::*;
use std::env;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        eprintln!("Usage: pkr-abstraction-precompute <num_samples> <k> [output_file]");
        std::process::exit(1);
    }
    let num_samples: usize = args[1].parse().unwrap_or(10_000);
    let k: usize = args[2].parse().unwrap_or(200);
    let output = if args.len() > 3 { args[3].clone() } else { "centroids.bin".to_string() };

    let evaluator = NlheEvaluator;
    let deck: Vec<u8> = (0..52).collect();

    let features: Vec<(f32, f32)> = (0..num_samples)
        .into_par_iter()
        .map(|_| {
            let mut local_rng = rand::rng();
            let mut cards = deck.clone();
            cards.shuffle(&mut local_rng);
            let hole = &cards[..2];
            let board = &cards[2..5];
            let (ehs, ehs_sq) = calculate_ehs(hole, board, &evaluator);
            (ehs, ehs_sq)
        })
        .collect();

    let centroids = simple_kmeans(&features, k, 50);

    let store = CentroidStore { centroids };
    save_centroids(&output, &store).expect("Failed to save centroids");
    println!("Saved {} centroids to {}", k, output);
}

fn simple_kmeans(data: &[(f32, f32)], k: usize, max_iters: usize) -> Vec<(f32, f32)> {
    let n = data.len();
    if n == 0 || k == 0 {
        return vec![];
    }
    let k = k.min(n);
    let mut rng = rand::rng();
    let mut centroids: Vec<(f32, f32)> = data.sample(&mut rng, k).cloned().collect();

    for _ in 0..max_iters {
        let assignments: Vec<usize> = data.par_iter().map(|&point| {
            centroids.iter()
                .enumerate()
                .min_by(|a, b| {
                    let c1 = a.1;
                    let c2 = b.1;
                    let d1 = (point.0 - c1.0).powi(2) + (point.1 - c1.1).powi(2);
                    let d2 = (point.0 - c2.0).powi(2) + (point.1 - c2.1).powi(2);
                    d1.partial_cmp(&d2).unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|(idx, _)| idx)
                .unwrap_or(0)
        }).collect();

        let mut sums = vec![(0.0f32, 0.0f32); k];
        let mut counts = vec![0usize; k];
        for (&point, &cluster) in data.iter().zip(assignments.iter()) {
            sums[cluster].0 += point.0;
            sums[cluster].1 += point.1;
            counts[cluster] += 1;
        }

        let mut changed = false;
        for i in 0..k {
            if counts[i] > 0 {
                let new_centroid = (sums[i].0 / counts[i] as f32, sums[i].1 / counts[i] as f32);
                if (new_centroid.0 - centroids[i].0).abs() > 1e-6 ||
                   (new_centroid.1 - centroids[i].1).abs() > 1e-6 {
                    changed = true;
                }
                centroids[i] = new_centroid;
            }
        }
        if !changed {
            break;
        }
    }
    centroids
}
