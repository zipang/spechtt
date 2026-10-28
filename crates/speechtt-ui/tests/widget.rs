//! Rendering smoke tests.
//!
//! Their only job is to prove that every widget screen can be laid out and
//! painted without panicking, at the default size and at extreme window sizes.
//! They do not assert the presence, placement or labels of any element, so the
//! UI can be restructured freely without touching this file. Workflow behavior
//! is covered by the pure controller tests in `tests/session_flow.rs`.

use gpui_kit::test::TestWindowExt;
use gpui_kit::{App, AppContext as _, Entity, TestAppContext, Window};
use speechtt_core::session::OutputTarget;
use speechtt_ui::app::SpeechTtApp;
use speechtt_ui::controller::ViewState;

mod common;

use common::{open_speechtt_window, open_speechtt_window_at};

/// Exercise every screen: prompt → form → ready → recording → paused →
/// stopped → prompt. Renders a frame after each transition.
fn drive_all_screens(view: &Entity<SpeechTtApp>, window: &mut Window, cx: &mut App) {
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
}

#[gpui_kit::test]
fn every_screen_renders_without_panicking(cx: &mut TestAppContext) {
    let (window, view) = open_speechtt_window(cx);

    cx.update_window(window.into(), |_, window, cx| {
        drive_all_screens(&view, window, cx);
    })
    .unwrap();
}

#[gpui_kit::test]
fn every_screen_renders_at_extreme_window_sizes(cx: &mut TestAppContext) {
    // Narrow/tall and wide/short stress the flex constraints: nothing may
    // panic or require a fixed 480×280 box.
    for (width, height) in [(320.0, 190.0), (960.0, 640.0)] {
        let (window, view) = open_speechtt_window_at(cx, width, height);

        cx.update_window(window.into(), |_, window, cx| {
            drive_all_screens(&view, window, cx);
        })
        .unwrap();
    }
}

// ----- T13: restored window geometry -----------------------------------

use gpui_kit::{Bounds, Point, Size, px};
use speechtt_core::settings::WindowLayout;

fn display(width: f32, height: f32) -> Bounds<gpui_kit::Pixels> {
    Bounds {
        origin: Point::default(),
        size: Size {
            width: px(width),
            height: px(height),
        },
    }
}

#[test]
fn restored_bounds_use_the_default_without_a_usable_layout() {
    let default = speechtt_ui::default_window_bounds();

    assert_eq!(speechtt_ui::restored_window_bounds(None, &[]), default);
    let tiny = WindowLayout::new(100, 100, 10, 10);
    assert_eq!(
        speechtt_ui::restored_window_bounds(Some(tiny), &[]),
        default
    );
}

#[test]
fn restored_bounds_restore_size_and_position() {
    let layout = WindowLayout::new(700, 600, 120, 80);
    let bounds = speechtt_ui::restored_window_bounds(Some(layout), &[]);

    assert_eq!(f32::from(bounds.size.width), 700.0);
    assert_eq!(f32::from(bounds.size.height), 600.0);
    assert_eq!(f32::from(bounds.origin.x), 120.0);
    assert_eq!(f32::from(bounds.origin.y), 80.0);
}

#[test]
fn restored_bounds_are_pulled_back_into_the_visible_display() {
    let layout = WindowLayout::new(480, 520, 5000, -300);
    let bounds = speechtt_ui::restored_window_bounds(Some(layout), &[display(1920.0, 1080.0)]);

    assert_eq!(f32::from(bounds.origin.x), 1920.0 - 60.0);
    assert_eq!(f32::from(bounds.origin.y), 0.0);
}

#[test]
fn restored_bounds_never_exceed_the_display() {
    let layout = WindowLayout::new(4000, 3000, 0, 0);
    let bounds = speechtt_ui::restored_window_bounds(Some(layout), &[display(1920.0, 1080.0)]);

    assert_eq!(f32::from(bounds.size.width), 1920.0);
    assert_eq!(f32::from(bounds.size.height), 1080.0);
}

/// The New Session form must fit the default window without scrolling.
#[gpui_kit::test]
fn the_default_window_height_fits_the_new_session_form(cx: &mut TestAppContext) {
    let (window, _view) = open_speechtt_window(cx);

    cx.update_window(window.into(), |_, window, _| {
        let line = f32::from(window.line_height());
        // Mirrors setting_row / app_header / app_footer and the form padding.
        let row = 24.0 + 2.0 * line + 2.0 + 1.0;
        let body = 40.0 + 4.0 * row + 3.0 * 8.0;
        let header = 16.0 + line + 1.0;
        let footer = 20.0 + 32.0 + 1.0;
        let error_strip = 12.0 + line + 2.0;
        let required = body + header + footer + error_strip;

        assert!(
            speechtt_ui::tokens::WIDGET_HEIGHT >= required,
            "default height {} does not fit the setup form ({required})",
            speechtt_ui::tokens::WIDGET_HEIGHT
        );
    })
    .unwrap();
}
