//! Help and Settings dialog behaviour.
//!
//! The interaction contract is exercised through `SessionController` commands
//! and the wiring that reaches them: opening and closing a window, committing a
//! menu selection, saving, and surviving a failed save. Nothing asserts which
//! rows a screen draws or in what order — a dialog is recomposed whenever the
//! layout improves, and a test that pins its composition only has to be deleted
//! the next time it is.

use speechtt_core::backend::{BackendError, SttSessionSink};
use speechtt_core::protocol::{
    ModelCapabilityInfo, ProviderCapability, SttCommand, TranscriptionMode,
};
use speechtt_core::settings::{DEFAULT_PROVIDER, Settings};
use speechtt_ui::controller::{
    AppError, BackendFailure, Dialog, HelpTab, STORED_KEY_REVEAL_HINT, SessionController,
    SettingsChange,
};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

const MIXED_CASE_KEY: &str = "vck_AbCdEfGhIjKlMnOpQrStUvWxYzH0123456789";

/// Catalogue the worker would send after a handshake.
fn catalogue() -> Vec<ProviderCapability> {
    vec![ProviderCapability {
        id: "vercel-ai-gateway".to_string(),
        label: "Vercel AI Gateway".to_string(),
        available: true,
        key_env_vars: vec!["AI_GATEWAY_API_KEY".to_string()],
        models: vec![
            ModelCapabilityInfo {
                id: "openai/gpt-realtime-whisper".to_string(),
                label: "gpt-realtime-whisper".to_string(),
                mode: TranscriptionMode::Streaming,
                description: None,
                pricing: std::collections::BTreeMap::new(),
            },
            ModelCapabilityInfo {
                id: "openai/gpt-4o-transcribe".to_string(),
                label: "GPT-4o Transcribe".to_string(),
                mode: TranscriptionMode::Segmented,
                description: None,
                pricing: std::collections::BTreeMap::new(),
            },
        ],
    }]
}

fn install_catalogue(controller: &mut SessionController) {
    controller.set_capabilities(catalogue());
}

/// A catalogue with a second provider, so the provider menu has a choice.
fn two_provider_catalogue() -> Vec<ProviderCapability> {
    let mut providers = catalogue();
    providers.push(ProviderCapability {
        id: "mistral".to_string(),
        label: "Mistral".to_string(),
        available: true,
        key_env_vars: vec!["MISTRAL_API_KEY".to_string()],
        models: Vec::new(),
    });
    providers
}

/// A catalogue holding exactly one provider, under an id the caller chooses.
fn single_provider_catalogue(id: &str, key_env: &str) -> Vec<ProviderCapability> {
    vec![ProviderCapability {
        id: id.to_string(),
        label: id.to_uppercase(),
        available: true,
        key_env_vars: vec![key_env.to_string()],
        models: Vec::new(),
    }]
}

fn settings_with_mixed_case_key() -> Settings {
    let mut settings = Settings {
        provider: "vercel-ai-gateway".to_string(),
        model_streaming: "openai/gpt-realtime-whisper".to_string(),
        model_batch: "openai/gpt-4o-transcribe".to_string(),
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
        Some("openai/gpt-4o-transcribe"),
        true,
    );
    controller
}

