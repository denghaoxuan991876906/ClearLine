# DeepFilterNet Background Inference Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move DeepFilterNet ONNX inference out of the real-time audio processing path and expose worker health diagnostics in the existing status tab.

**Architecture:** Keep `HighQualitySuppressor` and `HighQualityBackend` as the public integration point. Replace the current synchronous `DeepFilterNetExperimentalBackend` internals with a single-owner worker thread and a non-blocking realtime bridge that uses bounded standard-library channels. Runtime diagnostics are carried through `SuppressorRuntimeInfo` into `PipelineRuntimeInfo` and rendered by `clearline-app` without changing the tab layout.

**Tech Stack:** Rust 2021, `std::sync::mpsc::sync_channel`, `std::thread`, `std::sync::atomic`, existing `deep_filter` / `tract` feature-gated backend, existing `eframe/egui` UI.

---

## File Map

- Modify `clearline-core/src/suppressor.rs`
  - Add public `SuppressorWorkerDiagnostics` data type.
  - Extend `SuppressorRuntimeInfo` with optional worker diagnostics.
  - Add private DeepFilterNet worker metrics and non-blocking bridge under `#[cfg(feature = "deepfilternet")]`.
  - Replace synchronous `DeepFilterNetExperimentalBackend::process_frame` with queue send / processed-output drain / fallback concealment.
  - Add unit and ignored real-model integration tests.
- Modify `clearline-core/src/lib.rs`
  - Re-export `SuppressorWorkerDiagnostics` for the app and tests.
- Modify `clearline-app/src/main.rs`
  - Treat `deepfilternet-tract-worker` as DeepFilterNet enabled.
  - Add status rows for inference state, latency, queue, and worker drops.
  - Add app unit tests for the new Chinese labels.
- Modify `README.md` and `docs/mvp.md`
  - Update current status from synchronous DeepFilterNet inference to background worker inference.
  - Keep the next-step note focused on tuning, model packaging, and latency testing.
- Build artifacts after final verification
  - Rebuild Windows release exe and copy to `dist/ClearLine.exe`.

---

### Task 1: Add worker diagnostics to suppressor runtime info

**Files:**
- Modify: `clearline-core/src/suppressor.rs`
- Modify: `clearline-core/src/lib.rs`

- [ ] **Step 1: Write failing tests for diagnostics attachment**

Add these tests inside `#[cfg(test)] mod tests` in `clearline-core/src/suppressor.rs`:

```rust
#[test]
fn runtime_info_can_attach_worker_diagnostics() {
    let diagnostics = SuppressorWorkerDiagnostics::new(3, 1, 3, 2)
        .with_dropped_input_frames(4)
        .with_dropped_output_frames(5)
        .with_late_output_frames(6)
        .with_inference_errors(7)
        .with_last_inference_time_ms(8)
        .with_max_inference_time_ms(9)
        .with_degraded(true);

    let info = SuppressorRuntimeInfo::new(SuppressorMode::HighQuality, "backend", 480, true)
        .with_worker_diagnostics(diagnostics);

    assert_eq!(info.worker_diagnostics(), Some(diagnostics));
    assert_eq!(info.worker_diagnostics().unwrap().input_queue_capacity(), 3);
    assert_eq!(info.worker_diagnostics().unwrap().pending_input_frames(), 1);
    assert_eq!(info.worker_diagnostics().unwrap().output_queue_capacity(), 3);
    assert_eq!(info.worker_diagnostics().unwrap().pending_output_frames(), 2);
    assert_eq!(info.worker_diagnostics().unwrap().dropped_input_frames(), 4);
    assert_eq!(info.worker_diagnostics().unwrap().dropped_output_frames(), 5);
    assert_eq!(info.worker_diagnostics().unwrap().late_output_frames(), 6);
    assert_eq!(info.worker_diagnostics().unwrap().inference_errors(), 7);
    assert_eq!(info.worker_diagnostics().unwrap().last_inference_time_ms(), Some(8));
    assert_eq!(info.worker_diagnostics().unwrap().max_inference_time_ms(), Some(9));
    assert!(info.worker_diagnostics().unwrap().is_degraded());
}

#[test]
fn runtime_info_has_no_worker_diagnostics_by_default() {
    let info = SuppressorRuntimeInfo::new(SuppressorMode::LowLatency, "backend", 480, true);

    assert_eq!(info.worker_diagnostics(), None);
}
```

- [ ] **Step 2: Run tests and verify failure**

Run:

```bash
cargo test -p clearline-core runtime_info_
```

Expected: compile failure mentioning `SuppressorWorkerDiagnostics` or `worker_diagnostics` is not defined.

- [ ] **Step 3: Implement `SuppressorWorkerDiagnostics` and extend runtime info**

In `clearline-core/src/suppressor.rs`, insert this public type above `SuppressorRuntimeInfo`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SuppressorWorkerDiagnostics {
    input_queue_capacity: usize,
    pending_input_frames: usize,
    output_queue_capacity: usize,
    pending_output_frames: usize,
    dropped_input_frames: u64,
    dropped_output_frames: u64,
    late_output_frames: u64,
    inference_errors: u64,
    last_inference_time_ms: Option<u32>,
    max_inference_time_ms: Option<u32>,
    degraded: bool,
}

