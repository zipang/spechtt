//! Live take screen: one screen for idle, recording and paused.
//!
//! The status badge and the waveform carry the take state; the shared footer
//! always holds the actions, with back on the left, the take control centred
//! and Stop on the right (T0021). T0022 replaces the take control with the
//! microphone on/off control and adds Reset; until then the screen keeps the
//! previous transport glyphs so no behaviour is lost.

use gpui_kit::assets::IconName;
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, Context, MouseDownEvent};
use speechtt_core::session::SessionState;

use crate::app::SpeechTtApp;
use crate::components::{
    BadgeTone, CaptionWell, RoundButton, ScreenFooter, StatusBadge, StopControl, TransportHeader,
    Waveform,
};
use crate::layout::ScreenShell;
use crate::tokens::space;

/// What the centred take control does in the current state.
#[derive(Debug, Clone, Copy)]
enum TakeIntent {
    Start,
    Pause,
    Resume,
}

pub(crate) fn render(app: &SpeechTtApp, cx: &mut Context<SpeechTtApp>) -> AnyElement {
    let controller = app.controller();
    let state = app.session_state();
    let paused = state == SessionState::Paused;
    let active = state.is_active();
    let elapsed = controller.elapsed_seconds();
    let bars = controller.waveform_bars();
    let committed = controller.transcript_committed();
    let live = controller.transcript_live();
    let progress = controller.progress();
    let mode = controller.session_model_mode();

    let badge = match state {
        SessionState::Recording => StatusBadge::tone("RECORDING", BadgeTone::Recording),
        SessionState::Paused => StatusBadge::tone("PAUSED", BadgeTone::Paused),
        _ => StatusBadge::tone("READY", BadgeTone::Ready),
    };

    let (id, icon, intent) = if paused {
        ("btn-resume", IconName::Play, TakeIntent::Resume)
    } else if active {
        ("btn-pause", IconName::Pause, TakeIntent::Pause)
    } else {
        ("btn-take", IconName::Mic, TakeIntent::Start)
    };

    let control = RoundButton::new(id, icon)
        .on_press(
            cx.listener(move |this, _event: &MouseDownEvent, _window, cx| {
                this.ripple(id, cx);
            }),
        )
        .on_click(cx.listener(move |this, _event, _window, cx| match intent {
            TakeIntent::Start => this.start_recording(cx),
            TakeIntent::Pause => this.pause_recording(cx),
            TakeIntent::Resume => this.resume_recording(cx),
        }));

    let mut shell = ScreenShell::new(badge)
        .gap(space::XS)
        .body_gap(space::XS)
        .body(Waveform::new(bars, active && !paused))
        .body(CaptionWell::new(committed, live));

    if active {
        shell = shell.header_trailing(TransportHeader::new(elapsed, progress, mode));
    }

    let mut footer = ScreenFooter::new()
        .back(
            "btn-back",
            "Back",
            cx.listener(|this, _event, _window, cx| {
                this.go_back(cx);
            }),
        )
        .primary(control);
    if active {
        footer = footer.trailing(
            StopControl::new(paused)
                .on_click(cx.listener(|this, _event, _window, cx| this.finish_take(cx))),
        );
    }

    shell.footer(footer).into_any_element()
}
