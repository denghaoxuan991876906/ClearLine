use std::collections::VecDeque;
use std::fmt;

#[derive(Debug, Clone)]
pub struct FrameChunker {
    frame_size_samples: usize,
    pending: VecDeque<f32>,
}

impl FrameChunker {
    pub fn new(frame_size_samples: usize) -> Self {
        let frame_size_samples = frame_size_samples.max(1);
        Self {
            frame_size_samples,
            pending: VecDeque::with_capacity(frame_size_samples * 2),
        }
    }

    pub fn frame_size_samples(&self) -> usize {
        self.frame_size_samples
    }

    pub fn pending_samples(&self) -> usize {
        self.pending.len()
    }

    pub fn push_samples(&mut self, samples: &[f32]) {
        self.pending.extend(samples.iter().copied());
    }

    pub fn pop_frame(&mut self, output: &mut [f32]) -> bool {
        self.try_pop_frame(output).unwrap_or(false)
    }

    pub fn try_pop_frame(&mut self, output: &mut [f32]) -> Result<bool, FrameChunkerError> {
        if output.len() != self.frame_size_samples {
            return Err(FrameChunkerError::FrameBufferSizeMismatch {
                expected: self.frame_size_samples,
                actual: output.len(),
            });
        }

        if self.pending.len() < self.frame_size_samples {
            return Ok(false);
        }

        for sample in output {
            *sample = self
                .pending
                .pop_front()
                .expect("pending length checked before popping frame");
        }

        Ok(true)
    }

    pub fn reset(&mut self) {
        self.pending.clear();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameChunkerError {
    FrameBufferSizeMismatch { expected: usize, actual: usize },
}

impl fmt::Display for FrameChunkerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FrameChunkerError::FrameBufferSizeMismatch { expected, actual } => {
                write!(
                    formatter,
                    "frame buffer length {actual} does not match frame size {expected}"
                )
            }
        }
    }
}

impl std::error::Error for FrameChunkerError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunker_emits_complete_frames_and_keeps_tail() {
        let mut chunker = FrameChunker::new(4);
        let mut frame = [0.0; 4];

        chunker.push_samples(&[0.1, 0.2, 0.3]);
        assert!(!chunker.pop_frame(&mut frame));
        assert_eq!(chunker.pending_samples(), 3);

        chunker.push_samples(&[0.4, 0.5, 0.6, 0.7, 0.8]);

        assert!(chunker.pop_frame(&mut frame));
        assert_eq!(frame, [0.1, 0.2, 0.3, 0.4]);
        assert!(chunker.pop_frame(&mut frame));
        assert_eq!(frame, [0.5, 0.6, 0.7, 0.8]);
        assert!(!chunker.pop_frame(&mut frame));
        assert_eq!(chunker.pending_samples(), 0);
    }

    #[test]
    fn chunker_rejects_output_buffer_with_wrong_frame_size() {
        let mut chunker = FrameChunker::new(4);
        let mut wrong_frame = [0.0; 3];

        chunker.push_samples(&[0.1, 0.2, 0.3, 0.4]);

        let error = chunker.try_pop_frame(&mut wrong_frame).unwrap_err();
        assert_eq!(
            error.to_string(),
            "frame buffer length 3 does not match frame size 4"
        );
    }

    #[test]
    fn chunker_reset_drops_pending_samples() {
        let mut chunker = FrameChunker::new(4);
        let mut frame = [0.0; 4];

        chunker.push_samples(&[0.1, 0.2, 0.3]);
        chunker.reset();

        assert_eq!(chunker.pending_samples(), 0);
        assert!(!chunker.pop_frame(&mut frame));
    }

    #[test]
    fn chunker_uses_at_least_one_sample_frame() {
        let mut chunker = FrameChunker::new(0);
        let mut frame = [0.0; 1];

        chunker.push_samples(&[0.25]);

        assert_eq!(chunker.frame_size_samples(), 1);
        assert!(chunker.pop_frame(&mut frame));
        assert_eq!(frame, [0.25]);
    }
}
