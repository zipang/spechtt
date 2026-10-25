//! Transport screens: ready, recording and paused.

use gpui_kit::assets::IconName;
use gpui_kit::base::h_flex;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, Context, div, rems};

use crate::app::SpeechTtApp;
use crate::components::{
    BadgeTone, CaptionWell, ElapsedTimer, IconAction, StatusBadge, TransportButton, TransportTone,
    Waveform,
};
use crate::controller::{MOCK_BARS, MOCK_COMMITTED, MOCK_ELAPSED_SECONDS, MOCK_LIVE};
use crate::layout::ScreenShell;
use crate::tokens::rem;

/// Ready state: the large mic affordance.
pub(crate) fn render_ready(cx: &mut Context<SpeechTtApp>) -> AnyElement {
    ScreenShell::new(StatusBadge::tone("READY", BadgeTone::Ready))
        .centered()
        .gap(rems(0.5))
        .body_gap(rems(1.0))
        .body(
            TransportButton::new("btn-record", IconName::Mic)
                .on_click(cx.listener(|this, _event, _window, cx| this.start_recording(cx))),
        )
        .body(
            div()
                .text_size(rems(rem::SM))
                .text_color(cx.theme().muted_foreground)
                .child("Press to start dictating"),
        )
        .into_any_element()
}

pub(crate) fn render_recording(cx: &mut Context<SpeechTtApp>) -> AnyElement {
    ScreenShell::new(StatusBadge::tone("RECORDING", BadgeTone::Recording))
        .header_trailing(ElapsedTimer::new(MOCK_ELAPSED_SECONDS))
        .gap(rems(0.25))
        .body_gap(rems(0.25))
        .body(Waveform::new(MOCK_BARS, true))
        .body(CaptionWell::new(MOCK_COMMITTED, MOCK_LIVE))
        // Same two controls as Paused (layout stays put): Pause toggles the
        // take, Stop stays disabled until the take is paused.
        .footer(
            h_flex()
                .items_center()
                .justify_center()
                .gap_6()
                .child(
                    TransportButton::new("btn-pause", IconName::Pause).on_click(
                        cx.listener(|this, _event, _window, cx| this.pause_recording(cx)),
                    ),
                )
                .child(stop_control(false, cx)),
        )
        .into_any_element()
}

pub(crate) fn render_paused(cx: &mut Context<SpeechTtApp>) -> AnyElement {
    ScreenShell::new(StatusBadge::tone("PAUSED", BadgeTone::Paused))
        .header_trailing(ElapsedTimer::new(MOCK_ELAPSED_SECONDS))
        .gap(rems(0.25))
        .body_gap(rems(0.25))
        .body(Waveform::new(MOCK_BARS, false))
        .body(CaptionWell::new(MOCK_COMMITTED, MOCK_LIVE))
        .footer(
            h_flex()
                .items_center()
                .justify_center()
                .gap_6()
                .child(
                    TransportButton::new("btn-resume", IconName::Play)
                        .tone(TransportTone::Warning)
                        .on_click(
                            cx.listener(|this, _event, _window, cx| this.resume_recording(cx)),
                        ),
                )
                .child(stop_control(true, cx)),
        )
        .into_any_element()
}

/// Square Stop control, shown in both transport states so the layout does not
/// shift. Disabled (inert, dimmed) while recording.
fn stop_control(enabled: bool, cx: &mut Context<SpeechTtApp>) -> AnyElement {
    IconAction::new("btn-stop", IconName::Square)
        .disabled(!enabled)
        .on_click(cx.listener(|this, _event, _window, cx| this.finish_take(cx)))
        .into_any_element()
}
