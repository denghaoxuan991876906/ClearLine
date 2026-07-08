#[cfg(not(windows))]
fn main() {
    println!("VB-CABLE sine injection probe is only available on Windows");
}

#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::f32::consts::TAU;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use cpal::{FromSample, Sample, SampleFormat, SizedSample};

    let host = cpal::default_host();
    let output_device = host
        .output_devices()?
        .find(|device| is_vb_cable_render_device_name(&device.to_string()))
        .ok_or_else(|| "VB-CABLE render endpoint not found: expected CABLE Input or CABLE In")?;
    let output_name = output_device.to_string();
    let supported_config = output_device.default_output_config()?;
    let sample_format = supported_config.sample_format();
    let config = supported_config.config();
    let channels = config.channels.max(1);
    let phase = Arc::new(Mutex::new(0.0_f32));

    let stream = match sample_format {
        SampleFormat::I8 => build_sine_stream::<i8>(&output_device, config.clone(), phase.clone()),
        SampleFormat::I16 => {
            build_sine_stream::<i16>(&output_device, config.clone(), phase.clone())
        }
        SampleFormat::I32 => {
            build_sine_stream::<i32>(&output_device, config.clone(), phase.clone())
        }
        SampleFormat::I64 => {
            build_sine_stream::<i64>(&output_device, config.clone(), phase.clone())
        }
        SampleFormat::U8 => build_sine_stream::<u8>(&output_device, config.clone(), phase.clone()),
        SampleFormat::U16 => {
            build_sine_stream::<u16>(&output_device, config.clone(), phase.clone())
        }
        SampleFormat::U32 => {
            build_sine_stream::<u32>(&output_device, config.clone(), phase.clone())
        }
        SampleFormat::U64 => {
            build_sine_stream::<u64>(&output_device, config.clone(), phase.clone())
        }
        SampleFormat::F32 => {
            build_sine_stream::<f32>(&output_device, config.clone(), phase.clone())
        }
        SampleFormat::F64 => {
            build_sine_stream::<f64>(&output_device, config.clone(), phase.clone())
        }
        unsupported => {
            return Err(format!("unsupported VB-CABLE sample format: {unsupported}").into())
        }
    }?;

    stream.play()?;
    println!(
        "Injecting 440 Hz sine into {output_name:?} for 20s: {} Hz / {} ch / {:?}. Monitor or record CABLE Output now.",
        config.sample_rate, channels, sample_format
    );
    std::thread::sleep(Duration::from_secs(20));
    drop(stream);
    println!("VB-CABLE sine injection finished");

    fn build_sine_stream<T>(
        device: &cpal::Device,
        config: cpal::StreamConfig,
        phase: Arc<Mutex<f32>>,
    ) -> Result<cpal::Stream, cpal::Error>
    where
        T: SizedSample + Sample + FromSample<f32> + Send + 'static,
    {
        let sample_rate = config.sample_rate as f32;
        let channels = usize::from(config.channels.max(1));
        device.build_output_stream(
            config,
            move |data: &mut [T], _| {
                let mut phase = phase
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                for frame in data.chunks_mut(channels) {
                    let sample = (*phase).sin() * 0.20;
                    *phase += TAU * 440.0 / sample_rate;
                    if *phase >= TAU {
                        *phase -= TAU;
                    }
                    for output in frame {
                        *output = T::from_sample(sample);
                    }
                }
            },
            move |error| eprintln!("VB-CABLE sine output stream error: {error}"),
            None,
        )
    }

    fn is_vb_cable_render_device_name(name: &str) -> bool {
        let normalized = name.to_ascii_lowercase();
        (normalized.contains("cable input") || normalized.contains("cable in"))
            && !normalized.contains("cable-a")
            && !normalized.contains("cable-b")
            && !normalized.contains("cable-c")
            && !normalized.contains("cable-d")
    }

    Ok(())
}