fn open_valid_settings(controller: &mut SessionController) {
    install_catalogue(controller);
    controller.open_settings();
    assert!(controller.set_settings_provider("vercel-ai-gateway"));
    assert!(controller.set_settings_streaming_model("openai/gpt-realtime-whisper"));
    assert!(controller.set_settings_batch_model("openai/gpt-4o-transcribe"));
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
            model_streaming: "openai/gpt-realtime-whisper".to_string(),
            model_batch: "openai/gpt-4o-transcribe".to_string(),
            language: None,
            language_model: None,
            bun_executable: String::new(),
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
fn settings_offers_a_provider_choice_only_with_two_providers() {
    let mut controller = SessionController::new();
    assert!(
        !controller.settings_offers_provider_choice(),
        "no catalogue offers nothing to choose"
    );

    install_catalogue(&mut controller);
    assert!(
        !controller.settings_offers_provider_choice(),
        "a single provider is not a choice"
    );

    controller.set_capabilities(two_provider_catalogue());
    assert!(
        controller.settings_offers_provider_choice(),
        "two providers are a choice"
    );
}

#[test]
fn pin_single_provider_uses_the_catalogue_entry_or_the_default() {
    let mut single = SessionController::new();
    single.set_capabilities(vec![ProviderCapability {
        id: "mistral".to_string(),
        label: "Mistral".to_string(),
        available: true,
        key_env_vars: vec!["MISTRAL_API_KEY".to_string()],
        models: Vec::new(),
    }]);
    single.open_settings();
    assert_eq!(
        single.settings_draft().provider.as_deref(),
        Some("mistral"),
        "the only catalogue provider is pinned"
    );

    let mut empty = SessionController::new();
    empty.open_settings();
    assert_eq!(
        empty.settings_draft().provider.as_deref(),
        Some(DEFAULT_PROVIDER),
        "a fresh install without a catalogue pins the fallback"
    );
}

#[test]
fn pin_single_provider_keeps_a_matching_provider_and_leaves_a_choice_alone() {
    let mut chosen = SessionController::new();
    install_catalogue(&mut chosen);
    chosen.open_settings_with(Some("vercel-ai-gateway"), None, None, false);
    chosen.pin_single_provider();
    assert_eq!(
        chosen.settings_draft().provider.as_deref(),
        Some("vercel-ai-gateway"),
        "a draft already on the catalogue's only provider keeps it"
    );

    let mut choice = SessionController::new();
    choice.set_capabilities(two_provider_catalogue());
    choice.open_settings();
    assert_eq!(
        choice.settings_draft().provider,
        None,
        "the menu starts empty when there is a choice"
    );
    choice.pin_single_provider();
    assert_eq!(
        choice.settings_draft().provider,
        None,
        "a choice is never pinned"
    );
}

#[test]
fn a_catalogue_arriving_while_settings_is_open_pins_the_only_provider() {
    let mut controller = SessionController::new();
    controller.set_capabilities(two_provider_catalogue());
    controller.open_settings();
    assert_eq!(controller.settings_draft().provider, None);

    install_catalogue(&mut controller);

    assert_eq!(
        controller.settings_draft().provider.as_deref(),
        Some("vercel-ai-gateway"),
        "the arriving catalogue pins its only provider"
    );
}

#[test]
fn a_fresh_install_without_a_catalogue_saves_with_a_path_and_a_key() {
    let mut controller = SessionController::new();
    controller.note_backend_failure(BackendFailure::Unavailable);
    controller.open_settings();
    controller.set_settings_bun_executable("/home/ada/.bun/bin/bun");
    controller.set_settings_api_key("sk-fresh-install");

    let change = controller
        .settings_change()
        .expect("the pinned provider, path and key are enough");

    assert_eq!(change.provider, DEFAULT_PROVIDER);
    assert_eq!(change.bun_executable, "/home/ada/.bun/bin/bun");
    assert_eq!(change.api_key.as_deref(), Some("sk-fresh-install"));
}

#[test]
fn pin_single_provider_replaces_the_guess_the_catalogue_does_not_list() {
    let mut controller = SessionController::new();
    controller.open_settings();
    assert_eq!(
        controller.settings_draft().provider.as_deref(),
        Some(DEFAULT_PROVIDER),
        "a draft without a catalogue starts on the fallback guess"
    );
    controller.set_settings_api_key("sk-stale-guess");

    // The worker names one provider, and it is not the guess.
    controller.set_capabilities(single_provider_catalogue("mistral", "MISTRAL_API_KEY"));

    assert_eq!(
        controller.settings_draft().provider.as_deref(),
        Some("mistral"),
        "an arrived catalogue outranks the guess made before it existed"
    );
    assert!(
        controller.settings_provider_known(),
        "the draft names a provider the catalogue lists"
    );
}

#[test]
fn pin_single_provider_clears_the_models_of_a_provider_it_replaces() {
    let mut controller = SessionController::new();
    install_catalogue(&mut controller);
    controller.open_settings();
    assert!(controller.set_settings_streaming_model("openai/gpt-realtime-whisper"));
    assert!(controller.set_settings_batch_model("openai/gpt-4o-transcribe"));

    // A second handshake lists one provider, and it is not the drafted one.
    controller.set_capabilities(single_provider_catalogue("mistral", "MISTRAL_API_KEY"));

    assert_eq!(
        controller.settings_draft().provider.as_deref(),
        Some("mistral")
    );
    assert_eq!(
        controller.settings_draft().model_streaming,
        None,
        "the replaced provider's streaming default does not belong to the new one"
    );
    assert_eq!(
        controller.settings_draft().model_batch,
        None,
        "the replaced provider's batch default does not belong to the new one"
    );
}

#[test]
fn pin_single_provider_leaves_the_draft_alone_without_a_usable_single_provider() {
    let mut choice = SessionController::new();
    choice.set_capabilities(two_provider_catalogue());
    choice.open_settings();
    choice.pin_single_provider();
    assert_eq!(
        choice.settings_draft().provider,
        None,
        "a real choice is never answered for the user"
    );

    let mut empty = SessionController::new();
    empty.open_settings();
    assert_eq!(
        empty.settings_draft().provider.as_deref(),
        Some(DEFAULT_PROVIDER)
    );
    empty.set_capabilities(Vec::new());
    empty.pin_single_provider();
    assert_eq!(
        empty.settings_draft().provider.as_deref(),
        Some(DEFAULT_PROVIDER),
        "a catalogue with no provider has nothing to pin and leaves the draft alone"
    );
}

#[test]
fn an_empty_catalogue_says_it_reported_no_providers() {
    let mut controller = SessionController::new();
    assert_eq!(controller.settings_backend_note(), None);

    controller.set_capabilities(Vec::new());

    assert_eq!(
        controller.settings_backend_note().as_deref(),
        Some("The transcription backend reported no providers."),
        "a catalogue with no provider explains the disabled Save instead of staying silent"
    );
}

#[test]
fn cancel_discards_the_draft() {
    let mut controller = SessionController::new();
    controller.open_settings();
    controller.set_settings_provider("mistral");
    controller.set_settings_streaming_model("voxtral-mini-transcribe-3-0");
    controller.set_settings_batch_model("voxtral-mini-transcribe-3-0");
    controller.set_settings_api_key("sk-discard-me");

    controller.close_dialog();
    assert_eq!(controller.active_dialog(), None);

    controller.open_settings();
    assert_eq!(
        controller.settings_draft().provider.as_deref(),
        Some(DEFAULT_PROVIDER),
        "an empty catalogue pins the fallback provider"
    );
    assert_eq!(controller.settings_draft().model_streaming, None);
    assert_eq!(controller.settings_draft().model_batch, None);
    assert_eq!(controller.settings_draft().api_key, "");
    assert!(!controller.settings_draft_valid());
}

#[test]
fn opening_a_dialog_does_not_disturb_the_recording_session() {
    let mut controller = SessionController::new();
    install_catalogue(&mut controller);
    controller.choose_mode(speechtt_ui::controller::SessionMode::Live);
    controller.cycle_provider();
    controller.cycle_model();
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
        speechtt_ui::controller::ViewState::Live
    );
}

