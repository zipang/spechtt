//! `speechtt-core` — the framework-free domain logic behind the SpeechTT
//! widget: the session state machine, audio capture, the FFT waveform, the
//! keyboard command vocabulary, transcript accumulation, clipboard/paste/file
//! output, the NDJSON worker bridge, the backend client and the settings store.
//! Nothing here depends on GPUI.
pub mod actions;
pub mod audio;
pub mod backend;
pub mod fft;
pub mod media;
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
