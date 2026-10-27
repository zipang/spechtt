//! Microphone capture and conversion to the 16 kHz mono `i16` wire contract.
//!
//! The module is split into two layers:
//!
//! * Pure helpers ([`downmix_to_mono`], [`resample_linear`], [`f32_to_pcm16`],
//!   [`PcmChunker`]) that contain every format/rate decision and are unit
//!   tested without touching hardware.
//! * [`CpalAudioSource`], a thin `cpal` adapter that owns the device stream and
//!   forwards converted chunks through a bounded channel behind the
//!   [`AudioSource`] trait.

use async_channel::{Receiver, Sender};
use thiserror::Error;

/// Sample rate of every chunk that leaves this module.
pub const TARGET_SAMPLE_RATE: u32 = 16_000;
/// Channel count of every chunk that leaves this module.
pub const TARGET_CHANNELS: u16 = 1;
/// ~40 ms at 16 kHz.
pub const CHUNK_SAMPLES: usize = 640;
/// Samples per FFT window.
pub const FFT_WINDOW: usize = 1024;

/// A fixed-size block of 16 kHz mono PCM.
#[derive(Debug, Clone, PartialEq)]
pub struct PcmChunk {
    pub samples: Vec<i16>,
    pub sample_rate: u32,
}

/// Typed failures from device discovery, stream setup, or the stream itself.
#[derive(Debug, Error)]
pub enum AudioError {
    #[error("no input device available")]
    NoDevice,
    #[error("microphone permission denied")]
    PermissionDenied,
    #[error("unsupported device configuration: {0}")]
    UnsupportedConfig(String),
    #[error("audio backend failed: {0}")]
    Backend(String),
}

/// Push-based capture handle. Implementations own their device stream.
pub trait AudioSource: Send {
    /// Blocks until the next chunk is ready or the stream ends.
    fn next_chunk(&mut self) -> Result<Option<PcmChunk>, AudioError>;
}

/// Lifecycle handle for a capture source.
pub trait AudioCapture: Send {
    fn start(&mut self) -> Result<(), AudioError>;
    fn stop(&mut self);
    fn is_running(&self) -> bool;
}

/// Averages interleaved frames down to mono.
///
/// `frame_count` is the number of frames to consume; a shorter slice is
/// handled by averaging whatever complete samples remain.
#[must_use]
pub fn downmix_to_mono(channels: &[f32], frame_count: usize) -> Vec<f32> {
    if channels.is_empty() || frame_count == 0 {
        return Vec::new();
    }

    // The interleaved layout is inferred from the caller's frame count.
    let channel_count = channels.len() / frame_count;
    if channel_count == 0 {
        return Vec::new();
    }

    (0..frame_count)
        .map(|frame| {
            let start = frame * channel_count;
            let frame_samples = &channels[start..start + channel_count];
            frame_samples.iter().sum::<f32>() / channel_count as f32
        })
        .collect()
}

/// Resamples `input` from `from` Hz to `to` Hz with linear interpolation.
///
/// The first and last output samples land on real input samples, so no
/// extrapolation past the buffer is ever required.
#[must_use]
pub fn resample_linear(input: &[f32], from: u32, to: u32) -> Vec<f32> {
    if input.is_empty() || from == 0 || to == 0 {
        return Vec::new();
    }
    if from == to {
        return input.to_vec();
    }

    let from = u64::from(from);
    let to = u64::from(to);
    let output_len = usize::try_from((input.len() as u64 * to / from).max(1)).unwrap_or(usize::MAX);
    let last_index = input.len() - 1;

    (0..output_len)
        .map(|index| {
            // Exact integer source position expressed in units of `1 / to`
            // seconds, which avoids accumulating floating-point drift.
            let position = index as u64 * from;
            let base = usize::try_from(position / to)
                .unwrap_or(last_index)
                .min(last_index);
            let fraction = (position % to) as f32 / to as f32;
            let current = input[base];
            let next = input[(base + 1).min(last_index)];
            current + (next - current) * fraction
        })
        .collect()
}

