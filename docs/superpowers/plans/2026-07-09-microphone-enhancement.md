# Microphone Enhancement Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a default-on automatic microphone enhancement stage that raises quiet speech after noise suppression while limiting peaks to prevent clipping.

**Architecture:** Implement a lightweight `AutoGainProcessor` in `clearline-core::preprocess`, wire it into `AudioPipelineConfig`, `PipelineRuntimeInfo`, and both output paths after the suppressor stage. Expose a simple Chinese UI toggle and persist it in app settings.

**Tech Stack:** Rust, clearline-core audio pipeline, eframe/egui UI, serde settings, existing cargo test/check workflow.

---

## File Structure

- Modify `clearline-core/src/preprocess.rs`: add `AutoGainConfig`, `AutoGainProcessor`, and unit tests.
- Modify `clearline-core/src/lib.rs`: re-export auto gain types.
- Modify `clearline-core/src/pipeline.rs`: add config/runtime flags, create processor, run it after `NoiseSuppressor`, add tests.
- Modify `clearline-app/src/settings.rs`: persist `microphone_boost_enabled`, default true, add tests.
- Modify `clearline-app/src/main.rs`: add app state, settings snapshot/load, pipeline config, device-page toggle, status-page row, tests.
- Modify `README.md` and `docs/mvp.md`: document microphone enhancement behavior.

---

### Task 1: Core AutoGain Processor

**Files:**
- Modify: `clearline-core/src/preprocess.rs`
- Modify: `clearline-core/src/lib.rs`

- [ ] **Step 1: Write failing core tests**

Add tests to `clearline-core/src/preprocess.rs` under the existing `#[cfg(test)] mod tests`:

```rust
#[test]
fn auto_gain_is_enabled_by_default_config_helper() {
    assert!(AutoGainConfig::enabled().is_enabled());
    assert!(!AutoGainConfig::disabled().is_enabled());
}

#[test]
fn auto_gain_increases_quiet_voice_like_samples() {
    let format = AudioFrameFormat::new(48_000, 1);
    let mut processor = AutoGainProcessor::new(format, AutoGainConfig::enabled());
    let mut samples = vec![0.05; 480];
    let before = rms_for_test(&samples);

    processor.process_interleaved(&mut samples);

    assert!(rms_for_test(&samples) > before * 1.2);
}

#[test]
fn auto_gain_does_not_raise_near_silence() {
    let format = AudioFrameFormat::new(48_000, 1);
    let mut processor = AutoGainProcessor::new(format, AutoGainConfig::enabled());
    let mut samples = vec![0.001; 480];

    processor.process_interleaved(&mut samples);

    assert!(rms_for_test(&samples) < 0.002);
}

#[test]
fn auto_gain_limiter_keeps_output_below_ceiling() {
    let format = AudioFrameFormat::new(48_000, 1);
    let mut processor = AutoGainProcessor::new(format, AutoGainConfig::enabled());
    let mut samples = vec![2.0; 480];

    processor.process_interleaved(&mut samples);

    assert!(samples.iter().all(|sample| sample.is_finite()));
    assert!(samples.iter().all(|sample| sample.abs() <= 0.8911));
}

#[test]
fn auto_gain_disabled_passes_samples_through() {
    let format = AudioFrameFormat::new(48_000, 1);
    let mut processor = AutoGainProcessor::new(format, AutoGainConfig::disabled());
    let mut samples = vec![0.05, -0.1, 0.2, -0.3];
    let before = samples.clone();

    processor.process_interleaved(&mut samples);

    assert_eq!(samples, before);
}

fn rms_for_test(samples: &[f32]) -> f32 {
    let sum = samples.iter().map(|sample| sample * sample).sum::<f32>();
    (sum / samples.len().max(1) as f32).sqrt()
}
```

- [ ] **Step 2: Run failing tests**

Run:

```bash
cargo test -p clearline-core auto_gain_ -- --nocapture
```

Expected: fail to compile because `AutoGainConfig` and `AutoGainProcessor` do not exist.

- [ ] **Step 3: Implement AutoGain**

Add to `clearline-core/src/preprocess.rs` above `WindNoiseConfig`:

