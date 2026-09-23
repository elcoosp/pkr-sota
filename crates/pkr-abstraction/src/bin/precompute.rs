#![allow(clippy::needless_range_loop)] // numerics: indexed loops are idiomatic here

use pkr_abstraction::{calculate_ehs, load_centroids, save_centroids, CentroidStore};
use pkr_contracts::Evaluator;
use pkr_eval::lookup::choose;
use pkr_eval::lookup_fast::{
    combinadic_unrank_2, combinadic_unrank_3, combinadic_unrank_5, combinadic_unrank_6,
    combinadic_unrank_7, TableEvaluator,
};
use rand::rngs::StdRng;
use rand::SeedableRng;
use rand::{seq::IndexedRandom, RngExt};
use rayon::prelude::*;
use std::fs::File;
use std::io::Write;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: precompute <command> [args...]");
        eprintln!("Commands: flow, turn, preflop, flop, river, all7, abs5, abs6, all4, all6, all8");
        std::process::exit(1);
    }
    match args[1].as_str() {
        "flow" => {
            let centroids_path = args
                .get(2)
                .map(|s| s.as_str())
                .unwrap_or("centroids_10d.bin");
            let rank_table_path = args.get(3).map(|s| s.as_str()).unwrap_or("hand_ranks.bin");
            let output = args
                .get(4)
                .map(|s| s.as_str())
                .unwrap_or("abstraction_table.bin");
            println!(
                "Centroids: {} | Rank table: {}",
                centroids_path, rank_table_path
            );
            let _ = generate_abstraction_table(centroids_path, rank_table_path, output);
            println!("Done.");
        }
        "hand_ranks" => {
            let output = args.get(2).map(|s| s.as_str()).unwrap_or("hand_ranks.bin");
            println!("Generating hand ranks -> {}", output);
            let _ = generate_hand_ranks(output);
            println!("Done.");
        }
        "centroids" => {
            let num_samples = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(1000);
            let k = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(200);
            let rank_table_path = args.get(4).map(|s| s.as_str()).unwrap_or("hand_ranks.bin");
            let output = args.get(5).map(|s| s.as_str()).unwrap_or("centroids.bin");
            println!(
                "Samples: {} | K: {} | Rank table: {} | Output: {}",
                num_samples, k, rank_table_path, output
            );
            let _ = generate_centroids(num_samples, k, rank_table_path, output);
            println!("Done.");
        }
        "turn" => {
            let centroids_path = args
                .get(2)
                .map(|s| s.as_str())
                .unwrap_or("centroids_10d.bin");
            let rank_table_path = args.get(3).map(|s| s.as_str()).unwrap_or("hand_ranks.bin");
            let output = args.get(4).map(|s| s.as_str()).unwrap_or("turn_table.bin");
            println!(
                "Centroids: {} | Rank table: {} | output: {}",
                centroids_path, rank_table_path, output
            );
            let num_samples = args.get(5).and_then(|s| s.parse().ok()).unwrap_or(10_000);
            let _ = generate_turn_table(centroids_path, rank_table_path, output, num_samples);
            println!("Done.");
        }
        "preflop" => {
            let centroids_path = args
                .get(2)
                .map(|s| s.as_str())
                .unwrap_or("centroids_10d.bin");
            let rank_table_path = args.get(3).map(|s| s.as_str()).unwrap_or("hand_ranks.bin");
            let output = args
                .get(4)
                .map(|s| s.as_str())
                .unwrap_or("preflop_table.bin");
            let _ = generate_preflop_table(centroids_path, rank_table_path, output);
            println!("Done.");
        }
        "flop" => {
            let rank_table_path = args.get(2).map(|s| s.as_str()).unwrap_or("hand_ranks.bin");
            let output = args
                .get(3)
                .map(|s| s.as_str())
                .unwrap_or("flop_buckets.bin");
            let k = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(64);
            println!("Generating {} flop buckets -> {}", k, output);
            let _ = generate_flop_buckets(k, rank_table_path, output);
            println!("Done.");
        }
        "river" => {
            let rank_table_path = args.get(2).map(|s| s.as_str()).unwrap_or("hand_ranks.bin");
            let output = args
                .get(3)
                .map(|s| s.as_str())
                .unwrap_or("river_buckets.bin");
            let k = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(256);
            let _ = generate_river_buckets(k, rank_table_path, output);
            println!("Done.");
        }
        "all7" => {
            let rank_table_path = args.get(2).map(|s| s.as_str()).unwrap_or("hand_ranks.bin");
            let output = args.get(3).map(|s| s.as_str()).unwrap_or("all7_scores.bin");
            let _ = generate_all7_scores(rank_table_path, output);
            println!("Done.");
        }
        "abs5" => {
            let centroids_path = args
                .get(2)
                .map(|s| s.as_str())
                .unwrap_or("centroids_10d.bin");
            let rank_table_path = args.get(3).map(|s| s.as_str()).unwrap_or("hand_ranks.bin");
            let output = args
                .get(4)
                .map(|s| s.as_str())
                .unwrap_or("abstraction_table.bin");
            let _ = generate_abstraction_table(centroids_path, rank_table_path, output);
            println!("Done.");
        }
        "abs6" => {
            let centroids_path = args
                .get(2)
                .map(|s| s.as_str())
                .unwrap_or("centroids_10d.bin");
            let rank_table_path = args.get(3).map(|s| s.as_str()).unwrap_or("hand_ranks.bin");
            let output = args
                .get(4)
                .map(|s| s.as_str())
                .unwrap_or("abstraction_table.bin");
            let _ = generate_abstraction_table(centroids_path, rank_table_path, output);
            println!("Done.");
        }
        "all4" => {
            let centroids_path = args
                .get(2)
                .map(|s| s.as_str())
                .unwrap_or("centroids_10d.bin");
            let rank_table_path = args.get(3).map(|s| s.as_str()).unwrap_or("hand_ranks.bin");
            let output = args
                .get(4)
                .map(|s| s.as_str())
                .unwrap_or("abstraction_table.bin");
            let _ = generate_abstraction_table(centroids_path, rank_table_path, output);
            println!("Done.");
        }
        "all6" => {
            let centroids_path = args
                .get(2)
                .map(|s| s.as_str())
                .unwrap_or("centroids_10d.bin");
            let rank_table_path = args.get(3).map(|s| s.as_str()).unwrap_or("hand_ranks.bin");
            let output = args
                .get(4)
                .map(|s| s.as_str())
                .unwrap_or("abstraction_table.bin");
            let _ = generate_abstraction_table(centroids_path, rank_table_path, output);
            println!("Done.");
        }
        "all8" => {
            let centroids_path = args
                .get(2)
                .map(|s| s.as_str())
                .unwrap_or("centroids_10d.bin");
            let rank_table_path = args.get(3).map(|s| s.as_str()).unwrap_or("hand_ranks.bin");
            let output = args
                .get(4)
                .map(|s| s.as_str())
                .unwrap_or("abstraction_table.bin");
            let _ = generate_abstraction_table(centroids_path, rank_table_path, output);
            println!("Done.");
        }
        cmd => {
            eprintln!("Unknown command: {}", cmd);
            std::process::exit(1);
        }
    }
}

