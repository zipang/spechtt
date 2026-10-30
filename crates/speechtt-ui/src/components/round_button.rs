//! The circular action used by the mode chooser and every transport screen.
//!
//! One diameter everywhere (56 px, from `mockups/app-window/new-session.png`),
//! two tones, and a single expanding ring on click. The ring is what tells the
//! user their click landed before the screen changes, so it is driven by a
//! monotonic counter owned by the view (`SpeechTtApp::pulse_count`) and is
//! therefore testable without inspecting paint output.

use std::time::Duration;

use gpui_kit::assets::IconName;
use gpui_kit::base::{h_flex, v_flex};
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, Size};
use gpui_kit::prelude::*;
use gpui_kit::{Animation, AnimationExt as _, App, ClickEvent, ElementId, IntoElement};
use gpui_kit::{RenderOnce, Window, div, ease_out_quint, px};

/// The one round-button diameter, matching the mockup's `w-14` (56 px).
pub const ROUND_BUTTON_SIZE: f32 = 56.0;

/// Ring diameter at the end of the ripple.
const RIPPLE_MAX_SIZE: f32 = 84.0;

/// Ring alpha at the start of the ripple.
const RIPPLE_ALPHA: f32 = 0.55;

/// How long the ring takes to expand and fade.
const RIPPLE_DURATION: Duration = Duration::from_millis(400);

/// Emphasis of a [`RoundButton`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RoundTone {
    /// Filled brand colour: the action that moves the flow forward.
    #[default]
    Primary,
    /// Outlined surface: the alternative action.
    Secondary,
}

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

/// Ring geometry for an eased progress value: `(diameter, border alpha)`.
///
/// The diameter grows from [`ROUND_BUTTON_SIZE`] to [`RIPPLE_MAX_SIZE`] and the
/// alpha fades to zero. `delta` outside `0..=1` is clamped, so a paused or
/// over-running animation frame can never paint an inverted ring.
#[must_use]
pub fn ripple_frame(delta: f32) -> (f32, f32) {
    let delta = delta.clamp(0.0, 1.0);
    (
        ROUND_BUTTON_SIZE + (RIPPLE_MAX_SIZE - ROUND_BUTTON_SIZE) * delta,
        RIPPLE_ALPHA * (1.0 - delta),
    )
}

/// A circular icon action with an optional click ripple.
#[derive(IntoElement)]
pub struct RoundButton {
    id: ElementId,
    icon: IconName,
    tone: RoundTone,
    pulse: u64,
    on_click: Option<ClickHandler>,
}

impl RoundButton {
    pub fn new(id: impl Into<ElementId>, icon: IconName) -> Self {
        Self {
            id: id.into(),
            icon,
            tone: RoundTone::default(),
            pulse: 0,
            on_click: None,
        }
    }

    pub fn tone(mut self, tone: RoundTone) -> Self {
        self.tone = tone;
        self
    }

    /// Monotonic click counter from [`crate::app::SpeechTtApp::pulse_count`].
    /// `0` means "no ripple yet"; every later value starts exactly one ring.
    pub fn pulse(mut self, pulse: u64) -> Self {
        self.pulse = pulse;
        self
    }

    /// Without a handler the button is visual only (inside a [`Card`] that owns
    /// the click): no hover, no cursor, no click.
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
        let interactive = self.on_click.is_some();
        let theme = cx.theme();

        // `(background, hover background, icon colour, ripple colour)`.
        let (bg, hover_bg, fg, ripple) = match self.tone {
            RoundTone::Primary => (
                theme.primary,
                theme.primary_active,
                theme.primary_foreground,
                theme.primary,
            ),
            RoundTone::Secondary => (
                theme.background,
                theme.secondary_active,
                theme.foreground,
                theme.border,
            ),
        };
        let hover_border = theme.primary.opacity(0.4);

        let mut face = h_flex()
            .id(self.id.clone())
            .size(px(ROUND_BUTTON_SIZE))
            .flex_shrink_0()
            .rounded(px(ROUND_BUTTON_SIZE / 2.0))
            .justify_center()
            .items_center()
            .bg(bg)
            .text_color(fg);

        if self.tone == RoundTone::Secondary {
            face = face.border_1().border_color(theme.border);
        }

        if interactive {
            face = face
                .cursor_pointer()
                .hover(move |this| this.bg(hover_bg).border_color(hover_border));
            if let Some(handler) = self.on_click {
                face = face.on_click(handler);
            }
        }

        let face = face.child(Icon::new(self.icon).with_size(Size::Large));

        let mut container = v_flex()
            .relative()
            .flex_none()
            .size(px(ROUND_BUTTON_SIZE))
            .justify_center()
            .items_center();

        if self.pulse > 0 {
            let animation_id = format!("{}-ripple-{}", animation_key(&self.id), self.pulse);
            let ring = div()
                .rounded(px(ROUND_BUTTON_SIZE / 2.0))
                .border_2()
                .border_color(ripple.opacity(RIPPLE_ALPHA))
                .with_animation(
                    animation_id,
                    Animation::new(RIPPLE_DURATION).with_easing(ease_out_quint()),
                    move |ring, delta| {
                        let (diameter, alpha) = ripple_frame(delta);
                        ring.size(px(diameter))
                            .rounded(px(diameter / 2.0))
                            .border_color(ripple.opacity(alpha))
                    },
                );
            container = container.child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(ring),
            );
        }

        container.child(face)
    }
}

/// Stable string form of an element id, for deriving child animation ids.
fn animation_key(id: &ElementId) -> String {
    match id {
        ElementId::Name(name) => format!("{name}"),
        ElementId::Integer(value) => format!("#{value}"),
        ElementId::Uuid(uuid) => format!("{uuid}"),
        other => format!("{other:?}"),
    }
}
