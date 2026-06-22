use pkr_abstraction::{calculate_ehs, load_centroids, CentroidStore};
use pkr_contracts::Evaluator;
use pkr_eval::lookup::{choose, combinadic_unrank_2, combinadic_unrank_5, combinadic_unrank_6, TableEvaluator};
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
        eprintln!("Usage: pkr-abstraction-precompute <centroids|table|abstraction|turn_table|preflop_table> [args...]");
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
            generate_abstraction_table(centroids_path, rank_table_path, &output);
        }
        "turn_table" => {
            let centroids_path = args.get(2).expect("centroids file required");
            let rank_table_path = args.get(3).expect("hand ranks table file required");
            let output = args.get(4).cloned().unwrap_or("turn_abstraction.bin".to_string());
            generate_turn_table(centroids_path, rank_table_path, &output);
        }
        "preflop_table" => {
            let centroids_path = args.get(2).expect("centroids file required");
            let rank_table_path = args.get(3).expect("hand ranks table file required");
            let output = args.get(4).cloned().unwrap_or("preflop_abstraction.bin".to_string());
            generate_preflop_table(centroids_path, rank_table_path, &output);
        }
        _ => eprintln!("Unknown command"),
    }
}

fn generate_centroids(num_samples: usize, k: usize, output: &str) {
    assert!(k <= 255, "centroid count must be ≤ 255 for u8 cluster ids");
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
        }).collect();
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
        let cards = combinadic_unrank_5(idx as u32);
        let rank = evaluator.evaluate_hand(&[], &cards);
        *slot = rank;
    });
    let mut file = File::create(output).expect("failed to create rank table file");
    file.write_all(bytemuck::cast_slice(&table)).unwrap();
    println!("Generated rank table with {} entries -> {}", total, output);
}

fn generate_abstraction_table(centroids_path: &str, rank_table_path: &str, output: &str) {
    let store = load_centroids(centroids_path).expect("Failed to load centroids");
    assert!(store.centroids.len() <= 255, "centroid count must be ≤ 255 for u8 ids");
    let centroids = &store.centroids;
    let evaluator = TableEvaluator::new(rank_table_path).expect("Failed to load rank table");

    let total_combos = 2_598_960u64;
    let entries = total_combos as usize * 10;
    let mut table: Vec<u8> = vec![0u8; entries];

    const HOLE_MASKS_5: [[usize; 2]; 10] = [
        [0,1],[0,2],[0,3],[0,4],
        [1,2],[1,3],[1,4],[2,3],[2,4],[3,4],
    ];

    // Chunk exactly by 10 (1 combo per chunk)
    table.par_chunks_mut(10).enumerate().for_each(|(combo_idx, chunk)| {
        let cards = combinadic_unrank_5(combo_idx as u32);
        for (mask_idx, slot) in chunk.iter_mut().enumerate() {
            let pos = HOLE_MASKS_5[mask_idx];
            let hole = [cards[pos[0]], cards[pos[1]]];

            let mut board = [0u8; 3];
            let mut b_idx = 0;
            for j in 0..5 {
                if j != pos[0] && j != pos[1] {
                    board[b_idx] = cards[j];
                    b_idx += 1;
                }
            }

            let (ehs, ehs_sq) = calculate_ehs(&hole, &board, &evaluator);

            let mut best_idx = 0;
            let mut best_dist = f32::MAX;
            for (idx_c, c) in centroids.iter().enumerate() {
                let dx = ehs - c.0;
                let dy = ehs_sq - c.1;
                let dist = dx * dx + dy * dy;
                if dist < best_dist {
                    best_dist = dist;
                    best_idx = idx_c;
                }
            }
            *slot = best_idx as u8;
        }
    });

    let mut file = File::create(output).expect("failed to create abstraction table");
    file.write_all(&table).unwrap();
    println!("Generated abstraction table with {} entries -> {}", entries, output);
}