mod common;

use gpui_kit::AppContext as _;
use gpui_kit::test::{TestAppContextExt as _, TestWindowExt};

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
            app.set_settings_provider("vercel-ai-gateway", cx);
            app.set_settings_streaming_model("openai/gpt-realtime-whisper", cx);
            app.set_settings_batch_model("openai/gpt-4o-transcribe", cx);
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

#[gpui_kit::test]
fn help_switches_to_the_model_catalogue_tab(cx: &mut gpui_kit::TestAppContext) {
    let (window, view) = common::open_speechtt_window(cx);

    cx.update_window(window.into(), |_, window, cx| {
        view.update(cx, |app, cx| app.open_help(window, cx));
        window.render_frame(cx);
        assert_eq!(view.read(cx).active_dialog(), Some(Dialog::Help));
        assert_eq!(view.read(cx).help_tab(), HelpTab::Shortcuts);

        view.update(cx, |app, cx| app.set_help_tab(HelpTab::Models, cx));
        window.render_frame(cx);

        assert_eq!(view.read(cx).help_tab(), HelpTab::Models);
        assert_eq!(
            view.read(cx)
                .help_models()
                .iter()
                .map(|model| model.id.as_str())
                .collect::<Vec<_>>(),
            ["openai/gpt-4o-transcribe", "openai/gpt-realtime-whisper"]
        );
    })
    .unwrap();
}