```rust
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AutoGainConfig {
    enabled: bool,
    target_rms: f32,
    silence_rms: f32,
    max_gain: f32,
    limiter_ceiling: f32,
    attack: f32,
    release: f32,
}

#[derive(Debug, Clone)]
pub struct AutoGainProcessor {
    config: AutoGainConfig,
    current_gain: f32,
}

impl AutoGainConfig {
    pub fn enabled() -> Self {
        Self { enabled: true, ..Self::default() }
    }

    pub fn disabled() -> Self {
        Self { enabled: false, ..Self::default() }
    }

    pub fn is_enabled(self) -> bool {
        self.enabled
    }
}

impl Default for AutoGainConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            target_rms: 0.18,
            silence_rms: 0.01,
            max_gain: 4.0,
            limiter_ceiling: 0.891,
            attack: 0.20,
            release: 0.65,
        }
    }
}

impl AutoGainProcessor {
    pub fn new(_format: AudioFrameFormat, config: AutoGainConfig) -> Self {
        Self { config, current_gain: 1.0 }
    }

    pub fn is_enabled(&self) -> bool {
        self.config.is_enabled()
    }

    pub fn process_interleaved(&mut self, samples: &mut [f32]) {
        if !self.config.is_enabled() || samples.is_empty() {
            return;
        }

        let rms = signal_rms(samples);
        let target_gain = if rms < self.config.silence_rms {
            1.0
        } else {
            (self.config.target_rms / rms).clamp(1.0, self.config.max_gain)
        };

        let smoothing = if target_gain > self.current_gain {
            self.config.attack
        } else {
            self.config.release
        };
        self.current_gain += (target_gain - self.current_gain) * smoothing.clamp(0.0, 1.0);

        let ceiling = self.config.limiter_ceiling.clamp(0.1, 1.0);
        for sample in samples {
            let finite = if sample.is_finite() { *sample } else { 0.0 };
            *sample = (finite * self.current_gain).clamp(-ceiling, ceiling);
        }
    }
}

fn signal_rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum = samples
        .iter()
        .map(|sample| if sample.is_finite() { *sample } else { 0.0 })
        .map(|sample| sample * sample)
        .sum::<f32>();
    (sum / samples.len() as f32).sqrt()
}
```

Update `clearline-core/src/lib.rs`:

```rust
pub use preprocess::{AutoGainConfig, AutoGainProcessor, WindNoiseConfig, WindNoiseReducer};
```

- [ ] **Step 4: Verify core tests pass**

Run:

```bash
cargo test -p clearline-core auto_gain_ -- --nocapture
```

Expected: all auto gain tests pass.

- [ ] **Step 5: Commit**

```bash
git add clearline-core/src/preprocess.rs clearline-core/src/lib.rs
git commit -m "feat: add automatic microphone gain processor"
```

---

### Task 2: Pipeline Integration

**Files:**
- Modify: `clearline-core/src/pipeline.rs`

- [ ] **Step 1: Write failing pipeline tests**

Add tests to `clearline-core/src/pipeline.rs` tests module:

```rust
#[test]
fn pipeline_config_enables_microphone_boost_by_default() {
    let config = AudioPipelineConfig::new(
        DeviceId::new("mic-1"),
        DeviceId::new("out-1"),
        SuppressorMode::HighQuality,
    );

    assert!(config.microphone_boost_enabled());
    assert!(!config.with_microphone_boost(false).microphone_boost_enabled());
}

#[test]
fn runtime_info_exposes_microphone_boost_state() {
    let info = PipelineRuntimeInfo::new(
        AudioFrameFormat::new(48_000, 1),
        AudioFrameFormat::new(48_000, 2),
        SuppressorRuntimeInfo::new(SuppressorMode::HighQuality, "deepfilternet-tract-worker", 480, true),
        AudioOutputTarget::AudioDevice(DeviceId::new("out-1")),
    );

    assert!(!info.microphone_boost_enabled());
    assert!(info.with_microphone_boost(true).microphone_boost_enabled());
}
```

- [ ] **Step 2: Run failing tests**

Run:

```bash
cargo test -p clearline-core microphone_boost -- --nocapture
```

Expected: compile fail because pipeline config/runtime methods do not exist.

- [ ] **Step 3: Add config/runtime fields and callback wiring**

Modify `clearline-core/src/pipeline.rs`:

- Import `AutoGainConfig` and `AutoGainProcessor` with preprocess imports.
- Add `microphone_boost_enabled: bool` to `AudioPipelineConfig`, default true in both constructors.
- Add `with_microphone_boost` and `microphone_boost_enabled` methods.
- Add `microphone_boost_enabled: bool` to `PipelineRuntimeInfo`, default false in `new`, plus `with_microphone_boost` and `microphone_boost_enabled`.
- Create `AutoGainProcessor::new(processing_frame_format, AutoGainConfig::enabled()/disabled())` in both output branches.
- Pass processor to `build_input_passthrough_stream` and `build_input_virtual_microphone_stream`.
- Add `auto_gain_processor: &mut AutoGainProcessor` parameter to `process_input_callback_frame` and call `auto_gain_processor.process_interleaved(&mut scratch.processed);` after `suppressor.process(...)` succeeds.