/// Converts normalized floats to PCM16.
///
/// Values are clamped to `[-1.0, 1.0]` and scaled by [`i16::MAX`], so `-1.0`
/// maps to `-32767` and never wraps to [`i16::MIN`].
#[must_use]
pub fn f32_to_pcm16(samples: &[f32]) -> Vec<i16> {
    let scale = f32::from(i16::MAX);
    samples
        .iter()
        .map(|&sample| {
            let sample = if sample.is_finite() {
                sample.clamp(-1.0, 1.0)
            } else {
                0.0
            };
            (sample * scale).round() as i16
        })
        .collect()
}

/// Accumulates PCM samples and yields exact [`CHUNK_SAMPLES`] chunks.
#[derive(Debug, Default)]
pub struct PcmChunker {
    pending: Vec<i16>,
}

impl PcmChunker {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends samples and returns every complete chunk now available.
    pub fn push(&mut self, samples: &[i16]) -> Vec<PcmChunk> {
        self.pending.extend_from_slice(samples);

        let complete = self.pending.len() / CHUNK_SAMPLES;
        let mut chunks = Vec::with_capacity(complete);
        for _ in 0..complete {
            let remainder = self.pending.split_off(CHUNK_SAMPLES);
            let samples = std::mem::replace(&mut self.pending, remainder);
            chunks.push(PcmChunk {
                samples,
                sample_rate: TARGET_SAMPLE_RATE,
            });
        }
        chunks
    }

    /// Emits the retained partial buffer, if any.
    pub fn flush(&mut self) -> Option<PcmChunk> {
        if self.pending.is_empty() {
            return None;
        }
        Some(PcmChunk {
            samples: std::mem::take(&mut self.pending),
            sample_rate: TARGET_SAMPLE_RATE,
        })
    }

    /// Number of samples retained for the next chunk.
    #[must_use]
    pub fn pending(&self) -> usize {
        self.pending.len()
    }
}

/// Handle to a running capture stream, abstracted so tests need no hardware.
trait CaptureStream: Send {
    fn stop(&mut self);
}

/// Default input stream wrapped so stopping drops (and joins) the real one.
struct CpalStream(Option<cpal::Stream>);

impl CaptureStream for CpalStream {
    fn stop(&mut self) {
        if let Some(stream) = self.0.take() {
            drop(stream);
        }
    }
}

impl Drop for CpalStream {
    fn drop(&mut self) {
        self.stop();
    }
}

type ChunkReceiver = Receiver<Result<PcmChunk, AudioError>>;
type ChunkSender = Sender<Result<PcmChunk, AudioError>>;
type StreamOpener =
    Box<dyn FnMut() -> Result<(ChunkReceiver, Box<dyn CaptureStream>), AudioError> + Send>;

/// Microphone capture backed by `cpal`, exposing 16 kHz mono chunks.
pub struct CpalAudioSource {
    opener: Option<StreamOpener>,
    receiver: Option<ChunkReceiver>,
    stream: Option<Box<dyn CaptureStream>>,
}

impl CpalAudioSource {
    /// Opens the default input device and starts capturing.
    ///
    /// # Errors
    ///
    /// Returns [`AudioError::NoDevice`] when the host has no input device and
    /// a permission/backend error when the stream cannot be opened.
    pub fn open_default() -> Result<Self, AudioError> {
        let mut source = Self {
            opener: Some(Box::new(open_default_stream)),
            receiver: None,
            stream: None,
        };
        source.start()?;
        Ok(source)
    }

    #[cfg(test)]
    fn from_parts(receiver: ChunkReceiver, stream: Box<dyn CaptureStream>) -> Self {
        Self {
            opener: None,
            receiver: Some(receiver),
            stream: Some(stream),
        }
    }

    #[cfg(test)]
    fn from_opener(opener: StreamOpener) -> Self {
        Self {
            opener: Some(opener),
            receiver: None,
            stream: None,
        }
    }
}

