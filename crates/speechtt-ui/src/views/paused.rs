//! Paused screen: the take is held, so Resume and the now-enabled Stop.

use gpui_kit::assets::IconName;
use gpui_kit::base::h_flex;
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, Context, rems};

use crate::app::SpeechTtApp;
use crate::components::{
    BadgeTone, CaptionWell, RoundButton, StatusBadge, StopControl, TransportHeader, Waveform,
};
use crate::layout::ScreenShell;

pub(crate) fn render(app: &SpeechTtApp, cx: &mut Context<SpeechTtApp>) -> AnyElement {
    let controller = app.controller();
    let elapsed = controller.elapsed_seconds();
    let bars = controller.waveform_bars();
    let committed = controller.transcript_committed();
    let live = controller.transcript_live();
    let progress = controller.progress();
    let mode = controller.session_model_mode();

    ScreenShell::new(StatusBadge::tone("PAUSED", BadgeTone::Paused))
        .header_trailing(TransportHeader::new(elapsed, progress, mode))
        .gap(rems(0.25))
        .body_gap(rems(0.25))
        .body(Waveform::new(bars, false))
        .body(CaptionWell::new(committed, live))
        .footer(
            h_flex()
                .items_center()
                .justify_center()
                .gap_6()
                .child(
                    RoundButton::new("btn-resume", IconName::Play)
                        .pulse(app.pulse_count("btn-resume"))
                        .on_click(
                            cx.listener(|this, _event, _window, cx| this.resume_recording(cx)),
                        ),
                )
                .child(
                    StopControl::new(true)
                        .on_click(cx.listener(|this, _event, _window, cx| this.finish_take(cx))),
                ),
        )
        .into_any_element()
}
