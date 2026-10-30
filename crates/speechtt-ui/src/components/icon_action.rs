//! Small square icon actions (Stop, Copy, Save, Open file).

use gpui_kit::assets::IconName;
use gpui_kit::base::h_flex;
use gpui_kit::component::{ActiveTheme as _, Icon};
use gpui_kit::prelude::*;
use gpui_kit::{App, ClickEvent, ElementId, IntoElement, RenderOnce, Window, px};

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

/// Small square icon action (Stop, Copy, Save, Open file).
#[derive(IntoElement)]
pub struct IconAction {
    id: ElementId,
    icon: IconName,
    disabled: bool,
    on_click: Option<ClickHandler>,
}

impl IconAction {
    pub fn new(id: impl Into<ElementId>, icon: IconName) -> Self {
        Self {
            id: id.into(),
            icon,
            disabled: false,
            on_click: None,
        }
    }

    /// Dims the control and drops its cursor, hover and click behavior.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
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

impl RenderOnce for IconAction {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let mut action = h_flex()
            .id(self.id)
            .size(px(34.))
            .rounded(px(10.))
            .justify_center()
            .items_center()
            .bg(cx.theme().secondary)
            .text_color(cx.theme().muted_foreground)
            .opacity(if self.disabled { 0.35 } else { 1.0 });

        if !self.disabled {
            action = action
                .cursor_pointer()
                .hover(|this| this.bg(cx.theme().secondary_hover));
            if let Some(handler) = self.on_click {
                action = action.on_click(handler);
            }
        }

        action.child(Icon::new(self.icon))
    }
}
