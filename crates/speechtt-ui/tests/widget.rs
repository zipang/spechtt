//! Rendering smoke test.
//!
//! The only job of this test is to prove that every widget screen can be laid
//! out and painted without panicking. It does not assert the presence,
//! placement or labels of any element, so the UI can be restructured freely
//! without touching this file. Workflow behavior is covered by the pure
//! controller tests in `tests/session_flow.rs`.

use gpui_kit::test::TestWindowExt;
use gpui_kit::{AppContext as _, TestAppContext};
use speechtt_core::session::OutputTarget;
use speechtt_ui::controller::ViewState;

mod common;

use common::open_speechtt_window;

#[gpui_kit::test]
fn every_screen_renders_without_panicking(cx: &mut TestAppContext) {
    let (window, view) = open_speechtt_window(cx);

    cx.update_window(window.into(), |_, window, cx| {
        // Blank → prompt.
        window.render_frame(cx);
        assert_eq!(view.read(cx).view_state(), ViewState::Prompt);

        // Prompt → setup form.
        view.update(cx, |view, cx| view.open_form(window, cx));
        window.render_frame(cx);
        assert_eq!(view.read(cx).view_state(), ViewState::Form);

        // Fill the form so `Create session` is live.
        view.update(cx, |view, cx| {
            view.cycle_provider(cx);
            view.cycle_model(cx);
            view.select_output(OutputTarget::Memory, cx);
        });
        view.update(cx, |view, cx| view.set_draft_name("standup", window, cx));
        window.render_frame(cx);

        // Form → ready → recording → paused.
        view.update(cx, |view, cx| view.create_session(cx));
        window.render_frame(cx);
        assert_eq!(view.read(cx).view_state(), ViewState::Ready);

        view.update(cx, |view, cx| view.start_recording(cx));
        window.render_frame(cx);
        assert_eq!(view.read(cx).view_state(), ViewState::Recording);

        view.update(cx, |view, cx| view.pause_recording(cx));
        window.render_frame(cx);
        assert_eq!(view.read(cx).view_state(), ViewState::Paused);

        // Paused → stopped result → new take.
        view.update(cx, |view, cx| view.finish_take(cx));
        window.render_frame(cx);
        assert_eq!(view.read(cx).view_state(), ViewState::Stopped);

        view.update(cx, |view, cx| view.new_take(cx));
        window.render_frame(cx);
        assert_eq!(view.read(cx).view_state(), ViewState::Prompt);
    })
    .unwrap();
}
