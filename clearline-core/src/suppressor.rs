use std::{
    collections::VecDeque,
    fmt,
    path::{Path, PathBuf},
};

use crate::FrameChunker;
use crate::{ClearLineError, ClearLineResult};

#[cfg(feature = "deepfilternet")]
use std::{
    fs, io,
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        mpsc::{self, Receiver, Sender, SyncSender, TryRecvError, TrySendError},
        Arc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

#[cfg(feature = "deepfilternet")]
use flate2::{write::GzEncoder, Compression};
#[cfg(feature = "deepfilternet")]
use ndarray::Array2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuppressorMode {
    Bypass,
    LowLatency,
    HighQuality,
}

impl SuppressorMode {
    pub const ALL: [SuppressorMode; 3] = [
        SuppressorMode::Bypass,
        SuppressorMode::LowLatency,
        SuppressorMode::HighQuality,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            SuppressorMode::Bypass => "Bypass",
            SuppressorMode::LowLatency => "Low latency",
            SuppressorMode::HighQuality => "High quality",
        }
    }
}

impl fmt::Display for SuppressorMode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuppressionStrength {
    Gentle,
    Balanced,
    Strong,
}

impl SuppressionStrength {
    pub const ALL: [SuppressionStrength; 3] = [
        SuppressionStrength::Gentle,
        SuppressionStrength::Balanced,
        SuppressionStrength::Strong,
    ];
}

