//! A labelled setting row: a title, a fainter description and a control slot
//! on the trailing edge. Shared by the setup form and the Settings dialog.
//!
//! The default (compact) form uses an uppercase micro-label so several rows
//! fit inside the widget. [`SettingRow::prominent`] uses the roomier dialog
//! style: a larger title, muted helper text and a hairline separator between
//! rows, as used by the provider/model setup form and the Settings dialog.

use gpui_kit::base::{h_flex, v_flex};
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, FontWeight, IntoElement, RenderOnce, SharedString, Window, div, rems,
};

use crate::tokens::rem;

#[derive(IntoElement)]
pub struct SettingRow {
    label: SharedString,
    description: SharedString,
    control: Option<AnyElement>,
    prominent: bool,
}

impl SettingRow {
    pub fn new(label: impl Into<SharedString>, description: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            description: description.into(),
            control: None,
            prominent: false,
        }
    }

    /// Uses the dialog style: large light title, muted description and a
    /// bottom hairline separating rows.
    pub fn prominent(mut self) -> Self {
        self.prominent = true;
        self
    }

    /// Supplies the control rendered at the trailing edge of the row.
    pub fn control(mut self, control: impl IntoElement) -> Self {
        self.control = Some(control.into_any_element());
        self
    }
}

impl RenderOnce for SettingRow {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let mut slot = h_flex()
            .flex_1()
            .min_w(rems(8.0))
            .max_w(rems(18.0))
            .justify_end();
        if let Some(control) = self.control {
            slot = slot.child(control);
        }

        let title = if self.prominent {
            div()
                .text_size(rems(rem::MD))
                .font_weight(FontWeight::MEDIUM)
                .child(self.label)
        } else {
            div()
                .text_size(rems(rem::XS))
                .font_weight(FontWeight::BOLD)
                .text_color(cx.theme().muted_foreground)
                .child(self.label)
        };
        let description = if self.prominent {
            div()
                .text_size(rems(rem::MD))
                .text_color(cx.theme().muted_foreground)
                .child(self.description)
        } else {
            div().text_size(rems(rem::SM)).child(self.description)
        };

        let row = h_flex()
            .w_full()
            .justify_between()
            .items_center()
            .gap_4()
            .child(
                v_flex()
                    .flex_shrink_1()
                    .min_w_0()
                    .gap_0p5()
                    .child(title)
                    .child(description),
            )
            .child(slot);

        if self.prominent {
            row.py_3().border_b_1().border_color(cx.theme().border)
        } else {
            row.gap_3()
        }
    }
}
