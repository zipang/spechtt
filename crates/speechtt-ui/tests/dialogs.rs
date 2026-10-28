//! Help and Settings dialog behaviour.
//!
//! The interaction contract is exercised through `SessionController` commands
//! (and one headless render smoke test). No assertion depends on element
//! placement, geometry or labels, so the dialog design stays free to change.

use speechtt_core::protocol::{ModelCapabilityInfo, ProviderCapability, TranscriptionMode};
use speechtt_core::settings::Settings;
use speechtt_ui::controller::{Dialog, STORED_KEY_REVEAL_HINT, SessionController, SettingsChange};

const MIXED_CASE_KEY: &str = "vck_AbCdEfGhIjKlMnOpQrStUvWxYzH0123456789";

/// Catalogue the worker would send after a handshake.
fn catalogue() -> Vec<ProviderCapability> {
    vec![
        ProviderCapability {
            id: "vercel-ai-gateway".to_string(),
            label: "Vercel AI Gateway".to_string(),
            available: true,
            key_env_vars: vec!["AI_GATEWAY_API_KEY".to_string()],
            models: vec![
                ModelCapabilityInfo {
                    id: "openai/gpt-realtime-whisper".to_string(),
                    label: "gpt-realtime-whisper".to_string(),
                    mode: TranscriptionMode::Streaming,
                },
                ModelCapabilityInfo {
                    id: "openai/gpt-4o-transcribe".to_string(),
                    label: "GPT-4o Transcribe".to_string(),
                    mode: TranscriptionMode::Segmented,
                },
            ],
        },
        ProviderCapability {
            id: "mistral".to_string(),
            label: "Mistral AI".to_string(),
            available: false,
            key_env_vars: vec!["MISTRAL_API_KEY".to_string()],
            models: vec![ModelCapabilityInfo {
                id: "voxtral-mini-transcribe-3-0".to_string(),
                label: "Voxtral Mini Transcribe 3.0".to_string(),
                mode: TranscriptionMode::Segmented,
            }],
        },
    ]
}

fn install_catalogue(controller: &mut SessionController) {
    controller.set_capabilities(catalogue());
}

fn settings_with_mixed_case_key() -> Settings {
    let mut settings = Settings {
        provider: "vercel-ai-gateway".to_string(),
        model: "openai/gpt-realtime-whisper".to_string(),
        auto_paste: false,
        ..Settings::default()
    };
    settings.set_api_key("vercel-ai-gateway", MIXED_CASE_KEY);
    settings
}

fn open_settings_with_stored_key() -> SessionController {
    let mut controller = SessionController::new();
    install_catalogue(&mut controller);
    controller.apply_settings(settings_with_mixed_case_key());
    controller.open_settings_with(
        Some("vercel-ai-gateway"),
        Some("openai/gpt-realtime-whisper"),
        true,
    );
    controller
}

fn open_valid_settings(controller: &mut SessionController) {
    install_catalogue(controller);
    controller.open_settings();
    assert!(controller.set_settings_provider("vercel-ai-gateway"));
    assert!(controller.set_settings_model("openai/gpt-realtime-whisper"));
    controller.set_settings_api_key("sk-dialog-test");
}

#[test]
fn help_opens_and_escape_closes_it() {
    let mut controller = SessionController::new();

    controller.open_help();
    assert_eq!(controller.active_dialog(), Some(Dialog::Help));

    controller.dismiss_dialog_with_escape();
    assert_eq!(controller.active_dialog(), None);
}

#[test]
fn settings_saves_a_complete_draft_and_closes() {
    let mut controller = SessionController::new();
    open_valid_settings(&mut controller);

    let change = controller.save_settings().expect("valid draft saves");
    assert_eq!(
        change,
        SettingsChange {
            provider: "vercel-ai-gateway".to_string(),
            model: "openai/gpt-realtime-whisper".to_string(),
            api_key: Some("sk-dialog-test".to_string()),
        }
    );
    assert_eq!(controller.active_dialog(), None);
}

#[test]
fn stored_key_mask_is_surfaced_without_revealing_the_key() {
    let controller = open_settings_with_stored_key();

    assert_eq!(
        controller.settings_stored_key_mask().as_deref(),
        Some("vck…89")
    );
    assert_eq!(
        controller.settings_stored_key_label().as_deref(),
        Some("Stored key: vck…89")
    );
    assert_eq!(
        controller.settings_stored_key_hint(),
        Some(STORED_KEY_REVEAL_HINT)
    );

    let debug = format!("{controller:?}");
    assert!(
        !debug.contains(MIXED_CASE_KEY),
        "controller Debug exposed the key"
    );
    let settings_debug = format!("{:?}", controller.settings());
    assert!(
        !settings_debug.contains(MIXED_CASE_KEY),
        "Settings Debug exposed the key"
    );
}

#[test]
fn reveal_is_disabled_for_a_stored_key_until_a_value_is_typed() {
    let mut controller = open_settings_with_stored_key();

    assert!(!controller.settings_key_reveal_enabled());
    assert!(controller.settings_key_is_stored_unchanged());
    assert!(controller.settings_stored_key_label().is_some());

    controller.set_settings_api_key(MIXED_CASE_KEY);

    assert!(controller.settings_key_reveal_enabled());
    assert!(!controller.settings_key_is_stored_unchanged());
    assert_eq!(controller.settings_stored_key_label(), None);
    assert_eq!(controller.settings_stored_key_hint(), None);
    assert!(
        controller.settings_draft().api_key.as_bytes() == MIXED_CASE_KEY.as_bytes(),
        "the draft value was altered"
    );
}

