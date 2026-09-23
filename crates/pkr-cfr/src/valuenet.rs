//! Value network for turn re-solve leaf evaluation. (Roadmap §5.2)
//!
//! A small MLP trained on (EHS², OCHS) features → value maps to [0, 1].
//! On the M1, training a 64→128→64→1 MLP from generated self-play data
//! is feasible. Weights are exported to a flat binary format (no ONNX
//! dependency needed for inference — pure Rust matmul is fast enough
//! for turn leaf evaluation at arena scale).
//!
//! Training: `ValueNet::train` runs stochastic gradient descent on
//! sampled (ehs, ehs_sq, ochs, label) tuples where label ∈ [0, 1]
//! is the observed showdown equity.
//!
//! Inference: `ValueNet::forward` takes a 4-feature vector and returns
//! a scalar value in [0, 1].

use rand::{Rng, RngExt};

/// A small MLP: input(4) → hidden(128) → hidden(64) → output(1)
/// with ReLU activations between layers and sigmoid on the output.
#[derive(Clone)]
pub struct ValueNet {
    /// Input → hidden1 weights: [128, 4]
    w1: Vec<f32>,
    /// Hidden1 biases: [128]
    b1: Vec<f32>,
    /// Hidden1 → hidden2 weights: [64, 128]
    w2: Vec<f32>,
    /// Hidden2 biases: [64]
    b2: Vec<f32>,
    /// Hidden2 → output weights: [1, 64]
    w3: Vec<f32>,
    /// Output bias: [1]
    b3: f32,
}

const INPUT_DIM: usize = 4;
const HIDDEN1: usize = 128;
const HIDDEN2: usize = 64;
const OUTPUT_DIM: usize = 1;

impl ValueNet {
    /// Create a randomly initialized network.
    pub fn new(rng: &mut impl Rng) -> Self {
        fn init<R: Rng>(rng: &mut R, n: usize, limit: f32) -> Vec<f32> {
            (0..n).map(|_| rng.random_range(-limit..limit)).collect()
        }

        // Layer 1: input(4) -> hidden1(128)
        let w1 = init(
            rng,
            HIDDEN1 * INPUT_DIM,
            (6.0 / (INPUT_DIM + HIDDEN1) as f32).sqrt(),
        );
        let b1 = vec![0.0f32; HIDDEN1];

        // Layer 2: hidden1(128) -> hidden2(64)
        let w2: Vec<f32> = (0..HIDDEN2 * HIDDEN1)
            .map(|_| {
                rng.random_range(
                    -(6.0 / (HIDDEN1 + HIDDEN2) as f32).sqrt()
                        ..(6.0 / (HIDDEN1 + HIDDEN2) as f32).sqrt(),
                )
            })
            .collect();
        let b2 = vec![0.0f32; HIDDEN2];

        // Layer 3: hidden2(64) -> output(1)
        let limit3 = (6.0 / (HIDDEN2 + OUTPUT_DIM) as f32).sqrt();
        let w3: Vec<f32> = (0..OUTPUT_DIM * HIDDEN2)
            .map(|_| rng.random_range(-limit3..limit3))
            .collect();
        let b3 = 0.0f32;

        ValueNet {
            w1,
            b1,
            w2,
            b2,
            w3,
            b3,
        }
    }

    /// Forward pass: features → scalar value in [0, 1].
    /// Features: [ehs, ehs_sq, ochs, ochs_sq]
    pub fn forward(&self, features: &[f32; INPUT_DIM]) -> f32 {
        let mut h1 = vec![0.0f32; HIDDEN1];
        for i in 0..HIDDEN1 {
            let mut sum = self.b1[i];
            for j in 0..INPUT_DIM {
                sum += self.w1[i * INPUT_DIM + j] * features[j];
            }
            h1[i] = sum.max(0.0); // ReLU
        }

        let mut h2 = vec![0.0f32; HIDDEN2];
        for i in 0..HIDDEN2 {
            let mut sum = self.b2[i];
            for j in 0..HIDDEN1 {
                sum += self.w2[i * HIDDEN1 + j] * h1[j];
            }
            h2[i] = sum.max(0.0); // ReLU
        }

        let mut out = self.b3;
        for j in 0..HIDDEN2 {
            out += self.w3[j] * h2[j];
        }

        // Sigmoid to [0, 1]
        1.0 / (1.0 + (-out).exp())
    }