fn generate_hand_ranks(output: &str) -> Result<(), Box<dyn std::error::Error>> {
    use pkr_eval::NlheEvaluator;
    let evaluator = NlheEvaluator;
    let total = choose(52, 5) as usize;
    let mut ranks: Vec<u32> = vec![0u32; total];
    ranks.par_iter_mut().enumerate().for_each(|(idx, slot)| {
        let cards = combinadic_unrank_5(idx as u32);
        let hole = [cards[0], cards[1]];
        let board = [cards[2], cards[3], cards[4]];
        *slot = evaluator.evaluate_hand(&hole, &board);
    });
    let mut bytes: Vec<u8> = Vec::with_capacity(total * 4);
    for r in &ranks {
        bytes.extend_from_slice(&r.to_le_bytes());
    }
    let mut file = File::create(output).map_err(|e| format!("create {}: {}", output, e))?;
    file.write_all(&bytes)
        .map_err(|e| format!("write: {}", e))?;
    println!("Generated {} hand ranks -> {}", total, output);
    Ok(())
}

fn generate_centroids(
    num_samples: usize,
    k: usize,
    rank_table_path: &str,
    output: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let evaluator = TableEvaluator::new(rank_table_path)?;
    let total = choose(52, 2) as usize;
    let data: Vec<(f32, f32)> = (0..total)
        .into_par_iter()
        .map(|idx| {
            let hole = combinadic_unrank_2(idx as u32);
            let (ehs, ehs_sq) = calculate_ehs(&hole, &[], &evaluator);
            (ehs, ehs_sq)
        })
        .collect();
    let mut rng = StdRng::seed_from_u64(42);
    let sample: Vec<(f32, f32)> = if data.len() > num_samples {
        data.sample(&mut rng, num_samples).cloned().collect()
    } else {
        data
    };
    let centroids = simple_kmeans(&sample, k, 50);
    let store = CentroidStore { centroids };
    save_centroids(output, &store)?;
    println!("Generated {} centroids -> {}", k, output);
    Ok(())
}

