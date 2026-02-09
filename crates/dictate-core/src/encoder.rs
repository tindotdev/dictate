//! Audio encoding for transcription API upload.
//!
//! Converts raw `f32` samples into a byte format that transcription providers
//! accept. The MVP ships a [`WavEncoder`] (44-byte RIFF header + PCM-16
//! samples); the [`AudioEncoder`] trait allows swapping in MP3 later.

use bytes::Bytes;

use crate::error::TranscriptionError;

/// Encoded audio ready for upload to a transcription provider.
#[derive(Debug, Clone)]
pub struct EncodedAudio {
    /// Raw file bytes (header + payload).
    data: Bytes,
    /// MIME type for the HTTP multipart `Content-Type` (e.g. `audio/wav`).
    mime_type: &'static str,
    /// File extension without the dot (e.g. `wav`).
    extension: &'static str,
}

impl EncodedAudio {
    /// Raw file bytes (header + payload).
    pub const fn data(&self) -> &Bytes {
        &self.data
    }

    /// MIME type for the HTTP multipart `Content-Type` (e.g. `audio/wav`).
    pub const fn mime_type(&self) -> &'static str {
        self.mime_type
    }

    /// File extension without the dot (e.g. `wav`).
    pub const fn extension(&self) -> &'static str {
        self.extension
    }
}

/// Trait for encoding raw audio samples into an upload-ready format.
pub trait AudioEncoder {
    /// Encode `samples` (mono, at the given `sample_rate`) into bytes.
    ///
    /// # Errors
    ///
    /// Returns [`TranscriptionError::EncodingFailed`] on invalid input.
    fn encode(&self, samples: &[f32], sample_rate: u32)
    -> Result<EncodedAudio, TranscriptionError>;
}

// ─── WAV Encoder ─────────────────────────────────────────────────────────────

/// Encodes audio as a WAV file (RIFF, PCM-16, mono).
///
/// For our fixed format (16 kHz, mono, 16-bit signed PCM) the header is a
/// compile-time-known 44 bytes; only the data-size field changes per chunk.
#[derive(Debug, Default, Clone, Copy)]
pub struct WavEncoder;

impl AudioEncoder for WavEncoder {
    fn encode(
        &self,
        samples: &[f32],
        sample_rate: u32,
    ) -> Result<EncodedAudio, TranscriptionError> {
        if samples.is_empty() {
            return Err(TranscriptionError::EncodingFailed(
                "cannot encode empty audio".into(),
            ));
        }

        let data = encode_wav_pcm16(samples, sample_rate)?;

        Ok(EncodedAudio {
            data: data.into(),
            mime_type: "audio/wav",
            extension: "wav",
        })
    }
}

// ─── WAV internals ───────────────────────────────────────────────────────────

/// WAV header size in bytes (RIFF + fmt + data sub-chunk header).
const WAV_HEADER_SIZE: u32 = 44;

/// Number of audio channels (always mono for transcription).
const CHANNELS: u16 = 1;

/// Bits per sample (16-bit signed PCM).
const BITS_PER_SAMPLE: u16 = 16;

/// Bytes per sample (16-bit = 2 bytes).
const BYTES_PER_SAMPLE: u16 = BITS_PER_SAMPLE / 8;

