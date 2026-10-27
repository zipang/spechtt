//! Text action buttons for dialog footers and forms: one size everywhere.
//!
//! The size is fixed at the gpui-component medium size (`--font-size-md`,
//! `h_8`, roomy horizontal padding) so action buttons cannot drift apart
//! screen by screen. Only the primary/secondary tones and the disabled state
//! are configurable. Dense inline controls (Reveal, icon buttons) use their
//! own components instead.

use gpui_kit::component::Disableable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{Sizable as _, Size};
use gpui_kit::{App, ClickEvent, ElementId, IntoElement, RenderOnce, SharedString, Window};

/// Emphasis of an [`ActionButton`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ActionTone {
    /// The one committing action in a group (Save, Create, Got it).
    #[default]
    Primary,
    /// The dismissing or alternative action (Cancel, Back).
    Secondary,
}

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

/// A labelled text button with the shared action-button size.
#[derive(IntoElement)]
pub struct ActionButton {
    id: ElementId,
    label: SharedString,
    tone: ActionTone,
    disabled: bool,
    on_click: Option<ClickHandler>,
}

impl ActionButton {
    pub fn primary(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self::new(id, label, ActionTone::Primary)
    }

    pub fn secondary(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self::new(id, label, ActionTone::Secondary)
    }

    fn new(id: impl Into<ElementId>, label: impl Into<SharedString>, tone: ActionTone) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            tone,
            disabled: false,
            on_click: None,
        }
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

impl RenderOnce for ActionButton {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let mut button = Button::new(self.id)
            .with_size(Size::Medium)
            .label(self.label)
            .disabled(self.disabled);
        button = match self.tone {
            ActionTone::Primary => button.primary(),
            ActionTone::Secondary => button.secondary(),
        };
        if let Some(handler) = self.on_click {
            button = button.on_click(handler);
        }
        button
    }
}
