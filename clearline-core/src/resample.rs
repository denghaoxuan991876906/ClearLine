use std::collections::VecDeque;

use rubato::{
    Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction,
};

use crate::{ClearLineError, ClearLineResult};

const TARGET_CALLBACK_MS: u32 = 10;

pub struct StreamingSampleRateConverter {
    source_rate_hz: u32,
    target_rate_hz: u32,
    channels: usize,
    chunk_frames: usize,
    pending_interleaved: VecDeque<f32>,
    input_channels: Vec<Vec<f32>>,
    output_channels: Vec<Vec<f32>>,
    resampler: Option<SincFixedIn<f32>>,
}

impl StreamingSampleRateConverter {
    pub fn new(source_rate_hz: u32, target_rate_hz: u32, channels: u16) -> ClearLineResult<Self> {
        let source_rate_hz = source_rate_hz.max(1);
        let target_rate_hz = target_rate_hz.max(1);
        let channels = usize::from(channels.max(1));
        let chunk_frames =
            ((u64::from(source_rate_hz) * u64::from(TARGET_CALLBACK_MS)) / 1_000).max(1) as usize;

        let resampler = if source_rate_hz == target_rate_hz {
            None
        } else {
            let params = SincInterpolationParameters {
                sinc_len: 128,
                f_cutoff: 0.95,
                interpolation: SincInterpolationType::Linear,
                oversampling_factor: 128,
                window: WindowFunction::BlackmanHarris2,
            };
            Some(
                SincFixedIn::<f32>::new(
                    target_rate_hz as f64 / source_rate_hz as f64,
                    1.0,
                    params,
                    chunk_frames,
                    channels,
                )
                .map_err(|error| ClearLineError::StreamBuild(error.to_string()))?,
            )
        };

        let mut input_channels = Vec::with_capacity(channels);
        let mut output_channels = Vec::with_capacity(channels);
        for _ in 0..channels {
            input_channels.push(vec![0.0; chunk_frames]);
            output_channels.push(Vec::new());
        }

        let mut converter = Self {
            source_rate_hz,
            target_rate_hz,
            channels,
            chunk_frames,
            pending_interleaved: VecDeque::with_capacity(chunk_frames * channels * 2),
            input_channels,
            output_channels,
            resampler,
        };
        converter.resize_output_buffers();
        Ok(converter)
    }

    pub fn source_rate_hz(&self) -> u32 {
        self.source_rate_hz
    }

    pub fn target_rate_hz(&self) -> u32 {
        self.target_rate_hz
    }

    pub fn channels(&self) -> u16 {
        self.channels as u16
    }

    pub fn process_interleaved(
        &mut self,
        input: &[f32],
        output: &mut Vec<f32>,
    ) -> ClearLineResult<()> {
        if self.resampler.is_none() {
            output.extend(input.iter().map(|sample| sample.clamp(-1.0, 1.0)));
            return Ok(());
        }

        let complete_samples = input.len() / self.channels * self.channels;
        self.pending_interleaved.extend(
            input[..complete_samples]
                .iter()
                .map(|sample| sample.clamp(-1.0, 1.0)),
        );

        let needed_samples = self.chunk_frames * self.channels;
        while self.pending_interleaved.len() >= needed_samples {
            for channel in &mut self.input_channels {
                channel.fill(0.0);
            }

            for frame_index in 0..self.chunk_frames {
                for channel_index in 0..self.channels {
                    let sample = self
                        .pending_interleaved
                        .pop_front()
                        .expect("pending sample count checked before resampling");
                    self.input_channels[channel_index][frame_index] = sample;
                }
            }

            self.resize_output_buffers();
            let (_, output_frames) = self
                .resampler
                .as_mut()
                .expect("resampler exists for non-direct conversion")
                .process_into_buffer(&self.input_channels, &mut self.output_channels, None)
                .map_err(|error| ClearLineError::StreamBuild(error.to_string()))?;

            append_interleaved_from_planar(&self.output_channels, output_frames, output);
        }

        Ok(())
    }

    fn resize_output_buffers(&mut self) {
        let output_frames = self
            .resampler
            .as_ref()
            .map(Resampler::output_frames_max)
            .unwrap_or(self.chunk_frames);
        for channel in &mut self.output_channels {
            channel.resize(output_frames, 0.0);
        }
    }
}

fn append_interleaved_from_planar(channels: &[Vec<f32>], frames: usize, output: &mut Vec<f32>) {
    if channels.is_empty() || frames == 0 {
        return;
    }

    output.reserve(frames * channels.len());
    for frame_index in 0..frames {
        for channel in channels {
            output.push(channel[frame_index].clamp(-1.0, 1.0));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resample_direct_copy_preserves_48k_mono_samples() {
        let mut converter = StreamingSampleRateConverter::new(48_000, 48_000, 1).unwrap();
        let mut output = Vec::new();

        converter
            .process_interleaved(&[0.0, 0.25, -0.5, 1.0], &mut output)
            .unwrap();

        assert_eq!(output, [0.0, 0.25, -0.5, 1.0]);
    }

    #[test]
    fn resample_44100_to_48000_produces_expected_frame_count() {
        let mut converter = StreamingSampleRateConverter::new(44_100, 48_000, 1).unwrap();
        let input = sine_wave(44_100, 1, 440.0, 4_410);
        let mut output = Vec::new();

        converter.process_interleaved(&input, &mut output).unwrap();

        assert!(
            (output.len() as isize - 4_800).abs() <= 128,
            "expected about 4800 output samples, got {}",
            output.len()
        );
        assert!(output.iter().any(|sample| sample.abs() > 0.01));
    }

    #[test]
    fn resample_96000_to_48000_downsamples_expected_frame_count() {
        let mut converter = StreamingSampleRateConverter::new(96_000, 48_000, 1).unwrap();
        let input = sine_wave(96_000, 1, 440.0, 9_600);
        let mut output = Vec::new();

        converter.process_interleaved(&input, &mut output).unwrap();

        assert!(
            (output.len() as isize - 4_800).abs() <= 128,
            "expected about 4800 output samples, got {}",
            output.len()
        );
        assert!(output.iter().any(|sample| sample.abs() > 0.01));
    }

    #[test]
    fn resample_stereo_preserves_interleaved_channel_order() {
        let mut converter = StreamingSampleRateConverter::new(48_000, 48_000, 2).unwrap();
        let mut output = Vec::new();

        converter
            .process_interleaved(&[0.1, -0.1, 0.2, -0.2, 0.3, -0.3], &mut output)
            .unwrap();

        assert_eq!(output, [0.1, -0.1, 0.2, -0.2, 0.3, -0.3]);
    }

    fn sine_wave(sample_rate_hz: u32, channels: u16, frequency_hz: f32, frames: usize) -> Vec<f32> {
        let mut samples = Vec::with_capacity(frames * usize::from(channels));
        for frame in 0..frames {
            let value =
                (std::f32::consts::TAU * frequency_hz * frame as f32 / sample_rate_hz as f32).sin()
                    * 0.5;
            for _ in 0..channels {
                samples.push(value);
            }
        }
        samples
    }
}
