use clearline_core::VirtualMicControl;

const SAMPLE_RATE_HZ: usize = 48_000;
const SILENCE_MS: usize = 100;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let control = VirtualMicControl::new();
    let before = control.buffer_status()?;

    let sample_count = SAMPLE_RATE_HZ * SILENCE_MS / 1_000;
    let silence = vec![0i16; sample_count];
    let accepted_bytes = control.write_pcm_i16_mono_48k(&silence)?;

    let after = control.buffer_status()?;
    println!(
        "ClearLine PCM injection OK: wrote={} bytes, readable {} -> {}, capacity={} bytes, read={}, underruns={}, underrun_bytes={}, overflows={}",
        accepted_bytes,
        before.readable_bytes(),
        after.readable_bytes(),
        after.capacity_bytes(),
        after.total_read_bytes(),
        after.underrun_count(),
        after.total_underrun_bytes(),
        after.overflow_count()
    );

    Ok(())
}
