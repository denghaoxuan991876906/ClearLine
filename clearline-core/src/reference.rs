use std::collections::VecDeque;

use crate::AudioFrameFormat;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReferenceCaptureStats {
    last_level: f32,
    missing_frames: u64,
    dropped_samples: u64,
    buffered_samples: usize,
}

impl ReferenceCaptureStats {
    pub fn last_level(&self) -> f32 {
        self.last_level
    }

    pub fn missing_frames(&self) -> u64 {
        self.missing_frames
    }

    pub fn dropped_samples(&self) -> u64 {
        self.dropped_samples
    }

    pub fn buffered_samples(&self) -> usize {
        self.buffered_samples
    }
}

#[derive(Debug, Clone)]
pub struct ReferenceFrameBuffer {
    format: AudioFrameFormat,
    capacity_samples: usize,
    samples: VecDeque<f32>,
    stats: ReferenceCaptureStats,
}

impl ReferenceFrameBuffer {
    pub fn new(format: AudioFrameFormat, capacity_samples: usize) -> Self {
        let capacity_samples = capacity_samples.max(1);
        Self {
            format,
            capacity_samples,
            samples: VecDeque::with_capacity(capacity_samples),
            stats: ReferenceCaptureStats {
                last_level: 0.0,
                missing_frames: 0,
                dropped_samples: 0,
                buffered_samples: 0,
            },
        }
    }

    pub fn format(&self) -> AudioFrameFormat {
        self.format
    }

    pub fn stats(&self) -> ReferenceCaptureStats {
        ReferenceCaptureStats {
            buffered_samples: self.samples.len(),
            ..self.stats
        }
    }

    pub fn push_interleaved(&mut self, interleaved: &[f32]) {
        let channels = usize::from(self.format.channels().max(1));
        let mut peak = 0.0f32;
        let mut saw_frame = false;
        for frame in interleaved.chunks_exact(channels) {
            let mono = frame.iter().copied().sum::<f32>() / channels as f32;
            peak = peak.max(mono.abs());
            saw_frame = true;
            if self.samples.len() >= self.capacity_samples {
                self.samples.pop_front();
                self.stats.dropped_samples += 1;
            }
            self.samples.push_back(mono);
        }
        if saw_frame {
            self.stats.last_level = peak.min(1.0);
        }
        self.stats.buffered_samples = self.samples.len();
    }

    pub fn pop_mono_frame(&mut self, output: &mut [f32]) -> bool {
        if self.samples.len() < output.len() {
            output.fill(0.0);
            self.stats.last_level = 0.0;
            self.stats.missing_frames += 1;
            self.stats.buffered_samples = self.samples.len();
            return false;
        }

        let mut peak = 0.0f32;
        for sample in output.iter_mut() {
            let value = self
                .samples
                .pop_front()
                .expect("length checked before popping reference samples");
            peak = peak.max(value.abs());
            *sample = value;
        }
        self.stats.last_level = peak.min(1.0);
        self.stats.buffered_samples = self.samples.len();
        true
    }

    pub fn clear(&mut self) {
        self.samples.clear();
        self.stats.buffered_samples = 0;
        self.stats.last_level = 0.0;
    }
}

#[cfg(windows)]
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

#[cfg(windows)]
use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    FromSample, Sample, SampleFormat, SizedSample,
};

#[cfg(windows)]
#[derive(Clone)]
pub struct SharedReferenceFrameBuffer {
    inner: Arc<Mutex<ReferenceFrameBuffer>>,
}

#[cfg(windows)]
impl SharedReferenceFrameBuffer {
    fn new(buffer: ReferenceFrameBuffer) -> Self {
        Self {
            inner: Arc::new(Mutex::new(buffer)),
        }
    }

    pub fn format(&self) -> AudioFrameFormat {
        self.inner
            .lock()
            .expect("loopback reference buffer mutex poisoned")
            .format()
    }

    pub fn stats(&self) -> ReferenceCaptureStats {
        self.inner
            .lock()
            .expect("loopback reference buffer mutex poisoned")
            .stats()
    }

    pub fn pop_mono_frame(&mut self, output: &mut [f32]) -> bool {
        self.inner
            .lock()
            .expect("loopback reference buffer mutex poisoned")
            .pop_mono_frame(output)
    }

    fn push_interleaved(&self, samples: &[f32]) {
        if let Ok(mut buffer) = self.inner.lock() {
            buffer.push_interleaved(samples);
        }
    }
}

#[cfg(windows)]
impl std::fmt::Debug for SharedReferenceFrameBuffer {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SharedReferenceFrameBuffer")
            .field("format", &self.format())
            .field("stats", &self.stats())
            .finish()
    }
}

#[cfg(windows)]
pub struct LoopbackReferenceCapture {
    _stream: cpal::Stream,
    buffer: SharedReferenceFrameBuffer,
}

#[cfg(windows)]
impl LoopbackReferenceCapture {
    pub fn start_default(capacity_ms: u32) -> crate::ClearLineResult<Self> {
        let host = cpal::default_host();
        let device = host.default_output_device().ok_or_else(|| {
            crate::ClearLineError::DeviceNotFound("default output device".to_owned())
        })?;
        Self::start_for_output_device(device, capacity_ms)
    }

