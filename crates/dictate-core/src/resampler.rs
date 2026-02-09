//! Real-time audio resampling to 16kHz mono for transcription.
//!
//! Uses rubato's `process_into_buffer` with pre-allocated buffers to avoid
//! heap allocations in the real-time audio callback path.

use audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Fft, FixedSync, Indexing, Resampler};

use crate::error::AudioError;

/// Target sample rate for all transcription providers.
pub const TRANSCRIPTION_SAMPLE_RATE: u32 = 16_000;

/// Real-time frame-by-frame resampler for audio callbacks.
///
/// Converts audio from the device's native sample rate to 16kHz mono
/// during recording, processing samples incrementally as they arrive.
pub struct FrameResampler {
    resampler: Option<Fft<f32>>,
    channels: u16,
    input_buffer: Vec<f32>,
    /// Pre-allocated output buffer for one resampler chunk.
    output_chunk_buffer: Vec<f32>,
    /// Pre-allocated buffer for multichannel→mono conversion.
    mono_buffer: Vec<f32>,
    chunk_size: usize,
    output_frames_max: usize,
}

impl FrameResampler {
    /// Create a new frame resampler.
    ///
    /// If `source_rate` is already 16kHz and mono, resampling is bypassed.
    ///
    /// # Errors
    ///
    /// Returns [`AudioError::ResamplingError`] if the rubato FFT resampler
    /// cannot be initialized with the given sample rate.
    pub fn new(source_rate: u32, channels: u16) -> Result<Self, AudioError> {
        if source_rate == TRANSCRIPTION_SAMPLE_RATE && channels == 1 {
            return Ok(Self {
                resampler: None,
                channels,
                input_buffer: Vec::new(),
                output_chunk_buffer: Vec::new(),
                mono_buffer: Vec::new(),
                chunk_size: 0,
                output_frames_max: 0,
            });
        }

        let resampler = Fft::<f32>::new(
            source_rate as usize,
            TRANSCRIPTION_SAMPLE_RATE as usize,
            1024,
            2,
            1,
            FixedSync::Input,
        )
        .map_err(|e| AudioError::ResamplingError(e.to_string()))?;

        let chunk_size = resampler.input_frames_max();
        let output_frames_max = resampler.output_frames_max();

        Ok(Self {
            resampler: Some(resampler),
            channels,
            input_buffer: Vec::with_capacity(chunk_size * 2),
            output_chunk_buffer: vec![0.0; output_frames_max],
            mono_buffer: Vec::with_capacity(chunk_size),
            chunk_size,
            output_frames_max,
        })
    }

    /// Process incoming samples and return resampled 16kHz mono output.
    ///
    /// May return an empty vec if not enough samples have accumulated yet.
    ///
    /// # Errors
    ///
    /// Returns [`AudioError::ResamplingError`] if rubato fails to process
    /// the audio chunk.
    pub fn process(&mut self, samples: &[f32]) -> Result<Vec<f32>, AudioError> {
        let mut output = Vec::new();
        self.process_into(samples, &mut output)?;
        Ok(output)
    }

    /// Process incoming samples, appending resampled 16kHz mono output to `out`.
    ///
    /// This is the zero-allocation variant of [`process`](Self::process): the
    /// caller provides a reusable `Vec` that is appended to (never cleared),
    /// so steady-state calls involve no heap allocation.
    ///
    /// # Errors
    ///
    /// Returns [`AudioError::ResamplingError`] if rubato fails to process
    /// the audio chunk.
    pub fn process_into(&mut self, samples: &[f32], out: &mut Vec<f32>) -> Result<(), AudioError> {
        let Some(resampler) = &mut self.resampler else {
            out.extend_from_slice(samples);
            return Ok(());
        };

        // Convert to mono using pre-allocated buffer.
        if self.channels > 1 {
            self.mono_buffer.clear();
            mono_mix_into(samples, self.channels, &mut self.mono_buffer);
            self.input_buffer.extend_from_slice(&self.mono_buffer);
        } else {
            self.input_buffer.extend_from_slice(samples);
        }

        let num_chunks = self.input_buffer.len() / self.chunk_size;
        out.reserve(num_chunks * self.output_frames_max);

        // Process full chunks using Indexing.input_offset to avoid
        // copying each chunk out of the buffer.
        let mut consumed = 0;
        while self.input_buffer.len() - consumed >= self.chunk_size {
            let input_adapter =
                InterleavedSlice::new(&self.input_buffer, 1, self.input_buffer.len())
                    .map_err(|e| AudioError::ResamplingError(e.to_string()))?;

            let mut output_adapter =
                InterleavedSlice::new_mut(&mut self.output_chunk_buffer, 1, self.output_frames_max)
                    .map_err(|e| AudioError::ResamplingError(e.to_string()))?;

            let indexing = Indexing {
                input_offset: consumed,
                output_offset: 0,
                active_channels_mask: None,
                partial_len: None,
            };

            let (_read, written) = resampler
                .process_into_buffer(&input_adapter, &mut output_adapter, Some(&indexing))
                .map_err(|e| AudioError::ResamplingError(e.to_string()))?;

            out.extend_from_slice(&self.output_chunk_buffer[..written]);
            consumed += self.chunk_size;
        }

        // Remove all consumed samples in a single drain.
        if consumed > 0 {
            self.input_buffer.drain(..consumed);
        }

        Ok(())
    }

