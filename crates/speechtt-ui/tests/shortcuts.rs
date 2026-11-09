//! Window- and state-scoped keyboard commands, driven through real headless
//! windows.
//!
//! Every test opens the production widget (or one of its dialogs) and presses a
//! key with `window.press`, which resolves the binding exactly as the platform
//! would. Nothing here asserts geometry: the assertions are the session state,
//! the screen, the open dialog, the commands the worker received and the file
//! that was written.

mod common;

use gpui_kit::AppContext as _;
use gpui_kit::TestAppContext;
use gpui_kit::base::Root;
use gpui_kit::test::TestWindowExt;
use gpui_kit::{AnyWindowHandle, Entity, SharedString, WindowHandle};
use speechtt_core::protocol::SttEvent;
use speechtt_core::session::SessionState;
use speechtt_ui::app::SpeechTtApp;
use speechtt_ui::controller::{AppError, Dialog, SessionMode, ViewState};

const TRANSCRIPT: &str = "hello there";

/// Presses `key` in `window` and lets every deferred effect of the keystroke
/// run, so the assertions see the settled state.
fn press(cx: &mut TestAppContext, window: AnyWindowHandle, key: &str) {
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.press(key, cx);
    })
    .unwrap();
    cx.run_until_parked();
}

/// Takes the widget to its live take screen with a complete session.
fn prepare_live_take(view: &Entity<SpeechTtApp>, cx: &mut TestAppContext) {
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            app.choose_mode(SessionMode::Live, cx);
            app.set_draft_provider("vercel-ai-gateway", cx);
            app.set_draft_model("openai/gpt-4o-transcribe", cx);
            app.create_session(cx);
        });
    });
}

/// Takes the widget through a live take with text, leaving its transcript on
/// screen.
fn finish_a_take(
    cx: &mut TestAppContext,
    main: WindowHandle<Root>,
    view: &Entity<SpeechTtApp>,
    events: &common::EventSender,
) {
    prepare_live_take(view, cx);
    press(cx, main.into(), "space");
    events
        .send_blocking(Ok(SttEvent::Committed {
            session_id: "take-1".into(),
            text: TRANSCRIPT.into(),
            segment: None,
        }))
        .expect("send committed text");
    cx.run_until_parked();
    press(cx, main.into(), "enter");
    assert_eq!(
        cx.update(|cx| view.read(cx).view_state()),
        ViewState::SessionEnd
    );
}

/// The unique window that is not the widget.
fn dialog_window(cx: &TestAppContext, main: WindowHandle<Root>) -> AnyWindowHandle {
    cx.windows()
        .into_iter()
        .find(|window| window.window_id() != main.window_id())
        .expect("a dialog opened its own window")
}

fn active_dialog(cx: &TestAppContext, view: &Entity<SpeechTtApp>) -> Option<Dialog> {
    cx.update(|cx| view.read(cx).active_dialog())
}

// ----- Dialog toggles ---------------------------------------------------

#[gpui_kit::test]
fn f1_opens_help_and_f1_again_closes_it(cx: &mut TestAppContext) {
    let (main, view) = common::open_speechtt_window(cx);

    press(cx, main.into(), "f1");

    assert_eq!(active_dialog(cx, &view), Some(Dialog::Help));
    assert_eq!(cx.windows().len(), 2, "Help shows in its own window");

    press(cx, main.into(), "f1");

    assert_eq!(active_dialog(cx, &view), None, "the same key closes it");
    assert_eq!(cx.windows().len(), 1, "the dialog window is gone");
    assert!(!cx.update(|cx| view.read(cx).has_dialog_window()));
}

#[gpui_kit::test]
fn f2_opens_settings(cx: &mut TestAppContext) {
    let (main, view) = common::open_speechtt_window(cx);

    press(cx, main.into(), "f2");

    assert_eq!(active_dialog(cx, &view), Some(Dialog::Settings));
    assert!(cx.update(|cx| view.read(cx).has_dialog_window()));
}

