//! One footer, three zones, on every widget screen.
//!
//! Actions used to sit wherever each screen put them: a window-level footer on
//! the setup form, a header button on the stopped screen, a third arrangement
//! on the transport screens. "Go back" therefore moved from screen to screen.
//!
//! [`ScreenFooter`] fixes that. Every screen with actions renders this one
//! strip, and the zones always mean the same thing:
//!
//! | Zone | Meaning |
//! |---|---|
//! | [`FooterZone::Leading`] | back, one step towards the mode chooser |
//! | [`FooterZone::Primary`] | the screen's main action, centred |
//! | [`FooterZone::Trailing`] | secondary actions, left to right in insertion order |
//!
//! The side zones are equal flexible columns, so the primary action sits on the
//! footer's own axis whether one side is empty or both are full.

use gpui_kit::assets::IconName;
use gpui_kit::base::h_flex;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, ClickEvent, ElementId, IntoElement, RenderOnce, SharedString, Window, div,
};

use crate::components::IconAction;

/// The three zones of a [`ScreenFooter`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FooterZone {
    /// Left: the back affordance and any leading state.
    Leading,
    /// Centre: the screen's main action.
    Primary,
    /// Right: secondary actions, in insertion order.
    Trailing,
}

/// The shared bottom strip: back on the left, the main action centred,
/// secondary actions on the right.
#[derive(IntoElement)]
pub struct ScreenFooter {
    leading: Vec<AnyElement>,
    primary: Option<AnyElement>,
    trailing: Vec<AnyElement>,
    back_label: Option<SharedString>,
}

impl ScreenFooter {
    pub fn new() -> Self {
        Self {
            leading: Vec::new(),
            primary: None,
            trailing: Vec::new(),
            back_label: None,
        }
    }

    /// Adds the back affordance to the leading zone.
    ///
    /// Back is an [`IconAction`] — the same 34 px square as every other
    /// secondary action — so the big round button is unmistakably the main
    /// one. `label` is its accessible name ("Back", "Cancel", …).
    pub fn back(
        mut self,
        id: impl Into<ElementId>,
        label: &'static str,
        on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.leading.push(
            IconAction::new(id, IconName::ArrowBigLeft)
                .label(label)
                .on_click(on_click)
                .into_any_element(),
        );
        self.back_label = Some(label.into());
        self
    }

    /// Adds any other element to the leading zone (after the back action).
    pub fn leading(mut self, element: impl IntoElement) -> Self {
        self.leading.push(element.into_any_element());
        self
    }

    /// Adds the screen's main action to the centred zone.
    pub fn primary(mut self, element: impl IntoElement) -> Self {
        self.primary = Some(element.into_any_element());
        self
    }

    /// Adds a secondary action to the trailing zone, left to right in
    /// insertion order.
    pub fn trailing(mut self, element: impl IntoElement) -> Self {
        self.trailing.push(element.into_any_element());
        self
    }

    /// The accessible name of the back action, if one was added.
    #[must_use]
    pub fn back_label(&self) -> Option<&str> {
        self.back_label.as_deref()
    }

    /// Whether a back action was added (it always leads the leading zone).
    #[must_use]
    pub fn has_back(&self) -> bool {
        self.back_label.is_some()
    }

    /// How many elements sit in `zone`. Empty zones are common and render
    /// nothing; this is the builder's only testable shape.
    #[must_use]
    pub fn zone_len(&self, zone: FooterZone) -> usize {
        match zone {
            FooterZone::Leading => self.leading.len(),
            FooterZone::Primary => usize::from(self.primary.is_some()),
            FooterZone::Trailing => self.trailing.len(),
        }
    }

    /// Whether every zone is empty (the footer then renders nothing).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.leading.is_empty() && self.primary.is_none() && self.trailing.is_empty()
    }
}

impl Default for ScreenFooter {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderOnce for ScreenFooter {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let Self {
            leading,
            primary,
            trailing,
            back_label: _,
        } = self;

        if leading.is_empty() && primary.is_none() && trailing.is_empty() {
            return div().into_any_element();
        }

        let leading = h_flex()
            .flex_1()
            .min_w_0()
            .items_center()
            .gap_2()
            .children(leading);
        let mut centre = h_flex().flex_1().min_w_0().items_center().justify_center();
        if let Some(primary) = primary {
            centre = centre.child(primary);
        }
        let trailing = h_flex()
            .flex_1()
            .min_w_0()
            .items_center()
            .justify_end()
            .gap_2()
            .children(trailing);

        h_flex()
            .w_full()
            .flex_shrink_0()
            .items_center()
            .px_5()
            .py_2p5()
            .bg(cx.theme().title_bar)
            .border_t_1()
            .border_color(cx.theme().title_bar_border)
            .child(leading)
            .child(centre)
            .child(trailing)
            .into_any_element()
    }
}
