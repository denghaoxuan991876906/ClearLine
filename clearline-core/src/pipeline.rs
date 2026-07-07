use std::sync::{
    atomic::{AtomicU32, Ordering},
    Arc,
};
#[cfg(any(windows, test))]
use std::{collections::VecDeque, sync::Mutex};

use crate::device::DeviceId;
#[cfg(all(windows, feature = "aec"))]
use crate::echo::Aec3EchoWorker;
#[cfg(any(windows, test))]
use crate::echo::EchoCanceller;
#[cfg(windows)]
use crate::echo::NoopEchoCanceller;
use crate::echo::{EchoCancellerBackend, EchoCancellerRuntimeInfo};
#[cfg(windows)]
use crate::preprocess::WindNoiseConfig;
#[cfg(any(windows, test))]
use crate::preprocess::WindNoiseReducer;
use crate::reference::ReferenceCaptureStats;
#[cfg(any(windows, test))]
use crate::reference::ReferenceFrameBuffer;
#[cfg(windows)]
use crate::reference::{LoopbackReferenceCapture, SharedReferenceFrameBuffer};
#[cfg(windows)]
use crate::suppressor::try_create_suppressor_with_deepfilternet_bundle;
#[cfg(any(windows, test))]
use crate::suppressor::NoiseSuppressor;
use crate::suppressor::{
    AudioFrameFormat, DeepFilterNetModelBundle, SuppressionStrength, SuppressorMode,
    SuppressorRuntimeInfo,
};
use crate::{ClearLineError, ClearLineResult};

#[cfg(windows)]
use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    FromSample, Sample, SampleFormat, SizedSample,
};

#[cfg(any(windows, test))]
#[derive(Debug, Clone)]
struct AudioSampleBuffer {
    inner: Arc<Mutex<AudioSampleBufferState>>,
    capacity: usize,
    prebuffer_samples: usize,
}

#[cfg(any(windows, test))]
#[derive(Debug)]
struct AudioSampleBufferState {
    samples: VecDeque<f32>,
    underrun_sample_count: u64,
    dropped_sample_count: u64,
    is_primed: bool,
}

#[cfg(any(windows, test))]
impl AudioSampleBuffer {
    fn new(capacity: usize) -> Self {
        Self::with_prebuffer(capacity, 0)
    }

    fn with_prebuffer(capacity: usize, prebuffer_samples: usize) -> Self {
        let capacity = capacity.max(1);
        let prebuffer_samples = prebuffer_samples.min(capacity);
        Self {
            inner: Arc::new(Mutex::new(AudioSampleBufferState {
                samples: VecDeque::with_capacity(capacity),
                underrun_sample_count: 0,
                dropped_sample_count: 0,
                is_primed: prebuffer_samples == 0,
            })),
            capacity,
            prebuffer_samples,
        }
    }

    fn push_samples(&self, samples: &[f32]) {
        let mut state = self.lock_state();
        for sample in samples {
            if state.samples.len() == self.capacity {
                state.samples.pop_front();
                state.dropped_sample_count += 1;
            }
            state.samples.push_back(sample.clamp(-1.0, 1.0));
        }

        if state.samples.len() >= self.prebuffer_samples {
            state.is_primed = true;
        }
    }

    fn pop_samples_or_zero(&self, output: &mut [f32]) {
        let mut state = self.lock_state();
        if !state.is_primed {
            output.fill(0.0);
            return;
        }

        for sample in output {
            match state.samples.pop_front() {
                Some(value) => *sample = value,
                None => {
                    state.underrun_sample_count += 1;
                    state.is_primed = self.prebuffer_samples == 0;
                    *sample = 0.0;
                }
            }
        }
    }

    fn metrics(&self) -> PipelineMetrics {
        let state = self.lock_state();
        PipelineMetrics::new(
            state.samples.len(),
            self.capacity,
            state.underrun_sample_count,
            state.dropped_sample_count,
        )
    }