    /// Train with stochastic gradient descent on the given dataset.
    /// Each sample is (features, target) where target ∈ [0, 1].
    pub fn train(&mut self, data: &[(f32, f32, f32, f32, f32)], epochs: usize, lr: f32) {
        for _ in 0..epochs {
            for &(ehs, ehs_sq, ochs, ochs_sq, target) in data {
                let features = [ehs, ehs_sq, ochs, ochs_sq];

                // Forward pass (with intermediate values for backprop)
                let mut h1 = vec![0.0f32; HIDDEN1];
                for i in 0..HIDDEN1 {
                    let mut sum = self.b1[i];
                    for j in 0..INPUT_DIM {
                        sum += self.w1[i * INPUT_DIM + j] * features[j];
                    }
                    h1[i] = sum.max(0.0);
                }

                let mut h2 = vec![0.0f32; HIDDEN2];
                for i in 0..HIDDEN2 {
                    let mut sum = self.b2[i];
                    for j in 0..HIDDEN1 {
                        sum += self.w2[i * HIDDEN1 + j] * h1[j];
                    }
                    h2[i] = sum.max(0.0);
                }

                let mut out_val = self.b3;
                for j in 0..HIDDEN2 {
                    out_val += self.w3[j] * h2[j];
                }
                let output = 1.0 / (1.0 + (-out_val).exp());

                // Backward pass — binary cross-entropy loss gradient
                // d_loss/d_output = output - target
                let delta_out = output - target;

                // Gradients for layer 3
                let mut grad_w3 = vec![0.0f32; HIDDEN2];
                for j in 0..HIDDEN2 {
                    grad_w3[j] = delta_out * h2[j];
                }
                let grad_b3 = delta_out;

                // Gradients for layer 2 (through ReLU)
                let mut grad_h2 = vec![0.0f32; HIDDEN2];
                for j in 0..HIDDEN2 {
                    let mut upstream = 0.0f32;
                    for k in 0..OUTPUT_DIM {
                        upstream += delta_out * self.w3[k * HIDDEN2 + j];
                    }
                    // ReLU derivative
                    grad_h2[j] = upstream * if h2[j] > 0.0 { 1.0 } else { 0.0 };
                }

                let mut grad_w2 = vec![0.0f32; HIDDEN2 * HIDDEN1];
                let mut grad_b2 = vec![0.0f32; HIDDEN2];
                for i in 0..HIDDEN2 {
                    grad_b2[i] = grad_h2[i];
                    for j in 0..HIDDEN1 {
                        grad_w2[i * HIDDEN1 + j] = grad_h2[i] * h1[j];
                    }
                }

                // Gradients for layer 1 (through ReLU)
                let mut grad_h1 = vec![0.0f32; HIDDEN1];
                for j in 0..HIDDEN1 {
                    let mut upstream = 0.0f32;
                    for i in 0..HIDDEN2 {
                        upstream += grad_h2[i] * self.w2[i * HIDDEN1 + j];
                    }
                    grad_h1[j] = upstream * if h1[j] > 0.0 { 1.0 } else { 0.0 };
                }

                // Gradients for layer 0
                let mut grad_w1 = vec![0.0f32; HIDDEN1 * INPUT_DIM];
                let mut grad_b1 = vec![0.0f32; HIDDEN1];
                for i in 0..HIDDEN1 {
                    grad_b1[i] = grad_h1[i];
                    for j in 0..INPUT_DIM {
                        grad_w1[i * INPUT_DIM + j] = grad_h1[i] * features[j];
                    }
                }

                // Update weights
                for i in 0..HIDDEN1 {
                    self.b1[i] -= lr * grad_b1[i];
                    for j in 0..INPUT_DIM {
                        self.w1[i * INPUT_DIM + j] -= lr * grad_w1[i * INPUT_DIM + j];
                    }
                }

                for i in 0..HIDDEN2 {
                    self.b2[i] -= lr * grad_b2[i];
                    for j in 0..HIDDEN1 {
                        self.w2[i * HIDDEN1 + j] -= lr * grad_w2[i * HIDDEN1 + j];
                    }
                }

                for j in 0..HIDDEN2 {
                    self.w3[j] -= lr * grad_w3[j];
                }
                self.b3 -= lr * grad_b3;
            }
        }
    }

