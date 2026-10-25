//! Stopped screen: the finished take, split by output mode.

use gpui_kit::assets::IconName;
use gpui_kit::base::h_flex;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{ActiveTheme as _, Sizable as _};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, Context, FontWeight, div, rems};
use speechtt_core::session::OutputTarget;

use crate::app::SpeechTtApp;
use crate::components::{CaptionWell, IconAction, StatusBadge};
use crate::layout::{ScreenShell, SurfaceBox};
use crate::tokens::rem;

/// Finished take. File takes expose the written path and an open action;
/// memory takes expose the transcript with copy/save.
pub(crate) fn render_stopped(app: &mut SpeechTtApp, cx: &mut Context<SpeechTtApp>) -> AnyElement {
    let (is_file, path, committed, live) = {
        let Some(result) = app.controller().finished_take() else {
            return StatusBadge::new("STOPPED").into_any_element();
        };
        let is_file = matches!(result.session().output, OutputTarget::File(_));
        let path = match &result.session().output {
            OutputTarget::File(path) => path.clone(),
            OutputTarget::Memory => String::new(),
        };
        (
            is_file,
            path,
            result.committed().to_string(),
            result.live().to_string(),
        )
    };

    let controls = if is_file {
        // TODO(Phase 5): open the written file.
        h_flex()
            .gap_2()
            .child(icon_action("btn-open-file", IconName::FileText))
    } else {
        // TODO(Phase 5): clipboard and save adapters.
        h_flex()
            .gap_2()
            .child(icon_action("btn-copy", IconName::Copy))
            .child(icon_action("btn-save", IconName::HardDriveDownload))
    };

    let body = if is_file {
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
        .on_click(cx.listener(|this, _event, _window, cx| this.new_take(cx)));

    ScreenShell::new(StatusBadge::new("STOPPED"))
        .header_trailing(new_take)
        .gap(rems(0.5))
        .body_gap(rems(0.5))
        .body(body)
        .footer(h_flex().items_end().justify_end().child(controls))
        .into_any_element()
}

fn icon_action(id: &'static str, icon: IconName) -> IconAction {
    IconAction::new(id, icon).on_click(|_event, _window, _cx| {})
}