impl Default for SuppressionStrength {
    fn default() -> Self {
        Self::Balanced
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioFrameFormat {
    sample_rate_hz: u32,
    channels: u16,
}

impl AudioFrameFormat {
    pub fn new(sample_rate_hz: u32, channels: u16) -> Self {
        Self {
            sample_rate_hz,
            channels,
        }
    }

    pub fn sample_rate_hz(self) -> u32 {
        self.sample_rate_hz
    }

    pub fn channels(self) -> u16 {
        self.channels
    }
}

impl Default for AudioFrameFormat {
    fn default() -> Self {
        Self::new(48_000, 1)
    }
}

pub trait NoiseSuppressor: Send {
    fn mode(&self) -> SuppressorMode;

    fn format(&self) -> AudioFrameFormat;

    fn runtime_info(&self) -> SuppressorRuntimeInfo;

    fn process(&mut self, input: &[f32], output: &mut [f32]) -> ClearLineResult<()>;

    fn reset(&mut self) {}
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeepFilterNetModelBundle {
    root_dir: PathBuf,
    encoder_onnx: PathBuf,
    erb_decoder_onnx: PathBuf,
    df_decoder_onnx: PathBuf,
    config_ini: PathBuf,
}

impl DeepFilterNetModelBundle {
    pub const ENCODER_FILE: &'static str = "enc.onnx";
    pub const ERB_DECODER_FILE: &'static str = "erb_dec.onnx";
    pub const DF_DECODER_FILE: &'static str = "df_dec.onnx";
    pub const CONFIG_FILE: &'static str = "config.ini";

    pub fn from_dir(root_dir: impl AsRef<Path>) -> ClearLineResult<Self> {
        let root_dir = root_dir.as_ref().to_path_buf();
        let encoder_onnx = root_dir.join(Self::ENCODER_FILE);
        let erb_decoder_onnx = root_dir.join(Self::ERB_DECODER_FILE);
        let df_decoder_onnx = root_dir.join(Self::DF_DECODER_FILE);
        let config_ini = root_dir.join(Self::CONFIG_FILE);

        for path in [
            &encoder_onnx,
            &erb_decoder_onnx,
            &df_decoder_onnx,
            &config_ini,
        ] {
            if !path.is_file() {
                return Err(ClearLineError::ModelAssetMissing {
                    path: path.display().to_string(),
                });
            }
        }

        Ok(Self {
            root_dir,
            encoder_onnx,
            erb_decoder_onnx,
            df_decoder_onnx,
            config_ini,
        })
    }

    pub fn root_dir(&self) -> &Path {
        &self.root_dir
    }

    pub fn encoder_onnx(&self) -> &Path {
        &self.encoder_onnx
    }

    pub fn erb_decoder_onnx(&self) -> &Path {
        &self.erb_decoder_onnx
    }

    pub fn df_decoder_onnx(&self) -> &Path {
        &self.df_decoder_onnx
    }

    pub fn config_ini(&self) -> &Path {
        &self.config_ini
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SuppressorWorkerDiagnostics {
    input_queue_capacity: usize,
    pending_input_frames: usize,
    output_queue_capacity: usize,
    pending_output_frames: usize,
    dropped_input_frames: u64,
    dropped_output_frames: u64,
    late_output_frames: u64,
    inference_errors: u64,
    last_inference_time_ms: Option<u32>,
    max_inference_time_ms: Option<u32>,
    degraded: bool,
}

impl SuppressorWorkerDiagnostics {
    pub fn new(
        input_queue_capacity: usize,
        pending_input_frames: usize,
        output_queue_capacity: usize,
        pending_output_frames: usize,
    ) -> Self {
        Self {
            input_queue_capacity,
            pending_input_frames,
            output_queue_capacity,
            pending_output_frames,
            dropped_input_frames: 0,
            dropped_output_frames: 0,
            late_output_frames: 0,
            inference_errors: 0,
            last_inference_time_ms: None,
            max_inference_time_ms: None,
            degraded: false,
        }
    }

    pub fn input_queue_capacity(self) -> usize {
        self.input_queue_capacity
    }

    pub fn pending_input_frames(self) -> usize {
        self.pending_input_frames
    }

    pub fn output_queue_capacity(self) -> usize {
        self.output_queue_capacity
    }

    pub fn pending_output_frames(self) -> usize {
        self.pending_output_frames
    }

    pub fn dropped_input_frames(self) -> u64 {
        self.dropped_input_frames
    }

    pub fn dropped_output_frames(self) -> u64 {
        self.dropped_output_frames
    }

    pub fn late_output_frames(self) -> u64 {
        self.late_output_frames
    }

    pub fn inference_errors(self) -> u64 {
        self.inference_errors
    }

    pub fn last_inference_time_ms(self) -> Option<u32> {
        self.last_inference_time_ms
    }

    pub fn max_inference_time_ms(self) -> Option<u32> {
        self.max_inference_time_ms
    }

    pub fn is_degraded(self) -> bool {
        self.degraded
    }

    pub fn with_dropped_input_frames(mut self, count: u64) -> Self {
        self.dropped_input_frames = count;
        self
    }

    pub fn with_dropped_output_frames(mut self, count: u64) -> Self {
        self.dropped_output_frames = count;
        self
    }

    pub fn with_late_output_frames(mut self, count: u64) -> Self {
        self.late_output_frames = count;
        self
    }

    pub fn with_inference_errors(mut self, count: u64) -> Self {
        self.inference_errors = count;
        self
    }

    pub fn with_last_inference_time_ms(mut self, millis: u32) -> Self {
        self.last_inference_time_ms = Some(millis);
        self
    }

    pub fn with_max_inference_time_ms(mut self, millis: u32) -> Self {
        self.max_inference_time_ms = Some(millis);
        self
    }

    pub fn with_degraded(mut self, degraded: bool) -> Self {
        self.degraded = degraded;
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SuppressorRuntimeInfo {
    mode: SuppressorMode,
    backend_name: &'static str,
    frame_size_samples: usize,
    is_real_noise_suppression: bool,
    strength: Option<SuppressionStrength>,
    worker_diagnostics: Option<SuppressorWorkerDiagnostics>,
}

impl SuppressorRuntimeInfo {
    pub fn new(
        mode: SuppressorMode,
        backend_name: &'static str,
        frame_size_samples: usize,
        is_real_noise_suppression: bool,
    ) -> Self {
        Self {
            mode,
            backend_name,
            frame_size_samples,
            is_real_noise_suppression,
            strength: None,
            worker_diagnostics: None,
        }
    }

    pub fn mode(self) -> SuppressorMode {
        self.mode
    }

    pub fn backend_name(self) -> &'static str {
        self.backend_name
    }

    pub fn frame_size_samples(self) -> usize {
        self.frame_size_samples
    }

    pub fn is_real_noise_suppression(self) -> bool {
        self.is_real_noise_suppression
    }

    pub fn with_strength(mut self, strength: SuppressionStrength) -> Self {
        self.strength = Some(strength);
        self
    }

    pub fn strength(self) -> Option<SuppressionStrength> {
        self.strength
    }

    pub fn with_worker_diagnostics(mut self, diagnostics: SuppressorWorkerDiagnostics) -> Self {
        self.worker_diagnostics = Some(diagnostics);
        self
    }

    pub fn worker_diagnostics(self) -> Option<SuppressorWorkerDiagnostics> {
        self.worker_diagnostics
    }
}

#[derive(Debug, Clone)]
pub struct BypassSuppressor {
    format: AudioFrameFormat,
}

impl BypassSuppressor {
    pub fn new(format: AudioFrameFormat) -> Self {
        Self { format }
    }
}

impl NoiseSuppressor for BypassSuppressor {
    fn mode(&self) -> SuppressorMode {
        SuppressorMode::Bypass
    }

    fn format(&self) -> AudioFrameFormat {
        self.format
    }

    fn runtime_info(&self) -> SuppressorRuntimeInfo {
        SuppressorRuntimeInfo::new(self.mode(), "bypass-placeholder", 0, false)
    }

    fn process(&mut self, input: &[f32], output: &mut [f32]) -> ClearLineResult<()> {
        copy_samples(input, output)
    }
}

#[derive(Debug, Clone)]
pub struct LowLatencySuppressor {
    format: AudioFrameFormat,
    strength: SuppressionStrength,
    chunker: FrameChunker,
    backend: LowLatencyBackend,
    frame_input: Vec<f32>,
    frame_output: Vec<f32>,
    pending_output: VecDeque<f32>,
}

impl LowLatencySuppressor {
    pub fn new(format: AudioFrameFormat) -> Self {
        Self::new_with_strength(format, SuppressionStrength::default())
    }

    pub fn new_with_strength(format: AudioFrameFormat, strength: SuppressionStrength) -> Self {
        let frame_size_samples = low_latency_frame_size_samples(format);
        Self {
            format,
            strength,
            chunker: FrameChunker::new(frame_size_samples),
            backend: LowLatencyBackend::new(format, frame_size_samples, strength),
            frame_input: vec![0.0; frame_size_samples],
            frame_output: vec![0.0; frame_size_samples],
            pending_output: VecDeque::with_capacity(frame_size_samples * 2),
        }
    }

    pub fn backend_name(&self) -> &'static str {
        self.backend.name()
    }

    pub fn frame_size_samples(&self) -> usize {
        self.chunker.frame_size_samples()
    }
}

impl NoiseSuppressor for LowLatencySuppressor {
    fn mode(&self) -> SuppressorMode {
        SuppressorMode::LowLatency
    }

    fn format(&self) -> AudioFrameFormat {
        self.format
    }

    fn runtime_info(&self) -> SuppressorRuntimeInfo {
        SuppressorRuntimeInfo::new(
            self.mode(),
            self.backend.name(),
            self.frame_size_samples(),
            self.backend.is_real_noise_suppression(),
        )
        .with_strength(self.strength)
    }

    fn process(&mut self, input: &[f32], output: &mut [f32]) -> ClearLineResult<()> {
        if input.len() != output.len() {
            return Err(ClearLineError::BufferSizeMismatch {
                input: input.len(),
                output: output.len(),
            });
        }

        self.chunker.push_samples(input);
        while self.chunker.pop_frame(&mut self.frame_input) {
            self.backend
                .process_frame(&self.frame_input, &mut self.frame_output)?;
            self.pending_output
                .extend(self.frame_output.iter().copied());
        }

        for sample in output {
            *sample = self.pending_output.pop_front().unwrap_or(0.0);
        }

        Ok(())
    }

    fn reset(&mut self) {
        self.chunker.reset();
        self.backend.reset();
        self.pending_output.clear();
        self.frame_input.fill(0.0);
        self.frame_output.fill(0.0);
    }
}

#[derive(Clone)]
enum LowLatencyBackend {
    #[cfg(feature = "rnnoise")]
    Nnnoiseless(NnnoiselessBackend),
    Passthrough,
}

impl LowLatencyBackend {
    fn new(
        format: AudioFrameFormat,
        frame_size_samples: usize,
        strength: SuppressionStrength,
    ) -> Self {
        #[cfg(feature = "rnnoise")]
        if NnnoiselessBackend::supports(format, frame_size_samples) {
            return Self::Nnnoiseless(NnnoiselessBackend::new(format.channels(), strength));
        }
        #[cfg(not(feature = "rnnoise"))]
        let _ = (format, frame_size_samples, strength);

        Self::Passthrough
    }

    fn name(&self) -> &'static str {
        match self {
            #[cfg(feature = "rnnoise")]
            Self::Nnnoiseless(_) => "nnnoiseless-rnnoise",
            Self::Passthrough => "bypass-placeholder",
        }
    }

    fn is_real_noise_suppression(&self) -> bool {
        match self {
            #[cfg(feature = "rnnoise")]
            Self::Nnnoiseless(_) => true,
            Self::Passthrough => false,
        }
    }

    fn process_frame(&mut self, input: &[f32], output: &mut [f32]) -> ClearLineResult<()> {
        match self {
            #[cfg(feature = "rnnoise")]
            Self::Nnnoiseless(backend) => backend.process_frame(input, output),
            Self::Passthrough => copy_samples(input, output),
        }
    }

    fn reset(&mut self) {
        match self {
            #[cfg(feature = "rnnoise")]
            Self::Nnnoiseless(backend) => backend.reset(),
            Self::Passthrough => {}
        }
    }
}

impl fmt::Debug for LowLatencyBackend {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LowLatencyBackend")
            .field("name", &self.name())
            .finish()
    }
}

#[cfg(feature = "rnnoise")]
#[derive(Clone)]
struct NnnoiselessBackend {
    state: Box<nnnoiseless::DenoiseState<'static>>,
    channels: usize,
    strength: SuppressionStrength,
    scaled_input: Vec<f32>,
    scaled_output: Vec<f32>,
    discarded_warmup_frame: bool,
}

#[cfg(feature = "rnnoise")]
impl NnnoiselessBackend {
    const INPUT_SAMPLE_RATE_HZ: u32 = 48_000;
    const FLOAT_TO_I16_SCALE: f32 = 32_768.0;
    const I16_TO_FLOAT_SCALE: f32 = 1.0 / Self::FLOAT_TO_I16_SCALE;

    fn new(channels: u16, strength: SuppressionStrength) -> Self {
        Self {
            state: nnnoiseless::DenoiseState::new(),
            channels: usize::from(channels.max(1)),
            strength,
            scaled_input: vec![0.0; nnnoiseless::DenoiseState::FRAME_SIZE],
            scaled_output: vec![0.0; nnnoiseless::DenoiseState::FRAME_SIZE],
            discarded_warmup_frame: false,
        }
    }

    fn supports(format: AudioFrameFormat, frame_size_samples: usize) -> bool {
        let channels = usize::from(format.channels().max(1));
        format.sample_rate_hz() == Self::INPUT_SAMPLE_RATE_HZ
            && frame_size_samples == nnnoiseless::DenoiseState::FRAME_SIZE * channels
    }

    fn process_frame(&mut self, input: &[f32], output: &mut [f32]) -> ClearLineResult<()> {
        if input.len() != output.len() {
            return Err(ClearLineError::BufferSizeMismatch {
                input: input.len(),
                output: output.len(),
            });
        }
        let expected_frame_samples = nnnoiseless::DenoiseState::FRAME_SIZE * self.channels;
        if input.len() != expected_frame_samples {
            return Err(ClearLineError::BufferSizeMismatch {
                input: input.len(),
                output: expected_frame_samples,
            });
        }

        for (scaled, frame) in self
            .scaled_input
            .iter_mut()
            .zip(input.chunks_exact(self.channels))
        {
            let mono = frame.iter().copied().sum::<f32>() / self.channels as f32;
            *scaled = mono.clamp(-1.0, 1.0) * Self::FLOAT_TO_I16_SCALE;
        }

        let voice_probability = self
            .state
            .process_frame(&mut self.scaled_output, &self.scaled_input);

        if !self.discarded_warmup_frame {
            output.fill(0.0);
            self.discarded_warmup_frame = true;
            return Ok(());
        }

        for (frame, (processed, dry)) in output.chunks_exact_mut(self.channels).zip(
            self.scaled_output
                .iter()
                .copied()
                .zip(self.scaled_input.iter().copied()),
        ) {
            let processed_sample = (processed * Self::I16_TO_FLOAT_SCALE).clamp(-1.0, 1.0);
            let dry_sample = (dry * Self::I16_TO_FLOAT_SCALE).clamp(-1.0, 1.0);
            let dry_mix = rnnoise_dry_mix_for_strength(self.strength);
            let gate_gain = rnnoise_non_speech_gain_for_strength(self.strength, voice_probability);
            let sample =
                ((processed_sample * (1.0 - dry_mix)) + (dry_sample * dry_mix)) * gate_gain;
            frame.fill(sample);
        }

        Ok(())
    }

    fn reset(&mut self) {
        *self = Self::new(self.channels as u16, self.strength);
    }
}

#[cfg(feature = "rnnoise")]
fn rnnoise_dry_mix_for_strength(strength: SuppressionStrength) -> f32 {
    match strength {
        SuppressionStrength::Gentle => 0.30,
        SuppressionStrength::Balanced | SuppressionStrength::Strong => 0.0,
    }
}

#[cfg(feature = "rnnoise")]
fn rnnoise_non_speech_gain_for_strength(
    strength: SuppressionStrength,
    voice_probability: f32,
) -> f32 {
    match strength {
        SuppressionStrength::Gentle | SuppressionStrength::Balanced => 1.0,
        SuppressionStrength::Strong => {
            let vad = voice_probability.clamp(0.0, 1.0);
            if vad <= 0.35 {
                0.55
            } else if vad >= 0.75 {
                1.0
            } else {
                0.55 + ((vad - 0.35) / 0.40) * 0.45
            }
        }
    }
}

#[cfg(feature = "rnnoise")]
impl fmt::Debug for NnnoiselessBackend {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NnnoiselessBackend")
            .field("frame_size_samples", &nnnoiseless::DenoiseState::FRAME_SIZE)
            .field("channels", &self.channels)
            .field("discarded_warmup_frame", &self.discarded_warmup_frame)
            .finish()
    }
}

pub struct HighQualitySuppressor {
    format: AudioFrameFormat,
    strength: SuppressionStrength,
    chunker: FrameChunker,
    backend: HighQualityBackend,
    frame_input: Vec<f32>,
    frame_output: Vec<f32>,
    pending_output: VecDeque<f32>,
}

impl HighQualitySuppressor {
    pub fn new(format: AudioFrameFormat) -> Self {
        Self::new_with_strength(format, SuppressionStrength::default())
    }

    pub fn new_with_strength(format: AudioFrameFormat, strength: SuppressionStrength) -> Self {
        let frame_size_samples = high_quality_frame_size_samples(format);
        Self {
            format,
            strength,
            chunker: FrameChunker::new(frame_size_samples),
            backend: HighQualityBackend::Adaptive(AdaptiveHighQualityBackend::new(
                format.channels(),
                strength,
            )),
            frame_input: vec![0.0; frame_size_samples],
            frame_output: vec![0.0; frame_size_samples],
            pending_output: VecDeque::with_capacity(frame_size_samples * 2),
        }
    }

    #[cfg(feature = "deepfilternet")]
    pub fn new_with_deepfilternet_bundle(
        format: AudioFrameFormat,
        strength: SuppressionStrength,
        model_bundle: DeepFilterNetModelBundle,
    ) -> Self {
        let backend = DeepFilterNetExperimentalBackend::new(format, strength, model_bundle)
            .map(HighQualityBackend::DeepFilterNet)
            .unwrap_or_else(|error| {
                eprintln!("ClearLine DeepFilterNet load failed: {error}");
                HighQualityBackend::Adaptive(AdaptiveHighQualityBackend::new(
                    format.channels(),
                    strength,
                ))
            });
        let frame_size_samples = backend.frame_size_samples(format);
        Self {
            format,
            strength,
            chunker: FrameChunker::new(frame_size_samples),
            backend,
            frame_input: vec![0.0; frame_size_samples],
            frame_output: vec![0.0; frame_size_samples],
            pending_output: VecDeque::with_capacity(frame_size_samples * 2),
        }
    }

    #[cfg(feature = "deepfilternet")]
    fn try_new_with_deepfilternet_bundle(
        format: AudioFrameFormat,
        strength: SuppressionStrength,
        model_bundle: DeepFilterNetModelBundle,
    ) -> ClearLineResult<Self> {
        let backend = DeepFilterNetExperimentalBackend::new(format, strength, model_bundle)
            .map(HighQualityBackend::DeepFilterNet)?;
        let frame_size_samples = backend.frame_size_samples(format);
        Ok(Self {
            format,
            strength,
            chunker: FrameChunker::new(frame_size_samples),
            backend,
            frame_input: vec![0.0; frame_size_samples],
            frame_output: vec![0.0; frame_size_samples],
            pending_output: VecDeque::with_capacity(frame_size_samples * 2),
        })
    }

    pub fn backend_name(&self) -> &'static str {
        self.backend.name()
    }

    pub fn frame_size_samples(&self) -> usize {
        self.chunker.frame_size_samples()
    }
}

impl NoiseSuppressor for HighQualitySuppressor {
    fn mode(&self) -> SuppressorMode {
        SuppressorMode::HighQuality
    }