fn generate_abstraction_table(
    centroids_path: &str,
    rank_table_path: &str,
    output: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let store = load_centroids(centroids_path)?;
    assert!(
        store.centroids.len() <= 255,
        "centroid count must be <= 255 for u8 ids"
    );
    let centroids = &store.centroids;
    let evaluator = TableEvaluator::new(rank_table_path)?;
    let total_combos = choose(52, 5) as usize;
    let entries = total_combos * 10;
    let mut table: Vec<u8> = vec![0u8; entries];

    table
        .par_chunks_mut(10)
        .enumerate()
        .for_each(|(combo_idx, chunk)| {
            let cards = pkr_eval::lookup_fast::combinadic_unrank_5(combo_idx as u32);
            for (mask_idx, slot) in chunk.iter_mut().enumerate() {
                let pos = [
                    [0, 1],
                    [0, 2],
                    [0, 3],
                    [0, 4],
                    [1, 2],
                    [1, 3],
                    [1, 4],
                    [2, 3],
                    [2, 4],
                    [3, 4],
                ][mask_idx];
                let hole: [u8; 2] = [cards[pos[0]], cards[pos[1]]];
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
                for (idx, c) in centroids.iter().enumerate() {
                    let dx = ehs - c.0;
                    let dy = ehs_sq - c.1;
                    let dist = dx * dx + dy * dy;
                    if dist < best_dist {
                        best_dist = dist;
                        best_idx = idx;
                    }
                }
                *slot = best_idx as u8;
            }
        });

    let mut file = File::create(output).map_err(|e| format!("create {}: {}", output, e))?;
    file.write_all(&table)
        .map_err(|e| format!("write: {}", e))?;
    println!(
        "Generated abstraction table with {} entries -> {}",
        entries, output
    );
    Ok(())
}

fn generate_turn_table(
    centroids_path: &str,
    rank_table_path: &str,
    output: &str,
    _num_samples: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    let store = load_centroids(centroids_path).map_err(|e| format!("centroids: {}", e))?;
    assert!(
        store.centroids.len() <= 255,
        "turn table uses u8 bucket ids; keep centroid count <= 255"
    );
    let centroids = &store.centroids;
    let evaluator = TableEvaluator::new(rank_table_path)?;
    let total_combos = choose(52, 6) as usize;
    let entries = total_combos * 15;
    let mut table: Vec<u8> = vec![0u8; entries];

    let masks: [[usize; 2]; 15] = [
        [0, 1],
        [0, 2],
        [0, 3],
        [0, 4],
        [0, 5],
        [1, 2],
        [1, 3],
        [1, 4],
        [1, 5],
        [2, 3],
        [2, 4],
        [2, 5],
        [3, 4],
        [3, 5],
        [4, 5],
    ];

    table
        .par_chunks_mut(15)
        .enumerate()
        .for_each(|(combo_idx, chunk)| {
            let cards = combinadic_unrank_6(combo_idx as u32);
            for (mask_idx, slot) in chunk.iter_mut().enumerate() {
                let pos = masks[mask_idx];
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
                let mut best_idx = 0u8;
                let mut best_dist = f32::MAX;
                for (ci, c) in centroids.iter().enumerate() {
                    let dx = ehs - c.0;
                    let dy = ehs_sq - c.1;
                    let dist = dx * dx + dy * dy;
                    if dist < best_dist {
                        best_dist = dist;
                        best_idx = ci as u8;
                    }
                }
                *slot = best_idx;
            }
        });

    let mut file = File::create(output).map_err(|e| format!("create {}: {}", output, e))?;
    file.write_all(&table)
        .map_err(|e| format!("write: {}", e))?;
    println!(
        "Generated turn table ({} entries, {} centroids) -> {}",
        entries,
        centroids.len(),
        output
    );
    Ok(())
}

