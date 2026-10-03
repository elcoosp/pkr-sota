//! NOTE: the GPU regret path is dead code on the production path. The
//! trainer calls `flush_cpu_batch`, never `flush_gpu_batch`. The module is
//! kept for experiments and must be enabled via the `gpu` feature.
//!
//! The WGSL shader MUST mirror `dcfr.rs::update_regret_full` exactly. If
//! `dcfr.rs` changes, `SHADER` below must change in lockstep or the GPU
//! path silently diverges from production math.
//!
//! With `feature = "gpu"` off (the default), `GpuState` is a zero-cost stub
//! that allocates nothing and returns empty results. `flush_gpu_batch` in
//! `table.rs` still compiles and remains callable, but does no work.
#![cfg_attr(feature = "gpu", allow(unused))]
#[cfg(feature = "gpu")]
compile_error!(
    "the gpu feature is broken against the current i64 regret-only table \
     (RM_STRIDE=6, i64 cells); the WGSL shader indexes a stride-6 i32 \
     layout. Port gpu.rs to the i64 layout before enabling."
);

use bytemuck::{Pod, Zeroable};

/// One item in a regret-update batch. Shared with the CPU path (used by
/// `flush_cpu_batch` and by `table.rs` bookkeeping). No GPU dependency.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct BatchItem {
    pub index: u32,
    pub action: u32,
    pub iteration: u32,
    pub delta: f32,
}

/// One result row from a GPU batch. Kept always-available so callers do not
/// need to be feature-gated.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct BatchResult {
    pub regret: i32,
    pub momentum: i32,
    pub _pad: [u8; 8], // 16-byte stride, mirrors the WGSL output struct
}

// ==========================================================================
// Real GPU implementation (only when the `gpu` feature is enabled)
// ==========================================================================
#[cfg(feature = "gpu")]
pub use real_gpu::GpuState;

#[cfg(feature = "gpu")]
mod real_gpu {
    use super::{BatchItem, BatchResult};
    use bytemuck;
    use std::borrow::Cow;
    use std::time::Duration;
    use wgpu::*;

    const SHADER: &str = r#"
        struct BatchItem {
            index: u32,
            action: u32,
            iteration: u32,
            delta: f32,
        }

        struct BatchResult {
            regret: i32,
            momentum: i32,
            _pad: u32,
            _pad2: u32,
        }

        @group(0) @binding(0) var<storage, read_write> regrets: array<i32>;
        @group(0) @binding(1) var<storage, read_write> momentums: array<i32>;
        @group(0) @binding(2) var<storage, read> batch: array<BatchItem>;
        @group(0) @binding(3) var<storage, read_write> output: array<BatchResult>;