impl SuppressorWorkerDiagnostics {
    pub fn new(
        input_queue_capacity: usize,
        pending_input_frames: usize,
        output_queue_capacity: usize,
        pending_output_frames: usize,
    ) -> Self {
        Self {
            input_queue_capacity,
            pending_input_frames,
            output_queue_capacity,
            pending_output_frames,
            dropped_input_frames: 0,
            dropped_output_frames: 0,
            late_output_frames: 0,
            inference_errors: 0,
            last_inference_time_ms: None,
            max_inference_time_ms: None,
            degraded: false,
        }
    }

    pub fn input_queue_capacity(self) -> usize {
        self.input_queue_capacity
    }

    pub fn pending_input_frames(self) -> usize {
        self.pending_input_frames
    }

    pub fn output_queue_capacity(self) -> usize {
        self.output_queue_capacity
    }

    pub fn pending_output_frames(self) -> usize {
        self.pending_output_frames
    }

    pub fn dropped_input_frames(self) -> u64 {
        self.dropped_input_frames
    }

    pub fn dropped_output_frames(self) -> u64 {
        self.dropped_output_frames
    }

    pub fn late_output_frames(self) -> u64 {
        self.late_output_frames
    }

    pub fn inference_errors(self) -> u64 {
        self.inference_errors
    }

    pub fn last_inference_time_ms(self) -> Option<u32> {
        self.last_inference_time_ms
    }

    pub fn max_inference_time_ms(self) -> Option<u32> {
        self.max_inference_time_ms
    }

    pub fn is_degraded(self) -> bool {
        self.degraded
    }

    pub fn with_dropped_input_frames(mut self, count: u64) -> Self {
        self.dropped_input_frames = count;
        self
    }

    pub fn with_dropped_output_frames(mut self, count: u64) -> Self {
        self.dropped_output_frames = count;
        self
    }

    pub fn with_late_output_frames(mut self, count: u64) -> Self {
        self.late_output_frames = count;
        self
    }

    pub fn with_inference_errors(mut self, count: u64) -> Self {
        self.inference_errors = count;
        self
    }

    pub fn with_last_inference_time_ms(mut self, millis: u32) -> Self {
        self.last_inference_time_ms = Some(millis);
        self
    }

    pub fn with_max_inference_time_ms(mut self, millis: u32) -> Self {
        self.max_inference_time_ms = Some(millis);
        self
    }

    pub fn with_degraded(mut self, degraded: bool) -> Self {
        self.degraded = degraded;
        self
    }
}
```

Then modify `SuppressorRuntimeInfo`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SuppressorRuntimeInfo {
    mode: SuppressorMode,
    backend_name: &'static str,
    frame_size_samples: usize,
    is_real_noise_suppression: bool,
    strength: Option<SuppressionStrength>,
    worker_diagnostics: Option<SuppressorWorkerDiagnostics>,
}
```

Update `SuppressorRuntimeInfo::new` to initialize `worker_diagnostics: None`:

```rust
Self {
    mode,
    backend_name,
    frame_size_samples,
    is_real_noise_suppression,
    strength: None,
    worker_diagnostics: None,
}
```

Add these methods to `impl SuppressorRuntimeInfo`:

```rust
pub fn with_worker_diagnostics(mut self, diagnostics: SuppressorWorkerDiagnostics) -> Self {
    self.worker_diagnostics = Some(diagnostics);
    self
}

pub fn worker_diagnostics(self) -> Option<SuppressorWorkerDiagnostics> {
    self.worker_diagnostics
}
```

In `clearline-core/src/lib.rs`, update the suppressor re-export block:

```rust
pub use suppressor::{
    create_suppressor, create_suppressor_with_deepfilternet_bundle, AudioFrameFormat,
    BypassSuppressor, DeepFilterNetModelBundle, HighQualitySuppressor, LowLatencySuppressor,
    NoiseSuppressor, SuppressionStrength, SuppressorMode, SuppressorRuntimeInfo,
    SuppressorWorkerDiagnostics,
};
```

- [ ] **Step 4: Run tests and verify pass**

Run:

```bash
cargo test -p clearline-core runtime_info_
```

Expected: both tests pass.

- [ ] **Step 5: Commit**

```bash
git add clearline-core/src/suppressor.rs clearline-core/src/lib.rs
git commit -m "feat: add suppressor worker diagnostics"
```

---

### Task 2: Add private DeepFilterNet worker metrics and non-blocking bridge

**Files:**
- Modify: `clearline-core/src/suppressor.rs`

- [ ] **Step 1: Write failing tests for bridge behavior**

Add these tests inside `#[cfg(test)] mod tests` in `clearline-core/src/suppressor.rs`:

```rust
#[cfg(feature = "deepfilternet")]
#[test]
fn deepfilternet_bridge_falls_back_without_processed_output() {
    let (input_sender, _input_receiver) = std::sync::mpsc::sync_channel(1);
    let (_output_sender, output_receiver) = std::sync::mpsc::sync_channel(1);
    let metrics = std::sync::Arc::new(DeepFilterNetWorkerMetrics::new(1, 1));
    let mut bridge = DeepFilterNetRealtimeBridge::new(input_sender, output_receiver, metrics, 4);
    let input = [0.1, -0.2, 0.3, -0.4];
    let mut output = [0.0; 4];

    bridge.process_frame(&input, &mut output).unwrap();

    assert_eq!(output, input);
    let diagnostics = bridge.diagnostics();
    assert_eq!(diagnostics.pending_input_frames(), 1);
    assert_eq!(diagnostics.late_output_frames(), 1);
    assert_eq!(diagnostics.dropped_input_frames(), 0);
}

#[cfg(feature = "deepfilternet")]
#[test]
fn deepfilternet_bridge_drops_input_when_queue_is_full() {
    let (input_sender, _input_receiver) = std::sync::mpsc::sync_channel(1);
    let (_output_sender, output_receiver) = std::sync::mpsc::sync_channel(1);
    let metrics = std::sync::Arc::new(DeepFilterNetWorkerMetrics::new(1, 1));
    let mut bridge = DeepFilterNetRealtimeBridge::new(input_sender, output_receiver, metrics, 2);
    let input = [0.25, -0.25];
    let mut output = [0.0; 2];

    bridge.process_frame(&input, &mut output).unwrap();
    bridge.process_frame(&input, &mut output).unwrap();

    let diagnostics = bridge.diagnostics();
    assert_eq!(diagnostics.pending_input_frames(), 1);
    assert_eq!(diagnostics.dropped_input_frames(), 1);
    assert_eq!(diagnostics.late_output_frames(), 2);
}

#[cfg(feature = "deepfilternet")]
#[test]
fn deepfilternet_bridge_uses_latest_processed_output() {
    let (input_sender, _input_receiver) = std::sync::mpsc::sync_channel(1);
    let (output_sender, output_receiver) = std::sync::mpsc::sync_channel(2);
    let metrics = std::sync::Arc::new(DeepFilterNetWorkerMetrics::new(1, 2));
    let mut bridge = DeepFilterNetRealtimeBridge::new(input_sender, output_receiver, metrics.clone(), 2);
    let input = [0.1, 0.2];
    let mut output = [0.0; 2];

    output_sender.try_send(vec![0.7, -0.7]).unwrap();
    metrics.record_output_enqueued();

    bridge.process_frame(&input, &mut output).unwrap();

    assert_eq!(output, [0.7, -0.7]);
    let diagnostics = bridge.diagnostics();
    assert_eq!(diagnostics.pending_output_frames(), 0);
    assert_eq!(diagnostics.late_output_frames(), 0);
}
```

- [ ] **Step 2: Run tests and verify failure**

Run:

```bash
cargo test -p clearline-core --features deepfilternet deepfilternet_bridge_
```

Expected: compile failure mentioning `DeepFilterNetWorkerMetrics` or `DeepFilterNetRealtimeBridge` is not defined.

- [ ] **Step 3: Add imports for worker internals**

At the top of `clearline-core/src/suppressor.rs`, change the feature-gated std imports to:

```rust
#[cfg(feature = "deepfilternet")]
use std::{
    fs, io,
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        mpsc::{Receiver, SyncSender, TryRecvError, TrySendError},
        Arc,
    },
};
```

- [ ] **Step 4: Implement worker metrics and bridge**

Add this code below `unsafe impl Send for SendDfTract {}` in `clearline-core/src/suppressor.rs`:

```rust
#[cfg(feature = "deepfilternet")]
const DEEPFILTERNET_WORKER_QUEUE_CAPACITY: usize = 3;

#[cfg(feature = "deepfilternet")]
struct DeepFilterNetWorkerMetrics {
    input_queue_capacity: usize,
    output_queue_capacity: usize,
    pending_input_frames: AtomicUsize,
    pending_output_frames: AtomicUsize,
    dropped_input_frames: AtomicU64,
    dropped_output_frames: AtomicU64,
    late_output_frames: AtomicU64,
    inference_errors: AtomicU64,
    last_inference_time_ms: AtomicU64,
    max_inference_time_ms: AtomicU64,
    degraded: AtomicBool,
}

#[cfg(feature = "deepfilternet")]
impl DeepFilterNetWorkerMetrics {
    fn new(input_queue_capacity: usize, output_queue_capacity: usize) -> Self {
        Self {
            input_queue_capacity,
            output_queue_capacity,
            pending_input_frames: AtomicUsize::new(0),
            pending_output_frames: AtomicUsize::new(0),
            dropped_input_frames: AtomicU64::new(0),
            dropped_output_frames: AtomicU64::new(0),
            late_output_frames: AtomicU64::new(0),
            inference_errors: AtomicU64::new(0),
            last_inference_time_ms: AtomicU64::new(0),
            max_inference_time_ms: AtomicU64::new(0),
            degraded: AtomicBool::new(false),
        }
    }

    fn record_input_enqueued(&self) {
        self.pending_input_frames.fetch_add(1, Ordering::Relaxed);
    }

    fn record_input_dequeued(&self) {
        saturating_fetch_sub(&self.pending_input_frames, 1);
    }

    fn record_output_enqueued(&self) {
        self.pending_output_frames.fetch_add(1, Ordering::Relaxed);
    }

    fn record_output_dequeued(&self) {
        saturating_fetch_sub(&self.pending_output_frames, 1);
    }

    fn record_dropped_input(&self) {
        self.dropped_input_frames.fetch_add(1, Ordering::Relaxed);
    }

    fn record_dropped_output(&self) {
        self.dropped_output_frames.fetch_add(1, Ordering::Relaxed);
    }

    fn record_late_output(&self) {
        self.late_output_frames.fetch_add(1, Ordering::Relaxed);
    }

    fn record_inference_error(&self) {
        self.inference_errors.fetch_add(1, Ordering::Relaxed);
    }

    fn record_inference_time_ms(&self, millis: u32) {
        let millis = u64::from(millis.max(1));
        self.last_inference_time_ms.store(millis, Ordering::Relaxed);
        let mut current = self.max_inference_time_ms.load(Ordering::Relaxed);
        while millis > current {
            match self.max_inference_time_ms.compare_exchange(
                current,
                millis,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(actual) => current = actual,
            }
        }
    }

    fn mark_degraded(&self) {
        self.degraded.store(true, Ordering::Relaxed);
    }

    fn snapshot(&self) -> SuppressorWorkerDiagnostics {
        let last = nonzero_u64_to_u32(self.last_inference_time_ms.load(Ordering::Relaxed));
        let max = nonzero_u64_to_u32(self.max_inference_time_ms.load(Ordering::Relaxed));
        let mut diagnostics = SuppressorWorkerDiagnostics::new(
            self.input_queue_capacity,
            self.pending_input_frames.load(Ordering::Relaxed),
            self.output_queue_capacity,
            self.pending_output_frames.load(Ordering::Relaxed),
        )
        .with_dropped_input_frames(self.dropped_input_frames.load(Ordering::Relaxed))
        .with_dropped_output_frames(self.dropped_output_frames.load(Ordering::Relaxed))
        .with_late_output_frames(self.late_output_frames.load(Ordering::Relaxed))
        .with_inference_errors(self.inference_errors.load(Ordering::Relaxed))
        .with_degraded(self.degraded.load(Ordering::Relaxed));

        if let Some(last) = last {
            diagnostics = diagnostics.with_last_inference_time_ms(last);
        }
        if let Some(max) = max {
            diagnostics = diagnostics.with_max_inference_time_ms(max);
        }
        diagnostics
    }
}

#[cfg(feature = "deepfilternet")]
fn saturating_fetch_sub(value: &AtomicUsize, amount: usize) {
    let mut current = value.load(Ordering::Relaxed);
    loop {
        let next = current.saturating_sub(amount);
        match value.compare_exchange(current, next, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => break,
            Err(actual) => current = actual,
        }
    }
}

#[cfg(feature = "deepfilternet")]
fn nonzero_u64_to_u32(value: u64) -> Option<u32> {
    if value == 0 {
        None
    } else {
        Some(value.min(u64::from(u32::MAX)) as u32)
    }
}

#[cfg(feature = "deepfilternet")]
struct DeepFilterNetRealtimeBridge {
    input_sender: SyncSender<Vec<f32>>,
    output_receiver: Receiver<Vec<f32>>,
    metrics: Arc<DeepFilterNetWorkerMetrics>,
    frame_size_samples: usize,
    last_output: Vec<f32>,
    has_last_output: bool,
}

#[cfg(feature = "deepfilternet")]
impl DeepFilterNetRealtimeBridge {
    fn new(
        input_sender: SyncSender<Vec<f32>>,
        output_receiver: Receiver<Vec<f32>>,
        metrics: Arc<DeepFilterNetWorkerMetrics>,
        frame_size_samples: usize,
    ) -> Self {
        Self {
            input_sender,
            output_receiver,
            metrics,
            frame_size_samples,
            last_output: vec![0.0; frame_size_samples],
            has_last_output: false,
        }
    }

    fn process_frame(&mut self, input: &[f32], output: &mut [f32]) -> ClearLineResult<()> {
        if input.len() != self.frame_size_samples || output.len() != self.frame_size_samples {
            return Err(ClearLineError::BufferSizeMismatch {
                input: input.len(),
                output: output.len(),
            });
        }

        let newest_output = self.drain_processed_outputs();
        match self.input_sender.try_send(input.to_vec()) {
            Ok(()) => self.metrics.record_input_enqueued(),
            Err(TrySendError::Full(_)) => self.metrics.record_dropped_input(),
            Err(TrySendError::Disconnected(_)) => {
                self.metrics.record_dropped_input();
                self.metrics.mark_degraded();
            }
        }

        if let Some(samples) = newest_output {
            output.copy_from_slice(&samples);
            self.last_output.copy_from_slice(&samples);
            self.has_last_output = true;
            return Ok(());
        }

        self.metrics.record_late_output();
        if self.has_last_output {
            output.copy_from_slice(&self.last_output);
        } else {
            output.copy_from_slice(input);
        }
        Ok(())
    }

    fn reset(&mut self) {
        self.drain_processed_outputs();
        self.last_output.fill(0.0);
        self.has_last_output = false;
    }

    fn diagnostics(&self) -> SuppressorWorkerDiagnostics {
        self.metrics.snapshot()
    }

    fn drain_processed_outputs(&mut self) -> Option<Vec<f32>> {
        let mut newest = None;
        loop {
            match self.output_receiver.try_recv() {
                Ok(samples) => {
                    self.metrics.record_output_dequeued();
                    if samples.len() == self.frame_size_samples {
                        newest = Some(samples);
                    } else {
                        self.metrics.record_dropped_output();
                        self.metrics.mark_degraded();
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.metrics.mark_degraded();
                    break;
                }
            }
        }
        newest
    }
}
```

- [ ] **Step 5: Run tests and verify pass**

Run:

```bash
cargo test -p clearline-core --features deepfilternet deepfilternet_bridge_
```

Expected: 3 tests pass.