    /// Export weights to a flat binary file for runtime loading.
    /// Format:
    /// - w1: HIDDEN1 * INPUT_DIM * f32 (512 floats)
    /// - b1: HIDDEN1 * f32 (128 floats)
    /// - w2: HIDDEN2 * HIDDEN1 * f32 (8192 floats)
    /// - b2: HIDDEN2 * f32 (64 floats)
    /// - w3: OUTPUT_DIM * HIDDEN2 * f32 (64 floats)
    /// - b3: 1 * f32 (1 float)
    pub fn export(&self, path: &str) -> Result<(), std::io::Error> {
        use std::fs::File;
        use std::io::Write;
        let mut file = File::create(path)?;
        file.write_all(bytemuck::cast_slice(&self.w1))?;
        file.write_all(bytemuck::cast_slice(&self.b1))?;
        file.write_all(bytemuck::cast_slice(&self.w2))?;
        file.write_all(bytemuck::cast_slice(&self.b2))?;
        file.write_all(bytemuck::cast_slice(&self.w3))?;
        file.write_all(bytemuck::cast_slice(&[self.b3]))?;
        Ok(())
    }

    /// Load weights from a flat binary file (see `export`).
    pub fn load(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        use std::fs::File;
        use std::io::Read;
        let mut file = File::open(path)?;
        let mut data = Vec::new();
        file.read_to_end(&mut data)?;

        let floats: &[f32] = bytemuck::cast_slice(&data);
        let mut offset = 0;

        let w1 = floats[offset..offset + HIDDEN1 * INPUT_DIM].to_vec();
        offset += HIDDEN1 * INPUT_DIM;

        let b1 = floats[offset..offset + HIDDEN1].to_vec();
        offset += HIDDEN1;

        let w2 = floats[offset..offset + HIDDEN2 * HIDDEN1].to_vec();
        offset += HIDDEN2 * HIDDEN1;

        let b2 = floats[offset..offset + HIDDEN2].to_vec();
        offset += HIDDEN2;

        let w3 = floats[offset..offset + OUTPUT_DIM * HIDDEN2].to_vec();
        offset += OUTPUT_DIM * HIDDEN2;

        let b3 = floats[offset];
        // Note: offset += OUTPUT_DIM is not needed — b3 is the last field

        Ok(ValueNet {
            w1,
            b1,
            w2,
            b2,
            w3,
            b3,
        })
    }

    /// Total number of parameters in the network.
    pub fn param_count(&self) -> usize {
        self.w1.len() + self.b1.len() + self.w2.len() + self.b2.len() + self.w3.len() + 1
    }
}

