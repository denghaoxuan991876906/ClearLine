pub mod device;
pub mod echo;
pub mod frame;
pub mod pipeline;
pub mod preprocess;
pub mod reference;
pub mod suppressor;
pub mod virtual_mic;

pub use device::{
    AudioInputDevice, AudioOutputDevice, CpalDeviceEnumerator, DeviceEnumerator, DeviceId,
    InputDeviceSelector, OutputDeviceSelector,
};
pub use echo::{
    run_echo_canceller_on_fixture, EchoCanceller, EchoCancellerBackend, EchoCancellerRuntimeInfo,
    EchoReductionMetrics, GeneratedEchoFixture, NoopEchoCanceller, RealtimeAecProbeReport,
};
#[cfg(feature = "aec")]
pub use echo::{Aec3EchoCanceller, Aec3EchoWorker};
pub use frame::{FrameChunker, FrameChunkerError};
pub use pipeline::{
    AudioOutputTarget, AudioPipeline, AudioPipelineConfig, EchoReferenceDiagnostics, LevelMeter,
    PipelineMetrics, PipelineRuntimeInfo, PipelineState,
};
pub use preprocess::{WindNoiseConfig, WindNoiseReducer};
#[cfg(windows)]
pub use reference::{LoopbackReferenceCapture, SharedReferenceFrameBuffer};
pub use reference::{ReferenceCaptureStats, ReferenceFrameBuffer};
pub use suppressor::{
    create_suppressor, create_suppressor_with_deepfilternet_bundle,
    try_create_suppressor_with_deepfilternet_bundle, AudioFrameFormat, BypassSuppressor,
    DeepFilterNetModelBundle, HighQualitySuppressor, LowLatencySuppressor, NoiseSuppressor,
    SuppressionStrength, SuppressorMode, SuppressorRuntimeInfo, SuppressorWorkerDiagnostics,
};
pub use virtual_mic::{ClearLineBufferStatus, ClearLinePingResponse, VirtualMicControl};

use thiserror::Error;

pub type ClearLineResult<T> = Result<T, ClearLineError>;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ClearLineError {
    #[error("failed to enumerate audio input devices: {0}")]
    DeviceEnumeration(String),

    #[error("audio device not found: {0}")]
    DeviceNotFound(String),

    #[error("failed to build audio stream: {0}")]
    StreamBuild(String),

    #[error("failed to start audio stream: {0}")]
    StreamPlay(String),

    #[error("unsupported audio sample format: {0}")]
    UnsupportedSampleFormat(String),

    #[error("input and output buffers have different lengths: input {input}, output {output}")]
    BufferSizeMismatch { input: usize, output: usize },

    #[error("required model asset is missing: {path}")]
    ModelAssetMissing { path: String },

    #[error("failed to load model: {0}")]
    ModelLoad(String),

    #[error("model inference failed: {0}")]
    ModelInference(String),

    #[error("audio pipeline is already running")]
    PipelineAlreadyRunning,

    #[error("echo cancellation failed: {0}")]
    EchoCancellation(String),

    #[error("virtual microphone control failed: {0}")]
    VirtualMicControl(String),
}
