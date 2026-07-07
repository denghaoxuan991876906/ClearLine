# ClearLine User Settings Persistence Design

## Context

ClearLine now has a usable Windows desktop flow: the user selects an input microphone, selects an output device, chooses low-latency or high-quality noise suppression, optionally enables wind-noise reduction, and can configure a local DeepFilterNet ONNX model directory. These choices currently reset when the app restarts, so repeated testing requires reselecting the same devices and options.

The next step is to persist these local preferences without adding accounts, cloud sync, registry dependency, or a larger settings UI.

## Product Goal

ClearLine should remember the user's last working local setup and restore it on the next launch. The app should still behave safely if devices are unplugged, renamed, or temporarily unavailable.

## Scope

Included:

- Persist app settings to a local JSON file.
- Restore settings on app startup after device enumeration.
- Save changes when the user changes device, mode, strength, wind-noise reduction, or model directory.
- Match restored devices by saved device ID first, then saved clean display name.
- Fall back to Windows default devices if saved devices are unavailable.
- Keep unavailable saved device choices in the JSON file instead of overwriting them during fallback.
- Surface save/load failures in the existing status message where useful.

Excluded:

- No account system.
- No cloud sync.
- No Windows registry storage.
- No large settings page redesign.
- No encrypted settings; the file contains only local device IDs/names and paths.
- No automatic DeepFilterNet model download.

## Storage Location

Use the Windows user configuration directory through the `directories` crate:

```text
%APPDATA%\ClearLine\settings.json
```

For non-Windows development and tests, the same abstraction can resolve to the platform's config directory or use explicit test paths.

The settings file should be formatted JSON so it is easy to inspect while ClearLine is still in early development.

## Data Model

The settings model should be app-owned, not core-owned, because it describes UI preferences and local app behavior.

Initial fields:

```json
{
  "version": 1,
  "input_device_id": "...",
  "input_device_name": "MCHOSE V9 Turbo+",
  "output_device_id": "...",
  "output_device_name": "VB-CABLE Input",
  "suppressor_mode": "low_latency",
  "suppression_strength": "balanced",
  "wind_noise_reduction_enabled": false,
  "deepfilter_model_dir": "E:\\Dev\\模型onnx"
}
```

Rules:

- `version` starts at `1` for future migrations.
- Device IDs are stored as CPAL-provided string IDs through `DeviceId::as_str()`.
- Device names store the already-cleaned display names from `AudioInputDevice::name()` and `AudioOutputDevice::name()`.
- `suppressor_mode` supports only user-selectable modes: `low_latency` and `high_quality`.
- Unknown enum strings should be ignored and replaced by safe defaults.
- Empty model directory is valid and means no DeepFilterNet model path is configured.

## Restore Behavior

Startup flow:

1. Build `ClearLineApp` with defaults.
2. Load settings from disk if the settings file exists.
3. Refresh input/output device lists.
4. Apply settings to non-device fields immediately.
5. Resolve saved input and output devices against current device lists:
   - first by saved device ID;
   - then by saved clean device name;
   - then by current Windows default device;
   - then by first available device.
6. Display a normal status message if settings were loaded and devices were found.
7. Display a fallback status message if saved devices were unavailable and defaults were selected.

Important: falling back to another current device must not immediately erase the saved unavailable device from disk. This lets the user close the app, plug the device back in, and still recover the previous selection on the next launch.

## Save Behavior

Save settings when the user intentionally changes a persistent field:

- input device selection;
- output device selection;
- suppressor mode;
- suppression strength;
- wind-noise reduction toggle;
- DeepFilterNet model directory text;
- model directory clear button.

Saving can be synchronous because the JSON file is tiny and user interactions are infrequent. If saving fails, keep the app usable and update the status message with a concise Chinese error.

Refresh behavior:

- Pressing Refresh may choose fallback devices if the saved/selected device disappeared.
- Refresh alone should not overwrite the stored device ID/name unless the user explicitly chooses a device from the combo box afterward.

## Components

### `clearline-app/src/settings.rs`

Create a small app settings module.

Responsibilities:

- Define `PersistedSettings` with serde serialization/deserialization.
- Define app-facing enums or conversion helpers for suppressor mode and strength strings.
- Provide `SettingsStore` for loading and saving JSON.
- Provide test helpers that use explicit temp file paths.

### `clearline-app/src/main.rs`

Integrate settings with the existing app state.

Responsibilities:

- Load settings in `ClearLineApp::new`.
- Apply settings after device enumeration.
- Save settings when UI selections change.
- Keep UI layout unchanged.
- Show concise Chinese save/load/fallback messages through existing `status_message`.

## Error Handling

- Missing settings file is not an error.
- Invalid JSON should be ignored and reported as `设置文件无效，已使用默认设置`.
- Save failure should not block audio use; show `设置保存失败：...`.
- Unknown enum values should not crash startup; use defaults.
- Invalid or missing DeepFilterNet model directory should still be shown in the text field, and the existing model status pill should report validity.

## Testing

Unit tests should cover:

- Settings JSON round-trip preserves all fields.
- Missing settings file loads as no settings.
- Invalid JSON returns a load error that the app can turn into a status message.
- Suppressor mode strings parse `low_latency` and `high_quality`; unknown strings use default.
- Strength strings parse `gentle`, `balanced`, `strong`; unknown strings use default.
- Device restore prefers ID match over name match.
- Device restore falls back to name if ID changes.
- Device restore falls back to default device if saved device is unavailable.
- App tests verify changing mode/strength/model path can produce a persisted settings snapshot.

Full verification remains:

- `cargo fmt --check`
- `cargo check`
- `cargo test -p clearline-core`
- `cargo test -p clearline-core --features rnnoise`
- `cargo test -p clearline-core --features deepfilternet`
- `cargo test -p clearline-core --features rnnoise,deepfilternet`
- `cargo test -p clearline-app`
- Windows `cargo.exe check -p clearline-app`
- Windows `cargo.exe check -p clearline-app --no-default-features`
- Windows `cargo.exe build -p clearline-app --release`

## Manual Test Plan

1. Run `dist/ClearLine.exe`.
2. Select input microphone, output device, mode, strength, wind-noise setting, and DeepFilterNet model directory.
3. Close ClearLine.
4. Reopen ClearLine.
5. Confirm the same choices are restored.
6. Temporarily unplug or disable the previously selected device.
7. Reopen ClearLine and confirm it falls back to an available/default device without crashing.
8. Reconnect the original device and reopen ClearLine.
9. Confirm the original saved device can be restored by ID or name.

## Acceptance Criteria

- ClearLine creates and reads `%APPDATA%\ClearLine\settings.json`.
- User choices are restored after app restart.
- Device restore handles ID match, name match, and default fallback.
- Missing or invalid settings files do not prevent startup.
- Save errors are visible but non-fatal.
- Existing audio pipeline, DeepFilterNet worker mode, RNNoise mode, and UI layout remain unchanged.
- A fresh Windows release exe is built and copied to `dist/ClearLine.exe`.
