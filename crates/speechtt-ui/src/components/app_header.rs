//! The application header strip shared by the widget and dialog windows.
//!
//! One bold title on the left, optional actions (icon buttons) on the right,
//! drawn on the `title_bar` role with a bottom border. Window-specific chrome
//! such as a close button is passed in by the caller so every window keeps
//! the same Header + Body structure.
//!
//! The widget window adds its compiled version after the title through
//! [`AppHeader::version`]; `dialogs::window` renders the same strip
//! without it, because the Help footer already names the build.

use gpui_kit::base::h_flex;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, IntoElement, RenderOnce, SharedString, TestSupportExt as _, Window, div,
};

use crate::components::{Body, Label};

/// Top strip of an application window: title, optional version and trailing
/// actions.
#[derive(IntoElement)]
pub struct AppHeader {
    title: SharedString,
    version: Option<SharedString>,
    actions: Vec<AnyElement>,
}

impl AppHeader {
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            version: None,
            actions: Vec::new(),
        }
    }

    /// Shows the product version after the title, in the mono muted treatment.
    ///
    /// Off unless called: dialog windows keep a single title, because the Help
    /// footer already carries the version and repeating it there would say it
    /// twice in one window. The widget window passes
    /// `concat!("V", env!("CARGO_PKG_VERSION"))`.
    pub fn version(mut self, version: impl Into<SharedString>) -> Self {
        self.version = Some(version.into());
        self
    }

    /// Adds a trailing action, rendered left to right in insertion order.
    pub fn action(mut self, action: impl IntoElement) -> Self {
        self.actions.push(action.into_any_element());
        self
    }
}

impl RenderOnce for AppHeader {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let mut heading = h_flex()
            .items_center()
            .gap_1()
            .child(Body::new(self.title).bold());

        if let Some(version) = self.version {
            heading = heading.child(
                div()
                    .id("app-header-version")
                    .test_support()
                    .aria_label(version.clone())
                    .child(Label::new(version).mono().muted()),
            );
        }

        h_flex()
            .justify_between()
            .items_center()
            .px_4()
            .py_2()
            .bg(cx.theme().title_bar)
            .border_b_1()
            .border_color(cx.theme().title_bar_border)
            .child(heading)
            .child(h_flex().gap_1().children(self.actions))
    }
}
