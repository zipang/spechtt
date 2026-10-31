//! Uppercase state pill (`READY`, `RECORDING`, `PAUSED`, `NO SESSION`, …).

use gpui_kit::base::h_flex;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::prelude::*;
use gpui_kit::{App, Hsla, IntoElement, RenderOnce, SharedString, Window, px};

use crate::components::{Label, StatusDot};
use crate::tokens::{color, radius};

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

/// Uppercase state pill from the mockups.
#[derive(IntoElement)]
pub struct StatusBadge {
    label: SharedString,
    tone: Option<BadgeTone>,
    accent: Option<Hsla>,
}

impl StatusBadge {
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            tone: None,
            accent: None,
        }
    }

    pub fn tone(label: impl Into<SharedString>, tone: BadgeTone) -> Self {
        Self {
            label: label.into(),
            tone: Some(tone),
            accent: None,
        }
    }

    /// For setup screens: a brand-colored dot without a transport tone.
    pub fn accent(label: impl Into<SharedString>, dot: Hsla) -> Self {
        Self {
            label: label.into(),
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
            .child(StatusDot::new(dot))
            .child(Label::new(self.label).bold().tone(label_color))
    }
}
