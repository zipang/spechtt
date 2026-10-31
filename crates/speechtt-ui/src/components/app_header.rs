//! The application header strip shared by the widget and dialog windows.
//!
//! One bold title on the left, optional actions (icon buttons) on the right,
//! drawn on the `title_bar` role with a bottom border. Window-specific chrome
//! such as a close button is passed in by the caller so every window keeps
//! the same Header + Body structure.

use gpui_kit::base::h_flex;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, App, IntoElement, RenderOnce, SharedString, Window};

use crate::components::Body;

/// Top strip of an application window: title and trailing actions.
#[derive(IntoElement)]
pub struct AppHeader {
    title: SharedString,
    actions: Vec<AnyElement>,
}

impl AppHeader {
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            actions: Vec::new(),
        }
    }

    /// Adds a trailing action, rendered left to right in insertion order.
    pub fn action(mut self, action: impl IntoElement) -> Self {
        self.actions.push(action.into_any_element());
        self
    }
}

impl RenderOnce for AppHeader {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        h_flex()
            .justify_between()
            .items_center()
            .px_4()
            .py_2()
            .bg(cx.theme().title_bar)
            .border_b_1()
            .border_color(cx.theme().title_bar_border)
            .child(Body::new(self.title).bold())
            .child(h_flex().gap_1().children(self.actions))
    }
}
