//! Audio recording with real-time resampling to 16kHz mono.
//!
//! Uses cpal for cross-platform audio capture and resamples on-the-fly
//! in the audio callback thread. Resampled samples are pushed into a
//! lock-free SPSC ring buffer (`ringbuf`) with zero allocations in the
//! steady-state callback path.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ringbuf::traits::{Consumer, Observer, Producer, Split};
use ringbuf::{HeapCons, HeapProd, HeapRb};

use crate::error::AudioError;
use crate::resampler::{FrameResampler, TRANSCRIPTION_SAMPLE_RATE};

use super::devices::resolve_input_device;

/// Default ring buffer capacity in samples (~32 seconds at 16kHz mono).
const DEFAULT_BUFFER_CAPACITY_SAMPLES: usize = 512 * 1024;

/// How to select the audio input device.
#[derive(Debug, Clone, Default)]
pub enum DeviceSelection {
    /// Use the system default input device.
    #[default]
    Default,
    /// Select by index from the device enumeration.
    Index(usize),
    /// Select by query string (exact match first, then substring match).
    ///
    /// The query is matched against normalized device names.
    Query(String),
}

/// Recorder configuration.
#[derive(Debug, Clone)]
pub struct RecorderConfig {
    /// Which device to record from.
    pub device: DeviceSelection,
    /// Ring buffer capacity in samples.
    ///
    /// The audio callback never blocks; if the consumer is too slow and the
    /// buffer is full, samples are dropped and accounted for in stats.
    /// Default: `512 * 1024` (~32 seconds at 16kHz mono).
    pub buffer_capacity_samples: usize,
}

impl Default for RecorderConfig {
    fn default() -> Self {
        Self {
            device: DeviceSelection::Default,
            buffer_capacity_samples: DEFAULT_BUFFER_CAPACITY_SAMPLES,
        }
    }
}

/// Information about the started recording stream.
#[derive(Debug, Clone)]
pub struct RecorderInfo {
    pub device_name: String,
    pub device_sample_rate_hz: u32,
    pub device_channels: u16,
    pub target_sample_rate_hz: u32,
}

#[derive(Debug, Clone)]
pub struct RecorderStats {
    inner: Arc<RecorderStatsInner>,
}

#[derive(Debug, Clone)]
pub struct RecorderStatsSnapshot {
    pub stream_errors: u64,
    pub resample_errors: u64,
    pub dropped_samples: u64,
    pub resampler_lock_poisoned: u64,
    pub last_stream_error: Option<String>,
}

#[derive(Debug)]
struct RecorderStatsInner {
    stream_errors: AtomicU64,
    resample_errors: AtomicU64,
    dropped_samples: AtomicU64,
    resampler_lock_poisoned: AtomicU64,
    last_stream_error: Mutex<Option<String>>,
}

impl RecorderStats {
    #[must_use]
    pub fn snapshot(&self) -> RecorderStatsSnapshot {
        let last_stream_error = self
            .inner
            .last_stream_error
            .lock()
            .ok()
            .and_then(|guard| guard.clone());
        RecorderStatsSnapshot {
            stream_errors: self.inner.stream_errors.load(Ordering::Relaxed),
            resample_errors: self.inner.resample_errors.load(Ordering::Relaxed),
            dropped_samples: self.inner.dropped_samples.load(Ordering::Relaxed),
            resampler_lock_poisoned: self.inner.resampler_lock_poisoned.load(Ordering::Relaxed),
            last_stream_error,
        }
    }
}

impl Default for RecorderStats {
    fn default() -> Self {
        Self {
            inner: Arc::new(RecorderStatsInner {
                stream_errors: AtomicU64::new(0),
                resample_errors: AtomicU64::new(0),
                dropped_samples: AtomicU64::new(0),
                resampler_lock_poisoned: AtomicU64::new(0),
                last_stream_error: Mutex::new(None),
            }),
        }
    }
}

// ---------------------------------------------------------------------------
// Condvar-based notification for the ring buffer consumer
// ---------------------------------------------------------------------------

/// Efficient wakeup mechanism for the ring buffer consumer.
///
/// The producer calls [`notify`](Self::notify) after pushing samples; the
/// consumer calls [`wait_timeout`](Self::wait_timeout) when the buffer is
/// empty. The internal `Mutex` prevents missed wakeups (producer pushes
/// between the consumer's empty-check and wait-entry).
struct Notify {
    mutex: Mutex<()>,
    condvar: Condvar,
}

