#[cfg(all(windows, feature = "aec"))]
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

#[cfg(not(windows))]
fn main() {
    println!("realtime AEC probe is only available on Windows");
}

#[cfg(all(windows, not(feature = "aec")))]
fn main() {
    println!("realtime AEC probe requires: cargo run -p clearline-core --features aec --example probe_realtime_aec");
}

#[cfg(all(windows, feature = "aec"))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    run_windows()
}

#[cfg(all(windows, feature = "aec"))]
fn run_windows() -> Result<(), Box<dyn std::error::Error>> {
    use std::time::{Duration, Instant};

    use clearline_core::{
        Aec3EchoWorker, AudioFrameFormat, EchoCanceller, LoopbackReferenceCapture,
        RealtimeAecProbeReport,
    };

    let host = cpal::default_host();
    let input_device = host
        .default_input_device()
        .ok_or_else(|| "default input device not found".to_owned())?;
    let input_name = input_device.to_string();
    let supported_input_config = input_device.default_input_config()?;
    let sample_format = supported_input_config.sample_format();
    let stream_config = supported_input_config.config();
    let input_format = AudioFrameFormat::new(stream_config.sample_rate, stream_config.channels);
    let input_channels = usize::from(input_format.channels().max(1));
    let frame_count = (input_format.sample_rate_hz() / 100).max(1) as usize;
    let frame_samples = frame_count * input_channels;

    let microphone_buffer = MicrophoneCaptureBuffer::new(frame_samples * 100);
    let input_stream = build_microphone_stream(
        &input_device,
        sample_format,
        stream_config,
        microphone_buffer.clone(),
    )?;
    input_stream.play()?;

    let reference_capture = LoopbackReferenceCapture::start_default(1_000)?;
    let reference_format = reference_capture.format();
    if reference_format.sample_rate_hz() != input_format.sample_rate_hz() {
        return Err(format!(
            "input/reference sample-rate mismatch: input={} Hz reference={} Hz",
            input_format.sample_rate_hz(),
            reference_format.sample_rate_hz()
        )
        .into());
    }

    let mut canceller = Aec3EchoWorker::new(input_format)?;
    let backend = canceller.runtime_info().backend();
    let mut capture_frame = vec![0.0; frame_samples];
    let mut reference_mono = vec![0.0; frame_count];
    let mut reference_frame = vec![0.0; frame_samples];
    let mut output_frame = vec![0.0; frame_samples];

    println!(
        "ClearLine realtime AEC probe started: input=\"{}\" {} Hz / {} ch, reference={} Hz / {} ch",
        input_name,
        input_format.sample_rate_hz(),
        input_format.channels(),
        reference_format.sample_rate_hz(),
        reference_format.channels()
    );
    println!("Keep system audio playing during this 10s probe.");

    let started_at = Instant::now();
    let mut next_print = started_at;
    let mut processed_frames = 0_u64;
    let mut max_capture_level = 0.0_f32;
    let mut max_reference_level = 0.0_f32;
    let mut max_output_level = 0.0_f32;

    while started_at.elapsed() < Duration::from_secs(10) {
        if !microphone_buffer.pop_frame(&mut capture_frame) {
            std::thread::sleep(Duration::from_millis(2));
            continue;
        }

        reference_capture.pop_mono_frame(&mut reference_mono);
        expand_mono_reference(&reference_mono, input_channels, &mut reference_frame);
        canceller.process(&capture_frame, &reference_frame, &mut output_frame)?;

        processed_frames += 1;
        max_capture_level = max_capture_level.max(peak_level(&capture_frame));
        max_reference_level = max_reference_level.max(peak_level(&reference_frame));
        max_output_level = max_output_level.max(peak_level(&output_frame));

        if Instant::now() >= next_print {
            let mic_stats = microphone_buffer.stats();
            let reference_stats = reference_capture.stats();
            let report = RealtimeAecProbeReport::new(backend)
                .with_processed_frames(processed_frames)
                .with_capture_level(max_capture_level)
                .with_reference_level(max_reference_level)
                .with_missing_reference_frames(reference_stats.missing_frames());
            println!(
                "t={:02}s {} output_level={:.4} mic_buffer={} mic_dropped={} reference_buffer={} reference_dropped={}",
                started_at.elapsed().as_secs(),
                report.summary_line(),
                max_output_level,
                mic_stats.buffered_samples,
                mic_stats.dropped_samples,
                reference_stats.buffered_samples(),
                reference_stats.dropped_samples()
            );
            next_print += Duration::from_secs(1);
        }
    }

    let reference_stats = reference_capture.stats();
    let final_report = RealtimeAecProbeReport::new(backend)
        .with_processed_frames(processed_frames)
        .with_capture_level(max_capture_level)
        .with_reference_level(max_reference_level)
        .with_missing_reference_frames(reference_stats.missing_frames());

    if !final_report.has_processed_audio() {
        return Err("realtime AEC probe did not process microphone frames".into());
    }
    if !final_report.has_reference_audio(0.001) {
        return Err("realtime AEC probe did not capture audible loopback reference audio".into());
    }

    println!(
        "ClearLine realtime AEC probe OK: {} output_level={:.4}",
        final_report.summary_line(),
        max_output_level
    );

    Ok(())
}

