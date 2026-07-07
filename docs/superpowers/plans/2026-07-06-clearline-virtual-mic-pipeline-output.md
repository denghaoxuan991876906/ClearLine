# ClearLine Virtual Mic Pipeline Output Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Route ClearLine's real microphone denoise pipeline directly into the built-in `ClearLine Virtual Microphone` driver output.

**Architecture:** Keep the existing CPAL playback-device output as a fallback/debug target, but add a first-class `AudioOutputTarget::ClearLineVirtualMicrophone` pipeline target. In that mode the input callback still captures, preprocesses and suppresses real microphone audio, then mixes the processed interleaved samples to mono i16 at 48 kHz and writes them through `VirtualMicControl::write_pcm_i16_mono_48k()`. The app defaults to the built-in virtual microphone target and only requires a playback output device when the user explicitly selects the legacy audio-device target.

**Tech Stack:** Rust `clearline-core`, CPAL input streams, ClearLine driver control IOCTL, egui desktop UI, serde settings.

---

### Task 1: Core Output Target Contract

**Files:**
- Modify: `clearline-core/src/pipeline.rs`
- Modify: `clearline-core/src/lib.rs`

- [ ] **Step 1: Write failing tests**

Add tests that require:
- `AudioPipelineConfig::for_virtual_microphone(DeviceId::new("mic-1"), SuppressorMode::LowLatency)` stores `AudioOutputTarget::ClearLineVirtualMicrophone`.
- Existing `AudioPipelineConfig::new(...)` still stores `AudioOutputTarget::AudioDevice(DeviceId)`.
- `PipelineRuntimeInfo` can expose the selected output target.

- [ ] **Step 2: Run tests to verify RED**

Run: `cargo test -p clearline-core pipeline_config -- --nocapture`
Expected: compile failure or failing tests because `AudioOutputTarget` and `for_virtual_microphone` do not exist yet.

- [ ] **Step 3: Implement minimal contract**

Add `AudioOutputTarget`, store it in `AudioPipelineConfig`, preserve `output_device_id()` for audio-device compatibility, add `output_target()`, and re-export the enum from `clearline-core/src/lib.rs`.

- [ ] **Step 4: Run tests to verify GREEN**

Run: `cargo test -p clearline-core pipeline_config -- --nocapture`
Expected: tests pass.

### Task 2: Core Virtual Microphone Sample Sink

**Files:**
- Modify: `clearline-core/src/pipeline.rs`

- [ ] **Step 1: Write failing tests**

Add unit tests for:
- Mixing stereo f32 frames to mono i16 PCM.
- Clamping samples outside `[-1.0, 1.0]`.
- Virtual microphone output format requiring `48000 Hz / 1 ch`.

- [ ] **Step 2: Run tests to verify RED**

Run: `cargo test -p clearline-core virtual_microphone -- --nocapture`
Expected: compile failure or failing tests because the conversion helpers and format helper do not exist yet.

- [ ] **Step 3: Implement minimal conversion helpers**

Add helpers that derive the virtual microphone stream format from the driver ping response and append mono i16 samples from processed interleaved f32 input.

- [ ] **Step 4: Run tests to verify GREEN**

Run: `cargo test -p clearline-core virtual_microphone -- --nocapture`
Expected: tests pass.

### Task 3: Core Pipeline Runtime Integration

**Files:**
- Modify: `clearline-core/src/pipeline.rs`

- [ ] **Step 1: Write failing source-layout test**

Add a unit/source test that checks the Windows pipeline contains a virtual microphone branch using `build_input_virtual_microphone_stream` and `VirtualMicControl::write_pcm_i16_mono_48k`.

- [ ] **Step 2: Run test to verify RED**

Run: `cargo test -p clearline-core virtual_microphone_pipeline_source -- --nocapture`
Expected: failing test because the branch does not exist.

- [ ] **Step 3: Implement Windows branch**

In `start_platform_streams`, branch on `AudioOutputTarget`:
- `AudioDevice(DeviceId)`: keep existing CPAL output stream path.
- `ClearLineVirtualMicrophone`: ping the driver, require `48000 Hz / 1 ch`, build only the input stream, and write processed mono i16 PCM into the driver ring buffer from the input callback.

- [ ] **Step 4: Run test to verify GREEN**

Run: `cargo test -p clearline-core virtual_microphone_pipeline_source -- --nocapture`
Expected: source-layout test passes.

### Task 4: App UI and Settings Integration

**Files:**
- Modify: `clearline-app/src/settings.rs`
- Modify: `clearline-app/src/main.rs`

- [ ] **Step 1: Write failing app/settings tests**

Add tests that require:
- Settings default output target is `clearline_virtual_microphone`.
- App start button can start with input selected and virtual target selected even if no playback output device is selected.
- Output target labels are Chinese and distinguish built-in virtual microphone from playback/audio-device output.

- [ ] **Step 2: Run tests to verify RED**

Run: `cargo test -p clearline-app output_target -- --nocapture`
Expected: compile failure or failing tests because output target settings/UI helpers do not exist yet.

- [ ] **Step 3: Implement app integration**

Add persisted `output_target`, default it to built-in virtual microphone, add a segmented output-target selector on the device tab, build `AudioPipelineConfig::for_virtual_microphone(...)` for the virtual target, and keep the existing playback output selector as an optional debug/fallback target.

- [ ] **Step 4: Run tests to verify GREEN**

Run: `cargo test -p clearline-app output_target -- --nocapture`
Expected: tests pass.

### Task 5: Verify, Build, and Commit

**Commands:**
- `cargo fmt --all --check`
- `cargo test -p clearline-core pipeline -- --nocapture`
- `cargo test -p clearline-core virtual_mic -- --nocapture`
- `cargo test -p clearline-app -- --nocapture`
- `cargo check --workspace --examples`
- Windows: `cargo check --workspace --examples`

**Manual test for user:**
1. Reinstall the driver only if the current installed driver is older than the PCM-output build.
2. Run `cargo run -p clearline-app`.
3. On the device tab, keep output target as `ClearLine 虚拟麦克风`.
4. Select the real microphone, choose low-latency or high-quality mode, click `开始降噪`.
5. In Windows recording devices or a voice app, select `ClearLine Virtual Microphone` and confirm the real mic voice appears with denoise applied.

**Commit:**
- `git commit -m "feat: route pipeline to virtual microphone"`
