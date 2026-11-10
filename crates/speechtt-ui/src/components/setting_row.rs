//! A labelled setting row: a title, a fainter description and a control slot
//! on the trailing edge. Rendered by the Session setup form and the Settings
//! dialog.
//!
//! The default (compact) form uses an uppercase micro-label so several rows
//! fit inside the widget. [`SettingRow::prominent`] uses the roomier dialog
//! style: a larger title, muted helper text and a hairline separator between
//! rows, as used by the provider/model setup form and the Settings dialog.
//! [`SettingRow::dense`], [`SettingRow::stacked`], [`SettingRow::accessory`]
//! and [`SettingRow::mono_description`] are opt-in refinements for a dialog
//! that must fit its window; the default and prominent forms keep their
//! rhythm.

use gpui_kit::base::{h_flex, v_flex};
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, App, IntoElement, RenderOnce, SharedString, Window, div, rems};

use crate::components::{Body, Label};

#[derive(IntoElement)]
pub struct SettingRow {
    label: SharedString,
    description: SharedString,
    control: Option<AnyElement>,
    accessory: Option<AnyElement>,
    prominent: bool,
    dense: bool,
    stacked: bool,
    mono_description: bool,
}

impl SettingRow {
    pub fn new(label: impl Into<SharedString>, description: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            description: description.into(),
            control: None,
            accessory: None,
            prominent: false,
            dense: false,
            stacked: false,
            mono_description: false,
        }
    }

    /// Uses the dialog style: large light title, muted description and a
    /// bottom hairline separating rows.
    pub fn prominent(mut self) -> Self {
        self.prominent = true;
        self
    }

    /// Tighter, symmetric vertical rhythm for a dialog that must fit its window.
    ///
    /// The row keeps `prominent`'s typography and hairline; only its vertical
    /// padding shrinks and becomes equal above and below the content, so a
    /// divider sits centred between two rows. Off unless called:
    /// `views::session_setup` keeps the roomier prominent form.
    pub fn dense(mut self) -> Self {
        self.dense = true;
        self
    }

    /// Puts the control on its own line, spanning the row, below the label.
    ///
    /// For a control that needs the full width — a text field, not a dropdown —
    /// where a trailing slot would squeeze the label to nothing.
    pub fn stacked(mut self) -> Self {
        self.stacked = true;
        self
    }

    /// Adds an element to the label line's trailing edge, opposite the title.
    ///
    /// Independent of the control: a stacked row can carry a status badge
    /// beside its title while its control spans the row below.
    pub fn accessory(mut self, accessory: impl IntoElement) -> Self {
        self.accessory = Some(accessory.into_any_element());
        self
    }

    /// Renders the description in the monospace face.
    ///
    /// For a description that is itself an identifier — an environment-variable
    /// name, a path — which reads as code rather than prose. Off unless called:
    /// `DESIGN.md` §Settings panel keeps values in `base` and reserves mono for
    /// identifiers, so only the row whose description is one asks for it.
    pub fn mono_description(mut self) -> Self {
        self.mono_description = true;
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
        let dialog_style = self.prominent || self.dense;

        let (title, description) = if dialog_style {
            let mut description = Body::new(self.description).muted();
            if self.mono_description {
                description = description.mono();
            }
            (
                Body::new(self.label).bold().into_any_element(),
                description.into_any_element(),
            )
        } else {
            let mut description = Label::new(self.description);
            if self.mono_description {
                description = description.mono();
            }
            (
                Label::new(self.label).bold().muted().into_any_element(),
                description.into_any_element(),
            )
        };

        let mut label_line = h_flex().justify_between().items_start().gap_4().child(
            v_flex()
                .flex_shrink_1()
                .min_w_0()
                .gap_0p5()
                .child(title)
                .child(description),
        );
        if let Some(accessory) = self.accessory {
            label_line = label_line.child(accessory);
        }

        let row = if self.stacked {
            let mut row = v_flex().w_full().gap_2().child(label_line.w_full());
            if let Some(control) = self.control {
                row = row.child(div().w_full().child(control));
            }
            row
        } else {
            let mut slot = h_flex()
                .flex_1()
                .min_w(rems(8.0))
                .max_w(rems(18.0))
                .justify_end();
            if let Some(control) = self.control {
                slot = slot.child(control);
            }
            h_flex()
                .w_full()
                .justify_between()
                .items_center()
                .gap_4()
                .child(label_line.flex_shrink_1().min_w_0())
                .child(slot)
        };

        if self.dense {
            row.py_2().border_b_1().border_color(cx.theme().border)
        } else if self.prominent {
            row.py_3().border_b_1().border_color(cx.theme().border)
        } else {
            row.gap_3()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The face is not observable from outside a rendered frame: a row's copy is
    /// anonymous text, and the test kit exposes no rendered-text or font-family
    /// reader. What a test can pin is the flag the builder sets, which
    /// [`SettingRow::render`] is the only reader of.
    #[test]
    fn the_monospace_description_is_opt_in() {
        assert!(
            !SettingRow::new("API key", "AI_GATEWAY_API_KEY").mono_description,
            "an identifier is prose unless the caller says otherwise"
        );
        assert!(
            SettingRow::new("API key", "AI_GATEWAY_API_KEY")
                .mono_description()
                .mono_description
        );
    }
}
