use crate::{AudioFrameFormat, ClearLineError, ClearLineResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EchoCancellerBackend {
    Disabled,
    Aec3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EchoCancellerRuntimeInfo {
    backend: EchoCancellerBackend,
    format: AudioFrameFormat,
}

impl EchoCancellerRuntimeInfo {
    pub fn new(backend: EchoCancellerBackend, format: AudioFrameFormat) -> Self {
        Self { backend, format }
    }

    pub fn backend(&self) -> EchoCancellerBackend {
        self.backend
    }

    pub fn format(&self) -> AudioFrameFormat {
        self.format
    }
}

pub trait EchoCanceller {
    fn process(
        &mut self,
        capture: &[f32],
        render: &[f32],
        output: &mut [f32],
    ) -> ClearLineResult<()>;

    fn runtime_info(&self) -> EchoCancellerRuntimeInfo;
}

#[derive(Debug, Clone)]
pub struct NoopEchoCanceller {
    format: AudioFrameFormat,
}

impl NoopEchoCanceller {
    pub fn new(format: AudioFrameFormat) -> Self {
        Self { format }
    }
}

impl EchoCanceller for NoopEchoCanceller {
    fn process(
        &mut self,
        capture: &[f32],
        _render: &[f32],
        output: &mut [f32],
    ) -> ClearLineResult<()> {
        if capture.len() != output.len() {
            return Err(ClearLineError::BufferSizeMismatch {
                input: capture.len(),
                output: output.len(),
            });
        }

        output.copy_from_slice(capture);
        Ok(())
    }

    fn runtime_info(&self) -> EchoCancellerRuntimeInfo {
        EchoCancellerRuntimeInfo::new(EchoCancellerBackend::Disabled, self.format)
    }
}

#[derive(Debug, Clone)]
pub struct GeneratedEchoFixture {
    render: Vec<f32>,
    capture: Vec<f32>,
    output: Vec<f32>,
    sample_rate_hz: u32,
}

impl GeneratedEchoFixture {
    pub fn new(sample_rate_hz: u32, frames_10ms: usize) -> Self {
        let frame_size = (sample_rate_hz / 100).max(1) as usize;
        let samples = frame_size * frames_10ms.max(1);
        let delay_samples = (sample_rate_hz as f32 * 0.015).round() as usize;
        let mut render = Vec::with_capacity(samples);
        let mut capture = Vec::with_capacity(samples);
        let mut seed = 0x1234_5678_u32;

        for index in 0..samples {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let noise = ((seed >> 8) as f32 / 16_777_216.0) * 2.0 - 1.0;
            let t = index as f32 / sample_rate_hz as f32;
            let render_sample = 0.35 * (2.0 * std::f32::consts::PI * 437.0 * t).sin()
                + 0.20 * (2.0 * std::f32::consts::PI * 911.0 * t).sin()
                + 0.08 * noise;
            render.push(render_sample.clamp(-0.95, 0.95));

            let echo = index
                .checked_sub(delay_samples)
                .and_then(|source_index| render.get(source_index).copied())
                .unwrap_or(0.0)
                * 0.58;
            let nearend = 0.03 * (2.0 * std::f32::consts::PI * 223.0 * t).sin();
            capture.push((echo + nearend).clamp(-0.95, 0.95));
        }

        Self {
            output: vec![0.0; samples],
            render,
            capture,
            sample_rate_hz,
        }
    }

    pub fn render(&self) -> &[f32] {
        &self.render
    }

    pub fn capture(&self) -> &[f32] {
        &self.capture
    }

    pub fn output(&self) -> &[f32] {
        &self.output
    }

    pub fn output_mut(&mut self) -> &mut [f32] {
        &mut self.output
    }

    pub fn sample_rate_hz(&self) -> u32 {
        self.sample_rate_hz
    }

    pub fn frame_size_samples(&self) -> usize {
        (self.sample_rate_hz / 100).max(1) as usize
    }
}

pub fn run_echo_canceller_on_fixture(
    canceller: &mut dyn EchoCanceller,
    fixture: &GeneratedEchoFixture,
) -> ClearLineResult<EchoReductionMetrics> {
    let frame_size = fixture.frame_size_samples();
    let mut output = vec![0.0; fixture.render().len().min(fixture.capture().len())];

    for ((render, capture), output_frame) in fixture
        .render()
        .chunks_exact(frame_size)
        .zip(fixture.capture().chunks_exact(frame_size))
        .zip(output.chunks_exact_mut(frame_size))
    {
        canceller.process(capture, render, output_frame)?;
    }

    Ok(EchoReductionMetrics::from_signals(
        fixture.render(),
        fixture.capture(),
        &output,
    ))
}

#[cfg(feature = "aec")]
pub struct Aec3EchoCanceller {
    format: AudioFrameFormat,
    frame_size_samples: usize,
    pipeline: aec3::pipelines::linear::LinearPipeline,
}

#[cfg(feature = "aec")]
impl Aec3EchoCanceller {
    pub fn new(format: AudioFrameFormat) -> ClearLineResult<Self> {
        let frame_size_samples = ten_ms_frame_size(format);
        let aec_format =
            aec3::nodes::audio::AudioFormat::ten_ms(format.sample_rate_hz(), format.channels());
        let pipeline = aec3::pipelines::linear::builder(aec_format, aec_format)
            .initial_delay_ms(15)
            .enable_high_pass_filter(false)
            .enable_noise_suppression(false)
            .enable_gain_controller2(false)
            .build()
            .map_err(|error| ClearLineError::EchoCancellation(error.to_string()))?;

        Ok(Self {
            format,
            frame_size_samples,
            pipeline,
        })
    }
}

#[cfg(feature = "aec")]
impl std::fmt::Debug for Aec3EchoCanceller {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Aec3EchoCanceller")
            .field("format", &self.format)
            .field("frame_size_samples", &self.frame_size_samples)
            .finish_non_exhaustive()
    }
}

#[cfg(feature = "aec")]
enum Aec3WorkerCommand {
    Process {
        capture: Vec<f32>,
        render: Vec<f32>,
        respond_to: std::sync::mpsc::SyncSender<ClearLineResult<Vec<f32>>>,
    },
    Shutdown,
}

#[cfg(feature = "aec")]
pub struct Aec3EchoWorker {
    format: AudioFrameFormat,
    sender: std::sync::mpsc::SyncSender<Aec3WorkerCommand>,
    thread: Option<std::thread::JoinHandle<()>>,
}

#[cfg(feature = "aec")]
impl Aec3EchoWorker {
    pub fn new(format: AudioFrameFormat) -> ClearLineResult<Self> {
        let (command_sender, command_receiver) = std::sync::mpsc::sync_channel(8);
        let (startup_sender, startup_receiver) = std::sync::mpsc::sync_channel(1);
        let thread = std::thread::Builder::new()
            .name("clearline-aec3-worker".to_owned())
            .spawn(move || {
                let mut canceller = match Aec3EchoCanceller::new(format) {
                    Ok(canceller) => {
                        let _ = startup_sender.send(Ok(()));
                        canceller
                    }
                    Err(error) => {
                        let _ = startup_sender.send(Err(error));
                        return;
                    }
                };

                while let Ok(command) = command_receiver.recv() {
                    match command {
                        Aec3WorkerCommand::Process {
                            capture,
                            render,
                            respond_to,
                        } => {
                            let mut output = vec![0.0; capture.len()];
                            let result = canceller
                                .process(&capture, &render, &mut output)
                                .map(|()| output);
                            let _ = respond_to.send(result);
                        }
                        Aec3WorkerCommand::Shutdown => break,
                    }
                }
            })
            .map_err(|error| ClearLineError::EchoCancellation(error.to_string()))?;

        startup_receiver
            .recv()
            .map_err(|error| ClearLineError::EchoCancellation(error.to_string()))??;

        Ok(Self {
            format,
            sender: command_sender,
            thread: Some(thread),
        })
    }
}

#[cfg(feature = "aec")]
impl std::fmt::Debug for Aec3EchoWorker {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Aec3EchoWorker")
            .field("format", &self.format)
            .finish_non_exhaustive()
    }
}