fn generate_preflop_table(
    centroids_path: &str,
    rank_table_path: &str,
    output: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let store = load_centroids(centroids_path).map_err(|e| format!("centroids: {}", e))?;
    assert!(
        store.centroids.len() <= 255,
        "centroid count must be <= 255 for u8 ids"
    );
    let centroids = &store.centroids;
    let evaluator = TableEvaluator::new(rank_table_path)?;
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
    let mut file = File::create(output).map_err(|e| format!("create {}: {}", output, e))?;
    file.write_all(&table)
        .map_err(|e| format!("write: {}", e))?;
    println!(
        "Generated preflop table with {} entries -> {}",
        total, output
    );
    Ok(())
}

fn generate_flop_buckets(
    k: usize,
    rank_table_path: &str,
    output: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let evaluator = TableEvaluator::new(rank_table_path)?;
    let total_flops = choose(52, 3) as usize;
    let features: Vec<[f32; 10]> = (0..total_flops)
        .into_par_iter()
        .map(|flop_idx| {
            let flop = combinadic_unrank_3(flop_idx as u32);
            let mut histogram = [0.0f32; 10];
            let mut rng = StdRng::seed_from_u64(flop_idx as u64);
            let mut deck = [0u8; 49];
            let mut d_idx = 0;
            for c in 0..52u8 {
                if !flop.contains(&c) {
                    deck[d_idx] = c;
                    d_idx += 1;
                }
            }
            for _ in 0..500 {
                let hole = [deck[rng.random_range(0..49)], deck[rng.random_range(0..49)]];
                let (ehs, _) = calculate_ehs(&hole, &flop, &evaluator);
                let bucket = (ehs * 10.0).clamp(0.0, 9.0) as usize;
                histogram[bucket] += 1.0;
            }
            let mut norm = histogram;
            let sum: f32 = norm.iter().sum();
            if sum > 0.0 {
                for v in &mut norm {
                    *v /= sum;
                }
            }
            norm
        })
        .collect();

    let centroids = kmeans_10d(&features, k, 50);
    let mut buckets: Vec<u8> = vec![0; total_flops];
    buckets
        .par_iter_mut()
        .enumerate()
        .for_each(|(idx, bucket)| {
            let feat = &features[idx];
            let mut best_dist = f32::MAX;
            let mut best_bucket = 0u8;
            for (c_idx, c) in centroids.iter().enumerate() {
                let mut dist = 0.0;
                for i in 0..10 {
                    let d = feat[i] - c[i];
                    dist += d * d;
                }
                if dist < best_dist {
                    best_dist = dist;
                    best_bucket = c_idx as u8;
                }
            }
            *bucket = best_bucket;
        });
    let mut file = File::create(output).map_err(|e| format!("create {}: {}", output, e))?;
    file.write_all(&buckets)
        .map_err(|e| format!("write: {}", e))?;
    println!("Generated {} flop buckets -> {}", k, output);
    Ok(())
}