fn generate_turn_table(centroids_path: &str, rank_table_path: &str, output: &str) {
    let store = load_centroids(centroids_path).expect("Failed to load centroids");
    assert!(store.centroids.len() <= 255, "centroid count must be ≤ 255 for u8 ids");
    let centroids = &store.centroids;
    let evaluator = TableEvaluator::new(rank_table_path).expect("Failed to load rank table");

    let total_combos = choose(52, 6) as u64;
    let entries = total_combos as usize * 15;
    let mut table: Vec<u8> = vec![0u8; entries];

    const HOLE_MASKS_6: [[usize; 2]; 15] = [
        [0,1],[0,2],[0,3],[0,4],[0,5],
        [1,2],[1,3],[1,4],[1,5],
        [2,3],[2,4],[2,5],
        [3,4],[3,5],
        [4,5],
    ];

    table.par_chunks_mut(15).enumerate().for_each(|(combo_idx, chunk)| {
        let cards = combinadic_unrank_6(combo_idx as u32);
        for (mask_idx, slot) in chunk.iter_mut().enumerate() {
            let pos = HOLE_MASKS_6[mask_idx];
            let hole = [cards[pos[0]], cards[pos[1]]];

            let mut board = [0u8; 4];
            let mut b_idx = 0;
            for j in 0..6 {
                if j != pos[0] && j != pos[1] {
                    board[b_idx] = cards[j];
                    b_idx += 1;
                }
            }

            let (ehs, ehs_sq) = calculate_ehs(&hole, &board, &evaluator);

            let mut best_idx = 0;
            let mut best_dist = f32::MAX;
            for (idx_c, c) in centroids.iter().enumerate() {
                let dx = ehs - c.0;
                let dy = ehs_sq - c.1;
                let dist = dx * dx + dy * dy;
                if dist < best_dist {
                    best_dist = dist;
                    best_idx = idx_c;
                }
            }
            *slot = best_idx as u8;
        }
    });

    let mut file = File::create(output).expect("failed to create turn table");
    file.write_all(&table).unwrap();
    println!("Generated turn table with {} entries -> {}", entries, output);
}

fn generate_preflop_table(centroids_path: &str, rank_table_path: &str, output: &str) {
    let store = load_centroids(centroids_path).expect("Failed to load centroids");
    assert!(store.centroids.len() <= 255, "centroid count must be ≤ 255 for u8 ids");
    let centroids = &store.centroids;
    let evaluator = TableEvaluator::new(rank_table_path).expect("Failed to load rank table");

    let total = choose(52, 2) as usize;
    let mut table: Vec<u8> = vec![0u8; total];

    table.par_iter_mut().enumerate().for_each(|(idx, slot)| {
        let hole = combinadic_unrank_2(idx as u32);
        let (ehs, ehs_sq) = calculate_ehs(&hole, &[], &evaluator);

        let mut best_idx = 0;
        let mut best_dist = f32::MAX;
        for (idx_c, c) in centroids.iter().enumerate() {
            let dx = ehs - c.0;
            let dy = ehs_sq - c.1;
            let dist = dx * dx + dy * dy;
            if dist < best_dist {
                best_dist = dist;
                best_idx = idx_c;
            }
        }
        *slot = best_idx as u8;
    });

    let mut file = File::create(output).expect("failed to create preflop table");
    file.write_all(&table).unwrap();
    println!("Generated preflop table with {} entries -> {}", total, output);
}

fn save_centroids(path: &str, store: &CentroidStore) -> Result<(), Box<dyn std::error::Error>> {
    let file = File::create(path)?;
    bincode::serialize_into(file, store)?;
    Ok(())
}

fn simple_kmeans(data: &[(f32, f32)], k: usize, max_iters: usize) -> Vec<(f32, f32)> {
    let n = data.len();
    if n == 0 || k == 0 { return vec![]; }
    let k = k.min(n);
    if k == 1 {
        let mean = data.iter().fold((0.0,0.0), |a, &p| (a.0+p.0, a.1+p.1));
        return vec![(mean.0 / n as f32, mean.1 / n as f32)];
    }
    let mut rng = rand::rng();
    let mut centroids: Vec<(f32, f32)> = data.sample(&mut rng, k).cloned().collect();
    for _ in 0..max_iters {
        let assignments: Vec<usize> = data.par_iter().map(|&point| {
            let mut best = 0;
            let mut best_d = f32::MAX;
            for (i, c) in centroids.iter().enumerate() {
                let dx = point.0 - c.0;
                let dy = point.1 - c.1;
                let d = dx*dx + dy*dy;
                if d < best_d {
                    best_d = d;
                    best = i;
                }
            }
            best
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
