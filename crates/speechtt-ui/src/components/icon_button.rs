//! Icon-only buttons for window chrome: header actions and dialog close.
//!
//! One 32px target and one ghost treatment on every screen, with an
//! accessible label because the glyph is the only visible content.

use gpui_kit::assets::IconName;
use gpui_kit::component::Disableable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{Icon, Sizable as _, Size};
use gpui_kit::{App, ClickEvent, ElementId, IntoElement, RenderOnce, SharedString, Window};

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

/// A ghost icon button with the shared chrome size.
#[derive(IntoElement)]
pub struct IconButton {
    id: ElementId,
    icon: IconName,
    label: SharedString,
    disabled: bool,
    on_click: Option<ClickHandler>,
}

impl IconButton {
    pub fn new(id: impl Into<ElementId>, icon: IconName) -> Self {
        Self {
            id: id.into(),
            icon,
            label: SharedString::default(),
            disabled: false,
            on_click: None,
        }
    }

    /// The accessible name announced for the button.
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = label.into();
        self
    }

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

impl RenderOnce for IconButton {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let mut button = Button::new(self.id)
            .with_size(Size::Medium)
            .ghost()
            .icon(Icon::new(self.icon))
            .disabled(self.disabled);
        if !self.label.is_empty() {
            button = button.accessibility_label(self.label);
        }
        if let Some(handler) = self.on_click {
            button = button.on_click(handler);
        }
        button
    }
}
