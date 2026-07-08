use std::collections::VecDeque;

#[cfg(any(windows, test))]
use crate::resample::StreamingSampleRateConverter;
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

#[cfg(any(windows, test))]
struct LoopbackReferenceInputProcessor {
    target_format: AudioFrameFormat,
    converter: StreamingSampleRateConverter,
    converted: Vec<f32>,
}

#[cfg(any(windows, test))]
impl LoopbackReferenceInputProcessor {
    fn new(
        source_format: AudioFrameFormat,
        target_format: AudioFrameFormat,
    ) -> crate::ClearLineResult<Self> {
        if source_format.channels() != target_format.channels() {
            return Err(crate::ClearLineError::StreamBuild(format!(
                "loopback reference resampler expects matching channel counts, source {} channel(s), target {} channel(s)",
                source_format.channels(),
                target_format.channels()
            )));
        }

        Ok(Self {
            target_format,
            converter: StreamingSampleRateConverter::new(
                source_format.sample_rate_hz(),
                target_format.sample_rate_hz(),
                source_format.channels(),
            )?,
            converted: Vec::new(),
        })
    }

    fn process_interleaved(
        &mut self,
        input: &[f32],
        buffer: &mut ReferenceFrameBuffer,
    ) -> crate::ClearLineResult<()> {
        if buffer.format() != self.target_format {
            return Err(crate::ClearLineError::StreamBuild(format!(
                "loopback reference target format mismatch: buffer {:?}, processor {:?}",
                buffer.format(),
                self.target_format
            )));
        }

        self.converted.clear();
        self.converter
            .process_interleaved(input, &mut self.converted)?;
        if !self.converted.is_empty() {
            buffer.push_interleaved(&self.converted);
        }
        Ok(())
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

    pub fn start_default_with_target_format(
        capacity_ms: u32,
        target_format: AudioFrameFormat,
    ) -> crate::ClearLineResult<Self> {
        let host = cpal::default_host();
        let device = host.default_output_device().ok_or_else(|| {
            crate::ClearLineError::DeviceNotFound("default output device".to_owned())
        })?;
        Self::start_for_output_device_with_target_format(device, capacity_ms, target_format)
    }

    pub fn start_for_output_device(
        device: cpal::Device,
        capacity_ms: u32,
    ) -> crate::ClearLineResult<Self> {
        let supported_config = device
            .default_output_config()
            .map_err(|error| crate::ClearLineError::StreamBuild(error.to_string()))?;
        let stream_config = supported_config.config();
        let source_format =
            AudioFrameFormat::new(stream_config.sample_rate, stream_config.channels);
        Self::start_for_output_device_with_supported_config(
            device,
            capacity_ms,
            supported_config.sample_format(),
            stream_config,
            source_format,
        )
    }

    pub fn start_for_output_device_with_target_format(
        device: cpal::Device,
        capacity_ms: u32,
        target_format: AudioFrameFormat,
    ) -> crate::ClearLineResult<Self> {
        let supported_config = device
            .default_output_config()
            .map_err(|error| crate::ClearLineError::StreamBuild(error.to_string()))?;
        let stream_config = supported_config.config();
        let source_format =
            AudioFrameFormat::new(stream_config.sample_rate, stream_config.channels);
        let buffer_format =
            AudioFrameFormat::new(target_format.sample_rate_hz(), source_format.channels());
        Self::start_for_output_device_with_supported_config(
            device,
            capacity_ms,
            supported_config.sample_format(),
            stream_config,
            buffer_format,
        )
    }

    fn start_for_output_device_with_supported_config(
        device: cpal::Device,
        capacity_ms: u32,
        sample_format: SampleFormat,
        stream_config: cpal::StreamConfig,
        format: AudioFrameFormat,
    ) -> crate::ClearLineResult<Self> {
        let capacity_samples =
            ((u64::from(format.sample_rate_hz()) * u64::from(capacity_ms.max(10))) / 1_000).max(1)
                as usize;
        let buffer =
            SharedReferenceFrameBuffer::new(ReferenceFrameBuffer::new(format, capacity_samples));

        let source_format =
            AudioFrameFormat::new(stream_config.sample_rate, stream_config.channels);

        let stream = match sample_format {
            SampleFormat::I8 => build_loopback_input_stream::<i8>(
                &device,
                stream_config,
                source_format,
                format,
                buffer.clone(),
            ),
            SampleFormat::I16 => build_loopback_input_stream::<i16>(
                &device,
                stream_config,
                source_format,
                format,
                buffer.clone(),
            ),
            SampleFormat::I32 => build_loopback_input_stream::<i32>(
                &device,
                stream_config,
                source_format,
                format,
                buffer.clone(),
            ),
            SampleFormat::I64 => build_loopback_input_stream::<i64>(
                &device,
                stream_config,
                source_format,
                format,
                buffer.clone(),
            ),
            SampleFormat::U8 => build_loopback_input_stream::<u8>(
                &device,
                stream_config,
                source_format,
                format,
                buffer.clone(),
            ),
            SampleFormat::U16 => build_loopback_input_stream::<u16>(
                &device,
                stream_config,
                source_format,
                format,
                buffer.clone(),
            ),
            SampleFormat::U32 => build_loopback_input_stream::<u32>(
                &device,
                stream_config,
                source_format,
                format,
                buffer.clone(),
            ),
            SampleFormat::U64 => build_loopback_input_stream::<u64>(
                &device,
                stream_config,
                source_format,
                format,
                buffer.clone(),
            ),
            SampleFormat::F32 => build_loopback_input_stream::<f32>(
                &device,
                stream_config,
                source_format,
                format,
                buffer.clone(),
            ),
            SampleFormat::F64 => build_loopback_input_stream::<f64>(
                &device,
                stream_config,
                source_format,
                format,
                buffer.clone(),
            ),
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
    source_format: AudioFrameFormat,
    target_format: AudioFrameFormat,
    buffer: SharedReferenceFrameBuffer,
) -> crate::ClearLineResult<cpal::Stream>
where
    T: SizedSample + Sample + Send + 'static,
    f32: FromSample<T>,
{
    let mut processor = LoopbackReferenceInputProcessor::new(source_format, target_format)?;
    let mut samples = Vec::new();

    device
        .build_input_stream(
            config,
            move |data: &[T], _| {
                samples.clear();
                samples.extend(data.iter().map(|sample| sample.to_sample::<f32>()));
                let mut inner = buffer
                    .inner
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                if let Err(error) = processor.process_interleaved(&samples, &mut inner) {
                    eprintln!("ClearLine loopback reference resampling error: {error}");
                }
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
    #[cfg(any(windows, test))]
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

    #[test]
    fn reference_input_processor_resamples_source_to_target_rate_before_buffering() {
        let source_format = AudioFrameFormat::new(44_100, 2);
        let target_format = AudioFrameFormat::new(48_000, 2);
        let mut processor =
            LoopbackReferenceInputProcessor::new(source_format, target_format).unwrap();
        let mut buffer = ReferenceFrameBuffer::new(target_format, 8_000);
        let input = stereo_sine_wave(44_100, 440.0, 4_410);

        processor.process_interleaved(&input, &mut buffer).unwrap();

        let buffered = buffer.stats().buffered_samples();
        assert!(
            (buffered as isize - 4_800).abs() <= 128,
            "expected about 4800 buffered target-rate frames, got {buffered}"
        );
        assert_eq!(buffer.format(), target_format);
    }

    fn stereo_sine_wave(sample_rate_hz: u32, frequency_hz: f32, frames: usize) -> Vec<f32> {
        let mut samples = Vec::with_capacity(frames * 2);
        for frame in 0..frames {
            let left =
                (std::f32::consts::TAU * frequency_hz * frame as f32 / sample_rate_hz as f32).sin()
                    * 0.5;
            let right = -left;
            samples.push(left);
            samples.push(right);
        }
        samples
    }
}
