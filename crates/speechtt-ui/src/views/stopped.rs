//! Stopped screen: the finished take and its save state.
//!
//! An unsaved take (live or file) starts with Copy/Save: nothing is written
//! automatically. After Save the screen shows the written path and an Open
//! File action backed by the injected opener. "Copied"/"Saved"/"Opened"
//! feedback comes from the controller's transient output status.

use gpui_kit::assets::IconName;
use gpui_kit::base::h_flex;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{ActiveTheme as _, Sizable as _};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, Context, FontWeight, div, rems};

use crate::app::SpeechTtApp;
use crate::components::{CaptionWell, IconAction, StatusBadge};
use crate::layout::{ScreenShell, SurfaceBox};
use crate::tokens::rem;

/// Finished take. A written take exposes the path and an open action; an
/// unsaved take exposes copy/save.
pub(crate) fn render(app: &mut SpeechTtApp, cx: &mut Context<SpeechTtApp>) -> AnyElement {
    let Some(result) = app.controller().finished_take() else {
        return StatusBadge::new("STOPPED").into_any_element();
    };

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
            IconAction::new("btn-open-file", IconName::FileText).on_click(cx.listener(
                |this, _event, _window, cx| {
                    this.open_saved_file(cx);
                },
            )),
        )
    } else {
        h_flex()
            .gap_2()
            .child(
                IconAction::new("btn-copy", IconName::Copy).on_click(cx.listener(
                    |this, _event, _window, cx| {
                        this.copy_transcript(cx);
                    },
                )),
            )
            .child(
                IconAction::new("btn-save", IconName::HardDriveDownload).on_click(cx.listener(
                    |this, _event, _window, cx| {
                        this.save_transcript(cx);
                    },
                )),
            )
    };

    let body = if written {
        SurfaceBox::new()
            .child(
                div()
                    .text_size(rems(rem::XS))
                    .font_weight(FontWeight::BOLD)
                    .text_color(cx.theme().muted_foreground)
                    .child("TRANSCRIPT WRITTEN TO"),
            )
            .child(
                div()
                    .id("result-path")
                    .font_family(cx.theme().mono_font_family.clone())
                    .text_size(rems(rem::SM))
                    .text_color(cx.theme().foreground)
                    .child(path),
            )
            .into_any_element()
    } else {
        CaptionWell::new(committed, live).into_any_element()
    };

    let new_take = Button::new("btn-new-take")
        .secondary()
        .small()
        .label("New take")
        .on_click(cx.listener(|this, _event, _window, cx| {
            this.new_take(cx);
        }));

    let mut shell = ScreenShell::new(StatusBadge::new("STOPPED"))
        .header_trailing(new_take)
        .gap(rems(0.5))
        .body_gap(rems(0.5))
        .body(body);

    if let Some(status) = status {
        shell = shell.body(
            div()
                .text_size(rems(rem::XS))
                .text_color(cx.theme().muted_foreground)
                .child(status),
        );
    }

    shell
        .footer(h_flex().items_end().justify_end().child(controls))
        .into_any_element()
}
