//! FFT spectrum analysis for the UI waveform.
//!
//! One window of mono samples is transformed into exactly [`BAR_COUNT`]
//! normalized amplitudes in `0.0..=1.0`, using log-spaced bands and
//! exponential smoothing so bars decay instead of flickering.

use std::sync::Arc;

use rustfft::FftPlanner;
use rustfft::num_complex::Complex;

/// Number of bars the UI waveform expects.
pub const BAR_COUNT: usize = 36;
/// Weight of the newest frame during exponential smoothing.
pub const SMOOTHING_ALPHA: f32 = 0.6;

/// Lowest frequency represented by the bar bands.
pub const MIN_FREQUENCY: f32 = 60.0;
/// Highest frequency represented by the bar bands (Nyquist at 16 kHz).
pub const MAX_FREQUENCY: f32 = 8_000.0;

/// Reusable FFT-based analyzer with smoothed bar output.
pub struct SpectrumAnalyzer {
    sample_rate: u32,
    planner: FftPlanner<f32>,
    plan: Option<(usize, Arc<dyn rustfft::Fft<f32>>)>,
    hann: Vec<f32>,
    frame: Vec<Complex<f32>>,
    magnitudes: Vec<f32>,
    smoothing: [f32; BAR_COUNT],
    edges: [f32; BAR_COUNT + 1],
}

impl SpectrumAnalyzer {
    /// Creates an analyzer for windows captured at `sample_rate`.
    #[must_use]
    pub fn new(sample_rate: u32) -> Self {
        let mut edges = [MIN_FREQUENCY; BAR_COUNT + 1];
        let ratio = (MAX_FREQUENCY / MIN_FREQUENCY).powf(1.0 / BAR_COUNT as f32);
        for index in 1..=BAR_COUNT {
            edges[index] = edges[index - 1] * ratio;
        }

        Self {
            sample_rate: sample_rate.max(1),
            planner: FftPlanner::new(),
            plan: None,
            hann: Vec::new(),
            frame: Vec::new(),
            magnitudes: Vec::new(),
            smoothing: [0.0; BAR_COUNT],
            edges,
        }
    }

    /// Maps one FFT window to [`BAR_COUNT`] amplitudes in `0.0..=1.0`.
    pub fn bars(&mut self, window: &[f32]) -> Arc<[f32]> {
        let current = if window.len() >= 2 {
            self.band_levels(window)
        } else {
            [0.0; BAR_COUNT]
        };

        for (bar, level) in self.smoothing.iter_mut().zip(current) {
            *bar = (SMOOTHING_ALPHA * level + (1.0 - SMOOTHING_ALPHA) * *bar).clamp(0.0, 1.0);
        }

        Arc::from(self.smoothing)
    }

    /// Flat zeros — used when idle/paused.
    #[must_use]
    pub fn idle_bars(&self) -> Arc<[f32]> {
        Arc::from([0.0_f32; BAR_COUNT])
    }

    /// Returns the bin-center frequency of the largest spectral peak.
    #[must_use]
    pub fn dominant_frequency(&self, window: &[f32]) -> f32 {
        let len = window.len();
        if len < 2 {
            return 0.0;
        }

        let mut planner = FftPlanner::new();
        let plan = planner.plan_fft_forward(len);
        let mut buffer: Vec<Complex<f32>> = window
            .iter()
            .enumerate()
            .map(|(index, &sample)| {
                let sample = finite_or_zero(sample);
                Complex::new(sample * hann_sample(index, len), 0.0)
            })
            .collect();
        plan.process(&mut buffer);

        let bins = len / 2;
        let mut best_bin = 0;
        let mut best_magnitude = 0.0_f32;
        for (index, value) in buffer.iter().enumerate().take(bins + 1).skip(1) {
            let magnitude = value.norm();
            if magnitude > best_magnitude {
                best_magnitude = magnitude;
                best_bin = index;
            }
        }

        if best_magnitude <= 0.0 {
            0.0
        } else {
            best_bin as f32 * self.sample_rate as f32 / len as f32
        }
    }

