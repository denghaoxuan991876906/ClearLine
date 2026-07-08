#[cfg(not(windows))]
fn main() {
    println!("AudioPipeline AEC probe is only available on Windows");
}

#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::time::Duration;

    use clearline_core::{
        AudioPipeline, AudioPipelineConfig, CpalDeviceEnumerator, DeviceEnumerator,
        EchoCancellerBackend, SuppressorMode,
    };

    let enumerator = CpalDeviceEnumerator;
    let input_devices = enumerator.input_devices()?;
    let input_device = input_devices
        .iter()
        .find(|device| device.is_default())
        .or_else(|| input_devices.first())
        .ok_or_else(|| "no input microphone device found".to_owned())?;
    let output_device = enumerator
        .output_devices()?
        .into_iter()
        .find(|device| is_vb_cable_render_device_name(device.name()))
        .ok_or_else(|| {
            "VB-CABLE render endpoint not found: expected CABLE Input or CABLE In".to_owned()
        })?;

    let config = AudioPipelineConfig::new(
        input_device.id().clone(),
        output_device.id().clone(),
        SuppressorMode::LowLatency,
    )
    .with_echo_cancellation(true);
    let mut pipeline = AudioPipeline::new();
    pipeline.start(config)?;

    let runtime_info = pipeline
        .runtime_info()
        .ok_or_else(|| "pipeline did not publish runtime info".to_owned())?;
    let echo_backend = runtime_info.echo_cancellation().backend();
    if echo_backend != EchoCancellerBackend::Aec3 {
        return Err(format!("expected AEC3 backend, got {echo_backend:?}").into());
    }

    println!(
        "ClearLine AudioPipeline AEC probe started: input=\"{}\" output=\"{}\" backend={:?} input={} Hz / {} ch output={} Hz / {} ch",
        input_device.name(),
        output_device.name(),
        echo_backend,
        runtime_info.input_format().sample_rate_hz(),
        runtime_info.input_format().channels(),
        runtime_info.output_format().sample_rate_hz(),
        runtime_info.output_format().channels()
    );
    println!("Keep system audio playing during this 10s probe. Monitor CABLE Output if needed.");

    let mut max_reference_level = 0.0_f32;
    for second in 0..10 {
        std::thread::sleep(Duration::from_secs(1));
        let metrics = pipeline.metrics();
        let diagnostics = pipeline.echo_reference_diagnostics();
        if let Some(diagnostics) = diagnostics {
            max_reference_level = max_reference_level.max(diagnostics.level());
            println!(
                "t={second:02}s input_level={:.4} reference_level={:.4} reference_buffer={} missing_reference_frames={} dropped_reference_samples={} output_buffer={} underrun_samples={} dropped_output_samples={}",
                pipeline.input_level(),
                diagnostics.level(),
                diagnostics.buffered_samples(),
                diagnostics.missing_frames(),
                diagnostics.dropped_samples(),
                metrics.buffered_samples(),
                metrics.underrun_sample_count(),
                metrics.dropped_sample_count()
            );
        } else {
            println!(
                "t={second:02}s input_level={:.4} reference=unavailable output_buffer={} underrun_samples={} dropped_output_samples={}",
                pipeline.input_level(),
                metrics.buffered_samples(),
                metrics.underrun_sample_count(),
                metrics.dropped_sample_count()
            );
        }
    }

    pipeline.stop();

    if max_reference_level < 0.001 {
        return Err(
            "AudioPipeline AEC probe did not capture audible loopback reference audio".into(),
        );
    }

    println!(
        "ClearLine AudioPipeline AEC probe OK: max_reference_level={:.4}",
        max_reference_level
    );
    Ok(())
}

#[cfg(windows)]
fn is_vb_cable_render_device_name(name: &str) -> bool {
    let normalized = name.to_ascii_lowercase();
    (normalized.contains("cable input") || normalized.contains("cable in"))
        && !normalized.contains("cable-a")
        && !normalized.contains("cable-b")
        && !normalized.contains("cable-c")
        && !normalized.contains("cable-d")
}
