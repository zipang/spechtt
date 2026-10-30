//! A surface card: the two mode choices on the chooser screen.
//!
//! Cards own their click. A [`crate::components::RoundButton`] placed inside a
//! card is visual only, so a click anywhere in the card (title, description,
//! ring) takes the same path and the ripple is driven by the card.

use gpui_kit::base::v_flex;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, ClickEvent, DefiniteLength, ElementId, IntoElement, RenderOnce, Window, px,
};

use crate::tokens::{radius, space};

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

/// A rounded, bordered surface that optionally responds to clicks.
///
/// Cards fill their row by default. A chooser-style row pins each card to a
/// fixed [`Card::width`] instead, so the pair keeps its intrinsic size and the
/// surrounding space grows around it.
#[derive(IntoElement)]
pub struct Card {
    id: ElementId,
    children: Vec<AnyElement>,
    width: Option<DefiniteLength>,
    on_click: Option<ClickHandler>,
}

impl Card {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            children: Vec::new(),
            width: None,
            on_click: None,
        }
    }

    pub fn child(mut self, content: impl IntoElement) -> Self {
        self.children.push(content.into_any_element());
        self
    }

    /// Pin the card to an exact width instead of filling its row.
    pub fn width(mut self, width: impl Into<DefiniteLength>) -> Self {
        self.width = Some(width.into());
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for Card {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let hover_border = theme.primary.opacity(0.5);
        let pressed_bg = theme.button_active;

        let mut card = v_flex()
            .id(self.id)
            .gap(px(space::MD))
            .p(px(space::LG))
            .rounded(px(radius::MD))
            .bg(theme.secondary)
            .border_1()
            .border_color(theme.border)
            .children(self.children);

        card = match self.width {
            Some(width) => card.w(width),
            None => card.w_full(),
        };

        if let Some(handler) = self.on_click {
            card = card
                .cursor_pointer()
                .hover(move |this| this.border_color(hover_border))
                .active(move |this| this.bg(pressed_bg))
                .on_click(handler);
        }

        card
    }
}