#[gpui_kit::test]
async fn the_dialog_commits_the_dictation_language(cx: &mut gpui_kit::TestAppContext) {
    use speechtt_core::protocol::{CapabilitySource, LanguageOption, SttEvent};

    let (main, view, events) = common::open_speechtt_window_with_events(cx);

    // A language is committed as its ISO code; the handshake is the only source
    // of the offered list.
    events
        .send_blocking(Ok(SttEvent::Capabilities {
            request_id: None,
            source: CapabilitySource::Builtin,
            providers: catalogue(),
            languages: Some(vec![
                LanguageOption {
                    code: "fr".to_owned(),
                    label: "French".to_owned(),
                },
                LanguageOption {
                    code: "en".to_owned(),
                    label: "English".to_owned(),
                },
            ]),
            language_models: None,
            default_language_model: Some("openai/gpt-4.1-mini".to_owned()),
        }))
        .expect("event sent");
    cx.run_until_parked();

    cx.update_window(main.into(), |_, window, cx| {
        view.update(cx, |app, cx| app.open_settings(window, cx));
        window.render_frame(cx);
    })
    .unwrap();

    let dialog = cx
        .windows()
        .into_iter()
        .find(|window| window.window_id() != main.window_id())
        .expect("Settings opened its own window");

    cx.update_window(dialog, |_, window, cx| {
        window.render_frame(cx);
        window.within("field-settings-language").click("input", cx);
        window.press("down", cx);
        window.press("enter", cx);
    })
    .unwrap();

    cx.wait_for(dialog, std::time::Duration::from_millis(500), |_, cx| {
        view.read(cx).settings_draft().language.as_deref() == Some("fr")
    })
    .await;
}

/// A sink the injected spawn returns: it records the commands the app sends,
/// so a test can prove the worker was attached without starting one.
#[derive(Clone, Default)]
struct InjectedSink {
    commands: Arc<Mutex<Vec<SttCommand>>>,
}

impl InjectedSink {
    fn commands(&self) -> Vec<SttCommand> {
        self.commands.lock().expect("injected sink lock").clone()
    }
}

impl SttSessionSink for InjectedSink {
    fn send(&mut self, cmd: SttCommand) -> Result<(), BackendError> {
        self.commands.lock().expect("injected sink lock").push(cmd);
        Ok(())
    }
}

/// Saving a `Bun path` while no worker is attached spawns one and attaches it.
#[gpui_kit::test]
fn saving_a_bun_path_respawns_and_attaches_the_worker(cx: &mut gpui_kit::TestAppContext) {
    let (main, view, events) = common::open_speechtt_window_without_catalogue(cx);
    drop(events);
    cx.run_until_parked();
    assert_eq!(
        cx.update(|cx| view.read(cx).app_error().cloned()),
        Some(AppError::BackendUnavailable),
        "a closed event stream is the dead-worker state"
    );

    let spawns = Arc::new(AtomicUsize::new(0));
    let sink = InjectedSink::default();
    let recorder = sink.clone();
    let calls = spawns.clone();
    cx.update(|cx| {
        view.update(cx, |app, _| {
            app.set_backend_spawn(move |settings| {
                assert_eq!(
                    settings.bun_executable, "/home/ada/.bun/bin/bun",
                    "the save hands the spawn the corrected runtime"
                );
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(Box::new(recorder.clone()))
            });
        });
    });

    cx.update_window(main.into(), |_, window, cx| {
        view.update(cx, |app, cx| {
            app.open_settings(window, cx);
            app.set_settings_bun_executable("/home/ada/.bun/bin/bun", window, cx);
        });
        window.render_frame(cx);
        view.update(cx, |app, cx| assert!(app.save_settings(cx)));
    })
    .unwrap();

    assert_eq!(spawns.load(Ordering::SeqCst), 1, "exactly one respawn");
    assert_eq!(
        cx.update(|cx| view.read(cx).active_dialog()),
        None,
        "a successful save closes Settings"
    );
    assert_eq!(cx.update(|cx| view.read(cx).app_error().cloned()), None);
    assert!(
        sink.commands()
            .iter()
            .any(|command| matches!(command, SttCommand::Capabilities { .. })),
        "the attached worker is asked for the catalogue"
    );
}

