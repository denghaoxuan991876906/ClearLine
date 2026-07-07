# ClearLine DeepFilterNet Background Inference Design

## Context

ClearLine already has a working high-quality mode that can load a local DeepFilterNet ONNX bundle and run it through the `deep_filter` / `tract` backend. The current backend name is `deepfilternet-tract`, and the model directory must contain `enc.onnx`, `erb_dec.onnx`, `df_dec.onnx`, and `config.ini`.

The current implementation runs DeepFilterNet inference synchronously inside the suppressor processing path. That proves model integration works, but it can block real-time audio callbacks on machines where ONNX inference is slower than the audio frame cadence. The next step is to isolate model inference from the audio path so high-quality mode stays stable under load.

## Product Goal

High-quality mode should keep using the real DeepFilterNet model, but audio callbacks should not wait indefinitely for model inference. The user-facing result should be fewer glitches, fewer underruns, and clearer status diagnostics when the model cannot keep up.

## Scope

This round implements the first background inference architecture for DeepFilterNet high-quality mode.

Included:

- Move DeepFilterNet inference to a dedicated worker thread.
- Add bounded input/output queues around the DeepFilterNet worker.
- Add timeout and overload handling so the audio path can continue when inference is late.
- Expose runtime diagnostics for the high-quality model worker.
- Show these diagnostics on the existing status tab.
- Keep the current synchronous loading path and adaptive fallback as safety behavior.

Excluded:

- No virtual audio driver work.
- No VB-CABLE guide.
- No major UI redesign.
- No model auto-download inside the app.
- No packaging/signing changes.
- No separate GPU/DirectML backend.

## Recommended Architecture

The selected approach is a dedicated worker thread owned by the DeepFilterNet backend.

```text
Audio path
  -> chunk interleaved samples into DeepFilterNet frames
  -> enqueue frame to bounded input queue
  -> dequeue latest processed frame from bounded output queue
  -> if output is not ready, use fallback concealment

DeepFilterNet worker thread
  -> receive input frame
  -> run df::tract::DfTract::process
  -> push processed frame to output queue
  -> update metrics
```

The worker is created after the model loads successfully. If model loading fails, the existing adaptive high-quality backend remains the fallback. If the worker cannot start, high-quality mode also falls back to adaptive instead of failing the whole pipeline.

## Components

### `DeepFilterNetBackgroundBackend`

A new internal backend under `#[cfg(feature = "deepfilternet")]`.

Responsibilities:

- Own the worker thread handle and stop signal.
- Own bounded queues shared with the worker.
- Report backend name, frame size, and runtime diagnostics.
- Convert interleaved audio frames into worker jobs.
- Return processed frames when available.
- Provide fallback concealment when output is late.

The backend name should clearly distinguish the new implementation, for example `deepfilternet-tract-worker`.

### Worker queues

Use standard-library synchronization first to avoid adding unnecessary dependencies:

- `std::sync::mpsc::sync_channel` for bounded input jobs.
- `std::sync::mpsc::sync_channel` or `try_recv`-drained receiver for processed output frames.
- Small fixed capacities, initially 2-4 frames, to prevent unbounded latency growth.

The audio path must use non-blocking queue operations. If the input queue is full, it drops the oldest or skips enqueueing the newest frame and records an overload counter. The first implementation should prefer skipping the newest frame because `std::sync::mpsc::SyncSender::try_send` supports this directly and it keeps the audio callback non-blocking.

### Fallback concealment

When no processed frame is ready in time, the audio path should not block. It should use this order:

1. Reuse the last processed output frame if available.
2. Otherwise output the current input frame at a conservative level.
3. If input/output shape is invalid, output zeros and report an error.

This is not a replacement for proper processing; it is a real-time safety behavior. Metrics should make these events visible.

### Runtime diagnostics

Extend suppressor runtime information enough for the status tab to show background worker health.

Initial diagnostics:

- Worker backend name.
- Worker input queue pressure.
- Worker output queue pressure when available.
- Total dropped input frames.
- Total late output frames.
- Last inference time in milliseconds.
- Rolling or latest maximum inference time in milliseconds.

