//! Stopped screen: the finished take and its save state.
//!
//! An unsaved take (live or file) starts with Copy/Save: nothing is written
//! automatically. After Save the screen shows the written path and an Open
//! File action backed by the injected opener. "Copied"/"Saved"/"Opened"
//! feedback comes from the controller's transient output status.
//!
//! A take that recorded nothing is a normal outcome: Copy and Save are
//! disabled and a muted label replaces the transient status area, so no
//! failure is ever implied.
//!
//! Going home is the shared footer's back action (T0021); the header no longer
//! carries its own "New take" button.

use gpui_kit::assets::IconName;
use gpui_kit::base::h_flex;
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, Context};

use crate::app::SpeechTtApp;
use crate::components::{CaptionWell, IconAction, Label, ScreenFooter, StatusBadge};
use crate::layout::{ScreenShell, SurfaceBox};
use crate::tokens::space;

/// Finished take. A written take exposes the path and an open action; an
/// unsaved take exposes copy/save.
pub(crate) fn render(app: &mut SpeechTtApp, cx: &mut Context<SpeechTtApp>) -> AnyElement {
    let Some(result) = app.controller().finished_take() else {
        return StatusBadge::new("STOPPED").into_any_element();
    };

    let empty = app.controller().finished_take_is_empty();
    let written_path = app
        .controller()
        .written_path()
        .map(|path| path.to_string_lossy().into_owned());
    let status = app.controller().output_status().map(ToOwned::to_owned);
    let committed = result.committed().clone();
    let live = result.live().clone();

    // Save turns the transcript into a written-path view.
    let path = written_path.unwrap_or_default();
    let written = !path.is_empty();

    let controls = if written {
        h_flex().gap_2().child(
            IconAction::new("btn-open-file", IconName::FileText)
                .label("Open file")
                .on_click(cx.listener(|this, _event, _window, cx| {
                    this.open_saved_file(cx);
                })),
        )
    } else {
        h_flex()
            .gap_2()
            .child(
                IconAction::new("btn-copy", IconName::Copy)
                    .label("Copy")
                    .disabled(empty)
                    .on_click(cx.listener(|this, _event, _window, cx| {
                        this.copy_transcript(cx);
                    })),
            )
            .child(
                IconAction::new("btn-save", IconName::HardDriveDownload)
                    .label("Save")
                    .disabled(empty)
                    .on_click(cx.listener(|this, _event, _window, cx| {
                        this.save_transcript(cx);
                    })),
            )
    };

    let body = if written {
        SurfaceBox::new()
            .child(Label::new("TRANSCRIPT WRITTEN TO").bold().muted())
            .child(Label::new(path).mono())
            .into_any_element()
    } else {
        CaptionWell::new(committed, live).into_any_element()
    };

    let mut shell = ScreenShell::new(StatusBadge::new("STOPPED"))
        .gap(space::SM)
        .body_gap(space::SM)
        .body(body);

    if empty {
        shell = shell.body(Label::new("Nothing was recorded.").muted());
    } else if let Some(status) = status {
        shell = shell.body(Label::new(status).muted());
    }

    shell
        .footer(
            ScreenFooter::new()
                .back(
                    "btn-new-take",
                    "Back",
                    cx.listener(|this, _event, _window, cx| {
                        this.go_back(cx);
                    }),
                )
                .trailing(controls),
        )
        .into_any_element()
}
