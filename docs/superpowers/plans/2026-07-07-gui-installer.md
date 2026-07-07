# ClearLine Native Self-contained Installer Implementation Plan

**Goal:** Build a self-contained `ClearLineSetup.exe` that installs ClearLine, DeepFilterNet models, the ClearLine virtual microphone driver package, shortcuts/start-menu entries, install verification, and uninstall hooks without requiring the end user to install any setup runtime or external installer tool.

**Architecture:** Use Rust for the setup executable. `clearline-setup` embeds the release app, model assets, driver package, and `clearline-installer-helper.exe` at compile time. The build script produces `artifacts/installer/ClearLineSetup.exe`. At runtime, the setup executable self-elevates with UAC, extracts payload to `%ProgramFiles%\ClearLine`, writes uninstall registry entries, creates a Start Menu entry, calls the native helper for driver install/verification, and supports `--uninstall`.

**Tech Stack:** Rust, Windows Shell/UAC APIs, Windows registry via built-in `reg.exe`, native driver helper, `pnputil` inside helper, PowerShell only for developer-side build/verification scripts.

---

### Task 1: Native self-contained installer layout regression test

**Files:**
- Modify: `tests/test-installer-layout.ps1`

- [x] Require no legacy `.iss` setup script in the primary path.
- [x] Require `clearline-setup/Cargo.toml`, `clearline-setup/build.rs`, and `clearline-setup/src/main.rs`.
- [x] Require build script markers for `cargo build -p clearline-setup --release`, `CLEARLINE_SETUP_STRICT_PAYLOAD`, and `ClearLineSetup.exe`.
- [x] Require setup source markers for UAC self-elevation, helper calls, uninstall registry, and embedded payload install.

### Task 2: Native setup executable

**Files:**
- Modify: `Cargo.toml`
- Create: `clearline-setup/Cargo.toml`
- Create: `clearline-setup/build.rs`
- Create: `clearline-setup/src/main.rs`

- [x] Add `clearline-setup` to the Rust workspace.
- [x] Generate an embedded payload manifest with `include_bytes!`.
- [x] Embed:
  - `dist/ClearLine.exe`
  - required `dist/models/deepfilternet/*` runtime assets
  - `clearline-driver/artifacts/package/*`
  - `target/release/clearline-installer-helper.exe`
- [x] Implement `ClearLineSetup.exe [--install] [--target <dir>] [--quiet]`.
- [x] Implement `ClearLineSetup.exe --uninstall [--target <dir>] [--quiet]`.
- [x] Self-elevate with Windows `ShellExecuteW` + `runas` when not administrator.
- [x] Install to `%ProgramFiles%\ClearLine` by default.
- [x] Write uninstall registry entries including `UninstallString`.
- [x] Create Start Menu entry.
- [x] Call helper `install-driver`, `verify-install`, and `uninstall-driver`.

### Task 3: Build script without external installer prerequisites

**Files:**
- Modify: `clearline-installer/scripts/build-installer.ps1`

- [x] Build `clearline-installer-helper` in release mode.
- [x] Validate payload files.
- [x] With `-SkipCompile`, stop after payload validation.
- [x] Without `-SkipCompile`, build `clearline-setup` in release mode with strict payload embedding.
- [x] Copy `target/release/clearline-setup.exe` to `artifacts/installer/ClearLineSetup.exe`.
- [x] Run artifact verification after build.

### Task 4: Artifact verification

**Files:**
- Create/modify: `clearline-installer/scripts/verify-installer-artifact.ps1`

- [x] Verify `artifacts/installer/ClearLineSetup.exe` exists.
- [x] Verify plausible size.
- [x] Print SHA256.
- [x] Print version metadata and Authenticode status.
- [x] Warn, but do not fail, for unsigned development builds.

### Task 5: Installed/uninstalled acceptance verification

**Files:**
- Create: `clearline-installer/scripts/verify-installed-clearline.ps1`
- Create: `clearline-installer/scripts/verify-uninstalled-clearline.ps1`

