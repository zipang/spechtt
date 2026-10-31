//! Elapsed read-out for the top-right corner of a take screen.
//!
//! One thing only: how long the take has been running, as `mm:ss`. A live take
//! times its capture and a file take times its transcription; both reach this
//! component through the controller's elapsed clock.

use gpui_kit::base::h_flex;
use gpui_kit::prelude::*;
use gpui_kit::{App, IntoElement, RenderOnce, Window};

use crate::components::ElapsedTimer;

#[derive(IntoElement)]
pub struct TransportHeader {
    elapsed: u32,
}

impl TransportHeader {
    pub fn new(elapsed: u32) -> Self {
        Self { elapsed }
    }
}

impl RenderOnce for TransportHeader {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        h_flex()
            .gap_2()
            .items_center()
            .child(ElapsedTimer::new(self.elapsed))
    }
}
