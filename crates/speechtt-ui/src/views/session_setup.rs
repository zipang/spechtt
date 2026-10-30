//! Session setup: the provider + model form.
//!
//! Shown only when the persisted settings do not resolve to a real
//! provider/model pair. These are the only two fields a take has; the saved
//! file gives the take its name, so there is no name or output row.

use gpui_kit::base::{h_flex, v_flex};
use gpui_kit::component::button::Button;
use gpui_kit::component::button::ButtonVariants as _;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{Disableable as _, Sizable as _};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, Context};

use crate::app::SpeechTtApp;
use crate::components::{SettingRow, TranscriptionModeBadge};

/// Cancel / Continue live in the window-level [`crate::components::AppFooter`]
/// sibling, so the actions never scroll away and are separated from the body
/// by the same hairline as `AppHeader`.
pub(crate) fn render(app: &SpeechTtApp, cx: &mut Context<SpeechTtApp>) -> AnyElement {
    let (provider_label, model_label, model_mode, has_provider) = {
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
        .into_any_element()
}
