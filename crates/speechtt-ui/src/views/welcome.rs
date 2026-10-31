//! Welcome screen: the mode chooser.
//!
//! One left-aligned spine: the screen title and its description sit on the same
//! left edge as the two [`MODE_CARD_WIDTH`] cards, and the free space grows on
//! the right instead of around them. The live card opens the take screen
//! directly; the file card opens the picker. Whichever mode the user picks, the
//! take itself is only a provider and a model — no name, no output target.

use gpui_kit::base::{h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, Context};

use crate::app::SpeechTtApp;
use crate::components::{Body, Card, Label, RoundButton, Title};
use crate::controller::SessionMode;
use crate::layout::ScreenShell;
use crate::tokens::{MODE_CARD_WIDTH, space};

pub(crate) fn render(_app: &SpeechTtApp, cx: &mut Context<SpeechTtApp>) -> AnyElement {
    let live_card = Card::new("mode-live")
        .width(MODE_CARD_WIDTH)
        .on_press(
            cx.listener(|this, _event: &gpui_kit::MouseDownEvent, _window, cx| {
                this.ripple("btn-mode-live", cx);
            }),
        )
        .on_click(cx.listener(|this, _event, _window, cx| this.choose_mode(SessionMode::Live, cx)))
        .child(card_copy(
            "Live dictation",
            "Record and transcribe from your microphone in real time",
        ))
        .child(card_action(
            RoundButton::new("btn-mode-live", gpui_kit::assets::IconName::Mic),
            "Start",
        ));

    let file_card = Card::new("mode-file")
        .width(MODE_CARD_WIDTH)
        .on_press(
            cx.listener(|this, _event: &gpui_kit::MouseDownEvent, _window, cx| {
                this.ripple("btn-mode-file", cx);
            }),
        )
        .on_click(cx.listener(|this, _event, _window, cx| this.choose_mode(SessionMode::File, cx)))
        .child(card_copy(
            "From a file",
            "Import an audio or video recording",
        ))
        .child(card_action(
            RoundButton::new("btn-mode-file", gpui_kit::assets::IconName::FileUp)
                .tone(crate::components::RoundTone::Secondary),
            "Browse files",
        ));

    ScreenShell::headerless()
        .body_gap(space::MD)
        .body(
            v_flex()
                .w_full()
                .gap_1()
                .items_start()
                .child(Title::new("Choose a mode"))
                .child(Body::new("Select a transcription method").muted()),
        )
        .body(
            h_flex()
                .w_full()
                .flex_1()
                .min_h_0()
                .items_center()
                .justify_start()
                .child(
                    h_flex()
                        .items_stretch()
                        .gap_4()
                        .child(live_card)
                        .child(file_card),
                ),
        )
        .into_any_element()
}

/// Title then description, centered.
fn card_copy(title: &'static str, description: &'static str) -> AnyElement {
    v_flex()
        .w_full()
        .gap_2()
        .child(Title::new(title).center())
        .child(Body::new(description).muted().center())
        .into_any_element()
}

/// The round button plus its caption, in one block that sits at the card
/// bottom so both cards share a button baseline however their copy wraps.
fn card_action(button: RoundButton, caption: &'static str) -> AnyElement {
    v_flex()
        .flex_1()
        .w_full()
        .justify_end()
        .items_center()
        .gap_2()
        .child(button)
        .child(Label::new(caption))
        .into_any_element()
}