    pub fn start_for_output_device(
        device: cpal::Device,
        capacity_ms: u32,
    ) -> crate::ClearLineResult<Self> {
        let supported_config = device
            .default_output_config()
            .map_err(|error| crate::ClearLineError::StreamBuild(error.to_string()))?;
        let sample_format = supported_config.sample_format();
        let stream_config = supported_config.config();
        let format = AudioFrameFormat::new(stream_config.sample_rate, stream_config.channels);
        let capacity_samples =
            ((u64::from(format.sample_rate_hz()) * u64::from(capacity_ms.max(10))) / 1_000).max(1)
                as usize;
        let buffer =
            SharedReferenceFrameBuffer::new(ReferenceFrameBuffer::new(format, capacity_samples));

        let stream = match sample_format {
            SampleFormat::I8 => {
                build_loopback_input_stream::<i8>(&device, stream_config, buffer.clone())
            }
            SampleFormat::I16 => {
                build_loopback_input_stream::<i16>(&device, stream_config, buffer.clone())
            }
            SampleFormat::I32 => {
                build_loopback_input_stream::<i32>(&device, stream_config, buffer.clone())
            }
            SampleFormat::I64 => {
                build_loopback_input_stream::<i64>(&device, stream_config, buffer.clone())
            }
            SampleFormat::U8 => {
                build_loopback_input_stream::<u8>(&device, stream_config, buffer.clone())
            }
            SampleFormat::U16 => {
                build_loopback_input_stream::<u16>(&device, stream_config, buffer.clone())
            }
            SampleFormat::U32 => {
                build_loopback_input_stream::<u32>(&device, stream_config, buffer.clone())
            }
            SampleFormat::U64 => {
                build_loopback_input_stream::<u64>(&device, stream_config, buffer.clone())
            }
            SampleFormat::F32 => {
                build_loopback_input_stream::<f32>(&device, stream_config, buffer.clone())
            }
            SampleFormat::F64 => {
                build_loopback_input_stream::<f64>(&device, stream_config, buffer.clone())
            }
            unsupported => Err(crate::ClearLineError::UnsupportedSampleFormat(format!(
                "loopback output {unsupported}"
            ))),
        }?;

        stream
            .play()
            .map_err(|error| crate::ClearLineError::StreamPlay(error.to_string()))?;

        Ok(Self {
            _stream: stream,
            buffer,
        })
    }

    pub fn format(&self) -> AudioFrameFormat {
        self.buffer.format()
    }

    pub fn stats(&self) -> ReferenceCaptureStats {
        self.buffer.stats()
    }

    pub fn pop_mono_frame(&self, output: &mut [f32]) -> bool {
        let mut buffer = self.buffer.clone();
        buffer.pop_mono_frame(output)
    }

    pub fn shared_buffer(&self) -> SharedReferenceFrameBuffer {
        self.buffer.clone()
    }

    pub fn wait_for_level(&self, timeout: Duration, threshold: f32) -> bool {
        let start = Instant::now();
        while start.elapsed() < timeout {
            if self.stats().last_level() >= threshold {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        false
    }
}

#[cfg(windows)]
fn build_loopback_input_stream<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    buffer: SharedReferenceFrameBuffer,
) -> crate::ClearLineResult<cpal::Stream>
where
    T: SizedSample + Sample + Send + 'static,
    f32: FromSample<T>,
{
    device
        .build_input_stream(
            config,
            move |data: &[T], _| {
                let mut samples = Vec::with_capacity(data.len());
                samples.extend(data.iter().map(|sample| sample.to_sample::<f32>()));
                buffer.push_interleaved(&samples);
            },
            move |error| {
                eprintln!("ClearLine loopback reference stream error: {error}");
            },
            None,
        )
        .map_err(|error| crate::ClearLineError::StreamBuild(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AudioFrameFormat;

    #[test]
    fn reference_buffer_downmixes_interleaved_stereo_to_mono_frames() {
        let format = AudioFrameFormat::new(48_000, 2);
        let mut buffer = ReferenceFrameBuffer::new(format, 2);

        buffer.push_interleaved(&[0.2, 0.6, -0.4, 0.0]);

        let mut frame = vec![0.0; 2];
        assert!(buffer.pop_mono_frame(&mut frame));
        assert_eq!(frame, vec![0.4, -0.2]);
    }

    #[test]
    fn reference_buffer_reports_level_and_missing_frames() {
        let format = AudioFrameFormat::new(48_000, 1);
        let mut buffer = ReferenceFrameBuffer::new(format, 4);
        let mut frame = vec![1.0; 2];

        assert!(!buffer.pop_mono_frame(&mut frame));
        assert_eq!(frame, vec![0.0, 0.0]);
        assert_eq!(buffer.stats().missing_frames(), 1);

        buffer.push_interleaved(&[0.25, -0.5]);
        assert!(buffer.pop_mono_frame(&mut frame));
        assert_eq!(buffer.stats().last_level(), 0.5);
    }

    #[test]
    fn reference_buffer_reports_level_from_incoming_interleaved_samples() {
        let format = AudioFrameFormat::new(48_000, 2);
        let mut buffer = ReferenceFrameBuffer::new(format, 8);

        buffer.push_interleaved(&[0.25, -0.75, 0.2, 0.6]);

        assert_eq!(buffer.stats().last_level(), 0.4);
        assert_eq!(buffer.stats().buffered_samples(), 2);
    }

    #[test]
    fn reference_buffer_drops_oldest_samples_when_capacity_is_exceeded() {
        let format = AudioFrameFormat::new(48_000, 1);
        let mut buffer = ReferenceFrameBuffer::new(format, 3);
        let mut frame = vec![0.0; 3];

        buffer.push_interleaved(&[0.1, 0.2]);
        buffer.push_interleaved(&[0.3, 0.4, 0.5]);

        assert!(buffer.pop_mono_frame(&mut frame));
        assert_eq!(frame, vec![0.3, 0.4, 0.5]);
        assert_eq!(buffer.stats().dropped_samples(), 2);
    }
}