/// A backend that is already working is never replaced: a save only refreshes
/// the handshake, because respawning would drop a live take.
#[gpui_kit::test]
fn saving_with_a_healthy_backend_does_not_spawn_a_worker(cx: &mut gpui_kit::TestAppContext) {
    let (main, view) = common::open_speechtt_window(cx);

    let spawns = Arc::new(AtomicUsize::new(0));
    let calls = spawns.clone();
    cx.update(|cx| {
        view.update(cx, |app, _| {
            app.set_backend_spawn(move |_| {
                calls.fetch_add(1, Ordering::SeqCst);
                Err(BackendFailure::Unavailable)
            });
        });
    });

    cx.update_window(main.into(), |_, window, cx| {
        view.update(cx, |app, cx| {
            app.open_settings(window, cx);
            assert!(app.set_settings_provider("vercel-ai-gateway", cx));
            assert!(app.set_settings_streaming_model("openai/gpt-realtime-whisper", cx));
            assert!(app.set_settings_batch_model("openai/gpt-4o-transcribe", cx));
            app.set_settings_api_key_value("sk-app-test", window, cx);
        });
        window.render_frame(cx);
        view.update(cx, |app, cx| assert!(app.save_settings(cx)));
    })
    .unwrap();

    assert_eq!(
        spawns.load(Ordering::SeqCst),
        0,
        "a healthy backend is not replaced"
    );
    assert_eq!(cx.update(|cx| view.read(cx).active_dialog()), None);
}

/// A respawn that fails keeps the dialog, the error and the correction: the
/// user can fix the path again without reopening anything.
#[gpui_kit::test]
fn a_failed_respawn_keeps_settings_open_with_the_error_and_the_draft(
    cx: &mut gpui_kit::TestAppContext,
) {
    let (main, view, events) = common::open_speechtt_window_without_catalogue(cx);
    drop(events);
    cx.run_until_parked();

    cx.update(|cx| {
        view.update(cx, |app, _| {
            app.set_backend_spawn(|_| {
                Err(BackendFailure::BunMissing {
                    searched: vec![PathBuf::from("/home/ada/.nowhere/bun")],
                })
            });
        });
    });

    cx.update_window(main.into(), |_, window, cx| {
        view.update(cx, |app, cx| {
            app.open_settings(window, cx);
            app.set_settings_bun_executable("/home/ada/.nowhere/bun", window, cx);
        });
        window.render_frame(cx);
        view.update(cx, |app, cx| assert!(!app.save_settings(cx)));
    })
    .unwrap();

    assert_eq!(
        cx.update(|cx| view.read(cx).active_dialog()),
        Some(Dialog::Settings),
        "a failed respawn keeps the dialog open"
    );
    assert_eq!(
        cx.update(|cx| view.read(cx).app_error().cloned()),
        Some(AppError::BackendUnavailable),
        "the failure is still reported"
    );
    assert_eq!(
        cx.update(|cx| view.read(cx).settings_draft().bun_executable.clone()),
        "/home/ada/.nowhere/bun",
        "the correction is still in the draft"
    );

    // Editable, not merely visible: a second correction saves once the spawn
    // succeeds, so the dialog never became a dead end.
    cx.update(|cx| {
        view.update(cx, |app, _| {
            app.set_backend_spawn(|_| Ok(Box::new(InjectedSink::default())));
        });
    });
    cx.update_window(main.into(), |_, window, cx| {
        view.update(cx, |app, cx| {
            app.set_settings_bun_executable("/home/ada/.bun/bin/bun", window, cx);
            assert!(app.save_settings(cx));
        });
    })
    .unwrap();
    assert_eq!(cx.update(|cx| view.read(cx).active_dialog()), None);
    assert_eq!(cx.update(|cx| view.read(cx).app_error().cloned()), None);
}