#[cfg(all(windows, feature = "aec"))]
#[derive(Debug, Clone)]
struct MicrophoneCaptureBuffer {
    inner: std::sync::Arc<std::sync::Mutex<MicrophoneCaptureState>>,
    capacity_samples: usize,
}

#[cfg(all(windows, feature = "aec"))]
#[derive(Debug)]
struct MicrophoneCaptureState {
    samples: std::collections::VecDeque<f32>,
    dropped_samples: u64,
}

#[cfg(all(windows, feature = "aec"))]
#[derive(Debug, Clone, Copy)]
struct MicrophoneCaptureStats {
    buffered_samples: usize,
    dropped_samples: u64,
}

#[cfg(all(windows, feature = "aec"))]
impl MicrophoneCaptureBuffer {
    fn new(capacity_samples: usize) -> Self {
        let capacity_samples = capacity_samples.max(1);
        Self {
            inner: std::sync::Arc::new(std::sync::Mutex::new(MicrophoneCaptureState {
                samples: std::collections::VecDeque::with_capacity(capacity_samples),
                dropped_samples: 0,
            })),
            capacity_samples,
        }
    }

    fn push_samples(&self, samples: &[f32]) {
        let mut state = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for sample in samples {
            if state.samples.len() >= self.capacity_samples {
                state.samples.pop_front();
                state.dropped_samples += 1;
            }
            state.samples.push_back(sample.clamp(-1.0, 1.0));
        }
    }

    fn pop_frame(&self, output: &mut [f32]) -> bool {
        let mut state = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if state.samples.len() < output.len() {
            return false;
        }

        for sample in output {
            *sample = state
                .samples
                .pop_front()
                .expect("microphone frame availability checked before pop");
        }
        true
    }

    fn stats(&self) -> MicrophoneCaptureStats {
        let state = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        MicrophoneCaptureStats {
            buffered_samples: state.samples.len(),
            dropped_samples: state.dropped_samples,
        }
    }
}

#[cfg(all(windows, feature = "aec"))]
fn build_microphone_stream(
    device: &cpal::Device,
    sample_format: cpal::SampleFormat,
    config: cpal::StreamConfig,
    buffer: MicrophoneCaptureBuffer,
) -> clearline_core::ClearLineResult<cpal::Stream> {
    match sample_format {
        cpal::SampleFormat::I8 => build_typed_microphone_stream::<i8>(device, config, buffer),
        cpal::SampleFormat::I16 => build_typed_microphone_stream::<i16>(device, config, buffer),
        cpal::SampleFormat::I32 => build_typed_microphone_stream::<i32>(device, config, buffer),
        cpal::SampleFormat::I64 => build_typed_microphone_stream::<i64>(device, config, buffer),
        cpal::SampleFormat::U8 => build_typed_microphone_stream::<u8>(device, config, buffer),
        cpal::SampleFormat::U16 => build_typed_microphone_stream::<u16>(device, config, buffer),
        cpal::SampleFormat::U32 => build_typed_microphone_stream::<u32>(device, config, buffer),
        cpal::SampleFormat::U64 => build_typed_microphone_stream::<u64>(device, config, buffer),
        cpal::SampleFormat::F32 => build_typed_microphone_stream::<f32>(device, config, buffer),
        cpal::SampleFormat::F64 => build_typed_microphone_stream::<f64>(device, config, buffer),
        unsupported => Err(clearline_core::ClearLineError::UnsupportedSampleFormat(
            format!("microphone input {unsupported}"),
        )),
    }
}

#[cfg(all(windows, feature = "aec"))]
fn build_typed_microphone_stream<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    buffer: MicrophoneCaptureBuffer,
) -> clearline_core::ClearLineResult<cpal::Stream>
where
    T: cpal::SizedSample + cpal::Sample + Send + 'static,
    f32: cpal::FromSample<T>,
{
    device
        .build_input_stream(
            config,
            move |data: &[T], _| {
                let samples = data
                    .iter()
                    .map(|sample| sample.to_sample::<f32>())
                    .collect::<Vec<_>>();
                buffer.push_samples(&samples);
            },
            move |error| {
                eprintln!("ClearLine realtime microphone probe stream error: {error}");
            },
            None,
        )
        .map_err(|error| clearline_core::ClearLineError::StreamBuild(error.to_string()))
}

#[cfg(all(windows, feature = "aec"))]
fn expand_mono_reference(reference_mono: &[f32], channels: usize, output: &mut [f32]) {
    if channels <= 1 {
        output[..reference_mono.len()].copy_from_slice(reference_mono);
        return;
    }

    for (frame_index, sample) in reference_mono.iter().copied().enumerate() {
        let start = frame_index * channels;
        let end = start + channels;
        output[start..end].fill(sample);
    }
}

#[cfg(all(windows, feature = "aec"))]
fn peak_level(samples: &[f32]) -> f32 {
    samples
        .iter()
        .copied()
        .map(f32::abs)
        .fold(0.0_f32, f32::max)
        .min(1.0)
}