impl Notify {
    const fn new() -> Self {
        Self {
            mutex: Mutex::new(()),
            condvar: Condvar::new(),
        }
    }

    /// Wake the waiting consumer.
    fn notify(&self) {
        // Lock ensures the consumer can't miss a wakeup between its
        // empty-check and Condvar::wait_timeout call.
        drop(self.mutex.lock());
        self.condvar.notify_one();
    }

    /// Block until notified or `timeout` elapses.
    fn wait_timeout(&self, timeout: Duration) {
        if let Ok(guard) = self.mutex.lock() {
            let _ = self.condvar.wait_timeout(guard, timeout);
        }
    }
}

// ---------------------------------------------------------------------------
// NotifyingProducer — wrapper that notifies on Drop
// ---------------------------------------------------------------------------

/// Ring buffer producer that notifies the consumer on every push and on drop.
///
/// The `Drop` impl fires a notification so the consumer wakes immediately
/// when the audio stream stops, instead of waiting for the next timeout.
struct NotifyingProducer {
    inner: HeapProd<f32>,
    notify: Arc<Notify>,
    disconnected: Arc<AtomicBool>,
}

impl NotifyingProducer {
    /// Push samples into the ring buffer, returning the number actually pushed.
    fn push_slice(&mut self, data: &[f32]) -> usize {
        let pushed = self.inner.push_slice(data);
        if pushed > 0 {
            self.notify.notify();
        }
        pushed
    }
}

impl Drop for NotifyingProducer {
    fn drop(&mut self) {
        self.disconnected.store(true, Ordering::Release);
        self.notify.notify();
    }
}

// ---------------------------------------------------------------------------
// AudioReceiver — public consumer API
// ---------------------------------------------------------------------------

/// Result of an [`AudioReceiver::recv_timeout`] call.
pub enum RecvResult<'a> {
    /// Received samples (borrows from the receiver's internal buffer).
    Data(&'a [f32]),
    /// The timeout elapsed with no new data.
    Timeout,
    /// The producer has been dropped and all buffered data has been consumed.
    Disconnected,
}

/// Consumer end of the audio ring buffer.
///
/// Receives resampled 16kHz mono samples from the audio callback thread.
/// All reads reuse an internal buffer, so no heap allocation occurs after
/// warmup.
pub struct AudioReceiver {
    consumer: HeapCons<f32>,
    notify: Arc<Notify>,
    disconnected: Arc<AtomicBool>,
    read_buf: Vec<f32>,
}

impl AudioReceiver {
    /// Block until samples are available or `timeout` elapses.
    ///
    /// Returns a borrowed slice into an internal buffer. The slice is valid
    /// until the next call to `recv_timeout` or `try_recv`.
    pub fn recv_timeout(&mut self, timeout: Duration) -> RecvResult<'_> {
        // Fast path: data already available.
        if self.drain_into_buf() > 0 {
            return RecvResult::Data(&self.read_buf);
        }

        if self.is_disconnected() {
            return RecvResult::Disconnected;
        }

        // Slow path: wait for producer notification.
        self.notify.wait_timeout(timeout);

        if self.drain_into_buf() > 0 {
            return RecvResult::Data(&self.read_buf);
        }

        if self.is_disconnected() {
            RecvResult::Disconnected
        } else {
            RecvResult::Timeout
        }
    }

    /// Non-blocking drain of all currently buffered samples.
    ///
    /// Returns `Some(slice)` if any data was available, `None` otherwise.
    pub fn try_recv(&mut self) -> Option<&[f32]> {
        if self.drain_into_buf() > 0 {
            Some(&self.read_buf)
        } else {
            None
        }
    }

    /// Returns `true` when the producer has dropped **and** the buffer is empty.
    #[must_use]
    pub fn is_disconnected(&self) -> bool {
        self.disconnected.load(Ordering::Acquire) && self.consumer.occupied_len() == 0
    }

    /// Pop all available samples from the ring buffer into `self.read_buf`.
    fn drain_into_buf(&mut self) -> usize {
        self.read_buf.clear();

        let avail = self.consumer.occupied_len();
        if avail == 0 {
            return 0;
        }

        self.read_buf.resize(avail, 0.0);
        let popped = self.consumer.pop_slice(&mut self.read_buf);
        self.read_buf.truncate(popped);
        popped
    }
}