        // DCFR parameters (must match dcfr.rs):
        //   α = 1.5, β = 0.0, τ = 1000
        // Discount is canonical DCFR: w = t^p / (t^p + 1), bounded in [0.5, 1).
        // The old (t/τ)^p formula was removed after it produced NaN at t≈3000.
        const ALPHA: f32 = 1.5;
        const BETA: f32 = 0.0;
        const TAU: f32 = 1000.0;

        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) id: vec3<u32>) {
            let idx = id.x;
            if idx >= arrayLength(&batch) {
                return;
            }

            let item = batch[idx];
            let flat_idx = item.index * 6u + item.action;

            let SCALE: f32 = 1000.0;
            let t = f32(item.iteration);

            let cur_i = regrets[flat_idx];
            let mom_i = momentums[flat_idx];

            if t == 0.0 {
                regrets[flat_idx] = i32(item.delta * SCALE);
                momentums[flat_idx] = i32(item.delta * SCALE);
                output[idx].regret = i32(item.delta * SCALE);
                output[idx].momentum = i32(item.delta * SCALE);
                return;
            }

            let cur_f = f32(cur_i) / SCALE;
            let mom_f = f32(mom_i) / SCALE;

            let gamma_mom = 1.0 / sqrt(t + 1.0);
            let predicted_delta = (1.0 - gamma_mom) * mom_f + gamma_mom * item.delta;

            let r_pos = max(cur_f, 0.0);
            let r_neg = min(cur_f, 0.0);

            // Canonical DCFR discount: w = t^p / (t^p + 1), bounded in [0.5, 1).
            var w_pos: f32;
            if t < TAU {
                w_pos = 1.0;
            } else {
                let tp = pow(t, ALPHA);
                w_pos = tp / (tp + 1.0);
            }

            var w_neg: f32;
            if t < TAU {
                w_neg = 1.0;
            } else {
                let tp = pow(t, BETA);
                w_neg = tp / (tp + 1.0);
            }

            let discounted_regret = w_pos * r_pos + w_neg * r_neg;

            let new_regret = max(discounted_regret + predicted_delta, 0.0);
            regrets[flat_idx] = i32(new_regret * SCALE);
            momentums[flat_idx] = i32(predicted_delta * SCALE);
            output[idx].regret = i32(new_regret * SCALE);
            output[idx].momentum = i32(predicted_delta * SCALE);
        }
    "#;

    pub struct GpuState {
        pub device: Device,
        pub queue: Queue,
        pub pipeline: ComputePipeline,
        pub bind_group_layout: BindGroupLayout,
        pub regrets_buffer: Buffer,
        pub momentums_buffer: Buffer,
        pub batch_buffer: Buffer,
        pub output_buffer: Buffer,
        pub staging_output: Buffer,
        pub capacity: usize,
        max_batch_size: usize,
    }

    impl GpuState {
        pub fn new(capacity: usize) -> Self {
            let instance = Instance::new(InstanceDescriptor::new_without_display_handle());
            let adapter =
                pollster::block_on(instance.request_adapter(&RequestAdapterOptions::default()))
                    .expect("No GPU adapter found");

            let (device, queue) = pollster::block_on(adapter.request_device(&DeviceDescriptor {
                label: Some("M1 GPU"),
                required_features: Features::empty(),
                required_limits: Limits::downlevel_defaults(),
                memory_hints: Default::default(),
                experimental_features: ExperimentalFeatures::default(),
                trace: Trace::Off,
            }))
            .expect("Failed to get GPU device");

            let shader = device.create_shader_module(ShaderModuleDescriptor {
                label: Some("PCFR+ Shader"),
                source: ShaderSource::Wgsl(Cow::Borrowed(SHADER)),
            });

            let bind_group_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
                label: Some("CFR Bind Layout"),
                entries: &[
                    BindGroupLayoutEntry {
                        binding: 0,
                        visibility: ShaderStages::COMPUTE,
                        ty: BindingType::Buffer {
                            ty: BufferBindingType::Storage { read_only: false },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    BindGroupLayoutEntry {
                        binding: 1,
                        visibility: ShaderStages::COMPUTE,
                        ty: BindingType::Buffer {
                            ty: BufferBindingType::Storage { read_only: false },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    BindGroupLayoutEntry {
                        binding: 2,
                        visibility: ShaderStages::COMPUTE,
                        ty: BindingType::Buffer {
                            ty: BufferBindingType::Storage { read_only: true },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    BindGroupLayoutEntry {
                        binding: 3,
                        visibility: ShaderStages::COMPUTE,
                        ty: BindingType::Buffer {
                            ty: BufferBindingType::Storage { read_only: false },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                ],
            });

            let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
                label: Some("CFR Pipeline Layout"),
                bind_group_layouts: &[Some(&bind_group_layout)],
                immediate_size: 0,
            });

            let pipeline = device.create_compute_pipeline(&ComputePipelineDescriptor {
                label: Some("CFR Pipeline"),
                layout: Some(&pipeline_layout),
                module: &shader,
                entry_point: Some("main"),
                compilation_options: Default::default(),
                cache: None,
            });

            let buffer_size = (capacity * 6 * std::mem::size_of::<i32>()) as u64;
            let regrets_buffer = device.create_buffer(&BufferDescriptor {
                label: Some("Regrets Buffer"),
                size: buffer_size,
                usage: BufferUsages::STORAGE,
                mapped_at_creation: false,
            });
            let momentums_buffer = device.create_buffer(&BufferDescriptor {
                label: Some("Momentums Buffer"),
                size: buffer_size,
                usage: BufferUsages::STORAGE,
                mapped_at_creation: false,
            });

            let max_batch_size = 1_000_000;
            let batch_size_bytes = (max_batch_size * std::mem::size_of::<BatchItem>()) as u64;
            let batch_buffer = device.create_buffer(&BufferDescriptor {
                label: Some("Batch Input"),
                size: batch_size_bytes,
                usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });

            let output_size_bytes = (max_batch_size * std::mem::size_of::<BatchResult>()) as u64;
            let output_buffer = device.create_buffer(&BufferDescriptor {
                label: Some("Batch Output (GPU)"),
                size: output_size_bytes,
                usage: BufferUsages::STORAGE | BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            });
            let staging_output = device.create_buffer(&BufferDescriptor {
                label: Some("Batch Output (Staging)"),
                size: output_size_bytes,
                usage: BufferUsages::MAP_READ | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });

            Self {
                device,
                queue,
                pipeline,
                bind_group_layout,
                regrets_buffer,
                momentums_buffer,
                batch_buffer,
                output_buffer,
                staging_output,
                capacity,
                max_batch_size,
            }
        }

        pub fn max_batch_size(&self) -> usize {
            self.max_batch_size
        }

        pub fn flush_batch(&self, batch: &[BatchItem]) -> Vec<BatchResult> {
            if batch.is_empty() {
                return Vec::new();
            }
            assert!(batch.len() <= self.max_batch_size, "batch too large");

            self.queue
                .write_buffer(&self.batch_buffer, 0, bytemuck::cast_slice(batch));

            let bind_group = self.device.create_bind_group(&BindGroupDescriptor {
                label: Some("CFR Bind Group"),
                layout: &self.bind_group_layout,
                entries: &[
                    BindGroupEntry {
                        binding: 0,
                        resource: self.regrets_buffer.as_entire_binding(),
                    },
                    BindGroupEntry {
                        binding: 1,
                        resource: self.momentums_buffer.as_entire_binding(),
                    },
                    BindGroupEntry {
                        binding: 2,
                        resource: self.batch_buffer.as_entire_binding(),
                    },
                    BindGroupEntry {
                        binding: 3,
                        resource: self.output_buffer.as_entire_binding(),
                    },
                ],
            });

            let output_byte_len = (batch.len() * std::mem::size_of::<BatchResult>()) as u64;

            let mut encoder = self
                .device
                .create_command_encoder(&CommandEncoderDescriptor::default());
            {
                let mut pass = encoder.begin_compute_pass(&ComputePassDescriptor::default());
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, &bind_group, &[]);
                let workgroups = ((batch.len() as u32) + 63) / 64;
                pass.dispatch_workgroups(workgroups, 1, 1);
            }
            encoder.copy_buffer_to_buffer(
                &self.output_buffer,
                0,
                &self.staging_output,
                0,
                output_byte_len,
            );
            self.queue.submit(std::iter::once(encoder.finish()));

            let (tx, rx) = std::sync::mpsc::sync_channel(1);
            self.staging_output
                .slice(..output_byte_len)
                .map_async(MapMode::Read, move |r| {
                    tx.send(r).unwrap();
                });

            self.device
                .poll(PollType::Wait {
                    submission_index: None,
                    timeout: Some(Duration::from_secs(10)),
                })
                .expect("Failed to poll GPU device");

            rx.recv().unwrap().expect("staging map failed");

            let mut out = Vec::with_capacity(batch.len());
            {
                let mapping = self
                    .staging_output
                    .slice(..output_byte_len)
                    .get_mapped_range();
                let results: &[BatchResult] = bytemuck::cast_slice(&mapping);
                out.extend_from_slice(results);
            }
            self.staging_output.unmap();
            out
        }
    }
}

// ==========================================================================
// Stub GpuState (when the `gpu` feature is off)
// ==========================================================================
#[cfg(not(feature = "gpu"))]
pub struct GpuState;

#[cfg(not(feature = "gpu"))]
impl GpuState {
    /// Construct the stub. Does not allocate and does not touch a GPU.
    pub fn new(_capacity: usize) -> Self {
        Self
    }

    /// Zero capacity for the stub: any call to `flush_batch` is a no-op.
    pub fn max_batch_size(&self) -> usize {
        0
    }

    /// The stub never produces results. `flush_gpu_batch` on the production
    /// path is dead code; if someone enables it and the feature is off,
    /// they get an empty result set rather than a silent mistake.
    pub fn flush_batch(&self, _batch: &[BatchItem]) -> Vec<BatchResult> {
        Vec::new()
    }
}
