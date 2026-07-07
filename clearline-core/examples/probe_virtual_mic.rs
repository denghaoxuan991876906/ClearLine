use clearline_core::VirtualMicControl;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let control = VirtualMicControl::new();
    let response = control.ping()?;

    println!(
        "ClearLine virtual microphone control OK: version={} format={} Hz / {} ch",
        response.version(),
        response.sample_rate_hz(),
        response.channels()
    );

    Ok(())
}
