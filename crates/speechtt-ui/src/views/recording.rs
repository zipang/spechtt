//! Live take screen: one screen for idle, recording and paused.
//!
//! The screen speaks the language of *capture*, not of playback: there is one
//! microphone on/off control, and the take state is carried by the status badge
//! and the waveform — never by the button glyph alone.
//!
//! ```text
//!        press the mic                    press it again
//! idle ─────────────────▶ recording ⇄────────────────▶ paused
//!                            │  └───────────────────────┘
//!                            └── Stop ──▶ Stopped
//! ```
//!
//! The footer uses the shared three zones: Cancel (back to the mode chooser) on
//! the left, the mic control centred, Reset and Stop on the right. The screen is
//! a thin composition over [`ScreenShell`] and the shared components — every
//! colour, size and spacing decision lives in [`crate::components`].

use gpui_kit::assets::IconName;
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, Context, MouseDownEvent};
use speechtt_core::session::SessionState;

use crate::app::SpeechTtApp;
use crate::components::{
    BadgeTone, CaptionWell, IconAction, ScreenFooter, StatusBadge, ToggleRoundButton, ToggleState,
    TransportHeader, Waveform,
};
use crate::layout::ScreenShell;
use crate::tokens::space;

pub(crate) fn render(app: &SpeechTtApp, cx: &mut Context<SpeechTtApp>) -> AnyElement {
    let controller = app.controller();
    let state = app.session_state();
    let recording = state == SessionState::Recording;
    let active = state.is_active();
    // A file take is decoded by the worker: there is no microphone to switch.
    let file_take = controller.is_file_take();
    let elapsed = controller.elapsed_seconds();
    let bars = controller.waveform_bars();
    let committed = controller.transcript_committed();
    let live = controller.transcript_live();
    let progress = controller.progress();

    let badge = match state {
        SessionState::Recording => StatusBadge::tone("RECORDING", BadgeTone::Recording),
        SessionState::Paused => StatusBadge::tone("PAUSED", BadgeTone::Paused),
        _ => StatusBadge::tone("READY", BadgeTone::Ready),
    };

    // One control, one glyph: it is always the microphone, and it is a switch.
    // Pressing it while capturing pauses; pressing it while paused (or idle)
    // starts.
    let record = ToggleRoundButton::new(
        "btn-record",
        IconName::Mic,
        ToggleState::from_active(recording),
    )
    .disabled(file_take)
    .on_press(cx.listener(|this, _event: &MouseDownEvent, _window, cx| {
        this.ripple("btn-record", cx);
    }))
    .on_click(cx.listener(|this, _event, _window, cx| {
        if this.session_state() == SessionState::Recording {
            this.pause_recording(cx);
        } else if this.session_state() == SessionState::Paused {
            this.resume_recording(cx);
        } else {
            this.start_recording(cx);
        }
    }));

    let mut shell = ScreenShell::new(badge)
        .gap(space::XS)
        .body_gap(space::XS)
        .body(Waveform::new(bars, recording))
        .body(CaptionWell::new(committed, live));

    if active {
        shell = shell.header_trailing(TransportHeader::new(elapsed, progress));
    }

    let mut footer = ScreenFooter::new()
        // Cancel is always live, including mid-take: it discards the take and
        // goes home.
        .back(
            "btn-cancel-take",
            "Cancel",
            cx.listener(|this, _event, _window, cx| {
                this.cancel_take(cx);
            }),
        )
        .primary(record);

    // Reset clears the take and stays here; it is refused in idle and for a
    // file take, where there is no local take to clear.
    footer = footer.trailing(
        IconAction::new("btn-reset", IconName::RotateCcw)
            .label("Reset")
            .disabled(!active || file_take)
            .on_click(cx.listener(|this, _event, _window, cx| {
                this.reset_take(cx);
            })),
    );
    // The last slot is a forward arrow, the mirror of the footer's back arrow:
    // stopping the take is going *forward* — to the transcript — not a
    // destructive dead end. It is live whenever a take is open, no pause first,
    // and identical whether recording or paused.
    footer = footer.trailing(
        IconAction::new("btn-stop", IconName::ArrowBigRight)
            .label("Stop")
            .disabled(!active)
            .on_click(cx.listener(|this, _event, _window, cx| {
                this.finish_take(cx);
            })),
    );

    shell.footer(footer).into_any_element()
}
