//! The rounded caption card: committed text in muted ink, the live words in
//! brand accent (the `audio capture layer` line in the mockups).
//!
//! Grows into the remaining screen height and clips overflow; the window
//! decides how much of a long take is visible.

use gpui_kit::{App, IntoElement, RenderOnce, SharedString, Window};

use crate::components::Label;
use crate::layout::SurfaceBox;
use crate::tokens::color;

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
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        SurfaceBox::new()
            .flex_1()
            .clip()
            .child(Label::new(self.committed).mono().muted())
            .child(
                Label::new(self.live)
                    .mono()
                    .tone(gpui_kit::rgb(color::BRAND_ACCENT).into()),
            )
    }
}