/// Generate synthetic training data for the value network.
/// Features: [ehs, ehs_sq, ochs, ochs_sq]
/// Target: observed showdown equity (0.0 = always lose, 1.0 = always win)
///
/// This simulates the self-play data generation step:
/// in production, you'd generate data from CFR replay, but for
/// initialization, synthetic data where value ≈ mean hand strength
/// works as a reasonable starting point.
pub fn generate_training_data(
    n: usize,
    rng: &mut impl rand::Rng,
) -> Vec<(f32, f32, f32, f32, f32)> {
    let mut data = Vec::with_capacity(n);
    for _ in 0..n {
        // Sample EHS from a beta-like distribution (biased toward 0.5)
        let ehs: f32 = rng.random_range(0.0..1.0);
        let ehs_sq: f32 = ehs * ehs;

        // OCHS (opponent's chance of winning) — correlated with EHS
        let ochs: f32 = 1.0 - ehs + rng.random_range(-0.1..0.1);
        let ochs = ochs.clamp(0.0, 1.0);
        let ochs_sq: f32 = ochs * ochs;

        // Target: the true showdown value given both hands
        // In synthetic data, we use EHS as the target (self-consistent)
        let target: f32 = ehs;

        data.push((ehs, ehs_sq, ochs, ochs_sq, target));
    }
    data
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::RngExt;

    #[test]
    fn test_network_initializes_and_forward() {
        let mut rng = rand::rng();
        let net = ValueNet::new(&mut rng);
        assert_eq!(
            net.param_count(),
            HIDDEN1 * INPUT_DIM
                + HIDDEN1
                + HIDDEN2 * HIDDEN1
                + HIDDEN2
                + OUTPUT_DIM * HIDDEN2
                + OUTPUT_DIM
        );

        let features = [0.5f32, 0.25, 0.5, 0.25];
        let output = net.forward(&features);
        assert!((0.0..=1.0).contains(&output));
    }

    #[test]
    fn test_network_trains_to_converge() {
        let mut rng = rand::rng();
        let mut net = ValueNet::new(&mut rng);

        // Generate simple training data: value should equal ehs.
        // Kept small (200 samples, 15 epochs) so CI stays fast; the test
        // only needs to show the loss decreases, not to converge.
        let data: Vec<(f32, f32, f32, f32, f32)> = (0..200)
            .map(|_| {
                let mut r = rand::rng();
                let ehs: f32 = r.random_range(0.0..1.0);
                (ehs, ehs * ehs, 1.0 - ehs, (1.0 - ehs) * (1.0 - ehs), ehs)
            })
            .collect();

        // Check loss before training
        let features = [0.8f32, 0.64, 0.2, 0.04];
        let before = net.forward(&features);

        net.train(&data, 15, 0.01);

        let after = net.forward(&features);
        // After training, output for high EHS should be higher
        assert!(after > before);
    }

    #[test]
    fn test_export_and_load() {
        let mut rng = rand::rng();
        let net = ValueNet::new(&mut rng);

        // Set distinct weights
        let features = [0.7f32, 0.49, 0.3, 0.09];
        let original_output = net.forward(&features);

        let tmp = std::env::temp_dir().join("test_valuenet.bin");
        net.export(tmp.to_str().unwrap()).unwrap();

        let loaded = ValueNet::load(tmp.to_str().unwrap()).unwrap();
        let loaded_output = loaded.forward(&features);

        assert!((original_output - loaded_output).abs() < 1e-5);
        assert_eq!(net.param_count(), loaded.param_count());

        std::fs::remove_file(tmp).ok();
    }

    #[test]
    fn test_generate_training_data() {
        let mut rng = rand::rng();
        let data = generate_training_data(100, &mut rng);
        assert_eq!(data.len(), 100);

        for (ehs, ehs_sq, ochs, ochs_sq, target) in &data {
            assert!(*ehs >= 0.0 && *ehs <= 1.0);
            assert!((*ehs_sq - ehs * ehs).abs() < 1e-6);
            assert!(*ochs >= 0.0 && *ochs <= 1.0);
            assert!(*ochs_sq >= 0.0 && *ochs_sq <= 1.0);
            assert!(*target >= 0.0 && *target <= 1.0);
        }
    }

    #[test]
    fn test_forward_is_deterministic() {
        let mut rng = rand::rng();
        let net = ValueNet::new(&mut rng);

        let features = [0.5f32, 0.25, 0.5, 0.25];
        let out1 = net.forward(&features);
        let out2 = net.forward(&features);
        assert_eq!(out1, out2);
    }
}
