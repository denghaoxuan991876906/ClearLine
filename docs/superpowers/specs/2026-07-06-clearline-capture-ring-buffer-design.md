# ClearLine Capture Ring Buffer Design

## Goal

Make `ClearLine Virtual Microphone` produce user-mode injected PCM by connecting the existing SYSVAD WaveRT capture stream to the ClearLine driver ring buffer.

This step proves the full local path:

```text
Rust example -> \\.\ClearLineControl -> driver PCM ring buffer -> WaveRT capture DMA buffer -> Windows recording endpoint
```

## Scope

Included:

- Capture stream reads from the ClearLine PCM ring buffer.
- Ring buffer exposes consumer metrics: total read bytes, underrun bytes, underrun count.
- When there is not enough injected PCM, capture fills the rest with silence.
- Add a Rust sine-wave injection example for manual recording tests.

Excluded:

- No resampler in this step.
- No app UI integration in this step.
- No main audio pipeline to driver wiring in this step.
- No replacement of SYSVAD WaveRT timing or packet logic.

## Recommended approach

Use the minimal SYSVAD hook point: `CMiniportWaveRTStream::WriteBytes()`.

SYSVAD already calls `WriteBytes(ByteDisplacement)` for capture streams as the WaveRT clock advances. Previously, that method generated a test sine tone into the DMA buffer. ClearLine will instead read bytes from the global ClearLine PCM ring buffer and copy them into `m_pDmaBuffer`. If fewer bytes are available than requested, the remaining DMA span is zero-filled.

## Alternatives considered

1. Rewrite WaveRT packet production.
   - More control, but high risk and likely to break SYSVAD's packet accounting.

2. Add a second virtual endpoint dedicated to injected PCM.
   - Cleaner separation, but it reintroduces endpoint complexity we just trimmed.

3. Keep the existing capture endpoint and replace only `WriteBytes()` data source.
   - Lowest risk. Preserves Windows capture endpoint behavior and only changes audio contents.

Chosen: option 3.

## Data contract

- Injected format: signed 16-bit little-endian PCM, mono, 48 kHz.
- Ring buffer stores raw PCM bytes.
- Capture stream consumes raw bytes at the stream's current DMA movement rate.
- For the first working version, tests should use 48 kHz mono recording settings.

If Windows opens the endpoint at another supported sample rate, the driver still copies bytes but does not resample. That may sound slow or fast; resampling is a later step.

## Error handling

- If the ring buffer is not initialized, capture writes silence.
- If the ring buffer underruns, capture writes available PCM then silence.
- Underrun counters allow user-mode diagnostics without crashing the audio engine.
- User-mode `buffer_status()` reports both producer and consumer metrics.

## Testing

Automated verification:

- Layout test checks that `WriteBytes()` calls `ClearLineReadPcmFromRingBuffer` and that status includes read/underrun counters.
- Rust unit test checks IOCTL codes and struct accessors.
- Cargo checks Linux and Windows example compilation.
- Driver build validates C++ and catalog generation.

Manual verification:

1. Reinstall the test driver.
2. Run `cargo run -p clearline-core --example inject_virtual_mic_sine`.
3. Record from `ClearLine Virtual Microphone` in Windows Sound Recorder or any recording app.
4. Confirm the recording contains a test tone.
5. Run `cargo run -p clearline-core --example inject_virtual_mic_silence` or a status example and confirm readable bytes can be consumed by active recording.
