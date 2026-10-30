//! SpeechTT visual components (RenderOnce), built from design tokens.
//!
//! These are the pieces the mockups define that gpui-component does not
//! ship: the state badge, elapsed timer, waveform, caption well, mode cards,
//! round action buttons, transport fragments and setup-form controls. They read `crate::tokens` /
//! the active theme — no raw colors.
//!
//! Behavior lives in `controller.rs` / `speechtt-core`; components render what
//! they are given and forward requested actions through `on_click`.

mod action_button;
mod app_footer;
mod app_header;
mod caption_well;
mod card;
mod elapsed_timer;
mod icon_action;
mod icon_button;
mod key_cap;
mod round_button;
mod setting_row;
mod status_badge;
mod stop_control;
mod transcription_mode_badge;
mod transport_header;
mod waveform;

pub use action_button::{ActionButton, ActionTone};
pub use app_footer::AppFooter;
pub use app_header::AppHeader;
pub use caption_well::CaptionWell;
pub use card::Card;
pub use elapsed_timer::ElapsedTimer;
pub use icon_action::IconAction;
pub use icon_button::IconButton;
pub use key_cap::KeyCap;
pub use round_button::{ROUND_BUTTON_SIZE, RoundButton, RoundTone, ripple_frame};
pub use setting_row::SettingRow;
pub use status_badge::{BadgeTone, StatusBadge};
pub use stop_control::StopControl;
pub use transcription_mode_badge::{TranscriptionModeBadge, mode_label};
pub use transport_header::{Progress, TransportHeader};
pub use waveform::Waveform;
