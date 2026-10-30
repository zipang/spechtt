//! Rendering smoke tests.
//!
//! Their main job is to prove that every widget screen can be laid out and
//! painted without panicking, at the default size and at extreme window sizes.
//! They do not assert the presence, placement or labels of any element, so the
//! UI can be restructured freely without touching this file. Workflow behavior
//! is covered by the pure controller tests in `tests/session_flow.rs`.
//!
//! The one exception is the mode-chooser ripple: its counter lives on the view
//! and is the only headless proof that a click on a card / round button was
//! registered at all.

use gpui_kit::test::TestWindowExt;
use gpui_kit::{App, AppContext as _, Entity, TestAppContext, Window};
use speechtt_ui::app::SpeechTtApp;
use speechtt_ui::components::{ROUND_BUTTON_SIZE, ripple_frame};
use speechtt_ui::controller::{SessionMode, ViewState};

mod common;

use common::{open_speechtt_window, open_speechtt_window_at, open_speechtt_window_with_events};

/// Exercise every screen: mode → form → ready → recording → paused →
/// stopped → mode. Renders a frame after each transition.
fn drive_all_screens(view: &Entity<SpeechTtApp>, window: &mut Window, cx: &mut App) {
    // Blank → mode chooser.
    window.render_frame(cx);
    assert_eq!(view.read(cx).view_state(), ViewState::Mode);

    // Mode chooser → setup form (no settings configured in the test).
    view.update(cx, |view, cx| view.choose_mode(SessionMode::Live, cx));
    window.render_frame(cx);
    assert_eq!(view.read(cx).view_state(), ViewState::Form);

    // Fill the form so `Continue` is live.
    view.update(cx, |view, cx| {
        view.cycle_provider(cx);
        view.cycle_model(cx);
    });
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
    assert_eq!(view.read(cx).view_state(), ViewState::Mode);
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
    // panic or require a fixed box.
    for (width, height) in [(320.0, 190.0), (960.0, 640.0)] {
        let (window, view) = open_speechtt_window_at(cx, width, height);

        cx.update_window(window.into(), |_, window, cx| {
            drive_all_screens(&view, window, cx);
        })
        .unwrap();
    }
}

// ----- T17: mode chooser ripple ----------------------------------------

#[test]
fn ripple_frames_expand_and_fade_monotonically() {
    let (start_diameter, start_alpha) = ripple_frame(0.0);
    assert_eq!(start_diameter, ROUND_BUTTON_SIZE);
    assert!(start_alpha > 0.0);

    let mut previous = start_diameter;
    for step in 1..=10 {
        let delta = step as f32 / 10.0;
        let (diameter, alpha) = ripple_frame(delta);
        assert!(diameter > previous, "diameter must grow at {delta}");
        assert!(diameter <= 84.0, "diameter must stay bounded at {delta}");
        assert!(
            alpha < ripple_frame(delta - 0.1).1,
            "alpha must fade at {delta}"
        );
        previous = diameter;
    }

    let (end_diameter, end_alpha) = ripple_frame(1.0);
    assert_eq!(end_diameter, 84.0);
    assert_eq!(end_alpha, 0.0);
}

#[test]
fn ripple_frames_clamp_out_of_range_deltas() {
    assert_eq!(ripple_frame(-1.0), ripple_frame(0.0));
    assert_eq!(ripple_frame(2.0), ripple_frame(1.0));
}

#[gpui_kit::test]
fn clicking_a_mode_card_bumps_its_pulse_counter(cx: &mut TestAppContext) {
    let (window, view) = open_speechtt_window(cx);

    let counts = |cx: &mut TestAppContext| {
        cx.update(|cx| {
            let app = view.read(cx);
            (
                app.pulse_count("btn-mode-live"),
                app.pulse_count("btn-mode-file"),
            )
        })
    };
    assert_eq!(counts(cx), (0, 0));

    view.update(cx, |view, _cx| view.pulse("btn-mode-live"));
    assert_eq!(counts(cx), (1, 0));

    view.update(cx, |view, _cx| view.pulse("btn-mode-live"));
    assert_eq!(counts(cx), (2, 0));

    // The chooser still renders with an active ripple.
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
    })
    .unwrap();
}

#[gpui_kit::test]
fn the_ripple_renders_on_every_round_button(cx: &mut TestAppContext) {
    let (window, view) = open_speechtt_window(cx);

    view.update(cx, |view, cx| {
        view.pulse("btn-mode-live");
        view.pulse("btn-mode-file");
        view.choose_mode(SessionMode::Live, cx);
        view.cycle_provider(cx);
        view.cycle_model(cx);
        view.create_session(cx);
        view.pulse("btn-record");
    });

    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
    })
    .unwrap();

    cx.update(|cx| {
        let app = view.read(cx);
        assert_eq!(app.pulse_count("btn-record"), 1);
        assert_eq!(app.view_state(), ViewState::Ready);
    });
}

#[gpui_kit::test]
fn saving_from_the_stopped_screen_writes_the_chosen_path(cx: &mut TestAppContext) {
    let directory =
        std::env::temp_dir().join(format!("speechtt-widget-save-{}", std::process::id()));
    std::fs::create_dir_all(&directory).expect("temp dir");
    let target = directory.join("Board notes.md");
    let _ = std::fs::remove_file(&target);

    let (window, view, events) = open_speechtt_window_with_events(cx);
    cx.update_window(window.into(), |_, window, cx| {
        view.update(cx, |app, cx| {
            app.choose_mode(SessionMode::Live, cx);
            app.cycle_provider(cx);
            app.cycle_model(cx);
            app.create_session(cx);
            app.start_recording(cx);
        });
        window.render_frame(cx);
    })
    .unwrap();

    events
        .send_blocking(Ok(speechtt_core::protocol::SttEvent::Committed {
            session_id: "take-1".into(),
            text: "board decisions".into(),
        }))
        .expect("event sent");
    cx.run_until_parked();

    cx.update_window(window.into(), |_, window, cx| {
        view.update(cx, |app, cx| {
            app.pause_recording(cx);
            app.finish_take(cx);
            assert_eq!(app.view_state(), ViewState::Stopped);
            app.save_transcript(cx);
        });
        window.render_frame(cx);
    })
    .unwrap();

    // Answer the native save dialog with the target path.
    let chosen = target.clone();
    cx.simulate_new_path_selection(move |_suggested_dir| Some(chosen.clone()));
    cx.run_until_parked();

    assert!(target.exists(), "the save dialog path is written");
    cx.update(|cx| {
        let app = view.read(cx);
        assert_eq!(app.written_path(), Some(target.as_path()));
    });

    let contents = std::fs::read_to_string(&target).expect("saved file");
    assert_eq!(contents, "# Board notes\n\nboard decisions\n");

    let _ = std::fs::remove_dir_all(&directory);
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
fn restored_bounds_are_raised_to_the_minimum_window_size() {
    // Usable for the core (>= 420x320) but too small for the mode chooser.
    let layout = WindowLayout::new(430, 330, 40, 30);
    let bounds = speechtt_ui::restored_window_bounds(Some(layout), &[]);

    assert_eq!(
        f32::from(bounds.size.width),
        speechtt_ui::tokens::MIN_WINDOW_WIDTH
    );
    assert_eq!(
        f32::from(bounds.size.height),
        speechtt_ui::tokens::MIN_WINDOW_HEIGHT
    );
    // The remembered origin is kept: only the size is raised.
    assert_eq!(f32::from(bounds.origin.x), 40.0);
    assert_eq!(f32::from(bounds.origin.y), 30.0);
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
