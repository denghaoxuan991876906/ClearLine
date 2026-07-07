# AEC Integration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a phased acoustic echo cancellation path that can be validated with automated tools first, then exposed for final manual testing after the algorithm and API path run end-to-end.

**Architecture:** Keep AEC as a core audio-processing stage before wind reduction and RNNoise/DeepFilterNet. Capture render/reference audio from the Windows default output device through WASAPI/cpal loopback, feed 10 ms render frames plus 10 ms mic capture frames into a WebRTC AEC3-compatible processor, then pass cleaned capture into the existing ClearLine virtual microphone pipeline. The first milestones are testable without human listening: download official Microsoft AEC-Challenge fixtures, prove AEC3 reduces residual echo on real fixture WAVs, then move to reference buffering and pipeline stage ordering.

**Tech Stack:** Rust 2021, `cpal` WASAPI loopback on Windows, optional `aec3` crate for WebRTC AEC3, existing `FrameChunker`, `AudioFrameFormat`, `WindNoiseReducer`, RNNoise, DeepFilterNet, ClearLine Virtual Microphone driver.



**Plan adjustment 2026-07-07:** Per user direction, official AEC-Challenge audio fixtures are now the first gate. Do not wire AEC into the realtime ClearLine pipeline until `analyze_aec_challenge_fixture` and the ignored fixture test pass on downloaded official WAVs.

---

## File Structure

- `clearline-core/Cargo.toml`
  - Add optional `aec3` dependency and `aec` feature.
- `clearline-core/src/reference.rs`
  - Own render/reference frame buffering, reference metrics, and Windows loopback capture helpers.
- `clearline-core/src/echo.rs`
  - Own `EchoCanceller` trait, `NoopEchoCanceller`, optional `Aec3EchoCanceller`, synthetic test helpers, and runtime info.
- `clearline-core/src/pipeline.rs`
  - Add AEC config/runtime info and place AEC before wind reduction in the input callbacks.
- `clearline-core/src/lib.rs`
  - Re-export reference and echo types.
- `clearline-core/examples/probe_loopback_reference.rs`
  - Print reference level from default render loopback for Windows verification.
- `clearline-app/src/main.rs`
  - Later phase: add compact Chinese AEC toggle and status labels.
- `README.md` / `docs/mvp.md`
  - Later phase: document AEC test commands and staged status.

---

### Task 1: Official AEC-Challenge fixture downloader and analyzer

**Files:**
- Create: `scripts/download-aec-fixtures.py`
- Create: `clearline-core/tests/aec_challenge_fixture.rs`
- Create: `clearline-core/examples/analyze_aec_challenge_fixture.rs`
- Modify: `clearline-core/Cargo.toml`
- Modify: `Cargo.toml`

- [ ] **Step 1: Write failing tests for reference buffering**

Add tests that prove:

```rust
#[test]
fn reference_buffer_downmixes_interleaved_stereo_to_mono_frames() {
    let format = AudioFrameFormat::new(48_000, 2);
    let mut buffer = ReferenceFrameBuffer::new(format, 10);

    buffer.push_interleaved(&[0.2, 0.6, -0.4, 0.0]);

    let mut frame = vec![0.0; 2];
    assert!(buffer.pop_mono_frame(&mut frame));
    assert_eq!(frame, vec![0.4, -0.2]);
}

#[test]
fn reference_buffer_reports_level_and_missing_frames() {
    let format = AudioFrameFormat::new(48_000, 1);
    let mut buffer = ReferenceFrameBuffer::new(format, 4);
    let mut frame = vec![1.0; 2];

    assert!(!buffer.pop_mono_frame(&mut frame));
    assert_eq!(frame, vec![0.0, 0.0]);
    assert_eq!(buffer.stats().missing_frames(), 1);

    buffer.push_interleaved(&[0.25, -0.5]);
    assert!(buffer.pop_mono_frame(&mut frame));
    assert_eq!(buffer.stats().last_level(), 0.5);
}
```

