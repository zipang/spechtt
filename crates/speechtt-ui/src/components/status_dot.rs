//! The small coloured dot that prefixes "connected" / "streaming" states.
//!
//! A six-pixel token-sized circle; the caller supplies the role colour.

use gpui_kit::prelude::*;
use gpui_kit::{App, Hsla, IntoElement, RenderOnce, Window, div, px};

use crate::tokens::space;

/// A dot used inside status rows and badges.
#[derive(IntoElement)]
pub struct StatusDot {
    tone: Hsla,
}

impl StatusDot {
    pub fn new(tone: Hsla) -> Self {
        Self { tone }
    }
}

impl RenderOnce for StatusDot {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        div().size(px(space::XS * 1.5)).rounded_full().bg(self.tone)
    }
}
