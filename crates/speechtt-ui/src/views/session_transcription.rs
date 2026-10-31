//! File transcription screen: which file, how far the decode has got, and the
//! three actions.
//!
//! A file take has no microphone and no live transcript: the worker decodes the
//! file and transcribes it in one sequential batch run. What the user can watch
//! is the decoded position, so the body is the file's name and a [`ProgressBar`],
//! and the corner times the take like the live screen does.
//!
//! The screen never navigates on its own. When the worker reaches the end of the
//! file the pill changes from `In progress` to `Done`, Stop becomes inert and
//! Continue enables; the user decides when to open the transcript. A take the
//! user stops early reports `STOPPED` instead, so an interrupted transcription
//! never reads as a completed one.
//!
//! Footer zones, left to right:
//!
//! | Zone | Action | Behaviour |
//! |---|---|---|
//! | Leading | Back | discards the take, returns to the mode chooser |
//! | Primary | Stop | ends the decode early, keeping the text produced so far |
//! | Trailing | Continue | opens the result; enabled once the decode has ended |

use gpui_kit::assets::IconName;
use gpui_kit::base::v_flex;
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, Context};

use crate::app::SpeechTtApp;
use crate::components::{
    BadgeTone, IconAction, Label, ProgressBar, ScreenFooter, StatusBadge, TransportHeader,
};
use crate::layout::{ScreenShell, SurfaceBox};
use crate::tokens::space;

pub(crate) fn render(app: &SpeechTtApp, cx: &mut Context<SpeechTtApp>) -> AnyElement {
    let controller = app.controller();
    let decoding = controller.is_file_take();
    let finished = controller.is_awaiting_continue();
    let stopped_early = controller.file_take_stopped_early();
    // The take always names its file: the name is held from the moment the take
    // starts, so it is there while decoding and after.
    let file_name = controller
        .file_display_name()
        .unwrap_or_default()
        .to_owned();
    let (processed, total) = controller.progress().unwrap_or((0.0, None));
    let elapsed = controller.elapsed_seconds();

    // The pill reports the decode: amber while the worker works, green once it
    // has finished, danger when the user ended it early.
    let badge = if decoding {
        StatusBadge::tone("In progress", BadgeTone::InProgress)
    } else if stopped_early {
        StatusBadge::tone("STOPPED", BadgeTone::Stopped)
    } else {
        StatusBadge::tone("Done", BadgeTone::Done)
    };

    let body = SurfaceBox::new()
        .child(Label::new(file_name).mono())
        .child(ProgressBar::new(processed, total))
        .into_any_element();

    let footer = ScreenFooter::new()
        // Back is always live: it discards the take and goes home, whether the
        // worker is still decoding or the result is waiting.
        .back(
            "btn-cancel-file",
            "Back",
            cx.listener(|this, _event, _window, cx| {
                this.cancel_take(cx);
            }),
        )
        // Stop ends the decode early and keeps the partial transcript. It is
        // inert once the worker has finished on its own.
        .primary(
            IconAction::new("btn-stop-transcription", IconName::Square)
                .label("Stop")
                .disabled(!decoding)
                .on_click(cx.listener(|this, _event, _window, cx| {
                    this.finish_take(cx);
                })),
        )
        // Continue only reveals the finished result, so it stays inert until
        // there is one.
        .trailing(
            IconAction::new("btn-continue", IconName::ArrowBigRight)
                .label("Continue")
                .disabled(!finished)
                .on_click(cx.listener(|this, _event, _window, cx| {
                    this.continue_to_end(cx);
                })),
        );

    ScreenShell::new(badge)
        // The corner times the take, exactly as on the live screen; the body
        // reports the file position.
        .header_trailing(TransportHeader::new(elapsed))
        .gap(space::SM)
        .body(v_flex().w_full().child(body))
        .footer(footer)
        .into_any_element()
}
