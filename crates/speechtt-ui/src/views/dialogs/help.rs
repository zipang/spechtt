//! Help / Shortcuts dialog, per `mockups/dialogs/help.png`.

use gpui_kit::base::{h_flex, v_flex};
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, Context, FontWeight, IntoElement, div, px, rems};

use crate::components::{ActionButton, AppFooter, KeyCap};
use crate::tokens::{color, rem};
use crate::views::dialogs::{DialogShell, DialogWindow};

/// One row of the shortcuts reference. T0006 will drive this from the user's
/// bindings; keeping it in one table avoids prose drifting from behaviour.
pub(crate) const HELP_SHORTCUTS: &[(&str, &[&str])] = &[
    ("Record · pause · resume", &["Ctrl", "Shift", "R"]),
    ("Stop recording", &["Ctrl", "Shift", "S"]),
    ("Copy transcript to clipboard", &["Ctrl", "Shift", "C"]),
    ("Paste transcript as keystrokes", &["Ctrl", "Shift", "V"]),
    ("Save transcript to file", &["Ctrl", "Shift", "F"]),
    ("Open settings", &["Ctrl", "Shift", ","]),
];

pub(crate) fn render(cx: &mut Context<DialogWindow>) -> AnyElement {
    let mut rows = v_flex().w_full().gap_0();
    for (label, keys) in HELP_SHORTCUTS {
        rows = rows.child(
            h_flex()
                .w_full()
                .justify_between()
                .items_center()
                .gap_3()
                .py_2p5()
                .border_b_1()
                .border_color(cx.theme().border)
                .child(
                    div()
                        .flex_shrink_1()
                        .min_w_0()
                        .text_size(rems(rem::MD))
                        .child(*label),
                )
                .child(KeyCap::new(keys.iter().copied())),
        );
    }

    let status = h_flex()
        .gap_2()
        .items_center()
        .child(
            div()
                .font_family(cx.theme().mono_font_family.clone())
                .text_size(rems(rem::XS))
                .font_weight(FontWeight::MEDIUM)
                .text_color(cx.theme().muted_foreground)
                .child(concat!("SPEECHTT V", env!("CARGO_PKG_VERSION"))),
        )
        .child(
            h_flex()
                .gap_1p5()
                .items_center()
                .child(
                    div()
                        .size(px(5.))
                        .rounded(px(2.5))
                        .bg(gpui_kit::rgb(color::SUCCESS)),
                )
                .child(
                    div()
                        .text_size(rems(rem::XS))
                        .font_weight(FontWeight::BOLD)
                        .text_color(gpui_kit::rgb(color::SUCCESS))
                        .child("GATEWAY CONNECTED"),
                ),
        );

    let footer = AppFooter::new().leading(status).action(
        ActionButton::primary("btn-help-got-it", "Got it")
            .on_click(cx.listener(|this, _event, window, cx| this.close(window, cx))),
    );

    DialogShell::new()
        .body(
            div()
                .text_size(rems(rem::XS))
                .font_weight(FontWeight::BOLD)
                .text_color(cx.theme().muted_foreground)
                .child("KEYBOARD SHORTCUTS"),
        )
        .body(rows)
        .footer(footer)
        .into_any_element()
}
