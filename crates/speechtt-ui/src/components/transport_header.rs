//! Take read-out for the take screen's top-right corner.
//!
//! One thing only: when the take started and how long it has run. A live take
//! shows the elapsed `mm:ss` timer; a file take shows the decoded position the
//! worker reports, `processed / total s` once ffprobe knows the duration and
//! `processed s` otherwise.

use gpui_kit::base::h_flex;
use gpui_kit::prelude::*;
use gpui_kit::{App, IntoElement, RenderOnce, Window};

use crate::components::{ElapsedTimer, Label};

/// Decoded file position: `(processed, total)` seconds, `total` unknown until
/// the probe returns.
pub type Progress = (f32, Option<f32>);

#[derive(IntoElement)]
pub struct TransportHeader {
    elapsed: u32,
    progress: Option<Progress>,
}

impl TransportHeader {
    pub fn new(elapsed: u32, progress: Option<Progress>) -> Self {
        Self { elapsed, progress }
    }
}

impl RenderOnce for TransportHeader {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let row = h_flex().gap_2().items_center();
        match self.progress {
            Some((processed, total)) => {
                let label = match total {
                    Some(total) => format!("{processed:.0} / {total:.0} s"),
                    None => format!("{processed:.0} s"),
                };
                row.child(Label::new(label).mono())
            }
            None => row.child(ElapsedTimer::new(self.elapsed)),
        }
    }
}
