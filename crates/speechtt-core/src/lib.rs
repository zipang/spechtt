// speechtt-core: audio capture, FFT waveform, session state machine,
// clipboard, hotkeys, save. Audio/FFT arrive in Phase 3; the session
// machine is Phase 2's behavioral backbone.
pub mod audio;
pub mod backend;
pub mod fft;
pub mod hotkeys;
pub mod output;
pub mod protocol;
pub mod session;
pub mod settings;
pub mod transcript;

// The UI composition root builds async channels for the backend and audio
// pumps. Re-exporting the channel crate keeps that dependency on the core
// side and avoids adding a second copy to `speechtt-ui`.
pub use async_channel;
// UI error tests construct the core error variants (which wrap
// `serde_json::Error`) without adding a second direct dependency.
pub use serde_json;
