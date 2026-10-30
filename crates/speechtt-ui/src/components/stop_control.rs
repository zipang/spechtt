//! The square Stop action shown by the recording and paused screens.
//!
//! It is rendered in both states so the transport row never shifts when the
//! take pauses; it is inert (dimmed, no cursor, no click) while recording.

use gpui_kit::assets::IconName;
use gpui_kit::{App, ClickEvent, IntoElement, RenderOnce, Window};

use crate::components::IconAction;

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

#[derive(IntoElement)]
pub struct StopControl {
    enabled: bool,
    on_click: Option<ClickHandler>,
}

impl StopControl {
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            on_click: None,
        }
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for StopControl {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let mut control = IconAction::new("btn-stop", IconName::Square).disabled(!self.enabled);
        if let Some(handler) = self.on_click {
            control = control.on_click(handler);
        }
        control
    }
}
