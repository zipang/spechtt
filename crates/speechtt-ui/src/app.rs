//! The SpeechTT widget window view.
//!
//! Phase 2 skeleton: the top strip (title + `?` and cog icon buttons) and
//! a body placeholder. Idle/Recording/Paused views and the Settings/Help
//! dialogs are built on this shell from mock state (no backend — see
//! PRODUCT.md §6). ElementIds here are the test contract — they must stay
//! stable (`tests/widget.rs`).

use gpui_kit::assets::IconName;
use gpui_kit::base::{h_flex, v_flex};
use gpui_kit::component::button::Button;
use gpui_kit::component::{Icon, ActiveTheme as _};
use gpui_kit::prelude::*;
use gpui_kit::{FontWeight, Window, TestSupportExt as _, div, px};

/// Application title shown in the header strip.
pub const APP_TITLE: &str = "SpeechTT";

/// Root view of the widget window. Owns mock recording state until the
/// core crate is wired in (Phase 8).
pub struct SpeechTtApp;

impl SpeechTtApp {
    pub fn new() -> Self {
        Self
    }
}

impl Default for SpeechTtApp {
    fn default() -> Self {
        Self::new()
    }
}

impl gpui_kit::Render for SpeechTtApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl gpui_kit::IntoElement {
        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            // Top strip — `--color-surface-dark` via the `title_bar` role.
            .child(
                h_flex()
                    .justify_between()
                    .items_center()
                    .px_4()
                    .py_2()
                    .bg(cx.theme().title_bar)
                    .border_b_1()
                    .border_color(cx.theme().title_bar_border)
                    .child(
                        div()
                            .id("app-title")
                            .test_support()
                            .font_weight(FontWeight::BOLD)
                            .child(APP_TITLE),
                    )
                    .child(
                        h_flex().gap_1().child(help_button()).child(settings_button()),
                    ),
            )
            // Body placeholder — Idle view per mockups/app-window/idle.png
            // lands in its own task.
            .child(
                v_flex()
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .id("body-placeholder")
                            .test_support()
                            .aria_label("Idle")
                            .text_color(cx.theme().muted_foreground)
                            .text_size(px(12.8))
                            .child("Idle"),
                    ),
            )
    }
}

/// The `?` (help/shortcuts) header button. Opens the Help dialog in its
/// own Phase 2 task; a no-op stub for now.
fn help_button() -> gpui_kit::AnyElement {
    Button::new("btn-help")
        .icon(Icon::new(IconName::CircleQuestionMark))
        .accessibility_label("Open help and shortcuts")
        .into_any_element()
}

/// The cog (settings) header button. Same stub status.
fn settings_button() -> gpui_kit::AnyElement {
    Button::new("btn-settings")
        .icon(Icon::new(IconName::Settings))
        .accessibility_label("Open settings")
        .into_any_element()
}
