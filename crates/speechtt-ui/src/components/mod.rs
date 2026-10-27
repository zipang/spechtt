//! SpeechTT visual components (RenderOnce), built from design tokens.
//!
//! These are the pieces the mockups define that gpui-component does not
//! ship: the state badge, elapsed timer, waveform, caption well, transport
//! controls and setup-form controls. They read `crate::tokens` / the active
//! theme — no raw colors.
//!
//! Behavior lives in `controller.rs` / `speechtt-core`; components render what
//! they are given and forward requested actions through `on_click`.

mod action_button;
mod app_footer;
mod app_header;
mod caption_well;
mod choice_pill;
mod elapsed_timer;
mod icon_button;
mod key_cap;
mod setting_row;
mod status_badge;
mod transport_button;
mod waveform;

pub use action_button::{ActionButton, ActionTone};
pub use app_footer::AppFooter;
pub use app_header::AppHeader;
pub use caption_well::CaptionWell;
pub use choice_pill::ChoicePill;
pub use elapsed_timer::ElapsedTimer;
pub use icon_button::IconButton;
pub use key_cap::KeyCap;
pub use setting_row::SettingRow;
pub use status_badge::{BadgeTone, StatusBadge};
pub use transport_button::{IconAction, TransportButton, TransportTone};
pub use waveform::Waveform;
