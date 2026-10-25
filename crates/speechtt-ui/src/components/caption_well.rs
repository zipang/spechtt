//! The rounded caption card: committed text in muted ink, the live words in
//! brand accent (the `audio capture layer` line in the mockups).
//!
//! Grows into the remaining screen height and clips overflow; the window
//! decides how much of a long take is visible.

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::prelude::*;
use gpui_kit::{App, IntoElement, RenderOnce, SharedString, Window, div, rems};

use crate::layout::SurfaceBox;
use crate::tokens::{color, rem};

#[derive(IntoElement)]
pub struct CaptionWell {
    committed: SharedString,
    live: SharedString,
}

impl CaptionWell {
    pub fn new(committed: impl Into<SharedString>, live: impl Into<SharedString>) -> Self {
        Self {
            committed: committed.into(),
            live: live.into(),
        }
    }
}

impl RenderOnce for CaptionWell {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let mono = cx.theme().mono_font_family.clone();
        let text_size = rems(rem::SM);

        SurfaceBox::new()
            .flex_1()
            .clip()
            .child(
                div()
                    .font_family(mono.clone())
                    .text_size(text_size)
                    .text_color(cx.theme().muted_foreground)
                    .child(self.committed),
            )
            .child(
                div()
                    .font_family(mono)
                    .text_size(text_size)
                    .text_color(gpui_kit::rgb(color::BRAND_ACCENT))
                    .child(self.live),
            )
    }
}
