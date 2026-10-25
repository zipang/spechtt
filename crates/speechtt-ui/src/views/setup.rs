//! Setup screens: the empty prompt and the new-session form.

use gpui_kit::assets::IconName;
use gpui_kit::base::{h_flex, v_flex};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::Input;
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Sizable as _};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, Context, FontWeight, div, rems, rgb};
use speechtt_core::session::OutputTarget;

use crate::app::SpeechTtApp;
use crate::components::{ChoicePill, SettingRow, StatusBadge, TransportButton};
use crate::layout::ScreenShell;
use crate::tokens::{color, rem};

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

/// Setup form: provider, model, title and output target.
pub(crate) fn render_form(app: &mut SpeechTtApp, cx: &mut Context<SpeechTtApp>) -> AnyElement {
    let (provider_label, model_label, can_create, has_provider, has_model, output) = {
        let controller = app.controller();
        let session_name = app.session_name_input().read(cx).value();
        (
            controller.provider_label().unwrap_or("Choose…"),
            controller.model_label().unwrap_or("Choose…"),
            controller.is_draft_complete_with_name(&session_name),
            controller.has_provider(),
            controller.has_model(),
            controller.selected_output().cloned(),
        )
    };

    let provider_pill = ChoicePill::new("field-provider", provider_label)
        .selected(has_provider)
        .on_click(cx.listener(|this, _event, _window, cx| this.cycle_provider(cx)));
    let model_pill = ChoicePill::new("field-model", model_label)
        .selected(has_model)
        .on_click(cx.listener(|this, _event, _window, cx| this.cycle_model(cx)));
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

    let cancel = Button::new("btn-cancel-session")
        .secondary()
        .small()
        .label("Cancel")
        .on_click(cx.listener(|this, _event, window, cx| this.cancel_form(window, cx)));
    let create = Button::new("btn-create-session")
        .primary()
        .small()
        .label("Create session")
        .disabled(!can_create)
        .on_click(cx.listener(|this, _event, _window, cx| this.create_session(cx)));

    // Compact: the 480×280 widget body must fit pill + 4 rows + actions
    // without clipping.
    ScreenShell::new(StatusBadge::accent(
        "NEW SESSION",
        rgb(color::BRAND_ACCENT).into(),
    ))
    .gap(rems(0.125))
    .body_gap(rems(0.125))
    .body(
        SettingRow::new(
            "TRANSCRIPTION SERVICE",
            "Which engine transcribes your voice",
        )
        .control(provider_pill),
    )
    .body(SettingRow::new("MODEL", "Used for this session").control(model_pill))
    .body(
        SettingRow::new("SESSION TITLE", "Names this take and its default file").control(
            Input::new(app.session_name_input())
                .id("input-session-name")
                .w_full(),
        ),
    )
    .body(
        SettingRow::new("OUTPUT", "Keep it in the app or write a file")
            .control(h_flex().gap_1().child(memory_pill).child(file_pill)),
    )
    .body(h_flex().justify_end().gap_2().child(cancel).child(create))
    .into_any_element()
}
