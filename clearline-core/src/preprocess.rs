use crate::AudioFrameFormat;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AutoGainConfig {
    enabled: bool,
    gain: f32,
    limiter_ceiling: f32,
}

#[derive(Debug, Clone)]
pub struct AutoGainProcessor {
    config: AutoGainConfig,
}

impl AutoGainConfig {
    pub fn enabled() -> Self {
        Self {
            enabled: true,
            ..Self::default()
        }
    }

    pub fn disabled() -> Self {
        Self {
            enabled: false,
            ..Self::default()
        }
    }

    pub fn is_enabled(self) -> bool {
        self.enabled
    }

    pub fn gain(self) -> f32 {
        self.gain
    }

    pub fn limiter_ceiling(self) -> f32 {
        self.limiter_ceiling
    }
}

impl Default for AutoGainConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            gain: 2.0,
            limiter_ceiling: 0.891,
        }
    }
}

impl AutoGainProcessor {
    pub fn new(_format: AudioFrameFormat, config: AutoGainConfig) -> Self {
        Self { config }
    }

    pub fn is_enabled(&self) -> bool {
        self.config.is_enabled()
    }

    pub fn process_interleaved(&mut self, samples: &mut [f32]) {
        if !self.config.is_enabled() || samples.is_empty() {
            return;
        }

        let gain = self.config.gain().max(0.0);
        let ceiling = self.config.limiter_ceiling().clamp(0.1, 1.0);
        for sample in samples {
            let finite = if sample.is_finite() { *sample } else { 0.0 };
            *sample = (finite * gain).clamp(-ceiling, ceiling);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WindNoiseConfig {
    enabled: bool,
    high_pass_cutoff_hz: f32,
    impulse_threshold: f32,
    output_ceiling: f32,
}

#[derive(Debug, Clone)]
pub struct WindNoiseReducer {
    format: AudioFrameFormat,
    config: WindNoiseConfig,
    previous_input: Vec<f32>,
    previous_output: Vec<f32>,
}

impl WindNoiseConfig {
    pub fn enabled() -> Self {
        Self {
            enabled: true,
            ..Self::default()
        }
    }

    pub fn enabled_with_cutoff(high_pass_cutoff_hz: f32) -> Self {
        Self {
            enabled: true,
            high_pass_cutoff_hz: high_pass_cutoff_hz.max(1.0),
            ..Self::default()
        }
    }

    pub fn is_enabled(self) -> bool {
        self.enabled
    }

    pub fn high_pass_cutoff_hz(self) -> f32 {
        self.high_pass_cutoff_hz
    }

    pub fn impulse_threshold(self) -> f32 {
        self.impulse_threshold
    }

    pub fn output_ceiling(self) -> f32 {
        self.output_ceiling
    }
}

impl Default for WindNoiseConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            high_pass_cutoff_hz: 120.0,
            impulse_threshold: 1.25,
            output_ceiling: 0.92,
        }
    }
}

impl WindNoiseReducer {
    pub fn new(format: AudioFrameFormat, config: WindNoiseConfig) -> Self {
        let channels = usize::from(format.channels().max(1));
        Self {
            format,
            config,
            previous_input: vec![0.0; channels],
            previous_output: vec![0.0; channels],
        }
    }

    pub fn config(&self) -> WindNoiseConfig {
        self.config
    }

    pub fn process_interleaved(&mut self, samples: &mut [f32]) {
        if !self.config.is_enabled() {
            return;
        }

        let channels = usize::from(self.format.channels().max(1));
        let alpha = high_pass_alpha(
            self.format.sample_rate_hz(),
            self.config.high_pass_cutoff_hz(),
        );

        for (index, sample) in samples.iter_mut().enumerate() {
            let channel = index % channels;
            let input = sample.clamp(-1.0, 1.0);
            let high_passed =
                alpha * (self.previous_output[channel] + input - self.previous_input[channel]);
            self.previous_input[channel] = input;
            self.previous_output[channel] = high_passed;

            let impulse_limited = high_passed.clamp(
                -self.config.impulse_threshold(),
                self.config.impulse_threshold(),
            );
            *sample = soft_limit(impulse_limited, self.config.output_ceiling());
        }
    }

    pub fn reset(&mut self) {
        self.previous_input.fill(0.0);
        self.previous_output.fill(0.0);
    }
}

fn high_pass_alpha(sample_rate_hz: u32, cutoff_hz: f32) -> f32 {
    let sample_rate_hz = sample_rate_hz.max(1) as f32;
    let dt = 1.0 / sample_rate_hz;
    let rc = 1.0 / (2.0 * std::f32::consts::PI * cutoff_hz.max(1.0));
    rc / (rc + dt)
}