impl AudioSource for CpalAudioSource {
    fn next_chunk(&mut self) -> Result<Option<PcmChunk>, AudioError> {
        let Some(receiver) = self.receiver.as_ref() else {
            return Ok(None);
        };
        match receiver.recv_blocking() {
            Ok(Ok(chunk)) => Ok(Some(chunk)),
            Ok(Err(error)) => Err(error),
            // The producer side was dropped: the stream ended.
            Err(_) => Ok(None),
        }
    }
}

impl AudioCapture for CpalAudioSource {
    fn start(&mut self) -> Result<(), AudioError> {
        if self.is_running() {
            return Ok(());
        }
        let opener = self
            .opener
            .as_mut()
            .ok_or_else(|| AudioError::Backend("capture source cannot be restarted".to_owned()))?;
        let (receiver, stream) = opener()?;
        self.receiver = Some(receiver);
        self.stream = Some(stream);
        Ok(())
    }

    fn stop(&mut self) {
        if let Some(mut stream) = self.stream.take() {
            stream.stop();
        }
        // Dropping the receiver also makes a lingering callback drop chunks
        // instead of growing the queue.
        self.receiver = None;
    }

    fn is_running(&self) -> bool {
        self.stream.is_some()
    }
}

impl Drop for CpalAudioSource {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Frames buffered between the audio callback and the consumer (~1.3 s).
const CHANNEL_CAPACITY: usize = 32;

fn open_default_stream() -> Result<(ChunkReceiver, Box<dyn CaptureStream>), AudioError> {
    use cpal::SampleFormat;
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

    let host = cpal::default_host();
    let device = host.default_input_device().ok_or(AudioError::NoDevice)?;
    let supported = device.default_input_config().map_err(map_cpal_error)?;
    let sample_format = supported.sample_format();
    let config = supported.config();
    let source_rate = config.sample_rate;
    let channels = usize::from(config.channels);

    let (sender, receiver) = async_channel::bounded(CHANNEL_CAPACITY);

    let stream = match sample_format {
        SampleFormat::F32 => build_stream::<f32>(&device, config, channels, source_rate, sender),
        SampleFormat::F64 => build_stream::<f64>(&device, config, channels, source_rate, sender),
        SampleFormat::I8 => build_stream::<i8>(&device, config, channels, source_rate, sender),
        SampleFormat::I16 => build_stream::<i16>(&device, config, channels, source_rate, sender),
        SampleFormat::I32 => build_stream::<i32>(&device, config, channels, source_rate, sender),
        SampleFormat::U8 => build_stream::<u8>(&device, config, channels, source_rate, sender),
        SampleFormat::U16 => build_stream::<u16>(&device, config, channels, source_rate, sender),
        SampleFormat::U32 => build_stream::<u32>(&device, config, channels, source_rate, sender),
        other => {
            return Err(AudioError::UnsupportedConfig(format!(
                "unsupported sample format: {other}"
            )));
        }
    }?;

    stream.play().map_err(map_cpal_error)?;
    Ok((receiver, Box::new(CpalStream(Some(stream)))))
}

fn build_stream<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    channels: usize,
    source_rate: u32,
    sender: ChunkSender,
) -> Result<cpal::Stream, AudioError>
where
    T: cpal::SizedSample,
    f32: cpal::FromSample<T>,
{
    use cpal::Sample as _;
    use cpal::traits::DeviceTrait;

    let mut chunker = PcmChunker::new();
    let mut interleaved: Vec<f32> = Vec::new();
    let error_sender = sender.clone();

    device
        .build_input_stream(
            config,
            move |data: &[T], _: &cpal::InputCallbackInfo| {
                interleaved.clear();
                interleaved.extend(data.iter().map(|sample| f32::from_sample(*sample)));

                let frames = interleaved.len().checked_div(channels).unwrap_or(0);
                let mono = downmix_to_mono(&interleaved, frames);
                let resampled = if source_rate == TARGET_SAMPLE_RATE {
                    mono
                } else {
                    resample_linear(&mono, source_rate, TARGET_SAMPLE_RATE)
                };

                for chunk in chunker.push(&f32_to_pcm16(&resampled)) {
                    // A full queue means the consumer is behind. Drop the
                    // chunk rather than stalling the real-time callback.
                    let _ = sender.try_send(Ok(chunk));
                }
            },
            move |error| match error.kind() {
                // Glitches and route changes are non-fatal; the stream keeps
                // running and the next callback delivers more audio.
                cpal::ErrorKind::Xrun
                | cpal::ErrorKind::RealtimeDenied
                | cpal::ErrorKind::DeviceChanged => {}
                _ => {
                    let _ = error_sender.try_send(Err(map_cpal_error(error)));
                    // A fatal stream error is terminal; wake the consumer
                    // with `None` once the queued error has been read.
                    error_sender.close();
                }
            },
            None,
        )
        .map_err(map_cpal_error)
}