/// Build a complete WAV file from f32 samples at the given sample rate.
fn encode_wav_pcm16(samples: &[f32], sample_rate: u32) -> Result<Vec<u8>, TranscriptionError> {
    let num_samples = u32::try_from(samples.len()).map_err(|_| {
        TranscriptionError::EncodingFailed("audio too large for WAV format (>4 GB)".into())
    })?;
    let data_size = num_samples * u32::from(BYTES_PER_SAMPLE);
    let file_size = WAV_HEADER_SIZE - 8 + data_size; // RIFF chunk size = file_size - 8
    let byte_rate = sample_rate * u32::from(CHANNELS) * u32::from(BYTES_PER_SAMPLE);
    let block_align = CHANNELS * BYTES_PER_SAMPLE;

    let mut buf = Vec::with_capacity((WAV_HEADER_SIZE + data_size) as usize);

    // ── RIFF header (12 bytes) ──
    buf.extend_from_slice(b"RIFF");
    buf.extend_from_slice(&file_size.to_le_bytes());
    buf.extend_from_slice(b"WAVE");

    // ── fmt sub-chunk (24 bytes) ──
    buf.extend_from_slice(b"fmt ");
    buf.extend_from_slice(&16_u32.to_le_bytes()); // sub-chunk size (PCM = 16)
    buf.extend_from_slice(&1_u16.to_le_bytes()); // audio format (1 = PCM)
    buf.extend_from_slice(&CHANNELS.to_le_bytes());
    buf.extend_from_slice(&sample_rate.to_le_bytes());
    buf.extend_from_slice(&byte_rate.to_le_bytes());
    buf.extend_from_slice(&block_align.to_le_bytes());
    buf.extend_from_slice(&BITS_PER_SAMPLE.to_le_bytes());

    // ── data sub-chunk header (8 bytes) ──
    buf.extend_from_slice(b"data");
    buf.extend_from_slice(&data_size.to_le_bytes());

    // ── PCM-16 sample data ──
    for &sample in samples {
        let clamped = sample.clamp(-1.0, 1.0);
        // Safety: clamp guarantees the product is in [-32767.0, 32767.0], which fits in i16.
        #[allow(clippy::cast_possible_truncation)]
        let pcm16 = (clamped * f32::from(i16::MAX)) as i16;
        buf.extend_from_slice(&pcm16.to_le_bytes());
    }

    Ok(buf)
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resampler::TRANSCRIPTION_SAMPLE_RATE;

    #[test]
    fn wav_header_is_44_bytes() {
        let samples = vec![0.0; 16_000]; // 1 second of silence
        let data = encode_wav_pcm16(&samples, TRANSCRIPTION_SAMPLE_RATE).unwrap();
        // Header: 44 bytes, data: 16_000 * 2 = 32_000 bytes
        assert_eq!(data.len(), 44 + 32_000);
    }

    #[test]
    fn wav_header_riff_magic() {
        let data = encode_wav_pcm16(&[0.0; 100], TRANSCRIPTION_SAMPLE_RATE).unwrap();
        assert_eq!(&data[0..4], b"RIFF");
        assert_eq!(&data[8..12], b"WAVE");
        assert_eq!(&data[12..16], b"fmt ");
        assert_eq!(&data[36..40], b"data");
    }

    #[test]
    fn wav_header_fields_correct() {
        let samples = vec![0.0; 16_000];
        let data = encode_wav_pcm16(&samples, TRANSCRIPTION_SAMPLE_RATE).unwrap();

        // File size field = total - 8
        let file_size = u32::from_le_bytes(data[4..8].try_into().unwrap());
        assert_eq!(file_size as usize, data.len() - 8);

        // fmt sub-chunk size = 16 (PCM)
        let fmt_size = u32::from_le_bytes(data[16..20].try_into().unwrap());
        assert_eq!(fmt_size, 16);

        // Audio format = 1 (PCM)
        let audio_fmt = u16::from_le_bytes(data[20..22].try_into().unwrap());
        assert_eq!(audio_fmt, 1);

        // Channels = 1
        let channels = u16::from_le_bytes(data[22..24].try_into().unwrap());
        assert_eq!(channels, 1);

        // Sample rate = 16000
        let rate = u32::from_le_bytes(data[24..28].try_into().unwrap());
        assert_eq!(rate, 16_000);

        // Byte rate = 16000 * 1 * 2 = 32000
        let byte_rate = u32::from_le_bytes(data[28..32].try_into().unwrap());
        assert_eq!(byte_rate, 32_000);

        // Block align = 1 * 2 = 2
        let block_align = u16::from_le_bytes(data[32..34].try_into().unwrap());
        assert_eq!(block_align, 2);

        // Bits per sample = 16
        let bps = u16::from_le_bytes(data[34..36].try_into().unwrap());
        assert_eq!(bps, 16);

        // Data size = 16000 * 2 = 32000 (at offset 40, after the "data" marker)
        let data_size = u32::from_le_bytes(data[40..44].try_into().unwrap());
        assert_eq!(data_size, 32_000);
    }

    #[test]
    fn pcm16_conversion_clamps_extremes() {
        // A signal with values outside [-1, 1]
        let samples = vec![-2.0, -1.0, 0.0, 1.0, 2.0];
        let data = encode_wav_pcm16(&samples, TRANSCRIPTION_SAMPLE_RATE).unwrap();

        // Extract PCM16 values from after the 44-byte header
        let pcm: Vec<i16> = data[44..]
            .chunks_exact(2)
            .map(|b| i16::from_le_bytes([b[0], b[1]]))
            .collect();

        // -2.0 clamped to -1.0 → i16::MIN + 1 (because -1.0 * 32767 = -32767)
        assert_eq!(pcm[0], -i16::MAX); // -32767
        assert_eq!(pcm[1], -i16::MAX); // -32767
        assert_eq!(pcm[2], 0);
        assert_eq!(pcm[3], i16::MAX); // 32767
        assert_eq!(pcm[4], i16::MAX); // 32767 (clamped from 2.0)
    }

    #[test]
    fn encoder_trait_wav() {
        let wav = WavEncoder;
        let samples = vec![0.0; 16_000];
        let result = wav.encode(&samples, TRANSCRIPTION_SAMPLE_RATE).unwrap();

        assert_eq!(result.mime_type(), "audio/wav");
        assert_eq!(result.extension(), "wav");
        assert_eq!(result.data().len(), 44 + 32_000);
    }

    #[test]
    fn encoder_rejects_empty_samples() {
        let wav = WavEncoder;
        let result = wav.encode(&[], TRANSCRIPTION_SAMPLE_RATE);
        assert!(result.is_err());
    }

    #[test]
    #[allow(clippy::cast_precision_loss)]
    fn roundtrip_known_signal() {
        // Encode a simple ramp and verify we can read back approximately the same values.
        #[allow(clippy::cast_precision_loss)]
        let samples: Vec<f32> = (0..100)
            .map(|i: i32| (i as f32 / 100.0).mul_add(2.0, -1.0))
            .collect();
        let data = encode_wav_pcm16(&samples, TRANSCRIPTION_SAMPLE_RATE).unwrap();

        let pcm_values: Vec<i16> = data[44..]
            .chunks_exact(2)
            .map(|b| i16::from_le_bytes([b[0], b[1]]))
            .collect();

        assert_eq!(pcm_values.len(), 100);

        // Verify the reconstruction is within quantization tolerance (±1/32768 ≈ 3e-5).
        for (i, &pcm) in pcm_values.iter().enumerate() {
            let original = samples[i].clamp(-1.0, 1.0);
            let reconstructed = f32::from(pcm) / f32::from(i16::MAX);
            assert!(
                (original - reconstructed).abs() < 0.001,
                "sample {i}: original={original}, reconstructed={reconstructed}"
            );
        }
    }
}
