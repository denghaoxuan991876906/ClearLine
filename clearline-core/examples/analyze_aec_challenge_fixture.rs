#[cfg(not(feature = "aec"))]
fn main() {
    eprintln!("This example requires: cargo run -p clearline-core --features aec --example analyze_aec_challenge_fixture");
    std::process::exit(2);
}

#[cfg(feature = "aec")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::{collections::HashMap, env, path::PathBuf};

    use clearline_core::{Aec3EchoCanceller, AudioFrameFormat, EchoCanceller};

    struct Fixture {
        fileid: u32,
        sample_rate_hz: u32,
        nearend_scale: f32,
        farend: Vec<f32>,
        nearend: Vec<f32>,
        mic: Vec<f32>,
    }

    fn fixture_root() -> PathBuf {
        env::var_os("CLEARLINE_AEC_FIXTURE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(".dev/aec-fixtures/aec-challenge"))
    }

    fn read_wav_mono_f32(
        path: &std::path::Path,
    ) -> Result<(u32, Vec<f32>), Box<dyn std::error::Error>> {
        let mut reader = hound::WavReader::open(path)?;
        let spec = reader.spec();
        if spec.channels != 1
            || spec.bits_per_sample != 16
            || spec.sample_format != hound::SampleFormat::Int
        {
            return Err(format!("{} must be mono 16-bit PCM", path.display()).into());
        }
        let samples = reader
            .samples::<i16>()
            .map(|sample| sample.map(|sample| sample as f32 / i16::MAX as f32))
            .collect::<Result<Vec<_>, _>>()?;
        Ok((spec.sample_rate, samples))
    }

    fn read_nearend_scale(
        path: &std::path::Path,
        fileid: u32,
    ) -> Result<f32, Box<dyn std::error::Error>> {
        let text = std::fs::read_to_string(path)?;
        let headers: Vec<&str> = text
            .lines()
            .next()
            .ok_or("empty meta.csv")?
            .split(',')
            .collect();
        let header_index: HashMap<&str, usize> = headers
            .iter()
            .enumerate()
            .map(|(index, header)| (*header, index))
            .collect();
        let fileid_index = *header_index.get("fileid").ok_or("missing fileid column")?;
        let scale_index = *header_index
            .get("nearend_scale")
            .ok_or("missing nearend_scale column")?;
        for line in text.lines().skip(1) {
            let columns: Vec<&str> = line.split(',').collect();
            if columns.get(fileid_index) == Some(&fileid.to_string().as_str()) {
                return Ok(columns[scale_index].parse()?);
            }
        }
        Err(format!("missing metadata for fileid {fileid}").into())
    }

    fn load_fixture(
        root: &std::path::Path,
        fileid: u32,
    ) -> Result<Fixture, Box<dyn std::error::Error>> {
        let synthetic = root.join("synthetic");
        let (sample_rate_hz, farend) = read_wav_mono_f32(
            &synthetic
                .join("farend_speech")
                .join(format!("farend_speech_fileid_{fileid}.wav")),
        )?;
        let (nearend_rate, nearend) = read_wav_mono_f32(
            &synthetic
                .join("nearend_speech")
                .join(format!("nearend_speech_fileid_{fileid}.wav")),
        )?;
        let (mic_rate, mic) = read_wav_mono_f32(
            &synthetic
                .join("nearend_mic_signal")
                .join(format!("nearend_mic_fileid_{fileid}.wav")),
        )?;
        if nearend_rate != sample_rate_hz || mic_rate != sample_rate_hz {
            return Err(format!("sample-rate mismatch for fileid {fileid}").into());
        }
        let nearend_scale = read_nearend_scale(&synthetic.join("meta.csv"), fileid)?;
        Ok(Fixture {
            fileid,
            sample_rate_hz,
            nearend_scale,
            farend,
            nearend,
            mic,
        })
    }

    fn process_fixture(fixture: &Fixture) -> Result<Vec<f32>, Box<dyn std::error::Error>> {
        let frame_size = (fixture.sample_rate_hz / 100) as usize;
        let process_count = fixture.farend.len().min(fixture.mic.len()) / frame_size * frame_size;
        let mut canceller =
            Aec3EchoCanceller::new(AudioFrameFormat::new(fixture.sample_rate_hz, 1))?;
        let mut output = vec![0.0; process_count];
        for ((render, capture), output_frame) in fixture.farend[..process_count]
            .chunks_exact(frame_size)
            .zip(fixture.mic[..process_count].chunks_exact(frame_size))
            .zip(output.chunks_exact_mut(frame_size))
        {
            canceller.process(capture, render, output_frame)?;
        }
        Ok(output)
    }

    fn residual(samples: &[f32], target: &[f32]) -> Vec<f32> {
        samples
            .iter()
            .zip(target.iter())
            .map(|(sample, target)| sample - target)
            .collect()
    }

    fn mean_square(samples: &[f32]) -> f32 {
        if samples.is_empty() {
            return 0.0;
        }
        samples.iter().map(|sample| sample * sample).sum::<f32>() / samples.len() as f32
    }

    fn absolute_correlation(a: &[f32], b: &[f32]) -> f32 {
        let len = a.len().min(b.len());
        if len == 0 {
            return 0.0;
        }
        let (a, b) = (&a[..len], &b[..len]);
        let a_mean = a.iter().sum::<f32>() / len as f32;
        let b_mean = b.iter().sum::<f32>() / len as f32;
        let mut numerator = 0.0;
        let mut a_energy = 0.0;
        let mut b_energy = 0.0;
        for (&a_sample, &b_sample) in a.iter().zip(b.iter()) {
            let a_centered = a_sample - a_mean;
            let b_centered = b_sample - b_mean;
            numerator += a_centered * b_centered;
            a_energy += a_centered * a_centered;
            b_energy += b_centered * b_centered;
        }
        if a_energy <= f32::EPSILON || b_energy <= f32::EPSILON {
            return 0.0;
        }
        (numerator / (a_energy.sqrt() * b_energy.sqrt())).abs()
    }

    let root = fixture_root();
    let fileids: Vec<u32> = env::args()
        .skip(1)
        .map(|arg| arg.parse())
        .collect::<Result<_, _>>()
        .unwrap_or_else(|_| vec![0, 1, 10]);
    let fileids = if fileids.is_empty() {
        vec![0, 1, 10]
    } else {
        fileids
    };

    println!("AEC-Challenge fixture root: {}", root.display());
    println!(
        "fileid,input_residual_db,output_residual_db,improvement_db,input_corr,output_corr,result"
    );
    let mut all_passed = true;
    for fileid in fileids {
        let fixture = load_fixture(&root, fileid)?;
        let output = process_fixture(&fixture)?;
        let target: Vec<f32> = fixture
            .nearend
            .iter()
            .map(|sample| sample * fixture.nearend_scale)
            .collect();
        let input_error = residual(&fixture.mic, &target);
        let output_error = residual(&output, &target);
        let input_power = mean_square(&input_error).max(1.0e-12);
        let output_power = mean_square(&output_error).max(1.0e-12);
        let input_residual_db = 10.0 * input_power.log10();
        let output_residual_db = 10.0 * output_power.log10();
        let improvement_db = 10.0 * (input_power / output_power).log10();
        let input_corr = absolute_correlation(&fixture.farend, &fixture.mic);
        let output_corr = absolute_correlation(&fixture.farend, &output);
        let passed = improvement_db > 1.0 && output_corr < input_corr;
        all_passed &= passed;
        println!(
            "{},{:.2},{:.2},{:.2},{:.4},{:.4},{}",
            fixture.fileid,
            input_residual_db,
            output_residual_db,
            improvement_db,
            input_corr,
            output_corr,
            if passed { "PASS" } else { "FAIL" }
        );
    }

    if all_passed {
        Ok(())
    } else {
        Err("one or more AEC-Challenge fixtures failed thresholds".into())
    }
}
