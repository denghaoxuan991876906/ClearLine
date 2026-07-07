#[cfg(not(windows))]
fn main() {
    println!("loopback reference probe is only available on Windows");
}

#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::time::Duration;

    use clearline_core::LoopbackReferenceCapture;

    let capture = LoopbackReferenceCapture::start_default(1_000)?;
    let format = capture.format();
    println!(
        "ClearLine loopback reference capture OK: {} Hz / {} ch. Play system audio now.",
        format.sample_rate_hz(),
        format.channels()
    );

    for second in 0..10 {
        std::thread::sleep(Duration::from_secs(1));
        let stats = capture.stats();
        println!(
            "t={second:02}s level={:.4} buffered={} missing_frames={} dropped_samples={}",
            stats.last_level(),
            stats.buffered_samples(),
            stats.missing_frames(),
            stats.dropped_samples()
        );
    }

    Ok(())
}
