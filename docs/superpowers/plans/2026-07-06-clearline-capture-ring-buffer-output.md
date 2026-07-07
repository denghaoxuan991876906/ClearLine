# ClearLine Capture Ring Buffer Output Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `ClearLine Virtual Microphone` capture output consume PCM that Rust injected into the ClearLine driver ring buffer.

**Architecture:** Keep SYSVAD's existing WaveRT capture timing and packet accounting. Add a shared ClearLine ring-buffer header, add a `ClearLineReadPcmFromRingBuffer()` consumer function, and change `CMiniportWaveRTStream::WriteBytes()` to copy injected PCM into the capture DMA buffer with silence fallback on underrun. Rust gains an `inject_virtual_mic_sine` example for manual recording tests.

**Tech Stack:** Windows WDM/PortCls C++, SYSVAD WaveRT miniport, PowerShell layout tests, Rust examples.

---

### Task 1: Contract Tests

**Files:**
- Modify: `tests/test-driver-layout.ps1`
- Modify: `clearline-core/src/virtual_mic.rs`

- [x] Add layout checks for `clearline_ringbuffer.h`, `ClearLineReadPcmFromRingBuffer`, `ClearLineFillCaptureBuffer`, `TotalReadBytes`, `TotalUnderrunBytes`, and `UnderrunCount`.
- [x] Add Rust unit-test coverage for new `ClearLineBufferStatus` read/underrun getters.
- [x] Run tests and confirm failure before implementation:
  - `powershell -NoProfile -ExecutionPolicy Bypass -File .\tests\test-driver-layout.ps1`
  - `cargo test -p clearline-core virtual_mic -- --nocapture`

### Task 2: Shared Ring Buffer Contract

**Files:**
- Create: `clearline-driver/third_party/windows-driver-samples/audio/sysvad/clearline_ringbuffer.h`
- Modify: `clearline-driver/third_party/windows-driver-samples/audio/sysvad/adapter.cpp`

- [x] Move shared ClearLine IOCTL constants and `ClearLineBufferStatus` into `clearline_ringbuffer.h`.
- [x] Include the header from `adapter.cpp`.
- [x] Remove duplicate local struct definitions from `adapter.cpp`.
- [x] Add ring-buffer read counters and underrun counters.

### Task 3: Driver Capture Consumer

**Files:**
- Modify: `clearline-driver/third_party/windows-driver-samples/audio/sysvad/adapter.cpp`
- Modify: `clearline-driver/third_party/windows-driver-samples/audio/sysvad/EndpointsCommon/minwavertstream.cpp`

- [x] Implement `ClearLineReadPcmFromRingBuffer()` that consumes available bytes and zero-fills short reads via caller helper.
- [x] Implement `ClearLineFillCaptureBuffer()` that reads into a destination buffer and fills missing bytes with silence.
- [x] Include `clearline_ringbuffer.h` in `minwavertstream.cpp`.
- [x] Replace capture `WriteBytes()` sine generation with `ClearLineFillCaptureBuffer()`.

### Task 4: Rust Status and Sine Example

**Files:**
- Modify: `clearline-core/src/virtual_mic.rs`
- Create: `clearline-core/examples/inject_virtual_mic_sine.rs`

- [x] Extend `ClearLineBufferStatus` with getters for `total_read_bytes`, `total_underrun_bytes`, and `underrun_count`.
- [x] Add `inject_virtual_mic_sine` example that continuously injects 440 Hz mono i16 PCM for 30 seconds.
- [x] Print buffer status once per second while injecting.

### Task 5: Verify and Commit

**Commands:**
- `cargo fmt --all --check`
- `cargo test -p clearline-core virtual_mic -- --nocapture`
- `cargo check --workspace --examples`
- Windows: `cargo check --workspace --examples`
- `powershell -NoProfile -ExecutionPolicy Bypass -File .\tests\test-driver-layout.ps1`
- `powershell -NoProfile -ExecutionPolicy Bypass -File .\clearline-driver\scripts\build-driver.ps1 -Platform x64`
- `powershell -NoProfile -ExecutionPolicy Bypass -File .\clearline-driver\scripts\verify-driver-package.ps1`

**Commit:**
- `git commit -m "feat: output injected pcm from virtual microphone"`
