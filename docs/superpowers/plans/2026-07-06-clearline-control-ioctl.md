# ClearLine Control IOCTL Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a minimal user-mode control channel so Rust can open the ClearLine driver and send a ping IOCTL before implementing real PCM injection.

**Architecture:** The SYSVAD driver creates a small named WDM control device `\\.\ClearLineControl`. The driver handles only ClearLine control IOCTLs on that control device and forwards all non-control-device IRPs to PortCls. Rust adds a Windows-only `virtual_mic` module and an example that calls `DeviceIoControl` for `IOCTL_CLEARLINE_PING`.

**Tech Stack:** Windows WDM/PortCls C++, PowerShell layout tests, Rust `windows-sys`, existing Cargo workspace.

---

### Task 1: Contract and Tests

**Files:**
- Modify: `tests/test-driver-layout.ps1`
- Create: `docs/superpowers/plans/2026-07-06-clearline-control-ioctl.md`

- [x] Add layout checks for `CLEARLINE_CONTROL_DOS_SYMBOLIC_LINK`, `IOCTL_CLEARLINE_PING`, and `ClearLinePingResponse`.
- [x] Run `tests/test-driver-layout.ps1` and confirm it fails before implementation.

### Task 2: Driver Control Device

**Files:**
- Modify: `clearline-driver/third_party/windows-driver-samples/audio/sysvad/adapter.cpp`

- [x] Add a named WDM control device `\\Device\\ClearLineControl` and DOS link `\\DosDevices\\ClearLineControl`.
- [x] Save original PortCls dispatch entries for create/close/device-control.
- [x] Handle create/close on the control device and forward other device objects to PortCls.
- [x] Handle `IOCTL_CLEARLINE_PING` with a fixed `ClearLinePingResponse`.
- [x] Delete the control symbolic link and device in `DriverUnload`.

### Task 3: Rust Probe

**Files:**
- Modify: `Cargo.toml`
- Modify: `clearline-core/Cargo.toml`
- Modify: `clearline-core/src/lib.rs`
- Create: `clearline-core/src/virtual_mic.rs`
- Create: `clearline-core/examples/probe_virtual_mic.rs`

- [x] Add `windows-sys` dependency for Win32 file/device APIs.
- [x] Implement `VirtualMicControl::ping()` using `CreateFileW` and `DeviceIoControl`.
- [x] Add `probe_virtual_mic` example.

### Task 4: Verify and Commit

**Commands:**
- `powershell -NoProfile -ExecutionPolicy Bypass -File .\tests\test-driver-layout.ps1`
- `powershell -NoProfile -ExecutionPolicy Bypass -File .\clearline-driver\scripts\build-driver.ps1 -Platform x64`
- `powershell -NoProfile -ExecutionPolicy Bypass -File .\clearline-driver\scripts\verify-driver-package.ps1`
- `cargo fmt --all --check`
- `cargo check --workspace`

**Commit:**
- `git commit -m "feat: add virtual microphone control ioctl"`
