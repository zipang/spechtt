//! A labelled setting row: uppercase micro-label, helper line and a control
//! slot on the trailing edge. Shared by the setup form and Settings dialogs.

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
}

impl SettingRow {
    pub fn new(label: impl Into<SharedString>, description: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            description: description.into(),
            control: None,
        }
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

        h_flex()
            .justify_between()
            .items_center()
            .gap_3()
            .child(
                v_flex()
                    .flex_shrink_1()
                    .min_w_0()
                    .gap_0p5()
                    .child(
                        div()
                            .text_size(rems(rem::XS))
                            .font_weight(FontWeight::BOLD)
                            .text_color(cx.theme().muted_foreground)
                            .child(self.label),
                    )
                    .child(div().text_size(rems(rem::SM)).child(self.description)),
            )
            .child(slot)
    }
}