    fn format(&self) -> AudioFrameFormat {
        self.format
    }

    fn runtime_info(&self) -> SuppressorRuntimeInfo {
        let mut info = SuppressorRuntimeInfo::new(
            self.mode(),
            self.backend_name(),
            self.frame_size_samples(),
            self.backend.is_real_noise_suppression(),
        )
        .with_strength(self.strength);

        if let Some(diagnostics) = self.backend.worker_diagnostics() {
            info = info.with_worker_diagnostics(diagnostics);
        }

        info
    }

    fn process(&mut self, input: &[f32], output: &mut [f32]) -> ClearLineResult<()> {
        if input.len() != output.len() {
            return Err(ClearLineError::BufferSizeMismatch {
                input: input.len(),
                output: output.len(),
            });
        }

        self.chunker.push_samples(input);
        while self.chunker.pop_frame(&mut self.frame_input) {
            self.backend
                .process_frame(&self.frame_input, &mut self.frame_output)?;
            self.pending_output
                .extend(self.frame_output.iter().copied());
        }

        for sample in output {
            *sample = self.pending_output.pop_front().unwrap_or(0.0);
        }

        Ok(())
    }

    fn reset(&mut self) {
        self.chunker.reset();
        self.backend.reset();
        self.pending_output.clear();
        self.frame_input.fill(0.0);
        self.frame_output.fill(0.0);
    }
}

enum HighQualityBackend {
    Adaptive(AdaptiveHighQualityBackend),
    #[cfg(feature = "deepfilternet")]
    DeepFilterNet(DeepFilterNetExperimentalBackend),
}

impl HighQualityBackend {
    fn name(&self) -> &'static str {
        match self {
            Self::Adaptive(backend) => backend.name(),
            #[cfg(feature = "deepfilternet")]
            Self::DeepFilterNet(backend) => backend.name(),
        }
    }

    fn is_real_noise_suppression(&self) -> bool {
        match self {
            Self::Adaptive(backend) => backend.is_real_noise_suppression(),
            #[cfg(feature = "deepfilternet")]
            Self::DeepFilterNet(backend) => backend.is_real_noise_suppression(),
        }
    }

    fn process_frame(&mut self, input: &[f32], output: &mut [f32]) -> ClearLineResult<()> {
        match self {
            Self::Adaptive(backend) => backend.process_frame(input, output),
            #[cfg(feature = "deepfilternet")]
            Self::DeepFilterNet(backend) => backend.process_frame(input, output),
        }
    }

    fn reset(&mut self) {
        match self {
            Self::Adaptive(backend) => backend.reset(),
            #[cfg(feature = "deepfilternet")]
            Self::DeepFilterNet(backend) => backend.reset(),
        }
    }

    #[cfg(feature = "deepfilternet")]
    fn frame_size_samples(&self, format: AudioFrameFormat) -> usize {
        match self {
            Self::Adaptive(_) => high_quality_frame_size_samples(format),
            #[cfg(feature = "deepfilternet")]
            Self::DeepFilterNet(backend) => backend.frame_size_samples(),
        }
    }

    fn worker_diagnostics(&self) -> Option<SuppressorWorkerDiagnostics> {
        match self {
            Self::Adaptive(_) => None,
            #[cfg(feature = "deepfilternet")]
            Self::DeepFilterNet(backend) => Some(backend.diagnostics()),
        }
    }
}

#[cfg(feature = "deepfilternet")]
struct DeepFilterNetExperimentalBackend {
    bridge: DeepFilterNetRealtimeBridge,
    stop_sender: Option<Sender<()>>,
    worker: Option<JoinHandle<()>>,
    channels: usize,
    hop_size: usize,
}

#[cfg(feature = "deepfilternet")]
impl DeepFilterNetExperimentalBackend {
    fn new(
        format: AudioFrameFormat,
        strength: SuppressionStrength,
        model_bundle: DeepFilterNetModelBundle,
    ) -> ClearLineResult<Self> {
        let channels = usize::from(format.channels().max(1));
        let params_file = deepfilternet_bundle_as_temp_targz(&model_bundle)?;
        let df_params_result = deepfilternet_load_params(params_file.clone());
        let _ = fs::remove_file(params_file);
        let df_params = df_params_result?;
        let runtime_params = deepfilternet_runtime_params(channels, strength);
        let model = deepfilternet_create_model(df_params, &runtime_params)?;

        if model.sr as u32 != format.sample_rate_hz() {
            return Err(ClearLineError::ModelLoad(format!(
                "DeepFilterNet model sample rate is {} Hz, input is {} Hz",
                model.sr,
                format.sample_rate_hz()
            )));
        }

        let hop_size = model.hop_size;
        let frame_size_samples = hop_size * channels;
        let metrics = Arc::new(DeepFilterNetWorkerMetrics::new(
            DEEPFILTERNET_WORKER_QUEUE_CAPACITY,
            DEEPFILTERNET_WORKER_QUEUE_CAPACITY,
        ));
        let (input_sender, input_receiver) =
            mpsc::sync_channel(DEEPFILTERNET_WORKER_QUEUE_CAPACITY);
        let (output_sender, output_receiver) =
            mpsc::sync_channel(DEEPFILTERNET_WORKER_QUEUE_CAPACITY);
        let (stop_sender, stop_receiver) = mpsc::channel();
        let worker_metrics = metrics.clone();
        let worker_model = SendDfTract(model);
        let worker = thread::Builder::new()
            .name("clearline-deepfilternet".to_owned())
            .spawn(move || {
                deepfilternet_worker_loop(
                    worker_model,
                    channels,
                    hop_size,
                    input_receiver,
                    output_sender,
                    stop_receiver,
                    worker_metrics,
                );
            })
            .map_err(|error| ClearLineError::ModelLoad(error.to_string()))?;

        Ok(Self {
            bridge: DeepFilterNetRealtimeBridge::new(
                input_sender,
                output_receiver,
                metrics,
                frame_size_samples,
            ),
            stop_sender: Some(stop_sender),
            worker: Some(worker),
            channels,
            hop_size,
        })
    }

    fn name(&self) -> &'static str {
        "deepfilternet-tract-worker"
    }

    fn is_real_noise_suppression(&self) -> bool {
        true
    }

    fn process_frame(&mut self, input: &[f32], output: &mut [f32]) -> ClearLineResult<()> {
        if self
            .worker
            .as_ref()
            .is_some_and(|worker| worker.is_finished())
        {
            self.bridge.metrics.mark_degraded();
        }
        self.bridge.process_frame(input, output)
    }

    fn reset(&mut self) {
        self.bridge.reset();
    }

    fn frame_size_samples(&self) -> usize {
        self.hop_size * self.channels
    }

    fn diagnostics(&self) -> SuppressorWorkerDiagnostics {
        self.bridge.diagnostics()
    }
}

