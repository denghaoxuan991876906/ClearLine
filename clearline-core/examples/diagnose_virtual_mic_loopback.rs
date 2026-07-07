#[cfg(windows)]
use std::f32::consts::TAU;
#[cfg(windows)]
use std::sync::{Arc, Mutex};
#[cfg(windows)]
use std::thread;
#[cfg(windows)]
use std::time::{Duration, Instant};

#[cfg(windows)]
use clearline_core::VirtualMicControl;
#[cfg(windows)]
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
#[cfg(windows)]
use cpal::{FromSample, Sample, SampleFormat, SizedSample};

#[cfg(windows)]
const SAMPLE_RATE_HZ: usize = 48_000;
#[cfg(windows)]
const FREQUENCY_HZ: f32 = 440.0;
#[cfg(windows)]
const AMPLITUDE: f32 = 0.50;
#[cfg(windows)]
const CHUNK_MS: usize = 10;
#[cfg(windows)]
const RUN_SECONDS: u64 = 5;

#[cfg(windows)]
#[derive(Debug, Default)]
struct CaptureStats {
    samples: u64,
    sum_squares: f64,
    peak: f32,
}

#[cfg(windows)]
impl CaptureStats {
    fn push(&mut self, value: f32) {
        let value = value.clamp(-1.0, 1.0);
        self.samples += 1;
        self.sum_squares += (value as f64) * (value as f64);
        self.peak = self.peak.max(value.abs());
    }

    fn rms(&self) -> f64 {
        if self.samples == 0 {
            return 0.0;
        }
        (self.sum_squares / self.samples as f64).sqrt()
    }
}

#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let host = cpal::default_host();
    let device = host
        .input_devices()?
        .find(|device| device.to_string().contains("ClearLine"))
        .ok_or("ClearLine input device was not found")?;
    let supported_config = device.default_input_config()?;
    let sample_format = supported_config.sample_format();
    let stream_config: cpal::StreamConfig = supported_config.into();
    let stats = Arc::new(Mutex::new(CaptureStats::default()));
    let stream =
        build_capture_stream(&device, stream_config.clone(), sample_format, stats.clone())?;
    let control = VirtualMicControl::new();
    let before = control.buffer_status()?;

    println!(
        "Capturing from {:?} at {} Hz / {} ch / {:?}",
        device.to_string(),
        stream_config.sample_rate,
        stream_config.channels,
        sample_format
    );
    println!(
        "Injecting {FREQUENCY_HZ} Hz sine for {RUN_SECONDS}s and measuring captured RMS/peak."
    );

    stream.play()?;
    let injector = thread::spawn(move || inject_sine_for(Duration::from_secs(RUN_SECONDS)));
    thread::sleep(Duration::from_secs(RUN_SECONDS));
    drop(stream);

    let injector_result = injector.join().map_err(|_| "injector thread panicked")?;
    injector_result?;

    let after = VirtualMicControl::new().buffer_status()?;
    let stats = stats.lock().unwrap();
    println!(
        "Captured samples={} rms={:.6} peak={:.6}",
        stats.samples,
        stats.rms(),
        stats.peak
    );
    println!(
        "Driver status: readable {} -> {}, written {} -> {}, read {} -> {}, underruns {} -> {}, underrun_bytes {} -> {}",
        before.readable_bytes(),
        after.readable_bytes(),
        before.total_written_bytes(),
        after.total_written_bytes(),
        before.total_read_bytes(),
        after.total_read_bytes(),
        before.underrun_count(),
        after.underrun_count(),
        before.total_underrun_bytes(),
        after.total_underrun_bytes()
    );

    if stats.rms() < 0.01 || stats.peak < 0.05 {
        return Err(format!(
            "captured signal is too quiet: rms={:.6}, peak={:.6}",
            stats.rms(),
            stats.peak
        )
        .into());
    }

    println!("ClearLine virtual microphone loopback signal is non-silent.");
    Ok(())
}

#[cfg(not(windows))]
fn main() {
    println!("diagnose_virtual_mic_loopback is only available on Windows.");
}

#[cfg(windows)]
fn inject_sine_for(duration: Duration) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let control = VirtualMicControl::new();
    let samples_per_chunk = SAMPLE_RATE_HZ * CHUNK_MS / 1_000;
    let chunk_duration = Duration::from_millis(CHUNK_MS as u64);
    let start = Instant::now();
    let mut phase = 0.0f32;
    let phase_step = TAU * FREQUENCY_HZ / SAMPLE_RATE_HZ as f32;

    while start.elapsed() < duration {
        let loop_started = Instant::now();
        let mut samples = Vec::with_capacity(samples_per_chunk);
        for _ in 0..samples_per_chunk {
            samples.push((phase.sin() * AMPLITUDE * i16::MAX as f32) as i16);
            phase += phase_step;
            if phase >= TAU {
                phase -= TAU;
            }
        }
        control.write_pcm_i16_mono_48k(&samples)?;

        let elapsed = loop_started.elapsed();
        if elapsed < chunk_duration {
            thread::sleep(chunk_duration - elapsed);
        }
    }

    Ok(())
}

#[cfg(windows)]
fn build_capture_stream(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    sample_format: SampleFormat,
    stats: Arc<Mutex<CaptureStats>>,
) -> Result<cpal::Stream, Box<dyn std::error::Error + Send + Sync>> {
    match sample_format {
        SampleFormat::I8 => build_capture_stream_for::<i8>(device, config, stats),
        SampleFormat::I16 => build_capture_stream_for::<i16>(device, config, stats),
        SampleFormat::I32 => build_capture_stream_for::<i32>(device, config, stats),
        SampleFormat::I64 => build_capture_stream_for::<i64>(device, config, stats),
        SampleFormat::U8 => build_capture_stream_for::<u8>(device, config, stats),
        SampleFormat::U16 => build_capture_stream_for::<u16>(device, config, stats),
        SampleFormat::U32 => build_capture_stream_for::<u32>(device, config, stats),
        SampleFormat::U64 => build_capture_stream_for::<u64>(device, config, stats),
        SampleFormat::F32 => build_capture_stream_for::<f32>(device, config, stats),
        SampleFormat::F64 => build_capture_stream_for::<f64>(device, config, stats),
        unsupported => {
            Err(format!("unsupported ClearLine capture sample format: {unsupported:?}").into())
        }
    }
}

#[cfg(windows)]
fn build_capture_stream_for<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    stats: Arc<Mutex<CaptureStats>>,
) -> Result<cpal::Stream, Box<dyn std::error::Error + Send + Sync>>
where
    T: SizedSample + Sample + Send + 'static,
    f32: FromSample<T>,
{
    let stream = device.build_input_stream(
        config,
        move |data: &[T], _| {
            let mut stats = stats.lock().unwrap();
            for sample in data {
                stats.push(f32::from_sample(*sample));
            }
        },
        move |error| {
            eprintln!("ClearLine loopback capture stream error: {error}");
        },
        None,
    )?;
    Ok(stream)
}
