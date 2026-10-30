//! Ready screen: one affordance, one click away from the take.
//!
//! Live shows the mic button that starts dictating; File shows the browse
//! button (which re-opens the picker) plus a link back to the mode chooser.

use gpui_kit::assets::IconName;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, Context, div, rems};

use crate::app::SpeechTtApp;
use crate::components::{ActionButton, BadgeTone, RoundButton, StatusBadge};
use crate::controller::SessionMode;
use crate::layout::ScreenShell;
use crate::tokens::rem;

pub(crate) fn render(app: &SpeechTtApp, cx: &mut Context<SpeechTtApp>) -> AnyElement {
    let shell = ScreenShell::new(StatusBadge::tone("READY", BadgeTone::Ready))
        .centered()
        .gap(rems(0.5))
        .body_gap(rems(1.0));

    match app.controller().mode() {
        SessionMode::Live => shell
            .body(
                RoundButton::new("btn-record", IconName::Mic)
                    .pulse(app.pulse_count("btn-record"))
                    .on_click(cx.listener(|this, _event, _window, cx| this.start_recording(cx))),
            )
            .body(caption(cx, "Press to start dictating"))
            .into_any_element(),
        SessionMode::File => shell
            .body(
                RoundButton::new("btn-browse-file", IconName::FileUp)
                    .pulse(app.pulse_count("btn-browse-file"))
                    .on_click(
                        cx.listener(|this, _event, _window, cx| this.pick_transcription_file(cx)),
                    ),
            )
            .body(caption(cx, "Browse a file"))
            .body(
                ActionButton::secondary("btn-change-mode", "CHANGE MODE")
                    .on_click(cx.listener(|this, _event, _window, cx| this.change_mode(cx))),
            )
            .into_any_element(),
    }
}

fn caption(cx: &mut Context<SpeechTtApp>, text: &'static str) -> AnyElement {
    div()
        .text_size(rems(rem::SM))
        .text_color(cx.theme().muted_foreground)
        .child(text)
        .into_any_element()
}
