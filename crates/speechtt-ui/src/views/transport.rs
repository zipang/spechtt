//! Transport screens: ready, recording and paused.
//!
//! Recording and paused read live take telemetry from the controller:
//! elapsed seconds, the latest FFT frame and the streaming transcript.

use gpui_kit::assets::IconName;
use gpui_kit::base::h_flex;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, Context, div, rems};

use crate::app::SpeechTtApp;
use crate::components::{
    ActionButton, BadgeTone, CaptionWell, ElapsedTimer, IconAction, StatusBadge,
    TranscriptionModeBadge, TransportButton, TransportTone, Waveform,
};
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
        // Secondary affordance: transcribe media that already exists instead
        // of opening the microphone.
        .body(
            ActionButton::secondary("btn-prompt-transcribe-file", "TRANSCRIBE A FILE…").on_click(
                cx.listener(|this, _event, _window, cx| this.pick_transcription_file(cx)),
            ),
        )
        .into_any_element()
}

/// Take progress (files) or the elapsed timer (live), plus the model's mode.
///
/// A file take has no live clock: the worker reports the decoded position, so
/// the read-out is `processed / total s` once ffprobe knows the duration and
/// `processed s` otherwise.
fn header_trailing(app: &SpeechTtApp, elapsed: u32, cx: &mut Context<SpeechTtApp>) -> AnyElement {
    let mut row = h_flex().gap_2().items_center();
    row = match app.controller().progress() {
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
        None => row.child(ElapsedTimer::new(elapsed)),
    };
    if let Some(mode) = app.controller().session_model_mode() {
        row = row.child(TranscriptionModeBadge::new(mode));
    }
    row.into_any_element()
}

pub(crate) fn render_recording(app: &SpeechTtApp, cx: &mut Context<SpeechTtApp>) -> AnyElement {
    let controller = app.controller();
    let elapsed = controller.elapsed_seconds();
    let bars = controller.waveform_bars();
    let committed = controller.transcript_committed();
    let live = controller.transcript_live();

    ScreenShell::new(StatusBadge::tone("RECORDING", BadgeTone::Recording))
        .header_trailing(header_trailing(app, elapsed, cx))
        .gap(rems(0.25))
        .body_gap(rems(0.25))
        .body(Waveform::new(bars, true))
        .body(CaptionWell::new(committed, live))
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

pub(crate) fn render_paused(app: &SpeechTtApp, cx: &mut Context<SpeechTtApp>) -> AnyElement {
    let controller = app.controller();
    let elapsed = controller.elapsed_seconds();
    let bars = controller.waveform_bars();
    let committed = controller.transcript_committed();
    let live = controller.transcript_live();

    ScreenShell::new(StatusBadge::tone("PAUSED", BadgeTone::Paused))
        .header_trailing(header_trailing(app, elapsed, cx))
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
