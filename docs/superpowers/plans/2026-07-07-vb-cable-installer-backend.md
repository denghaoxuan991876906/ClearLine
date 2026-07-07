# VB-CABLE Installer Backend Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Switch the default ClearLine installer backend from the experimental ClearLine kernel virtual microphone driver to the official basic VB-Audio VB-CABLE package, while keeping the in-repo ClearLine driver code for future Microsoft-signed distribution.

**Architecture:** The native Rust `ClearLineSetup.exe` continues to be self-contained. Its payload embeds ClearLine, DeepFilterNet models, `clearline-installer-helper.exe`, and the unmodified official basic `VBCABLE_Driver_Pack45.zip`. At install time setup writes the zip to `%ProgramFiles%\ClearLine\virtual-audio\vb-cable\`, extracts it without modifying `.inf`, `.sys`, or `.cat` files, verifies existing endpoints, and if needed calls the helper to install one root-enumerated VB-CABLE devnode with hardware ID `VBAudioVACWDM`. ClearLine defaults to outputting to the VB-CABLE render endpoint (`CABLE Input` or `CABLE In 16 Ch`, depending on the official package version); user apps select `CABLE Output`.

**Tech Stack:** Rust, `zip` crate for local zip extraction, cpal endpoint enumeration in the installer helper, Windows SetupAPI / `UpdateDriverForPlugAndPlayDevicesW`, `pnputil`, existing native setup logging/UAC/message box flow, official VB-Audio VB-CABLE basic zip, PowerShell only for developer build/verification scripts.

---

### Task 1: Payload and layout tests

**Files:**
- Modify: `tests/test-installer-layout.ps1`
- Modify: `clearline-installer/scripts/build-installer.ps1`
- Modify: `clearline-setup/build.rs`

- [x] Require official `third_party/vb-cable/VBCABLE_Driver_Pack45.zip`.
- [x] Stop requiring `clearline-driver/artifacts/package/*` in the default installer build.
- [x] Embed the official zip as `virtual-audio/vb-cable/VBCABLE_Driver_Pack45.zip`.
- [x] Keep `clearline-driver/` untouched for future backend work.

### Task 2: Helper VB-CABLE detection and install

**Files:**
- Modify: `clearline-installer-helper/Cargo.toml`
- Modify: `clearline-installer-helper/src/main.rs`

- [x] Add `verify-vb-cable` and `verify-install --require-vb-cable` command paths.
- [x] Add `install-vbcable --package <official-vb-cable-dir>`.
- [x] Enumerate render and capture devices with `cpal`.
- [x] Pass only when a render endpoint matching `CABLE Input` and a capture endpoint matching `CABLE Output` are present.
- [x] Accept official 2024 render endpoint name `CABLE In 16 Ch` while still excluding A+B / C+D endpoints.
- [x] Enumerate MEDIA-class root devnodes and fail if more than one `ROOT\VB-AUDIO_VIRTUAL_CABLE\...` instance exists.
- [x] Validate `vbMmeCable64_win10.inf`, `vbaudio_cable64_win10.sys`, and `vbaudio_cable64_win10.cat` from the official package.
- [x] Reuse root-enumerated devnode creation and `UpdateDriverForPlugAndPlayDevicesW` binding with hardware ID `VBAudioVACWDM`.
- [x] Keep ClearLine self-driver commands available but no longer used by default setup.

### Task 3: Setup flow switch

**Files:**
- Modify: `clearline-setup/Cargo.toml`
- Modify: `clearline-setup/src/main.rs`

- [x] Remove default `install-driver` / `uninstall-driver` calls from setup/uninstall.
- [x] Before/after VB-CABLE install attempt, call helper `verify-vb-cable` and log stdout/stderr.
- [x] If missing, extract official `VBCABLE_Driver_Pack45.zip` without modifying files.
- [x] Call helper `install-vbcable --package <extracted-dir>`.
- [x] If endpoints are still missing, fail with a clear log-backed error telling the user to reboot if needed and re-run setup.
- [x] On uninstall, keep VB-CABLE installed and mention it is a shared component.

### Task 4: App defaults and copy

**Files:**
- Modify: `clearline-app/src/main.rs`
- Modify: `clearline-app/src/settings.rs`

- [x] Default/pending output selection prefers `CABLE Input` when no saved output device is present.
- [x] Start pipeline outputs to the selected `CABLE Input` audio device, not `ClearLineVirtualMicrophone`.
- [x] Keep saved user-selected output device when settings already specify one.
- [x] Change UI guidance from `ClearLine Virtual Microphone` to `CABLE Output`.

### Task 5: Docs and verification

**Files:**
- Modify: `README.md`
- Modify: `clearline-installer/README.md`
- Modify: `clearline-installer/scripts/verify-installed-clearline.ps1`
- Modify: `clearline-installer/scripts/verify-uninstalled-clearline.ps1`

- [x] Document VB-CABLE backend and self-driver preservation.
- [x] Include required VB-Audio attribution and donationware text.
- [x] Verify installed ClearLine files and VB-CABLE endpoints instead of ClearLine self-driver PnP device.
- [x] Run `cargo fmt --all -- --check`, installer layout test, relevant cargo tests, `cargo check`, and `build-installer.ps1`.