- [ ] **Step 2: Verify RED**

Run:

```bash
cargo test -p clearline-core reference_buffer
```

Expected: compile failure because `reference` module and `ReferenceFrameBuffer` do not exist.

- [ ] **Step 3: Implement reference module**

Create `ReferenceFrameBuffer`, `ReferenceCaptureStats`, and Windows-only `LoopbackReferenceCapture` that uses cpal output devices as input devices to trigger WASAPI loopback.

- [ ] **Step 4: Verify GREEN**

Run:

```bash
cargo test -p clearline-core reference_buffer
cargo test -p clearline-core
```

Expected: all core tests pass.

- [ ] **Step 5: Commit**

```bash
git add clearline-core/src/reference.rs clearline-core/src/lib.rs clearline-core/examples/probe_loopback_reference.rs
git commit -m "feat: add reference audio buffering"
```

Manual Windows command for this task:

```powershell
cargo run -p clearline-core --example probe_loopback_reference
```

Expected when playing system audio: reference level rises above 0; when silent: level stays near 0.

---

### Task 2: AEC abstraction and synthetic reduction test

**Files:**
- Modify: `clearline-core/Cargo.toml`
- Create: `clearline-core/src/echo.rs`
- Modify: `clearline-core/src/lib.rs`

- [ ] **Step 1: Write failing tests for echo processor API**

Add tests that prove:

```rust
#[test]
fn noop_echo_canceller_copies_capture_and_reports_disabled() {
    let format = AudioFrameFormat::new(48_000, 1);
    let mut canceller = NoopEchoCanceller::new(format);
    let capture = [0.1, -0.2, 0.3];
    let render = [0.3, 0.2, 0.1];
    let mut output = [0.0; 3];

    canceller.process(&capture, &render, &mut output).unwrap();

    assert_eq!(output, capture);
    assert_eq!(canceller.runtime_info().backend(), EchoCancellerBackend::Disabled);
}

#[test]
fn synthetic_echo_fixture_measures_echo_correlation_drop() {
    let metrics = synthetic_echo_reduction_fixture();

    assert!(metrics.input_echo_correlation() > 0.45);
    assert!(metrics.output_echo_correlation() < metrics.input_echo_correlation() * 0.75);
}
```

- [ ] **Step 2: Verify RED**

Run:

```bash
cargo test -p clearline-core echo_canceller
cargo test -p clearline-core synthetic_echo_fixture_measures_echo_correlation_drop --features aec
```

Expected: compile failure because `echo` module and `aec` feature do not exist.

- [ ] **Step 3: Implement echo module**

Add:

- `EchoCancellerBackend::{Disabled, Aec3}`
- `EchoCancellerRuntimeInfo`
- `EchoCanceller` trait
- `NoopEchoCanceller`
- feature-gated `Aec3EchoCanceller`
- `synthetic_echo_reduction_fixture()` that generates render/capture data, runs AEC3, and returns before/after correlation metrics.

- [ ] **Step 4: Verify GREEN**

Run:

```bash
cargo test -p clearline-core echo_canceller
cargo test -p clearline-core synthetic_echo_fixture_measures_echo_correlation_drop --features aec
cargo test -p clearline-core
cargo test -p clearline-core --features aec
```

Expected: all selected and full core tests pass.

- [ ] **Step 5: Commit**

```bash
git add clearline-core/Cargo.toml clearline-core/src/echo.rs clearline-core/src/lib.rs Cargo.lock
git commit -m "feat: add AEC3 echo canceller abstraction"
```

---

### Task 3: Pipeline AEC stage behind config

**Files:**
- Modify: `clearline-core/src/pipeline.rs`
- Modify: `clearline-core/src/lib.rs`

- [ ] **Step 1: Write failing tests for pipeline configuration and ordering**

Add tests that prove:

```rust
#[test]
fn pipeline_config_disables_echo_cancellation_by_default() {
    let config = AudioPipelineConfig::for_virtual_microphone(
        DeviceId::new("mic"),
        SuppressorMode::LowLatency,
    );

    assert!(!config.echo_cancellation_enabled());
}

#[test]
fn pipeline_config_can_enable_echo_cancellation() {
    let config = AudioPipelineConfig::for_virtual_microphone(
        DeviceId::new("mic"),
        SuppressorMode::LowLatency,
    )
    .with_echo_cancellation(true);

    assert!(config.echo_cancellation_enabled());
}
```

- [ ] **Step 2: Verify RED**

Run:

```bash
cargo test -p clearline-core pipeline_config_can_enable_echo_cancellation
```

Expected: compile failure because config methods do not exist.

- [ ] **Step 3: Add config/runtime fields and callback order**

Add `echo_cancellation_enabled` to `AudioPipelineConfig`, include `EchoCancellerRuntimeInfo` in `PipelineRuntimeInfo`, and route mic samples through AEC before `WindNoiseReducer` when enabled. For non-Windows/default builds, create `NoopEchoCanceller` so tests remain portable.

- [ ] **Step 4: Verify GREEN**

Run:

```bash
cargo test -p clearline-core pipeline_config_can_enable_echo_cancellation
cargo test -p clearline-core
cargo test -p clearline-core --features aec
```

Expected: all tests pass.

- [ ] **Step 5: Commit**

```bash
git add clearline-core/src/pipeline.rs clearline-core/src/lib.rs
git commit -m "feat: wire AEC stage into audio pipeline"
```

---

### Task 4: App toggle and diagnostics

**Files:**
- Modify: `clearline-app/src/main.rs`
- Modify: `README.md`

- [ ] **Step 1: Write failing UI label/config tests**

Add tests that prove Chinese labels and config path exist:

```rust
#[test]
fn echo_cancellation_labels_are_chinese() {
    assert_eq!(echo_cancellation_title(), "回音消除");
    assert_eq!(echo_cancellation_enabled_label(true), "已启用");
    assert_eq!(echo_cancellation_enabled_label(false), "未启用");
}
```

- [ ] **Step 2: Verify RED**

Run:

```bash
cargo test -p clearline-app echo_cancellation_labels_are_chinese
```

Expected: compile failure because label helpers do not exist.

- [ ] **Step 3: Add compact toggle**

Add a device-tab switch-like checkbox named `回音消除`; keep reference source implicit as `系统默认播放设备`. Pass `.with_echo_cancellation(self.echo_cancellation_enabled)` into `AudioPipelineConfig`.

- [ ] **Step 4: Verify GREEN**

Run:

```bash
cargo test -p clearline-app echo_cancellation_labels_are_chinese
cargo test -p clearline-app
cargo check
```

Expected: app tests and workspace check pass.

- [ ] **Step 5: Commit**

```bash
git add clearline-app/src/main.rs README.md
git commit -m "feat: expose echo cancellation toggle"
```

---

### Task 5: Final automated AEC verification bundle

**Files:**
- Create: `clearline-core/examples/analyze_synthetic_aec.rs`
- Modify: `README.md`

- [ ] **Step 1: Write example command path**

Create an example that prints input/output echo correlation, ERLE-like dB improvement, and pass/fail threshold.

- [ ] **Step 2: Verify example**

Run:

```bash
cargo run -p clearline-core --features aec --example analyze_synthetic_aec
```

Expected: prints `PASS` and output correlation below input correlation threshold.

- [ ] **Step 3: Full verification**

Run:

```bash
cargo fmt --all
cargo test -p clearline-core
cargo test -p clearline-core --features aec
cargo test -p clearline-app
cargo check
```

Expected: all pass.

- [ ] **Step 4: Commit**

```bash
git add clearline-core/examples/analyze_synthetic_aec.rs README.md
git commit -m "test: add synthetic AEC analyzer"
```