#[test]
fn saving_an_incomplete_draft_keeps_the_dialog_open() {
    let mut controller = SessionController::new();
    controller.open_settings();
    controller.set_settings_api_key("sk-dialog-test");

    assert_eq!(controller.save_settings(), None);
    assert_eq!(controller.active_dialog(), Some(Dialog::Settings));
}

#[test]
fn cancel_discards_the_draft() {
    let mut controller = SessionController::new();
    controller.open_settings();
    controller.set_settings_provider("mistral");
    controller.set_settings_model("voxtral-mini-transcribe-3-0");
    controller.set_settings_api_key("sk-discard-me");

    controller.close_dialog();
    assert_eq!(controller.active_dialog(), None);

    controller.open_settings();
    assert_eq!(controller.settings_draft().provider, None);
    assert_eq!(controller.settings_draft().model, None);
    assert_eq!(controller.settings_draft().api_key, "");
    assert!(!controller.settings_draft_valid());
}

#[test]
fn opening_a_dialog_does_not_disturb_the_recording_session() {
    let mut controller = SessionController::new();
    install_catalogue(&mut controller);
    controller.open_form();
    controller.cycle_provider();
    controller.cycle_model();
    controller.set_draft_output(speechtt_core::session::OutputTarget::Memory);
    controller.set_draft_name("standup");
    assert!(controller.create_session());
    assert!(controller.start_recording());

    let state = controller.session_state();
    controller.open_help();
    controller.close_dialog();
    controller.open_settings();
    controller.close_dialog();

    assert_eq!(controller.session_state(), state);
    assert_eq!(
        controller.view_state(),
        speechtt_ui::controller::ViewState::Recording
    );
}

mod common;

use gpui_kit::AppContext as _;
use gpui_kit::test::TestWindowExt;

#[gpui_kit::test]
fn dialogs_render_and_save_through_the_app(cx: &mut gpui_kit::TestAppContext) {
    let (window, view) = common::open_speechtt_window(cx);

    cx.update_window(window.into(), |_, window, cx| {
        view.update(cx, |app, cx| {
            app.apply_capabilities(catalogue(), cx);
            app.open_help(window, cx);
        });
        window.render_frame(cx);
        assert_eq!(view.read(cx).active_dialog(), Some(Dialog::Help));

        view.update(cx, |app, cx| app.open_settings(window, cx));
        window.render_frame(cx);
        assert_eq!(view.read(cx).active_dialog(), Some(Dialog::Settings));

        view.update(cx, |app, cx| {
            app.cycle_settings_provider(cx);
            app.cycle_settings_model(cx);
            app.set_settings_api_key_value("sk-app-test", window, cx);
        });
        window.render_frame(cx);

        view.update(cx, |app, cx| {
            assert_eq!(app.pending_settings_change(), None);
            assert!(app.save_settings(cx));
        });
        window.render_frame(cx);

        assert_eq!(view.read(cx).active_dialog(), None);
        let pending = view
            .read(cx)
            .pending_settings_change()
            .expect("saved change is staged");
        assert_eq!(pending.provider, "vercel-ai-gateway");
        assert_eq!(pending.api_key.as_deref(), Some("sk-app-test"));
    })
    .unwrap();
}

#[gpui_kit::test]
fn each_dialog_opens_its_own_floating_window(cx: &mut gpui_kit::TestAppContext) {
    let (main, view) = common::open_speechtt_window(cx);
    assert_eq!(cx.windows().len(), 1);

    cx.update_window(main.into(), |_, window, cx| {
        view.update(cx, |app, cx| app.open_help(window, cx));
    })
    .unwrap();
    assert_eq!(cx.windows().len(), 2, "Help has its own window");
    assert!(cx.update(|cx| view.read(cx).has_dialog_window()));

    // Replacing Help with Settings keeps exactly one dialog window.
    cx.update_window(main.into(), |_, window, cx| {
        view.update(cx, |app, cx| app.open_settings(window, cx));
    })
    .unwrap();
    assert_eq!(cx.windows().len(), 2, "Settings has its own window");
    assert_eq!(
        cx.update(|cx| view.read(cx).active_dialog()),
        Some(Dialog::Settings)
    );

    // Escape closes the dialog window, not the widget.
    cx.update_window(main.into(), |_, window, cx| {
        view.update(cx, |app, cx| {
            app.dismiss_dialog_with_escape(window, cx);
        });
    })
    .unwrap();
    assert_eq!(cx.windows().len(), 1);
    assert_eq!(cx.update(|cx| view.read(cx).active_dialog()), None);
    assert!(!cx.update(|cx| view.read(cx).has_dialog_window()));
}

#[gpui_kit::test]
fn escape_key_closes_the_dialog_window(cx: &mut gpui_kit::TestAppContext) {
    let (main, view) = common::open_speechtt_window(cx);
    cx.update_window(main.into(), |_, window, cx| {
        view.update(cx, |app, cx| app.open_help(window, cx));
    })
    .unwrap();
    assert_eq!(cx.windows().len(), 2);

    let dialog = cx
        .windows()
        .into_iter()
        .find(|window| window.window_id() != main.window_id())
        .expect("Help opened a dialog window");

    cx.simulate_keystrokes(dialog, "escape");
    cx.run_until_parked();

    assert_eq!(cx.windows().len(), 1, "Escape closed the dialog window");
    assert_eq!(cx.update(|cx| view.read(cx).active_dialog()), None);
    assert!(!cx.update(|cx| view.read(cx).has_dialog_window()));
}
