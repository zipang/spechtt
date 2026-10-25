// speechtt-core: audio capture, FFT waveform, session state machine,
// clipboard, hotkeys, save. Audio/FFT arrive in Phase 3; the session
// machine is Phase 2's behavioral backbone.
pub mod backend;
pub mod protocol;
pub mod session;
pub mod transcript;