#[cfg(feature = "deepfilternet")]
impl Drop for DeepFilterNetExperimentalBackend {
    fn drop(&mut self) {
        if let Some(stop_sender) = self.stop_sender.take() {
            let _ = stop_sender.send(());
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[cfg(feature = "deepfilternet")]
fn deepfilternet_worker_loop(
    mut model: SendDfTract,
    channels: usize,
    hop_size: usize,
    input_receiver: Receiver<Vec<f32>>,
    output_sender: SyncSender<Vec<f32>>,
    stop_receiver: Receiver<()>,
    metrics: Arc<DeepFilterNetWorkerMetrics>,
) {
    let mut input_frame = Array2::zeros((channels, hop_size));
    let mut output_frame = Array2::zeros((channels, hop_size));
    let frame_size_samples = channels * hop_size;

    loop {
        if stop_receiver.try_recv().is_ok() {
            break;
        }

        let samples = match input_receiver.recv_timeout(Duration::from_millis(10)) {
            Ok(samples) => {
                metrics.record_input_dequeued();
                samples
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        };

        if samples.len() != frame_size_samples {
            metrics.record_inference_error();
            metrics.mark_degraded();
            continue;
        }

        deinterleave_deepfilternet_frame(&samples, channels, hop_size, &mut input_frame);
        let started = Instant::now();
        let result = model.0.process(input_frame.view(), output_frame.view_mut());
        let elapsed_ms = started
            .elapsed()
            .as_millis()
            .max(1)
            .min(u128::from(u32::MAX)) as u32;
        metrics.record_inference_time_ms(elapsed_ms);

        if result.is_err() {
            metrics.record_inference_error();
            metrics.mark_degraded();
            continue;
        }

        let mut processed = vec![0.0; frame_size_samples];
        interleave_deepfilternet_frame(output_frame.view(), channels, hop_size, &mut processed);
        match output_sender.try_send(processed) {
            Ok(()) => metrics.record_output_enqueued(),
            Err(TrySendError::Full(_)) => metrics.record_dropped_output(),
            Err(TrySendError::Disconnected(_)) => break,
        }
    }
}

#[cfg(feature = "deepfilternet")]
fn deinterleave_deepfilternet_frame(
    input: &[f32],
    channels: usize,
    hop_size: usize,
    output: &mut Array2<f32>,
) {
    for frame_index in 0..hop_size {
        let input_offset = frame_index * channels;
        for channel in 0..channels {
            output[[channel, frame_index]] = input[input_offset + channel];
        }
    }
}

#[cfg(feature = "deepfilternet")]
fn interleave_deepfilternet_frame(
    input: ndarray::ArrayView2<'_, f32>,
    channels: usize,
    hop_size: usize,
    output: &mut [f32],
) {
    for frame_index in 0..hop_size {
        let output_offset = frame_index * channels;
        for channel in 0..channels {
            output[output_offset + channel] = input[[channel, frame_index]].clamp(-1.0, 1.0);
        }
    }
}

#[cfg(feature = "deepfilternet")]
struct SendDfTract(df::tract::DfTract);

#[cfg(feature = "deepfilternet")]
unsafe impl Send for SendDfTract {}

#[cfg(feature = "deepfilternet")]
const DEEPFILTERNET_WORKER_QUEUE_CAPACITY: usize = 3;

#[cfg(feature = "deepfilternet")]
struct DeepFilterNetWorkerMetrics {
    input_queue_capacity: usize,
    output_queue_capacity: usize,
    pending_input_frames: AtomicUsize,
    pending_output_frames: AtomicUsize,
    dropped_input_frames: AtomicU64,
    dropped_output_frames: AtomicU64,
    late_output_frames: AtomicU64,
    inference_errors: AtomicU64,
    last_inference_time_ms: AtomicU64,
    max_inference_time_ms: AtomicU64,
    degraded: AtomicBool,
}

#[cfg(feature = "deepfilternet")]
impl DeepFilterNetWorkerMetrics {
    fn new(input_queue_capacity: usize, output_queue_capacity: usize) -> Self {
        Self {
            input_queue_capacity,
            output_queue_capacity,
            pending_input_frames: AtomicUsize::new(0),
            pending_output_frames: AtomicUsize::new(0),
            dropped_input_frames: AtomicU64::new(0),
            dropped_output_frames: AtomicU64::new(0),
            late_output_frames: AtomicU64::new(0),
            inference_errors: AtomicU64::new(0),
            last_inference_time_ms: AtomicU64::new(0),
            max_inference_time_ms: AtomicU64::new(0),
            degraded: AtomicBool::new(false),
        }
    }

    fn record_input_enqueued(&self) {
        self.pending_input_frames.fetch_add(1, Ordering::Relaxed);
    }

    fn record_input_dequeued(&self) {
        saturating_fetch_sub(&self.pending_input_frames, 1);
    }

    fn record_output_enqueued(&self) {
        self.pending_output_frames.fetch_add(1, Ordering::Relaxed);
    }

    fn record_output_dequeued(&self) {
        saturating_fetch_sub(&self.pending_output_frames, 1);
    }

    fn record_dropped_input(&self) {
        self.dropped_input_frames.fetch_add(1, Ordering::Relaxed);
    }

    fn record_dropped_output(&self) {
        self.dropped_output_frames.fetch_add(1, Ordering::Relaxed);
    }

    fn record_late_output(&self) {
        self.late_output_frames.fetch_add(1, Ordering::Relaxed);
    }

    fn record_inference_error(&self) {
        self.inference_errors.fetch_add(1, Ordering::Relaxed);
    }

    fn record_inference_time_ms(&self, millis: u32) {
        let millis = u64::from(millis.max(1));
        self.last_inference_time_ms.store(millis, Ordering::Relaxed);
        let mut current = self.max_inference_time_ms.load(Ordering::Relaxed);
        while millis > current {
            match self.max_inference_time_ms.compare_exchange(
                current,
                millis,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(actual) => current = actual,
            }
        }
    }

    fn mark_degraded(&self) {
        self.degraded.store(true, Ordering::Relaxed);
    }

    fn snapshot(&self) -> SuppressorWorkerDiagnostics {
        let last = nonzero_u64_to_u32(self.last_inference_time_ms.load(Ordering::Relaxed));
        let max = nonzero_u64_to_u32(self.max_inference_time_ms.load(Ordering::Relaxed));
        let mut diagnostics = SuppressorWorkerDiagnostics::new(
            self.input_queue_capacity,
            self.pending_input_frames.load(Ordering::Relaxed),
            self.output_queue_capacity,
            self.pending_output_frames.load(Ordering::Relaxed),
        )
        .with_dropped_input_frames(self.dropped_input_frames.load(Ordering::Relaxed))
        .with_dropped_output_frames(self.dropped_output_frames.load(Ordering::Relaxed))
        .with_late_output_frames(self.late_output_frames.load(Ordering::Relaxed))
        .with_inference_errors(self.inference_errors.load(Ordering::Relaxed))
        .with_degraded(self.degraded.load(Ordering::Relaxed));

        if let Some(last) = last {
            diagnostics = diagnostics.with_last_inference_time_ms(last);
        }
        if let Some(max) = max {
            diagnostics = diagnostics.with_max_inference_time_ms(max);
        }
        diagnostics
    }
}

#[cfg(feature = "deepfilternet")]
fn saturating_fetch_sub(value: &AtomicUsize, amount: usize) {
    let mut current = value.load(Ordering::Relaxed);
    loop {
        let next = current.saturating_sub(amount);
        match value.compare_exchange(current, next, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => break,
            Err(actual) => current = actual,
        }
    }
}

#[cfg(feature = "deepfilternet")]
fn nonzero_u64_to_u32(value: u64) -> Option<u32> {
    if value == 0 {
        None
    } else {
        Some(value.min(u64::from(u32::MAX)) as u32)
    }
}

#[cfg(feature = "deepfilternet")]
struct DeepFilterNetRealtimeBridge {
    input_sender: SyncSender<Vec<f32>>,
    output_receiver: Receiver<Vec<f32>>,
    metrics: Arc<DeepFilterNetWorkerMetrics>,
    frame_size_samples: usize,
    last_output: Vec<f32>,
    has_last_output: bool,
}

#[cfg(feature = "deepfilternet")]
impl DeepFilterNetRealtimeBridge {
    fn new(
        input_sender: SyncSender<Vec<f32>>,
        output_receiver: Receiver<Vec<f32>>,
        metrics: Arc<DeepFilterNetWorkerMetrics>,
        frame_size_samples: usize,
    ) -> Self {
        Self {
            input_sender,
            output_receiver,
            metrics,
            frame_size_samples,
            last_output: vec![0.0; frame_size_samples],
            has_last_output: false,
        }
    }

    fn process_frame(&mut self, input: &[f32], output: &mut [f32]) -> ClearLineResult<()> {
        if input.len() != self.frame_size_samples || output.len() != self.frame_size_samples {
            return Err(ClearLineError::BufferSizeMismatch {
                input: input.len(),
                output: output.len(),
            });
        }

        let newest_output = self.drain_processed_outputs();
        match self.input_sender.try_send(input.to_vec()) {
            Ok(()) => self.metrics.record_input_enqueued(),
            Err(TrySendError::Full(_)) => self.metrics.record_dropped_input(),
            Err(TrySendError::Disconnected(_)) => {
                self.metrics.record_dropped_input();
                self.metrics.mark_degraded();
            }
        }

        if let Some(samples) = newest_output {
            output.copy_from_slice(&samples);
            self.last_output.copy_from_slice(&samples);
            self.has_last_output = true;
            return Ok(());
        }

        self.metrics.record_late_output();
        if self.has_last_output {
            output.copy_from_slice(&self.last_output);
        } else {
            output.copy_from_slice(input);
        }
        Ok(())
    }

    fn reset(&mut self) {
        self.drain_processed_outputs();
        self.last_output.fill(0.0);
        self.has_last_output = false;
    }

    fn diagnostics(&self) -> SuppressorWorkerDiagnostics {
        self.metrics.snapshot()
    }

    fn drain_processed_outputs(&mut self) -> Option<Vec<f32>> {
        let mut newest = None;
        loop {
            match self.output_receiver.try_recv() {
                Ok(samples) => {
                    self.metrics.record_output_dequeued();
                    if samples.len() == self.frame_size_samples {
                        newest = Some(samples);
                    } else {
                        self.metrics.record_dropped_output();
                        self.metrics.mark_degraded();
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.metrics.mark_degraded();
                    break;
                }
            }
        }
        newest
    }
}

#[cfg(feature = "deepfilternet")]
fn deepfilternet_load_params(params_file: PathBuf) -> ClearLineResult<df::tract::DfParams> {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        df::tract::DfParams::new(params_file)
    }));

    match result {
        Ok(Ok(params)) => Ok(params),
        Ok(Err(error)) => Err(ClearLineError::ModelLoad(error.to_string())),
        Err(payload) => Err(ClearLineError::ModelLoad(format!(
            "DeepFilterNet model parameter loader panicked: {}",
            panic_payload_to_string(payload.as_ref())
        ))),
    }
}

#[cfg(feature = "deepfilternet")]
fn deepfilternet_create_model(
    df_params: df::tract::DfParams,
    runtime_params: &df::tract::RuntimeParams,
) -> ClearLineResult<df::tract::DfTract> {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        df::tract::DfTract::new(df_params, runtime_params)
    }));

    match result {
        Ok(Ok(model)) => Ok(model),
        Ok(Err(error)) => Err(ClearLineError::ModelLoad(error.to_string())),
        Err(payload) => Err(ClearLineError::ModelLoad(format!(
            "DeepFilterNet model creation panicked: {}",
            panic_payload_to_string(payload.as_ref())
        ))),
    }
}

