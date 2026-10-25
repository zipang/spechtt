//! Waveform bars from FFT bin amplitudes (`0.0..=1.0`).
//!
//! Recording draws brand-orange bars; paused draws the flat muted dotted row
//! from `paused.png`. The row fills the available width and scales with the
//! window rem size.
//!
//! Bars live in an `Arc<[f32]>` so a live frame reaches the renderer with a
//! refcount bump instead of a per-frame copy or `Vec` allocation.

use std::sync::Arc;

use gpui_kit::base::h_flex;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::prelude::*;
use gpui_kit::{App, Hsla, IntoElement, RenderOnce, Window, div, px, relative, rems};

use crate::tokens::color;

#[derive(IntoElement)]
pub struct Waveform {
    bars: Arc<[f32]>,
    active: bool,
}

impl Waveform {
    /// `bars` accepts any slice-like source (`&[f32]`, `Vec<f32>`,
    /// `Arc<[f32]>`) and is stored shared, never copied per frame.
    pub fn new(bars: impl Into<Arc<[f32]>>, active: bool) -> Self {
        Self {
            bars: bars.into(),
            active,
        }
    }
}

impl RenderOnce for Waveform {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let active_color: Hsla = gpui_kit::rgb(color::BRAND_PRIMARY).into();
        let idle_color = cx.theme().border;

        h_flex()
            .id("waveform")
            .w_full()
            .min_h(rems(2.0))
            .max_h(rems(4.0))
            .h(rems(2.25))
            .items_center()
            .justify_center()
            .gap_1()
            .children(self.bars.iter().map(|amplitude| {
                let fraction = if self.active {
                    (5.0 + amplitude.clamp(0.0, 1.0) * 31.0).clamp(3.0, 36.0) / 36.0
                } else {
                    3.0 / 36.0
                };
                div()
                    .flex_1()
                    .min_w(px(2.))
                    .max_w(px(6.))
                    .h(relative(fraction))
                    .rounded(px(1.5))
                    .bg(if self.active {
                        active_color
                    } else {
                        idle_color
                    })
            }))
    }
}
