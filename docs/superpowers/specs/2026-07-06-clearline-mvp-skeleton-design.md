# ClearLine MVP Skeleton Design

## Context

`/mnt/e/Dev/ClearLine` is not currently a git repository. The directory contains agent configuration files (`.agents/`, `skills-lock.json`) but no Rust project files. The first implementation will preserve the existing agent files and add a Rust workspace in place.

## Product Goal

ClearLine is a Windows-focused microphone noise suppression utility. A user selects a real microphone, ClearLine applies real-time noise suppression, and the processed audio is intended to be routed to an existing virtual audio line or virtual microphone device. Applications such as Discord, WeChat, QQ, browser meetings, and games can then use that virtual recording device.

## First-Round Scope

This round builds a compilable project skeleton and the core abstractions needed for later audio work:

- Enumerate audio input devices through a `DeviceEnumerator` abstraction.
- Store and resolve the selected input device through `InputDeviceSelector`.
- Define `NoiseSuppressor` and three implementations: bypass, low-latency placeholder, high-quality placeholder.
- Define `PipelineState` as `Stopped`, `Starting`, `Running`, and `Error`.
- Provide an `AudioPipeline` state/config shell without opening real audio streams yet.
- Provide a simple `eframe/egui` desktop UI shell.
- Document MVP phases and development commands.

This round explicitly excludes a self-authored Windows virtual audio driver, account/cloud features, multi-microphone mixing, and livestream sound-card features.

## Architecture

The workspace contains two crates:

```text
clearline-core  -> reusable device, suppressor, and pipeline abstractions
clearline-app   -> minimal desktop UI that depends on clearline-core
```

`clearline-core` owns all platform-facing and audio-pipeline concepts. `clearline-app` is a thin shell that displays devices, lets the user choose a mode, and triggers pipeline state transitions. This keeps the UI replaceable and lets later RNNoise, DeepFilterNet, virtual-line output, and Windows default-device integration land behind stable core interfaces.

## Core Components

### `device.rs`

- `DeviceId`: a small stable-looking wrapper for selected devices. In the CPAL implementation it is currently derived from enumeration index and device name because CPAL does not expose a universal stable hardware ID.
- `AudioInputDevice`: display name, ID, default flag, and optional default sample format hints.
- `DeviceEnumerator`: trait for testable input-device enumeration.
- `CpalDeviceEnumerator`: CPAL-backed implementation for current host input devices.
- `InputDeviceSelector`: selected-device state plus resolver against the latest device list.

### `suppressor.rs`

- `NoiseSuppressor`: mode reporting, `process(input, output)`, and `reset()`.
- `BypassSuppressor`: copies input to output and validates buffer sizes.
- `LowLatencySuppressor`: placeholder for RNNoise-like processing; currently delegates to bypass semantics.
- `HighQualitySuppressor`: placeholder for DeepFilterNet-like processing; currently delegates to bypass semantics.
- `SuppressorMode`: `Bypass`, `LowLatency`, `HighQuality`.

### `pipeline.rs`

- `PipelineState`: `Stopped`, `Starting`, `Running`, `Error(String)`.
- `AudioPipelineConfig`: selected input device and suppressor mode.
- `AudioPipeline`: holds state and config; first round transitions `Stopped -> Starting -> Running` without opening CPAL streams.

## UI Design

The UI uses `eframe/egui` with a minimal single-column utility layout:

- Clear heading and short status description.
- Device selector with visible label.
- Noise mode selector with visible label.
- Input level placeholder shown as text plus progress bar.
- Start/Stop buttons with disabled states where appropriate.
- Status text that does not rely on color alone.

The UI avoids decorative complexity, icons, accounts, navigation, or advanced visual effects. It follows a high-contrast minimal utility style suitable for a Windows desktop tool.

## Data Flow

1. App starts and asks `CpalDeviceEnumerator` for input devices.
2. User selects an `AudioInputDevice` by `DeviceId`.
3. User selects a `SuppressorMode`.
4. Pressing Start builds `AudioPipelineConfig` and calls `AudioPipeline::start`.
5. Pipeline enters `Running` if the config is accepted.
6. Pressing Stop calls `AudioPipeline::stop` and returns to `Stopped`.

## Error Handling

- Core returns `ClearLineResult<T>` with `ClearLineError` variants.
- CPAL enumeration errors are converted to `ClearLineError::DeviceEnumeration`.
- Buffer-size mismatches are reported as `ClearLineError::BufferSizeMismatch`.
- UI stores the latest human-readable status message and sets pipeline state to `Error` when start cannot proceed.

## Testing

Core behavior is covered with unit tests before implementation:

- Bypass copies input samples.
- Bypass rejects mismatched buffer lengths.
- Input selection resolves the selected device from an enumerated list.
- Pipeline starts and stops with expected states.
- Low-latency and high-quality placeholders preserve samples while exposing the correct mode.

`clearline-app` remains a thin UI shell for this round and is verified by `cargo check`.

## Deferred Work

- Replace low-latency placeholder with RNNoise or equivalent.
- Replace high-quality placeholder with DeepFilterNet or equivalent.
- Add real CPAL input stream, output stream, buffering, and level metering.
- Route processed audio to an existing virtual audio line / virtual microphone.
- Add Windows default recording-device integration behind a Windows-specific interface.
