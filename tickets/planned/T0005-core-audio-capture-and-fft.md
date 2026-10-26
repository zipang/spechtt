# T0005 — Core: Microphone Capture and FFT Waveform Frames

- **Track**: Rust core (`crates/speechtt-core`)
- **Wave**: 1 (independent)
- **Depends on**: nothing (session machine already landed)
- **Blocks**: T0009 (end-to-end wiring)
- **PRODUCT.md**: Phase 3

## Context & Goal

The UI currently animates canned bars. This ticket delivers the real audio
pipeline in `speechtt-core`:

1. Capture microphone samples with `cpal`.
2. Convert any device format to the wire contract: **16 kHz, mono, `i16`
   PCM**, delivered in fixed ~40 ms chunks.
3. Compute an FFT magnitude spectrum and map it to exactly
   `WAVEFORM_BARS = 36` normalized amplitudes in `0.0..=1.0`, matching what
   `Waveform` already accepts as `Arc<[f32]>`.

Hardware and the OS audio server must never be required by the test suite.

## Scope Boundary

Only touch:

```text
crates/speechtt-core/Cargo.toml     # cpal, rustfft (append-only)
crates/speechtt-core/src/lib.rs     # `pub mod audio; pub mod fft;` (append-only)
crates/speechtt-core/src/audio.rs   # new
crates/speechtt-core/src/fft.rs     # new
```

Do **not** touch `crates/speechtt-ui/` or `src/stt/`.

> Shared registration files (`Cargo.toml`, `lib.rs`) are append-only across
> T0005/T0006/T0007. When those run in parallel, merge them by keeping every
> module/dependency line; no line should be rewritten.

## 1. Audio capture (`audio.rs`)

### Types

```rust
pub const TARGET_SAMPLE_RATE: u32 = 16_000;
pub const TARGET_CHANNELS: u16 = 1;
/// ~40 ms at 16 kHz.
pub const CHUNK_SAMPLES: usize = 640;
/// Samples per FFT window.
pub const FFT_WINDOW: usize = 1024;

#[derive(Debug, Clone, PartialEq)]
pub struct PcmChunk {
    pub samples: Vec<i16>,   // 16 kHz mono
    pub sample_rate: u32,    // always TARGET_SAMPLE_RATE
}

#[derive(Debug, thiserror::Error)]
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

pub trait AudioCapture: Send {
    fn start(&mut self) -> Result<(), AudioError>;
    fn stop(&mut self);
    fn is_running(&self) -> bool;
}
```

### Pure conversion helpers (all unit-testable, no `cpal`)

- `downmix_to_mono(channels: &[f32], frame_count: usize) -> Vec<f32>` for
  interleaved input (1, 2 or N channels), averaging active channels.
- `resample_linear(input: &[f32], from: u32, to: u32) -> Vec<f32>` —
  deterministic, endpoint-safe, no external crate.
- `f32_to_pcm16(samples: &[f32]) -> Vec<i16>` — clamp to `[-1.0, 1.0]`,
  scale by `i16::MAX`, avoid the `-1.0 → i16::MIN` off-by-one trap.
- `PcmChunker` — accumulates converted samples and yields exact
  `CHUNK_SAMPLES`-sized `PcmChunk`s; `flush()` emits a short final chunk;
  partial buffers below one chunk are retained.

### `CpalAudioSource`

- Enumerates the default input device, chooses a config, starts a stream and
  forwards converted chunks over a bounded `async_channel`/`std::sync::mpsc`
  receiver behind the `AudioSource` trait.
- Maps `cpal` failures to `AudioError` (permission, no device, backend).
- Dropping it stops and joins the stream; `stop()` is idempotent.

## 2. FFT waveform (`fft.rs`)

```rust
pub const BAR_COUNT: usize = 36;

pub struct SpectrumAnalyzer { /* planner, window, smoothing */ }

impl SpectrumAnalyzer {
    pub fn new(sample_rate: u32) -> Self;
    /// Maps one FFT window to `BAR_COUNT` amplitudes in 0.0..=1.0.
    pub fn bars(&mut self, window: &[f32]) -> Arc<[f32]>;
    /// Flat zeros — used when idle/paused.
    pub fn idle_bars(&self) -> Arc<[f32]>;
    pub fn dominant_frequency(&self, window: &[f32]) -> f32;
}
```

Requirements:

- Hann window before the FFT; `rustfft::FftPlanner` reused (no allocation per
  frame beyond the output `Arc`).
- Bars use **log-spaced** frequency bands from ~60 Hz to ~8 kHz (Nyquist at
  16 kHz), each bar = mean magnitude of its bins, normalized against the
  frame peak.
- Exponential smoothing (`new = alpha * current + (1 - alpha) * previous`,
  `alpha ≈ 0.6`) so bars decay instead of flickering; expose the constant.
- Silence / all-zero input returns all zeros and must not divide by zero.
- `dominant_frequency` returns the bin-center frequency of the largest peak
  (used by tests, not by the UI).

## 3. TDD Tasks

- [ ] **T5.1 — PCM conversion tests**: stereo downmix averages channels;
  mono passthrough; linear resample 48 kHz → 16 kHz preserves a 440 Hz tone's
  zero-crossing rate within tolerance; `f32_to_pcm16` clamps ±1.0 and rounds.
- [ ] **T5.2 — Chunking tests**: feeding 1500 samples yields two 640-sample
  chunks plus a 220-sample buffer; `flush()` emits 220; sample rate is always
  16 kHz; chunks stay `i16`.
- [ ] **T5.3 — `AudioSource` contract tests**: a `MockAudioSource` yields a
  scripted sequence, then `Ok(None)`; dropping the `CpalAudioSource` handle
  stops it (tested through a fake inner stream, not real hardware).
- [ ] **T5.4 — FFT tests**: 1024-sample 440 Hz sine at 16 kHz →
  `dominant_frequency ≈ 440 ± 20 Hz`; 2 kHz sine lands in a strictly higher
  bar index than 440 Hz; silence → all-zero bars; output length always 36 and
  every value within `0.0..=1.0`; smoothing decays a loud frame toward zero
  over successive silent frames.
- [ ] **T5.5 — Implementation**: add `cpal` + `rustfft` to
  `crates/speechtt-core/Cargo.toml`, write `audio.rs`/`fft.rs`, register both
  modules in `lib.rs`.
- [ ] **T5.6 — Gates**: `cargo fmt -- --check`,
  `cargo clippy --all-targets -- -D warnings`, `cargo test` clean; no test
  opens a real microphone.

## Acceptance Criteria

- `SpectrumAnalyzer::bars` always returns exactly 36 values in `0.0..=1.0`.
- A 440 Hz tone produces a dominant frequency of 440 Hz ± 20 Hz.
- `CpalAudioSource` yields 16 kHz mono `i16` chunks of 640 samples (~40 ms).
- The whole test suite passes on a machine with no audio device.
- Errors are typed (`AudioError`), not stringly.