// ---------------------------------------------------------------------------
// AudioRecorder
// ---------------------------------------------------------------------------

/// Audio recorder that captures from the system input device and resamples
/// to 16kHz mono in real-time.
///
/// Resampled samples are pushed into a lock-free SPSC ring buffer for
/// downstream consumption (e.g. by a
/// [`ProgressiveChunker`](super::chunker::ProgressiveChunker)).
pub struct AudioRecorder {
    stream: Option<cpal::Stream>,
    resampler: Arc<Mutex<FrameResampler>>,
    stats: RecorderStats,
    flushed_tail: Vec<f32>,
}

impl AudioRecorder {
    /// Start recording with the provided configuration.
    ///
    /// Returns a recorder handle, an [`AudioReceiver`] that yields resampled
    /// 16kHz mono f32 samples, and info about the selected device/config.
    ///
    /// # Errors
    ///
    /// Returns [`AudioError`] if the device is not found, the input config
    /// cannot be queried, the resampler fails to initialize, or the audio
    /// stream cannot be built or started.
    pub fn start(
        recorder_config: RecorderConfig,
    ) -> Result<(Self, AudioReceiver, RecorderInfo), AudioError> {
        let host = cpal::default_host();

        let device = match recorder_config.device {
            DeviceSelection::Default => host
                .default_input_device()
                .ok_or_else(|| AudioError::device_not_found("no default input device"))?,
            DeviceSelection::Index(index) => resolve_input_device(&host, &format!("#{index}"))?,
            DeviceSelection::Query(query) => resolve_input_device(&host, &query)?,
        };

        let device_name_str = device
            .description()
            .map_or_else(|_| "<unknown>".into(), |d| d.to_string());
        let device_config = device
            .default_input_config()
            .map_err(|e| AudioError::RecordingFailed(e.to_string()))?;

        let device_channels = device_config.channels();
        let device_rate = device_config.sample_rate();

        let info = RecorderInfo {
            device_name: device_name_str,
            device_sample_rate_hz: device_rate,
            device_channels,
            target_sample_rate_hz: TRANSCRIPTION_SAMPLE_RATE,
        };

        let resampler = FrameResampler::new(device_rate, device_channels)
            .map_err(|e| AudioError::RecordingFailed(e.to_string()))?;
        let resampler = Arc::new(Mutex::new(resampler));

        let stats = RecorderStats::default();

        // Lock-free SPSC ring buffer for resampled samples.
        let rb = HeapRb::<f32>::new(recorder_config.buffer_capacity_samples);
        let (prod, cons) = rb.split();

        let notify = Arc::new(Notify::new());
        let disconnected = Arc::new(AtomicBool::new(false));

        let producer = NotifyingProducer {
            inner: prod,
            notify: Arc::clone(&notify),
            disconnected: Arc::clone(&disconnected),
        };

        let receiver = AudioReceiver {
            consumer: cons,
            notify: Arc::clone(&notify),
            disconnected,
            read_buf: Vec::with_capacity(4096),
        };

        let stream_config = cpal::StreamConfig {
            channels: device_channels,
            sample_rate: device_config.sample_rate(),
            buffer_size: cpal::BufferSize::Default,
        };

        let stream = build_input_stream(
            &device,
            &stream_config,
            device_config.sample_format(),
            Arc::clone(&resampler),
            producer,
            stats.clone(),
        )?;

        stream
            .play()
            .map_err(|e| AudioError::from_play_stream(&e))?;

        Ok((
            Self {
                stream: Some(stream),
                resampler,
                stats,
                flushed_tail: Vec::new(),
            },
            receiver,
            info,
        ))
    }

    #[must_use]
    pub fn stats(&self) -> RecorderStats {
        self.stats.clone()
    }

    /// Take the resampler tail samples that were flushed during [`stop`](Self::stop).
    ///
    /// When resampling is active, the resampler buffers partial frames and must be
    /// flushed at stop time to avoid losing trailing audio. These tail samples are
    /// returned here so the caller can append them to the downstream pipeline
    /// after draining any queued ring buffer samples.
    #[must_use]
    pub fn take_flushed_tail(&mut self) -> Vec<f32> {
        std::mem::take(&mut self.flushed_tail)
    }