fn map_cpal_error(error: cpal::Error) -> AudioError {
    use cpal::ErrorKind;

    match error.kind() {
        ErrorKind::PermissionDenied => AudioError::PermissionDenied,
        ErrorKind::DeviceNotAvailable => AudioError::NoDevice,
        ErrorKind::UnsupportedConfig
        | ErrorKind::UnsupportedOperation
        | ErrorKind::InvalidInput => AudioError::UnsupportedConfig(error.to_string()),
        _ => AudioError::Backend(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::f32::consts::PI;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    fn sine(frequency: f32, sample_rate: u32, samples: usize) -> Vec<f32> {
        (0..samples)
            .map(|i| (2.0 * PI * frequency * i as f32 / sample_rate as f32).sin())
            .collect()
    }

    fn zero_crossings(samples: &[f32]) -> usize {
        samples
            .windows(2)
            .filter(|pair| (pair[0] < 0.0) != (pair[1] < 0.0))
            .count()
    }

    fn pcm(values: &[i16]) -> PcmChunk {
        PcmChunk {
            samples: values.to_vec(),
            sample_rate: TARGET_SAMPLE_RATE,
        }
    }

    // --- T5.1: conversion helpers -------------------------------------

    #[test]
    fn downmix_stereo_averages_channels() {
        let interleaved = [0.5, -0.5, 0.25, 0.75, -1.0, 1.0];
        assert_eq!(downmix_to_mono(&interleaved, 3), vec![0.0, 0.5, 0.0]);
    }

    #[test]
    fn downmix_mono_is_passthrough() {
        let mono = [0.1, -0.2, 0.3, -0.4];
        assert_eq!(downmix_to_mono(&mono, 4), mono.to_vec());
    }

    #[test]
    fn downmix_multichannel_averages_all_channels() {
        let interleaved = [0.2, 0.4, 0.6, -0.3, -0.6, -0.9];
        let downmixed = downmix_to_mono(&interleaved, 2);
        assert_eq!(downmixed.len(), 2);
        assert!((downmixed[0] - 0.4).abs() < 1e-6);
        assert!((downmixed[1] + 0.6).abs() < 1e-6);
    }

    #[test]
    fn downmix_ignores_incomplete_trailing_frame() {
        // 4 samples, 2 channels requested plus a dangling sample.
        let interleaved = [1.0, 1.0, 0.0, 0.0, 9.0];
        assert_eq!(downmix_to_mono(&interleaved, 2), vec![1.0, 0.0]);
    }

    #[test]
    fn downmix_handles_empty_input() {
        assert!(downmix_to_mono(&[], 0).is_empty());
        assert!(downmix_to_mono(&[], 4).is_empty());
    }

    #[test]
    fn resample_same_rate_is_identity() {
        let input = sine(440.0, 16_000, 512);
        let output = resample_linear(&input, 16_000, 16_000);
        assert_eq!(output, input);
    }

    #[test]
    fn resample_48k_to_16k_preserves_tone() {
        let input = sine(440.0, 48_000, 24_000);
        let output = resample_linear(&input, 48_000, 16_000);

        assert_eq!(output.len(), 8_000);
        let source_crossings = zero_crossings(&input);
        let output_crossings = zero_crossings(&output);
        let difference = source_crossings.abs_diff(output_crossings);
        assert!(
            difference <= 2,
            "zero crossings diverged: {source_crossings} vs {output_crossings}"
        );
    }

    #[test]
    fn resample_keeps_endpoints() {
        let input = [0.0, 0.5, 1.0, 0.5, 0.0];
        let output = resample_linear(&input, 48_000, 16_000);
        assert_eq!(output.first().copied(), Some(0.0));
        assert_eq!(output.last().copied(), Some(0.0));
    }

    #[test]
    fn resample_handles_empty_and_degenerate_rates() {
        assert!(resample_linear(&[], 48_000, 16_000).is_empty());
        assert!(resample_linear(&[0.5], 0, 16_000).is_empty());
        assert!(resample_linear(&[0.5], 48_000, 0).is_empty());
    }

    #[test]
    fn pcm16_clamps_and_avoids_min() {
        let converted = f32_to_pcm16(&[0.0, 1.0, -1.0, 2.0, -2.0]);
        assert_eq!(converted, vec![0, i16::MAX, -i16::MAX, i16::MAX, -i16::MAX]);
        assert!(!converted.contains(&i16::MIN));
    }

    #[test]
    fn pcm16_rounds_to_nearest() {
        assert_eq!(f32_to_pcm16(&[0.5]), vec![16_384]);
        assert_eq!(f32_to_pcm16(&[-0.5]), vec![-16_384]);
        assert_eq!(f32_to_pcm16(&[0.001]), vec![33]);
    }

    // --- T5.2: chunking -----------------------------------------------

    #[test]
    fn chunker_yields_exact_chunks_and_retains_partial() {
        let mut chunker = PcmChunker::new();
        let chunks = chunker.push(&vec![7; 1_500]);

        assert_eq!(chunks.len(), 2);
        assert!(
            chunks
                .iter()
                .all(|chunk| chunk.samples.len() == CHUNK_SAMPLES)
        );
        assert!(
            chunks
                .iter()
                .all(|chunk| chunk.sample_rate == TARGET_SAMPLE_RATE)
        );
        assert_eq!(chunker.pending(), 220);
    }

    #[test]
    fn chunker_preserves_sample_order_across_chunks() {
        let mut chunker = PcmChunker::new();
        let input: Vec<i16> = (0..CHUNK_SAMPLES as i16 * 2 + 5).collect();
        let mut output: Vec<i16> = chunker
            .push(&input)
            .into_iter()
            .flat_map(|chunk| chunk.samples)
            .collect();
        output.extend(chunker.flush().expect("partial buffer").samples);
        assert_eq!(output, input);
    }

    #[test]
    fn flush_emits_short_final_chunk_and_clears_buffer() {
        let mut chunker = PcmChunker::new();
        chunker.push(&vec![-3; 1_500]);

        let tail = chunker.flush().expect("flush returns the partial chunk");
        assert_eq!(tail.samples.len(), 220);
        assert_eq!(tail.sample_rate, TARGET_SAMPLE_RATE);
        assert_eq!(chunker.pending(), 0);
        assert!(chunker.flush().is_none());
    }

    #[test]
    fn flush_with_empty_buffer_yields_nothing() {
        let mut chunker = PcmChunker::new();
        assert!(chunker.flush().is_none());
        chunker.push(&[1, 2, 3]);
        assert!(chunker.flush().is_some());
        assert!(chunker.flush().is_none());
    }

    // --- T5.3: AudioSource contract -----------------------------------

    struct MockAudioSource {
        scripted: VecDeque<Result<Option<PcmChunk>, AudioError>>,
    }

    impl AudioSource for MockAudioSource {
        fn next_chunk(&mut self) -> Result<Option<PcmChunk>, AudioError> {
            self.scripted.pop_front().unwrap_or(Ok(None))
        }
    }

    #[test]
    fn mock_source_yields_scripted_chunks_then_none() {
        let first = pcm(&[1, 2, 3]);
        let second = pcm(&[4, 5, 6]);
        let mut source = MockAudioSource {
            scripted: VecDeque::from([Ok(Some(first.clone())), Ok(Some(second.clone())), Ok(None)]),
        };

        assert_eq!(source.next_chunk().expect("first chunk"), Some(first));
        assert_eq!(source.next_chunk().expect("second chunk"), Some(second));
        assert!(source.next_chunk().expect("end of stream").is_none());
        // Exhausted scripts stay closed.
        assert!(source.next_chunk().expect("still closed").is_none());
    }

    #[derive(Clone, Default)]
    struct FakeStream {
        stops: Arc<AtomicUsize>,
    }

    impl CaptureStream for FakeStream {
        fn stop(&mut self) {
            self.stops.fetch_add(1, Ordering::SeqCst);
        }
    }

    fn fake_source(stops: Arc<AtomicUsize>) -> CpalAudioSource {
        let (_sender, receiver) = async_channel::bounded(1);
        CpalAudioSource::from_parts(receiver, Box::new(FakeStream { stops }))
    }

    #[test]
    fn dropping_cpal_source_stops_the_stream() {
        let stops = Arc::new(AtomicUsize::new(0));
        {
            let source = fake_source(Arc::clone(&stops));
            assert!(source.is_running());
        }
        assert_eq!(stops.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn stop_is_idempotent_and_drop_does_not_repeat_it() {
        let stops = Arc::new(AtomicUsize::new(0));
        let mut source = fake_source(Arc::clone(&stops));

        source.stop();
        assert!(!source.is_running());
        source.stop();
        assert!(!source.is_running());
        assert_eq!(stops.load(Ordering::SeqCst), 1);

        drop(source);
        assert_eq!(stops.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn start_after_stop_reopens_the_stream() {
        let opens = Arc::new(AtomicUsize::new(0));
        let stops = Arc::new(AtomicUsize::new(0));
        let (opener_opens, opener_stops) = (Arc::clone(&opens), Arc::clone(&stops));

        let opener = move || {
            opener_opens.fetch_add(1, Ordering::SeqCst);
            let (_sender, receiver) = async_channel::bounded(1);
            Ok((
                receiver,
                Box::new(FakeStream {
                    stops: Arc::clone(&opener_stops),
                }) as Box<dyn CaptureStream>,
            ))
        };

        let mut source = CpalAudioSource::from_opener(Box::new(opener));
        assert!(!source.is_running());

        source.start().expect("first start");
        assert!(source.is_running());
        // Starting an already running source is a no-op.
        source.start().expect("idempotent start");
        assert_eq!(opens.load(Ordering::SeqCst), 1);

        source.stop();
        assert_eq!(stops.load(Ordering::SeqCst), 1);

        source.start().expect("restart");
        assert!(source.is_running());
        assert_eq!(opens.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn next_chunk_returns_none_when_channel_closes() {
        let (sender, receiver) = async_channel::bounded(1);
        drop(sender);
        let mut source = CpalAudioSource::from_parts(receiver, Box::new(FakeStream::default()));
        assert!(source.next_chunk().expect("closed channel").is_none());
    }

    #[test]
    fn next_chunk_propagates_capture_errors() {
        let (sender, receiver) = async_channel::bounded(1);
        sender
            .try_send(Err(AudioError::PermissionDenied))
            .expect("queue one error");
        drop(sender);

        let mut source = CpalAudioSource::from_parts(receiver, Box::new(FakeStream::default()));
        assert!(matches!(
            source.next_chunk(),
            Err(AudioError::PermissionDenied)
        ));
        assert!(source.next_chunk().expect("closed channel").is_none());
    }

    #[test]
    fn next_chunk_delivers_queued_chunks() {
        let (sender, receiver) = async_channel::bounded(2);
        let chunk = pcm(&[9; CHUNK_SAMPLES]);
        sender.try_send(Ok(chunk.clone())).expect("queue chunk");
        drop(sender);

        let mut source = CpalAudioSource::from_parts(receiver, Box::new(FakeStream::default()));
        assert_eq!(source.next_chunk().expect("queued chunk"), Some(chunk));
        assert!(source.next_chunk().expect("end of stream").is_none());
    }
}
