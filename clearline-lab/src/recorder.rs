use std::{
    fs,
    path::Path,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    thread,
    time::Duration,
};

use anyhow::{anyhow, bail, Context, Result};
use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    Device, SampleFormat, Stream, StreamConfig, SupportedStreamConfig,
};

use crate::RecordOptions;

pub fn list_devices() -> Result<()> {
    let host = cpal::default_host();
    let devices = input_devices(&host)?;

    if devices.is_empty() {
        println!("No input devices found.");
        return Ok(());
    }

    let default_device = host.default_input_device();

    println!("Input capture devices:");
    for (index, device) in devices.iter().enumerate() {
        let name = device.to_string();
        let marker = if default_device.as_ref() == Some(device) {
            " (default)"
        } else {
            ""
        };
        println!("  [{index}] {name}{marker}");
    }

    Ok(())
}

pub fn record_device(options: RecordOptions) -> Result<()> {
    if let Some(parent) = options
        .out
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create output directory {}", parent.display()))?;
    }

    let host = cpal::default_host();
    let devices = input_devices(&host)?;
    let device = select_input_device(&devices, &options.device_query)?;
    let device_name = device.to_string();
    let supported_config = device
        .default_input_config()
        .with_context(|| format!("failed to read default input config for {device_name}"))?;

    println!(
        "Recording {device_name} for {}s -> {}",
        options.duration.as_secs(),
        options.out.display()
    );
    println!(
        "Input format: {} Hz / {} channel(s) / {:?}",
        supported_config.sample_rate(),
        supported_config.channels(),
        supported_config.sample_format()
    );

    let recorded_frames =
        build_and_run_stream(&device, &supported_config, &options.out, options.duration)?;

    println!(
        "Recorded {recorded_frames} mono frame(s) to {}",
        options.out.display()
    );
    Ok(())
}

fn input_devices(host: &cpal::Host) -> Result<Vec<Device>> {
    host.input_devices()
        .context("failed to enumerate input devices")?
        .collect::<Vec<_>>()
        .pipe(Ok)
}

fn select_input_device(devices: &[Device], query: &str) -> Result<Device> {
    if let Ok(index) = query.parse::<usize>() {
        return devices
            .get(index)
            .cloned()
            .ok_or_else(|| anyhow!("input device index {index} was not found"));
    }

    let query = query.to_lowercase();
    let matches = devices
        .iter()
        .filter_map(|device| {
            let name = device.to_string();
            name.to_lowercase()
                .contains(&query)
                .then(|| (device.clone(), name))
        })
        .collect::<Vec<_>>();

    match matches.as_slice() {
        [(device, _name)] => Ok(device.clone()),
        [] => bail!("no input device matched '{query}'"),
        many => {
            let names = many
                .iter()
                .map(|(_, name)| format!("  - {name}"))
                .collect::<Vec<_>>()
                .join("\n");
            bail!("multiple input devices matched '{query}':\n{names}\nUse an index from `clearline-lab list`.")
        }
    }
}

fn build_and_run_stream(
    device: &Device,
    supported_config: &SupportedStreamConfig,
    out: &Path,
    duration: Duration,
) -> Result<u64> {
    let sample_rate = supported_config.sample_rate();
    let channels = supported_config.channels();
    let target_frames = u64::from(sample_rate) * duration.as_secs();
    let written_frames = Arc::new(AtomicU64::new(0));
    let writer = Arc::new(Mutex::new(Some(create_wav_writer(out, sample_rate)?)));
    let config = supported_config.config();

    let stream = match supported_config.sample_format() {
        SampleFormat::F32 => build_stream::<f32>(
            device,
            &config,
            channels,
            target_frames,
            Arc::clone(&written_frames),
            Arc::clone(&writer),
            |sample| sample,
        )?,
        SampleFormat::I16 => build_stream::<i16>(
            device,
            &config,
            channels,
            target_frames,
            Arc::clone(&written_frames),
            Arc::clone(&writer),
            |sample| sample as f32 / i16::MAX as f32,
        )?,
        SampleFormat::U16 => build_stream::<u16>(
            device,
            &config,
            channels,
            target_frames,
            Arc::clone(&written_frames),
            Arc::clone(&writer),
            |sample| (sample as f32 / u16::MAX as f32) * 2.0 - 1.0,
        )?,
        other => bail!("unsupported input sample format: {other:?}"),
    };

    stream.play().context("failed to start input stream")?;
    thread::sleep(duration + Duration::from_millis(250));
    drop(stream);

    let mut guard = writer
        .lock()
        .map_err(|_| anyhow!("wav writer lock poisoned"))?;
    if let Some(writer) = guard.take() {
        writer.finalize().context("failed to finalize WAV file")?;
    }

    Ok(written_frames.load(Ordering::SeqCst))
}

fn build_stream<T>(
    device: &Device,
    config: &StreamConfig,
    channels: u16,
    target_frames: u64,
    written_frames: Arc<AtomicU64>,
    writer: Arc<Mutex<Option<hound::WavWriter<std::io::BufWriter<std::fs::File>>>>>,
    convert: fn(T) -> f32,
) -> Result<Stream>
where
    T: cpal::SizedSample + Copy + Send + 'static,
{
    let channels = usize::from(channels.max(1));
    let error_callback = |error| eprintln!("input stream error: {error}");

    device
        .build_input_stream(
            config.clone(),
            move |data: &[T], _info| {
                write_input_data(
                    data,
                    channels,
                    target_frames,
                    &written_frames,
                    &writer,
                    convert,
                )
            },
            error_callback,
            None,
        )
        .context("failed to build input stream")
}

fn write_input_data<T>(
    data: &[T],
    channels: usize,
    target_frames: u64,
    written_frames: &AtomicU64,
    writer: &Mutex<Option<hound::WavWriter<std::io::BufWriter<std::fs::File>>>>,
    convert: fn(T) -> f32,
) where
    T: Copy,
{
    let Ok(mut guard) = writer.lock() else {
        return;
    };
    let Some(writer) = guard.as_mut() else {
        return;
    };

    for frame in data.chunks(channels) {
        let current = written_frames.load(Ordering::Relaxed);
        if current >= target_frames {
            break;
        }

        let mono = frame.iter().copied().map(convert).sum::<f32>() / frame.len().max(1) as f32;
        if writer.write_sample(mono.clamp(-1.0, 1.0)).is_err() {
            break;
        }
        written_frames.store(current + 1, Ordering::Relaxed);
    }
}

fn create_wav_writer(
    out: &Path,
    sample_rate: u32,
) -> Result<hound::WavWriter<std::io::BufWriter<std::fs::File>>> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };

    hound::WavWriter::create(out, spec)
        .with_context(|| format!("failed to create WAV file {}", out.display()))
}

trait Pipe: Sized {
    fn pipe<T>(self, f: impl FnOnce(Self) -> T) -> T {
        f(self)
    }
}

impl<T> Pipe for T {}
