//! The application footer strip shared by the widget and dialog windows.
//!
//! Sibling of the body, never inside its padding box: an optional leading
//! status slot on the left and trailing actions on the right, drawn on the
//! `title_bar` role with a top hairline. This is the bottom counterpart to
//! [`crate::components::AppHeader`], so every window keeps the same
//! Header + Body + Footer structure.

use gpui_kit::base::h_flex;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, App, IntoElement, RenderOnce, Window};

/// Bottom strip of an application window: optional leading status and
/// trailing actions.
#[derive(IntoElement)]
pub struct AppFooter {
    leading: Option<AnyElement>,
    actions: Vec<AnyElement>,
}

impl AppFooter {
    pub fn new() -> Self {
        Self {
            leading: None,
            actions: Vec::new(),
        }
    }

    /// Adds the left-hand slot (version, gateway status, selection summary…).
    pub fn leading(mut self, leading: impl IntoElement) -> Self {
        self.leading = Some(leading.into_any_element());
        self
    }

    /// Adds a trailing action, rendered left to right in insertion order.
    pub fn action(mut self, action: impl IntoElement) -> Self {
        self.actions.push(action.into_any_element());
        self
    }
}

impl Default for AppFooter {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderOnce for AppFooter {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let has_leading = self.leading.is_some();
        let mut row = h_flex()
            .w_full()
            .flex_shrink_0()
            .items_center()
            .px_5()
            .py_2p5()
            .bg(cx.theme().title_bar)
            .border_t_1()
            .border_color(cx.theme().title_bar_border);

        row = if has_leading {
            row.justify_between()
        } else {
            row.justify_end()
        };

        if let Some(leading) = self.leading {
            row = row.child(leading);
        }

        row.child(h_flex().gap_2().children(self.actions))
    }
}