    /// Reuses the cached plan/window, allocating only when the window size
    /// changes.
    fn prepare(&mut self, len: usize) {
        if self.plan.as_ref().is_some_and(|(size, _)| *size == len) {
            return;
        }

        let plan = self.planner.plan_fft_forward(len);
        self.hann = (0..len).map(|index| hann_sample(index, len)).collect();
        self.frame = vec![Complex::new(0.0, 0.0); len];
        self.magnitudes = vec![0.0; len / 2 + 1];
        self.plan = Some((len, plan));
    }

    /// Computes one normalized frame of bar levels without smoothing.
    fn band_levels(&mut self, window: &[f32]) -> [f32; BAR_COUNT] {
        let len = window.len();
        self.prepare(len);

        for ((slot, &sample), &weight) in self.frame.iter_mut().zip(window).zip(&self.hann) {
            *slot = Complex::new(finite_or_zero(sample) * weight, 0.0);
        }
        if let Some((_, plan)) = &self.plan {
            plan.process(&mut self.frame);
        }

        let bins = len / 2;
        for (slot, value) in self.magnitudes.iter_mut().take(bins + 1).zip(&self.frame) {
            *slot = value.norm();
        }

        // Convert the log-spaced band edges into monotonic bin ranges.
        let mut starts = [0_usize; BAR_COUNT + 1];
        for (index, edge) in self.edges.iter().enumerate() {
            let bin = (edge / self.sample_rate as f32 * len as f32).round() as usize;
            starts[index] = bin.clamp(1, bins);
        }
        for index in 1..=BAR_COUNT {
            starts[index] = starts[index].max(starts[index - 1] + 1).min(bins);
        }

        let mut levels = [0.0_f32; BAR_COUNT];
        for (index, level) in levels.iter_mut().enumerate() {
            let start = starts[index].min(bins.saturating_sub(1));
            let end = starts[index + 1].max(start + 1).min(bins);
            let sum: f32 = self.magnitudes[start..end].iter().sum();
            *level = sum / (end - start) as f32;
        }

        let peak = levels.iter().copied().fold(0.0_f32, f32::max);
        if peak > 0.0 && peak.is_finite() {
            for level in &mut levels {
                *level = (*level / peak).clamp(0.0, 1.0);
            }
        } else {
            return [0.0; BAR_COUNT];
        }

        levels
    }
}

/// Hann window sample at `index` for a window of `len` samples.
fn hann_sample(index: usize, len: usize) -> f32 {
    if len <= 1 {
        return 1.0;
    }
    let phase = std::f32::consts::TAU * index as f32 / (len - 1) as f32;
    0.5 - 0.5 * phase.cos()
}

