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
    /// The state's role colour, as the token value it comes from.
    ///
    /// Exposed so the colour of each scoreboard state is testable without
    /// pixels.
    #[must_use]
    pub const fn role_rgb(self) -> Option<u32> {
        match self {
            BadgeTone::Ready => Some(color::SUCCESS),
            BadgeTone::Paused => Some(color::WARNING),
            BadgeTone::Recording => Some(color::DANGER),
        }
    }

    /// Dot colour. Ready is the one "you may start" state, so it wears the
    /// success role — the same mint that means "saved" and "connected". It adds
    /// no colour: `--color-action-success` already exists.
    fn dot(self) -> Hsla {
        match self {
            BadgeTone::Ready => gpui_kit::rgb(color::SUCCESS).into(),
            BadgeTone::Recording => gpui_kit::rgb(color::DANGER).into(),
            BadgeTone::Paused => gpui_kit::rgb(color::WARNING).into(),
        }
    }

    /// Label colour: the state's own role, so the pill reads as one signal
    /// (green ready, red recording, amber paused) rather than a coloured dot
    /// beside neutral text.
    fn label_color(self, cx: &App) -> Hsla {
        match self {
            BadgeTone::Ready => gpui_kit::rgb(color::SUCCESS).into(),
            BadgeTone::Recording => cx.theme().muted_foreground,
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
            (Some(tone), _) => tone.dot(),
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Ready is the success green — the "the microphone is free, press me"
    /// signal — while paused is amber and recording is red.
    #[test]
    fn ready_is_the_success_green_and_paused_is_amber() {
        assert_eq!(BadgeTone::Ready.role_rgb(), Some(color::SUCCESS));
        assert_eq!(BadgeTone::Paused.role_rgb(), Some(color::WARNING));
        // Recording stays on the red the pill already used.
        assert_eq!(BadgeTone::Recording.role_rgb(), Some(color::DANGER));
    }

    /// The three transport states must never share a colour.
    #[test]
    fn every_transport_state_has_its_own_colour() {
        let tones = [BadgeTone::Ready, BadgeTone::Recording, BadgeTone::Paused];
        for (index, left) in tones.iter().enumerate() {
            for right in &tones[index + 1..] {
                assert_ne!(left.role_rgb(), right.role_rgb(), "{left:?} vs {right:?}");
            }
        }
    }
}
