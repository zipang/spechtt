//! `mm:ss` elapsed read-out, right-aligned in the transport header.

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::prelude::*;
use gpui_kit::{App, IntoElement, RenderOnce, Window, div, rems};

use crate::tokens::rem;

#[derive(IntoElement)]
pub struct ElapsedTimer {
    seconds: u32,
}

impl ElapsedTimer {
    pub fn new(seconds: u32) -> Self {
        Self { seconds }
    }
}

impl RenderOnce for ElapsedTimer {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let mono = cx.theme().mono_font_family.clone();
        div()
            .font_family(mono)
            .text_size(rems(rem::SM))
            .text_color(cx.theme().muted_foreground)
            .child(format!("{:02}:{:02}", self.seconds / 60, self.seconds % 60))
    }
}
