//! Setup screens: the empty prompt and the new-session form.

use gpui_kit::assets::IconName;
use gpui_kit::base::{h_flex, v_flex};
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::button::Button;
use gpui_kit::component::button::ButtonVariants as _;
use gpui_kit::component::input::Input;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{Disableable as _, Sizable as _};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, Context, FontWeight, div, rems};
use speechtt_core::session::OutputTarget;

use crate::app::SpeechTtApp;
use crate::components::{
    ChoicePill, SettingRow, StatusBadge, TranscriptionModeBadge, TransportButton,
};
use crate::layout::ScreenShell;
use crate::tokens::rem;

/// Blank state: the large "new session" affordance.
pub(crate) fn render_prompt(cx: &mut Context<SpeechTtApp>) -> AnyElement {
    ScreenShell::new(StatusBadge::new("NO SESSION"))
        .centered()
        .body_gap(rems(1.0))
        .body(
            TransportButton::new("btn-new-session", IconName::Plus)
                .on_click(cx.listener(|this, _event, window, cx| this.open_form(window, cx))),
        )
        .body(
            v_flex()
                .items_center()
                .gap_1()
                .child(
                    div()
                        .text_size(rems(rem::LG))
                        .font_weight(FontWeight::MEDIUM)
                        .child("New Session"),
                )
                .child(
                    div()
                        .text_size(rems(rem::SM))
                        .text_color(cx.theme().muted_foreground)
                        .child("Choose a service, model and output"),
                ),
        )
        .into_any_element()
}

/// New-session form, visually aligned with the Settings dialog.
///
/// The four prominent rows scroll inside the body; Cancel / Create session
/// live in the window-level [`crate::components::AppFooter`] sibling, so the
/// actions never scroll away and are separated from the body by the same
/// hairline as `AppHeader`.
pub(crate) fn render_form(app: &SpeechTtApp, cx: &mut Context<SpeechTtApp>) -> AnyElement {
    let (provider_label, model_label, model_mode, has_provider, output) = {
        let controller = app.controller();
        let has_provider = controller.has_provider();
        (
            controller.provider_label().unwrap_or("Choose provider"),
            controller.model_label().unwrap_or(if has_provider {
                "Choose model"
            } else {
                "Choose provider first"
            }),
            controller.draft_model_mode(),
            has_provider,
            controller.selected_output().cloned(),
        )
    };

    let provider_select = Button::new("field-provider")
        .secondary()
        .small()
        .label(provider_label)
        .dropdown_caret(true)
        .on_click(cx.listener(|this, _event, _window, cx| this.cycle_provider(cx)));
    let model_select = Button::new("field-model")
        .secondary()
        .small()
        .label(model_label)
        .dropdown_caret(true)
        .disabled(!has_provider)
        .on_click(cx.listener(|this, _event, _window, cx| this.cycle_model(cx)));

    let model_control = {
        let mut row = h_flex().gap_2().items_center().child(model_select);
        if let Some(mode) = model_mode {
            row = row.child(TranscriptionModeBadge::new(mode));
        }
        row
    };

    let memory_pill = ChoicePill::new("field-output-memory", "In memory")
        .selected(output == Some(OutputTarget::Memory))
        .on_click(
            cx.listener(|this, _event, _window, cx| this.select_output(OutputTarget::Memory, cx)),
        );
    let file_pill = ChoicePill::new("field-output-file", "To file")
        .selected(matches!(output, Some(OutputTarget::File(_))))
        .on_click(cx.listener(|this, _event, _window, cx| {
            this.select_output(OutputTarget::File(String::new()), cx)
        }));

    v_flex()
        .size_full()
        .flex_1()
        .min_h_0()
        .w_full()
        .gap_2()
        .p_5()
        .overflow_y_scrollbar()
        .child(
            SettingRow::new(
                "Transcription Service",
                "Which engine transcribes your voice",
            )
            .prominent()
            .control(provider_select),
        )
        .child(
            SettingRow::new("Model", "Used for this session")
                .prominent()
                .control(model_control),
        )
        .child(
            SettingRow::new("Session Title", "Names this take and its default file")
                .prominent()
                .control(
                    Input::new(app.session_name_input())
                        .id("input-session-name")
                        .w_full(),
                ),
        )
        .child(
            SettingRow::new("Output", "Keep it in the app or write a file")
                .prominent()
                .control(h_flex().gap_1().child(memory_pill).child(file_pill)),
        )
        .into_any_element()
}