/// The live take screen declares the composed `SpeechTT Take`, so the
/// `SpeechTT` fact F1 is bound to still matches there.
#[gpui_kit::test]
fn f1_opens_help_on_the_live_take_screen(cx: &mut TestAppContext) {
    let (main, view) = common::open_speechtt_window(cx);
    prepare_live_take(&view, cx);

    press(cx, main.into(), "f1");

    assert_eq!(active_dialog(cx, &view), Some(Dialog::Help));
    assert_eq!(cx.windows().len(), 2, "Help shows in its own window");
}

/// The transcript screen declares the composed `SpeechTT SessionEnd`, so F1
/// still finds the `SpeechTT` fact it is bound to.
#[gpui_kit::test]
fn f1_opens_help_on_the_transcript_screen(cx: &mut TestAppContext) {
    let (main, view, _sink, _clipboard, events) = common::open_speechtt_window_with_channels(cx);
    finish_a_take(cx, main, &view, &events);

    press(cx, main.into(), "f1");

    assert_eq!(active_dialog(cx, &view), Some(Dialog::Help));
    assert_eq!(cx.windows().len(), 2, "Help shows in its own window");
}

/// While a dialog owns focus, the same keys belong to it: F2 replaces Help with
/// Settings and F1 replaces Settings with Help.
#[gpui_kit::test]
fn the_dialog_toggles_switch_between_the_two_dialogs(cx: &mut TestAppContext) {
    let (main, view) = common::open_speechtt_window(cx);
    press(cx, main.into(), "f1");
    assert_eq!(active_dialog(cx, &view), Some(Dialog::Help));

    let help = dialog_window(cx, main);
    press(cx, help, "f2");

    assert_eq!(active_dialog(cx, &view), Some(Dialog::Settings));
    assert_eq!(cx.windows().len(), 2, "Help closed as Settings opened");

    let settings = dialog_window(cx, main);
    press(cx, settings, "f1");

    assert_eq!(active_dialog(cx, &view), Some(Dialog::Help));
    assert_eq!(cx.windows().len(), 2, "Settings closed as Help opened");
}

// ----- Recording commands ----------------------------------------------

#[gpui_kit::test]
fn space_walks_a_live_take_from_start_to_pause_to_resume(cx: &mut TestAppContext) {
    let (main, view, sink, _clipboard, _events) = common::open_speechtt_window_with_channels(cx);
    prepare_live_take(&view, cx);

    press(cx, main.into(), "space");
    assert_eq!(
        cx.update(|cx| view.read(cx).session_state()),
        SessionState::Recording
    );
    assert!(
        matches!(
            sink.commands().last(),
            Some(speechtt_core::protocol::SttCommand::Start { .. })
        ),
        "the first press opens one stream"
    );

    press(cx, main.into(), "space");
    assert_eq!(
        cx.update(|cx| view.read(cx).session_state()),
        SessionState::Paused
    );
    assert_eq!(
        sink.commands().last(),
        Some(&speechtt_core::protocol::SttCommand::Pause)
    );

    press(cx, main.into(), "space");
    assert_eq!(
        cx.update(|cx| view.read(cx).session_state()),
        SessionState::Recording
    );
    assert_eq!(
        sink.commands().last(),
        Some(&speechtt_core::protocol::SttCommand::Resume)
    );
}

#[gpui_kit::test]
fn enter_stops_the_take_and_shows_the_transcript(cx: &mut TestAppContext) {
    let (main, view, _sink, _clipboard, events) = common::open_speechtt_window_with_channels(cx);
    prepare_live_take(&view, cx);

    press(cx, main.into(), "space");
    events
        .send_blocking(Ok(SttEvent::Committed {
            session_id: "take-1".into(),
            text: TRANSCRIPT.into(),
            segment: None,
        }))
        .expect("send committed text");
    cx.run_until_parked();

    press(cx, main.into(), "enter");

    assert_eq!(
        cx.update(|cx| view.read(cx).session_state()),
        SessionState::Blank
    );
    assert_eq!(
        cx.update(|cx| view.read(cx).view_state()),
        ViewState::SessionEnd
    );
}

