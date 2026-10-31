//! File-take progress drawn as a waveform, plus the decoded-position read-out.
//!
//! The live take screen shows a waveform while the microphone records, so a file
//! take reports its progress with the same vocabulary instead of a separate
//! continuous bar: the row is a dense, regular sine wave of bars, revealed from
//! the left as the worker advances through the file. Revealed bars are
//! `brand.primary`; the part still to come stays on the theme's border colour, so
//! the full length is always visible and the reveal reads as a fill.
//!
//! Bar widths and heights follow [`crate::components::Waveform`]: narrow rounded
//! bars on a shared baseline. No colour, size or radius of its own.

use gpui_kit::base::h_flex;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::prelude::*;
use gpui_kit::{App, IntoElement, RenderOnce, Window, div, px, relative, rems};

use crate::components::Label;
use crate::tokens::color;

/// How many bars the row draws.
///
/// Dense enough that several bars trace each oscillation, wide enough to stay
/// inside the widget at its minimum size.
pub const PROGRESS_BARS: usize = 80;

/// Complete up-and-down oscillations across the row.
///
/// A dense wave reads as a signal the user is progressing through, rather than a
/// single slow hump that could be mistaken for a styling flourish.
pub const PROGRESS_OSCILLATIONS: usize = 10;

/// Where the decode sits in the file, as `0.0..=1.0`.
///
/// `None` when the duration is unknown or unusable (`None`, non-finite or
/// `<= 0.0`), because a fraction of an unknown total is meaningless: the row then
/// shows no revealed bar. A negative or non-finite `processed` yields
/// `Some(0.0)`. The result is always finite and inside `0.0..=1.0`.
#[must_use]
pub fn progress_fraction(processed: f32, total: Option<f32>) -> Option<f32> {
    let total = total?;
    if !total.is_finite() || total <= 0.0 {
        return None;
    }
    if processed.is_finite() {
        Some((processed / total).clamp(0.0, 1.0))
    } else {
        Some(0.0)
    }
}

/// How many of `bars` are revealed at `fraction`.
///
/// This is the clipping box at bar resolution: the boundary bar is revealed once
/// the fraction has passed its midpoint, so the row never over- or under-reports
/// by a whole mark. `fraction` outside `0.0..=1.0`, or a `NaN`, clamps.
#[must_use]
pub fn revealed_bars(fraction: Option<f32>, bars: usize) -> usize {
    let Some(fraction) = fraction else {
        return 0;
    };
    if !fraction.is_finite() {
        return 0;
    }
    let fraction = fraction.clamp(0.0, 1.0);
    let count = (fraction * bars as f32).round() as usize;
    count.min(bars)
}

/// Normalised height of bar `index`, in `0.0..=1.0`.
///
/// [`PROGRESS_OSCILLATIONS`] complete periods across `bars`, as a raised cosine
/// so each period is one clean rise and fall with a trough at both ends of the
/// row. Heights stay inside the same 5..36 px band the live waveform uses, so the
/// troughs keep a visible mark instead of vanishing.
#[must_use]
pub fn bar_height(index: usize, bars: usize) -> f32 {
    if bars == 0 {
        return 0.0;
    }
    let phase = std::f32::consts::TAU * PROGRESS_OSCILLATIONS as f32 * (index as f32 / bars as f32);
    let wave = (1.0 - phase.cos()) / 2.0;
    (5.0 + wave * 31.0) / 36.0
}

/// The read-out under the row: `"12 / 60 s"`, or `"12 s"` without a total.
///
/// Whole seconds, so the number changes at a readable pace.
#[must_use]
pub fn progress_readout(processed: f32, total: Option<f32>) -> String {
    let processed = if processed.is_finite() {
        processed.max(0.0).round() as u64
    } else {
        0
    };
    match total.filter(|total| total.is_finite() && *total > 0.0) {
        Some(total) => format!("{processed} / {} s", total.round() as u64),
        None => format!("{processed} s"),
    }
}

#[derive(IntoElement)]
pub struct ProgressBar {
    processed: f32,
    total: Option<f32>,
}

impl ProgressBar {
    /// `processed` and `total` are decoded seconds, as reported by the worker.
    pub fn new(processed: f32, total: Option<f32>) -> Self {
        Self { processed, total }
    }

    /// The revealed fraction, or `None` when the duration is unknown (test
    /// surface: the view never needs to ask).
    #[must_use]
    pub fn fraction(&self) -> Option<f32> {
        progress_fraction(self.processed, self.total)
    }

    /// How many bars the row reveals at this position (test surface).
    #[must_use]
    pub fn revealed(&self) -> usize {
        revealed_bars(self.fraction(), PROGRESS_BARS)
    }

    /// The seconds read-out this component renders (test surface).
    #[must_use]
    pub fn readout(&self) -> String {
        progress_readout(self.processed, self.total)
    }
}

impl RenderOnce for ProgressBar {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let revealed = self.revealed();
        let done: gpui_kit::Hsla = gpui_kit::rgb(color::BRAND_PRIMARY).into();
        let pending = cx.theme().border;
        let readout = self.readout();