- [ ] **Step 6: Commit**

```bash
git add clearline-core/src/suppressor.rs
git commit -m "feat: add DeepFilterNet realtime bridge"
```

---

### Task 3: Move DeepFilterNet processing into a worker thread

**Files:**
- Modify: `clearline-core/src/suppressor.rs`

- [ ] **Step 1: Update the real-model integration test expectation first**

In `high_quality_runs_downloaded_deepfilternet_model`, change the backend expectation and add diagnostics assertions:

```rust
assert_eq!(info.backend_name(), "deepfilternet-tract-worker");
assert_eq!(info.frame_size_samples(), 480);
assert!(info.is_real_noise_suppression());
assert_eq!(info.worker_diagnostics().unwrap().last_inference_time_ms(), None);

let input = vec![0.01; info.frame_size_samples()];
let mut output = vec![0.0; info.frame_size_samples()];
suppressor.process(&input, &mut output).unwrap();
std::thread::sleep(std::time::Duration::from_millis(50));
suppressor.process(&input, &mut output).unwrap();

let diagnostics = suppressor.runtime_info().worker_diagnostics().unwrap();
assert!(output.iter().all(|sample| sample.is_finite()));
assert_eq!(output.len(), input.len());
assert!(diagnostics.last_inference_time_ms().is_some());
```

- [ ] **Step 2: Run ignored test and verify failure**

Run:

```bash
CLEARLINE_DF_MODEL_DIR='/mnt/e/Dev/模型onnx' cargo test -p clearline-core --features deepfilternet high_quality_runs_downloaded_deepfilternet_model -- --ignored --nocapture
```

Expected: failure because backend is still `deepfilternet-tract` or worker diagnostics are missing.

- [ ] **Step 3: Add thread imports**

Extend the feature-gated imports in `clearline-core/src/suppressor.rs` to include thread and time:

```rust
#[cfg(feature = "deepfilternet")]
use std::{
    fs, io,
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        mpsc::{self, Receiver, Sender, SyncSender, TryRecvError, TrySendError},
        Arc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
```

- [ ] **Step 4: Replace `DeepFilterNetExperimentalBackend` fields**

Replace the current struct definition:

```rust
#[cfg(feature = "deepfilternet")]
struct DeepFilterNetExperimentalBackend {
    bridge: DeepFilterNetRealtimeBridge,
    stop_sender: Option<Sender<()>>,
    worker: Option<JoinHandle<()>>,
    channels: usize,
    hop_size: usize,
}
```

- [ ] **Step 5: Replace backend constructor with worker creation**

Replace `DeepFilterNetExperimentalBackend::new` with:

```rust
fn new(
    format: AudioFrameFormat,
    strength: SuppressionStrength,
    model_bundle: DeepFilterNetModelBundle,
) -> ClearLineResult<Self> {
    let channels = usize::from(format.channels().max(1));
    let params_file = deepfilternet_bundle_as_temp_targz(&model_bundle)?;
    let df_params_result = deepfilternet_load_params(params_file.clone());
    let _ = fs::remove_file(params_file);
    let df_params = df_params_result?;
    let runtime_params = deepfilternet_runtime_params(channels, strength);
    let model = deepfilternet_create_model(df_params, &runtime_params)?;

    if model.sr as u32 != format.sample_rate_hz() {
        return Err(ClearLineError::ModelLoad(format!(
            "DeepFilterNet model sample rate is {} Hz, input is {} Hz",
            model.sr,
            format.sample_rate_hz()
        )));
    }

    let hop_size = model.hop_size;
    let frame_size_samples = hop_size * channels;
    let metrics = Arc::new(DeepFilterNetWorkerMetrics::new(
        DEEPFILTERNET_WORKER_QUEUE_CAPACITY,
        DEEPFILTERNET_WORKER_QUEUE_CAPACITY,
    ));
    let (input_sender, input_receiver) = mpsc::sync_channel(DEEPFILTERNET_WORKER_QUEUE_CAPACITY);
    let (output_sender, output_receiver) = mpsc::sync_channel(DEEPFILTERNET_WORKER_QUEUE_CAPACITY);
    let (stop_sender, stop_receiver) = mpsc::channel();
    let worker_metrics = metrics.clone();
    let worker = thread::Builder::new()
        .name("clearline-deepfilternet".to_owned())
        .spawn(move || {
            deepfilternet_worker_loop(
                SendDfTract(model),
                channels,
                hop_size,
                input_receiver,
                output_sender,
                stop_receiver,
                worker_metrics,
            );
        })
        .map_err(|error| ClearLineError::ModelLoad(error.to_string()))?;

    Ok(Self {
        bridge: DeepFilterNetRealtimeBridge::new(
            input_sender,
            output_receiver,
            metrics,
            frame_size_samples,
        ),
        stop_sender: Some(stop_sender),
        worker: Some(worker),
        channels,
        hop_size,
    })
}
```

- [ ] **Step 6: Replace backend methods**

Replace `name`, `process_frame`, `reset`, and `frame_size_samples` in the DeepFilterNet backend impl with:

```rust
fn name(&self) -> &'static str {
    "deepfilternet-tract-worker"
}

fn is_real_noise_suppression(&self) -> bool {
    true
}

fn process_frame(&mut self, input: &[f32], output: &mut [f32]) -> ClearLineResult<()> {
    if self.worker.as_ref().is_some_and(|worker| worker.is_finished()) {
        self.bridge.metrics.mark_degraded();
    }
    self.bridge.process_frame(input, output)
}

fn reset(&mut self) {
    self.bridge.reset();
}

fn frame_size_samples(&self) -> usize {
    self.hop_size * self.channels
}

fn diagnostics(&self) -> SuppressorWorkerDiagnostics {
    self.bridge.diagnostics()
}
```

- [ ] **Step 7: Add worker shutdown**

Below the backend impl, add:

```rust
#[cfg(feature = "deepfilternet")]
impl Drop for DeepFilterNetExperimentalBackend {
    fn drop(&mut self) {
        if let Some(stop_sender) = self.stop_sender.take() {
            let _ = stop_sender.send(());
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
```

- [ ] **Step 8: Add worker loop and frame conversion helpers**

Add below the `Drop` impl:

```rust
#[cfg(feature = "deepfilternet")]
fn deepfilternet_worker_loop(
    mut model: SendDfTract,
    channels: usize,
    hop_size: usize,
    input_receiver: Receiver<Vec<f32>>,
    output_sender: SyncSender<Vec<f32>>,
    stop_receiver: Receiver<()>,
    metrics: Arc<DeepFilterNetWorkerMetrics>,
) {
    let mut input_frame = Array2::zeros((channels, hop_size));
    let mut output_frame = Array2::zeros((channels, hop_size));
    let frame_size_samples = channels * hop_size;

    loop {
        if stop_receiver.try_recv().is_ok() {
            break;
        }

        let samples = match input_receiver.recv_timeout(Duration::from_millis(10)) {
            Ok(samples) => {
                metrics.record_input_dequeued();
                samples
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        };

        if samples.len() != frame_size_samples {
            metrics.record_inference_error();
            metrics.mark_degraded();
            continue;
        }

        deinterleave_deepfilternet_frame(&samples, channels, hop_size, &mut input_frame);
        let started = Instant::now();
        let result = model.0.process(input_frame.view(), output_frame.view_mut());
        let elapsed_ms = started.elapsed().as_millis().max(1).min(u128::from(u32::MAX)) as u32;
        metrics.record_inference_time_ms(elapsed_ms);

        if result.is_err() {
            metrics.record_inference_error();
            metrics.mark_degraded();
            continue;
        }

        let mut processed = vec![0.0; frame_size_samples];
        interleave_deepfilternet_frame(output_frame.view(), channels, hop_size, &mut processed);
        match output_sender.try_send(processed) {
            Ok(()) => metrics.record_output_enqueued(),
            Err(TrySendError::Full(_)) => metrics.record_dropped_output(),
            Err(TrySendError::Disconnected(_)) => break,
        }
    }
}

#[cfg(feature = "deepfilternet")]
fn deinterleave_deepfilternet_frame(
    input: &[f32],
    channels: usize,
    hop_size: usize,
    output: &mut Array2<f32>,
) {
    for frame_index in 0..hop_size {
        let input_offset = frame_index * channels;
        for channel in 0..channels {
            output[[channel, frame_index]] = input[input_offset + channel];
        }
    }
}

#[cfg(feature = "deepfilternet")]
fn interleave_deepfilternet_frame(
    input: ndarray::ArrayView2<'_, f32>,
    channels: usize,
    hop_size: usize,
    output: &mut [f32],
) {
    for frame_index in 0..hop_size {
        let output_offset = frame_index * channels;
        for channel in 0..channels {
            output[output_offset + channel] = input[[channel, frame_index]].clamp(-1.0, 1.0);
        }
    }
}
```

- [ ] **Step 9: Attach diagnostics through `HighQualityBackend` and `HighQualitySuppressor`**

Add this method to `impl HighQualityBackend`:

```rust
fn worker_diagnostics(&self) -> Option<SuppressorWorkerDiagnostics> {
    match self {
        Self::Adaptive(_) => None,
        #[cfg(feature = "deepfilternet")]
        Self::DeepFilterNet(backend) => Some(backend.diagnostics()),
    }
}
```

Update `HighQualitySuppressor::runtime_info` to attach diagnostics:

```rust
let mut info = SuppressorRuntimeInfo::new(
    self.mode(),
    self.backend_name(),
    self.frame_size_samples(),
    self.backend.is_real_noise_suppression(),
)
.with_strength(self.strength);

if let Some(diagnostics) = self.backend.worker_diagnostics() {
    info = info.with_worker_diagnostics(diagnostics);
}

info
```

- [ ] **Step 10: Run tests and verify pass**

Run:

```bash
cargo test -p clearline-core --features deepfilternet deepfilternet_bridge_
CLEARLINE_DF_MODEL_DIR='/mnt/e/Dev/模型onnx' cargo test -p clearline-core --features deepfilternet high_quality_runs_downloaded_deepfilternet_model -- --ignored --nocapture
```

Expected: bridge tests pass; ignored real-model test passes and reports backend `deepfilternet-tract-worker`.

- [ ] **Step 11: Commit**

```bash
git add clearline-core/src/suppressor.rs
git commit -m "feat: run DeepFilterNet inference on worker thread"
```

---

### Task 4: Render worker diagnostics in the status tab

**Files:**
- Modify: `clearline-app/src/main.rs`

- [ ] **Step 1: Write failing app tests for labels**

Add these tests in `#[cfg(test)] mod tests` in `clearline-app/src/main.rs`:

```rust
#[test]
fn deepfilternet_worker_status_reports_real_backend() {
    let info = PipelineRuntimeInfo::new(
        clearline_core::AudioFrameFormat::new(48_000, 1),
        clearline_core::AudioFrameFormat::new(48_000, 2),
        clearline_core::SuppressorRuntimeInfo::new(
            SuppressorMode::HighQuality,
            "deepfilternet-tract-worker",
            480,
            true,
        )
        .with_strength(clearline_core::SuppressionStrength::Balanced)
        .with_worker_diagnostics(
            clearline_core::SuppressorWorkerDiagnostics::new(3, 1, 3, 0)
                .with_last_inference_time_ms(7)
                .with_max_inference_time_ms(11),
        ),
    );

    assert_eq!(noise_suppression_status_label(Some(&info)), "DeepFilterNet 已启用");
    assert_eq!(inference_health_label(Some(&info)), "稳定");
    assert_eq!(inference_latency_label(Some(&info)), "最近 7ms / 最大 11ms");
    assert_eq!(inference_queue_label(Some(&info)), "输入 1/3 · 输出 0/3");
    assert_eq!(inference_drop_label(Some(&info)), "输入丢弃 0 · 输出丢弃 0 · 迟到 0");
}

#[test]
fn deepfilternet_worker_status_reports_slow_or_degraded_state() {
    let slow_info = PipelineRuntimeInfo::new(
        clearline_core::AudioFrameFormat::new(48_000, 1),
        clearline_core::AudioFrameFormat::new(48_000, 2),
        clearline_core::SuppressorRuntimeInfo::new(
            SuppressorMode::HighQuality,
            "deepfilternet-tract-worker",
            480,
            true,
        )
        .with_worker_diagnostics(
            clearline_core::SuppressorWorkerDiagnostics::new(3, 3, 3, 2)
                .with_late_output_frames(4)
                .with_last_inference_time_ms(35)
                .with_max_inference_time_ms(42),
        ),
    );
    assert_eq!(inference_health_label(Some(&slow_info)), "推理偏慢");

    let degraded_info = PipelineRuntimeInfo::new(
        clearline_core::AudioFrameFormat::new(48_000, 1),
        clearline_core::AudioFrameFormat::new(48_000, 2),
        clearline_core::SuppressorRuntimeInfo::new(
            SuppressorMode::HighQuality,
            "deepfilternet-tract-worker",
            480,
            true,
        )
        .with_worker_diagnostics(
            clearline_core::SuppressorWorkerDiagnostics::new(3, 0, 3, 0)
                .with_inference_errors(1)
                .with_degraded(true),
        ),
    );
    assert_eq!(inference_health_label(Some(&degraded_info)), "已降级");
}
```

- [ ] **Step 2: Run tests and verify failure**

Run:

```bash
cargo test -p clearline-app deepfilternet_worker_status
```

Expected: compile failure because the new helper labels do not exist and `deepfilternet-tract-worker` is not mapped.

- [ ] **Step 3: Update DeepFilterNet label mapping**

In `noise_suppression_status_label`, change the DeepFilterNet match arm to:

```rust
"deepfilternet" | "deepfilternet-tract" | "deepfilternet-tract-worker" => {
    "DeepFilterNet 已启用".to_owned()
}
```

- [ ] **Step 4: Add diagnostic rows to `processing_card`**

After the existing frame-size row in `processing_card`, add:

```rust
ui.add_space(6.0);
info_row(ui, "推理状态", inference_health_label(runtime_info));
ui.add_space(6.0);
info_row(ui, "推理延迟", inference_latency_label(runtime_info));
ui.add_space(6.0);
info_row(ui, "推理队列", inference_queue_label(runtime_info));
ui.add_space(6.0);
info_row(ui, "推理丢帧", inference_drop_label(runtime_info));
```

- [ ] **Step 5: Add helper label functions**

Add these functions near `frame_size_label`:

```rust
fn inference_health_label(runtime_info: Option<&PipelineRuntimeInfo>) -> String {
    let Some(diagnostics) = runtime_info.and_then(|info| info.suppressor().worker_diagnostics()) else {
        return "未使用后台推理".to_owned();
    };

    if diagnostics.is_degraded() || diagnostics.inference_errors() > 0 {
        "已降级".to_owned()
    } else if diagnostics.late_output_frames() > 0
        || diagnostics.dropped_input_frames() > 0
        || diagnostics.dropped_output_frames() > 0
    {
        "推理偏慢".to_owned()
    } else {
        "稳定".to_owned()
    }
}

fn inference_latency_label(runtime_info: Option<&PipelineRuntimeInfo>) -> String {
    let Some(diagnostics) = runtime_info.and_then(|info| info.suppressor().worker_diagnostics()) else {
        return "未使用后台推理".to_owned();
    };

    match (
        diagnostics.last_inference_time_ms(),
        diagnostics.max_inference_time_ms(),
    ) {
        (Some(last), Some(max)) => format!("最近 {last}ms / 最大 {max}ms"),
        (Some(last), None) => format!("最近 {last}ms / 最大 --"),
        (None, Some(max)) => format!("最近 -- / 最大 {max}ms"),
        (None, None) => "等待首帧".to_owned(),
    }
}

fn inference_queue_label(runtime_info: Option<&PipelineRuntimeInfo>) -> String {
    let Some(diagnostics) = runtime_info.and_then(|info| info.suppressor().worker_diagnostics()) else {
        return "未使用后台推理".to_owned();
    };

    format!(
        "输入 {}/{} · 输出 {}/{}",
        diagnostics.pending_input_frames(),
        diagnostics.input_queue_capacity(),
        diagnostics.pending_output_frames(),
        diagnostics.output_queue_capacity()
    )
}

fn inference_drop_label(runtime_info: Option<&PipelineRuntimeInfo>) -> String {
    let Some(diagnostics) = runtime_info.and_then(|info| info.suppressor().worker_diagnostics()) else {
        return "未使用后台推理".to_owned();
    };

    format!(
        "输入丢弃 {} · 输出丢弃 {} · 迟到 {}",
        diagnostics.dropped_input_frames(),
        diagnostics.dropped_output_frames(),
        diagnostics.late_output_frames()
    )
}
```

