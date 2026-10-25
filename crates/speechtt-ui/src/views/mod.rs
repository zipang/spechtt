//! Screen renderers, grouped by workflow stage.
//!
//! Each function is a thin composition layer over `ScreenShell`: it reads the
//! controller, builds reusable components and forwards user intent to the
//! `SpeechTtApp` command methods.

pub(crate) mod setup;
pub(crate) mod stopped;
pub(crate) mod transport;