fn finite_or_zero(sample: f32) -> f32 {
    if sample.is_finite() { sample } else { 0.0 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    const SAMPLE_RATE: u32 = 16_000;

    fn sine(frequency: f32, samples: usize) -> Vec<f32> {
        (0..samples)
            .map(|i| (2.0 * PI * frequency * i as f32 / SAMPLE_RATE as f32).sin())
            .collect()
    }

    fn loudest_index(bars: &[f32]) -> usize {
        bars.iter()
            .enumerate()
            .max_by(|left, right| left.1.total_cmp(right.1))
            .map(|(index, _)| index)
            .expect("bars are never empty")
    }

    fn assert_bounded(bars: &[f32]) {
        assert_eq!(bars.len(), BAR_COUNT);
        for (index, value) in bars.iter().enumerate() {
            assert!(
                (0.0..=1.0).contains(value) && value.is_finite(),
                "bar {index} out of range: {value}"
            );
        }
    }

    // --- T5.4: FFT -----------------------------------------------------

    #[test]
    fn dominant_frequency_of_440_hz_tone_is_440() {
        let analyzer = SpectrumAnalyzer::new(SAMPLE_RATE);
        let frequency = analyzer.dominant_frequency(&sine(440.0, 1024));
        assert!(
            (frequency - 440.0).abs() <= 20.0,
            "expected ~440 Hz, got {frequency} Hz"
        );
    }

    #[test]
    fn dominant_frequency_of_2khz_tone_is_2khz() {
        let analyzer = SpectrumAnalyzer::new(SAMPLE_RATE);
        let frequency = analyzer.dominant_frequency(&sine(2_000.0, 1024));
        assert!(
            (frequency - 2_000.0).abs() <= 20.0,
            "expected ~2000 Hz, got {frequency} Hz"
        );
    }

    #[test]
    fn dominant_frequency_of_silence_is_zero() {
        let analyzer = SpectrumAnalyzer::new(SAMPLE_RATE);
        assert_eq!(analyzer.dominant_frequency(&[0.0; 1024]), 0.0);
    }

    #[test]
    fn higher_tone_lands_in_a_higher_bar() {
        let mut low_analyzer = SpectrumAnalyzer::new(SAMPLE_RATE);
        let low = low_analyzer.bars(&sine(440.0, 1024));

        let mut high_analyzer = SpectrumAnalyzer::new(SAMPLE_RATE);
        let high = high_analyzer.bars(&sine(2_000.0, 1024));

        assert!(
            loudest_index(&high) > loudest_index(&low),
            "2 kHz bar {} is not above 440 Hz bar {}",
            loudest_index(&high),
            loudest_index(&low)
        );
    }

    #[test]
    fn silence_returns_all_zero_bars() {
        let mut analyzer = SpectrumAnalyzer::new(SAMPLE_RATE);
        let bars = analyzer.bars(&[0.0; 1024]);
        assert_eq!(bars.len(), BAR_COUNT);
        assert!(bars.iter().all(|value| *value == 0.0));
    }

    #[test]
    fn bars_are_always_bounded_and_fixed_length() {
        let mut analyzer = SpectrumAnalyzer::new(SAMPLE_RATE);
        let windows = [
            sine(100.0, 1024),
            sine(3_000.0, 1024),
            vec![],
            vec![0.5; 3],
            vec![0.0; 1024],
            vec![f32::NAN; 16],
            sine(440.0, 512),
        ];

        for window in windows {
            let bars = analyzer.bars(&window);
            assert_bounded(&bars);
        }
    }

    #[test]
    fn idle_bars_are_36_zeros() {
        let analyzer = SpectrumAnalyzer::new(SAMPLE_RATE);
        let bars = analyzer.idle_bars();
        assert_eq!(bars.len(), BAR_COUNT);
        assert!(bars.iter().all(|value| *value == 0.0));
    }

    #[test]
    fn smoothing_decays_a_loud_frame_toward_zero() {
        let mut analyzer = SpectrumAnalyzer::new(SAMPLE_RATE);

        let loud = analyzer.bars(&sine(1_000.0, 1024));
        let mut previous = loud.iter().copied().fold(0.0_f32, f32::max);
        assert!(previous > 0.5, "expected a loud frame, peak was {previous}");

        for frame in 0..12 {
            let bars = analyzer.bars(&[0.0; 1024]);
            let current = bars.iter().copied().fold(0.0_f32, f32::max);
            assert!(
                current < previous,
                "frame {frame}: {current} did not decay below {previous}"
            );
            assert_bounded(&bars);
            previous = current;
        }

        assert!(previous < 0.01, "residual peak {previous} is not near zero");
    }

    #[test]
    fn smoothing_stays_bounded_after_alternating_frames() {
        let mut analyzer = SpectrumAnalyzer::new(SAMPLE_RATE);
        for frame in 0..8 {
            let window = if frame % 2 == 0 {
                sine(440.0, 1024)
            } else {
                vec![0.0; 1024]
            };
            assert_bounded(&analyzer.bars(&window));
        }
    }
}
