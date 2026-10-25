//! Transport controls: the large circular action and the small square icon
//! action used for Stop / Copy / Save / Open file.

use gpui_kit::assets::IconName;
use gpui_kit::base::h_flex;
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, Size};
use gpui_kit::prelude::*;
use gpui_kit::{App, ClickEvent, ElementId, Hsla, IntoElement, RenderOnce, Window, px};

/// Visual and geometric variant of a [`TransportButton`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TransportTone {
    /// Record / pause / new session: brand primary, 74px.
    #[default]
    Primary,
    /// Resume: warning amber, 64px.
    Warning,
}

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

/// The large circular transport action from the mockups.
#[derive(IntoElement)]
pub struct TransportButton {
    id: ElementId,
    icon: IconName,
    tone: TransportTone,
    on_click: Option<ClickHandler>,
}

impl TransportButton {
    pub fn new(id: impl Into<ElementId>, icon: IconName) -> Self {
        Self {
            id: id.into(),
            icon,
            tone: TransportTone::default(),
            on_click: None,
        }
    }

    pub fn tone(mut self, tone: TransportTone) -> Self {
        self.tone = tone;
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

impl RenderOnce for TransportButton {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let (diameter, bg, hover_bg, fg): (f32, Hsla, Hsla, Hsla) = match self.tone {
            TransportTone::Primary => (
                74.0,
                cx.theme().primary,
                cx.theme().primary_hover,
                cx.theme().primary_foreground,
            ),
            TransportTone::Warning => (
                64.0,
                cx.theme().warning,
                cx.theme().warning,
                cx.theme().warning_foreground,
            ),
        };

        let mut button = h_flex()
            .id(self.id)
            .size(px(diameter))
            .rounded(px(diameter / 2.0))
            .justify_center()
            .items_center()
            .bg(bg)
            .text_color(fg);

        if let Some(handler) = self.on_click {
            button = button
                .hover(move |this| this.bg(hover_bg))
                .cursor_pointer()
                .on_click(handler);
        }

        button.child(Icon::new(self.icon).with_size(Size::Large))
    }
}

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