/// The mode chooser has no take to start, so the key falls through untouched.
#[gpui_kit::test]
fn space_on_the_mode_chooser_starts_nothing(cx: &mut TestAppContext) {
    let (main, view, sink, _clipboard, _events) = common::open_speechtt_window_with_channels(cx);
    // The construction handshake reaches the worker; only what the key sends
    // matters here.
    let before = sink.commands().len();

    press(cx, main.into(), "space");

    assert_eq!(
        cx.update(|cx| view.read(cx).session_state()),
        SessionState::Blank
    );
    assert_eq!(sink.commands().len(), before, "no command may be sent");
}

// ----- State-scoped commands -------------------------------------------

/// A path the widget must never write: it is removed first, so `exists()`
/// afterwards proves that no save reached the filesystem.
fn unwritten_target(name: &str) -> std::path::PathBuf {
    let target = std::env::temp_dir().join(format!(
        "speechtt-unwritten-{}-{name}.md",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&target);
    target
}

/// The screen facts a key must leave untouched.
fn state_and_screen(cx: &TestAppContext, view: &Entity<SpeechTtApp>) -> (SessionState, ViewState) {
    cx.update(|cx| (view.read(cx).session_state(), view.read(cx).view_state()))
}

/// `Ctrl+S` has no finished transcript to name on the mode chooser.
#[gpui_kit::test]
fn ctrl_s_is_inert_on_the_mode_chooser(cx: &mut TestAppContext) {
    let target = unwritten_target("mode-chooser");
    let prompt = common::ScriptedSavePrompt::chosen(&target);
    let (main, view, _events) =
        common::open_speechtt_window_with_save_prompt(cx, Box::new(prompt.clone()));

    press(cx, main.into(), "ctrl-s");

    assert_eq!(state_and_screen(cx, &view).1, ViewState::Mode);
    assert!(prompt.asks().is_empty(), "no dialog on the mode chooser");
    assert!(!target.exists(), "nothing is written");
}

/// The setup form is still before the take, so there is nothing to save.
#[gpui_kit::test]
fn ctrl_s_is_inert_on_the_setup_form(cx: &mut TestAppContext) {
    let target = unwritten_target("setup-form");
    let prompt = common::ScriptedSavePrompt::chosen(&target);
    let (main, view, _events) =
        common::open_speechtt_window_with_save_prompt(cx, Box::new(prompt.clone()));
    cx.update(|cx| view.update(cx, |app, cx| app.choose_mode(SessionMode::Live, cx)));
    assert_eq!(state_and_screen(cx, &view).1, ViewState::Form);

    press(cx, main.into(), "ctrl-s");

    assert_eq!(state_and_screen(cx, &view).1, ViewState::Form);
    assert!(prompt.asks().is_empty(), "no dialog on the setup form");
    assert!(!target.exists(), "nothing is written");
}

/// The live take has no finished transcript yet; the key belongs to it only
/// after Enter.
#[gpui_kit::test]
fn ctrl_s_is_inert_on_the_live_take_screen(cx: &mut TestAppContext) {
    let target = unwritten_target("live-take");
    let prompt = common::ScriptedSavePrompt::chosen(&target);
    let (main, view, _events) =
        common::open_speechtt_window_with_save_prompt(cx, Box::new(prompt.clone()));
    prepare_live_take(&view, cx);
    assert_eq!(state_and_screen(cx, &view).1, ViewState::Live);

    press(cx, main.into(), "ctrl-s");

    assert_eq!(state_and_screen(cx, &view).1, ViewState::Live);
    assert!(prompt.asks().is_empty(), "no dialog on the live take");
    assert!(!target.exists(), "nothing is written");
}

/// Enter stops a take only while one is open; the mode chooser keeps its own.
#[gpui_kit::test]
fn enter_is_inert_on_the_mode_chooser(cx: &mut TestAppContext) {
    let (main, view, sink, _clipboard, _events) = common::open_speechtt_window_with_channels(cx);
    let before = sink.commands().len();
    let state = state_and_screen(cx, &view);

    press(cx, main.into(), "enter");

    assert_eq!(state_and_screen(cx, &view), state);
    assert_eq!(sink.commands().len(), before, "no command may be sent");
}

/// On the finished transcript Enter has already done its job: a second press
/// must not stop or start anything.
#[gpui_kit::test]
fn enter_is_inert_on_the_transcript_screen(cx: &mut TestAppContext) {
    let (main, view, sink, _clipboard, events) = common::open_speechtt_window_with_channels(cx);
    finish_a_take(cx, main, &view, &events);
    let before = sink.commands().len();
    let state = state_and_screen(cx, &view);

    press(cx, main.into(), "enter");

    assert_eq!(state_and_screen(cx, &view), state);
    assert_eq!(sink.commands().len(), before, "no command may be sent");
}

/// Space belongs to the live take, not to the setup form's controls.
#[gpui_kit::test]
fn space_is_inert_on_the_setup_form(cx: &mut TestAppContext) {
    let (main, view, sink, _clipboard, _events) = common::open_speechtt_window_with_channels(cx);
    cx.update(|cx| view.update(cx, |app, cx| app.choose_mode(SessionMode::Live, cx)));
    assert_eq!(state_and_screen(cx, &view).1, ViewState::Form);
    let before = sink.commands().len();

    press(cx, main.into(), "space");

    assert_eq!(state_and_screen(cx, &view).1, ViewState::Form);
    assert_eq!(sink.commands().len(), before, "no command may be sent");
}

/// Nor does Space start a new take from the finished transcript.
#[gpui_kit::test]
fn space_is_inert_on_the_transcript_screen(cx: &mut TestAppContext) {
    let (main, view, sink, _clipboard, events) = common::open_speechtt_window_with_channels(cx);
    finish_a_take(cx, main, &view, &events);
    let before = sink.commands().len();
    let state = state_and_screen(cx, &view);

    press(cx, main.into(), "space");

    assert_eq!(state_and_screen(cx, &view), state);
    assert_eq!(sink.commands().len(), before, "no command may be sent");
}

/// The dialog owns its own keys: Space typed into the API-key field is a
/// character, not the take command.
///
/// This is the **behaviour** proof: the field ends up holding `"sk "` and the
/// session stays `Blank`. It does not prove the predicate on its own, because
/// `DialogWindow` registers no take handler, so a keystroke that matched
/// `SpeechTT || SpeechTTDialog` would fall through to the field and produce the
/// same result. The predicate itself is pinned by the table tests in
/// `src/shortcuts.rs`; the runtime absence of the take bindings cannot be
/// asserted here, because `Window::bindings_for_action` returns no bindings for
/// any action in a headless window (it is empty even for the copy shortcut,
/// which does dispatch when pressed).
///
/// The field is the last row of the scrolling dialog body, so the test scrolls
/// it into view before typing — the reachability step a user performs, not a
/// geometry assertion.
#[gpui_kit::test]
fn space_typed_into_the_settings_api_key_field_stays_a_character(cx: &mut TestAppContext) {
    let (main, view) = common::open_speechtt_window(cx);
    press(cx, main.into(), "f2");
    let settings = dialog_window(cx, main);

    cx.update_window(settings, |_, window, cx| {
        window.render_frame(cx);
        window.within("field-settings-provider").scroll(
            "input",
            gpui_kit::ScrollDelta::Lines(gpui_kit::point(0., -5.)),
            cx,
        );
        window.render_frame(cx);
        window.click("field-settings-api-key", cx);
        window.input("sk", cx);
        window.press("space", cx);
    })
    .unwrap();
    cx.run_until_parked();

    assert_eq!(
        cx.update(|cx| view.read(cx).settings_api_key_value(cx)),
        "sk ",
        "the space reached the focused field"
    );
    assert_eq!(
        cx.update(|cx| view.read(cx).session_state()),
        SessionState::Blank,
        "the take command did not run"
    );
    assert_eq!(active_dialog(cx, &view), Some(Dialog::Settings));
}

/// F2 pressed in the widget window is not the Help dialog's own key: the
/// widget-owned path closes the dialog from outside, then opens Settings.
#[gpui_kit::test]
fn f2_in_the_widget_window_replaces_an_open_help_dialog(cx: &mut TestAppContext) {
    let (main, view) = common::open_speechtt_window(cx);
    press(cx, main.into(), "f1");
    assert_eq!(active_dialog(cx, &view), Some(Dialog::Help));
    assert_eq!(cx.windows().len(), 2);

    press(cx, main.into(), "f2");

    assert_eq!(active_dialog(cx, &view), Some(Dialog::Settings));
    assert_eq!(cx.windows().len(), 2, "Help closed as Settings opened");
    assert!(cx.update(|cx| view.read(cx).has_dialog_window()));
}

// ----- Ctrl+S -----------------------------------------------------------

/// A finished take saves where the prompt points, through the injected seam.
#[gpui_kit::test]
fn ctrl_s_writes_the_finished_transcript(cx: &mut TestAppContext) {
    let directory = std::env::temp_dir().join(format!("speechtt-ctrl-s-{}", std::process::id()));
    std::fs::create_dir_all(&directory).expect("temp directory");
    let target = directory.join("meeting notes.md");
    let _ = std::fs::remove_file(&target);

    let prompt = common::ScriptedSavePrompt::answering(&target);
    let (main, view, events) =
        common::open_speechtt_window_with_save_prompt(cx, Box::new(prompt.clone()));
    prepare_live_take(&view, cx);

    press(cx, main.into(), "space");
    events
        .send_blocking(Ok(SttEvent::Committed {
            session_id: "take-1".into(),
            text: TRANSCRIPT.into(),
            segment: None,
        }))
        .expect("send committed text");
    cx.run_until_parked();
    press(cx, main.into(), "enter");
    assert_eq!(
        cx.update(|cx| view.read(cx).view_state()),
        ViewState::SessionEnd
    );

    press(cx, main.into(), "ctrl-s");

    assert_eq!(prompt.asks().len(), 1, "the dialog is asked once");
    assert_eq!(
        std::fs::read_to_string(&target).expect("the transcript is written"),
        "# meeting notes\n\nhello there\n"
    );
    assert!(
        !cx.update(|cx| view.read(cx).save_prompt_open()),
        "the prompt flag is cleared once it answers"
    );
    let _ = std::fs::remove_dir_all(&directory);
}

/// With nothing recorded there is no file to name, so the dialog is never
/// opened.
#[gpui_kit::test]
fn ctrl_s_without_a_finished_take_never_asks(cx: &mut TestAppContext) {
    let prompt = common::ScriptedSavePrompt::answering("/tmp/speechtt-never-written.md");
    let (main, view, _events) =
        common::open_speechtt_window_with_save_prompt(cx, Box::new(prompt.clone()));

    press(cx, main.into(), "ctrl-s");

    assert!(prompt.asks().is_empty(), "no dialog for an empty widget");
    assert!(!cx.update(|cx| view.read(cx).save_prompt_open()));
}

/// Two keystrokes in one frame cannot stack two dialogs.
#[gpui_kit::test]
fn a_second_ctrl_s_while_the_prompt_is_open_asks_once(cx: &mut TestAppContext) {
    let directory =
        std::env::temp_dir().join(format!("speechtt-ctrl-s-twice-{}", std::process::id()));
    std::fs::create_dir_all(&directory).expect("temp directory");
    let target = directory.join("twice.md");

    let prompt = common::ScriptedSavePrompt::answering(&target);
    let (main, view, events) =
        common::open_speechtt_window_with_save_prompt(cx, Box::new(prompt.clone()));
    prepare_live_take(&view, cx);
    press(cx, main.into(), "space");
    events
        .send_blocking(Ok(SttEvent::Committed {
            session_id: "take-1".into(),
            text: TRANSCRIPT.into(),
            segment: None,
        }))
        .expect("send committed text");
    cx.run_until_parked();
    press(cx, main.into(), "enter");

    cx.update_window(main.into(), |_, window, cx| {
        window.render_frame(cx);
        window.press("ctrl-s", cx);
        window.press("ctrl-s", cx);
    })
    .unwrap();
    cx.run_until_parked();

    assert_eq!(prompt.asks().len(), 1, "the second press is ignored");
    let _ = std::fs::remove_dir_all(&directory);
}

/// A dismissed dialog is the user's decision: the flag clears and no file is
/// written.
#[gpui_kit::test]
fn a_cancelled_save_prompt_writes_nothing_and_clears_the_flag(cx: &mut TestAppContext) {
    let target = unwritten_target("cancelled");
    let prompt = common::ScriptedSavePrompt::cancelled();
    let (main, view, events) =
        common::open_speechtt_window_with_save_prompt(cx, Box::new(prompt.clone()));
    finish_a_take(cx, main, &view, &events);

    press(cx, main.into(), "ctrl-s");

    assert_eq!(prompt.asks().len(), 1, "the dialog is asked once");
    assert!(
        !cx.update(|cx| view.read(cx).save_prompt_open()),
        "the prompt flag is cleared once it answers"
    );
    assert!(!target.exists(), "a cancelled dialog writes nothing");
    assert!(
        cx.update(|cx| view.read(cx).app_error().is_none()),
        "cancelling is not an error"
    );
}

/// A dialog that could not open is a failure, not a cancellation, and the user
/// must hear about it.
#[gpui_kit::test]
fn a_failed_save_prompt_surfaces_the_file_dialog_error(cx: &mut TestAppContext) {
    let prompt = common::ScriptedSavePrompt::failed();
    let (main, view, events) =
        common::open_speechtt_window_with_save_prompt(cx, Box::new(prompt.clone()));
    finish_a_take(cx, main, &view, &events);

    press(cx, main.into(), "ctrl-s");

    assert_eq!(prompt.asks().len(), 1, "the dialog is asked once");
    assert!(
        !cx.update(|cx| view.read(cx).save_prompt_open()),
        "the prompt flag is cleared once it answers"
    );
    assert_eq!(
        cx.update(|cx| view.read(cx).app_error().cloned()),
        Some(AppError::FileDialogFailed)
    );
}

// ----- Help rows --------------------------------------------------------

/// The Shortcuts tab draws one row per binding, from the table that registers
/// the keys: adding a row to `BINDINGS` adds it to the dialog.
///
/// Known limit: the assertion is by element id, so it proves each row exists.
/// It does not prove the row's label text is the binding's description.
#[gpui_kit::test]
fn the_help_shortcuts_tab_draws_every_binding(cx: &mut TestAppContext) {
    let (main, _view) = common::open_speechtt_window(cx);
    press(cx, main.into(), "f1");
    let help = dialog_window(cx, main);

    cx.update_window(help, |_, window, cx| {
        window.render_frame(cx);
        for binding in &speechtt_ui::shortcuts::BINDINGS {
            let id = format!("help-shortcut-{}", binding.action.as_str());
            assert!(
                window.try_find(SharedString::from(id)).is_some(),
                "the {} row is missing",
                binding.action
            );
        }
    })
    .unwrap();
}