        h_flex()
            .id("progress-bar")
            .w_full()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                h_flex()
                    .w_full()
                    .min_h(rems(2.0))
                    .max_h(rems(4.0))
                    .h(rems(2.25))
                    .items_center()
                    .gap_1()
                    .children((0..PROGRESS_BARS).map(|index| {
                        div()
                            .flex_1()
                            .min_w(px(2.))
                            .max_w(px(6.))
                            .h(relative(bar_height(index, PROGRESS_BARS)))
                            .rounded(px(1.5))
                            .bg(if index < revealed { done } else { pending })
                    })),
            )
            .child(Label::new(readout).mono())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_known_total_yields_the_fraction() {
        assert_eq!(progress_fraction(30.0, Some(120.0)), Some(0.25));
        assert_eq!(progress_fraction(0.0, Some(120.0)), Some(0.0));
        assert_eq!(progress_fraction(120.0, Some(120.0)), Some(1.0));
    }

    #[test]
    fn the_fraction_is_clamped_into_the_row() {
        assert_eq!(progress_fraction(240.0, Some(120.0)), Some(1.0));
        assert_eq!(progress_fraction(-30.0, Some(120.0)), Some(0.0));
    }

    #[test]
    fn an_unknown_or_unusable_total_leaves_the_row_unrevealed() {
        assert_eq!(progress_fraction(30.0, None), None);
        assert_eq!(progress_fraction(30.0, Some(0.0)), None);
        assert_eq!(progress_fraction(30.0, Some(-5.0)), None);
        assert_eq!(progress_fraction(30.0, Some(f32::NAN)), None);
        assert_eq!(progress_fraction(30.0, Some(f32::INFINITY)), None);
        assert_eq!(revealed_bars(None, PROGRESS_BARS), 0);
    }

    #[test]
    fn the_fraction_is_never_nan() {
        for processed in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            if let Some(fraction) = progress_fraction(processed, Some(60.0)) {
                assert!(fraction.is_finite(), "{processed}");
            }
        }
        assert_eq!(revealed_bars(Some(f32::NAN), PROGRESS_BARS), 0);
    }

    #[test]
    fn the_reveal_grows_bar_by_bar_and_stops_at_the_end() {
        assert_eq!(revealed_bars(Some(0.0), 80), 0);
        assert_eq!(revealed_bars(Some(0.5), 80), 40);
        assert_eq!(revealed_bars(Some(1.0), 80), 80);
        // Clamped, so a stale or ahead-of-time reading cannot overshoot.
        assert_eq!(revealed_bars(Some(-1.0), 80), 0);
        assert_eq!(revealed_bars(Some(2.0), 80), 80);

        let mut previous = 0;
        for step in 0..=20 {
            let count = revealed_bars(Some(step as f32 / 20.0), 80);
            assert!(count >= previous, "must not go backwards at {step}");
            previous = count;
        }
    }

    #[test]
    fn the_bars_trace_at_least_ten_complete_oscillations() {
        let bars = PROGRESS_BARS;
        // The design asks for a dense wave: at least ten complete oscillations.
        const { assert!(PROGRESS_OSCILLATIONS >= 10) };

        // A complete oscillation is a local peak: count them across the row.
        let heights: Vec<f32> = (0..bars).map(|index| bar_height(index, bars)).collect();
        let peaks = heights
            .windows(3)
            .filter(|pair| pair[1] > pair[0] && pair[1] >= pair[2])
            .count();
        assert_eq!(
            peaks, PROGRESS_OSCILLATIONS,
            "one peak per oscillation, got {peaks}"
        );

        // Each period starts at a trough and peaks half way through it.
        let period = bars / PROGRESS_OSCILLATIONS;
        for step in 0..PROGRESS_OSCILLATIONS {
            let trough = heights[step * period];
            assert!(
                (trough - 5.0 / 36.0).abs() < 1e-6,
                "period {step} starts at a trough, got {trough}"
            );
            let peak = heights[step * period + period / 2];
            assert!(
                (peak - 1.0).abs() < 1e-6,
                "period {step} peaks half way, got {peak}"
            );
        }
        // The row is a whole number of periods, so it opens and closes at rest.
        assert!((heights[0] - 5.0 / 36.0).abs() < 1e-6, "starts at a trough");

        for (index, height) in heights.iter().enumerate() {
            assert!(
                (5.0 / 36.0..=1.0).contains(height),
                "bar {index} is {height}"
            );
        }
        assert_eq!(bar_height(0, 0), 0.0, "an empty row has no height");
    }

    #[test]
    fn the_readout_names_the_total_when_it_is_known() {
        assert_eq!(progress_readout(12.4, Some(60.0)), "12 / 60 s");
        assert_eq!(progress_readout(12.6, None), "13 s");
        assert_eq!(progress_readout(-3.0, None), "0 s");
        assert_eq!(progress_readout(-3.0, Some(60.0)), "0 / 60 s");
    }

    #[test]
    fn the_component_exposes_the_reveal_and_the_readout() {
        let known = ProgressBar::new(30.0, Some(120.0));
        assert_eq!(known.fraction(), Some(0.25));
        assert_eq!(known.revealed(), PROGRESS_BARS / 4);
        assert_eq!(known.readout(), "30 / 120 s");

        let unknown = ProgressBar::new(30.0, None);
        assert_eq!(unknown.fraction(), None, "the row stays unrevealed");
        assert_eq!(unknown.revealed(), 0);
        assert_eq!(unknown.readout(), "30 s");
    }
}