- [ ] **Step 6: Run app tests and verify pass**

Run:

```bash
cargo test -p clearline-app deepfilternet_worker_status
cargo test -p clearline-app
```

Expected: targeted tests pass; all app tests pass.

- [ ] **Step 7: Commit**

```bash
git add clearline-app/src/main.rs
git commit -m "ui: show DeepFilterNet worker diagnostics"
```

---

### Task 5: Update docs and full verification

**Files:**
- Modify: `README.md`
- Modify: `docs/mvp.md`

- [ ] **Step 1: Update README current status**

In `README.md`, replace the sentence that says DeepFilterNet currently uses synchronous inference with:

```markdown
`deepfilternet` feature 下已接入 DeepFilterNet ONNX 模型调用，当前使用官方 `deep_filter` / `tract` 后端，并通过 `deepfilternet-tract-worker` 后台推理线程运行模型，避免在音频处理路径里直接阻塞。
```

In the next-step list, replace the background-thread item with:

```markdown
1. 根据高质量模式实测结果继续调整后台推理队列容量、迟到帧补偿策略和状态页诊断阈值。
```

- [ ] **Step 2: Update MVP doc current status**

In `docs/mvp.md`, replace the sentence that says current implementation synchronously calls the model with:

```markdown
当前实现已从同步调用模型迁移到 `deepfilternet-tract-worker` 后台推理线程：音频路径只做非阻塞入队、处理结果读取和迟到帧保护，状态页显示推理延迟、队列水位、迟到帧和丢弃帧。下一步需要根据真实设备测试继续调队列容量、迟到帧补偿策略和模型资源发布方式。
```

- [ ] **Step 3: Run full WSL verification**

Run:

```bash
cargo fmt
cargo fmt --check
cargo check
cargo test -p clearline-core
cargo test -p clearline-core --features rnnoise
cargo test -p clearline-core --features deepfilternet
cargo test -p clearline-core --features rnnoise,deepfilternet
cargo test -p clearline-app
CLEARLINE_DF_MODEL_DIR='/mnt/e/Dev/模型onnx' cargo test -p clearline-core --features deepfilternet high_quality_runs_downloaded_deepfilternet_model -- --ignored --nocapture
cargo check -p clearline-app --no-default-features
```

Expected: all commands exit 0. The ignored real-model test must show `1 passed`.

- [ ] **Step 4: Run Windows verification and build release exe**

Run:

```bash
'/mnt/c/Users/DHX/.cargo/bin/cargo.exe' check -p clearline-app
'/mnt/c/Users/DHX/.cargo/bin/cargo.exe' check -p clearline-app --no-default-features
'/mnt/c/Users/DHX/.cargo/bin/cargo.exe' build -p clearline-app --release
mkdir -p dist
cp target/release/clearline-app.exe dist/ClearLine.exe
cp target/release/clearline-app.exe dist/ClearLine-deepfilternet-worker.exe
file dist/ClearLine.exe dist/ClearLine-deepfilternet-worker.exe
```

Expected: Windows checks and release build exit 0; `file` reports PE32+ GUI x86-64 executables.

- [ ] **Step 5: Commit docs and any final fixes**

```bash
git add README.md docs/mvp.md
git commit -m "docs: update DeepFilterNet worker status"
```

- [ ] **Step 6: Final status check**

Run:

```bash
git status --short
git log --oneline -5
```

Expected: no uncommitted source/doc changes. `dist/` and `*.exe` are ignored by git, so mention the built file paths in the final response instead of trying to commit them.

---

## Manual Testing Instructions After Implementation

1. Run `E:\Dev\ClearLine\dist\ClearLine.exe`.
2. In `设备`, choose the real microphone and output device.
3. Select `高质量降噪`.
4. Set DeepFilterNet model directory to `E:\Dev\模型onnx`.
5. Start processing.
6. Open `状态` tab.
7. Confirm backend is `deepfilternet-tract-worker`.
8. Confirm `降噪` shows `DeepFilterNet 已启用`.
9. Confirm new rows show:
   - `推理状态`
   - `推理延迟`
   - `推理队列`
   - `推理丢帧`
10. Speak normally and create fan/wind noise.
11. Watch whether `推理状态` stays `稳定` or changes to `推理偏慢`.
12. Stop and start again twice. Confirm the app does not hang and status values refresh.
13. If audio becomes robotic, distorted, or frequently shows dropped/late frames, report the visible `推理延迟`, `推理队列`, and `推理丢帧` values.
