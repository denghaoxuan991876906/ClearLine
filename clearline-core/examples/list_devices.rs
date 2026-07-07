use clearline_core::device::{CpalDeviceEnumerator, DeviceEnumerator};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let enumerator = CpalDeviceEnumerator;

    println!("Input devices:");
    for device in enumerator.input_devices()? {
        println!(
            "- name={:?}; default={}; format={} Hz / {} ch; id={}",
            device.name(),
            device.is_default(),
            device
                .sample_rate_hz()
                .map(|value| value.to_string())
                .unwrap_or_else(|| "unknown".to_owned()),
            device
                .channels()
                .map(|value| value.to_string())
                .unwrap_or_else(|| "unknown".to_owned()),
            device.id().as_str()
        );
    }

    println!("Output devices:");
    for device in enumerator.output_devices()? {
        println!(
            "- name={:?}; default={}; format={} Hz / {} ch; id={}",
            device.name(),
            device.is_default(),
            device
                .sample_rate_hz()
                .map(|value| value.to_string())
                .unwrap_or_else(|| "unknown".to_owned()),
            device
                .channels()
                .map(|value| value.to_string())
                .unwrap_or_else(|| "unknown".to_owned()),
            device.id().as_str()
        );
    }

    Ok(())
}
