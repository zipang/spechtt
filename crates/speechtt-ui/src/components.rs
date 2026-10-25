//! SpeechTT visual components (RenderOnce), built from design tokens.
//!
//! These are the pieces the mockups define that gpui-component does not
//! ship: the state badge, the elapsed timer, the waveform, and the caption
//! well. They read `crate::tokens` / the active theme — no raw values.
//!
//! Behavior lives in `app.rs` / `speechtt-core`; these components render
//! what they are given (bars, seconds, text).

use gpui_kit::base::h_flex;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::prelude::*;
use gpui_kit::{App, FontWeight, Hsla, IntoElement, RenderOnce, Window, div, px, relative, rems};

use crate::layout::SurfaceBox;
use crate::tokens::{color, radius, rem};

/// Which transport state a badge describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BadgeTone {
    Ready,
    Recording,
    Paused,
}

impl BadgeTone {
    fn dot(self, cx: &App) -> Hsla {
        match self {
            BadgeTone::Ready => cx.theme().muted_foreground,
            BadgeTone::Recording => gpui_kit::rgb(color::DANGER).into(),
            BadgeTone::Paused => gpui_kit::rgb(color::WARNING).into(),
        }
    }

    fn label_color(self, cx: &App) -> Hsla {
        match self {
            BadgeTone::Ready | BadgeTone::Recording => cx.theme().muted_foreground,
            BadgeTone::Paused => gpui_kit::rgb(color::WARNING).into(),
        }
    }
}

/// Uppercase state pill from the mockups (`READY`, `RECORDING`, `PAUSED`,
/// `NO SESSION`, `NEW SESSION`).
#[derive(IntoElement)]
pub struct StatusBadge {
    label: &'static str,
    tone: Option<BadgeTone>,
    accent: Option<Hsla>,
}

impl StatusBadge {
    pub fn new(label: &'static str) -> Self {
        Self {
            label,
            tone: None,
            accent: None,
        }
    }

    pub fn tone(label: &'static str, tone: BadgeTone) -> Self {
        Self {
            label,
            tone: Some(tone),
            accent: None,
        }
    }

    /// For setup screens: a brand-colored dot without a transport tone.
    pub fn accent(label: &'static str, dot: Hsla) -> Self {
        Self {
            label,
            tone: None,
            accent: Some(dot),
        }
    }
}

impl RenderOnce for StatusBadge {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let dot = match (self.tone, self.accent) {
            (Some(tone), _) => tone.dot(cx),
            (None, Some(accent)) => accent,
            (None, None) => cx.theme().muted_foreground,
        };
        let label_color = self
            .tone
            .map_or(cx.theme().muted_foreground, |tone| tone.label_color(cx));

        h_flex()
            .gap_2()
            .items_center()
            .px_3()
            .py_1()
            .rounded(px(radius::MD))
            .bg(cx.theme().secondary)
            .child(div().size(px(6.)).rounded(px(3.)).bg(dot))
            .child(
                div()
                    .text_size(rems(rem::XS))
                    .font_weight(FontWeight::BOLD)
                    .text_color(label_color)
                    .child(self.label),
            )
    }
}

/// `mm:ss` elapsed read-out, right-aligned in the transport header.
#[derive(IntoElement)]
pub struct ElapsedTimer {
    seconds: u32,
}

impl ElapsedTimer {
    pub fn new(seconds: u32) -> Self {
        Self { seconds }
    }
}

impl RenderOnce for ElapsedTimer {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let mono = cx.theme().mono_font_family.clone();
        div()
            .font_family(mono)
            .text_size(rems(rem::SM))
            .text_color(cx.theme().muted_foreground)
            .child(format!("{:02}:{:02}", self.seconds / 60, self.seconds % 60))
    }
}

/// Waveform bars from FFT bin amplitudes (`0.0..=1.0`).
///
/// Recording draws brand-orange bars; paused draws the flat muted dotted
/// row from `paused.png`.
#[derive(IntoElement)]
pub struct Waveform {
    bars: Vec<f32>,
    active: bool,
}

impl Waveform {
    pub fn new(bars: impl Into<Vec<f32>>, active: bool) -> Self {
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

/// The rounded caption card: committed text in muted ink, the live words
/// in brand accent (the `audio capture layer` line in the mockups).
#[derive(IntoElement)]
pub struct CaptionWell {
    committed: String,
    live: String,
}

impl CaptionWell {
    pub fn new(committed: impl Into<String>, live: impl Into<String>) -> Self {
        Self {
            committed: committed.into(),
            live: live.into(),
        }
    }
}

impl RenderOnce for CaptionWell {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let mono = cx.theme().mono_font_family.clone();
        let text_size = rems(rem::SM);

        SurfaceBox::new()
            .flex_1()
            .clip()
            .child(
                div()
                    .font_family(mono.clone())
                    .text_size(text_size)
                    .text_color(cx.theme().muted_foreground)
                    .child(self.committed),
            )
            .child(
                div()
                    .font_family(mono)
                    .text_size(text_size)
                    .text_color(gpui_kit::rgb(color::BRAND_ACCENT))
                    .child(self.live),
            )
    }
}