fn soft_limit(sample: f32, ceiling: f32) -> f32 {
    let ceiling = ceiling.clamp(0.1, 1.0);
    ceiling * (sample / ceiling).tanh()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_gain_is_enabled_by_default_config_helper() {
        assert!(AutoGainConfig::enabled().is_enabled());
        assert!(!AutoGainConfig::disabled().is_enabled());
    }

    #[test]
    fn auto_gain_applies_fixed_output_gain_to_processed_audio() {
        let format = AudioFrameFormat::new(48_000, 1);
        let mut processor = AutoGainProcessor::new(format, AutoGainConfig::enabled());
        let mut samples = vec![0.10_f32, -0.20, 0.30, -0.40];

        processor.process_interleaved(&mut samples);

        assert_close(samples[0], 0.20);
        assert_close(samples[1], -0.40);
        assert_close(samples[2], 0.60);
        assert_close(samples[3], -0.80);
    }

    #[test]
    fn auto_gain_applies_fixed_output_gain_to_near_silence() {
        let format = AudioFrameFormat::new(48_000, 1);
        let mut processor = AutoGainProcessor::new(format, AutoGainConfig::enabled());
        let mut samples = vec![0.001_f32, -0.002];

        processor.process_interleaved(&mut samples);

        assert_close(samples[0], 0.0020);
        assert_close(samples[1], -0.0040);
    }

    #[test]
    fn auto_gain_preserves_shape_below_limiter() {
        let format = AudioFrameFormat::new(48_000, 1);
        let mut processor = AutoGainProcessor::new(format, AutoGainConfig::enabled());
        let mut samples = vec![0.04_f32; 480];
        samples[120] = 0.40_f32;
        let original_ratio = samples[120].abs() / samples[0].abs();

        processor.process_interleaved(&mut samples);

        let processed_ratio = samples[120].abs() / samples[0].abs().max(f32::EPSILON);
        assert_close(processed_ratio, original_ratio);
    }

    #[test]
    fn auto_gain_limiter_keeps_output_below_ceiling() {
        let format = AudioFrameFormat::new(48_000, 1);
        let mut processor = AutoGainProcessor::new(format, AutoGainConfig::enabled());
        let mut samples = vec![2.0; 480];

        processor.process_interleaved(&mut samples);

        assert!(samples.iter().all(|sample| sample.is_finite()));
        assert!(samples.iter().all(|sample| sample.abs() <= 0.8911));
    }

    #[test]
    fn auto_gain_disabled_passes_samples_through() {
        let format = AudioFrameFormat::new(48_000, 1);
        let mut processor = AutoGainProcessor::new(format, AutoGainConfig::disabled());
        let mut samples = vec![0.05, -0.1, 0.2, -0.3];
        let before = samples.clone();

        processor.process_interleaved(&mut samples);

        assert_eq!(samples, before);
    }

    fn assert_close(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() <= 0.000_01,
            "expected {expected}, got {actual}"
        );
    }

    #[test]
    fn config_is_disabled_by_default() {
        let config = WindNoiseConfig::default();

        assert!(!config.is_enabled());
        assert_eq!(config.high_pass_cutoff_hz(), 120.0);
    }

    #[test]
    fn reducer_passes_samples_through_when_disabled() {
        let format = AudioFrameFormat::new(48_000, 1);
        let mut reducer = WindNoiseReducer::new(format, WindNoiseConfig::default());
        let mut samples = [0.0, 0.25, -0.5, 0.9];

        reducer.process_interleaved(&mut samples);

        assert_eq!(samples, [0.0, 0.25, -0.5, 0.9]);
    }

    #[test]
    fn reducer_attenuates_sustained_low_frequency_energy() {
        let format = AudioFrameFormat::new(48_000, 1);
        let mut reducer = WindNoiseReducer::new(format, WindNoiseConfig::enabled());
        let mut samples = vec![0.6; 4_800];

        reducer.process_interleaved(&mut samples);

        let tail_peak = samples[4_000..]
            .iter()
            .copied()
            .map(f32::abs)
            .fold(0.0, f32::max);
        assert!(tail_peak < 0.08, "tail peak was {tail_peak}");
    }

    #[test]
    fn reducer_limits_large_impulses_without_exceeding_unit_range() {
        let format = AudioFrameFormat::new(48_000, 1);
        let mut reducer = WindNoiseReducer::new(format, WindNoiseConfig::enabled());
        let mut samples = [0.0, 4.0, -4.0, 0.0];

        reducer.process_interleaved(&mut samples);

        assert!(samples.iter().all(|sample| sample.is_finite()));
        assert!(samples.iter().all(|sample| sample.abs() <= 1.0));
        assert!(samples[1].abs() < 0.95);
        assert!(samples[2].abs() < 0.95);
    }
}
