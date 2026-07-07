# ClearLine MVP Skeleton Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a compilable Windows-focused Rust workspace skeleton for ClearLine with core audio abstractions, a minimal egui desktop shell, and MVP documentation.

**Architecture:** `clearline-core` owns device enumeration, device selection, suppressor abstractions, and pipeline state. `clearline-app` is a thin eframe/egui UI that depends on the core crate and performs no real DSP or virtual-device routing yet.

**Tech Stack:** Rust 1.96, Cargo workspace resolver 2, `cpal 0.18.1`, `eframe 0.35.0`, `thiserror 2.0.18`, `anyhow 1.0.103`.

---

## File Structure

- Create `Cargo.toml`: workspace members and shared package/dependency versions.
- Create `clearline-core/Cargo.toml`: core library manifest with CPAL and thiserror.
- Create `clearline-core/src/lib.rs`: module exports and shared error/result types.
- Create `clearline-core/src/device.rs`: input-device models, selector, and CPAL enumerator.
- Create `clearline-core/src/suppressor.rs`: suppressor trait and bypass/placeholder implementations.
- Create `clearline-core/src/pipeline.rs`: pipeline config and state shell.
- Create `clearline-app/Cargo.toml`: egui desktop app manifest.
- Create `clearline-app/src/main.rs`: minimal app UI and state wiring.
- Create `README.md`: project goal, scope, commands, next steps.
- Create `docs/mvp.md`: staged MVP roadmap.

## Tasks

### Task 1: Workspace Manifests

**Files:**
- Create: `Cargo.toml`
- Create: `clearline-core/Cargo.toml`
- Create: `clearline-app/Cargo.toml`

- [ ] Create the workspace root manifest with members `clearline-core` and `clearline-app`, resolver `2`, edition `2021`, and shared dependency versions.
- [ ] Create `clearline-core` manifest as a library crate depending on `cpal` and `thiserror`, with empty `rnnoise` and `deepfilternet` feature gates.
- [ ] Create `clearline-app` manifest as a binary crate depending on `clearline-core`, `anyhow`, and `eframe` with `glow`, `x11`, and `default_fonts` features for local checking.

### Task 2: Core Tests First

**Files:**
- Create: `clearline-core/src/lib.rs`
- Create: `clearline-core/src/device.rs`
- Create: `clearline-core/src/suppressor.rs`
- Create: `clearline-core/src/pipeline.rs`

- [ ] Write unit tests for selector resolution, bypass processing, placeholder modes, and pipeline state transitions before implementing production logic.
- [ ] Run `cargo test -p clearline-core` and confirm the tests fail because the referenced API is not implemented.

### Task 3: Core Implementation

**Files:**
- Modify: `clearline-core/src/lib.rs`
- Modify: `clearline-core/src/device.rs`
- Modify: `clearline-core/src/suppressor.rs`
- Modify: `clearline-core/src/pipeline.rs`

- [ ] Implement `ClearLineError`, `ClearLineResult`, and module exports.
- [ ] Implement `DeviceId`, `AudioInputDevice`, `InputDeviceSelector`, `DeviceEnumerator`, and `CpalDeviceEnumerator`.
- [ ] Implement `SuppressorMode`, `AudioFrameFormat`, `NoiseSuppressor`, `BypassSuppressor`, `LowLatencySuppressor`, `HighQualitySuppressor`, and `create_suppressor`.
- [ ] Implement `PipelineState`, `AudioPipelineConfig`, and `AudioPipeline`.
- [ ] Run `cargo test -p clearline-core` and confirm all core tests pass.

### Task 4: Desktop UI Shell

**Files:**
- Create: `clearline-app/src/main.rs`

- [ ] Implement `ClearLineApp` with device list, selected device ID, selected suppressor mode, pipeline, input-level placeholder, and status message.
- [ ] Implement `eframe::App` to show a labeled device selector, labeled mode selector, progress bar, Start/Stop buttons, refresh action, and status text.
- [ ] Keep UI minimal, single-column, high-contrast, and without decorative iconography.

### Task 5: Documentation

**Files:**
- Create: `README.md`
- Create: `docs/mvp.md`

- [ ] Document ClearLine's goal and first-round scope.
- [ ] Document current commands: `cargo fmt`, `cargo check`, `cargo test -p clearline-core`, and `cargo run -p clearline-app`.
- [ ] Document MVP phases 1 through 5 exactly as agreed.

### Task 6: Verification

**Files:**
- All created files

- [ ] Run `cargo fmt`.
- [ ] Run `cargo check`.
- [ ] Run `cargo test -p clearline-core`.
- [ ] Summarize completed work, deferred work, and recommended next steps.
