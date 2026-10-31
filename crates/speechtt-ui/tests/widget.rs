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
use gpui_kit::{
    App, AppContext as _, Entity, Modifiers, MouseButton, TestAppContext, VisualTestContext, Window,
};
use speechtt_ui::app::SpeechTtApp;
use speechtt_ui::components::{
    FooterZone, RIPPLE_MAX_SIZE, RIPPLE_START_SIZE, RIPPLE_TRAVEL, ROUND_BUTTON_SIZE, Ripple,
    ScreenFooter, ripple_frame,
};
use speechtt_ui::controller::{SessionMode, ViewState};

mod common;

use common::{open_speechtt_window, open_speechtt_window_at, open_speechtt_window_with_events};

/// Exercise every screen: mode → form → live (idle → recording → paused) →
/// stopped → mode. Renders a frame after each transition. The live take screen
/// is one screen in three states, so each step asserts the view state stays
/// `Live` while the machine changes.
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

    // Form → live (idle) → recording → paused: one screen throughout (T0021).
    view.update(cx, |view, cx| view.create_session(cx));
    window.render_frame(cx);
    assert_eq!(view.read(cx).view_state(), ViewState::Live);

    view.update(cx, |view, cx| view.start_recording(cx));
    window.render_frame(cx);
    assert_eq!(view.read(cx).view_state(), ViewState::Live);

    view.update(cx, |view, cx| view.pause_recording(cx));
    window.render_frame(cx);
    assert_eq!(view.read(cx).view_state(), ViewState::Live);

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
    assert_eq!(
        start_diameter, RIPPLE_START_SIZE,
        "the wave starts at exactly the button diameter"
    );
    assert_eq!(
        start_diameter, ROUND_BUTTON_SIZE,
        "and the button diameter is its starting point"
    );
    assert_eq!(
        start_alpha, 1.0,
        "it starts fully opaque, hidden by the button"
    );

    let mut previous = start_diameter;
    for step in 1..=10 {
        let delta = step as f32 / 10.0;
        let (diameter, alpha) = ripple_frame(delta);
        assert!(diameter > previous, "diameter must grow at {delta}");
        assert!(
            diameter <= RIPPLE_MAX_SIZE,
            "diameter must stay bounded at {delta}"
        );
        assert!(
            alpha < ripple_frame(delta - 0.1).1,
            "alpha must fade at {delta}"
        );
        previous = diameter;
    }

    let (end_diameter, end_alpha) = ripple_frame(1.0);
    assert_eq!(end_diameter, RIPPLE_MAX_SIZE);
    assert_eq!(
        end_diameter,
        ROUND_BUTTON_SIZE + RIPPLE_TRAVEL,
        "it fades to transparent after travelling RIPPLE_TRAVEL"
    );
    assert_eq!(end_alpha, 0.0);

    // Opacity is the travelled radius: half-way through it is half transparent.
    let (_, half_alpha) = ripple_frame(0.5);
    assert_eq!(half_alpha, 0.5);
}

#[test]
fn ripple_frames_clamp_out_of_range_deltas() {
    assert_eq!(ripple_frame(-1.0), ripple_frame(0.0));
    assert_eq!(ripple_frame(2.0), ripple_frame(1.0));
}

/// `(ripple, state)` in one hop.
fn snapshot(cx: &TestAppContext, view: &Entity<SpeechTtApp>) -> (Option<Ripple>, ViewState) {
    cx.update(|app| {
        let app = view.read(app);
        (app.current_ripple(), app.view_state())
    })
}

