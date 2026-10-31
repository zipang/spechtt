//! `mm:ss` elapsed read-out, right-aligned in the transport header.

use gpui_kit::{App, IntoElement, RenderOnce, Window};

use crate::components::Label;

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
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        Label::new(format!("{:02}:{:02}", self.seconds / 60, self.seconds % 60))
            .mono()
            .muted()
    }
}
