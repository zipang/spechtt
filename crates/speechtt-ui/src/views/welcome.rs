//! Welcome screen: the mode chooser.
//!
//! Two equal cards, pinned to [`MODE_CARD_WIDTH`] and centered as a pair, so
//! the free space grows around them instead of inside them. The live card goes
//! to the Ready screen; the file card opens the picker. Whichever mode the user
//! picks, the take itself is only a provider and a model — no name, no output
//! target.

use gpui_kit::base::{h_flex, v_flex};
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, Context, FontWeight, div, px, rems};

use crate::app::SpeechTtApp;
use crate::components::{Card, RoundButton};
use crate::controller::SessionMode;
use crate::layout::ScreenShell;
use crate::tokens::{MODE_CARD_WIDTH, rem};

pub(crate) fn render(app: &SpeechTtApp, cx: &mut Context<SpeechTtApp>) -> AnyElement {
    let live_pulse = app.pulse_count("btn-mode-live");
    let file_pulse = app.pulse_count("btn-mode-file");
    let muted = cx.theme().muted_foreground;

    let live_card = Card::new("mode-live")
        .width(px(MODE_CARD_WIDTH))
        .on_click(cx.listener(|this, _event, _window, cx| this.choose_mode(SessionMode::Live, cx)))
        .child(card_title("Live dictation"))
        .child(card_description(
            "Record and transcribe from your microphone in real time",
            muted,
        ))
        .child(div().flex_1().flex().items_center().justify_center().child(
            RoundButton::new("btn-mode-live", gpui_kit::assets::IconName::Mic).pulse(live_pulse),
        ))
        .child(card_caption("Start"));

    let file_card = Card::new("mode-file")
        .width(px(MODE_CARD_WIDTH))
        .on_click(cx.listener(|this, _event, _window, cx| this.choose_mode(SessionMode::File, cx)))
        .child(card_title("From a file"))
        .child(card_description(
            "Import an audio or video recording",
            muted,
        ))
        .child(
            div().flex_1().flex().items_center().justify_center().child(
                RoundButton::new("btn-mode-file", gpui_kit::assets::IconName::FileUp)
                    .tone(crate::components::RoundTone::Secondary)
                    .pulse(file_pulse),
            ),
        )
        .child(card_caption("Browse files"));

    ScreenShell::headerless()
        .body_gap(rems(0.75))
        .body(
            v_flex()
                .w_full()
                .gap_1()
                .items_center()
                .child(
                    div()
                        .text_size(rems(rem::LG))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("Choose a mode"),
                )
                .child(
                    div()
                        .text_size(rems(rem::SM))
                        .text_color(muted)
                        .child("Select a transcription method"),
                ),
        )
        .body(
            h_flex()
                .w_full()
                .flex_1()
                .min_h_0()
                .items_center()
                .justify_center()
                .child(
                    h_flex()
                        .items_stretch()
                        .gap(rems(1.0))
                        .child(live_card)
                        .child(file_card),
                ),
        )
        .into_any_element()
}

fn card_title(text: &'static str) -> AnyElement {
    div()
        .w_full()
        .text_center()
        .text_size(rems(rem::MD))
        .font_weight(FontWeight::SEMIBOLD)
        .child(text)
        .into_any_element()
}

fn card_description(text: &'static str, muted: gpui_kit::Hsla) -> AnyElement {
    div()
        .w_full()
        .text_center()
        .text_size(rems(rem::SM))
        .text_color(muted)
        .child(text)
        .into_any_element()
}

fn card_caption(text: &'static str) -> AnyElement {
    div()
        .w_full()
        .text_center()
        .text_size(rems(rem::XS))
        .font_weight(FontWeight::MEDIUM)
        .child(text)
        .into_any_element()
}
