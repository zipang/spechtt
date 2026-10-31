//! A [`RoundButton`] that is a **switch**: one glyph, two states, on or off.
//!
//! The caller supplies the glyph and the state; the two tones and the motion
//! that marks "on" belong to the component.
//!
//! | State | Tone | Face | Motion |
//! |---|---|---|---|
//! | [`ToggleState::Off`] | `RoundTone::Secondary` | `surface` fill, `brand.secondary` outline | none |
//! | [`ToggleState::On`] | `RoundTone::Primary` | `brand.primary` fill and outline | pulse + ring wave |
//!
//! The take screen uses it for its record button: a microphone that is on while
//! capturing. `Off` is visually at rest but still clickable — pressing it is how
//! it turns on.
//!
//! The motion reuses the press-wave trick documented in [`super::ripple`]: two
//! discs in the button's own colour are drawn *behind* the face, so the part
//! that overlaps the button is invisible and all the user sees is a sliver
//! appearing past its edge. GPUI exposes a transform only for SVG, so the
//! "pulse" is the same disc drawn at [`pulse_scale`] of the diameter; the
//! geometry is pure and pinned by unit tests.

use std::time::Duration;

use gpui_kit::assets::IconName;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::prelude::*;
use gpui_kit::{
    Animation, AnimationExt as _, App, ClickEvent, ElementId, IntoElement, MouseDownEvent,
    RenderOnce, Window, div, px,
};

use crate::components::round_button::{RIPPLE_TRAVEL, ROUND_BUTTON_SIZE, RoundButton, RoundTone};

/// One full pulse: grow, then settle back. Slow enough to read as breathing,
/// fast enough that a glance confirms the button is on.
pub const TOGGLE_PULSE_PERIOD: Duration = Duration::from_millis(1600);

/// How much larger than the button the pulse reaches (6 %).
pub const TOGGLE_PULSE_AMPLITUDE: f32 = 0.06;

/// Diameter at which an "on" wave has fully faded.
///
/// The same travel as the press wave ([`RIPPLE_TRAVEL`]), which also makes the
/// wave exactly the height of the footer's button band: it fades out before it
/// can reach the strip's edge.
pub const TOGGLE_WAVE_MAX_SIZE: f32 = ROUND_BUTTON_SIZE + RIPPLE_TRAVEL;

/// Debug selector prefix of the repeating wave.
pub const TOGGLE_WAVE_SELECTOR: &str = "toggle-wave";

/// Debug selector prefix of the pulse disc.
pub const TOGGLE_PULSE_SELECTOR: &str = "toggle-pulse";

/// Whether the switch is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToggleState {
    /// At rest: dark face, completely still.
    Off,
    /// Active: brand-coloured face, pulsing with a repeating wave.
    On,
}

impl ToggleState {
    /// The round-button tone this state reuses. No new tone is introduced.
    #[must_use]
    pub const fn tone(self) -> RoundTone {
        match self {
            ToggleState::On => RoundTone::Primary,
            ToggleState::Off => RoundTone::Secondary,
        }
    }

    /// Whether the state animates (only `On` moves).
    #[must_use]
    pub const fn is_animating(self) -> bool {
        matches!(self, ToggleState::On)
    }

    /// The state for a `switch`: `true` is on.
    #[must_use]
    pub const fn from_active(active: bool) -> Self {
        if active {
            ToggleState::On
        } else {
            ToggleState::Off
        }
    }
}

/// Diameter multiplier of the pulse at `phase` (0..=1, clamped).
///
/// `1.00` at both ends and `1.00 + TOGGLE_PULSE_AMPLITUDE` at the midpoint, so
/// a period starts and ends on the resting size and the loop is seamless. The
/// curve is the raised cosine, which has zero slope at both ends: no visible
/// snap when the animation wraps.
#[must_use]
pub fn pulse_scale(phase: f32) -> f32 {
    let phase = phase.clamp(0.0, 1.0);
    let eased = (1.0 - (std::f32::consts::TAU * phase).cos()) / 2.0;
    1.0 + TOGGLE_PULSE_AMPLITUDE * eased
}

