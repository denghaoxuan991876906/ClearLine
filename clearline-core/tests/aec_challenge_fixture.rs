#![cfg(feature = "aec")]

use std::{collections::HashMap, env, path::PathBuf};

use clearline_core::{Aec3EchoWorker, AudioFrameFormat, EchoCanceller};

#[test]
#[ignore = "requires official Microsoft AEC-Challenge fixtures downloaded by scripts/download-aec-fixtures.py"]
fn aec3_reduces_residual_echo_on_official_synthetic_fixture() {
    for fileid in [0, 1, 10] {
        let fixture = AecChallengeFixture::load(fileid);
        let mut canceller = Aec3EchoWorker::new(AudioFrameFormat::new(fixture.sample_rate_hz, 1))
            .expect("create AEC3 worker");
        let output = process_fixture(&mut canceller, &fixture);
        let metrics = OfficialFixtureMetrics::from_fixture(&fixture, &output);

        eprintln!(
            "fileid={} input_residual_db={:.2} output_residual_db={:.2} improvement_db={:.2} input_corr={:.3} output_corr={:.3}",
            fixture.fileid,
            metrics.input_residual_db,
            metrics.output_residual_db,
            metrics.residual_improvement_db,
            metrics.input_farend_correlation,
            metrics.output_farend_correlation
        );

        assert!(
            metrics.residual_improvement_db > 1.0,
            "expected official fixture {fileid} residual echo/noise error to improve by >1 dB, got {:.2} dB",
            metrics.residual_improvement_db
        );
        assert!(
            metrics.output_farend_correlation < metrics.input_farend_correlation,
            "expected fixture {fileid} farend correlation to decrease: input {:.3}, output {:.3}",
            metrics.input_farend_correlation,
            metrics.output_farend_correlation
        );
    }
}

struct AecChallengeFixture {
    fileid: u32,
    sample_rate_hz: u32,
    nearend_scale: f32,
    farend: Vec<f32>,
    nearend: Vec<f32>,
    mic: Vec<f32>,
}

impl AecChallengeFixture {
    fn load(fileid: u32) -> Self {
        let root = env::var_os("CLEARLINE_AEC_FIXTURE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .parent()
                    .expect("clearline-core has workspace parent")
                    .join(".dev")
                    .join("aec-fixtures")
                    .join("aec-challenge")
            });
        let synthetic = root.join("synthetic");
        assert!(
            synthetic.exists(),
            "missing official AEC fixture dir {}; run: python3 scripts/download-aec-fixtures.py",
            synthetic.display()
        );

        let farend = read_wav_mono_f32(
            &synthetic
                .join("farend_speech")
                .join(format!("farend_speech_fileid_{fileid}.wav")),
        );
        let nearend = read_wav_mono_f32(
            &synthetic
                .join("nearend_speech")
                .join(format!("nearend_speech_fileid_{fileid}.wav")),
        );
        let mic = read_wav_mono_f32(
            &synthetic
                .join("nearend_mic_signal")
                .join(format!("nearend_mic_fileid_{fileid}.wav")),
        );
        let sample_rate_hz = farend.0;
        assert_eq!(nearend.0, sample_rate_hz);
        assert_eq!(mic.0, sample_rate_hz);
        let nearend_scale = read_nearend_scale(&synthetic.join("meta.csv"), fileid);

        Self {
            fileid,
            sample_rate_hz,
            nearend_scale,
            farend: farend.1,
            nearend: nearend.1,
            mic: mic.1,
        }
    }

    fn target_nearend(&self) -> Vec<f32> {
        self.nearend
            .iter()
            .map(|sample| sample * self.nearend_scale)
            .collect()
    }
}

struct OfficialFixtureMetrics {
    input_residual_db: f32,
    output_residual_db: f32,
    residual_improvement_db: f32,
    input_farend_correlation: f32,
    output_farend_correlation: f32,
}

impl OfficialFixtureMetrics {
    fn from_fixture(fixture: &AecChallengeFixture, output: &[f32]) -> Self {
        let target = fixture.target_nearend();
        let input_error = residual(&fixture.mic, &target);
        let output_error = residual(output, &target);
        let input_residual_power = mean_square(&input_error);
        let output_residual_power = mean_square(&output_error);
        let input_residual_db = power_db(input_residual_power);
        let output_residual_db = power_db(output_residual_power);
        let residual_improvement_db =
            10.0 * (input_residual_power.max(1.0e-12) / output_residual_power.max(1.0e-12)).log10();
        let input_farend_correlation = absolute_correlation(&fixture.farend, &fixture.mic);
        let output_farend_correlation = absolute_correlation(&fixture.farend, output);

        Self {
            input_residual_db,
            output_residual_db,
            residual_improvement_db,
            input_farend_correlation,
            output_farend_correlation,
        }
    }
}

fn process_fixture(canceller: &mut dyn EchoCanceller, fixture: &AecChallengeFixture) -> Vec<f32> {
    let frame_size = (fixture.sample_rate_hz / 100) as usize;
    let sample_count = fixture.farend.len().min(fixture.mic.len());
    let process_count = sample_count / frame_size * frame_size;
    let mut output = vec![0.0; process_count];

    for ((render, capture), output_frame) in fixture.farend[..process_count]
        .chunks_exact(frame_size)
        .zip(fixture.mic[..process_count].chunks_exact(frame_size))
        .zip(output.chunks_exact_mut(frame_size))
    {
        canceller
            .process(capture, render, output_frame)
            .expect("process AEC frame");
    }

    output
}

fn read_wav_mono_f32(path: &std::path::Path) -> (u32, Vec<f32>) {
    let mut reader = hound::WavReader::open(path)
        .unwrap_or_else(|error| panic!("failed to open {}: {error}", path.display()));
    let spec = reader.spec();
    assert_eq!(spec.channels, 1, "{} must be mono", path.display());
    assert_eq!(spec.sample_format, hound::SampleFormat::Int);
    assert_eq!(spec.bits_per_sample, 16);
    let samples = reader
        .samples::<i16>()
        .map(|sample| sample.expect("valid wav sample") as f32 / i16::MAX as f32)
        .collect();
    (spec.sample_rate, samples)
}

fn read_nearend_scale(path: &std::path::Path, fileid: u32) -> f32 {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
    let headers: Vec<&str> = text
        .lines()
        .next()
        .expect("meta header")
        .split(',')
        .collect();
    let header_index: HashMap<&str, usize> = headers
        .iter()
        .enumerate()
        .map(|(index, header)| (*header, index))
        .collect();
    let fileid_index = header_index["fileid"];
    let scale_index = header_index["nearend_scale"];
    for line in text.lines().skip(1) {
        let columns: Vec<&str> = line.split(',').collect();
        if columns[fileid_index] == fileid.to_string() {
            return columns[scale_index].parse().expect("nearend_scale is f32");
        }
    }
    panic!("missing metadata for fileid {fileid}");
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

fn power_db(power: f32) -> f32 {
    10.0 * power.max(1.0e-12).log10()
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