#[cfg(feature = "deepfilternet")]
fn panic_payload_to_string(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<&'static str>() {
        return (*message).to_owned();
    }
    if let Some(message) = payload.downcast_ref::<String>() {
        return message.clone();
    }
    "unknown panic payload".to_owned()
}

#[cfg(feature = "deepfilternet")]
fn deepfilternet_runtime_params(
    channels: usize,
    strength: SuppressionStrength,
) -> df::tract::RuntimeParams {
    let atten_lim_db = match strength {
        SuppressionStrength::Gentle => 12.0,
        SuppressionStrength::Balanced => 24.0,
        SuppressionStrength::Strong => 50.0,
    };

    df::tract::RuntimeParams::default_with_ch(channels).with_atten_lim(atten_lim_db)
}

#[cfg(feature = "deepfilternet")]
fn deepfilternet_bundle_as_targz_bytes(
    model_bundle: &DeepFilterNetModelBundle,
) -> ClearLineResult<Vec<u8>> {
    let encoder = GzEncoder::new(Vec::new(), Compression::fast());
    let mut archive = tar::Builder::new(encoder);
    append_model_asset(
        &mut archive,
        model_bundle.encoder_onnx(),
        DeepFilterNetModelBundle::ENCODER_FILE,
    )?;
    append_model_asset(
        &mut archive,
        model_bundle.erb_decoder_onnx(),
        DeepFilterNetModelBundle::ERB_DECODER_FILE,
    )?;
    append_model_asset(
        &mut archive,
        model_bundle.df_decoder_onnx(),
        DeepFilterNetModelBundle::DF_DECODER_FILE,
    )?;
    append_model_asset(
        &mut archive,
        model_bundle.config_ini(),
        DeepFilterNetModelBundle::CONFIG_FILE,
    )?;

    let encoder = archive
        .into_inner()
        .map_err(|error| ClearLineError::ModelLoad(error.to_string()))?;
    encoder
        .finish()
        .map_err(|error| ClearLineError::ModelLoad(error.to_string()))
}

#[cfg(feature = "deepfilternet")]
fn deepfilternet_bundle_as_temp_targz(
    model_bundle: &DeepFilterNetModelBundle,
) -> ClearLineResult<PathBuf> {
    let bytes = deepfilternet_bundle_as_targz_bytes(model_bundle)?;
    let mut path = std::env::temp_dir();
    path.push(format!(
        "clearline-deepfilternet-{}-{}.tar.gz",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|error| ClearLineError::ModelLoad(error.to_string()))?
            .as_nanos()
    ));
    fs::write(&path, bytes).map_err(|error| ClearLineError::ModelLoad(error.to_string()))?;
    Ok(path)
}

#[cfg(feature = "deepfilternet")]
fn append_model_asset<W: io::Write>(
    archive: &mut tar::Builder<W>,
    source: &Path,
    archive_name: &str,
) -> ClearLineResult<()> {
    let mut file =
        fs::File::open(source).map_err(|error| ClearLineError::ModelLoad(error.to_string()))?;
    archive
        .append_file(archive_name, &mut file)
        .map_err(|error| ClearLineError::ModelLoad(error.to_string()))
}

#[derive(Debug, Clone)]
struct AdaptiveHighQualityBackend {
    channels: usize,
    strength: SuppressionStrength,
    params: AdaptiveHighQualityParams,
    noise_floor: f32,
    gain: f32,
    initialized: bool,
}

impl AdaptiveHighQualityBackend {
    const MIN_NOISE_FLOOR: f32 = 0.000_1;

    fn new(channels: u16, strength: SuppressionStrength) -> Self {
        let params = AdaptiveHighQualityParams::for_strength(strength);
        Self {
            channels: usize::from(channels.max(1)),
            strength,
            params,
            noise_floor: params.initial_noise_floor_cap,
            gain: 1.0,
            initialized: false,
        }
    }

    fn name(&self) -> &'static str {
        "adaptive-quality-v1"
    }

    fn is_real_noise_suppression(&self) -> bool {
        true
    }

    fn process_frame(&mut self, input: &[f32], output: &mut [f32]) -> ClearLineResult<()> {
        if input.len() != output.len() {
            return Err(ClearLineError::BufferSizeMismatch {
                input: input.len(),
                output: output.len(),
            });
        }
        if input.len() % self.channels != 0 {
            return Err(ClearLineError::BufferSizeMismatch {
                input: input.len(),
                output: input.len() + (self.channels - input.len() % self.channels),
            });
        }

        let level = interleaved_rms(input, self.channels).max(Self::MIN_NOISE_FLOOR);
        let was_initialized = self.initialized;
        self.update_noise_floor(level);
        let target_gain = self.target_gain(level);
        if was_initialized {
            let smoothing = if target_gain > self.gain { 0.85 } else { 0.22 };
            self.gain += (target_gain - self.gain) * smoothing;
        } else {
            self.gain = target_gain;
        }
        let gain = self.gain;

        for (input, output) in input.iter().copied().zip(output.iter_mut()) {
            *output = (input * gain).clamp(-1.0, 1.0);
        }

        Ok(())
    }

    fn update_noise_floor(&mut self, level: f32) {
        if !self.initialized {
            self.noise_floor = level
                .min(self.params.initial_noise_floor_cap)
                .max(Self::MIN_NOISE_FLOOR);
            self.initialized = true;
            return;
        }

        let alpha = if level < self.noise_floor {
            0.25
        } else if level < self.noise_floor * 1.5 {
            0.08
        } else {
            0.005
        };
        self.noise_floor =
            (self.noise_floor + (level - self.noise_floor) * alpha).max(Self::MIN_NOISE_FLOOR);
    }

    fn target_gain(&self, level: f32) -> f32 {
        let snr = level / self.noise_floor.max(Self::MIN_NOISE_FLOOR);
        let speech_presence = smoothstep(
            self.params.speech_presence_start,
            self.params.speech_presence_full,
            snr,
        );
        self.params.min_gain + (1.0 - self.params.min_gain) * speech_presence
    }

    fn reset(&mut self) {
        *self = Self::new(self.channels as u16, self.strength);
    }
}

#[derive(Debug, Clone, Copy)]
struct AdaptiveHighQualityParams {
    min_gain: f32,
    initial_noise_floor_cap: f32,
    speech_presence_start: f32,
    speech_presence_full: f32,
}

impl AdaptiveHighQualityParams {
    fn for_strength(strength: SuppressionStrength) -> Self {
        match strength {
            SuppressionStrength::Gentle => Self {
                min_gain: 0.35,
                initial_noise_floor_cap: 0.025,
                speech_presence_start: 1.15,
                speech_presence_full: 2.4,
            },
            SuppressionStrength::Balanced => Self {
                min_gain: 0.18,
                initial_noise_floor_cap: 0.03,
                speech_presence_start: 1.35,
                speech_presence_full: 3.5,
            },
            SuppressionStrength::Strong => Self {
                min_gain: 0.08,
                initial_noise_floor_cap: 0.035,
                speech_presence_start: 1.8,
                speech_presence_full: 4.5,
            },
        }
    }
}

