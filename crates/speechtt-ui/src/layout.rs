//! Composable box layout primitives shared by every widget screen.
//!
//! [`ScreenShell`] enforces the same three-band structure on all screens:
//! a status header, a flexible body and an optional footer of actions. That
//! keeps the status badge, content and transport controls on stable spines
//! while individual screens stay free to change their contents.
//!
//! [`SurfaceBox`] is the themed recessed card used by the caption well and
//! the "transcript written to" panel.

use gpui_kit::base::{h_flex, v_flex};
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, App, DefiniteLength, IntoElement, RenderOnce, Window, px, rems};

use crate::tokens::radius;

/// Three-band screen container: status header, flexible body, optional footer.
#[derive(IntoElement)]
pub struct ScreenShell {
    status: AnyElement,
    header_trailing: Option<AnyElement>,
    body: Vec<AnyElement>,
    footer: Option<AnyElement>,
    center_body: bool,
    gap: DefiniteLength,
    body_gap: DefiniteLength,
}

impl ScreenShell {
    /// Starts a shell whose leading header edge shows `status`.
    pub fn new(status: impl IntoElement) -> Self {
        Self {
            status: status.into_any_element(),
            header_trailing: None,
            body: Vec::new(),
            footer: None,
            center_body: false,
            gap: rems(0.25).into(),
            body_gap: rems(0.25).into(),
        }
    }

    /// Adds content to the trailing header edge (timer, "New take", …).
    pub fn header_trailing(mut self, trailing: impl IntoElement) -> Self {
        self.header_trailing = Some(trailing.into_any_element());
        self
    }

    /// Appends a child to the flexible body band.
    pub fn body(mut self, content: impl IntoElement) -> Self {
        self.body.push(content.into_any_element());
        self
    }

    /// Pins an action row at the bottom edge of the screen.
    pub fn footer(mut self, footer: impl IntoElement) -> Self {
        self.footer = Some(footer.into_any_element());
        self
    }

    /// Centers the body horizontally and vertically (prompt and ready screens).
    pub fn centered(mut self) -> Self {
        self.center_body = true;
        self
    }

    /// Gap between the three bands.
    pub fn gap(mut self, gap: impl Into<DefiniteLength>) -> Self {
        self.gap = gap.into();
        self
    }

    /// Gap between children of the body band.
    pub fn body_gap(mut self, gap: impl Into<DefiniteLength>) -> Self {
        self.body_gap = gap.into();
        self
    }
}

impl RenderOnce for ScreenShell {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let mut header = h_flex()
            .w_full()
            .flex_shrink_0()
            .justify_between()
            .items_center()
            .child(self.status);
        if let Some(trailing) = self.header_trailing {
            header = header.child(trailing);
        }

        let mut body = v_flex()
            .flex_1()
            .min_h_0()
            .w_full()
            .gap(self.body_gap)
            .children(self.body);
        if self.center_body {
            body = body.items_center().justify_center();
        }

        let mut shell = v_flex()
            .size_full()
            .min_h_0()
            .gap(self.gap)
            .child(header)
            .child(body);
        if let Some(footer) = self.footer {
            shell = shell.child(footer);
        }
        shell
    }
}

/// Themed recessed card for transcript and file-path panels.
#[derive(IntoElement)]
pub struct SurfaceBox {
    flex_grow: bool,
    clip: bool,
    children: Vec<AnyElement>,
}

impl Default for SurfaceBox {
    fn default() -> Self {
        Self::new()
    }
}

impl SurfaceBox {
    pub fn new() -> Self {
        Self {
            flex_grow: false,
            clip: false,
            children: Vec::new(),
        }
    }

    /// Grows into the remaining vertical space and may shrink below content.
    pub fn flex_1(mut self) -> Self {
        self.flex_grow = true;
        self
    }

    /// Clips overflowing content (transcript text, where the window decides
    /// how much of the take is visible).
    pub fn clip(mut self) -> Self {
        self.clip = true;
        self
    }

    pub fn child(mut self, content: impl IntoElement) -> Self {
        self.children.push(content.into_any_element());
        self
    }
}

impl RenderOnce for SurfaceBox {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let mut surface = v_flex()
            .w_full()
            .gap_1()
            .px_3p5()
            .py_2()
            .rounded(px(radius::MD))
            .bg(cx.theme().popover)
            .children(self.children);
        if self.flex_grow {
            surface = surface.flex_1().min_h_0();
        }
        if self.clip {
            surface = surface.overflow_y_hidden();
        }
        surface
    }
}
