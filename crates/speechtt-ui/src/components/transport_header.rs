//! Take progress read-out for the recording/paused header.
//!
//! Files have no live clock: the worker reports the decoded position, so the
//! read-out is `processed / total s` once ffprobe knows the duration and
//! `processed s` otherwise. Live takes show the elapsed `mm:ss` timer. The
//! transcription-mode badge rides along whenever the take has a model.

use gpui_kit::base::h_flex;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::prelude::*;
use gpui_kit::{App, IntoElement, RenderOnce, Window, div, rems};

use speechtt_core::protocol::TranscriptionMode;

use crate::components::{ElapsedTimer, TranscriptionModeBadge};
use crate::tokens::rem;

/// Decoded file position: `(processed, total)` seconds, `total` unknown until
/// the probe returns.
pub type Progress = (f32, Option<f32>);

#[derive(IntoElement)]
pub struct TransportHeader {
    elapsed: u32,
    progress: Option<Progress>,
    mode: Option<TranscriptionMode>,
}

impl TransportHeader {
    pub fn new(elapsed: u32, progress: Option<Progress>, mode: Option<TranscriptionMode>) -> Self {
        Self {
            elapsed,
            progress,
            mode,
        }
    }
}

impl RenderOnce for TransportHeader {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let mut row = h_flex().gap_2().items_center();
        row = match self.progress {
            Some((processed, total)) => {
                let label = match total {
                    Some(total) => format!("{processed:.0} / {total:.0} s"),
                    None => format!("{processed:.0} s"),
                };
                row.child(
                    div()
                        .font_family(cx.theme().mono_font_family.clone())
                        .text_size(rems(rem::SM))
                        .child(label),
                )
            }
            None => row.child(ElapsedTimer::new(self.elapsed)),
        };
        if let Some(mode) = self.mode {
            row = row.child(TranscriptionModeBadge::new(mode));
        }
        row
    }
}