pub fn create_suppressor(
    mode: SuppressorMode,
    format: AudioFrameFormat,
    strength: SuppressionStrength,
) -> Box<dyn NoiseSuppressor> {
    create_suppressor_with_deepfilternet_bundle(mode, format, strength, None)
}

pub fn create_suppressor_with_deepfilternet_bundle(
    mode: SuppressorMode,
    format: AudioFrameFormat,
    strength: SuppressionStrength,
    deepfilternet_model_bundle: Option<DeepFilterNetModelBundle>,
) -> Box<dyn NoiseSuppressor> {
    match mode {
        SuppressorMode::Bypass => Box::new(BypassSuppressor::new(format)),
        SuppressorMode::LowLatency => {
            Box::new(LowLatencySuppressor::new_with_strength(format, strength))
        }
        SuppressorMode::HighQuality => {
            #[cfg(feature = "deepfilternet")]
            {
                if let Some(model_bundle) = deepfilternet_model_bundle {
                    match HighQualitySuppressor::try_new_with_deepfilternet_bundle(
                        format,
                        strength,
                        model_bundle,
                    ) {
                        Ok(suppressor) => return Box::new(suppressor),
                        Err(error) => eprintln!(
                            "ClearLine DeepFilterNet load failed, falling back to RNNoise: {error}"
                        ),
                    }
                }
            }
            #[cfg(not(feature = "deepfilternet"))]
            let _ = deepfilternet_model_bundle;

            Box::new(LowLatencySuppressor::new_with_strength(format, strength))
        }
    }
}

pub fn try_create_suppressor_with_deepfilternet_bundle(
    mode: SuppressorMode,
    format: AudioFrameFormat,
    strength: SuppressionStrength,
    deepfilternet_model_bundle: Option<DeepFilterNetModelBundle>,
) -> ClearLineResult<Box<dyn NoiseSuppressor>> {
    match mode {
        SuppressorMode::Bypass => Ok(Box::new(BypassSuppressor::new(format))),
        SuppressorMode::LowLatency => Ok(Box::new(LowLatencySuppressor::new_with_strength(
            format, strength,
        ))),
        SuppressorMode::HighQuality => {
            #[cfg(feature = "deepfilternet")]
            {
                let Some(model_bundle) = deepfilternet_model_bundle else {
                    return Err(ClearLineError::ModelLoad(
                        "DeepFilterNet model bundle is required for high quality mode".to_owned(),
                    ));
                };

                HighQualitySuppressor::try_new_with_deepfilternet_bundle(
                    format,
                    strength,
                    model_bundle,
                )
                .map(|suppressor| Box::new(suppressor) as Box<dyn NoiseSuppressor>)
            }

            #[cfg(not(feature = "deepfilternet"))]
            {
                let _ = deepfilternet_model_bundle;
                Err(ClearLineError::ModelLoad(
                    "ClearLine was built without DeepFilterNet support".to_owned(),
                ))
            }
        }
    }
}

fn copy_samples(input: &[f32], output: &mut [f32]) -> ClearLineResult<()> {
    if input.len() != output.len() {
        return Err(ClearLineError::BufferSizeMismatch {
            input: input.len(),
            output: output.len(),
        });
    }

    output.copy_from_slice(input);
    Ok(())
}

fn low_latency_frame_size_samples(format: AudioFrameFormat) -> usize {
    let samples_per_channel = (format.sample_rate_hz() / 100).max(1) as usize;
    samples_per_channel * usize::from(format.channels().max(1))
}

fn high_quality_frame_size_samples(format: AudioFrameFormat) -> usize {
    let samples_per_channel = (format.sample_rate_hz() / 50).max(1) as usize;
    samples_per_channel * usize::from(format.channels().max(1))
}

fn interleaved_rms(input: &[f32], _channels: usize) -> f32 {
    if input.is_empty() {
        return 0.0;
    }

    let sum_squares = input.iter().map(|sample| sample * sample).sum::<f32>();
    (sum_squares / input.len() as f32).sqrt()
}

