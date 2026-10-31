//! The press wave: a filled circle the size of the button that spreads and
//! fades.
//!
//! It is *the button's own colour*, starting at the button's own diameter and
//! centre: at rest it is completely hidden behind the button face. As it grows
//! by [`RIPPLE_TRAVEL`] it fades to transparent, so all the user ever sees is a
//! thin slice of the disc appearing around the button edge and vanishing.
//!
//! It lives at the **window root**, painted before the screen, for two reasons:
//!
//! - painted *behind* the screen it emerges from under the button instead of
//!   tinting the glyph on top of it;
//! - owned by the window it survives the navigation every round action
//!   triggers (chooser → form, Ready → Recording, Pause → Paused). A wave drawn
//!   inside the pressed screen would be destroyed on release, which is exactly
//!   why an earlier version animated something nobody could see.
//!
//! The centre comes from [`super::button_anchors`], recorded while the button
//! prepainted, so the wave is concentric with the button no matter where inside
//! it the pointer landed. Geometry is [`crate::components::ripple_frame`], the
//! pure function the unit tests pin down.

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::prelude::*;
use gpui_kit::{Animation, AnimationExt as _, App, IntoElement, RenderOnce, Window, div, px};

use crate::components::round_button::{
    RIPPLE_DURATION, RIPPLE_MAX_SIZE, RIPPLE_SELECTOR, RIPPLE_START_SIZE, RoundTone, ripple_frame,
};

/// Which press a wave belongs to. A new press gets a new sequence number, so
/// the animation element is rebuilt and the wave starts again from the edge.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ripple {
    pub seq: u64,
    /// Window coordinates of the pressed button's centre.
    pub x: f32,
    pub y: f32,
    /// The pressed button's tone: the wave is that button's own colour.
    pub tone: RoundTone,
}

impl Ripple {
    pub fn new(seq: u64, x: f32, y: f32, tone: RoundTone) -> Self {
        Self { seq, x, y, tone }
    }
}

#[derive(IntoElement)]
pub struct RippleRing {
    ripple: Ripple,
}

impl RippleRing {
    pub fn new(ripple: Ripple) -> Self {
        Self { ripple }
    }
}

impl RenderOnce for RippleRing {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let color = self.ripple.tone.fill(cx.theme());
        let ripple = self.ripple;

        // A full-size box centred on the button, so the growing circle is
        // positioned by its own geometry rather than by flex alignment.
        div()
            .absolute()
            .top(px(ripple.y - RIPPLE_MAX_SIZE / 2.0))
            .left(px(ripple.x - RIPPLE_MAX_SIZE / 2.0))
            .size(px(RIPPLE_MAX_SIZE))
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .debug_selector(move || RIPPLE_SELECTOR.to_owned())
                    .flex_none()
                    .rounded(px(RIPPLE_START_SIZE / 2.0))
                    .bg(color)
                    .with_animation(
                        format!("{RIPPLE_SELECTOR}-{}", ripple.seq),
                        Animation::new(RIPPLE_DURATION),
                        move |wave, delta| {
                            let (diameter, opacity) = ripple_frame(delta);
                            wave.size(px(diameter))
                                .rounded(px(diameter / 2.0))
                                .bg(color.opacity(opacity))
                        },
                    ),
            )
    }
}