fn generate_river_buckets(
    k: usize,
    rank_table_path: &str,
    output: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    assert!(k <= 255, "river bucket count must be <= 255 (u8 buckets)");
    let evaluator = TableEvaluator::new(rank_table_path)?;
    let total_boards = choose(52, 5) as usize;
    let features: Vec<[f32; 10]> = (0..total_boards)
        .into_par_iter()
        .map(|board_idx| {
            let board = combinadic_unrank_5(board_idx as u32);
            let mut histogram = [0.0f32; 10];
            let mut rng = StdRng::seed_from_u64(board_idx as u64);
            let mut deck = [0u8; 47];
            let mut d_idx = 0;
            for c in 0..52u8 {
                if !board.contains(&c) {
                    deck[d_idx] = c;
                    d_idx += 1;
                }
            }
            for _ in 0..200 {
                let hole = [deck[rng.random_range(0..47)], deck[rng.random_range(0..47)]];
                let (ehs, _) = calculate_ehs(&hole, &board, &evaluator);
                let bucket = (ehs * 10.0).clamp(0.0, 9.0) as usize;
                histogram[bucket] += 1.0;
            }
            let mut norm = histogram;
            let sum: f32 = norm.iter().sum();
            if sum > 0.0 {
                for v in &mut norm {
                    *v /= sum;
                }
            }
            norm
        })
        .collect();

    let centroids = kmeans_10d(&features, k, 50);
    let mut buckets: Vec<u8> = vec![0; total_boards];
    buckets
        .par_iter_mut()
        .enumerate()
        .for_each(|(idx, bucket)| {
            let feat = &features[idx];
            let mut best_dist = f32::MAX;
            let mut best_bucket = 0u8;
            for (c_idx, c) in centroids.iter().enumerate() {
                let mut dist = 0.0;
                for i in 0..10 {
                    let d = feat[i] - c[i];
                    dist += d * d;
                }
                if dist < best_dist {
                    best_dist = dist;
                    best_bucket = c_idx as u8;
                }
            }
            *bucket = best_bucket;
        });
    let mut file = File::create(output).map_err(|e| format!("create {}: {}", output, e))?;
    file.write_all(&buckets)
        .map_err(|e| format!("write: {}", e))?;
    println!(
        "Generated {} river board-buckets ({} boards) -> {}",
        k, total_boards, output
    );
    Ok(())
}

fn generate_all7_scores(
    rank_table_path: &str,
    output: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let evaluator = TableEvaluator::new(rank_table_path)?;
    let total = choose(52, 7) as usize;
    let scores: Vec<u8> = (0..total)
        .into_par_iter()
        .map(|idx| {
            let cards = combinadic_unrank_7(idx as u32);
            let hole = [cards[0], cards[1]];
            let board = [cards[2], cards[3], cards[4], cards[5], cards[6]];
            let (ehs, _) = calculate_ehs(&hole, &board, &evaluator);
            (ehs * 255.0).clamp(0.0, 255.0) as u8
        })
        .collect();
    let mut file = File::create(output).map_err(|e| format!("create {}: {}", output, e))?;
    file.write_all(&scores)
        .map_err(|e| format!("write: {}", e))?;
    println!(
        "Generated all7 scores table with {} entries -> {}",
        total, output
    );
    Ok(())
}

fn kmeans_10d(data: &[[f32; 10]], k: usize, max_iters: usize) -> Vec<[f32; 10]> {
    let n = data.len();
    if n == 0 || k == 0 {
        return vec![];
    }
    let k = k.min(n);
    let mut centroids: Vec<[f32; 10]> = data.sample(&mut rand::rng(), k).cloned().collect();

    for _ in 0..max_iters {
        let mut assignments: Vec<usize> = vec![0; n];
        let mut counts: Vec<usize> = vec![0; k];
        let mut sums: Vec<[f32; 10]> = vec![[0f32; 10]; k];

        for (i, point) in data.iter().enumerate() {
            let mut best_dist = f32::MAX;
            let mut best_idx = 0;
            for (c_idx, c) in centroids.iter().enumerate() {
                let dist = point
                    .iter()
                    .zip(c.iter())
                    .map(|(p, c)| (p - c) * (p - c))
                    .sum::<f32>();
                if dist < best_dist {
                    best_dist = dist;
                    best_idx = c_idx;
                }
            }
            assignments[i] = best_idx;
            counts[best_idx] += 1;
            for j in 0..10 {
                sums[best_idx][j] += point[j];
            }
        }

        let mut moved = false;
        for c_idx in 0..k {
            if counts[c_idx] > 0 {
                let mut new_c = [0f32; 10];
                for j in 0..10 {
                    new_c[j] = sums[c_idx][j] / counts[c_idx] as f32;
                }
                if (new_c[0] - centroids[c_idx][0]).abs() > 1e-6 {
                    moved = true;
                }
                centroids[c_idx] = new_c;
            }
        }
        if !moved {
            break;
        }
    }
    centroids
}

