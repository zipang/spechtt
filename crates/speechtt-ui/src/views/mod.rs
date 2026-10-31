//! Screen renderers, one screen per file.
//!
//! Each file exports exactly one `render(app, cx)` and is named for the screen
//! it draws, so the file holding the visible screen is never a guess. Every
//! renderer is a thin composition layer over [`crate::layout::ScreenShell`]: it
//! reads the controller, builds reusable components and forwards user intent to
//! the `SpeechTtApp` command methods. Shared fragments live in
//! [`crate::components`], never in a sibling view file.

pub(crate) mod dialogs;
pub(crate) mod recording;
pub(crate) mod session_end;
pub(crate) mod session_setup;
pub(crate) mod session_transcription;
pub(crate) mod welcome;
