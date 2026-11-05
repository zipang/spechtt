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
//!                            └── Stop ──▶ session end
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
    BadgeTone, CaptionWell, IconAction, LiveModelSelect, ScreenFooter, StatusBadge,
    ToggleRoundButton, ToggleState, TransportHeader, TransportModel, Waveform,
};
use crate::layout::ScreenShell;
use crate::tokens::space;

pub(crate) fn render(app: &SpeechTtApp, cx: &mut Context<SpeechTtApp>) -> AnyElement {
    let controller = app.controller();
    let state = app.session_state();
    let recording = state == SessionState::Recording;
    let active = state.is_active();
    let elapsed = controller.elapsed_seconds();
    let bars = controller.waveform_bars();
    let committed = controller.transcript_committed();
    let live = controller.transcript_live();
    let has_models = !controller.live_model_options().is_empty();

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
        .body(CaptionWell::new(committed, live).follow(app.caption_scroll()));

    // The header is always attached: `00:00` before the first word is what makes
    // the later `00:07` legible, and the model beside it names the engine that
    // will listen.
    let model = if active {
        // The session configuration is frozen from the first recorded frame, so
        // the picker would be a control that cannot act.
        match app.live_model_label() {
            Some(label) => TransportModel::Label(label.into()),
            None => TransportModel::Absent,
        }
    } else if has_models {
        TransportModel::Picker(
            LiveModelSelect::new("field-live-model", app.live_model_field().state())
                .placeholder("Choose model"),
        )
    } else {
        match app.live_model_label() {
            Some(label) => TransportModel::Label(label.into()),
            None => TransportModel::Absent,
        }
    };
    shell = shell.header_trailing(TransportHeader::new(elapsed, model));

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

    // Reset clears the take and stays here; it is inert in idle.
    footer = footer.trailing(
        IconAction::new("btn-reset", IconName::RotateCcw)
            .label("Reset")
            .disabled(!active)
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
