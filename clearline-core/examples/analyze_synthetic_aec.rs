#[cfg(not(feature = "aec"))]
fn main() {
    println!(
        "synthetic AEC analyzer requires: cargo run -p clearline-core --features aec --example analyze_synthetic_aec"
    );
}

#[cfg(feature = "aec")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use clearline_core::{
        run_echo_canceller_on_fixture, Aec3EchoWorker, AudioFrameFormat, GeneratedEchoFixture,
    };

    const SAMPLE_RATE_HZ: u32 = 48_000;
    const FRAMES_10MS: usize = 240;
    const MAX_OUTPUT_TO_INPUT_CORRELATION_RATIO: f32 = 0.85;
    const MIN_POWER_REDUCTION_DB: f32 = 1.5;

    let format = AudioFrameFormat::new(SAMPLE_RATE_HZ, 1);
    let mut canceller = Aec3EchoWorker::new(format)?;
    let fixture = GeneratedEchoFixture::new(SAMPLE_RATE_HZ, FRAMES_10MS);
    let metrics = run_echo_canceller_on_fixture(&mut canceller, &fixture)?;
    let passed = metrics.passes_reduction_thresholds(
        MAX_OUTPUT_TO_INPUT_CORRELATION_RATIO,
        MIN_POWER_REDUCTION_DB,
    );

    println!("ClearLine synthetic AEC analysis");
    println!(
        "format={} Hz / 1 ch frame_size={} samples frames_10ms={}",
        SAMPLE_RATE_HZ,
        fixture.frame_size_samples(),
        FRAMES_10MS
    );
    println!(
        "input_echo_correlation={:.4}",
        metrics.input_echo_correlation()
    );
    println!(
        "output_echo_correlation={:.4}",
        metrics.output_echo_correlation()
    );
    println!(
        "echo_power_reduction_db={:.2}",
        metrics.echo_power_reduction_db()
    );
    println!(
        "thresholds: output_corr < input_corr * {:.2}, power_reduction >= {:.1} dB",
        MAX_OUTPUT_TO_INPUT_CORRELATION_RATIO, MIN_POWER_REDUCTION_DB
    );

    if passed {
        println!("PASS");
        Ok(())
    } else {
        println!("FAIL");
        Err("synthetic AEC analysis did not meet thresholds".into())
    }
}
