//! The circular action used by the mode chooser and every transport screen.
//!
//! One diameter everywhere (56 px, from `mockups/app-window/new-session.png`)
//! and two tones. The press ripple is **not** drawn here: the ring is owned by
//! the window ([`crate::components::RippleRing`]) because every round action
//! navigates, and a ring drawn inside the pressed screen would be destroyed the
//! moment the screen changed. This module keeps the geometry
//! ([`ripple_frame`]), the tone's fill colour and the duration the window wave
//! uses.

use std::time::Duration;

use gpui_kit::assets::IconName;
use gpui_kit::base::h_flex;
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, Size};
use gpui_kit::prelude::*;
use gpui_kit::{
    App, ClickEvent, ElementId, IntoElement, MouseDownEvent, RenderOnce, Window, div, px,
};

use crate::components::button_anchors::record_button_anchor;

/// The one round-button diameter, matching the mockup's `w-14` (56 px).
pub const ROUND_BUTTON_SIZE: f32 = 56.0;

/// How far the wave travels before it is gone.
///
/// The circle starts at exactly the button diameter, so it is hidden behind the
/// button, and grows by this much while fading to transparent. Twenty pixels is
/// enough for the edge to appear around the button and vanish again quickly.
pub const RIPPLE_TRAVEL: f32 = 20.0;

/// Diameter the wave starts at: the button's own diameter.
pub const RIPPLE_START_SIZE: f32 = ROUND_BUTTON_SIZE;

/// Diameter at which the wave has fully faded.
pub const RIPPLE_MAX_SIZE: f32 = ROUND_BUTTON_SIZE + RIPPLE_TRAVEL;

/// Debug selector prefix of the window wave.
pub const RIPPLE_SELECTOR: &str = "ripple-ring";

/// How long the wave takes to grow and fade. Short on purpose: the wave is
/// feedback that the press landed, not an animation to watch.
pub const RIPPLE_DURATION: Duration = Duration::from_millis(450);

/// Emphasis of a [`RoundButton`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RoundTone {
    /// Filled brand colour: the action that moves the flow forward.
    #[default]
    Primary,
    /// Outlined surface: the alternative action.
    Secondary,
}

impl RoundTone {
    /// The colour this tone paints its face with — and therefore the colour of
    /// its ripple, which is the same disc seen spreading past the button edge.
    #[must_use]
    pub fn fill(self, theme: &gpui_kit::component::Theme) -> gpui_kit::Hsla {
        match self {
            RoundTone::Primary => theme.primary,
            RoundTone::Secondary => theme.background,
        }
    }
}

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;
type PressHandler = Box<dyn Fn(&MouseDownEvent, &mut Window, &mut App) + 'static>;

/// Wave geometry for a progress value: `(diameter, opacity)`.
///
/// The diameter grows from [`RIPPLE_START_SIZE`] (the button's own diameter) to
/// [`RIPPLE_MAX_SIZE`], and the opacity falls from fully opaque to zero over
/// exactly that travel. `delta` outside `0..=1` is clamped, so a paused or
/// over-running animation frame can never paint an inverted wave.
#[must_use]
pub fn ripple_frame(delta: f32) -> (f32, f32) {
    let delta = delta.clamp(0.0, 1.0);
    // Diameter and opacity advance together: the circle is opaque at the button
    // diameter and fully transparent at `RIPPLE_MAX_SIZE`, so the wave's
    // visibility is exactly its travelled radius.
    (RIPPLE_START_SIZE + RIPPLE_TRAVEL * delta, 1.0 - delta)
}

/// A circular icon action with an optional click ripple.
#[derive(IntoElement)]
pub struct RoundButton {
    id: ElementId,
    icon: IconName,
    tone: RoundTone,
    on_press: Option<PressHandler>,
    on_click: Option<ClickHandler>,
}

impl RoundButton {
    pub fn new(id: impl Into<ElementId>, icon: IconName) -> Self {
        Self {
            id: id.into(),
            icon,
            tone: RoundTone::default(),
            on_press: None,
            on_click: None,
        }
    }

    pub fn tone(mut self, tone: RoundTone) -> Self {
        self.tone = tone;
        self
    }

    /// Runs on press (mouse down), before the click completes — this is where
    /// the window records the ripple, so the ring starts while the pointer is
    /// down and keeps animating after the click navigates.
    pub fn on_press(
        mut self,
        handler: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_press = Some(Box::new(handler));
        self
    }

    /// Without a handler the button runs no command of its own (inside a
    /// [`Card`] that owns the click), but it still shows the hover and press
    /// treatment so the affordance reads as one control.
    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for RoundButton {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();

        // `(background at rest, background on hover)`.
        let (bg, hover_bg) = match self.tone {
            RoundTone::Primary => (theme.primary, theme.primary_active),
            RoundTone::Secondary => (theme.background, theme.secondary_active),
        };
        // The glyph is always the warm off-white ink; a tone never tints it.
        let fg = theme.foreground;
        // The outline is always present and only changes colour, so hovering
        // never nudges the layout.
        let base_outline = match self.tone {
            RoundTone::Primary => theme.primary,
            RoundTone::Secondary => theme.border,
        };
        let hover_outline = theme.primary;

        let anchor_key = element_key(&self.id);
        let tone = self.tone;

        let mut face = h_flex()
            .id(self.id.clone())
            .size(px(ROUND_BUTTON_SIZE))
            .flex_shrink_0()
            .rounded(px(ROUND_BUTTON_SIZE / 2.0))
            .justify_center()
            .items_center()
            .bg(bg)
            .border_2()
            .border_color(base_outline)
            .text_color(fg)
            .cursor_pointer()
            .hover(move |this| this.bg(hover_bg).border_color(hover_outline));

        // Recorded only in test builds: lets a headless test measure the button.
        let selector_key = anchor_key.clone();
        face = face.debug_selector(move || selector_key.clone());

        if let Some(handler) = self.on_press {
            face = face.on_mouse_down(gpui_kit::MouseButton::Left, handler);
        }
        if let Some(handler) = self.on_click {
            face = face.on_click(handler);
        }

        let face = face.child(Icon::new(self.icon).with_size(Size::Large));

        // Record where this button landed, so a press anywhere inside it starts
        // a wave concentric with the button (see `button_anchors`).
        div()
            .on_children_prepainted(move |bounds, _window, cx| {
                if let Some(bounds) = bounds.first() {
                    record_button_anchor(&anchor_key, *bounds, tone, cx);
                }
            })
            .child(face)
    }
}

/// Stable string form of an element id, used as the anchor key.
fn element_key(id: &ElementId) -> String {
    match id {
        ElementId::Name(name) => format!("{name}"),
        ElementId::Integer(value) => format!("#{value}"),
        ElementId::Uuid(uuid) => format!("{uuid}"),
        other => format!("{other:?}"),
    }
}
