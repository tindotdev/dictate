//! Progressive audio chunking for streaming transcription.
//!
//! Breaks a continuous audio stream into overlapping chunks so transcription
//! can begin before recording completes. Uses fixed-duration boundaries
//! with a 2-second overlap between consecutive chunks for continuity.

use std::collections::VecDeque;

use crate::resampler::TRANSCRIPTION_SAMPLE_RATE;

/// Overlap duration in seconds between consecutive chunks.
const OVERLAP_SECS: usize = 2;

/// Overlap in samples at 16kHz.
pub(crate) const OVERLAP_SAMPLES: usize = OVERLAP_SECS * TRANSCRIPTION_SAMPLE_RATE as usize;

/// A chunk of audio ready for transcription.
#[derive(Debug, Clone)]
pub struct AudioChunk {
    /// Chunk index (0-based).
    pub index: usize,
    /// Audio samples (16kHz mono f32).
    pub samples: Vec<f32>,
    /// Whether this chunk includes leading overlap from the previous chunk.
    pub has_leading_overlap: bool,
}

impl AudioChunk {
    /// Duration of this chunk in seconds.
    #[must_use]
    // Precision loss is negligible: this is used only for display logging,
    // and the chunking boundary check uses integer arithmetic.
    #[allow(clippy::cast_precision_loss)]
    pub fn duration_secs(&self) -> f32 {
        self.samples.len() as f32 / TRANSCRIPTION_SAMPLE_RATE as f32
    }

    /// Returns the duration of leading overlap in seconds (0.0 for the first chunk).
    #[must_use]
    #[allow(clippy::cast_precision_loss)]
    pub const fn leading_overlap_secs(&self) -> f32 {
        if self.has_leading_overlap {
            OVERLAP_SECS as f32
        } else {
            0.0
        }
    }
}

/// Configuration for progressive chunking.
#[derive(Debug, Clone)]
pub struct ChunkerConfig {
    /// Target chunk duration in seconds (default: 90).
    pub target_duration_secs: u64,
}

impl Default for ChunkerConfig {
    fn default() -> Self {
        Self {
            target_duration_secs: 90,
        }
    }
}

/// Internal buffer for accumulating samples and managing overlap.
struct ChunkBuffer {
    current_chunk: Vec<f32>,
    overlap_buffer: VecDeque<f32>,
    chunk_index: usize,
}

impl ChunkBuffer {
    fn new() -> Self {
        Self {
            current_chunk: Vec::new(),
            overlap_buffer: VecDeque::with_capacity(OVERLAP_SAMPLES + 1024),
            chunk_index: 0,
        }
    }

    fn add_samples(&mut self, samples: &[f32]) {
        self.current_chunk.extend(samples);

        self.overlap_buffer.extend(samples);
        while self.overlap_buffer.len() > OVERLAP_SAMPLES {
            self.overlap_buffer.pop_front();
        }
    }

    fn duration_secs(&self) -> u64 {
        self.current_chunk.len() as u64 / u64::from(TRANSCRIPTION_SAMPLE_RATE)
    }

    fn create_chunk(&mut self) -> AudioChunk {
        let chunk = AudioChunk {
            index: self.chunk_index,
            samples: std::mem::take(&mut self.current_chunk),
            has_leading_overlap: self.chunk_index > 0,
        };

        // Prepend overlap to the next chunk for transcription continuity.
        self.current_chunk.extend(self.overlap_buffer.iter());

        self.chunk_index += 1;
        chunk
    }

    fn create_final_chunk(&mut self) -> Option<AudioChunk> {
        if self.current_chunk.is_empty() {
            return None;
        }

        let chunk = AudioChunk {
            index: self.chunk_index,
            samples: std::mem::take(&mut self.current_chunk),
            has_leading_overlap: self.chunk_index > 0,
        };
        self.chunk_index += 1;
        Some(chunk)
    }
}

/// Synchronous push-based progressive chunker.
///
/// Feed samples via [`push_samples`](Self::push_samples) and collect chunks
/// as they become ready. Call [`flush`](Self::flush) when recording ends
/// to retrieve the final partial chunk.
pub struct ProgressiveChunker {
    config: ChunkerConfig,
    buffer: ChunkBuffer,
}

impl ProgressiveChunker {
    #[must_use]
    pub fn new(config: ChunkerConfig) -> Self {
        Self {
            config,
            buffer: ChunkBuffer::new(),
        }
    }

    /// Push samples into the chunker. Returns a chunk if the target
    /// duration has been reached.
    pub fn push_samples(&mut self, samples: &[f32]) -> Option<AudioChunk> {
        self.buffer.add_samples(samples);

        if self.buffer.duration_secs() >= self.config.target_duration_secs {
            Some(self.buffer.create_chunk())
        } else {
            None
        }
    }