/// Wave geometry for a progress value: `(diameter, opacity)`.
///
/// The diameter grows from the button's own diameter to [`TOGGLE_WAVE_MAX_SIZE`]
/// while the opacity falls from fully opaque to zero over exactly that travel,
/// mirroring [`super::ripple_frame`]. `phase` outside `0..=1` is clamped, so an
/// over-running frame can never paint an inverted wave.
#[must_use]
pub fn toggle_wave_frame(phase: f32) -> (f32, f32) {
    let phase = phase.clamp(0.0, 1.0);
    let travel = TOGGLE_WAVE_MAX_SIZE - ROUND_BUTTON_SIZE;
    (ROUND_BUTTON_SIZE + travel * phase, 1.0 - phase)
}

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;
type PressHandler = Box<dyn Fn(&MouseDownEvent, &mut Window, &mut App) + 'static>;

/// The round button as a two-state switch: dark and still, or brand-coloured
/// and pulsing.
#[derive(IntoElement)]
pub struct ToggleRoundButton {
    id: ElementId,
    icon: IconName,
    state: ToggleState,
    disabled: bool,
    on_press: Option<PressHandler>,
    on_click: Option<ClickHandler>,
}

impl ToggleRoundButton {
    pub fn new(id: impl Into<ElementId>, icon: IconName, state: ToggleState) -> Self {
        Self {
            id: id.into(),
            icon,
            state,
            disabled: false,
            on_press: None,
            on_click: None,
        }
    }

    /// Dims the control and drops its cursor, press and click behaviour.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Runs on press, where the window records its press wave.
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

    /// The state this switch was built with (test surface).
    #[must_use]
    pub fn state(&self) -> ToggleState {
        self.state
    }

    /// Whether the switch will animate when rendered (test surface).
    #[must_use]
    pub fn is_animating(&self) -> bool {
        !self.disabled && self.state.is_animating()
    }
}

impl RenderOnce for ToggleRoundButton {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let color = cx.theme().primary;
        let animating = self.is_animating();
        let mut button = RoundButton::new(self.id, self.icon).tone(self.state.tone());

        if !self.disabled {
            if let Some(handler) = self.on_press {
                button = button.on_press(handler);
            }
            if let Some(handler) = self.on_click {
                button = button.on_click(handler);
            }
        }
        let button = button.into_any_element();

        if !animating {
            let face = div().flex_none().child(button);
            return if self.disabled {
                face.opacity(0.35).into_any_element()
            } else {
                face.into_any_element()
            };
        }

        // Both discs live in a square bigger than the face, centred on it, so
        // their growth spills past the button edge instead of moving the
        // footer. They are painted before the face, so they sit behind it.
        let surround = (TOGGLE_WAVE_MAX_SIZE - ROUND_BUTTON_SIZE) / 2.0;
        let host = |selector: &'static str| {
            div()
                .absolute()
                .top(px(-surround))
                .left(px(-surround))
                .size(px(TOGGLE_WAVE_MAX_SIZE))
                .flex()
                .items_center()
                .justify_center()
                .debug_selector(move || selector.to_owned())
        };

        let pulse = div()
            .flex_none()
            .bg(color)
            .rounded(px(ROUND_BUTTON_SIZE / 2.0))
            .with_animation(
                "toggle-pulse",
                Animation::new(TOGGLE_PULSE_PERIOD).repeat(),
                move |disc, phase| {
                    let diameter = ROUND_BUTTON_SIZE * pulse_scale(phase);
                    disc.size(px(diameter)).rounded(px(diameter / 2.0))
                },
            );

        let wave = div()
            .flex_none()
            .bg(color)
            .rounded(px(ROUND_BUTTON_SIZE / 2.0))
            .with_animation(
                "toggle-wave",
                Animation::new(TOGGLE_PULSE_PERIOD).repeat(),
                move |disc, phase| {
                    let (diameter, opacity) = toggle_wave_frame(phase);
                    disc.size(px(diameter))
                        .rounded(px(diameter / 2.0))
                        .bg(color.opacity(opacity))
                },
            );

        div()
            .relative()
            .size(px(ROUND_BUTTON_SIZE))
            .flex_none()
            .child(host(TOGGLE_PULSE_SELECTOR).child(pulse))
            .child(host(TOGGLE_WAVE_SELECTOR).child(wave))
            .child(button)
            .into_any_element()
    }
}
