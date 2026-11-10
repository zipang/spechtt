//! Settings dialog, per `mockups/dialogs/settings.png`.
//!
//! Seven rows, top to bottom: **Dictation Language** (Auto first — it is what the
//! transcriber is told, so it leads the model rows), **Transcription Service**,
//! the two take defaults, **Cleanup Model**, **Bun path** and the API key. Each
//! dropdown is a component owned by `views::dialogs::window`; this file only
//! composes the rows and passes the current values.
//!
//! The `Bun path` row is a `SettingRow` with an `Input`: empty means the runtime
//! is resolved automatically, a value is the user's explicit override. The
//! provider row's placeholder and the failure note come from the controller, so
//! an unavailable backend states its cause instead of implying that a catalogue
//! is still loading, and the API-key badge reports `NOT VERIFIED` until a
//! catalogue has arrived.
//!
//! The dialog edits a draft (see `controller::SettingsDraft`); saving stages a
//! `SettingsChange` through `SpeechTtApp::apply_settings_change`, which writes
//! it to the Rust settings store.
//!
//! Rendered by `views::dialogs::window` for the `Dialog::Settings` window.

use gpui_kit::base::{h_flex, v_flex};
use gpui_kit::component::button::Button;
use gpui_kit::component::button::ButtonVariants as _;
use gpui_kit::component::input::Input;
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Sizable as _};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, Context, IntoElement};

use crate::components::{
    ActionButton, AppFooter, Body, InfoNote, Label, LanguageSelect, ModelSelect, ProviderSelect,
    SettingRow, StatusBadge, StatusDot, TranscriptionModeBadge,
};
use speechtt_core::protocol::TranscriptionMode;

use crate::controller::ConnectionState;
use crate::tokens::color;
use crate::views::dialogs::{DialogShell, DialogWindow};

