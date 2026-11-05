//! Session end: the finished take, its cleanup and its save state.
//!
//! An unsaved take (live or file) starts with Clean up, Copy and Save: nothing
//! is written automatically. After Save the screen shows the written path and an
//! Open File action backed by the injected opener. "Copied"/"Saved"/"Opened"/
//! "Cleaned"/"Original restored" feedback comes from the controller's transient
//! output status.
//!
//! The magic-wand action repairs the transcript through the worker's language
//! model and then offers an undo; while the request is outstanding the status
//! line reads "Cleaning up…" and the action is disabled. Copy, Save and the
//! automatic paste all read the text that is on screen, so a repaired take is
//! what leaves the widget and an undo puts the original back.
//!
//! A take that recorded nothing is a normal outcome: Clean up, Copy and Save are
//! disabled and a muted label replaces the transient status area, so no failure
//! is ever implied.
//!
//! Going home is the shared footer's back action, in the same place as on every
//! other screen. The screen carries no status pill: the take is over, and what is
//! left to do is clean it up, copy it or save it.

use gpui_kit::assets::IconName;
use gpui_kit::base::h_flex;
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, Context};

use crate::app::SpeechTtApp;
use crate::components::{CaptionWell, IconAction, Label, ScreenFooter};
use crate::controller::CleanStatus;
use crate::layout::{ScreenShell, SurfaceBox};
use crate::tokens::space;

/// Finished take. A written take exposes the path and an open action; an
/// unsaved take exposes cleanup, copy and save.
pub(crate) fn render(app: &mut SpeechTtApp, cx: &mut Context<SpeechTtApp>) -> AnyElement {
    let Some(result) = app.controller().finished_take() else {
        return ScreenShell::headerless().into_any_element();
    };

    let empty = app.controller().finished_take_is_empty();
    let written_path = app
        .controller()
        .written_path()
        .map(|path| path.to_string_lossy().into_owned());
    let status = app.controller().output_status().map(ToOwned::to_owned);
    let committed = result.committed().clone();
    let live = result.live().clone();

    let clean_status = app.clean_status();
    let cleanable = app.can_clean_transcript();
    let cleaning = clean_status == CleanStatus::Running;
    let cleaned = clean_status == CleanStatus::Cleaned;

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
        // The trailing zone renders left to right in insertion order, so the
        // wand and its undo sit to the left of Copy and Save.
        let mut controls = h_flex().gap_2().child(
            IconAction::new("btn-clean-transcript", IconName::WandSparkles)
                .label("Clean up text")
                .disabled(!cleanable)
                .on_click(cx.listener(|this, _event, _window, cx| {
                    this.clean_transcript(cx);
                })),
        );
        if cleaned {
            controls = controls.child(
                IconAction::new("btn-undo-clean", IconName::RotateCcw)
                    .label("Undo cleanup")
                    .on_click(cx.listener(|this, _event, _window, cx| {
                        this.undo_clean_transcript(cx);
                    })),
            );
        }
        controls
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

    let mut shell = ScreenShell::headerless()
        .gap(space::SM)
        .body_gap(space::SM)
        .body(body);

    if empty {
        shell = shell.body(Label::new("Nothing was recorded.").muted());
    } else if cleaning {
        shell = shell.body(Label::new("Cleaning up…").muted());
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