    /// Flush remaining buffered samples at end of recording.
    ///
    /// # Errors
    ///
    /// Returns [`AudioError::ResamplingError`] if rubato fails to process
    /// the final audio chunk.
    pub fn flush(&mut self) -> Result<Vec<f32>, AudioError> {
        let Some(resampler) = &mut self.resampler else {
            return Ok(std::mem::take(&mut self.input_buffer));
        };

        if self.input_buffer.is_empty() {
            return Ok(Vec::new());
        }

        let remaining = self.input_buffer.len();

        // Pad to chunk_size for the adapter, but use partial_len to tell
        // rubato only `remaining` frames are real data.
        self.input_buffer.resize(self.chunk_size, 0.0);

        let input_adapter = InterleavedSlice::new(&self.input_buffer, 1, self.chunk_size)
            .map_err(|e| AudioError::ResamplingError(e.to_string()))?;

        let mut output_adapter =
            InterleavedSlice::new_mut(&mut self.output_chunk_buffer, 1, self.output_frames_max)
                .map_err(|e| AudioError::ResamplingError(e.to_string()))?;

        let indexing = Indexing {
            input_offset: 0,
            output_offset: 0,
            active_channels_mask: None,
            partial_len: Some(remaining),
        };

        let (_read, written) = resampler
            .process_into_buffer(&input_adapter, &mut output_adapter, Some(&indexing))
            .map_err(|e| AudioError::ResamplingError(e.to_string()))?;

        self.input_buffer.clear();

        Ok(self.output_chunk_buffer[..written].to_vec())
    }
}

/// Mix multichannel audio down to mono by averaging, writing into a pre-allocated buffer.
fn mono_mix_into(samples: &[f32], channels: u16, out: &mut Vec<f32>) {
    out.extend(
        samples
            .chunks(usize::from(channels))
            .map(|frame| frame.iter().sum::<f32>() / f32::from(channels)),
    );
}

#[cfg(test)]
#[allow(clippy::cast_precision_loss)]
mod tests {
    use super::*;

    #[test]
    fn passthrough_when_already_16k_mono() {
        let mut resampler = FrameResampler::new(16_000, 1).unwrap();
        let input = vec![0.1, 0.2, 0.3];
        let output = resampler.process(&input).unwrap();
        assert_eq!(output, input);
    }

    #[test]
    fn mono_mix_averages_channels() {
        let stereo = vec![1.0, 0.0, 0.5, 0.5, 0.0, 1.0];
        let mut out = Vec::new();
        mono_mix_into(&stereo, 2, &mut out);
        assert_eq!(out, vec![0.5, 0.5, 0.5]);
    }

    #[test]
    fn resampler_produces_output_for_large_input() {
        let mut resampler = FrameResampler::new(48_000, 1).unwrap();
        // Feed enough samples to produce output (need >= chunk_size)
        let input: Vec<f32> = (0..2048).map(|i| (i as f32 * 0.001).sin()).collect();
        let output = resampler.process(&input).unwrap();
        // 48kHz -> 16kHz means roughly 1/3 output samples
        assert!(!output.is_empty(), "should produce output for large input");
    }

    #[test]
    fn flush_returns_remaining_samples() {
        let mut resampler = FrameResampler::new(48_000, 1).unwrap();
        // Feed fewer samples than chunk_size
        let input: Vec<f32> = (0..512).map(|i| (i as f32 * 0.001).sin()).collect();
        let output = resampler.process(&input).unwrap();
        assert!(output.is_empty(), "not enough for a full chunk");

        let flushed = resampler.flush().unwrap();
        assert!(!flushed.is_empty(), "flush should return remaining samples");
    }
}