pub(crate) fn render(dialog: &DialogWindow, cx: &mut Context<DialogWindow>) -> AnyElement {
    let (
        has_provider,
        catalogue_loaded,
        streaming_mode,
        batch_mode,
        connection,
        valid,
        key_env,
        unavailable,
        revealed,
        api_key_input,
        bun_path_input,
        backend_note,
        provider_placeholder,
        reveal_enabled,
        stored_key_label,
        stored_key_hint,
        has_cleanup_models,
        resolved_cleanup_model,
    ) = {
        let app = dialog.app().read(cx);
        let controller = app.controller();
        let has_provider = app.settings_draft().provider.is_some();
        let catalogue_loaded = controller.catalogue_loaded();
        let unavailable = app
            .settings_draft()
            .provider
            .as_deref()
            .and_then(|provider| controller.provider_available(provider))
            == Some(false);
        (
            has_provider,
            catalogue_loaded,
            controller.settings_streaming_model_mode(),
            controller.settings_batch_model_mode(),
            controller.settings_connection_state(),
            controller.settings_can_save(),
            controller
                .settings_api_key_env()
                .map(std::string::ToString::to_string)
                .unwrap_or_else(|| "Select a provider to see its key".to_owned()),
            unavailable,
            app.settings_key_revealed(),
            app.settings_api_key_input().clone(),
            app.settings_bun_path_input().clone(),
            controller.settings_backend_note(),
            controller.settings_provider_placeholder(),
            app.settings_key_reveal_enabled(),
            app.settings_stored_key_label(),
            app.settings_stored_key_hint(),
            !controller.cleanup_models().is_empty(),
            controller
                .default_cleanup_model()
                .map(std::string::ToString::to_string),
        )
    };

    let provider_select =
        ProviderSelect::new("field-settings-provider", dialog.selects().provider())
            .placeholder(provider_placeholder)
            .disabled(!catalogue_loaded);
    let model_placeholder = if has_provider {
        "Choose model"
    } else {
        "Choose provider first"
    };
    let streaming_select =
        ModelSelect::new("field-settings-streaming", dialog.selects().streaming())
            .placeholder(model_placeholder)
            .disabled(!has_provider);
    let batch_select = ModelSelect::new("field-settings-batch", dialog.selects().batch())
        .placeholder(model_placeholder)
        .disabled(!has_provider);

    let model_control = |select: AnyElement, mode: Option<TranscriptionMode>| {
        let mut row = h_flex().gap_2().items_center().child(select);
        if let Some(mode) = mode {
            row = row.child(TranscriptionModeBadge::new(mode));
        }
        row.into_any_element()
    };
    let streaming_control = model_control(streaming_select.into_any_element(), streaming_mode);
    let batch_control = model_control(batch_select.into_any_element(), batch_mode);

    let language_select =
        LanguageSelect::new("field-settings-language", dialog.selects().language());

    // A worker that discovered no language model leaves the row without a
    // menu: the resolved default id is shown instead, so the setting still
    // reads as configured.
    let cleanup_control = if has_cleanup_models {
        // The trigger names the model that would actually be used, so the row
        // reads as configured on an install that never chose one. The stored
        // value stays empty in that case: the worker keeps owning its default.
        let placeholder = resolved_cleanup_model.map_or_else(
            || "Use DEFAULT_LANGUAGE_MODEL".to_owned(),
            |model| format!("Use {model} (default)"),
        );
        ModelSelect::new("field-settings-cleanup", dialog.selects().cleanup())
            .placeholder(placeholder)
            .into_any_element()
    } else {
        Label::new(resolved_cleanup_model.unwrap_or_else(|| "Not configured".to_owned()))
            .mono()
            .into_any_element()
    };

    let badge = match connection {
        ConnectionState::Connected => h_flex()
            .gap_1p5()
            .items_center()
            .child(StatusDot::new(gpui_kit::rgb(color::SUCCESS).into()))
            .child(
                Label::new("CONNECTED")
                    .bold()
                    .tone(gpui_kit::rgb(color::SUCCESS).into()),
            )
            .into_any_element(),
        ConnectionState::NotConnected => StatusBadge::new("NOT CONNECTED").into_any_element(),
        // No catalogue has arrived, so availability is not known: the badge
        // must not claim either outcome.
        ConnectionState::Unknown => StatusBadge::new("NOT VERIFIED").into_any_element(),
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
                        .child(Body::new("API key").bold())
                        .child(Body::new(key_env).mono().muted()),
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
        let mut stored = v_flex().gap_1().child(Label::new(stored_key).mono());
        if let Some(hint) = stored_key_hint {
            stored = stored.child(Label::new(hint).muted());
        }
        api_key = api_key.child(stored);
    }

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

    let mut shell = DialogShell::new()
        .body(
            SettingRow::new(
                "Dictation Language",
                "The language you speak; improves accuracy",
            )
            .prominent()
            .control(language_select),
        )
        .body(
            SettingRow::new("Transcription Service", "The default backend API provider")
                .prominent()
                .control(provider_select),
        )
        .body(
            SettingRow::new("Default Streaming Model", "Used for live dictation")
                .prominent()
                .control(streaming_control),
        )
        .body(
            SettingRow::new("Default Batch Model", "Used for audio and video files")
                .prominent()
                .control(batch_control),
        )
        .body(
            SettingRow::new(
                "Cleanup Model",
                "Repairs typos and broken sentences after transcription",
            )
            .prominent()
            .control(cleanup_control),
        )
        .body(
            SettingRow::new(
                "Bun path",
                "The bun runtime the worker runs on; empty finds it automatically",
            )
            .prominent()
            .control(
                Input::new(&bun_path_input)
                    .id("field-settings-bun-path")
                    .w_full(),
            ),
        )
        .body(api_key);

    // The note explains why this provider cannot be used yet, so it is drawn
    // only while the worker reports the provider as unavailable: with the
    // provider connected there is nothing left to explain.
    if unavailable {
        shell = shell.body(
            InfoNote::new("This provider is not connected. Add an API key to enable it.")
                .id("note-settings-provider"),
        );
    }

    // A backend that never started cannot deliver a catalogue, so the note
    // names the cause instead of leaving the provider row loading forever.
    if let Some(note) = backend_note {
        shell = shell.body(InfoNote::new(note).id("note-settings-backend"));
    }

    shell.footer(footer).into_any_element()
}
