//! Selectable pill used for provider, model and output choices.
//!
//! The owner supplies `selected` and receives a requested change through
//! `on_click`; the component keeps no state.

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::prelude::*;
use gpui_kit::{
    App, ClickEvent, ElementId, IntoElement, RenderOnce, SharedString, Window, div, px, rems,
};

use crate::tokens::{radius, rem};

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

#[derive(IntoElement)]
pub struct ChoicePill {
    id: ElementId,
    label: SharedString,
    selected: bool,
    on_click: Option<ClickHandler>,
}

impl ChoicePill {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            selected: false,
            on_click: None,
        }
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
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

impl RenderOnce for ChoicePill {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let (bg, fg) = if self.selected {
            (cx.theme().primary, cx.theme().primary_foreground)
        } else {
            (cx.theme().secondary, cx.theme().foreground)
        };

        let mut pill = div()
            .id(self.id)
            .px_3()
            .py_1()
            .rounded(px(radius::SM))
            .bg(bg)
            .text_color(fg)
            .text_size(rems(rem::SM))
            .cursor_pointer()
            .hover(|this| this.bg(cx.theme().secondary_hover));
        if let Some(handler) = self.on_click {
            pill = pill.on_click(handler);
        }
        pill.child(self.label)
    }
}
