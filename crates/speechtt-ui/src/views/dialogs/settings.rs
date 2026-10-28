//! Settings dialog, per `mockups/dialogs/settings.png`.
//!
//! The dialog edits a UI-only draft (see `controller::SettingsDraft`); saving
//! stages a `SettingsChange` through `SpeechTtApp::apply_settings_change`.
//! T0009 replaces that hook with the Rust settings store.

use gpui_kit::assets::IconName;
use gpui_kit::base::{h_flex, v_flex};
use gpui_kit::component::button::Button;
use gpui_kit::component::button::ButtonVariants as _;
use gpui_kit::component::input::Input;
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Icon, Sizable as _};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, Context, FontWeight, IntoElement, div, px, rems};

use crate::components::{ActionButton, AppFooter, SettingRow, StatusBadge, TranscriptionModeBadge};
use crate::tokens::{color, radius, rem};
use crate::views::dialogs::{DialogShell, DialogWindow};

pub(crate) fn render(dialog: &DialogWindow, cx: &mut Context<DialogWindow>) -> AnyElement {
    let (
        has_provider,
        catalogue_loaded,
        provider_label,
        model_label,
        model_mode,
        connected,
        valid,
        key_env,
        unavailable,
        revealed,
        api_key_input,
        reveal_enabled,
        stored_key_label,
        stored_key_hint,
    ) = {
        let app = dialog.app().read(cx);
        let controller = app.controller();
        let has_provider = app.settings_draft().provider.is_some();
        let catalogue_loaded = controller.catalogue_loaded();
        let provider_label = controller
            .settings_provider_label()
            .unwrap_or(if catalogue_loaded {
                "Choose provider"
            } else {
                "Loading providers…"
            });
        let model_label = controller
            .settings_model_label()
            .unwrap_or(if has_provider {
                "Choose model"
            } else {
                "Choose provider first"
            });
        let unavailable = app
            .settings_draft()
            .provider
            .as_deref()
            .and_then(|provider| controller.provider_available(provider))
            == Some(false);
        (
            has_provider,
            catalogue_loaded,
            provider_label,
            model_label,
            controller.settings_model_mode(),
            controller.settings_connected(),
            controller.settings_draft_valid(),
            controller
                .settings_api_key_env()
                .map(std::string::ToString::to_string)
                .unwrap_or_else(|| "Select a provider to see its key".to_owned()),
            unavailable,
            app.settings_key_revealed(),
            app.settings_api_key_input().clone(),
            app.settings_key_reveal_enabled(),
            app.settings_stored_key_label(),
            app.settings_stored_key_hint(),
        )
    };

    let mut provider_select = Button::new("field-settings-provider")
        .secondary()
        .small()
        .label(provider_label)
        .dropdown_caret(true)
        .disabled(!catalogue_loaded)
        .on_click(cx.listener(|this, _event, _window, cx| this.cycle_provider(cx)));
    if !catalogue_loaded {
        provider_select = provider_select.disabled(true);
    }
    let mut model_select = Button::new("field-settings-model")
        .secondary()
        .small()
        .label(model_label)
        .dropdown_caret(true)
        .disabled(!has_provider)
        .on_click(cx.listener(|this, _event, _window, cx| this.cycle_model(cx)));
    if !has_provider {
        model_select = model_select.disabled(true);
    }

    let model_control = {
        let mut row = h_flex().gap_2().items_center().child(model_select);
        if let Some(mode) = model_mode {
            row = row.child(TranscriptionModeBadge::new(mode));
        }
        row
    };

    let badge = if connected {
        h_flex()
            .gap_1p5()
            .items_center()
            .child(
                div()
                    .size(px(5.))
                    .rounded(px(2.5))
                    .bg(gpui_kit::rgb(color::SUCCESS)),
            )
            .child(
                div()
                    .text_size(rems(rem::XS))
                    .font_weight(FontWeight::BOLD)
                    .text_color(gpui_kit::rgb(color::SUCCESS))
                    .child("CONNECTED"),
            )
            .into_any_element()
    } else {
        StatusBadge::new("NOT CONNECTED").into_any_element()
    };

    let mut reveal = Button::new("btn-settings-reveal")
        .ghost()
        .small()
        .label(if revealed && reveal_enabled {
            "Hide"
        } else {
            "Reveal"
        })
        .on_click(cx.listener(|this, _event, window, cx| this.toggle_key_reveal(window, cx)));
    if !reveal_enabled {
        reveal = reveal.disabled(true);
    }

    let mut api_key = v_flex()
        .w_full()
        .gap_2()
        .py_3()
        .border_b_1()
        .border_color(cx.theme().border)
        .child(
            h_flex()
                .w_full()
                .justify_between()
                .items_center()
                .gap_4()
                .child(
                    v_flex()
                        .flex_shrink_1()
                        .min_w_0()
                        .gap_1()
                        .child(
                            div()
                                .text_size(rems(rem::MD))
                                .font_weight(FontWeight::MEDIUM)
                                .child("API key"),
                        )
                        .child(
                            div()
                                .font_family(cx.theme().mono_font_family.clone())
                                .text_size(rems(rem::MD))
                                .text_color(cx.theme().muted_foreground)
                                .child(key_env),
                        ),
                )
                .child(badge),
        )
        .child(
            Input::new(&api_key_input)
                .id("field-settings-api-key")
                .w_full()
                .suffix(reveal),
        );

    if let Some(stored_key) = stored_key_label {
        let mut stored = v_flex().gap_1().child(
            div()
                .font_family(cx.theme().mono_font_family.clone())
                .text_size(rems(rem::SM))
                .text_color(cx.theme().foreground)
                .child(stored_key),
        );
        if let Some(hint) = stored_key_hint {
            stored = stored.child(
                div()
                    .text_size(rems(rem::XS))
                    .text_color(cx.theme().muted_foreground)
                    .child(hint),
            );
        }
        api_key = api_key.child(stored);
    }

    let note = h_flex()
        .w_full()
        .items_start()
        .gap_2()
        .p_2()
        .rounded(px(radius::MD))
        .bg(cx.theme().secondary)
        .child(
            Icon::new(IconName::Info)
                .small()
                .text_color(gpui_kit::rgb(color::INFO)),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_size(rems(rem::XS))
                .text_color(cx.theme().muted_foreground)
                .child(if unavailable {
                    "This provider is not connected. Add an API key to enable it."
                } else {
                    "No API key? SpeechTT falls back to a local model and keeps working fully offline."
                }),
        );

    let footer = AppFooter::new()
        .action(
            ActionButton::secondary("btn-settings-cancel", "Cancel")
                .on_click(cx.listener(|this, _event, window, cx| this.close(window, cx))),
        )
        .action(
            ActionButton::primary("btn-settings-save", "Save changes")
                .disabled(!valid)
                .on_click(cx.listener(|this, _event, window, cx| this.save(window, cx))),
        );

    DialogShell::new()
        .body(
            SettingRow::new("Transcription Service", "The default backend API provider")
                .prominent()
                .control(provider_select),
        )
        .body(
            SettingRow::new("Default Model", "Which engine transcribes your voice")
                .prominent()
                .control(model_control),
        )
        .body(api_key)
        .body(note)
        .footer(footer)
        .into_any_element()
}
