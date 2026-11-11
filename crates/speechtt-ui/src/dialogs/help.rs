//! Help dialog: the window-scoped commands and the discovered model catalogue.
//!
//! Two tabs share the dialog: a shortcut reference and a compact list of the
//! selected provider's models with the streaming/batch badge, description and
//! price. The reference is drawn from [`crate::shortcuts::BINDINGS`] — the same
//! table that registers the bindings — so the tab cannot advertise a key that
//! does nothing. Model metadata comes from the capability handshake; nothing
//! here performs I/O.
//!
//! Rendered by `dialogs::window` for the `Dialog::Help` window.

use gpui_kit::base::{h_flex, v_flex};
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, App, Context, IntoElement, TestSupportExt as _, div};

use speechtt_core::actions::SessionAction;
use speechtt_core::protocol::{CapabilitySource, ModelCapabilityInfo};

use crate::components::{
    ActionButton, AppFooter, Body, BuildStatus, KeyCap, Label, TranscriptionModeBadge,
};
use crate::controller::{HelpTab, pricing_lines};
use crate::dialogs::{DialogShell, DialogWindow};
use crate::shortcuts::{self, Binding};

pub(crate) fn render(dialog: &DialogWindow, cx: &mut Context<DialogWindow>) -> AnyElement {
    let (tab, provider_label, source, model_rows, connection) = {
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
            app.gateway_state(),
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

    let footer = AppFooter::new()
        .leading(BuildStatus::new(connection))
        .action(
            ActionButton::primary("btn-help-got-it", "Got it")
                .on_click(cx.listener(|this, _event, window, cx| this.close(window, cx))),
        );

    DialogShell::new()
        .body(tabs)
        .body(content)
        .footer(footer)
        .into_any_element()
}

/// The Shortcuts tab: one row per binding, in [`shortcuts::BINDINGS`] order.
///
/// Each row carries the element id `help-shortcut-<action>`, which is stable
/// per command and lets the headless tests prove the tab draws the whole table.
fn shortcuts(cx: &App) -> AnyElement {
    let is_mac = shortcuts::is_mac(cx);
    let mut rows = v_flex().w_full().gap_0();
    for binding in &shortcuts::BINDINGS {
        rows = rows.child(shortcut_row(binding, is_mac, cx));
    }
    rows.into_any_element()
}

/// One shortcut row: the command's description, then its keycaps.
fn shortcut_row(binding: &Binding, is_mac: bool, cx: &App) -> AnyElement {
    h_flex()
        .id(shortcut_row_id(binding.action))
        .test_support()
        .w_full()
        .justify_between()
        .items_center()
        .gap_3()
        .py_2p5()
        .border_b_1()
        .border_color(cx.theme().border)
        .child(Body::new(binding.description))
        .child(KeyCap::new(shortcuts::key_caps(binding, is_mac)))
        .into_any_element()
}

/// The element id of a command's Help row.
fn shortcut_row_id(action: SessionAction) -> String {
    format!("help-shortcut-{}", action.as_str())
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