- [ ] **Step 4: Verify pipeline tests pass**

Run:

```bash
cargo test -p clearline-core microphone_boost -- --nocapture
cargo test -p clearline-core input_processing_runs_echo_before_noise_suppression -- --nocapture
```

Expected: tests pass.

- [ ] **Step 5: Commit**

```bash
git add clearline-core/src/pipeline.rs
git commit -m "feat: apply microphone boost in audio pipeline"
```

---

### Task 3: Settings and UI

**Files:**
- Modify: `clearline-app/src/settings.rs`
- Modify: `clearline-app/src/main.rs`

- [ ] **Step 1: Write failing settings/UI tests**

In `clearline-app/src/settings.rs`, update existing settings tests to assert default true and round-trip field:

```rust
assert!(settings.microphone_boost_enabled);
```

In `clearline-app/src/main.rs`, add tests:

```rust
#[test]
fn app_settings_snapshot_includes_microphone_boost() {
    let mut app = ClearLineApp::new_without_loading_settings_for_tests();
    app.microphone_boost_enabled = false;

    let settings = app.persisted_settings_snapshot();

    assert!(!settings.microphone_boost_enabled);
}

#[test]
fn microphone_boost_labels_are_chinese() {
    assert_eq!(microphone_boost_config_label(true), "麦克风增强：开启");
    assert_eq!(microphone_boost_config_label(false), "麦克风增强：关闭");
    assert_eq!(microphone_boost_runtime_label(None), "未连接音频流");
}

#[test]
fn app_start_source_passes_microphone_boost_to_pipeline() {
    let source = include_str!("main.rs");
    assert!(source.contains(".with_microphone_boost(self.microphone_boost_enabled)"));
}
```

- [ ] **Step 2: Run failing app tests**

Run:

```bash
cargo test -p clearline-app microphone_boost -- --nocapture
```

Expected: compile fail because fields and label helpers do not exist.

- [ ] **Step 3: Implement settings and UI**

- Add `microphone_boost_enabled: bool` to `PersistedSettings`, default true through serde default.
- Add `microphone_boost_enabled: true` to app initial state.
- Load/save the setting.
- Add `set_microphone_boost_enabled` mirroring wind/echo setters.
- Pass `.with_microphone_boost(self.microphone_boost_enabled)` in `start_pipeline`.
- Add a third button in `auxiliary_toggles_row` using `microphone_boost_config_label`.
- Add `info_row(ui, "麦克风增强", microphone_boost_runtime_label(runtime_info));` in `processing_card`.

- [ ] **Step 4: Verify app tests pass**

Run:

```bash
cargo test -p clearline-app microphone_boost -- --nocapture
cargo test -p clearline-app
```

Expected: tests pass.

- [ ] **Step 5: Commit**

```bash
git add clearline-app/src/settings.rs clearline-app/src/main.rs
git commit -m "feat: add microphone boost control to app"
```

---

### Task 4: Docs, Verification, Installer

**Files:**
- Modify: `README.md`
- Modify: `docs/mvp.md`

- [ ] **Step 1: Update docs**

Document that ClearLine now includes default-on microphone enhancement after DeepFilterNet and before VB-CABLE output, with automatic gain and limiter.

- [ ] **Step 2: Run full verification**

Run:

```bash
cargo fmt --all -- --check
cargo test -p clearline-core
cargo test -p clearline-app
cargo check --workspace
```

Expected: all pass.

- [ ] **Step 3: Build installer**

Run from WSL:

```bash
/mnt/c/Windows/System32/WindowsPowerShell/v1.0/powershell.exe -NoProfile -ExecutionPolicy Bypass -File 'E:\Dev\ClearLine\clearline-installer\scripts\build-installer.ps1'
```

Expected: prints `ClearLine installer built: E:\Dev\ClearLine\artifacts\installer\ClearLineSetup.exe` and SHA256.

- [ ] **Step 4: Commit docs**

```bash
git add README.md docs/mvp.md
git commit -m "docs: document microphone enhancement"
```

- [ ] **Step 5: Report manual test instructions**

Tell the user to install the new `ClearLineSetup.exe`, confirm the device page shows `麦克风增强：开启`, compare `CABLE Output` level with the toggle on/off, and verify quiet-room noise and loud speech do not produce obvious pumping or clipping.