#[cfg(feature = "aec")]
impl Drop for Aec3EchoWorker {
    fn drop(&mut self) {
        let _ = self.sender.send(Aec3WorkerCommand::Shutdown);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(feature = "aec")]
impl EchoCanceller for Aec3EchoWorker {
    fn process(
        &mut self,
        capture: &[f32],
        render: &[f32],
        output: &mut [f32],
    ) -> ClearLineResult<()> {
        if capture.len() != output.len() {
            return Err(ClearLineError::BufferSizeMismatch {
                input: capture.len(),
                output: output.len(),
            });
        }

        let (response_sender, response_receiver) = std::sync::mpsc::sync_channel(1);
        self.sender
            .send(Aec3WorkerCommand::Process {
                capture: capture.to_vec(),
                render: render.to_vec(),
                respond_to: response_sender,
            })
            .map_err(|error| ClearLineError::EchoCancellation(error.to_string()))?;
        let processed = response_receiver
            .recv()
            .map_err(|error| ClearLineError::EchoCancellation(error.to_string()))??;
        if processed.len() != output.len() {
            return Err(ClearLineError::EchoCancellation(format!(
                "AEC worker returned {} samples for {} sample output",
                processed.len(),
                output.len()
            )));
        }
        output.copy_from_slice(&processed);
        Ok(())
    }

