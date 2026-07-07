# ClearLine Virtual Audio Driver Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build ClearLine's own Windows virtual microphone driver, starting from a verified SYSVAD baseline and ending with a root-enumerated capture device that the Rust app can feed.

**Architecture:** Keep audio processing in the Rust app. Use a WDM/WaveRT SYSVAD-derived kernel driver to expose `ClearLine Virtual Microphone`; add a user-mode PCM injection channel after the baseline device can build, install, and enumerate.

**Tech Stack:** Rust app, Windows WDK, Visual Studio MSBuild, C++ SYSVAD driver code, PowerShell SetupAPI scripts.

---

### Task 1: Baseline Driver Scaffold

**Files:**
- Create: `clearline-driver/third_party/README.md`
- Create: `clearline-driver/ClearLineVirtualAudio/ClearLineVirtualAudio.vcxproj`
- Create: `clearline-driver/ClearLineVirtualAudio/ClearLineVirtualAudio.inf`
- Create: `clearline-driver/scripts/*.ps1`
- Create: `tests/test-driver-layout.ps1`
- Create: `docs/driver.md`

- [x] Write a failing layout test that checks required driver files and ClearLine INF markers.
- [x] Import SYSVAD and WIL third-party source snapshots.
- [x] Add ClearLine INF and wrapper project.
- [x] Add environment, build, signing, install, uninstall, and device-check scripts.
- [ ] Run layout test and environment probe.
- [ ] Commit baseline scaffold.

### Task 2: Buildable ClearLine-Owned Driver Fork

**Files:**
- Create: `clearline-driver/ClearLineVirtualAudio/src/**`
- Modify: `clearline-driver/ClearLineVirtualAudio/ClearLineVirtualAudio.vcxproj`
- Modify: `clearline-driver/ClearLineVirtualAudio/ClearLineVirtualAudio.inf`

- [ ] Copy only required SYSVAD driver sources into `ClearLineVirtualAudio/src`.
- [ ] Rename service and binary from `TabletAudioSample` to `ClearLineVirtualAudio`.
- [ ] Build with WDK MSBuild and generate a signed development package.
- [ ] Install on the local Windows machine and verify PnP enumeration.
- [ ] Commit buildable fork.

### Task 3: Single Capture Endpoint

**Files:**
- Modify: `clearline-driver/ClearLineVirtualAudio/src/**`
- Modify: `clearline-driver/ClearLineVirtualAudio/ClearLineVirtualAudio.inf`

- [ ] Remove render endpoints and unrelated APO sample components from the ClearLine-owned fork.
- [ ] Keep one capture endpoint named `ClearLine Virtual Microphone`.
- [ ] Verify it appears in Windows sound input devices.
- [ ] Commit single-endpoint driver.

### Task 4: User-Mode PCM Injection Channel

**Files:**
- Modify: `clearline-driver/ClearLineVirtualAudio/src/**`
- Modify: `clearline-core/src/pipeline.rs`
- Modify: `clearline-app/src/main.rs`

- [ ] Add IOCTL/shared-buffer protocol for 48 kHz PCM from ClearLine app to driver.
- [ ] Add Rust-side writer abstraction and diagnostics.
- [ ] Verify audio from the Rust app reaches the virtual microphone endpoint.
- [ ] Commit PCM injection path.