- [x] Verify installed app/model/driver/helper files.
- [x] Run helper `verify-install --app`.
- [x] Verify uninstall registry entry.
- [x] Verify Start Menu entry.
- [x] Verify ClearLine PnP device unless skipped.
- [x] Verify uninstall removes app exe, registry entry, and PnP device.

### Task 6: Current verification

- [x] `cargo test -p clearline-setup`
- [x] `cargo check`
- [x] `build-installer.ps1 -SkipCompile`
- [x] `build-installer.ps1` produced `artifacts/installer/ClearLineSetup.exe`
- [x] `verify-installer-artifact.ps1` ran through build script

### Task 7: Manual install/uninstall cycle

- [ ] Run `artifacts/installer/ClearLineSetup.exe`.
- [ ] Run `verify-installed-clearline.ps1`.
- [ ] Uninstall via Windows Apps or `ClearLineSetup.exe --uninstall`.
- [ ] Run `verify-uninstalled-clearline.ps1`.

### Task 8: Administrator manifest and GUI subsystem verification

**Files:**
- Create: `clearline-setup/ClearLineSetup.exe.manifest`
- Modify: `clearline-setup/Cargo.toml`
- Modify: `clearline-setup/build.rs`
- Modify: `clearline-installer/scripts/verify-installer-artifact.ps1`
- Modify: `tests/test-installer-layout.ps1`

- [x] Embed a Windows application manifest with `requestedExecutionLevel level="requireAdministrator"`.
- [x] Keep release installer as Windows GUI subsystem so no console window opens.
- [x] Verify generated `ClearLineSetup.exe` has PE `Subsystem: 2`.
- [x] Verify generated `ClearLineSetup.exe` contains `requireAdministrator`.
- [x] Run full build/test/check verification after manifest embedding.

### Task 9: Setup logging for real install diagnostics

**Files:**
- Modify: `clearline-setup/src/main.rs`
- Modify: `tests/test-installer-layout.ps1`
- Modify: `README.md`
- Modify: `clearline-installer/README.md`

- [x] Create `%ProgramData%\ClearLine\logs\ClearLineSetup-*.log` at install/uninstall start.
- [x] Log install/uninstall steps.
- [x] Capture and log native helper stdout/stderr.
- [x] Capture and log registry command stdout/stderr.
- [x] Include log path in success/error message boxes.

### Task 10: Wait for elevated setup process

**Files:**
- Modify: `clearline-setup/Cargo.toml`
- Modify: `clearline-setup/src/main.rs`
- Modify: `tests/test-installer-layout.ps1`

- [x] Keep UAC self-elevation, but use `ShellExecuteExW` instead of fire-and-forget `ShellExecuteW`.
- [x] Use `SEE_MASK_NOCLOSEPROCESS` to receive the elevated setup process handle.
- [x] Wait for the elevated install/uninstall process with `WaitForSingleObject`.
- [x] Read and propagate the elevated process exit code with `GetExitCodeProcess`.
- [x] Close the process handle with `CloseHandle`.
- [x] Add a regression test for elevation argument construction.

### Task 11: Robust uninstall directory cleanup

**Files:**
- Modify: `clearline-setup/src/main.rs`
- Modify: `tests/test-installer-layout.ps1`

- [x] Reproduce real uninstall residue where registry/driver were removed but `C:\Program Files\ClearLine\ClearLine.exe` remained.
- [x] Replace background `cmd rmdir` cleanup with a hidden `--cleanup-install-dir` setup mode.
- [x] Copy the setup exe to `%ProgramData%\ClearLine\cleanup\ClearLineSetup-cleanup.exe` before deleting the install directory.
- [x] Launch the cleanup copy from `env::temp_dir()` so the cleanup process does not run from the install directory.
- [x] Delete the install directory from Rust with a 30-attempt retry loop and per-attempt logging.
- [x] Use `ping -n 2 127.0.0.1 >nul` only for deleting the temporary cleanup exe after it exits.
- [x] Add regression coverage for cleanup mode parsing and cleanup architecture markers.
