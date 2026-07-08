# Multi Sample Rate Input Design

## Goal

ClearLine should accept recording devices whose default input sample rate is not 48 kHz while keeping the current DeepFilterNet, echo cancellation, wind reduction, and virtual microphone output path stable at 48 kHz.

## Current problem

The ClearLine virtual microphone output contract is fixed at `48000 Hz / 1 channel`. The current virtual microphone pipeline rejects any selected input stream whose sample rate is not 48 kHz before stream startup. This prevents devices that expose default formats such as `44100 Hz`, `96000 Hz`, or `16000 Hz` from using the production pipeline.

## Chosen approach

Use `rubato 0.14` as the sample-rate conversion engine. The project already resolves `rubato 0.14.1` through the official DeepFilterNet Rust dependency, so adding a direct `rubato = "0.14"` dependency avoids pulling a second major version.

The internal processing domain remains 48 kHz:

1. CPAL captures the real microphone in its default input format.
2. The input callback converts samples to `f32` and, when needed, resamples the interleaved microphone stream to 48 kHz.
3. Input metering continues to use the real pre-resampled input so the UI reflects microphone activity immediately.
4. Echo cancellation, wind-noise reduction, and noise suppression run on the 48 kHz processing stream.
5. The virtual microphone writer mixes the processed stream to mono `i16` and writes 48 kHz PCM to the existing VB-CABLE / virtual microphone output path.

## Components

### Streaming sample-rate converter

Create a focused converter under `clearline-core`, for example `clearline-core/src/resample.rs`.

Responsibilities:

- Convert interleaved `f32` frames from `source_rate` to `target_rate`.
- Preserve channel count.
- Reuse allocations across callbacks.
- Return no output until enough source frames are buffered for the underlying resampler.
- Use direct copy when `source_rate == target_rate`.

The first implementation should target realtime voice reliability over broad format features. It only needs to support finite, positive sample rates and 1+ channels.

### Pipeline integration

In `AudioOutputTarget::ClearLineVirtualMicrophone`:

- Remove the startup error that rejects non-48k input streams.
- Build the suppressor, echo canceller, and wind reducer using a 48 kHz processing format with the original input channel count.
- Pass a converter into the input callback.
- Convert `scratch.input` into a new `scratch.processing_input` buffer before calling the shared processing function.

For normal `AudioDevice` passthrough output, keep the existing behavior for now. That path already asks the output stream to match the input sample rate and does not have the same fixed 48 kHz device contract.

### Echo reference handling

AEC only works correctly when capture and render reference frames use the same rate and frame length. The loopback reference buffer should therefore store mono reference samples in the processing domain when AEC is active. Add a target format parameter to reference capture so default-output loopback can resample to `48000 Hz / 1 channel` before buffering.

## Runtime information

Keep `PipelineRuntimeInfo::input_format()` as the real device input format. Keep `output_format()` as the actual output target format. The suppressor and echo runtime info already expose the processing format through their own `format()` fields, so the status tab can distinguish real input from 48 kHz processing without adding a new public field in this step.

## Testing

Use TDD. Unit tests should cover:

- `rubato` direct dependency is available through a real converter API.
- 48 kHz input uses direct copy.
- 44.1 kHz input produces approximately the correct number of 48 kHz output frames.
- 96 kHz input downsamples to approximately the correct number of 48 kHz output frames.
- Channel ordering is preserved for stereo streams.
- The virtual microphone pipeline source no longer contains the old non-48k rejection message.
- Reference capture can be configured with a processing target format.

Manual test after building on Windows:

1. Select a non-48 kHz microphone if available.
2. Start ClearLine.
3. Confirm the pipeline starts instead of reporting the old 48 kHz-only error.
4. Confirm the status page shows the real input format and 48 kHz output/processing path.
5. Record from `CABLE Output` and confirm audio is not chipmunk/slow/electronic.

## Out of scope

- Changing DeepFilterNet model sample rate.
- Supporting arbitrary output sample rates for the virtual microphone driver.
- Replacing VB-CABLE.
- UI redesign.
- Professional mastering-grade SRC tuning.
