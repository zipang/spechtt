//! Between renders: where each round button sits in window coordinates.
//!
//! The press ripple is painted at the window root (so it outlives the screen
//! change its click triggers), which means it needs the *button's* centre, not
//! the pointer position: a click near a button's edge must still produce a wave
//! concentric with that button.
//!
//! A [`crate::components::RoundButton`] records its bounds here while
//! prepainting, and the window looks the anchor up when a press starts. This is
//! layout bookkeeping only — it never drives what a frame draws.

use std::cell::RefCell;
use std::collections::BTreeMap;

use gpui_kit::{App, Bounds, Global, Pixels, Point};

use crate::components::RoundTone;

/// Where a round button was painted, and the tone that gives its wave colour.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Anchor {
    pub center: Point<Pixels>,
    pub tone: RoundTone,
}

/// Anchor of every round button painted in the last frame, keyed by element id.
#[derive(Default)]
pub struct ButtonAnchors {
    anchors: RefCell<BTreeMap<String, Anchor>>,
}

impl Global for ButtonAnchors {}

/// Records a button's bounds and tone for the next press. Called from prepaint.
pub fn record_button_anchor(id: &str, bounds: Bounds<Pixels>, tone: RoundTone, cx: &mut App) {
    cx.default_global::<ButtonAnchors>()
        .anchors
        .borrow_mut()
        .insert(
            id.to_owned(),
            Anchor {
                center: bounds.center(),
                tone,
            },
        );
}

/// The anchor of button `id`, if it was painted.
pub fn button_anchor(id: &str, cx: &App) -> Option<Anchor> {
    cx.try_global::<ButtonAnchors>()
        .and_then(|anchors| anchors.anchors.borrow().get(id).copied())
}
