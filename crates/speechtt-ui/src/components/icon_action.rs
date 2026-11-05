//! Small square icon actions (Clean up, Copy, Save, Open file).
//!
//! Rendered by `views::session_end` and every screen footer's leading and
//! trailing zones (`components::ScreenFooter`).

use gpui_kit::assets::IconName;
use gpui_kit::base::h_flex;
use gpui_kit::component::{ActiveTheme as _, Icon};
use gpui_kit::prelude::*;
use gpui_kit::{App, ClickEvent, ElementId, IntoElement, RenderOnce, SharedString, Window, px};

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

/// Small square icon action (Clean up, Copy, Save, Open file).
#[derive(IntoElement)]
pub struct IconAction {
    id: ElementId,
    icon: IconName,
    label: SharedString,
    disabled: bool,
    on_click: Option<ClickHandler>,
}

impl IconAction {
    pub fn new(id: impl Into<ElementId>, icon: IconName) -> Self {
        Self {
            id: id.into(),
            icon,
            label: SharedString::default(),
            disabled: false,
            on_click: None,
        }
    }

    /// The accessible name announced for the action.
    ///
    /// The glyph is the only visible content, so every non-decorative
    /// `IconAction` should name itself (Back, Reset, Stop…).
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = label.into();
        self
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

/// Stable string form of an element id, for the debug selector.
fn element_key(id: &ElementId) -> String {
    match id {
        ElementId::Name(name) => format!("{name}"),
        ElementId::Integer(value) => format!("#{value}"),
        ElementId::Uuid(uuid) => format!("{uuid}"),
        other => format!("{other:?}"),
    }
}

impl RenderOnce for IconAction {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        // Recorded only in test builds; lets a headless test press the action.
        let debug_key = element_key(&self.id);

        let mut action = h_flex()
            .id(self.id)
            .debug_selector(move || debug_key)
            .size(px(34.))
            .rounded(px(10.))
            .justify_center()
            .items_center()
            .bg(cx.theme().secondary)
            .text_color(cx.theme().muted_foreground)
            .opacity(if self.disabled { 0.35 } else { 1.0 });

        if !self.label.is_empty() {
            action = action.aria_label(self.label);
        }

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
