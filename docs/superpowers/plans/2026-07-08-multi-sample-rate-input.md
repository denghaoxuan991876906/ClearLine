# Multi Sample Rate Input Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Allow ClearLine's virtual microphone pipeline to accept non-48 kHz recording devices by resampling microphone and loopback reference audio into the existing 48 kHz processing/output domain.

**Architecture:** Add a focused `clearline-core/src/resample.rs` streaming converter backed by `rubato 0.14`. Integrate it only into the virtual microphone path first, leaving audio-device passthrough behavior unchanged. Run suppressor, AEC, and wind reduction at 48 kHz while preserving the real input format in runtime diagnostics.

**Tech Stack:** Rust, CPAL, rubato 0.14, existing ClearLine audio pipeline tests.

---

## Files

- Create: `clearline-core/src/resample.rs` — streaming interleaved `f32` sample-rate converter.
- Modify: `clearline-core/Cargo.toml` — add direct `rubato = "0.14"` dependency.
- Modify: `clearline-core/src/lib.rs` — export the converter module/types.
- Modify: `clearline-core/src/pipeline.rs` — resample virtual microphone input callbacks to 48 kHz and remove the old startup rejection.
- Modify: `clearline-core/src/reference.rs` — allow loopback reference capture to store a target processing format.
- Modify: `README.md` and/or `docs/mvp.md` — document multi-sample-rate input support.

## Task 1: Add streaming resampler

- [ ] Write failing tests in `clearline-core/src/resample.rs` for 48k direct copy, 44.1k to 48k, 96k to 48k, and stereo channel preservation.
- [ ] Run `cargo test -p clearline-core resample_` and confirm the module/functions are missing.
- [ ] Add `rubato = "0.14"` and implement `StreamingSampleRateConverter` with an interleaved `process_interleaved` API.
- [ ] Run `cargo test -p clearline-core resample_` and confirm tests pass.
- [ ] Commit with `feat: add streaming sample rate converter`.

## Task 2: Integrate converter into virtual microphone path

- [ ] Write/adjust tests in `clearline-core/src/pipeline.rs` proving the old 48k-only virtual microphone input rejection is gone and processing format selection returns 48k for the virtual microphone target.
- [ ] Run the targeted tests and confirm failure before code changes.
- [ ] Add a `processing_input` scratch buffer and a converter parameter to `build_input_virtual_microphone_stream`.
- [ ] Use real input format for `PipelineRuntimeInfo::input_format()` and `48000 Hz / input_channels` for suppressor/AEC/wind processing format.
- [ ] Run targeted pipeline tests and confirm pass.
- [ ] Commit with `feat: resample virtual microphone input to 48k`.

## Task 3: Resample AEC loopback reference to processing format

- [ ] Add tests in `clearline-core/src/reference.rs` proving reference capture accepts a target format and stores/populates mono frames in that target rate.
- [ ] Run targeted reference tests and confirm failure.
- [ ] Add `start_default_with_target_format` / `start_for_output_device_with_target_format` and use the streaming converter in the loopback callback before `ReferenceFrameBuffer::push_interleaved`.
- [ ] Update `start_reference_capture_for_echo` to pass `echo_cancellation.format()` when AEC3 is active.
- [ ] Run reference and pipeline tests.
- [ ] Commit with `feat: resample echo reference capture`.

## Task 4: Documentation and verification

- [ ] Update docs to mention that ClearLine supports non-48 kHz input devices by resampling to the 48 kHz processing path.
- [ ] Run `cargo fmt --all -- --check`.
- [ ] Run `cargo test -p clearline-core resample_`.
- [ ] Run `cargo test -p clearline-core pipeline_ virtual_microphone reference_`.
- [ ] Run `cargo check --workspace`.
- [ ] Commit with `docs: document multi sample rate input` if docs changed after code commits.

## Manual Windows test

1. Build/run ClearLine on Windows.
2. Select a non-48 kHz microphone if available.
3. Start ClearLine.
4. Confirm the old error `requires a 48000 Hz input stream` does not appear.
5. Confirm status shows the real input format and output `48000 Hz / 1 channel`.
6. Record from `CABLE Output` and confirm voice speed/pitch is normal.
