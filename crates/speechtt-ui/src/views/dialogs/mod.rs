//! The Help / Shortcuts and Settings dialogs.
//!
//! Each dialog is its own floating OS window ([`DialogWindow`]), opened beside
//! the widget rather than drawn over it. The window uses the same Header +
//! Body structure as the main widget ([`crate::components::AppHeader`]); the
//! view-model state lives in `crate::controller`, and these modules only
//! present it and forward intents.

pub(crate) mod help;
pub(crate) mod settings;
mod window;

pub(crate) use window::DialogWindow;

use gpui_kit::base::v_flex;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, App, IntoElement, RenderOnce, Window};

use crate::app::APP_TITLE;
use crate::controller::Dialog;

/// Fixed client size of both dialog windows, in logical pixels.
pub(crate) const DIALOG_WIDTH: f32 = 600.;
pub(crate) const HELP_DIALOG_HEIGHT: f32 = 520.;
pub(crate) const SETTINGS_DIALOG_HEIGHT: f32 = 520.;

/// Title used by the OS window and its in-window header.
pub(crate) fn window_title(dialog: Dialog) -> String {
    match dialog {
        Dialog::Help => format!("{APP_TITLE} - Help & Shortcuts"),
        Dialog::Settings => format!("{APP_TITLE} - Settings"),
    }
}

/// The body area shared by both dialog windows: scrollable content above an
/// optional fixed footer. The header is provided by the window root so all
/// windows share the same chrome.
#[derive(IntoElement)]
pub(crate) struct DialogShell {
    body: Vec<AnyElement>,
    footer: Option<AnyElement>,
}

impl DialogShell {
    pub(crate) fn new() -> Self {
        Self {
            body: Vec::new(),
            footer: None,
        }
    }

    pub(crate) fn body(mut self, content: impl IntoElement) -> Self {
        self.body.push(content.into_any_element());
        self
    }

    pub(crate) fn footer(mut self, footer: impl IntoElement) -> Self {
        self.footer = Some(footer.into_any_element());
        self
    }
}

impl RenderOnce for DialogShell {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        // Header (window root) → scrollable body → full-bleed footer. The
        // footer is a sibling of the padded body, never inside it, so it pins
        // to the window edge exactly like `AppHeader` does at the top.
        let body = v_flex()
            .flex_1()
            .min_h_0()
            .w_full()
            .gap_2()
            .p_5()
            .overflow_y_scrollbar()
            .children(self.body);

        let mut panel = v_flex().size_full().flex_1().min_h_0().w_full().child(body);
        if let Some(footer) = self.footer {
            panel = panel.child(footer);
        }
        panel
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dialog_titles_prefix_the_app_title() {
        assert_eq!(window_title(Dialog::Help), "SpeechTT - Help & Shortcuts");
        assert_eq!(window_title(Dialog::Settings), "SpeechTT - Settings");
    }
}
