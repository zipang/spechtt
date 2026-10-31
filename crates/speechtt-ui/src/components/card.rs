//! A surface card: the two mode choices on the chooser screen.
//!
//! Cards own their click. A [`crate::components::RoundButton`] placed inside a
//! card is visual only, so a click anywhere in the card (title, description,
//! ring) takes the same path and the ripple is driven by the card.

use gpui_kit::base::v_flex;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, ClickEvent, ElementId, IntoElement, MouseDownEvent, RenderOnce, Window, px,
};

use crate::tokens::{radius, space};

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;
type PressHandler = Box<dyn Fn(&MouseDownEvent, &mut Window, &mut App) + 'static>;

/// A rounded, bordered surface that optionally responds to clicks.
///
/// Cards fill their row by default. A chooser-style row pins each card to a
/// fixed [`Card::width`] instead, so the pair keeps its intrinsic size and the
/// surrounding space grows around it.
#[derive(IntoElement)]
pub struct Card {
    id: ElementId,
    children: Vec<AnyElement>,
    width: Option<f32>,
    on_press: Option<PressHandler>,
    on_click: Option<ClickHandler>,
}

impl Card {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            children: Vec::new(),
            width: None,
            on_press: None,
            on_click: None,
        }
    }

    pub fn child(mut self, content: impl IntoElement) -> Self {
        self.children.push(content.into_any_element());
        self
    }

    /// Pin the card to an exact width (a token value) instead of filling its
    /// row. Views pass a `crate::tokens` constant, never a literal.
    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }

    /// Runs on press, before the click completes: the ripple counter is
    /// bumped here so the ring is visible even when the click navigates.
    pub fn on_press(
        mut self,
        handler: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_press = Some(Box::new(handler));
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

        // Recorded only in test builds; lets a headless test click the card.
        let debug_key = element_key(&self.id);

        let mut card = v_flex()
            .id(self.id)
            .debug_selector(move || debug_key)
            .gap(px(space::MD))
            .p(px(space::LG))
            .rounded(px(radius::MD))
            .bg(theme.secondary)
            .border_1()
            .border_color(theme.border)
            .children(self.children);

        card = match self.width {
            Some(width) => card.w(px(width)),
            None => card.w_full(),
        };

        if self.on_click.is_some() || self.on_press.is_some() {
            card = card
                .cursor_pointer()
                .hover(move |this| this.border_color(hover_border))
                .active(move |this| this.bg(pressed_bg));
        }
        if let Some(handler) = self.on_press {
            card = card.on_mouse_down(gpui_kit::MouseButton::Left, handler);
        }
        if let Some(handler) = self.on_click {
            card = card.on_click(handler);
        }

        card
    }
}

/// Stable string form of an element id, for the debug selector.
fn element_key(id: &ElementId) -> String {
    match id {
        ElementId::Name(name) => format!("{name}"),
        ElementId::Integer(value) => format!("#{value}"),
        ElementId::Uuid(uuid) => format!("{uuid}"),
        other => format!("{other:?}"),
    }
}
