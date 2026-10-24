//! Session setup flow (headless, behavior only).
//!
//! Per PRODUCT.md §5 policy: assert transitions and screen changes found by
//! stable ElementIds. Never assert labels, placement or pixels.
//!
//! Covers the two screens inferred for the Blank state (no mockup frame):
//! the "New Session" prompt and the setup form.

use gpui_kit::test::TestWindowExt;
use gpui_kit::{AppContext as _, TestAppContext};
use speechtt_core::session::SessionState;

mod common;

use common::open_speechtt_window;

/// Read the view's machine state through the test app context.
fn state(
    view: &gpui_kit::Entity<speechtt_ui::app::SpeechTtApp>,
    cx: &mut TestAppContext,
) -> SessionState {
    cx.update(|cx| view.read(cx).session_state())
}

#[gpui_kit::test]
fn blank_state_shows_the_new_session_prompt(cx: &mut TestAppContext) {
    let (window, _view) = open_speechtt_window(cx);

    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("blank-prompt").is_some());
        assert!(window.try_find("btn-new-session").is_some());
        // No transport until a session exists.
        assert!(window.try_find("btn-record").is_none());
    })
    .unwrap();
}

#[gpui_kit::test]
fn clicking_new_session_opens_the_setup_form(cx: &mut TestAppContext) {
    let (window, _view) = open_speechtt_window(cx);

    cx.update_window(window.into(), |_, window, cx| {
        window.click("btn-new-session", cx);
        assert!(window.try_find("session-form").is_some());
        assert!(window.try_find("btn-create-session").is_some());
    })
    .unwrap();
}

#[gpui_kit::test]
fn cancelling_the_form_returns_to_the_prompt(cx: &mut TestAppContext) {
    let (window, view) = open_speechtt_window(cx);

    cx.update_window(window.into(), |_, window, cx| {
        window.click("btn-new-session", cx);
        window.click("btn-cancel-session", cx);
        assert!(window.try_find("blank-prompt").is_some());
    })
    .unwrap();

    assert_eq!(state(&view, cx), SessionState::Blank);
}

#[gpui_kit::test]
fn create_is_inert_until_every_required_field_is_set(cx: &mut TestAppContext) {
    let (window, view) = open_speechtt_window(cx);

    cx.update_window(window.into(), |_, window, cx| {
        window.click("btn-new-session", cx);

        // Nothing chosen yet: click must not create a session.
        window.click("btn-create-session", cx);
        assert!(window.try_find("session-form").is_some());

        // Partial setup (provider only) is still not enough.
        window.click("field-provider", cx);
        window.click("btn-create-session", cx);
        assert!(window.try_find("session-form").is_some());
    })
    .unwrap();

    assert_eq!(state(&view, cx), SessionState::Blank);
}

#[gpui_kit::test]
fn completing_the_form_moves_the_machine_to_ready(cx: &mut TestAppContext) {
    let (window, view) = open_speechtt_window(cx);

    cx.update_window(window.into(), |_, window, cx| {
        window.click("btn-new-session", cx);
        window.click("field-provider", cx);
        window.click("field-model", cx);
        window.click("field-output-memory", cx);
        window.click("input-session-name", cx);
        window.input("meeting notes", cx);
        window.click("btn-create-session", cx);

        // Form gone, Ready transport visible.
        assert!(window.try_find("session-form").is_none());
        assert!(window.try_find("btn-record").is_some());
        assert!(view.read(cx).session_missing().is_empty());
    })
    .unwrap();

    assert_eq!(state(&view, cx), SessionState::Ready);
}

#[gpui_kit::test]
fn recording_pausing_finishing_memory_take(cx: &mut TestAppContext) {
    let (window, view) = open_speechtt_window(cx);

    cx.update_window(window.into(), |_, window, cx| {
        // Configure a session.
        window.click("btn-new-session", cx);
        window.click("field-provider", cx);
        window.click("field-model", cx);
        window.click("field-output-memory", cx);
        window.click("input-session-name", cx);
        window.input("standup", cx);
        window.click("btn-create-session", cx);
        assert_eq!(view.read(cx).session_state(), SessionState::Ready);

        // Ready → Recording → Paused → Recording.
        window.click("btn-record", cx);
        assert_eq!(view.read(cx).session_state(), SessionState::Recording);
        window.click("btn-pause", cx);
        assert_eq!(view.read(cx).session_state(), SessionState::Paused);
        window.click("btn-resume", cx);
        assert_eq!(view.read(cx).session_state(), SessionState::Recording);

        // Recording and Paused share the same two controls so the layout
        // does not move: Play/Pause + Stop. Stop is inert while recording.
        assert!(window.try_find("btn-pause").is_some());
        assert!(window.try_find("btn-stop").is_some());
        assert!(window.try_find("btn-copy").is_none());
        assert!(window.try_find("btn-save").is_none());
        window.click("btn-stop", cx);
        assert_eq!(view.read(cx).session_state(), SessionState::Recording);

        window.click("btn-pause", cx);
        assert_eq!(view.read(cx).session_state(), SessionState::Paused);

        // Paused: same row, Stop now live; copy/save wait for Stopped.
        assert!(window.try_find("btn-resume").is_some());
        assert!(window.try_find("btn-stop").is_some());
        assert!(window.try_find("btn-copy").is_none());
        assert!(window.try_find("btn-save").is_none());

        // Finish (memory mode) → Stopped screen with copy/save.
        window.click("btn-stop", cx);
        assert_eq!(view.read(cx).session_state(), SessionState::Blank);
        assert!(window.try_find("result-body").is_some());
        assert!(window.try_find("btn-copy").is_some());
        assert!(window.try_find("btn-save").is_some());
        assert!(window.try_find("btn-open-file").is_none());

        // New take returns to the Blank prompt (name + output required again).
        window.click("btn-new-take", cx);
        assert!(window.try_find("blank-prompt").is_some());
        let missing_fields = view.read(cx).session_missing();
        assert_eq!(
            missing_fields,
            vec![
                speechtt_core::session::SessionField::SessionName,
                speechtt_core::session::SessionField::Output,
            ]
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn file_take_shows_only_the_path_and_an_open_file_control(cx: &mut TestAppContext) {
    let (window, view) = open_speechtt_window(cx);

    cx.update_window(window.into(), |_, window, cx| {
        // Configure a "to file" session named standup.
        window.click("btn-new-session", cx);
        window.click("field-provider", cx);
        window.click("field-model", cx);
        window.click("field-output-file", cx);
        window.click("input-session-name", cx);
        window.input("standup", cx);
        window.click("btn-create-session", cx);
        assert_eq!(view.read(cx).session_state(), SessionState::Ready);

        window.click("btn-record", cx);
        window.click("btn-pause", cx);
        window.click("btn-stop", cx);

        // The Stopped screen for a file take shows the written path only:
        // no transcript, no copy/save — just the way to open the file.
        assert!(window.try_find("result-body").is_some());
        assert!(window.try_find("result-path").is_some());
        assert!(window.try_find("btn-open-file").is_some());
        assert!(window.try_find("btn-copy").is_none());
        assert!(window.try_find("btn-save").is_none());
        assert!(window.try_find("caption-well").is_none());
    })
    .unwrap();
}
