//! Chip telling the user how the selected model consumes audio.
//!
//! Streaming models produce a live transcript over a WebSocket; segmented
//! models batch audio and return text after each silence-aligned segment. The
//! distinction is invisible otherwise, so it is rendered next to every model
//! choice and in the recording header.
//!
//! Text is always rendered (never an icon alone), so the chip needs no tooltip
//! and stays readable in a screenshot or for assistive tooling.

use gpui_kit::assets::IconName;
use gpui_kit::base::h_flex;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::{Icon, Sizable as _, Size};
use gpui_kit::prelude::*;
use gpui_kit::{App, Hsla, IntoElement, RenderOnce, Window, px};

use speechtt_core::protocol::TranscriptionMode;

use crate::components::Label;
use crate::tokens::{color, radius};

/// Human-readable label for a transcription mode.
#[must_use]
pub fn mode_label(mode: TranscriptionMode) -> &'static str {
    match mode {
        TranscriptionMode::Streaming => "Stream",
        TranscriptionMode::Segmented => "Batch",
    }
}

fn mode_icon(mode: TranscriptionMode) -> IconName {
    match mode {
        TranscriptionMode::Streaming => IconName::Zap,
        TranscriptionMode::Segmented => IconName::Scissors,
    }
}

fn mode_color(mode: TranscriptionMode, cx: &App) -> Hsla {
    match mode {
        TranscriptionMode::Streaming => gpui_kit::rgb(color::SUCCESS).into(),
        TranscriptionMode::Segmented => cx.theme().muted_foreground,
    }
}

/// `⚡ Stream` or `✂ Batch` chip.
#[derive(IntoElement)]
pub struct TranscriptionModeBadge {
    mode: TranscriptionMode,
}

impl TranscriptionModeBadge {
    pub fn new(mode: TranscriptionMode) -> Self {
        Self { mode }
    }
}

impl RenderOnce for TranscriptionModeBadge {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let tone = mode_color(self.mode, cx);

        h_flex()
            .gap_1()
            .items_center()
            .px_2()
            .py_1()
            .rounded(px(radius::SM))
            .bg(cx.theme().secondary)
            .child(
                Icon::new(mode_icon(self.mode))
                    .with_size(Size::XSmall)
                    .text_color(tone),
            )
            .child(Label::new(mode_label(self.mode)).bold().tone(tone))
    }
}