fn smoothstep(edge0: f32, edge1: f32, value: f32) -> f32 {
    if edge0 >= edge1 {
        return if value >= edge1 { 1.0 } else { 0.0 };
    }

    let t = ((value - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ClearLineError;
    use std::fs;

    #[test]
    fn runtime_info_can_attach_worker_diagnostics() {
        let diagnostics = SuppressorWorkerDiagnostics::new(3, 1, 3, 2)
            .with_dropped_input_frames(4)
            .with_dropped_output_frames(5)
            .with_late_output_frames(6)
            .with_inference_errors(7)
            .with_last_inference_time_ms(8)
            .with_max_inference_time_ms(9)
            .with_degraded(true);

        let info = SuppressorRuntimeInfo::new(SuppressorMode::HighQuality, "backend", 480, true)
            .with_worker_diagnostics(diagnostics);

        assert_eq!(info.worker_diagnostics(), Some(diagnostics));
        assert_eq!(info.worker_diagnostics().unwrap().input_queue_capacity(), 3);
        assert_eq!(info.worker_diagnostics().unwrap().pending_input_frames(), 1);
        assert_eq!(
            info.worker_diagnostics().unwrap().output_queue_capacity(),
            3
        );
        assert_eq!(
            info.worker_diagnostics().unwrap().pending_output_frames(),
            2
        );
        assert_eq!(info.worker_diagnostics().unwrap().dropped_input_frames(), 4);
        assert_eq!(
            info.worker_diagnostics().unwrap().dropped_output_frames(),
            5
        );
        assert_eq!(info.worker_diagnostics().unwrap().late_output_frames(), 6);
        assert_eq!(info.worker_diagnostics().unwrap().inference_errors(), 7);
        assert_eq!(
            info.worker_diagnostics().unwrap().last_inference_time_ms(),
            Some(8)
        );
        assert_eq!(
            info.worker_diagnostics().unwrap().max_inference_time_ms(),
            Some(9)
        );
        assert!(info.worker_diagnostics().unwrap().is_degraded());
    }

    #[test]
    fn runtime_info_has_no_worker_diagnostics_by_default() {
        let info = SuppressorRuntimeInfo::new(SuppressorMode::LowLatency, "backend", 480, true);

        assert_eq!(info.worker_diagnostics(), None);
    }

    #[cfg(feature = "deepfilternet")]
    #[test]
    fn deepfilternet_bridge_falls_back_without_processed_output() {
        let (input_sender, _input_receiver) = std::sync::mpsc::sync_channel(1);
        let (_output_sender, output_receiver) = std::sync::mpsc::sync_channel(1);
        let metrics = std::sync::Arc::new(DeepFilterNetWorkerMetrics::new(1, 1));
        let mut bridge =
            DeepFilterNetRealtimeBridge::new(input_sender, output_receiver, metrics, 4);
        let input = [0.1, -0.2, 0.3, -0.4];
        let mut output = [0.0; 4];

        bridge.process_frame(&input, &mut output).unwrap();

        assert_eq!(output, input);
        let diagnostics = bridge.diagnostics();
        assert_eq!(diagnostics.pending_input_frames(), 1);
        assert_eq!(diagnostics.late_output_frames(), 1);
        assert_eq!(diagnostics.dropped_input_frames(), 0);
    }

    #[cfg(feature = "deepfilternet")]
    #[test]
    fn deepfilternet_bridge_drops_input_when_queue_is_full() {
        let (input_sender, _input_receiver) = std::sync::mpsc::sync_channel(1);
        let (_output_sender, output_receiver) = std::sync::mpsc::sync_channel(1);
        let metrics = std::sync::Arc::new(DeepFilterNetWorkerMetrics::new(1, 1));
        let mut bridge =
            DeepFilterNetRealtimeBridge::new(input_sender, output_receiver, metrics, 2);
        let input = [0.25, -0.25];
        let mut output = [0.0; 2];

        bridge.process_frame(&input, &mut output).unwrap();
        bridge.process_frame(&input, &mut output).unwrap();

        let diagnostics = bridge.diagnostics();
        assert_eq!(diagnostics.pending_input_frames(), 1);
        assert_eq!(diagnostics.dropped_input_frames(), 1);
        assert_eq!(diagnostics.late_output_frames(), 2);
    }

    #[cfg(feature = "deepfilternet")]
    #[test]
    fn deepfilternet_bridge_uses_latest_processed_output() {
        let (input_sender, _input_receiver) = std::sync::mpsc::sync_channel(1);
        let (output_sender, output_receiver) = std::sync::mpsc::sync_channel(2);
        let metrics = std::sync::Arc::new(DeepFilterNetWorkerMetrics::new(1, 2));
        let mut bridge =
            DeepFilterNetRealtimeBridge::new(input_sender, output_receiver, metrics.clone(), 2);
        let input = [0.1, 0.2];
        let mut output = [0.0; 2];

        output_sender.try_send(vec![0.7, -0.7]).unwrap();
        metrics.record_output_enqueued();

        bridge.process_frame(&input, &mut output).unwrap();

        assert_eq!(output, [0.7, -0.7]);
        let diagnostics = bridge.diagnostics();
        assert_eq!(diagnostics.pending_output_frames(), 0);
        assert_eq!(diagnostics.late_output_frames(), 0);
    }

    #[test]
    fn bypass_suppressor_copies_samples() {
        let format = AudioFrameFormat::new(48_000, 1);
        let mut suppressor = BypassSuppressor::new(format);
        let input = [0.0, 0.25, -0.5, 1.0];
        let mut output = [0.0; 4];

        suppressor.process(&input, &mut output).unwrap();

        assert_eq!(output, input);
        assert_eq!(suppressor.mode(), SuppressorMode::Bypass);
    }

    #[test]
    fn bypass_suppressor_rejects_mismatched_buffer_lengths() {
        let format = AudioFrameFormat::new(48_000, 1);
        let mut suppressor = BypassSuppressor::new(format);
        let input = [0.0, 0.25];
        let mut output = [0.0; 1];

        let error = suppressor.process(&input, &mut output).unwrap_err();

        assert_eq!(
            error,
            ClearLineError::BufferSizeMismatch {
                input: 2,
                output: 1
            }
        );
    }

    #[test]
    fn bypass_and_low_latency_placeholder_preserve_samples_and_report_modes() {
        let input = [0.1, -0.2, 0.3];

        let mut low_latency = LowLatencySuppressor::new(AudioFrameFormat::new(300, 1));
        let mut low_output = [0.0; 3];
        low_latency.process(&input, &mut low_output).unwrap();
        assert_eq!(low_output, input);
        assert_eq!(low_latency.mode(), SuppressorMode::LowLatency);
    }

    #[test]
    fn low_latency_suppressor_uses_fixed_frames_with_stable_latency() {
        let format = AudioFrameFormat::new(400, 1);
        let mut suppressor = LowLatencySuppressor::new(format);

        assert_eq!(suppressor.frame_size_samples(), 4);

        let mut first_output = [1.0; 2];
        suppressor.process(&[0.1, 0.2], &mut first_output).unwrap();
        assert_eq!(first_output, [0.0, 0.0]);

        let mut second_output = [0.0; 2];
        suppressor.process(&[0.3, 0.4], &mut second_output).unwrap();
        assert_eq!(second_output, [0.1, 0.2]);

        let mut third_output = [0.0; 2];
        suppressor.process(&[0.5, 0.6], &mut third_output).unwrap();
        assert_eq!(third_output, [0.3, 0.4]);
    }

    #[test]
    fn low_latency_suppressor_reset_clears_pending_frame_state() {
        let format = AudioFrameFormat::new(400, 1);
        let mut suppressor = LowLatencySuppressor::new(format);
        let mut output = [0.0; 2];

        suppressor.process(&[0.1, 0.2], &mut output).unwrap();
        suppressor.process(&[0.3, 0.4], &mut output).unwrap();
        assert_eq!(output, [0.1, 0.2]);

        suppressor.reset();
        suppressor.process(&[0.5, 0.6], &mut output).unwrap();

        assert_eq!(output, [0.0, 0.0]);
    }

    #[test]
    fn low_latency_runtime_info_reports_backend_frame_and_effective_status() {
        let suppressor = LowLatencySuppressor::new(AudioFrameFormat::new(400, 1));
        let info = suppressor.runtime_info();

        assert_eq!(info.mode(), SuppressorMode::LowLatency);
        assert_eq!(info.backend_name(), "bypass-placeholder");
        assert_eq!(info.frame_size_samples(), 4);
        assert!(!info.is_real_noise_suppression());
    }

    #[test]
    fn low_latency_create_suppressor_reports_selected_strength() {
        let suppressor = create_suppressor(
            SuppressorMode::LowLatency,
            AudioFrameFormat::new(48_000, 1),
            SuppressionStrength::Strong,
        );

        assert_eq!(
            suppressor.runtime_info().strength(),
            Some(SuppressionStrength::Strong)
        );
    }

    #[cfg(all(feature = "rnnoise", feature = "deepfilternet"))]
    #[test]
    fn create_suppressor_falls_back_when_deepfilternet_model_cannot_load() {
        let model_dir = unique_temp_model_dir("create-deepfilternet-backend");
        fs::create_dir_all(&model_dir).unwrap();
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

        let suppressor = create_suppressor_with_deepfilternet_bundle(
            SuppressorMode::HighQuality,
            AudioFrameFormat::new(48_000, 1),
            SuppressionStrength::Gentle,
            Some(bundle),
        );

        assert_eq!(suppressor.runtime_info().mode(), SuppressorMode::LowLatency);
        assert_eq!(
            suppressor.runtime_info().backend_name(),
            "nnnoiseless-rnnoise"
        );
        assert_eq!(suppressor.runtime_info().frame_size_samples(), 480);
        assert!(suppressor.runtime_info().is_real_noise_suppression());
        assert_eq!(
            suppressor.runtime_info().strength(),
            Some(SuppressionStrength::Gentle)
        );
        let _ = fs::remove_dir_all(&model_dir);
    }

    #[test]
    fn high_quality_create_suppressor_without_model_uses_low_latency_fallback() {
        let suppressor = create_suppressor(
            SuppressorMode::HighQuality,
            AudioFrameFormat::new(48_000, 1),
            SuppressionStrength::Balanced,
        );
        let info = suppressor.runtime_info();

        assert_eq!(info.mode(), SuppressorMode::LowLatency);
        assert_eq!(info.backend_name(), expected_low_latency_backend_name());
        assert_eq!(info.strength(), Some(SuppressionStrength::Balanced));
    }

    #[cfg(feature = "deepfilternet")]
    #[test]
    fn strict_deepfilternet_creation_requires_model_bundle() {
        let error = match try_create_suppressor_with_deepfilternet_bundle(
            SuppressorMode::HighQuality,
            AudioFrameFormat::new(48_000, 1),
            SuppressionStrength::Balanced,
            None,
        ) {
            Ok(_) => panic!("invalid DeepFilterNet bundle should not fall back to RNNoise"),
            Err(error) => error,
        };

        assert!(matches!(error, ClearLineError::ModelLoad(_)));
    }

    #[test]
    fn high_quality_runtime_info_reports_adaptive_backend_with_larger_frame() {
        let suppressor = HighQualitySuppressor::new(AudioFrameFormat::new(48_000, 2));
        let info = suppressor.runtime_info();

        assert_eq!(info.mode(), SuppressorMode::HighQuality);
        assert_eq!(info.backend_name(), "adaptive-quality-v1");
        assert_eq!(info.frame_size_samples(), 1_920);
        assert!(info.is_real_noise_suppression());
    }

    #[test]
    fn deepfilternet_model_bundle_requires_three_onnx_assets() {
        let model_dir = unique_temp_model_dir("missing-assets");
        fs::create_dir_all(&model_dir).unwrap();
        fs::write(model_dir.join(DeepFilterNetModelBundle::ENCODER_FILE), []).unwrap();
        fs::write(
            model_dir.join(DeepFilterNetModelBundle::ERB_DECODER_FILE),
            [],
        )
        .unwrap();

        let error = DeepFilterNetModelBundle::from_dir(&model_dir).unwrap_err();

        assert!(matches!(
            error,
            ClearLineError::ModelAssetMissing { path } if path.ends_with(DeepFilterNetModelBundle::DF_DECODER_FILE)
        ));
        let _ = fs::remove_dir_all(&model_dir);
    }

    #[test]
    fn deepfilternet_model_bundle_requires_config_ini() {
        let model_dir = unique_temp_model_dir("missing-config");
        fs::create_dir_all(&model_dir).unwrap();
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

        let error = DeepFilterNetModelBundle::from_dir(&model_dir).unwrap_err();

        assert!(matches!(
            error,
            ClearLineError::ModelAssetMissing { path } if path.ends_with(DeepFilterNetModelBundle::CONFIG_FILE)
        ));
        let _ = fs::remove_dir_all(&model_dir);
    }

    #[test]
    fn deepfilternet_model_bundle_resolves_asset_paths() {
        let model_dir = unique_temp_model_dir("complete-assets");
        fs::create_dir_all(&model_dir).unwrap();
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

        assert_eq!(bundle.root_dir(), model_dir.as_path());
        assert_eq!(
            bundle.encoder_onnx(),
            model_dir
                .join(DeepFilterNetModelBundle::ENCODER_FILE)
                .as_path()
        );
        assert_eq!(
            bundle.erb_decoder_onnx(),
            model_dir
                .join(DeepFilterNetModelBundle::ERB_DECODER_FILE)
                .as_path()
        );
        assert_eq!(
            bundle.df_decoder_onnx(),
            model_dir
                .join(DeepFilterNetModelBundle::DF_DECODER_FILE)
                .as_path()
        );
        assert_eq!(
            bundle.config_ini(),
            model_dir
                .join(DeepFilterNetModelBundle::CONFIG_FILE)
                .as_path()
        );
        let _ = fs::remove_dir_all(&model_dir);
    }

    #[cfg(feature = "deepfilternet")]
    #[test]
    fn high_quality_falls_back_when_deepfilternet_model_cannot_load() {
        let model_dir = unique_temp_model_dir("deepfilternet-backend");
        fs::create_dir_all(&model_dir).unwrap();
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

        let suppressor = HighQualitySuppressor::new_with_deepfilternet_bundle(
            AudioFrameFormat::new(48_000, 1),
            SuppressionStrength::Strong,
            bundle,
        );
        let info = suppressor.runtime_info();

        assert_eq!(info.backend_name(), "adaptive-quality-v1");
        assert_eq!(info.frame_size_samples(), 960);
        assert_eq!(info.strength(), Some(SuppressionStrength::Strong));
        assert!(info.is_real_noise_suppression());
        let _ = fs::remove_dir_all(&model_dir);
    }

    #[cfg(feature = "deepfilternet")]
    #[test]
    #[ignore = "requires downloaded DeepFilterNet3 ONNX model files"]
    fn high_quality_runs_downloaded_deepfilternet_model() {
        let model_dir = std::env::var("CLEARLINE_DF_MODEL_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| std::path::PathBuf::from("/mnt/e/Dev/模型onnx"));
        let bundle = DeepFilterNetModelBundle::from_dir(&model_dir).unwrap();
        let mut suppressor = HighQualitySuppressor::new_with_deepfilternet_bundle(
            AudioFrameFormat::new(48_000, 1),
            SuppressionStrength::Balanced,
            bundle,
        );
        let info = suppressor.runtime_info();

        assert_eq!(info.backend_name(), "deepfilternet-tract-worker");
        assert_eq!(info.frame_size_samples(), 480);
        assert!(info.is_real_noise_suppression());
        assert_eq!(
            info.worker_diagnostics().unwrap().last_inference_time_ms(),
            None
        );

        let input = vec![0.01; info.frame_size_samples()];
        let mut output = vec![0.0; info.frame_size_samples()];
        suppressor.process(&input, &mut output).unwrap();
        let diagnostics = wait_for_deepfilternet_inference(&suppressor);
        suppressor.process(&input, &mut output).unwrap();

        assert!(output.iter().all(|sample| sample.is_finite()));
        assert_eq!(output.len(), input.len());
        assert!(diagnostics.last_inference_time_ms().is_some());
    }

    #[test]
    fn high_quality_suppressor_attenuates_noise_floor_and_preserves_speech() {
        let mut suppressor = HighQualitySuppressor::new(AudioFrameFormat::new(1_000, 1));
        let noise = vec![0.02; 20];
        let mut noise_output = vec![0.0; 20];

        suppressor.process(&noise, &mut noise_output).unwrap();

        let noise_average =
            noise_output.iter().map(|sample| sample.abs()).sum::<f32>() / noise_output.len() as f32;
        assert!(
            noise_average < 0.01,
            "expected stationary noise to be attenuated, got average {noise_average}"
        );

        let speech = vec![0.5; 20];
        let mut speech_output = vec![0.0; 20];

        suppressor.process(&speech, &mut speech_output).unwrap();

        let speech_average = speech_output.iter().map(|sample| sample.abs()).sum::<f32>()
            / speech_output.len() as f32;
        assert!(
            speech_average > 0.35,
            "expected speech-like input to be preserved, got average {speech_average}"
        );
    }

    #[test]
    fn high_quality_strength_changes_noise_attenuation() {
        let noise = vec![0.02; 20];
        let mut gentle = HighQualitySuppressor::new_with_strength(
            AudioFrameFormat::new(1_000, 1),
            SuppressionStrength::Gentle,
        );
        let mut strong = HighQualitySuppressor::new_with_strength(
            AudioFrameFormat::new(1_000, 1),
            SuppressionStrength::Strong,
        );
        let mut gentle_output = vec![0.0; 20];
        let mut strong_output = vec![0.0; 20];

        gentle.process(&noise, &mut gentle_output).unwrap();
        strong.process(&noise, &mut strong_output).unwrap();

        let average_abs = |samples: &[f32]| {
            samples.iter().map(|sample| sample.abs()).sum::<f32>() / samples.len() as f32
        };
        assert!(
            average_abs(&strong_output) < average_abs(&gentle_output),
            "strong strength should attenuate stable noise more than gentle strength"
        );
        assert_eq!(
            strong.runtime_info().strength(),
            Some(SuppressionStrength::Strong)
        );
    }

    #[cfg(feature = "deepfilternet")]
    #[test]
    fn deepfilternet_strength_uses_conservative_attenuation_limits() {
        assert_eq!(
            deepfilternet_runtime_params(1, SuppressionStrength::Gentle).atten_lim_db,
            12.0
        );
        assert_eq!(
            deepfilternet_runtime_params(1, SuppressionStrength::Balanced).atten_lim_db,
            24.0
        );
        assert_eq!(
            deepfilternet_runtime_params(1, SuppressionStrength::Strong).atten_lim_db,
            50.0
        );
    }

    #[cfg(feature = "rnnoise")]
    #[test]
    fn rnnoise_backend_is_selected_for_native_48k_mono_frames() {
        let suppressor = LowLatencySuppressor::new(AudioFrameFormat::new(48_000, 1));

        assert_eq!(suppressor.frame_size_samples(), 480);
        assert_eq!(suppressor.backend_name(), "nnnoiseless-rnnoise");
    }

    #[cfg(feature = "rnnoise")]
    #[test]
    fn rnnoise_runtime_info_marks_real_backend() {
        let suppressor = LowLatencySuppressor::new(AudioFrameFormat::new(48_000, 1));
        let info = suppressor.runtime_info();

        assert_eq!(info.backend_name(), "nnnoiseless-rnnoise");
        assert_eq!(info.frame_size_samples(), 480);
        assert!(info.is_real_noise_suppression());
    }

    #[cfg(feature = "rnnoise")]
    #[test]
    fn rnnoise_backend_is_selected_for_48k_stereo_by_downmixing_to_mono() {
        let suppressor = LowLatencySuppressor::new(AudioFrameFormat::new(48_000, 2));

        assert_eq!(suppressor.frame_size_samples(), 960);
        assert_eq!(suppressor.backend_name(), "nnnoiseless-rnnoise");
        assert!(suppressor.runtime_info().is_real_noise_suppression());
    }

    #[cfg(feature = "rnnoise")]
    #[test]
    fn rnnoise_backend_falls_back_when_sample_rate_is_not_native() {
        let suppressor = LowLatencySuppressor::new(AudioFrameFormat::new(44_100, 2));

        assert_eq!(suppressor.frame_size_samples(), 882);
        assert_eq!(suppressor.backend_name(), "bypass-placeholder");
    }

    #[cfg(feature = "rnnoise")]
    #[test]
    fn rnnoise_stereo_backend_accepts_10ms_interleaved_frames() {
        let mut suppressor = LowLatencySuppressor::new(AudioFrameFormat::new(48_000, 2));
        let input = [0.0; 960];
        let mut output = [1.0; 960];

        suppressor.process(&input, &mut output).unwrap();

        assert!(output.iter().all(|sample| *sample == 0.0));
    }

    #[cfg(feature = "rnnoise")]
    #[test]
    fn rnnoise_backend_outputs_silence_during_warmup_and_after_reset() {
        let mut suppressor = LowLatencySuppressor::new(AudioFrameFormat::new(48_000, 1));
        let input = [0.0; 480];
        let mut output = [1.0; 480];

        suppressor.process(&input, &mut output).unwrap();
        assert!(output.iter().all(|sample| *sample == 0.0));

        output.fill(1.0);
        suppressor.process(&input, &mut output).unwrap();
        assert!(output.iter().all(|sample| sample.abs() < 0.000_001));

        suppressor.reset();
        output.fill(1.0);
        suppressor.process(&input, &mut output).unwrap();
        assert!(output.iter().all(|sample| *sample == 0.0));
    }

    #[cfg(feature = "rnnoise")]
    #[test]
    fn rnnoise_strength_changes_dry_mix_and_non_speech_gate() {
        assert!(rnnoise_dry_mix_for_strength(SuppressionStrength::Gentle) > 0.0);
        assert_eq!(
            rnnoise_dry_mix_for_strength(SuppressionStrength::Balanced),
            0.0
        );
        assert_eq!(
            rnnoise_dry_mix_for_strength(SuppressionStrength::Strong),
            0.0
        );

        assert_eq!(
            rnnoise_non_speech_gain_for_strength(SuppressionStrength::Gentle, 0.0),
            1.0
        );
        assert_eq!(
            rnnoise_non_speech_gain_for_strength(SuppressionStrength::Balanced, 0.0),
            1.0
        );
        assert!(
            rnnoise_non_speech_gain_for_strength(SuppressionStrength::Strong, 0.1)
                < rnnoise_non_speech_gain_for_strength(SuppressionStrength::Strong, 0.9),
            "strong strength should attenuate low-VAD frames more than likely speech frames"
        );
    }

    fn unique_temp_model_dir(name: &str) -> std::path::PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "clearline-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        path
    }

    fn expected_low_latency_backend_name() -> &'static str {
        #[cfg(feature = "rnnoise")]
        {
            "nnnoiseless-rnnoise"
        }
        #[cfg(not(feature = "rnnoise"))]
        {
            "bypass-placeholder"
        }
    }

    #[cfg(feature = "deepfilternet")]
    fn wait_for_deepfilternet_inference(
        suppressor: &HighQualitySuppressor,
    ) -> SuppressorWorkerDiagnostics {
        let timeout = std::time::Duration::from_secs(5);
        let poll_interval = std::time::Duration::from_millis(10);
        let started = std::time::Instant::now();

        loop {
            let diagnostics = suppressor.runtime_info().worker_diagnostics().unwrap();
            if diagnostics.last_inference_time_ms().is_some() {
                return diagnostics;
            }
            if started.elapsed() >= timeout {
                panic!("timed out waiting for DeepFilterNet inference: {diagnostics:?}");
            }

            std::thread::sleep(poll_interval);
        }
    }
}