    fn lock_state(&self) -> std::sync::MutexGuard<'_, AudioSampleBufferState> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[cfg(any(windows, test))]
impl Default for AudioSampleBuffer {
    fn default() -> Self {
        Self::new(48_000)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PipelineMetrics {
    buffered_samples: usize,
    capacity_samples: usize,
    underrun_sample_count: u64,
    dropped_sample_count: u64,
}

impl PipelineMetrics {
    pub fn new(
        buffered_samples: usize,
        capacity_samples: usize,
        underrun_sample_count: u64,
        dropped_sample_count: u64,
    ) -> Self {
        Self {
            buffered_samples,
            capacity_samples,
            underrun_sample_count,
            dropped_sample_count,
        }
    }

    pub fn buffered_samples(self) -> usize {
        self.buffered_samples
    }

    pub fn capacity_samples(self) -> usize {
        self.capacity_samples
    }

    pub fn underrun_sample_count(self) -> u64 {
        self.underrun_sample_count
    }

    pub fn dropped_sample_count(self) -> u64 {
        self.dropped_sample_count
    }

    pub fn fill_ratio(self) -> f32 {
        if self.capacity_samples == 0 {
            0.0
        } else {
            (self.buffered_samples as f32 / self.capacity_samples as f32).clamp(0.0, 1.0)
        }
    }

    pub fn buffered_latency_ms(self, format: AudioFrameFormat) -> Option<u32> {
        samples_to_latency_ms(self.buffered_samples, format)
    }

    pub fn capacity_latency_ms(self, format: AudioFrameFormat) -> Option<u32> {
        samples_to_latency_ms(self.capacity_samples, format)
    }
}

fn samples_to_latency_ms(samples: usize, format: AudioFrameFormat) -> Option<u32> {
    if samples == 0 {
        return None;
    }

    let samples_per_second =
        u64::from(format.sample_rate_hz().max(1)) * u64::from(format.channels().max(1));
    if samples_per_second == 0 {
        return None;
    }

    let latency_ms = samples as f64 / samples_per_second as f64 * 1_000.0;
    Some(latency_ms.round().max(1.0) as u32)
}

impl Default for PipelineMetrics {
    fn default() -> Self {
        Self::new(0, 0, 0, 0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EchoReferenceDiagnostics {
    level: f32,
    buffered_samples: usize,
    missing_frames: u64,
    dropped_samples: u64,
}

impl EchoReferenceDiagnostics {
    pub fn new(
        level: f32,
        buffered_samples: usize,
        missing_frames: u64,
        dropped_samples: u64,
    ) -> Self {
        Self {
            level: level.clamp(0.0, 1.0),
            buffered_samples,
            missing_frames,
            dropped_samples,
        }
    }

    pub fn from_reference_stats(stats: ReferenceCaptureStats) -> Self {
        Self::new(
            stats.last_level(),
            stats.buffered_samples(),
            stats.missing_frames(),
            stats.dropped_samples(),
        )
    }

    pub fn level(self) -> f32 {
        self.level
    }

    pub fn buffered_samples(self) -> usize {
        self.buffered_samples
    }

    pub fn missing_frames(self) -> u64 {
        self.missing_frames
    }

    pub fn dropped_samples(self) -> u64 {
        self.dropped_samples
    }

    pub fn has_reference_audio(self, threshold: f32) -> bool {
        self.level >= threshold.max(0.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AudioOutputTarget {
    AudioDevice(DeviceId),
    ClearLineVirtualMicrophone,
}

impl AudioOutputTarget {
    pub fn audio_device_id(&self) -> Option<&DeviceId> {
        match self {
            Self::AudioDevice(device_id) => Some(device_id),
            Self::ClearLineVirtualMicrophone => None,
        }
    }

    pub fn is_clearline_virtual_microphone(&self) -> bool {
        matches!(self, Self::ClearLineVirtualMicrophone)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PipelineRuntimeInfo {
    input_format: AudioFrameFormat,
    output_format: AudioFrameFormat,
    suppressor: SuppressorRuntimeInfo,
    echo_cancellation: EchoCancellerRuntimeInfo,
    wind_noise_reduction_enabled: bool,
    output_target: AudioOutputTarget,
}

impl PipelineRuntimeInfo {
    pub fn new(
        input_format: AudioFrameFormat,
        output_format: AudioFrameFormat,
        suppressor: SuppressorRuntimeInfo,
        output_target: AudioOutputTarget,
    ) -> Self {
        Self {
            input_format,
            output_format,
            suppressor,
            echo_cancellation: EchoCancellerRuntimeInfo::new(
                EchoCancellerBackend::Disabled,
                input_format,
            ),
            wind_noise_reduction_enabled: false,
            output_target,
        }
    }

    pub fn input_format(&self) -> AudioFrameFormat {
        self.input_format
    }

    pub fn output_format(&self) -> AudioFrameFormat {
        self.output_format
    }

    pub fn suppressor(&self) -> SuppressorRuntimeInfo {
        self.suppressor
    }

    pub fn with_echo_cancellation(mut self, echo_cancellation: EchoCancellerRuntimeInfo) -> Self {
        self.echo_cancellation = echo_cancellation;
        self
    }

    pub fn echo_cancellation(&self) -> EchoCancellerRuntimeInfo {
        self.echo_cancellation
    }

    pub fn with_wind_noise_reduction(mut self, enabled: bool) -> Self {
        self.wind_noise_reduction_enabled = enabled;
        self
    }

    pub fn wind_noise_reduction_enabled(&self) -> bool {
        self.wind_noise_reduction_enabled
    }

    pub fn output_target(&self) -> &AudioOutputTarget {
        &self.output_target
    }
}

#[cfg(any(windows, test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AudioStreamFormat {
    sample_rate_hz: u32,
    channels: u16,
}

#[cfg(any(windows, test))]
impl AudioStreamFormat {
    fn new(sample_rate_hz: u32, channels: u16) -> Self {
        Self {
            sample_rate_hz: sample_rate_hz.max(1),
            channels: channels.max(1),
        }
    }

    #[cfg(windows)]
    fn from_cpal_config(config: &cpal::StreamConfig) -> Self {
        Self::new(config.sample_rate, config.channels)
    }
}

#[cfg(any(windows, test))]
fn passthrough_output_format(
    input: AudioStreamFormat,
    output_default: AudioStreamFormat,
) -> AudioStreamFormat {
    AudioStreamFormat::new(input.sample_rate_hz, output_default.channels)
}

#[cfg(any(windows, test))]
fn append_converted_channels(
    input: &[f32],
    input_channels: u16,
    output_channels: u16,
    output: &mut Vec<f32>,
) {
    let input_channels = usize::from(input_channels.max(1));
    let output_channels = usize::from(output_channels.max(1));
    output.reserve(input.len() / input_channels * output_channels);

    for frame in input.chunks_exact(input_channels) {
        if output_channels == input_channels {
            output.extend_from_slice(frame);
            continue;
        }

        if output_channels == 1 {
            let mixed = frame.iter().copied().sum::<f32>() / input_channels as f32;
            output.push(mixed);
            continue;
        }

        if input_channels == 1 {
            output.extend(std::iter::repeat_n(frame[0], output_channels));
            continue;
        }

        for channel_index in 0..output_channels {
            let source_index = channel_index.min(input_channels - 1);
            output.push(frame[source_index]);
        }
    }
}

#[cfg(any(windows, test))]
fn clearline_virtual_microphone_output_format(
    sample_rate_hz: u32,
    channels: u32,
) -> ClearLineResult<AudioStreamFormat> {
    if sample_rate_hz != 48_000 || channels != 1 {
        return Err(ClearLineError::StreamBuild(format!(
            "ClearLine Virtual Microphone expects 48000 Hz / 1 channel, driver reported {sample_rate_hz} Hz / {channels} channel(s)"
        )));
    }

    Ok(AudioStreamFormat::new(sample_rate_hz, channels as u16))
}

#[cfg(any(windows, test))]
fn append_virtual_microphone_pcm_i16(input: &[f32], input_channels: u16, output: &mut Vec<i16>) {
    let input_channels = usize::from(input_channels.max(1));
    output.reserve(input.len() / input_channels);

    for frame in input.chunks_exact(input_channels) {
        let mono = if input_channels == 1 {
            frame[0]
        } else {
            frame.iter().copied().sum::<f32>() / input_channels as f32
        };
        output.push(f32_to_i16_pcm(mono));
    }
}

#[cfg(any(windows, test))]
fn f32_to_i16_pcm(sample: f32) -> i16 {
    let sample = sample.clamp(-1.0, 1.0);
    if sample <= -1.0 {
        i16::MIN
    } else {
        (sample * i16::MAX as f32).round() as i16
    }
}

#[cfg(any(windows, test))]
#[derive(Debug, Default)]
struct InputCallbackScratch {
    input: Vec<f32>,
    echo_cancelled: Vec<f32>,
    render_reference: Vec<f32>,
    reference_mono: Vec<f32>,
    processed: Vec<f32>,
    converted: Vec<f32>,
    #[cfg(windows)]
    virtual_microphone_pcm: Vec<i16>,
}

#[cfg(any(windows, test))]
impl InputCallbackScratch {
    #[cfg(test)]
    fn copy_from_f32_samples(&mut self, samples: &[f32]) {
        self.input.clear();
        self.input.extend_from_slice(samples);
    }

    #[cfg(windows)]
    fn copy_from_samples<T>(&mut self, samples: &[T])
    where
        T: Sample,
        f32: FromSample<T>,
    {
        self.input.clear();
        self.input
            .extend(samples.iter().map(|sample| sample.to_sample::<f32>()));
    }

    fn prepare_processed_buffer(&mut self) {
        self.echo_cancelled.resize(self.input.len(), 0.0);
        self.processed.resize(self.input.len(), 0.0);
    }

    #[cfg(test)]
    fn capacities(&self) -> (usize, usize, usize) {
        (
            self.input.capacity(),
            self.processed.capacity(),
            self.converted.capacity(),
        )
    }
}

#[cfg(any(windows, test))]
trait ReferenceFrameSource {
    fn pop_mono_frame(&mut self, output: &mut [f32]) -> bool;
}

#[cfg(any(windows, test))]
impl ReferenceFrameSource for ReferenceFrameBuffer {
    fn pop_mono_frame(&mut self, output: &mut [f32]) -> bool {
        ReferenceFrameBuffer::pop_mono_frame(self, output)
    }
}

#[cfg(windows)]
impl ReferenceFrameSource for SharedReferenceFrameBuffer {
    fn pop_mono_frame(&mut self, output: &mut [f32]) -> bool {
        SharedReferenceFrameBuffer::pop_mono_frame(self, output)
    }
}

#[cfg(any(windows, test))]
fn process_input_callback_frame(
    scratch: &mut InputCallbackScratch,
    echo_canceller: &mut dyn EchoCanceller,
    reference_buffer: Option<&mut dyn ReferenceFrameSource>,
    wind_noise_reducer: &mut WindNoiseReducer,
    suppressor: &mut dyn NoiseSuppressor,
    input_channels: u16,
) -> ClearLineResult<()> {
    scratch.prepare_processed_buffer();
    apply_echo_cancellation_stage(
        echo_canceller,
        reference_buffer,
        input_channels,
        &scratch.input,
        &mut scratch.echo_cancelled,
        &mut scratch.render_reference,
        &mut scratch.reference_mono,
    )?;
    wind_noise_reducer.process_interleaved(&mut scratch.echo_cancelled);
    suppressor.process(&scratch.echo_cancelled, &mut scratch.processed)
}

#[cfg(any(windows, test))]
fn apply_echo_cancellation_stage(
    echo_canceller: &mut dyn EchoCanceller,
    reference_buffer: Option<&mut dyn ReferenceFrameSource>,
    input_channels: u16,
    capture: &[f32],
    output: &mut Vec<f32>,
    render_reference: &mut Vec<f32>,
    reference_mono: &mut Vec<f32>,
) -> ClearLineResult<()> {
    output.resize(capture.len(), 0.0);
    build_render_reference_frame(
        reference_buffer,
        input_channels,
        capture.len(),
        render_reference,
        reference_mono,
    );
    echo_canceller.process(capture, render_reference, output)
}

#[cfg(any(windows, test))]
fn build_render_reference_frame(
    reference_buffer: Option<&mut dyn ReferenceFrameSource>,
    input_channels: u16,
    capture_len: usize,
    render_reference: &mut Vec<f32>,
    reference_mono: &mut Vec<f32>,
) {
    let input_channels = usize::from(input_channels.max(1));
    render_reference.clear();
    render_reference.resize(capture_len, 0.0);

    let frame_count = capture_len / input_channels;
    if frame_count == 0 {
        return;
    }

    let Some(reference_buffer) = reference_buffer else {
        return;
    };

    reference_mono.clear();
    reference_mono.resize(frame_count, 0.0);
    reference_buffer.pop_mono_frame(reference_mono);

    if input_channels == 1 {
        render_reference[..frame_count].copy_from_slice(reference_mono);
        return;
    }

    for (frame_index, sample) in reference_mono.iter().copied().enumerate() {
        let start = frame_index * input_channels;
        let end = start + input_channels;
        render_reference[start..end].fill(sample);
    }
}

#[cfg(any(windows, test))]
#[derive(Debug, Default)]
struct OutputCallbackScratch {
    samples: Vec<f32>,
}

#[cfg(any(windows, test))]
impl OutputCallbackScratch {
    fn resize(&mut self, len: usize) {
        self.samples.resize(len, 0.0);
    }

    fn samples_mut(&mut self) -> &mut [f32] {
        &mut self.samples
    }

    fn samples(&self) -> &[f32] {
        &self.samples
    }

    #[cfg(test)]
    fn capacity(&self) -> usize {
        self.samples.capacity()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PipelineState {
    Stopped,
    Starting,
    Running,
    Error(String),
}

impl PipelineState {
    pub fn is_running(&self) -> bool {
        matches!(self, PipelineState::Running)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioPipelineConfig {
    input_device_id: DeviceId,
    output_target: AudioOutputTarget,
    suppressor_mode: SuppressorMode,
    suppression_strength: SuppressionStrength,
    wind_noise_reduction_enabled: bool,
    echo_cancellation_enabled: bool,
    deepfilternet_model_bundle: Option<DeepFilterNetModelBundle>,
}

impl AudioPipelineConfig {
    pub fn new(
        input_device_id: DeviceId,
        output_device_id: DeviceId,
        suppressor_mode: SuppressorMode,
    ) -> Self {
        Self {
            input_device_id,
            output_target: AudioOutputTarget::AudioDevice(output_device_id),
            suppressor_mode,
            suppression_strength: SuppressionStrength::default(),
            wind_noise_reduction_enabled: false,
            echo_cancellation_enabled: false,
            deepfilternet_model_bundle: None,
        }
    }

    pub fn for_virtual_microphone(
        input_device_id: DeviceId,
        suppressor_mode: SuppressorMode,
    ) -> Self {
        Self {
            input_device_id,
            output_target: AudioOutputTarget::ClearLineVirtualMicrophone,
            suppressor_mode,
            suppression_strength: SuppressionStrength::default(),
            wind_noise_reduction_enabled: false,
            echo_cancellation_enabled: false,
            deepfilternet_model_bundle: None,
        }
    }

    pub fn input_device_id(&self) -> &DeviceId {
        &self.input_device_id
    }

    pub fn output_device_id(&self) -> Option<&DeviceId> {
        self.output_target.audio_device_id()
    }

    pub fn output_target(&self) -> &AudioOutputTarget {
        &self.output_target
    }

    pub fn suppressor_mode(&self) -> SuppressorMode {
        self.suppressor_mode
    }

    pub fn suppression_strength(&self) -> SuppressionStrength {
        self.suppression_strength
    }

    pub fn with_suppression_strength(mut self, strength: SuppressionStrength) -> Self {
        self.suppression_strength = strength;
        self
    }

    pub fn with_wind_noise_reduction(mut self, enabled: bool) -> Self {
        self.wind_noise_reduction_enabled = enabled;
        self
    }

    pub fn wind_noise_reduction_enabled(&self) -> bool {
        self.wind_noise_reduction_enabled
    }

    pub fn with_echo_cancellation(mut self, enabled: bool) -> Self {
        self.echo_cancellation_enabled = enabled;
        self
    }

    pub fn echo_cancellation_enabled(&self) -> bool {
        self.echo_cancellation_enabled
    }

    pub fn with_deepfilternet_model_bundle(mut self, bundle: DeepFilterNetModelBundle) -> Self {
        self.deepfilternet_model_bundle = Some(bundle);
        self
    }

    pub fn deepfilternet_model_bundle(&self) -> Option<&DeepFilterNetModelBundle> {
        self.deepfilternet_model_bundle.as_ref()
    }
}

#[derive(Debug, Clone)]
pub struct LevelMeter {
    level_bits: Arc<AtomicU32>,
}

impl LevelMeter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn level(&self) -> f32 {
        f32::from_bits(self.level_bits.load(Ordering::Relaxed))
    }

    pub fn reset(&self) {
        self.set_level(0.0);
    }

    pub fn update_from_f32_samples(&self, samples: &[f32]) {
        self.set_level(peak_level(samples.iter().copied()));
    }

    fn set_level(&self, level: f32) {
        self.level_bits
            .store(level.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    }
}

impl Default for LevelMeter {
    fn default() -> Self {
        Self {
            level_bits: Arc::new(AtomicU32::new(0.0f32.to_bits())),
        }
    }
}

fn peak_level(samples: impl IntoIterator<Item = f32>) -> f32 {
    samples
        .into_iter()
        .map(|sample| sample.abs())
        .fold(0.0_f32, f32::max)
        .clamp(0.0, 1.0)
}

pub struct AudioPipeline {
    state: PipelineState,
    config: Option<AudioPipelineConfig>,
    runtime_info: Option<PipelineRuntimeInfo>,
    level_meter: LevelMeter,
    #[cfg(windows)]
    sample_buffer: Option<AudioSampleBuffer>,
    #[cfg(windows)]
    input_stream: Option<cpal::Stream>,
    #[cfg(windows)]
    output_stream: Option<cpal::Stream>,
    #[cfg(windows)]
    reference_capture: Option<LoopbackReferenceCapture>,
}

impl AudioPipeline {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn state(&self) -> &PipelineState {
        &self.state
    }

    pub fn config(&self) -> Option<&AudioPipelineConfig> {
        self.config.as_ref()
    }

    pub fn runtime_info(&self) -> Option<&PipelineRuntimeInfo> {
        self.runtime_info.as_ref()
    }

    pub fn input_level(&self) -> f32 {
        self.level_meter.level()
    }

    pub fn metrics(&self) -> PipelineMetrics {
        #[cfg(windows)]
        {
            self.sample_buffer
                .as_ref()
                .map(AudioSampleBuffer::metrics)
                .unwrap_or_default()
        }

        #[cfg(not(windows))]
        {
            PipelineMetrics::default()
        }
    }

    pub fn echo_reference_diagnostics(&self) -> Option<EchoReferenceDiagnostics> {
        #[cfg(windows)]
        {
            self.reference_capture
                .as_ref()
                .map(|capture| EchoReferenceDiagnostics::from_reference_stats(capture.stats()))
        }

        #[cfg(not(windows))]
        {
            None
        }
    }

    pub fn start(&mut self, config: AudioPipelineConfig) -> ClearLineResult<()> {
        if matches!(self.state, PipelineState::Starting | PipelineState::Running) {
            return Err(ClearLineError::PipelineAlreadyRunning);
        }

        self.state = PipelineState::Starting;
        self.level_meter.reset();

        if let Err(error) = self.start_platform_streams(&config) {
            self.fail(error.to_string());
            return Err(error);
        }

        self.config = Some(config);
        self.state = PipelineState::Running;
        Ok(())
    }

    pub fn stop(&mut self) {
        self.stop_platform_streams();
        self.config = None;
        self.runtime_info = None;
        self.level_meter.reset();
        self.state = PipelineState::Stopped;
    }

    pub fn fail(&mut self, message: impl Into<String>) {
        self.stop_platform_streams();
        self.config = None;
        self.runtime_info = None;
        self.level_meter.reset();
        self.state = PipelineState::Error(message.into());
    }

    #[cfg(windows)]
    fn start_platform_streams(&mut self, config: &AudioPipelineConfig) -> ClearLineResult<()> {
        let host = cpal::default_host();
        let input_device = resolve_input_device(&host, config.input_device_id())?;

        let input_supported_config = input_device
            .default_input_config()
            .map_err(|error| ClearLineError::StreamBuild(error.to_string()))?;
        let input_sample_format = input_supported_config.sample_format();
        let input_stream_config = input_supported_config.config();
        let input_stream_format = AudioStreamFormat::from_cpal_config(&input_stream_config);

        match config.output_target() {
            AudioOutputTarget::AudioDevice(output_device_id) => {
                let output_device = resolve_output_device(&host, output_device_id)?;
                let output_supported_config = output_device
                    .default_output_config()
                    .map_err(|error| ClearLineError::StreamBuild(error.to_string()))?;
                let output_sample_format = output_supported_config.sample_format();
                let mut output_stream_config = output_supported_config.config();
                let output_stream_format = passthrough_output_format(
                    input_stream_format,
                    AudioStreamFormat::from_cpal_config(&output_stream_config),
                );
                output_stream_config.sample_rate = output_stream_format.sample_rate_hz;
                output_stream_config.channels = output_stream_format.channels;

                let sample_buffer_capacity =
                    buffer_capacity_samples(&input_stream_config, &output_stream_config);
                let sample_buffer = AudioSampleBuffer::with_prebuffer(
                    sample_buffer_capacity,
                    startup_prebuffer_samples(&output_stream_config),
                );
                let meter = self.level_meter.clone();
                let input_frame_format = AudioFrameFormat::new(
                    input_stream_format.sample_rate_hz,
                    input_stream_format.channels,
                );
                let suppressor = try_create_suppressor_with_deepfilternet_bundle(
                    config.suppressor_mode(),
                    input_frame_format,
                    config.suppression_strength(),
                    config.deepfilternet_model_bundle().cloned(),
                )?;
                let echo_canceller =
                    create_echo_canceller(config.echo_cancellation_enabled(), input_frame_format)?;
                let echo_cancellation = echo_canceller.runtime_info();
                let reference_capture = start_reference_capture_for_echo(echo_cancellation)?;
                let reference_buffer = reference_capture
                    .as_ref()
                    .map(LoopbackReferenceCapture::shared_buffer);
                let runtime_info = PipelineRuntimeInfo::new(
                    input_frame_format,
                    AudioFrameFormat::new(
                        output_stream_format.sample_rate_hz,
                        output_stream_format.channels,
                    ),
                    suppressor.runtime_info(),
                    config.output_target().clone(),
                )
                .with_echo_cancellation(echo_cancellation)
                .with_wind_noise_reduction(config.wind_noise_reduction_enabled());
                let wind_noise_reducer = WindNoiseReducer::new(
                    input_frame_format,
                    if config.wind_noise_reduction_enabled() {
                        WindNoiseConfig::enabled()
                    } else {
                        WindNoiseConfig::default()
                    },
                );

                let input_stream = match input_sample_format {
                    SampleFormat::I8 => build_input_passthrough_stream::<i8>(
                        &input_device,
                        input_stream_config,
                        meter,
                        sample_buffer.clone(),
                        suppressor,
                        echo_canceller,
                        reference_buffer,
                        wind_noise_reducer,
                        input_stream_format.channels,
                        output_stream_format.channels,
                    ),
                    SampleFormat::I16 => build_input_passthrough_stream::<i16>(
                        &input_device,
                        input_stream_config,
                        meter,
                        sample_buffer.clone(),
                        suppressor,
                        echo_canceller,
                        reference_buffer,
                        wind_noise_reducer,
                        input_stream_format.channels,
                        output_stream_format.channels,
                    ),
                    SampleFormat::I32 => build_input_passthrough_stream::<i32>(
                        &input_device,
                        input_stream_config,
                        meter,
                        sample_buffer.clone(),
                        suppressor,
                        echo_canceller,
                        reference_buffer,
                        wind_noise_reducer,
                        input_stream_format.channels,
                        output_stream_format.channels,
                    ),
                    SampleFormat::I64 => build_input_passthrough_stream::<i64>(
                        &input_device,
                        input_stream_config,
                        meter,
                        sample_buffer.clone(),
                        suppressor,
                        echo_canceller,
                        reference_buffer,
                        wind_noise_reducer,
                        input_stream_format.channels,
                        output_stream_format.channels,
                    ),
                    SampleFormat::U8 => build_input_passthrough_stream::<u8>(
                        &input_device,
                        input_stream_config,
                        meter,
                        sample_buffer.clone(),
                        suppressor,
                        echo_canceller,
                        reference_buffer,
                        wind_noise_reducer,
                        input_stream_format.channels,
                        output_stream_format.channels,
                    ),
                    SampleFormat::U16 => build_input_passthrough_stream::<u16>(
                        &input_device,
                        input_stream_config,
                        meter,
                        sample_buffer.clone(),
                        suppressor,
                        echo_canceller,
                        reference_buffer,
                        wind_noise_reducer,
                        input_stream_format.channels,
                        output_stream_format.channels,
                    ),
                    SampleFormat::U32 => build_input_passthrough_stream::<u32>(
                        &input_device,
                        input_stream_config,
                        meter,
                        sample_buffer.clone(),
                        suppressor,
                        echo_canceller,
                        reference_buffer,
                        wind_noise_reducer,
                        input_stream_format.channels,
                        output_stream_format.channels,
                    ),
                    SampleFormat::U64 => build_input_passthrough_stream::<u64>(
                        &input_device,
                        input_stream_config,
                        meter,
                        sample_buffer.clone(),
                        suppressor,
                        echo_canceller,
                        reference_buffer,
                        wind_noise_reducer,
                        input_stream_format.channels,
                        output_stream_format.channels,
                    ),
                    SampleFormat::F32 => build_input_passthrough_stream::<f32>(
                        &input_device,
                        input_stream_config,
                        meter,
                        sample_buffer.clone(),
                        suppressor,
                        echo_canceller,
                        reference_buffer,
                        wind_noise_reducer,
                        input_stream_format.channels,
                        output_stream_format.channels,
                    ),
                    SampleFormat::F64 => build_input_passthrough_stream::<f64>(
                        &input_device,
                        input_stream_config,
                        meter,
                        sample_buffer.clone(),
                        suppressor,
                        echo_canceller,
                        reference_buffer,
                        wind_noise_reducer,
                        input_stream_format.channels,
                        output_stream_format.channels,
                    ),
                    unsupported => Err(ClearLineError::UnsupportedSampleFormat(format!(
                        "input {unsupported}"
                    ))),
                }?;

                let output_stream = match output_sample_format {
                    SampleFormat::I8 => build_output_passthrough_stream::<i8>(
                        &output_device,
                        output_stream_config,
                        sample_buffer.clone(),
                    ),
                    SampleFormat::I16 => build_output_passthrough_stream::<i16>(
                        &output_device,
                        output_stream_config,
                        sample_buffer.clone(),
                    ),
                    SampleFormat::I32 => build_output_passthrough_stream::<i32>(
                        &output_device,
                        output_stream_config,
                        sample_buffer.clone(),
                    ),
                    SampleFormat::I64 => build_output_passthrough_stream::<i64>(
                        &output_device,
                        output_stream_config,
                        sample_buffer.clone(),
                    ),
                    SampleFormat::U8 => build_output_passthrough_stream::<u8>(
                        &output_device,
                        output_stream_config,
                        sample_buffer.clone(),
                    ),
                    SampleFormat::U16 => build_output_passthrough_stream::<u16>(
                        &output_device,
                        output_stream_config,
                        sample_buffer.clone(),
                    ),
                    SampleFormat::U32 => build_output_passthrough_stream::<u32>(
                        &output_device,
                        output_stream_config,
                        sample_buffer.clone(),
                    ),
                    SampleFormat::U64 => build_output_passthrough_stream::<u64>(
                        &output_device,
                        output_stream_config,
                        sample_buffer.clone(),
                    ),
                    SampleFormat::F32 => build_output_passthrough_stream::<f32>(
                        &output_device,
                        output_stream_config,
                        sample_buffer.clone(),
                    ),
                    SampleFormat::F64 => build_output_passthrough_stream::<f64>(
                        &output_device,
                        output_stream_config,
                        sample_buffer.clone(),
                    ),
                    unsupported => Err(ClearLineError::UnsupportedSampleFormat(format!(
                        "output {unsupported}"
                    ))),
                }?;

                output_stream
                    .play()
                    .map_err(|error| ClearLineError::StreamPlay(error.to_string()))?;
                input_stream
                    .play()
                    .map_err(|error| ClearLineError::StreamPlay(error.to_string()))?;

                self.output_stream = Some(output_stream);
                self.input_stream = Some(input_stream);
                self.sample_buffer = Some(sample_buffer);
                self.reference_capture = reference_capture;
                self.runtime_info = Some(runtime_info);
            }
            AudioOutputTarget::ClearLineVirtualMicrophone => {
                let virtual_control = crate::virtual_mic::VirtualMicControl::new();
                let ping = virtual_control.ping()?;
                let output_stream_format = clearline_virtual_microphone_output_format(
                    ping.sample_rate_hz(),
                    ping.channels(),
                )?;
                if input_stream_format.sample_rate_hz != output_stream_format.sample_rate_hz {
                    return Err(ClearLineError::StreamBuild(format!(
                        "ClearLine Virtual Microphone output currently requires a 48000 Hz input stream; selected input uses {} Hz",
                        input_stream_format.sample_rate_hz
                    )));
                }

                let meter = self.level_meter.clone();
                let input_frame_format = AudioFrameFormat::new(
                    input_stream_format.sample_rate_hz,
                    input_stream_format.channels,
                );
                let suppressor = try_create_suppressor_with_deepfilternet_bundle(
                    config.suppressor_mode(),
                    input_frame_format,
                    config.suppression_strength(),
                    config.deepfilternet_model_bundle().cloned(),
                )?;
                let echo_canceller =
                    create_echo_canceller(config.echo_cancellation_enabled(), input_frame_format)?;
                let echo_cancellation = echo_canceller.runtime_info();
                let reference_capture = start_reference_capture_for_echo(echo_cancellation)?;
                let reference_buffer = reference_capture
                    .as_ref()
                    .map(LoopbackReferenceCapture::shared_buffer);
                let runtime_info = PipelineRuntimeInfo::new(
                    input_frame_format,
                    AudioFrameFormat::new(
                        output_stream_format.sample_rate_hz,
                        output_stream_format.channels,
                    ),
                    suppressor.runtime_info(),
                    config.output_target().clone(),
                )
                .with_echo_cancellation(echo_cancellation)
                .with_wind_noise_reduction(config.wind_noise_reduction_enabled());
                let wind_noise_reducer = WindNoiseReducer::new(
                    input_frame_format,
                    if config.wind_noise_reduction_enabled() {
                        WindNoiseConfig::enabled()
                    } else {
                        WindNoiseConfig::default()
                    },
                );

                let input_stream = match input_sample_format {
                    SampleFormat::I8 => build_input_virtual_microphone_stream::<i8>(
                        &input_device,
                        input_stream_config,
                        meter,
                        virtual_control,
                        suppressor,
                        echo_canceller,
                        reference_buffer,
                        wind_noise_reducer,
                        input_stream_format.channels,
                    ),
                    SampleFormat::I16 => build_input_virtual_microphone_stream::<i16>(
                        &input_device,
                        input_stream_config,
                        meter,
                        virtual_control,
                        suppressor,
                        echo_canceller,
                        reference_buffer,
                        wind_noise_reducer,
                        input_stream_format.channels,
                    ),
                    SampleFormat::I32 => build_input_virtual_microphone_stream::<i32>(
                        &input_device,
                        input_stream_config,
                        meter,
                        virtual_control,
                        suppressor,
                        echo_canceller,
                        reference_buffer,
                        wind_noise_reducer,
                        input_stream_format.channels,
                    ),
                    SampleFormat::I64 => build_input_virtual_microphone_stream::<i64>(
                        &input_device,
                        input_stream_config,
                        meter,
                        virtual_control,
                        suppressor,
                        echo_canceller,
                        reference_buffer,
                        wind_noise_reducer,
                        input_stream_format.channels,
                    ),
                    SampleFormat::U8 => build_input_virtual_microphone_stream::<u8>(
                        &input_device,
                        input_stream_config,
                        meter,
                        virtual_control,
                        suppressor,
                        echo_canceller,
                        reference_buffer,
                        wind_noise_reducer,
                        input_stream_format.channels,
                    ),
                    SampleFormat::U16 => build_input_virtual_microphone_stream::<u16>(
                        &input_device,
                        input_stream_config,
                        meter,
                        virtual_control,
                        suppressor,
                        echo_canceller,
                        reference_buffer,
                        wind_noise_reducer,
                        input_stream_format.channels,
                    ),
                    SampleFormat::U32 => build_input_virtual_microphone_stream::<u32>(
                        &input_device,
                        input_stream_config,
                        meter,
                        virtual_control,
                        suppressor,
                        echo_canceller,
                        reference_buffer,
                        wind_noise_reducer,
                        input_stream_format.channels,
                    ),
                    SampleFormat::U64 => build_input_virtual_microphone_stream::<u64>(
                        &input_device,
                        input_stream_config,
                        meter,
                        virtual_control,
                        suppressor,
                        echo_canceller,
                        reference_buffer,
                        wind_noise_reducer,
                        input_stream_format.channels,
                    ),
                    SampleFormat::F32 => build_input_virtual_microphone_stream::<f32>(
                        &input_device,
                        input_stream_config,
                        meter,
                        virtual_control,
                        suppressor,
                        echo_canceller,
                        reference_buffer,
                        wind_noise_reducer,
                        input_stream_format.channels,
                    ),
                    SampleFormat::F64 => build_input_virtual_microphone_stream::<f64>(
                        &input_device,
                        input_stream_config,
                        meter,
                        virtual_control,
                        suppressor,
                        echo_canceller,
                        reference_buffer,
                        wind_noise_reducer,
                        input_stream_format.channels,
                    ),
                    unsupported => Err(ClearLineError::UnsupportedSampleFormat(format!(
                        "input {unsupported}"
                    ))),
                }?;

                input_stream
                    .play()
                    .map_err(|error| ClearLineError::StreamPlay(error.to_string()))?;

                self.output_stream = None;
                self.input_stream = Some(input_stream);
                self.sample_buffer = None;
                self.reference_capture = reference_capture;
                self.runtime_info = Some(runtime_info);
            }
        }

        Ok(())
    }

    #[cfg(not(windows))]
    fn start_platform_streams(&mut self, _config: &AudioPipelineConfig) -> ClearLineResult<()> {
        Ok(())
    }

    #[cfg(windows)]
    fn stop_platform_streams(&mut self) {
        self.input_stream = None;
        self.output_stream = None;
        self.sample_buffer = None;
        self.reference_capture = None;
    }

    #[cfg(not(windows))]
    fn stop_platform_streams(&mut self) {}
}

impl Default for AudioPipeline {
    fn default() -> Self {
        Self {
            state: PipelineState::Stopped,
            config: None,
            runtime_info: None,
            level_meter: LevelMeter::default(),
            #[cfg(windows)]
            sample_buffer: None,
            #[cfg(windows)]
            input_stream: None,
            #[cfg(windows)]
            output_stream: None,
            #[cfg(windows)]
            reference_capture: None,
        }
    }
}

#[cfg(windows)]
fn resolve_input_device(host: &cpal::Host, device_id: &DeviceId) -> ClearLineResult<cpal::Device> {
    if let Ok(cpal_id) = device_id.as_str().parse::<cpal::DeviceId>() {
        if let Some(device) = host.device_by_id(&cpal_id) {
            return Ok(device);
        }
    }

    let mut devices = host
        .input_devices()
        .map_err(|error| ClearLineError::DeviceEnumeration(error.to_string()))?;
    devices
        .find(|device| {
            device
                .id()
                .is_ok_and(|id| id.to_string() == device_id.as_str())
        })
        .ok_or_else(|| ClearLineError::DeviceNotFound(device_id.as_str().to_owned()))
}

#[cfg(windows)]
fn resolve_output_device(host: &cpal::Host, device_id: &DeviceId) -> ClearLineResult<cpal::Device> {
    if let Ok(cpal_id) = device_id.as_str().parse::<cpal::DeviceId>() {
        if let Some(device) = host.device_by_id(&cpal_id) {
            return Ok(device);
        }
    }

    let mut devices = host
        .output_devices()
        .map_err(|error| ClearLineError::DeviceEnumeration(error.to_string()))?;
    devices
        .find(|device| {
            device
                .id()
                .is_ok_and(|id| id.to_string() == device_id.as_str())
        })
        .ok_or_else(|| ClearLineError::DeviceNotFound(device_id.as_str().to_owned()))
}

#[cfg(windows)]
fn buffer_capacity_samples(input: &cpal::StreamConfig, output: &cpal::StreamConfig) -> usize {
    let input_samples_per_second = input.sample_rate as usize * usize::from(input.channels);
    let output_samples_per_second = output.sample_rate as usize * usize::from(output.channels);

    input_samples_per_second
        .max(output_samples_per_second)
        .max(1)
        * 2
}

#[cfg(windows)]
fn startup_prebuffer_samples(output: &cpal::StreamConfig) -> usize {
    let output_samples_per_second = output.sample_rate as usize * usize::from(output.channels);

    (output_samples_per_second / 50).max(1)
}

#[cfg(windows)]
fn create_echo_canceller(
    enabled: bool,
    format: AudioFrameFormat,
) -> ClearLineResult<Box<dyn EchoCanceller + Send>> {
    if !enabled {
        return Ok(Box::new(NoopEchoCanceller::new(format)));
    }

    #[cfg(feature = "aec")]
    {
        Ok(Box::new(Aec3EchoWorker::new(format)?))
    }

    #[cfg(not(feature = "aec"))]
    {
        Ok(Box::new(NoopEchoCanceller::new(format)))
    }
}

#[cfg(windows)]
fn start_reference_capture_for_echo(
    echo_cancellation: EchoCancellerRuntimeInfo,
) -> ClearLineResult<Option<LoopbackReferenceCapture>> {
    if echo_cancellation.backend() == EchoCancellerBackend::Aec3 {
        LoopbackReferenceCapture::start_default(1_000).map(Some)
    } else {
        Ok(None)
    }
}

#[cfg(windows)]
fn build_input_passthrough_stream<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    meter: LevelMeter,
    sample_buffer: AudioSampleBuffer,
    mut suppressor: Box<dyn crate::suppressor::NoiseSuppressor>,
    mut echo_canceller: Box<dyn EchoCanceller + Send>,
    mut reference_buffer: Option<SharedReferenceFrameBuffer>,
    mut wind_noise_reducer: WindNoiseReducer,
    input_channels: u16,
    output_channels: u16,
) -> ClearLineResult<cpal::Stream>
where
    T: SizedSample + Sample + Send + 'static,
    f32: FromSample<T>,
{
    let mut scratch = InputCallbackScratch::default();

    device
        .build_input_stream(
            config,
            move |data: &[T], _| {
                scratch.copy_from_samples(data);

                meter.update_from_f32_samples(&scratch.input);
                let reference_source = reference_buffer
                    .as_mut()
                    .map(|buffer| buffer as &mut dyn ReferenceFrameSource);
                if let Err(error) = process_input_callback_frame(
                    &mut scratch,
                    echo_canceller.as_mut(),
                    reference_source,
                    &mut wind_noise_reducer,
                    suppressor.as_mut(),
                    input_channels,
                ) {
                    eprintln!("ClearLine input processing error: {error}");
                    return;
                }

                scratch.converted.clear();
                append_converted_channels(
                    &scratch.processed,
                    input_channels,
                    output_channels,
                    &mut scratch.converted,
                );
                sample_buffer.push_samples(&scratch.converted);
            },
            move |error| {
                eprintln!("ClearLine input stream error: {error}");
            },
            None,
        )
        .map_err(|error| ClearLineError::StreamBuild(error.to_string()))
}

#[cfg(windows)]
fn build_input_virtual_microphone_stream<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    meter: LevelMeter,
    virtual_control: crate::virtual_mic::VirtualMicControl,
    mut suppressor: Box<dyn crate::suppressor::NoiseSuppressor>,
    mut echo_canceller: Box<dyn EchoCanceller + Send>,
    mut reference_buffer: Option<SharedReferenceFrameBuffer>,
    mut wind_noise_reducer: WindNoiseReducer,
    input_channels: u16,
) -> ClearLineResult<cpal::Stream>
where
    T: SizedSample + Sample + Send + 'static,
    f32: FromSample<T>,
{
    let mut scratch = InputCallbackScratch::default();

    device
        .build_input_stream(
            config,
            move |data: &[T], _| {
                scratch.copy_from_samples(data);

                meter.update_from_f32_samples(&scratch.input);
                let reference_source = reference_buffer
                    .as_mut()
                    .map(|buffer| buffer as &mut dyn ReferenceFrameSource);
                if let Err(error) = process_input_callback_frame(
                    &mut scratch,
                    echo_canceller.as_mut(),
                    reference_source,
                    &mut wind_noise_reducer,
                    suppressor.as_mut(),
                    input_channels,
                ) {
                    eprintln!("ClearLine input processing error: {error}");
                    return;
                }

                scratch.virtual_microphone_pcm.clear();
                append_virtual_microphone_pcm_i16(
                    &scratch.processed,
                    input_channels,
                    &mut scratch.virtual_microphone_pcm,
                );
                if let Err(error) =
                    virtual_control.write_pcm_i16_mono_48k(&scratch.virtual_microphone_pcm)
                {
                    eprintln!("ClearLine virtual microphone output error: {error}");
                }
            },
            move |error| {
                eprintln!("ClearLine input stream error: {error}");
            },
            None,
        )
        .map_err(|error| ClearLineError::StreamBuild(error.to_string()))
}

#[cfg(windows)]
fn build_output_passthrough_stream<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    sample_buffer: AudioSampleBuffer,
) -> ClearLineResult<cpal::Stream>
where
    T: SizedSample + Sample + Send + 'static + FromSample<f32>,
{
    let mut scratch = OutputCallbackScratch::default();

    device
        .build_output_stream(
            config,
            move |data: &mut [T], _| {
                scratch.resize(data.len());
                sample_buffer.pop_samples_or_zero(scratch.samples_mut());

                for (sample, value) in data.iter_mut().zip(scratch.samples().iter().copied()) {
                    *sample = T::from_sample(value);
                }
            },
            move |error| {
                eprintln!("ClearLine output stream error: {error}");
            },
            None,
        )
        .map_err(|error| ClearLineError::StreamBuild(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::DeviceId;
    use crate::echo::{EchoCanceller, EchoCancellerBackend, EchoCancellerRuntimeInfo};
    use crate::preprocess::{WindNoiseConfig, WindNoiseReducer};
    use crate::reference::ReferenceFrameBuffer;
    use crate::suppressor::{
        AudioFrameFormat, BypassSuppressor, DeepFilterNetModelBundle, SuppressionStrength,
        SuppressorMode, SuppressorRuntimeInfo,
    };
    use std::fs;

    #[test]
    fn pipeline_starts_and_stops_with_selected_config() {
        let mut pipeline = AudioPipeline::new();
        assert_eq!(pipeline.state(), &PipelineState::Stopped);

        let config = AudioPipelineConfig::new(
            DeviceId::new("mic-1"),
            DeviceId::new("out-1"),
            SuppressorMode::Bypass,
        );
        pipeline.start(config.clone()).unwrap();

        assert_eq!(pipeline.state(), &PipelineState::Running);
        assert_eq!(pipeline.config(), Some(&config));
        assert_eq!(
            pipeline
                .config()
                .unwrap()
                .output_device_id()
                .map(DeviceId::as_str),
            Some("out-1")
        );

        pipeline.stop();

        assert_eq!(pipeline.state(), &PipelineState::Stopped);
        assert_eq!(pipeline.config(), None);
    }

    #[test]
    fn pipeline_can_store_error_state() {
        let mut pipeline = AudioPipeline::new();

        pipeline.fail("input device unavailable");

        assert_eq!(
            pipeline.state(),
            &PipelineState::Error("input device unavailable".to_owned())
        );
    }

    #[test]
    fn level_meter_tracks_peak_absolute_sample() {
        let meter = LevelMeter::new();

        meter.update_from_f32_samples(&[0.0, -0.25, 0.75, -0.5]);

        assert!((meter.level() - 0.75).abs() < f32::EPSILON);
    }

    #[test]
    fn level_meter_clamps_samples_to_unit_range() {
        let meter = LevelMeter::new();

        meter.update_from_f32_samples(&[0.0, -2.0, 1.5]);

        assert!((meter.level() - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn pipeline_exposes_initial_zero_input_level() {
        let pipeline = AudioPipeline::new();

        assert_eq!(pipeline.input_level(), 0.0);
    }

    #[test]
    fn echo_reference_diagnostics_preserve_reference_capture_stats() {
        let format = AudioFrameFormat::new(48_000, 2);
        let mut buffer = ReferenceFrameBuffer::new(format, 1);

        buffer.push_interleaved(&[0.2, -0.6, 0.8, 0.4]);
        let stats = buffer.stats();
        let diagnostics = EchoReferenceDiagnostics::from_reference_stats(stats);

        assert_eq!(diagnostics.level(), 0.6);
        assert_eq!(diagnostics.buffered_samples(), 1);
        assert_eq!(diagnostics.dropped_samples(), 1);
        assert_eq!(diagnostics.missing_frames(), 0);
        assert!(diagnostics.has_reference_audio(0.01));
    }

    #[test]
    fn sample_buffer_preserves_fifo_order() {
        let buffer = AudioSampleBuffer::new(4);

        buffer.push_samples(&[0.1, 0.2, 0.3]);

        let mut output = [0.0; 3];
        buffer.pop_samples_or_zero(&mut output);

        assert_eq!(output, [0.1, 0.2, 0.3]);
    }

    #[test]
    fn sample_buffer_drops_oldest_samples_when_full() {
        let buffer = AudioSampleBuffer::new(3);

        buffer.push_samples(&[0.1, 0.2, 0.3, 0.4]);

        let mut output = [0.0; 3];
        buffer.pop_samples_or_zero(&mut output);

        assert_eq!(output, [0.2, 0.3, 0.4]);
    }

    #[test]
    fn sample_buffer_zero_fills_when_empty() {
        let buffer = AudioSampleBuffer::new(3);

        buffer.push_samples(&[0.5]);

        let mut output = [0.0; 3];
        buffer.pop_samples_or_zero(&mut output);

        assert_eq!(output, [0.5, 0.0, 0.0]);
    }

    #[test]
    fn sample_buffer_reports_fill_underrun_and_drop_metrics() {
        let buffer = AudioSampleBuffer::new(3);

        buffer.push_samples(&[0.1, 0.2, 0.3, 0.4]);
        let after_push = buffer.metrics();
        assert_eq!(after_push.buffered_samples(), 3);
        assert_eq!(after_push.capacity_samples(), 3);
        assert_eq!(after_push.dropped_sample_count(), 1);
        assert!((after_push.fill_ratio() - 1.0).abs() < f32::EPSILON);

        let mut output = [0.0; 5];
        buffer.pop_samples_or_zero(&mut output);
        let after_pop = buffer.metrics();

        assert_eq!(after_pop.buffered_samples(), 0);
        assert_eq!(after_pop.underrun_sample_count(), 2);
    }

    #[test]
    fn sample_buffer_does_not_count_underrun_before_prebuffer_is_ready() {
        let buffer = AudioSampleBuffer::with_prebuffer(8, 4);

        let mut early_output = [1.0; 2];
        buffer.pop_samples_or_zero(&mut early_output);

        assert_eq!(early_output, [0.0, 0.0]);
        assert_eq!(buffer.metrics().underrun_sample_count(), 0);
        assert_eq!(buffer.metrics().buffered_samples(), 0);

        buffer.push_samples(&[0.1, 0.2, 0.3]);
        let mut not_ready_output = [1.0; 2];
        buffer.pop_samples_or_zero(&mut not_ready_output);

        assert_eq!(not_ready_output, [0.0, 0.0]);
        assert_eq!(buffer.metrics().underrun_sample_count(), 0);
        assert_eq!(buffer.metrics().buffered_samples(), 3);

        buffer.push_samples(&[0.4]);
        let mut ready_output = [0.0; 2];
        buffer.pop_samples_or_zero(&mut ready_output);

        assert_eq!(ready_output, [0.1, 0.2]);
        assert_eq!(buffer.metrics().underrun_sample_count(), 0);
        assert_eq!(buffer.metrics().buffered_samples(), 2);
    }

    #[test]
    fn sample_buffer_reenters_prebuffer_after_real_underrun() {
        let buffer = AudioSampleBuffer::with_prebuffer(8, 2);
        buffer.push_samples(&[0.1, 0.2]);

        let mut output = [0.0; 4];
        buffer.pop_samples_or_zero(&mut output);

        assert_eq!(output, [0.1, 0.2, 0.0, 0.0]);
        assert_eq!(buffer.metrics().underrun_sample_count(), 2);

        buffer.push_samples(&[0.3]);
        let mut rebuffering_output = [1.0; 1];
        buffer.pop_samples_or_zero(&mut rebuffering_output);

        assert_eq!(rebuffering_output, [0.0]);
        assert_eq!(buffer.metrics().underrun_sample_count(), 2);
        assert_eq!(buffer.metrics().buffered_samples(), 1);
    }

    #[test]
    fn pipeline_exposes_initial_zero_metrics() {
        let pipeline = AudioPipeline::new();
        let metrics = pipeline.metrics();

        assert_eq!(metrics.buffered_samples(), 0);
        assert_eq!(metrics.capacity_samples(), 0);
        assert_eq!(metrics.underrun_sample_count(), 0);
        assert_eq!(metrics.dropped_sample_count(), 0);
        assert_eq!(metrics.fill_ratio(), 0.0);
    }

    #[test]
    fn metrics_estimate_buffer_latency_from_audio_format() {
        let metrics = PipelineMetrics::new(960, 96_000, 0, 0);
        let format = AudioFrameFormat::new(48_000, 2);

        assert_eq!(metrics.buffered_latency_ms(format), Some(10));
        assert_eq!(metrics.capacity_latency_ms(format), Some(1_000));
        assert_eq!(PipelineMetrics::default().buffered_latency_ms(format), None);
    }

    #[test]
    fn pipeline_exposes_no_runtime_info_before_stream_starts() {
        let pipeline = AudioPipeline::new();

        assert_eq!(pipeline.runtime_info(), None);
    }

    #[test]
    fn runtime_info_exposes_stream_formats_and_suppressor_details() {
        let suppressor = SuppressorRuntimeInfo::new(
            SuppressorMode::LowLatency,
            "nnnoiseless-rnnoise",
            480,
            true,
        );
        let info = PipelineRuntimeInfo::new(
            AudioFrameFormat::new(48_000, 1),
            AudioFrameFormat::new(48_000, 2),
            suppressor,
            AudioOutputTarget::AudioDevice(DeviceId::new("out-1")),
        );

        assert_eq!(info.input_format(), AudioFrameFormat::new(48_000, 1));
        assert_eq!(info.output_format(), AudioFrameFormat::new(48_000, 2));
        assert_eq!(info.suppressor().backend_name(), "nnnoiseless-rnnoise");
        assert!(info.suppressor().is_real_noise_suppression());
    }

    #[test]
    fn wind_noise_reduction_is_disabled_by_default_and_can_be_enabled() {
        let config = AudioPipelineConfig::new(
            DeviceId::new("mic-1"),
            DeviceId::new("out-1"),
            SuppressorMode::LowLatency,
        );

        assert!(!config.wind_noise_reduction_enabled());

        let config = config.with_wind_noise_reduction(true);

        assert!(config.wind_noise_reduction_enabled());
    }

    #[test]
    fn pipeline_config_disables_echo_cancellation_by_default() {
        let config = AudioPipelineConfig::for_virtual_microphone(
            DeviceId::new("mic-1"),
            SuppressorMode::LowLatency,
        );

        assert!(!config.echo_cancellation_enabled());
    }

    #[test]
    fn pipeline_config_can_enable_echo_cancellation() {
        let config = AudioPipelineConfig::for_virtual_microphone(
            DeviceId::new("mic-1"),
            SuppressorMode::LowLatency,
        )
        .with_echo_cancellation(true);

        assert!(config.echo_cancellation_enabled());
    }

    #[test]
    fn pipeline_config_tracks_suppression_strength() {
        let config = AudioPipelineConfig::new(
            DeviceId::new("mic-1"),
            DeviceId::new("out-1"),
            SuppressorMode::HighQuality,
        );

        assert_eq!(config.suppression_strength(), SuppressionStrength::Balanced);

        let config = config.with_suppression_strength(SuppressionStrength::Strong);

        assert_eq!(config.suppression_strength(), SuppressionStrength::Strong);
    }

    #[test]
    fn pipeline_config_defaults_to_audio_device_output_target() {
        let config = AudioPipelineConfig::new(
            DeviceId::new("mic-1"),
            DeviceId::new("out-1"),
            SuppressorMode::LowLatency,
        );

        assert_eq!(
            config.output_target(),
            &AudioOutputTarget::AudioDevice(DeviceId::new("out-1"))
        );
        assert_eq!(
            config.output_device_id().map(DeviceId::as_str),
            Some("out-1")
        );
    }

    #[test]
    fn pipeline_config_can_target_clearline_virtual_microphone() {
        let config = AudioPipelineConfig::for_virtual_microphone(
            DeviceId::new("mic-1"),
            SuppressorMode::HighQuality,
        );

        assert_eq!(config.input_device_id().as_str(), "mic-1");
        assert_eq!(
            config.output_target(),
            &AudioOutputTarget::ClearLineVirtualMicrophone
        );
        assert_eq!(config.output_device_id(), None);
    }

    #[test]
    fn runtime_info_exposes_output_target() {
        let info = PipelineRuntimeInfo::new(
            AudioFrameFormat::new(48_000, 1),
            AudioFrameFormat::new(48_000, 1),
            SuppressorRuntimeInfo::new(
                SuppressorMode::LowLatency,
                "nnnoiseless-rnnoise",
                480,
                true,
            ),
            AudioOutputTarget::ClearLineVirtualMicrophone,
        );

        assert_eq!(
            info.output_target(),
            &AudioOutputTarget::ClearLineVirtualMicrophone
        );
    }

    #[test]
    fn runtime_info_exposes_echo_cancellation_details() {
        let input_format = AudioFrameFormat::new(48_000, 1);
        let info = PipelineRuntimeInfo::new(
            input_format,
            AudioFrameFormat::new(48_000, 1),
            SuppressorRuntimeInfo::new(
                SuppressorMode::LowLatency,
                "nnnoiseless-rnnoise",
                480,
                true,
            ),
            AudioOutputTarget::ClearLineVirtualMicrophone,
        );

        assert_eq!(
            info.echo_cancellation().backend(),
            EchoCancellerBackend::Disabled
        );

        let info = info.with_echo_cancellation(EchoCancellerRuntimeInfo::new(
            EchoCancellerBackend::Aec3,
            input_format,
        ));

        assert_eq!(
            info.echo_cancellation().backend(),
            EchoCancellerBackend::Aec3
        );
        assert_eq!(info.echo_cancellation().format(), input_format);
    }

    #[test]
    fn pipeline_config_can_store_deepfilternet_model_bundle() {
        let model_dir = unique_temp_model_dir("pipeline-deepfilter-config");
        fs::write(model_dir.join(DeepFilterNetModelBundle::ENCODER_FILE), []).unwrap();
        fs::write(
            model_dir.join(DeepFilterNetModelBundle::ERB_DECODER_FILE),
            [],
        )
        .unwrap();
        fs::write(
            model_dir.join(DeepFilterNetModelBundle::DF_DECODER_FILE),
            [],
        )
        .unwrap();
        fs::write(model_dir.join(DeepFilterNetModelBundle::CONFIG_FILE), []).unwrap();
        let bundle = DeepFilterNetModelBundle::from_dir(&model_dir).unwrap();

        let config = AudioPipelineConfig::new(
            DeviceId::new("mic-1"),
            DeviceId::new("out-1"),
            SuppressorMode::HighQuality,
        );

        assert_eq!(config.deepfilternet_model_bundle(), None);

        let config = config.with_deepfilternet_model_bundle(bundle);

        assert_eq!(
            config
                .deepfilternet_model_bundle()
                .map(DeepFilterNetModelBundle::root_dir),
            Some(model_dir.as_path())
        );
        let _ = fs::remove_dir_all(&model_dir);
    }

    #[test]
    fn passthrough_output_format_uses_input_sample_rate() {
        let input = AudioStreamFormat::new(44_100, 1);
        let output_default = AudioStreamFormat::new(48_000, 2);

        let output = passthrough_output_format(input, output_default);

        assert_eq!(output, AudioStreamFormat::new(44_100, 2));
    }

    #[test]
    fn channel_adapter_duplicates_mono_to_stereo() {
        let mut output = Vec::new();

        append_converted_channels(&[0.25, -0.5], 1, 2, &mut output);

        assert_eq!(output, [0.25, 0.25, -0.5, -0.5]);
    }

    #[test]
    fn channel_adapter_averages_stereo_to_mono() {
        let mut output = Vec::new();

        append_converted_channels(&[0.25, 0.75, -0.25, 0.25], 2, 1, &mut output);

        assert_eq!(output, [0.5, 0.0]);
    }

    #[test]
    fn virtual_microphone_pcm_conversion_mixes_stereo_to_mono_i16() {
        let mut output = Vec::new();

        append_virtual_microphone_pcm_i16(&[0.25, 0.75, -0.25, 0.25], 2, &mut output);

        assert_eq!(output, [16_384, 0]);
    }

    #[test]
    fn virtual_microphone_pcm_conversion_clamps_to_i16_range() {
        let mut output = Vec::new();

        append_virtual_microphone_pcm_i16(&[-2.0, 2.0], 1, &mut output);

        assert_eq!(output, [i16::MIN, i16::MAX]);
    }

    #[test]
    fn virtual_microphone_output_format_requires_driver_contract() {
        assert_eq!(
            clearline_virtual_microphone_output_format(48_000, 1).unwrap(),
            AudioStreamFormat::new(48_000, 1)
        );

        assert!(clearline_virtual_microphone_output_format(44_100, 1).is_err());
        assert!(clearline_virtual_microphone_output_format(48_000, 2).is_err());
    }

    #[test]
    fn virtual_microphone_pipeline_source_writes_processed_pcm_to_driver() {
        let source = include_str!("pipeline.rs");
        let builder_symbol = ["build_input", "_virtual_microphone_stream"].concat();
        let write_symbol = ["write_pcm", "_i16_mono_48k"].concat();
        let control_symbol = ["VirtualMic", "Control::new"].concat();

        assert!(source.contains(&builder_symbol));
        assert!(source.contains(&write_symbol));
        assert!(source.contains(&control_symbol));
    }

    #[test]
    fn echo_cancellation_stage_duplicates_mono_reference_for_stereo_capture() {
        let mut reference = ReferenceFrameBuffer::new(AudioFrameFormat::new(48_000, 2), 8);
        reference.push_interleaved(&[0.2, 0.6, -0.4, 0.0]);
        let mut canceller = RecordingEchoCanceller::new(AudioFrameFormat::new(48_000, 2));
        let mut output = Vec::new();
        let mut render = Vec::new();
        let mut mono = Vec::new();

        apply_echo_cancellation_stage(
            &mut canceller,
            Some(&mut reference),
            2,
            &[1.0, 0.5, -1.0, -0.5],
            &mut output,
            &mut render,
            &mut mono,
        )
        .unwrap();

        assert_eq!(canceller.last_render, vec![0.4, 0.4, -0.2, -0.2]);
        assert_slice_approx_eq(&output, &[0.6, 0.1, -0.8, -0.3]);
    }

    #[test]
    fn input_processing_runs_echo_before_noise_suppression() {
        let format = AudioFrameFormat::new(48_000, 2);
        let mut reference = ReferenceFrameBuffer::new(format, 4);
        reference.push_interleaved(&[0.5, 0.5]);
        let mut echo = RecordingEchoCanceller::new(format);
        let mut suppressor = BypassSuppressor::new(format);
        let mut wind = WindNoiseReducer::new(format, WindNoiseConfig::default());
        let mut scratch = InputCallbackScratch::default();

        scratch.copy_from_f32_samples(&[1.0, 0.75]);
        process_input_callback_frame(
            &mut scratch,
            &mut echo,
            Some(&mut reference),
            &mut wind,
            &mut suppressor,
            2,
        )
        .unwrap();

        assert_slice_approx_eq(&scratch.processed, &[0.5, 0.25]);
        assert_eq!(echo.last_capture, vec![1.0, 0.75]);
        assert_eq!(echo.last_render, vec![0.5, 0.5]);
    }

    #[test]
    fn input_stream_callbacks_use_shared_processing_frame_path() {
        let source = include_str!("pipeline.rs");

        assert!(
            source.matches("process_input_callback_frame(").count() >= 4,
            "passthrough and virtual microphone callbacks should both use the shared AEC -> wind -> suppressor frame path"
        );
    }

    #[test]
    fn input_callback_scratch_reuses_allocations_for_same_or_smaller_buffers() {
        let mut scratch = InputCallbackScratch::default();

        scratch.copy_from_f32_samples(&[0.1, 0.2, 0.3, 0.4]);
        scratch.prepare_processed_buffer();
        scratch.converted.clear();
        append_converted_channels(&scratch.processed, 1, 2, &mut scratch.converted);
        let capacities = scratch.capacities();

        scratch.copy_from_f32_samples(&[0.5, 0.6, 0.7, 0.8]);
        scratch.prepare_processed_buffer();
        scratch.converted.clear();
        append_converted_channels(&scratch.processed, 1, 2, &mut scratch.converted);

        assert_eq!(scratch.capacities(), capacities);

        scratch.copy_from_f32_samples(&[0.9, 1.0]);
        scratch.prepare_processed_buffer();
        scratch.converted.clear();
        append_converted_channels(&scratch.processed, 1, 2, &mut scratch.converted);

        assert_eq!(scratch.capacities(), capacities);
    }

    #[test]
    fn output_callback_scratch_reuses_allocations_for_same_or_smaller_buffers() {
        let mut scratch = OutputCallbackScratch::default();

        scratch.resize(128);
        scratch.samples_mut()[0] = 0.25;
        assert_eq!(scratch.samples()[0], 0.25);
        let capacity = scratch.capacity();

        scratch.resize(128);
        assert_eq!(scratch.capacity(), capacity);

        scratch.resize(64);
        assert_eq!(scratch.capacity(), capacity);
    }

    fn unique_temp_model_dir(prefix: &str) -> std::path::PathBuf {
        let mut dir = std::env::temp_dir();
        dir.push(format!(
            "clearline-{prefix}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn assert_slice_approx_eq(actual: &[f32], expected: &[f32]) {
        assert_eq!(actual.len(), expected.len());
        for (index, (actual, expected)) in actual.iter().zip(expected.iter()).enumerate() {
            assert!(
                (*actual - *expected).abs() < 1.0e-6,
                "sample {index}: expected {expected}, got {actual}"
            );
        }
    }

    #[derive(Debug)]
    struct RecordingEchoCanceller {
        format: AudioFrameFormat,
        last_capture: Vec<f32>,
        last_render: Vec<f32>,
    }

    impl RecordingEchoCanceller {
        fn new(format: AudioFrameFormat) -> Self {
            Self {
                format,
                last_capture: Vec::new(),
                last_render: Vec::new(),
            }
        }
    }

    impl EchoCanceller for RecordingEchoCanceller {
        fn process(
            &mut self,
            capture: &[f32],
            render: &[f32],
            output: &mut [f32],
        ) -> ClearLineResult<()> {
            self.last_capture.clear();
            self.last_capture.extend_from_slice(capture);
            self.last_render.clear();
            self.last_render.extend_from_slice(render);
            for ((output, capture), render) in output
                .iter_mut()
                .zip(capture.iter().copied())
                .zip(render.iter().copied())
            {
                *output = capture - render;
            }
            Ok(())
        }

        fn runtime_info(&self) -> EchoCancellerRuntimeInfo {
            EchoCancellerRuntimeInfo::new(EchoCancellerBackend::Aec3, self.format)
        }
    }
}
