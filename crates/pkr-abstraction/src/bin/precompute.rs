use pkr_abstraction::{calculate_ehs, load_centroids, CentroidStore};
use pkr_contracts::Evaluator;
use pkr_eval::lookup::TableEvaluator;
use pkr_eval::slow::NlheEvaluator;
use rand::prelude::IndexedRandom;
use rand::seq::SliceRandom;
use rayon::prelude::*;
use std::env;
use std::fs::File;
use std::io::Write;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: pkr-abstraction-precompute <centroids|table|abstraction> [args...]");
        std::process::exit(1);
    }
    match args[1].as_str() {
        "centroids" => {
            let num_samples: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(10_000);
            let k: usize = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(200);
            let output = args.get(4).cloned().unwrap_or("centroids.bin".to_string());
            generate_centroids(num_samples, k, &output);
        }
        "table" => {
            let output = args.get(2).cloned().unwrap_or("hand_ranks.bin".to_string());
            generate_rank_table(&output);
        }
        "abstraction" => {
            let centroids_path = args.get(2).expect("centroids file required");
            let rank_table_path = args.get(3).expect("hand ranks table file required");
            let output = args.get(4).cloned().unwrap_or("abstraction.bin".to_string());
            // Set environment variable for EHS samples
            generate_abstraction_table(centroids_path, rank_table_path, &output);
        }
        _ => eprintln!("Unknown command"),
    }
}

fn generate_centroids(num_samples: usize, k: usize, output: &str) {
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
    save_centroids(output, &store).expect("Failed to save centroids");
    println!("Saved {} centroids to {}", k, output);
}

fn generate_rank_table(output: &str) {
    let evaluator = NlheEvaluator;
    let total = 2_598_960usize;
    let mut table: Vec<u32> = vec![0u32; total];

    table.par_iter_mut().enumerate().for_each(|(idx, slot)| {
        let cards = combinadic_unrank(idx as u32, 5, 52);
        let rank = evaluator.evaluate_hand(&[], &cards);
        *slot = rank;
    });

    let mut file = File::create(output).expect("failed to create rank table file");
    for rank in table {
        file.write_all(&rank.to_le_bytes()).unwrap();
    }
    println!("Generated rank table with {} entries -> {}", total, output);
}

fn generate_abstraction_table(centroids_path: &str, rank_table_path: &str, output: &str) {
    let store = load_centroids(centroids_path).expect("Failed to load centroids");
    let centroids = &store.centroids;
    let evaluator = TableEvaluator::new(rank_table_path).expect("Failed to load rank table");

    let total_combos = 2_598_960u64;
    let entries = total_combos as usize * 10;
    let mut table: Vec<u8> = vec![0u8; entries];

    let hole_masks: Vec<[usize; 2]> = vec![
        [0,1], [0,2], [0,3], [0,4],
        [1,2], [1,3], [1,4], [2,3], [2,4], [3,4],
    ];

    // Use larger chunks for rayon to reduce overhead
    table.par_chunks_mut(1024).enumerate().for_each(|(chunk_idx, chunk)| {
        for (i, slot) in chunk.iter_mut().enumerate() {
            let flat_idx = chunk_idx * 1024 + i;
            let combo_idx = (flat_idx / 10) as u32;
            let mask_idx = flat_idx % 10;
            let cards = combinadic_unrank(combo_idx, 5, 52);
            let pos = &hole_masks[mask_idx];
            let hole = [cards[pos[0]], cards[pos[1]]];
            let board: Vec<u8> = (0..5).filter(|j| !pos.contains(j)).map(|j| cards[j]).collect();
            let (ehs, ehs_sq) = calculate_ehs(&hole, &board, &evaluator);
            let cluster_id = centroids.iter()
                .enumerate()
                .min_by(|a, b| {
                    let c1 = a.1; let c2 = b.1;
                    let d1 = (ehs - c1.0).powi(2) + (ehs_sq - c1.1).powi(2);
                    let d2 = (ehs - c2.0).powi(2) + (ehs_sq - c2.1).powi(2);
                    d1.partial_cmp(&d2).unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|(idx, _)| idx as u8)
                .unwrap_or(0);
            *slot = cluster_id;
        }
    });

    let mut file = File::create(output).expect("failed to create abstraction table");
    file.write_all(&table).unwrap();
    println!("Generated abstraction table with {} entries -> {}", entries, output);
}

fn save_centroids(path: &str, store: &CentroidStore) -> Result<(), Box<dyn std::error::Error>> {
    let file = File::create(path)?;
    bincode::serialize_into(file, store)?;
    Ok(())
}

fn choose(n: u32, k: u32) -> u32 {
    if k > n { return 0; }
    match k {
        0 => 1,
        1 => n,
        2 => n * (n - 1) / 2,
        3 => n * (n - 1) * (n - 2) / 6,
        4 => n * (n - 1) * (n - 2) * (n - 3) / 24,
        5 => n * (n - 1) * (n - 2) * (n - 3) * (n - 4) / 120,
        _ => panic!("k > 5 not supported"),
    }
}

fn combinadic_unrank(mut index: u32, k: u32, n: u32) -> Vec<u8> {
    let mut result = Vec::with_capacity(k as usize);
    let mut remaining = n;
    for i in (1..=k).rev() {
        let mut x = remaining - 1;
        while choose(x, i) > index { x -= 1; }
        result.push(x as u8);
        index -= choose(x, i);
        remaining = x;
    }
    result.sort_unstable_by(|a, b| b.cmp(a));
    result
}

fn simple_kmeans(data: &[(f32, f32)], k: usize, max_iters: usize) -> Vec<(f32, f32)> {
    let n = data.len();
    if n == 0 || k == 0 { return vec![]; }
    let k = k.min(n);
    let mut rng = rand::rng();
    let mut centroids: Vec<(f32, f32)> = data.sample(&mut rng, k).cloned().collect();
    for _ in 0..max_iters {
        let assignments: Vec<usize> = data.par_iter().map(|&point| {
            centroids.iter()
                .enumerate()
                .min_by(|a, b| {
                    let c1 = a.1; let c2 = b.1;
                    let d1 = (point.0 - c1.0).powi(2) + (point.1 - c1.1).powi(2);
                    let d2 = (point.0 - c2.0).powi(2) + (point.1 - c2.1).powi(2);
                    d1.partial_cmp(&d2).unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|(idx, _)| idx).unwrap_or(0)
        }).collect();
        let mut sums = vec![(0.0f32,0.0f32); k];
        let mut counts = vec![0usize; k];
        for (&point, &cluster) in data.iter().zip(assignments.iter()) {
            sums[cluster].0 += point.0; sums[cluster].1 += point.1;
            counts[cluster] += 1;
        }
        let mut changed = false;
        for i in 0..k {
            if counts[i] > 0 {
                let new = (sums[i].0 / counts[i] as f32, sums[i].1 / counts[i] as f32);
                if (new.0 - centroids[i].0).abs() > 1e-6 || (new.1 - centroids[i].1).abs() > 1e-6 {
                    changed = true;
                }
                centroids[i] = new;
            }
        }
        if !changed { break; }
    }
    centroids
}