The first implementation can use atomic counters plus a small metrics snapshot. It does not need full histograms.

## Data Flow

1. User selects high-quality mode and a valid DeepFilterNet model directory.
2. Pipeline builds `HighQualitySuppressor::new_with_deepfilternet_bundle`.
3. The DeepFilterNet model loads on start.
4. The backend starts one worker thread and creates bounded queues.
5. Audio input chunks samples into DeepFilterNet frame size.
6. Audio path enqueues each frame with `try_send`.
7. Worker receives a frame, runs `DfTract::process`, and pushes the result to the output queue.
8. Audio path drains available processed frames and emits the newest usable processed output.
9. Status tab reads runtime info and displays worker health.
10. On stop or reset, backend signals the worker to stop and clears queued frames.

## Error Handling

- Model-load errors stay mapped to `ClearLineError::ModelLoad` and fall back to adaptive high-quality mode.
- Inference errors increment an error counter and return a `ModelInference` error only if processing cannot safely continue.
- Queue-full events do not fail the pipeline; they increment drop/late counters.
- Worker shutdown should be best-effort and must not block app exit indefinitely.
- If worker thread panics, the backend should stop using it and report a degraded state. The first implementation can fall back to passthrough/concealment for that backend instance and surface diagnostics.

## UI Impact

No layout redesign.

Status tab should show, when DeepFilterNet worker backend is active:

- Backend: `deepfilternet-tract-worker`
- 降噪：`DeepFilterNet 已启用`
- 推理延迟：latest/max ms
- 推理状态：稳定 / 推理偏慢 / 已降级
- 丢弃帧 / 迟到帧 counters

Existing device tab behavior stays the same. The model directory field still points to the local ONNX bundle.

## Testing

Unit tests should cover the queue and fallback behavior without requiring the real ONNX model:

- Worker metrics start at zero.
- Late output falls back without blocking.
- Full input queue increments dropped-frame metrics.
- Runtime info reports the worker backend and frame size.
- Reset clears pending audio and stops or recreates worker state safely.

Feature-gated integration test should continue using `CLEARLINE_DF_MODEL_DIR` and verify:

- A real model starts as `deepfilternet-tract-worker`.
- Processing a frame returns finite samples.
- Diagnostics report at least one inference time after processing.

Existing verification remains:

- `cargo fmt --check`
- `cargo check`
- `cargo test -p clearline-core`
- `cargo test -p clearline-core --features rnnoise`
- `cargo test -p clearline-core --features deepfilternet`
- `cargo test -p clearline-core --features rnnoise,deepfilternet`
- `cargo test -p clearline-app`
- Windows `cargo.exe check -p clearline-app`
- Windows `cargo.exe build -p clearline-app --release`

## Manual Test Plan

1. Run `dist/ClearLine.exe`.
2. Select high-quality mode.
3. Set model directory to `E:\Dev\模型onnx`.
4. Start processing with the normal microphone and output device.
5. Open the status tab.
6. Confirm backend is `deepfilternet-tract-worker`.
7. Confirm `DeepFilterNet 已启用` is shown.
8. Speak normally and add fan/wind noise.
9. Watch inference latency, late frames, dropped frames, and buffer health.
10. Stop and start again to confirm the worker shuts down and restarts cleanly.

## Risks

- Extra buffering may add latency. Queue capacity should stay small and visible in diagnostics.
- If inference is consistently slower than real time, output quality can degrade because concealment is used often.
- Thread ownership and shutdown must be simple to avoid deadlocks during Stop/Start cycles.
- The official DeepFilterNet/tract model type is wrapped for `Send`; worker ownership must keep it single-threaded and never share it concurrently.

## Acceptance Criteria

- High-quality model backend no longer performs ONNX inference directly on the audio callback path.
- Valid model directory starts `deepfilternet-tract-worker`.
- Audio processing remains non-blocking when inference is late.
- Status tab exposes worker health and inference latency.
- Existing RNNoise, adaptive high-quality fallback, and device selection behavior remain unchanged.
- All automated checks pass.
- A fresh Windows release exe is built and copied to `dist/ClearLine.exe`.
