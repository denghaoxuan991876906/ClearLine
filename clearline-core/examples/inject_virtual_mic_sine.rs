use std::f32::consts::TAU;
use std::thread;
use std::time::{Duration, Instant};

use clearline_core::VirtualMicControl;

const SAMPLE_RATE_HZ: usize = 48_000;
const FREQUENCY_HZ: f32 = 440.0;
const AMPLITUDE: f32 = 0.25;
const CHUNK_MS: usize = 10;
const RUN_SECONDS: u64 = 30;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let control = VirtualMicControl::new();
    let samples_per_chunk = SAMPLE_RATE_HZ * CHUNK_MS / 1_000;
    let chunk_duration = Duration::from_millis(CHUNK_MS as u64);
    let start = Instant::now();
    let mut next_status_at = start;
    let mut phase = 0.0f32;
    let phase_step = TAU * FREQUENCY_HZ / SAMPLE_RATE_HZ as f32;

    println!(
        "Injecting {FREQUENCY_HZ} Hz sine into ClearLine Virtual Microphone for {RUN_SECONDS}s. Start recording from the ClearLine input now."
    );

    while start.elapsed() < Duration::from_secs(RUN_SECONDS) {
        let loop_started = Instant::now();
        let mut samples = Vec::with_capacity(samples_per_chunk);
        for _ in 0..samples_per_chunk {
            let sample = (phase.sin() * AMPLITUDE * i16::MAX as f32) as i16;
            samples.push(sample);
            phase += phase_step;
            if phase >= TAU {
                phase -= TAU;
            }
        }

        control.write_pcm_i16_mono_48k(&samples)?;

        let now = Instant::now();
        if now >= next_status_at {
            let status = control.buffer_status()?;
            println!(
                "t={:02}s readable={} written={} read={} underruns={} underrun_bytes={} overflows={}",
                start.elapsed().as_secs(),
                status.readable_bytes(),
                status.total_written_bytes(),
                status.total_read_bytes(),
                status.underrun_count(),
                status.total_underrun_bytes(),
                status.overflow_count()
            );
            next_status_at = now + Duration::from_secs(1);
        }

        let elapsed = loop_started.elapsed();
        if elapsed < chunk_duration {
            thread::sleep(chunk_duration - elapsed);
        }
    }

    let status = control.buffer_status()?;
    println!(
        "Done. readable={} written={} read={} dropped={} underruns={} underrun_bytes={} overflows={}",
        status.readable_bytes(),
        status.total_written_bytes(),
        status.total_read_bytes(),
        status.total_dropped_bytes(),
        status.underrun_count(),
        status.total_underrun_bytes(),
        status.overflow_count()
    );

    Ok(())
}