    fn runtime_info(&self) -> EchoCancellerRuntimeInfo {
        EchoCancellerRuntimeInfo::new(EchoCancellerBackend::Aec3, self.format)
    }
}

#[cfg(feature = "aec")]
impl EchoCanceller for Aec3EchoCanceller {
    fn process(
        &mut self,
        capture: &[f32],
        render: &[f32],
        output: &mut [f32],
    ) -> ClearLineResult<()> {
        if capture.len() != output.len() {
            return Err(ClearLineError::BufferSizeMismatch {
                input: capture.len(),
                output: output.len(),
            });
        }
        if capture.len() != self.frame_size_samples || render.len() != self.frame_size_samples {
            return Err(ClearLineError::EchoCancellation(format!(
                "AEC3 expects 10 ms frames of {} samples, got capture {} and render {}",
                self.frame_size_samples,
                capture.len(),
                render.len()
            )));
        }

        self.pipeline
            .handle_render_frame(render)
            .map_err(|error| ClearLineError::EchoCancellation(error.to_string()))?;
        let produced = self
            .pipeline
            .process_capture_frame(capture, output)
            .map_err(|error| ClearLineError::EchoCancellation(error.to_string()))?;
        if !produced {
            output.copy_from_slice(capture);
        }
        Ok(())
    }

