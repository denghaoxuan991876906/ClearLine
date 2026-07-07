# ClearLine self-contained installer

This directory contains build and verification scripts for the native Rust ClearLine installer. The generated installer is a single self-contained `ClearLineSetup.exe` and does not require users to install any external setup runtime. Double-clicking `ClearLineSetup.exe` shows a native MSI-style install wizard where users can keep the default path or choose another install directory, then choose whether ClearLine should start with Windows.

The installer embeds:

- `dist/ClearLine.exe`
- `dist/models/deepfilternet/*` required by runtime
- local official basic `third_party/vb-cable/VBCABLE_Driver_Pack45.zip` (not tracked in Git)
- `target/release/clearline-installer-helper.exe`

`build-installer.ps1` builds `clearline-app` first and copies `target\release\clearline-app.exe` to `dist\ClearLine.exe`, so the generated installer uses the latest app binary.

Current backend: `vb-cable`.

- ClearLine uses VB-Audio VB-CABLE as the virtual audio device.
- VB-CABLE source: <https://www.vb-cable.com> / <https://vb-audio.com/Cable/>
- VB-CABLE is donationware and users may support/license it through VB-Audio.
- The VB-CABLE zip is a local build payload, not ClearLine-licensed source code.
- ClearLine outputs to the VB-CABLE render endpoint, shown as `CABLE Input` or `CABLE In 16 Ch` depending on the official VB-CABLE package version.
- User applications should select `CABLE Output` as the microphone.
- Setup saves the current Windows default render/capture endpoints before VB-CABLE installation and restores them afterwards so Windows does not silently leave VB-CABLE as the default speaker or microphone.

At install time, `ClearLineSetup.exe` asks for the install directory and startup preference, elevates with UAC when needed, extracts the embedded payload to the selected install directory, registers uninstall information, creates Start Menu entries for `ClearLine` and `卸载 ClearLine`, writes a double-clickable `installer\ClearLineUninstall.exe`, extracts the official VB-CABLE zip without modifying its `.inf`, `.sys`, or `.cat` files, writes/removes the current-user `HKCU\...\Run\ClearLine` startup entry according to the user's choice, and calls the native helper to create/bind one `VBAudioVACWDM` root-enumerated VB-CABLE device when `CABLE Input` / `CABLE Output` are not already present.

The single-device rule applies to the root-enumerated VB-CABLE driver devnode. Windows still exposes render/capture `AudioEndpoint` entries for that one devnode; those `SWD\MMDEVAPI` endpoints are expected and are not additional ClearLine-created root devices.

At uninstall time, the installed `ClearLineUninstall.exe`, Windows Apps & Features entry, or Start Menu uninstall entry removes registry/shortcut entries and deletes the install directory. GUI uninstall asks whether to remove VB-CABLE too. Quiet uninstall keeps VB-CABLE unless `--remove-vb-cable` is passed.

## Validate payload without building the installer

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\clearline-installer\scripts\build-installer.ps1 -SkipCompile
```

## Build installer

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\clearline-installer\scripts\build-installer.ps1
```

Output:

```text
artifacts\installer\ClearLineSetup.exe
```

When launched from a non-administrator command prompt, the setup exe triggers UAC and waits for the elevated install/uninstall process before returning its exit code. This keeps quiet-mode developer verification scripts tied to the real elevated result:

```powershell
.\artifacts\installer\ClearLineSetup.exe --quiet
```

For normal manual testing, double-click `artifacts\installer\ClearLineSetup.exe` and use the native path prompt instead of `--quiet`.

The build script automatically verifies the generated artifact. You can also run artifact verification manually:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\clearline-installer\scripts\verify-installer-artifact.ps1
```

The verifier prints file size, SHA256, version information, and Authenticode status. Unsigned development builds produce a warning but remain usable for local testing.

Install and uninstall logs are written to:

```text
%ProgramData%\ClearLine\logs\ClearLineSetup-*.log
```

The log records setup steps, registry commands, VB-CABLE zip extraction, helper stdout/stderr, `pnputil` output, single VB-CABLE root devnode creation/reuse/binding results, and final `CABLE Input` / `CABLE In 16 Ch` plus `CABLE Output` detection.

## Verify installed state

After running `ClearLineSetup.exe`, verify installed files, uninstall registry entry, Start Menu entry, helper verification, and the VB-CABLE endpoints:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\clearline-installer\scripts\verify-installed-clearline.ps1
```

To verify only files/registry while skipping the VB-CABLE endpoint check:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\clearline-installer\scripts\verify-installed-clearline.ps1 -SkipDevice
```

After uninstalling, verify cleanup:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\clearline-installer\scripts\verify-uninstalled-clearline.ps1
```

Quiet uninstall variants:

```powershell
# Keep VB-CABLE, the default quiet-uninstall behavior.
"C:\Program Files\ClearLine\installer\ClearLineUninstall.exe" --quiet --keep-vb-cable
powershell -NoProfile -ExecutionPolicy Bypass -File .\clearline-installer\scripts\verify-uninstalled-clearline.ps1 -ExpectVbCablePresent

# Remove VB-CABLE too.
"C:\Program Files\ClearLine\installer\ClearLineUninstall.exe" --quiet --remove-vb-cable
powershell -NoProfile -ExecutionPolicy Bypass -File .\clearline-installer\scripts\verify-uninstalled-clearline.ps1 -ExpectVbCableRemoved
```

## Notes

This installer requests administrator rights because it installs the official VB-CABLE driver when missing. The in-repo `clearline-driver/` self-driver is preserved but not installed by the default release path until a Microsoft-signed driver package is available. The current VB-CABLE backend does not require Windows TESTSIGNING.
