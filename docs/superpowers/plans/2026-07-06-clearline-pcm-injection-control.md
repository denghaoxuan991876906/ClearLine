# ClearLine PCM Injection Control Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a minimal user-mode PCM injection path so Rust can write mono 48 kHz i16 samples into the ClearLine driver control buffer before wiring the virtual microphone capture pin.

**Architecture:** The existing `\\.\ClearLineControl` WDM control device gains two buffered IOCTLs: one to write PCM bytes into a small nonpaged ring buffer and one to query buffer counters. The Rust `VirtualMicControl` wrapper exposes `write_pcm_i16_mono_48k()` and `buffer_status()`, plus an example that injects a short block of silence. This step intentionally does not make the virtual microphone output that audio yet; it only verifies safe user-mode to kernel-mode audio payload transfer.

**Tech Stack:** Windows WDM/PortCls C++, PowerShell layout tests, Rust `windows-sys`, existing Cargo workspace.

---

### Task 1: Contract Tests

**Files:**
- Modify: `tests/test-driver-layout.ps1`
- Modify: `clearline-core/src/virtual_mic.rs`

- [x] Add layout checks for `IOCTL_CLEARLINE_WRITE_PCM`, `IOCTL_CLEARLINE_GET_BUFFER_STATUS`, `ClearLineBufferStatus`, and ring buffer markers.
- [x] Add Rust unit tests for new IOCTL codes and status struct validation.
- [x] Run tests and confirm they fail before implementation:
  - `powershell -NoProfile -ExecutionPolicy Bypass -File .\tests\test-driver-layout.ps1`
  - `cargo test -p clearline-core virtual_mic -- --nocapture`

### Task 2: Driver PCM Buffer

**Files:**
- Modify: `clearline-driver/third_party/windows-driver-samples/audio/sysvad/adapter.cpp`

- [x] Add `IOCTL_CLEARLINE_WRITE_PCM` and `IOCTL_CLEARLINE_GET_BUFFER_STATUS` constants.
- [x] Add a fixed-size nonpaged ring buffer and counters for write bytes, dropped bytes, and overflow count.
- [x] Initialize the ring buffer when creating the control device.
- [x] Free the ring buffer when destroying the control device.
- [x] Implement buffered PCM write handling.
- [x] Implement buffer status query handling.

### Task 3: Rust API and Example

**Files:**
- Modify: `clearline-core/src/lib.rs`
- Modify: `clearline-core/src/virtual_mic.rs`
- Create: `clearline-core/examples/inject_virtual_mic_silence.rs`

- [x] Add `ClearLineBufferStatus` and export it.
- [x] Implement `VirtualMicControl::write_pcm_i16_mono_48k()`.
- [x] Implement `VirtualMicControl::buffer_status()`.
- [x] Add `inject_virtual_mic_silence` example that writes 100 ms of silence and prints status.

### Task 4: Verify and Commit

**Commands:**
- `cargo fmt --all --check`
- `cargo test -p clearline-core virtual_mic -- --nocapture`
- `cargo check --workspace --examples`
- Windows: `cargo check --workspace --examples`
- `powershell -NoProfile -ExecutionPolicy Bypass -File .\tests\test-driver-layout.ps1`
- `powershell -NoProfile -ExecutionPolicy Bypass -File .\clearline-driver\scripts\build-driver.ps1 -Platform x64`
- `powershell -NoProfile -ExecutionPolicy Bypass -File .\clearline-driver\scripts\verify-driver-package.ps1`

**Commit:**
- `git commit -m "feat: add virtual microphone pcm injection control"`
