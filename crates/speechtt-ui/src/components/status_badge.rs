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
    /// A file take whose worker is decoding the audio.
    InProgress,
    /// A file take whose worker has finished.
    Done,
    /// A file take the user ended before the worker reached the end.
    Stopped,
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
            BadgeTone::InProgress => Some(color::WARNING),
            BadgeTone::Done => Some(color::SUCCESS),
            BadgeTone::Stopped => Some(color::DANGER),
        }
    }

    /// Dot colour, one role per state: success for ready and done, danger for
    /// recording and for a take ended early, warning for paused and for a decode
    /// in progress. Each role already exists in the token set.
    fn dot(self) -> Hsla {
        match self {
            BadgeTone::Ready | BadgeTone::Done => gpui_kit::rgb(color::SUCCESS).into(),
            BadgeTone::Recording | BadgeTone::Stopped => gpui_kit::rgb(color::DANGER).into(),
            BadgeTone::Paused | BadgeTone::InProgress => gpui_kit::rgb(color::WARNING).into(),
        }
    }

    /// Label colour: a working state keeps muted text beside its coloured dot
    /// (recording, decoding), while a resting state colours the label too
    /// (ready, done, paused, stopped), so the pill reads as one signal.
    fn label_color(self, cx: &App) -> Hsla {
        match self {
            BadgeTone::Ready | BadgeTone::Done => gpui_kit::rgb(color::SUCCESS).into(),
            BadgeTone::Recording | BadgeTone::InProgress => cx.theme().muted_foreground,
            BadgeTone::Paused => gpui_kit::rgb(color::WARNING).into(),
            BadgeTone::Stopped => gpui_kit::rgb(color::DANGER).into(),
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

    /// The live transport states must never share a colour.
    #[test]
    fn every_live_state_has_its_own_colour() {
        let tones = [BadgeTone::Ready, BadgeTone::Recording, BadgeTone::Paused];
        for (index, left) in tones.iter().enumerate() {
            for right in &tones[index + 1..] {
                assert_ne!(left.role_rgb(), right.role_rgb(), "{left:?} vs {right:?}");
            }
        }
    }

    /// A take the user ended early wears the danger role, so an interrupted
    /// transcription never reads as a completed one.
    #[test]
    fn a_take_ended_early_is_danger() {
        assert_eq!(BadgeTone::Stopped.role_rgb(), Some(color::DANGER));
        assert_ne!(
            BadgeTone::Stopped.role_rgb(),
            BadgeTone::Done.role_rgb(),
            "an interrupted take must not read as done"
        );
        assert_eq!(
            BadgeTone::Stopped.role_rgb(),
            BadgeTone::Recording.role_rgb(),
            "the same danger the recording state uses"
        );
    }

    /// A decode borrows the paused amber while it runs and the success green
    /// once it is done.
    #[test]
    fn a_file_decode_is_amber_while_running_and_green_when_done() {
        assert_eq!(BadgeTone::InProgress.role_rgb(), Some(color::WARNING));
        assert_eq!(BadgeTone::Done.role_rgb(), Some(color::SUCCESS));
        assert_eq!(
            BadgeTone::InProgress.role_rgb(),
            BadgeTone::Paused.role_rgb(),
            "the working amber"
        );
        assert_eq!(
            BadgeTone::Done.role_rgb(),
            BadgeTone::Ready.role_rgb(),
            "the resting success green"
        );
        assert_ne!(BadgeTone::InProgress.role_rgb(), BadgeTone::Done.role_rgb());
    }
}
