//! Help and Settings dialog behaviour.
//!
//! The interaction contract is exercised through `SessionController` commands
//! (and one headless render smoke test). No assertion depends on element
//! placement, geometry or labels, so the dialog design stays free to change.

use speechtt_ui::controller::{Dialog, SessionController, SettingsChange};

fn open_valid_settings(controller: &mut SessionController) {
    controller.open_settings();
    assert!(controller.set_settings_provider("vercel"));
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
            provider: "vercel".to_string(),
            model: "openai/gpt-realtime-whisper".to_string(),
            api_key: Some("sk-dialog-test".to_string()),
        }
    );
    assert_eq!(controller.active_dialog(), None);
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
        view.update(cx, |app, cx| app.open_help(window, cx));
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
        assert_eq!(pending.provider, "vercel");
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