/// The ripple must be started by the real press, must keep animating after the
/// screen changes, and must visibly grow. Each clause is a bug that shipped
/// once: no ripple was ever drawn (dead counter), the ring was squeezed back to
/// the button size (flex shrink), and the ring died on navigation (owned by the
/// pressed screen).
#[gpui_kit::test]
fn pressing_a_mode_card_starts_a_wave_centred_on_it_that_outlives_the_screen_change(
    cx: &mut TestAppContext,
) {
    let (window, view) = open_speechtt_window(cx);
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.run_until_parked();

    let (ripple, state) = snapshot(&visual.cx, &view);
    assert_eq!(ripple, None, "no ripple before the first press");
    assert_eq!(state, ViewState::Mode);

    let card = visual.debug_bounds("mode-live").expect("live card bounds");
    let button = visual
        .debug_bounds("btn-mode-live")
        .expect("live button bounds");

    // Press deliberately near the card's top-left corner, far from the button:
    // the wave must still be centred on the button, not on the pointer.
    let press_point = card.center() - gpui_kit::point(gpui_kit::px(60.0), gpui_kit::px(40.0));
    assert!(
        !button.contains(&press_point),
        "the press point must be outside the button for this assertion to mean anything"
    );

    // Press only: the ripple starts, the mode has not been chosen yet.
    visual.simulate_mouse_down(press_point, MouseButton::Left, Modifiers::none());
    let (ripple, state) = snapshot(&visual.cx, &view);
    let ripple = ripple.expect("pressing the card must start a ripple");
    assert_eq!(
        (ripple.x, ripple.y),
        (f32::from(button.center().x), f32::from(button.center().y)),
        "the wave is concentric with the button, not with the pointer"
    );
    assert_eq!(
        state,
        ViewState::Mode,
        "the mode changes on click, not on press"
    );

    // The ring is in the painted frame at the button diameter, and growing.
    // The wave is in the painted frame, at the button diameter — identical to
    // the button, so it is hidden until it grows. Growth and the fade are the
    // pure `ripple_frame` spec; this test owns the wiring.
    let start = visual
        .debug_bounds("ripple-ring")
        .expect("the wave must be laid out after a press")
        .size
        .width;
    assert_eq!(
        f32::from(start),
        RIPPLE_START_SIZE,
        "the wave starts at the button diameter"
    );

    // Release: the command runs, and the ring is still there — it belongs to
    // the window, so the screen change must not kill it.
    visual.simulate_mouse_up(press_point, MouseButton::Left, Modifiers::none());
    let (ripple, state) = snapshot(&visual.cx, &view);
    assert_eq!(state, ViewState::Form);
    assert!(ripple.is_some(), "the ring survives the screen change");
    let after = visual
        .debug_bounds("ripple-ring")
        .expect("the ring keeps painting on the new screen");
    assert!(
        f32::from(after.size.width) >= RIPPLE_START_SIZE,
        "the ring keeps its size on the new screen"
    );
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

// ----- T21: the shared screen footer -----------------------------------

#[test]
fn a_fresh_screen_footer_is_empty() {
    let footer = ScreenFooter::new();

    assert!(footer.is_empty());
    assert!(!footer.has_back());
    assert_eq!(footer.back_label(), None);
    for zone in [
        FooterZone::Leading,
        FooterZone::Primary,
        FooterZone::Trailing,
    ] {
        assert_eq!(footer.zone_len(zone), 0, "{zone:?}");
    }
}

#[test]
fn back_fills_the_leading_zone() {
    let footer = ScreenFooter::new().back("btn-back", "Back", |_, _, _| {});

    assert!(footer.has_back());
    assert_eq!(footer.back_label(), Some("Back"));
    assert_eq!(footer.zone_len(FooterZone::Leading), 1);
    assert_eq!(footer.zone_len(FooterZone::Primary), 0);
    assert!(!footer.is_empty());
}

#[test]
fn each_builder_fills_only_its_own_zone() {
    let footer = ScreenFooter::new()
        .leading(gpui_kit::div())
        .primary(gpui_kit::div())
        .trailing(gpui_kit::div())
        .trailing(gpui_kit::div());

    assert_eq!(footer.zone_len(FooterZone::Leading), 1);
    assert_eq!(footer.zone_len(FooterZone::Primary), 1);
    // Trailing actions keep insertion order by construction (a Vec).
    assert_eq!(footer.zone_len(FooterZone::Trailing), 2);
    assert!(!footer.has_back(), "a plain leading slot is not a back");
}

#[gpui_kit::test]
fn back_returns_to_the_mode_chooser_from_every_screen(cx: &mut TestAppContext) {
    let (window, view) = open_speechtt_window(cx);

    cx.update_window(window.into(), |_, window, cx| {
        // Mode → form: back discards the draft.
        view.update(cx, |view, cx| view.choose_mode(SessionMode::Live, cx));
        window.render_frame(cx);
        assert_eq!(view.read(cx).view_state(), ViewState::Form);
        view.update(cx, |view, cx| view.go_back(cx));
        window.render_frame(cx);
        assert_eq!(view.read(cx).view_state(), ViewState::Mode);
        assert!(!view.read(cx).is_draft_complete());

        // Mode → live idle: back returns home without starting a take.
        view.update(cx, |view, cx| {
            view.choose_mode(SessionMode::Live, cx);
            view.cycle_provider(cx);
            view.cycle_model(cx);
            view.create_session(cx);
        });
        window.render_frame(cx);
        assert_eq!(view.read(cx).view_state(), ViewState::Live);
        view.update(cx, |view, cx| view.go_back(cx));
        window.render_frame(cx);
        assert_eq!(view.read(cx).view_state(), ViewState::Mode);

        // A finished take: back clears it instead of showing it again.
        view.update(cx, |view, cx| {
            view.choose_mode(SessionMode::Live, cx);
            view.cycle_provider(cx);
            view.cycle_model(cx);
            view.create_session(cx);
            view.start_recording(cx);
            view.pause_recording(cx);
            view.finish_take(cx);
        });
        window.render_frame(cx);
        assert_eq!(view.read(cx).view_state(), ViewState::Stopped);
        view.update(cx, |view, cx| view.go_back(cx));
        window.render_frame(cx);
        assert_eq!(view.read(cx).view_state(), ViewState::Mode);
        assert!(view.read(cx).finished_take().is_none());
    })
    .unwrap();
}