fn simple_kmeans(data: &[(f32, f32)], k: usize, max_iters: usize) -> Vec<(f32, f32)> {
    let n = data.len();
    if n == 0 || k == 0 {
        return vec![];
    }
    let k = k.min(n);
    let mut centroids: Vec<(f32, f32)> = data.sample(&mut rand::rng(), k).cloned().collect();

    for _ in 0..max_iters {
        let mut assignments: Vec<usize> = vec![0; n];
        let mut counts: Vec<usize> = vec![0; k];
        let mut sums: Vec<(f32, f32)> = vec![(0f32, 0f32); k];

        for (i, &(x, y)) in data.iter().enumerate() {
            let mut best_dist = f32::MAX;
            let mut best_idx = 0;
            for (c_idx, &(cx, cy)) in centroids.iter().enumerate() {
                let d = (x - cx) * (x - cx) + (y - cy) * (y - cy);
                if d < best_dist {
                    best_dist = d;
                    best_idx = c_idx;
                }
            }
            assignments[i] = best_idx;
            counts[best_idx] += 1;
            sums[best_idx].0 += x;
            sums[best_idx].1 += y;
        }

        let mut moved = false;
        for c_idx in 0..k {
            if counts[c_idx] > 0 {
                let new_x = sums[c_idx].0 / counts[c_idx] as f32;
                let new_y = sums[c_idx].1 / counts[c_idx] as f32;
                if (new_x - centroids[c_idx].0).abs() > 1e-6
                    || (new_y - centroids[c_idx].1).abs() > 1e-6
                {
                    moved = true;
                }
                centroids[c_idx] = (new_x, new_y);
            }
        }
        if !moved {
            break;
        }
    }
    centroids
}

#[cfg(test)]
mod generator_tests {
    use super::*;

    /// The turn table must be exactly choose(52,6) * 15 bytes, indexed by
    /// the same layout flat_index_turn uses. Any change here breaks
    /// pkr-abstraction's runtime fall-through and must be a coordinated bump.
    #[test]
    fn turn_table_expected_size() {
        let n = choose(52, 6) as usize;
        assert_eq!(n, 20_358_520);
        assert_eq!(n * 15, 305_377_800);
    }

    /// The river board bucket table must be exactly choose(52,5) bytes.
    #[test]
    fn river_table_expected_size() {
        let n = choose(52, 5) as usize;
        assert_eq!(n, 2_598_960);
    }

    /// combinadic_unrank_6 must round-trip the boundary indices that
    /// flat_index_turn relies on. Indices 0 and C(52,6)-1 are the extremes.
    #[test]
    fn combinadic_unrank_6_boundaries() {
        let first = combinadic_unrank_6(0);
        let last = combinadic_unrank_6((choose(52, 6) - 1) as u32);
        // Ascending sort by descending card id: first combo is (5,4,3,2,1,0),
        // last combo is (51,50,49,48,47,46).
        assert_eq!(first, [5, 4, 3, 2, 1, 0]);
        assert_eq!(last, [51, 50, 49, 48, 47, 46]);
    }

    /// combinadic_unrank_5 for river board enumeration: boundaries only.
    #[test]
    fn combinadic_unrank_5_boundaries() {
        let first = combinadic_unrank_5(0);
        let last = combinadic_unrank_5((choose(52, 5) - 1) as u32);
        assert_eq!(first, [4, 3, 2, 1, 0]);
        assert_eq!(last, [51, 50, 49, 48, 47]);
    }
}
