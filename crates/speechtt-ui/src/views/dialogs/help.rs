//! Help dialog: keyboard shortcuts and the discovered model catalogue.
//!
//! Two tabs share the dialog: the historical shortcut reference and a compact
//! list of the selected provider's models with the streaming/batch badge,
//! description and price. Model metadata comes from the capability handshake;
//! nothing here performs I/O.

use gpui_kit::base::{h_flex, v_flex};
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, App, Context, IntoElement, div};

use speechtt_core::protocol::{CapabilitySource, ModelCapabilityInfo};

use crate::components::{
    ActionButton, AppFooter, Body, KeyCap, Label, StatusDot, TranscriptionModeBadge,
};
use crate::controller::{HelpTab, pricing_lines};
use crate::tokens::color;
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

pub(crate) fn render(dialog: &DialogWindow, cx: &mut Context<DialogWindow>) -> AnyElement {
    let (tab, provider_label, source, model_rows) = {
        let app = dialog.app().read(cx);
        let controller = app.controller();
        (
            controller.help_tab(),
            controller
                .help_provider_label()
                .unwrap_or("No provider catalogue")
                .to_owned(),
            controller.catalogue_source(),
            controller.help_models().to_vec(),
        )
    };

    let mono = cx.theme().mono_font_family.clone();
    let tabs = TabBar::new("help-tabs")
        .underline()
        .selected_index(tab.index())
        .on_click(cx.listener(|this, index: &usize, _window, cx| {
            this.select_help_tab(HelpTab::from_index(*index), cx);
        }))
        .child(Tab::from("SHORTCUTS").font_family(mono.clone()))
        .child(Tab::from("MODELS").font_family(mono));

    let content = match tab {
        HelpTab::Shortcuts => shortcuts(cx),
        HelpTab::Models => models(cx, &provider_label, source, &model_rows),
    };

    let status = h_flex()
        .gap_2()
        .items_center()
        .child(
            Label::new(concat!("SPEECHTT V", env!("CARGO_PKG_VERSION")))
                .mono()
                .muted(),
        )
        .child(
            h_flex()
                .gap_1p5()
                .items_center()
                .child(StatusDot::new(gpui_kit::rgb(color::SUCCESS).into()))
                .child(
                    Label::new("GATEWAY CONNECTED")
                        .bold()
                        .tone(gpui_kit::rgb(color::SUCCESS).into()),
                ),
        );

    let footer = AppFooter::new().leading(status).action(
        ActionButton::primary("btn-help-got-it", "Got it")
            .on_click(cx.listener(|this, _event, window, cx| this.close(window, cx))),
    );

    DialogShell::new()
        .body(tabs)
        .body(content)
        .footer(footer)
        .into_any_element()
}

fn shortcuts(cx: &App) -> AnyElement {
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
                .child(Body::new(*label))
                .child(KeyCap::new(keys.iter().copied())),
        );
    }
    rows.into_any_element()
}

fn models(
    cx: &App,
    provider_label: &str,
    source: Option<CapabilitySource>,
    models: &[ModelCapabilityInfo],
) -> AnyElement {
    let source_label = match source {
        Some(CapabilitySource::Discovered) => "LIVE CATALOGUE",
        Some(CapabilitySource::Builtin) => "BUILT-IN CATALOGUE",
        None => "CATALOGUE NOT LOADED",
    };

    let header = h_flex()
        .w_full()
        .justify_between()
        .items_center()
        .gap_3()
        .child(Body::new(format!("{provider_label} models")).bold())
        .child(Label::new(source_label).bold().muted());

    let body = if models.is_empty() {
        div()
            .py_4()
            .child(
                Label::new("No model metadata yet. Open Settings to refresh the catalogue.")
                    .muted(),
            )
            .into_any_element()
    } else {
        v_flex()
            .id("help-model-list")
            .w_full()
            .gap_0()
            .children(models.iter().map(|model| model_row(model, cx)))
            .into_any_element()
    };

    v_flex()
        .flex_1()
        .min_h_0()
        .w_full()
        .child(
            div()
                .id("help-models")
                .flex_1()
                .min_h_0()
                .w_full()
                .overflow_y_scrollbar()
                .child(v_flex().w_full().gap_2().child(header).child(body)),
        )
        .into_any_element()
}

fn model_row(model: &ModelCapabilityInfo, cx: &App) -> AnyElement {
    let mut details = v_flex()
        .flex_1()
        .min_w_0()
        .gap_0p5()
        .child(
            h_flex()
                .gap_2()
                .items_center()
                .child(TranscriptionModeBadge::new(model.mode))
                .child(Body::new(model.label.clone()).bold()),
        )
        .child(Label::new(model.id.clone()).mono().muted());

    if let Some(description) = &model.description {
        details = details.child(Body::new(description.clone()).muted());
    }

    for line in pricing_lines(&model.pricing) {
        details = details.child(Label::new(line).muted());
    }

    h_flex()
        .w_full()
        .py_2p5()
        .border_b_1()
        .border_color(cx.theme().border)
        .child(details)
        .into_any_element()
}