    /// Flush the remaining samples as a final chunk.
    /// Returns `None` if no samples have been accumulated.
    pub fn flush(&mut self) -> Option<AudioChunk> {
        self.buffer.create_final_chunk()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Generate `duration_secs` worth of silent 16kHz samples.
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    fn silence(duration_secs: f32) -> Vec<f32> {
        vec![0.0; (TRANSCRIPTION_SAMPLE_RATE as f32 * duration_secs) as usize]
    }

    #[test]
    fn no_chunk_before_target_duration() {
        let mut chunker = ProgressiveChunker::new(ChunkerConfig {
            target_duration_secs: 10,
        });

        // Push 9 seconds — should not produce a chunk.
        let result = chunker.push_samples(&silence(9.0));
        assert!(result.is_none());
    }

    #[test]
    fn chunk_at_target_duration() {
        let mut chunker = ProgressiveChunker::new(ChunkerConfig {
            target_duration_secs: 10,
        });

        let result = chunker.push_samples(&silence(10.0));
        assert!(result.is_some());

        let chunk = result.unwrap();
        assert_eq!(chunk.index, 0);
        assert!(!chunk.has_leading_overlap);
        // 10 seconds at 16kHz
        assert_eq!(chunk.samples.len(), 160_000);
    }

    #[test]
    fn second_chunk_has_leading_overlap() {
        let mut chunker = ProgressiveChunker::new(ChunkerConfig {
            target_duration_secs: 10,
        });

        // First chunk
        let first = chunker.push_samples(&silence(10.0));
        assert!(first.is_some());

        // After the first chunk, the buffer should have 2s overlap prepended.
        // Push another 8 seconds to reach 10s total (2s overlap + 8s new).
        let second = chunker.push_samples(&silence(8.0));
        assert!(second.is_some());

        let chunk = second.unwrap();
        assert_eq!(chunk.index, 1);
        assert!(chunk.has_leading_overlap);
        // 2s overlap + 8s new = 10s = 160_000 samples
        assert_eq!(chunk.samples.len(), 160_000);
    }

    #[test]
    fn flush_returns_remaining_samples() {
        let mut chunker = ProgressiveChunker::new(ChunkerConfig {
            target_duration_secs: 60,
        });

        chunker.push_samples(&silence(5.0));
        let final_chunk = chunker.flush();
        assert!(final_chunk.is_some());

        let chunk = final_chunk.unwrap();
        assert_eq!(chunk.index, 0);
        assert!(!chunk.has_leading_overlap);
        assert_eq!(chunk.samples.len(), 80_000); // 5s at 16kHz
    }

    #[test]
    fn flush_after_chunk_has_overlap() {
        let mut chunker = ProgressiveChunker::new(ChunkerConfig {
            target_duration_secs: 10,
        });

        // Produce first chunk
        chunker.push_samples(&silence(10.0));

        // Add 3 more seconds and flush
        chunker.push_samples(&silence(3.0));
        let final_chunk = chunker.flush().unwrap();

        assert_eq!(final_chunk.index, 1);
        assert!(final_chunk.has_leading_overlap);
        // 2s overlap + 3s new = 5s = 80_000 samples
        assert_eq!(final_chunk.samples.len(), 80_000);
    }

    #[test]
    fn flush_on_empty_returns_none() {
        let mut chunker = ProgressiveChunker::new(ChunkerConfig::default());
        assert!(chunker.flush().is_none());
    }

    #[test]
    fn multiple_chunks_sequential() {
        let mut chunker = ProgressiveChunker::new(ChunkerConfig {
            target_duration_secs: 5,
        });

        let mut chunks = Vec::new();

        // Push 25 seconds in 1-second increments
        for _ in 0..25 {
            if let Some(chunk) = chunker.push_samples(&silence(1.0)) {
                chunks.push(chunk);
            }
        }

        if let Some(final_chunk) = chunker.flush() {
            chunks.push(final_chunk);
        }

        // First chunk fills at 5s, subsequent fill every 3s (due to 2s overlap).
        // 25s total: chunks at 5, 8, 11, 14, 17, 20, 23 = 7 full + 1 final (2s overlap + 2s new).
        assert_eq!(chunks.len(), 8);

        // First chunk has no leading overlap
        assert!(!chunks[0].has_leading_overlap);
        assert_eq!(chunks[0].index, 0);

        // Subsequent chunks have leading overlap
        for chunk in &chunks[1..] {
            assert!(chunk.has_leading_overlap);
        }

        // Verify chunk indices are sequential
        for (i, chunk) in chunks.iter().enumerate() {
            assert_eq!(chunk.index, i);
        }
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn leading_overlap_secs_returns_zero_for_first_chunk() {
        let chunk = AudioChunk {
            index: 0,
            samples: vec![0.0; 160_000],
            has_leading_overlap: false,
        };
        assert_eq!(chunk.leading_overlap_secs(), 0.0);
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn leading_overlap_secs_returns_overlap_for_subsequent_chunks() {
        let chunk = AudioChunk {
            index: 1,
            samples: vec![0.0; 160_000],
            has_leading_overlap: true,
        };
        assert_eq!(chunk.leading_overlap_secs(), 2.0);
    }

    #[test]
    #[allow(clippy::cast_precision_loss)]
    fn overlap_samples_correct() {
        let config = ChunkerConfig {
            target_duration_secs: 5,
        };
        let mut chunker = ProgressiveChunker::new(config);

        // Push 5 seconds of identifiable data
        let first_data: Vec<f32> = (0..80_000).map(|i| i as f32).collect();
        let chunk0 = chunker.push_samples(&first_data).unwrap();
        assert_eq!(chunk0.samples.len(), 80_000);

        // The overlap buffer should contain the last 2 seconds (32_000 samples)
        // of the first chunk. The next chunk starts with this overlap.
        // Push 3 more seconds to reach 5s (2s overlap + 3s new).
        let more_data: Vec<f32> = (0..48_000).map(|i| 100_000.0 + i as f32).collect();
        let chunk1 = chunker.push_samples(&more_data).unwrap();

        assert!(chunk1.has_leading_overlap);

        // The first 32_000 samples of chunk1 should match the last 32_000 of chunk0.
        let overlap_from_chunk1 = &chunk1.samples[..OVERLAP_SAMPLES];
        let tail_of_chunk0 = &chunk0.samples[chunk0.samples.len() - OVERLAP_SAMPLES..];
        assert_eq!(overlap_from_chunk1, tail_of_chunk0);
    }
}
