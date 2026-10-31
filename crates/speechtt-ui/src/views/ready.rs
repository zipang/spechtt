//! Ready screen: one affordance, one click away from the take.
//!
//! Live shows the mic button that starts dictating; File shows the browse
//! button (which re-opens the picker) plus a link back to the mode chooser.

use gpui_kit::assets::IconName;
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, Context};

use crate::app::SpeechTtApp;
use crate::components::{ActionButton, BadgeTone, Label, RoundButton, StatusBadge};
use crate::controller::SessionMode;
use crate::layout::ScreenShell;
use crate::tokens::space;

pub(crate) fn render(app: &SpeechTtApp, cx: &mut Context<SpeechTtApp>) -> AnyElement {
    let shell = ScreenShell::new(StatusBadge::tone("READY", BadgeTone::Ready))
        .centered()
        .gap(space::SM)
        .body_gap(space::BASE);

    match app.controller().mode() {
        SessionMode::Live => shell
            .body(
                RoundButton::new("btn-record", IconName::Mic)
                    .on_press(press_ripple(cx, "btn-record"))
                    .on_click(cx.listener(|this, _event, _window, cx| this.start_recording(cx))),
            )
            .body(caption("Press to start dictating"))
            .into_any_element(),
        SessionMode::File => shell
            .body(
                RoundButton::new("btn-browse-file", IconName::FileUp)
                    .on_press(press_ripple(cx, "btn-browse-file"))
                    .on_click(
                        cx.listener(|this, _event, _window, cx| this.pick_transcription_file(cx)),
                    ),
            )
            .body(caption("Browse a file"))
            .body(
                ActionButton::secondary("btn-change-mode", "CHANGE MODE")
                    .on_click(cx.listener(|this, _event, _window, cx| this.change_mode(cx))),
            )
            .into_any_element(),
    }
}

/// Press handler that starts the window ripple of round button `id`.
fn press_ripple(
    cx: &mut Context<SpeechTtApp>,
    id: &'static str,
) -> impl Fn(&gpui_kit::MouseDownEvent, &mut gpui_kit::Window, &mut gpui_kit::App) + 'static {
    cx.listener(
        move |this, _event: &gpui_kit::MouseDownEvent, _window, cx| {
            this.ripple(id, cx);
        },
    )
}

fn caption(text: &'static str) -> AnyElement {
    Label::new(text).muted().into_any_element()
}
