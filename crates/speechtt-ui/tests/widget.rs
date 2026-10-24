//! Widget shell tests (headless).
//!
//! Per PRODUCT.md §5 UI test policy: test the *function*, not the pixels.
//! We assert that the labeled controls exist and are visible, and that
//! interactions have the intended effect. We never assert placement,
//! relative geometry, or pixel output — the UI must stay free to evolve
//! (add/remove/rearrange controls) without breaking these tests.
//! Mockups (`mockups/**/*.png`) are compared during *manual* visual QA.

use gpui_kit::test::TestWindowExt;
use gpui_kit::{AppContext as _, TestAppContext};

mod common;

use common::open_speechtt_window;

#[gpui_kit::test]
fn the_header_shows_the_title_and_the_two_action_controls(cx: &mut TestAppContext) {
    let (window, _view) = open_speechtt_window(cx);

    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);

        // The ElementIds are the test contract (they must stay stable);
        // labels and wording are free to change.
        assert!(
            window.try_find("app-title").is_some(),
            "the header shows the app title"
        );
        assert!(
            window.try_find("btn-help").is_some(),
            "the header shows the help/shortcuts control"
        );
        assert!(
            window.try_find("btn-settings").is_some(),
            "the header shows the settings control"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn clicking_the_action_controls_leaves_the_app_running(cx: &mut TestAppContext) {
    let (window, _view) = open_speechtt_window(cx);

    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);

        // The dialogs are not implemented yet (next Phase 2 tasks) —
        // clicking the stub controls must not crash the app. Once the
        // dialogs land, these clicks will be asserted to open/close them
        // (behavior only).
        window.click("btn-help", cx);
        window.click("btn-settings", cx);
        window.render_frame(cx);

        assert!(
            window.try_find("app-title").is_some(),
            "the window still renders after the stub clicks"
        );
    })
    .unwrap();
}