    /// Stop recording and release the audio device.
    ///
    /// # Errors
    ///
    /// Currently does not return an error; this is kept as a `Result` for
    /// forward compatibility.
    pub fn stop(&mut self) -> Result<(), AudioError> {
        // Dropping the stream stops audio capture. Flush the resampler to avoid
        // losing any buffered trailing samples when resampling is active.
        self.stream = None;

        let tail = match self.resampler.lock() {
            Ok(mut guard) => guard.flush()?,
            Err(poisoned) => {
                self.stats
                    .inner
                    .resampler_lock_poisoned
                    .fetch_add(1, Ordering::Relaxed);
                let mut guard = poisoned.into_inner();
                guard.flush()?
            }
        };
        if !tail.is_empty() {
            self.flushed_tail = tail;
        }
        Ok(())
    }
}

impl Drop for AudioRecorder {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

/// Build a cpal input stream that resamples audio and pushes it into the ring buffer.
fn build_input_stream(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    sample_format: cpal::SampleFormat,
    resampler: Arc<Mutex<FrameResampler>>,
    producer: NotifyingProducer,
    stats: RecorderStats,
) -> Result<cpal::Stream, AudioError> {
    match sample_format {
        cpal::SampleFormat::F32 => {
            build_stream_typed::<f32>(device, config, resampler, producer, stats)
        }
        cpal::SampleFormat::I16 => {
            build_stream_typed::<i16>(device, config, resampler, producer, stats)
        }
        cpal::SampleFormat::U16 => {
            build_stream_typed::<u16>(device, config, resampler, producer, stats)
        }
        fmt => Err(AudioError::RecordingFailed(format!(
            "unsupported sample format: {fmt:?}"
        ))),
    }
}

fn build_stream_typed<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    resampler: Arc<Mutex<FrameResampler>>,
    mut producer: NotifyingProducer,
    stats: RecorderStats,
) -> Result<cpal::Stream, AudioError>
where
    T: cpal::Sample + cpal::SizedSample,
    f32: cpal::FromSample<T>,
{
    let err_stats = stats.clone();
    let err_fn = move |err: cpal::StreamError| {
        err_stats
            .inner
            .stream_errors
            .fetch_add(1, Ordering::Relaxed);
        eprintln!("[dictate] warning: audio stream error: {err}");
        if let Ok(mut last) = err_stats.inner.last_stream_error.lock() {
            *last = Some(err.to_string());
        }
    };

    let stream = device
        .build_input_stream(
            config,
            {
                // Pre-allocated scratch buffers — reused every callback, zero alloc
                // in steady state.
                let mut scratch = Vec::<f32>::new();
                let mut output_buf = Vec::<f32>::new();

                move |data: &[T], _: &cpal::InputCallbackInfo| {
                    // Convert input samples to f32.
                    scratch.clear();
                    scratch.reserve(data.len());
                    for &sample in data {
                        scratch.push(<f32 as cpal::FromSample<T>>::from_sample_(sample));
                    }

                    // Resample to 16kHz mono.
                    let mut guard = match resampler.lock() {
                        Ok(guard) => guard,
                        Err(poisoned) => {
                            stats
                                .inner
                                .resampler_lock_poisoned
                                .fetch_add(1, Ordering::Relaxed);
                            poisoned.into_inner()
                        }
                    };

                    output_buf.clear();
                    if let Err(err) = guard.process_into(&scratch, &mut output_buf) {
                        stats.inner.resample_errors.fetch_add(1, Ordering::Relaxed);
                        eprintln!(
                            "[dictate] warning: audio resample error: {err} (audio data lost)"
                        );
                        return;
                    }
                    drop(guard);

                    if output_buf.is_empty() {
                        return;
                    }

                    // Push into the ring buffer — non-blocking.
                    let pushed = producer.push_slice(&output_buf);
                    let dropped = output_buf.len() - pushed;
                    if dropped > 0 {
                        stats
                            .inner
                            .dropped_samples
                            .fetch_add(dropped as u64, Ordering::Relaxed);
                    }
                }
            },
            err_fn,
            None,
        )
        .map_err(|e| AudioError::from_build_stream(&e))?;

    Ok(stream)
}