    fn runtime_info(&self) -> EchoCancellerRuntimeInfo {
        EchoCancellerRuntimeInfo::new(EchoCancellerBackend::Aec3, self.format)
    }
}

#[cfg(feature = "aec")]
fn ten_ms_frame_size(format: AudioFrameFormat) -> usize {
    (format.sample_rate_hz() / 100).max(1) as usize * usize::from(format.channels().max(1))
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EchoReductionMetrics {
    input_echo_correlation: f32,
    output_echo_correlation: f32,
    echo_power_reduction_db: f32,
}

impl EchoReductionMetrics {
    pub fn from_signals(render: &[f32], capture: &[f32], output: &[f32]) -> Self {
        let len = render.len().min(capture.len()).min(output.len());
        let render = &render[..len];
        let capture = &capture[..len];
        let output = &output[..len];
        let input_echo_correlation = absolute_correlation(render, capture);
        let output_echo_correlation = absolute_correlation(render, output);
        let input_power = mean_square(capture);
        let output_power = mean_square(output);
        let echo_power_reduction_db = power_reduction_db(input_power, output_power);

        Self {
            input_echo_correlation,
            output_echo_correlation,
            echo_power_reduction_db,
        }
    }

    pub fn input_echo_correlation(&self) -> f32 {
        self.input_echo_correlation
    }

    pub fn output_echo_correlation(&self) -> f32 {
        self.output_echo_correlation
    }

    pub fn echo_power_reduction_db(&self) -> f32 {
        self.echo_power_reduction_db
    }

    pub fn passes_reduction_thresholds(
        &self,
        max_output_to_input_correlation_ratio: f32,
        min_power_reduction_db: f32,
    ) -> bool {
        let max_output_correlation =
            self.input_echo_correlation * max_output_to_input_correlation_ratio;
        self.output_echo_correlation < max_output_correlation
            && self.echo_power_reduction_db >= min_power_reduction_db
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RealtimeAecProbeReport {
    backend: EchoCancellerBackend,
    processed_frames: u64,
    capture_level: f32,
    reference_level: f32,
    reference_missing_frames: u64,
}

impl RealtimeAecProbeReport {
    pub fn new(backend: EchoCancellerBackend) -> Self {
        Self {
            backend,
            processed_frames: 0,
            capture_level: 0.0,
            reference_level: 0.0,
            reference_missing_frames: 0,
        }
    }

    pub fn with_processed_frames(mut self, processed_frames: u64) -> Self {
        self.processed_frames = processed_frames;
        self
    }

    pub fn with_capture_level(mut self, capture_level: f32) -> Self {
        self.capture_level = capture_level.clamp(0.0, 1.0);
        self
    }

    pub fn with_reference_level(mut self, reference_level: f32) -> Self {
        self.reference_level = reference_level.clamp(0.0, 1.0);
        self
    }

    pub fn with_missing_reference_frames(mut self, reference_missing_frames: u64) -> Self {
        self.reference_missing_frames = reference_missing_frames;
        self
    }

    pub fn backend(&self) -> EchoCancellerBackend {
        self.backend
    }

    pub fn processed_frames(&self) -> u64 {
        self.processed_frames
    }

    pub fn capture_level(&self) -> f32 {
        self.capture_level
    }

    pub fn reference_level(&self) -> f32 {
        self.reference_level
    }

    pub fn reference_missing_frames(&self) -> u64 {
        self.reference_missing_frames
    }

    pub fn has_processed_audio(&self) -> bool {
        self.processed_frames > 0
    }

    pub fn has_reference_audio(&self, threshold: f32) -> bool {
        self.reference_level >= threshold.max(0.0)
    }

    pub fn summary_line(&self) -> String {
        format!(
            "backend={} processed_frames={} capture_level={:.4} reference_level={:.4} missing_reference_frames={}",
            backend_label(self.backend),
            self.processed_frames,
            self.capture_level,
            self.reference_level,
            self.reference_missing_frames
        )
    }
}

fn backend_label(backend: EchoCancellerBackend) -> &'static str {
    match backend {
        EchoCancellerBackend::Disabled => "Disabled",
        EchoCancellerBackend::Aec3 => "AEC3",
    }
}

fn absolute_correlation(a: &[f32], b: &[f32]) -> f32 {
    let len = a.len().min(b.len());
    if len == 0 {
        return 0.0;
    }

    let (a, b) = (&a[..len], &b[..len]);
    let a_mean = a.iter().copied().sum::<f32>() / len as f32;
    let b_mean = b.iter().copied().sum::<f32>() / len as f32;
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

fn mean_square(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }

    samples.iter().map(|sample| sample * sample).sum::<f32>() / samples.len() as f32
}

fn power_reduction_db(input_power: f32, output_power: f32) -> f32 {
    let input_power = input_power.max(1.0e-12);
    let output_power = output_power.max(1.0e-12);
    10.0 * (input_power / output_power).log10()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AudioFrameFormat;

    #[test]
    fn noop_echo_canceller_copies_capture_and_reports_disabled() {
        let format = AudioFrameFormat::new(48_000, 1);
        let mut canceller = NoopEchoCanceller::new(format);
        let capture = [0.1, -0.2, 0.3];
        let render = [0.3, 0.2, 0.1];
        let mut output = [0.0; 3];

        canceller.process(&capture, &render, &mut output).unwrap();

        assert_eq!(output, capture);
        assert_eq!(
            canceller.runtime_info().backend(),
            EchoCancellerBackend::Disabled
        );
    }

    #[cfg(feature = "aec")]
    #[test]
    fn aec3_worker_is_send_and_reduces_generated_echo() {
        fn assert_send<T: Send>() {}
        assert_send::<Aec3EchoWorker>();

        let format = AudioFrameFormat::new(16_000, 1);
        let mut worker = Aec3EchoWorker::new(format).unwrap();
        let fixture = GeneratedEchoFixture::new(16_000, 240);
        let metrics = run_echo_canceller_on_fixture(&mut worker, &fixture).unwrap();

        assert!(metrics.input_echo_correlation() > 0.35);
        assert!(
            metrics.output_echo_correlation() < metrics.input_echo_correlation() * 0.85,
            "expected worker AEC to reduce echo correlation: input={} output={}",
            metrics.input_echo_correlation(),
            metrics.output_echo_correlation()
        );
        assert!(metrics.echo_power_reduction_db() > 1.5);
    }

    #[cfg(feature = "aec")]
    #[test]
    fn aec3_echo_canceller_reduces_delayed_echo_in_generated_signal() {
        let format = AudioFrameFormat::new(16_000, 1);
        let mut canceller = Aec3EchoCanceller::new(format).unwrap();
        let fixture = GeneratedEchoFixture::new(16_000, 240);
        let metrics = run_echo_canceller_on_fixture(&mut canceller, &fixture).unwrap();

        assert!(metrics.input_echo_correlation() > 0.35);
        assert!(
            metrics.output_echo_correlation() < metrics.input_echo_correlation() * 0.85,
            "expected AEC3 to reduce echo correlation: input={} output={}",
            metrics.input_echo_correlation(),
            metrics.output_echo_correlation()
        );
        assert!(
            metrics.echo_power_reduction_db() > 1.5,
            "expected positive echo power reduction, got {} dB",
            metrics.echo_power_reduction_db()
        );
    }

    #[test]
    fn echo_metrics_report_residual_echo_reduction() {
        let render = [0.0, 0.5, -0.5, 0.25, -0.25, 0.0];
        let capture = [0.0, 0.4, -0.4, 0.2, -0.2, 0.0];
        let output = [0.0, 0.1, -0.1, 0.05, -0.05, 0.0];

        let metrics = EchoReductionMetrics::from_signals(&render, &capture, &output);

        assert!(metrics.input_echo_correlation() > 0.99);
        assert!(metrics.output_echo_correlation() > 0.99);
        assert!(metrics.echo_power_reduction_db() > 11.0);
    }

    #[test]
    fn echo_metrics_apply_reduction_thresholds() {
        let passing = EchoReductionMetrics {
            input_echo_correlation: 0.80,
            output_echo_correlation: 0.40,
            echo_power_reduction_db: 4.0,
        };
        let weak_correlation = EchoReductionMetrics {
            input_echo_correlation: 0.80,
            output_echo_correlation: 0.72,
            echo_power_reduction_db: 4.0,
        };
        let weak_power = EchoReductionMetrics {
            input_echo_correlation: 0.80,
            output_echo_correlation: 0.40,
            echo_power_reduction_db: 0.5,
        };

        assert!(passing.passes_reduction_thresholds(0.75, 1.5));
        assert!(!weak_correlation.passes_reduction_thresholds(0.75, 1.5));
        assert!(!weak_power.passes_reduction_thresholds(0.75, 1.5));
    }

    #[test]
    fn realtime_aec_probe_report_flags_processed_reference_audio() {
        let report = RealtimeAecProbeReport::new(EchoCancellerBackend::Aec3)
            .with_processed_frames(24)
            .with_capture_level(0.04)
            .with_reference_level(0.08)
            .with_missing_reference_frames(3);

        assert!(report.has_processed_audio());
        assert!(report.has_reference_audio(0.01));
        assert_eq!(report.processed_frames(), 24);
        assert_eq!(report.reference_missing_frames(), 3);
        assert!(report.summary_line().contains("AEC3"));
    }
}
