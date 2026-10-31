//! T9.2–T9.5 — integration wiring and the happy path.
//!
//! Every test uses injected fakes (`AudioSource`, `SttSessionSink`,
//! `SettingsStore`, clipboard/paste/opener, shortcut listener and clock), so
//! no microphone, display server, real clipboard, editor or network provider
//! is touched.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use speechtt_core::audio::{AudioError, AudioSource, PcmChunk};
use speechtt_core::backend::{BackendError, SttSessionSink};
use speechtt_core::fft::SpectrumAnalyzer;
use speechtt_core::hotkeys::{HotkeyError, SessionAction, Shortcut, ShortcutListener};
use speechtt_core::output::OutputError;
use speechtt_core::output::clipboard::ClipboardSink;
use speechtt_core::output::file::FileOpener;
use speechtt_core::output::paste::PasteSink;
use speechtt_core::protocol::{
    ModelCapabilityInfo, ProviderCapability, SecretString, SttCommand, SttEvent, TranscriptionMode,
};
use speechtt_core::settings::{Settings, SettingsError, SettingsStore, WindowLayout};
use speechtt_ui::app::{AppServices, SpeechTtApp};
use speechtt_ui::controller::{
    AppError, Clock, Dialog, SAMPLE_RATE, SessionController, SessionMode, SettingsChange, ViewState,
};

// ----- Fakes -----------------------------------------------------------

#[derive(Clone, Default)]
struct RecordingSink {
    commands: Arc<Mutex<Vec<SttCommand>>>,
}

impl RecordingSink {
    fn take(&self) -> Vec<SttCommand> {
        std::mem::take(&mut *self.commands.lock().expect("sink lock"))
    }

    fn commands(&self) -> Vec<SttCommand> {
        self.commands.lock().expect("sink lock").clone()
    }
}

impl SttSessionSink for RecordingSink {
    fn send(&mut self, cmd: SttCommand) -> Result<(), BackendError> {
        self.commands.lock().expect("sink lock").push(cmd);
        Ok(())
    }
}

struct ScriptedAudio {
    chunks: VecDeque<Result<Option<PcmChunk>, AudioError>>,
}

impl ScriptedAudio {
    fn with_chunks(chunks: impl IntoIterator<Item = Vec<i16>>) -> Self {
        Self {
            chunks: chunks
                .into_iter()
                .map(|samples| {
                    Ok(Some(PcmChunk {
                        samples,
                        sample_rate: SAMPLE_RATE,
                    }))
                })
                .collect(),
        }
    }
}

impl AudioSource for ScriptedAudio {
    fn next_chunk(&mut self) -> Result<Option<PcmChunk>, AudioError> {
        self.chunks.pop_front().unwrap_or(Ok(None))
    }
}

struct SilentAudio;

impl AudioSource for SilentAudio {
    fn next_chunk(&mut self) -> Result<Option<PcmChunk>, AudioError> {
        Ok(None)
    }
}

#[derive(Clone, Default)]
struct RecordingClipboard {
    copies: Arc<Mutex<Vec<String>>>,
}

impl ClipboardSink for RecordingClipboard {
    fn copy(&mut self, text: &str) -> Result<(), OutputError> {
        self.copies
            .lock()
            .expect("clipboard lock")
            .push(text.to_owned());
        Ok(())
    }
}

#[derive(Default)]
struct FailingClipboard;

impl ClipboardSink for FailingClipboard {
    fn copy(&mut self, _text: &str) -> Result<(), OutputError> {
        Err(OutputError::Clipboard("no display".into()))
    }
}

#[derive(Clone, Default)]
struct RecordingPaste {
    pastes: Arc<Mutex<usize>>,
}

impl PasteSink for RecordingPaste {
    fn paste(&mut self) -> Result<(), OutputError> {
        *self.pastes.lock().expect("paste lock") += 1;
        Ok(())
    }
}

#[derive(Clone, Default)]
struct RecordingOpener {
    opened: Arc<Mutex<Vec<PathBuf>>>,
}

impl FileOpener for RecordingOpener {
    fn open(&mut self, path: &Path) -> Result<(), OutputError> {
        self.opened
            .lock()
            .expect("opener lock")
            .push(path.to_path_buf());
        Ok(())
    }
}

#[derive(Clone, Default)]
struct RecordingListener {
    registrations: Arc<Mutex<Vec<(SessionAction, Shortcut)>>>,
    unregistered: Arc<AtomicUsize>,
}

impl ShortcutListener for RecordingListener {
    fn register(&mut self, shortcuts: &[(SessionAction, Shortcut)]) -> Result<(), HotkeyError> {
        *self.registrations.lock().expect("listener lock") = shortcuts.to_vec();
        Ok(())
    }

    fn unregister_all(&mut self) {
        self.unregistered.fetch_add(1, Ordering::SeqCst);
        self.registrations.lock().expect("listener lock").clear();
    }
}

#[derive(Clone)]
struct MemoryStore {
    settings: Arc<Mutex<Settings>>,
    path: PathBuf,
}

impl MemoryStore {
    fn new(settings: Settings) -> Self {
        Self {
            settings: Arc::new(Mutex::new(settings)),
            path: PathBuf::from("/tmp/speechtt-test-settings.json"),
        }
    }

    fn get(&self) -> Settings {
        self.settings.lock().expect("store lock").clone()
    }
}

impl SettingsStore for MemoryStore {
    fn load(&self) -> Result<Settings, SettingsError> {
        Ok(self.get())
    }

    fn save(&self, settings: &Settings) -> Result<(), SettingsError> {
        *self.settings.lock().expect("store lock") = settings.clone();
        Ok(())
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

struct ManualClock {
    now: Instant,
    system: SystemTime,
}

impl ManualClock {
    fn new() -> Self {
        Self {
            now: Instant::now(),
            system: UNIX_EPOCH + Duration::from_secs(1_793_109_785),
        }
    }

    fn advance(&mut self, duration: Duration) -> Instant {
        self.now += duration;
        self.now
    }
}

impl Clock for ManualClock {
    fn now(&self) -> Instant {
        self.now
    }

    fn system_now(&self) -> SystemTime {
        self.system
    }
}

/// Unique temp directory, removed on drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "speechtt-e2e-{}-{unique}-{label}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("temp dir");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

// ----- Helpers ---------------------------------------------------------

/// Catalogue the worker would send after a handshake.
fn catalogue() -> Vec<ProviderCapability> {
    vec![ProviderCapability {
        id: "vercel-ai-gateway".to_string(),
        label: "Vercel AI Gateway".to_string(),
        available: true,
        key_env_vars: vec!["AI_GATEWAY_API_KEY".to_string()],
        models: vec![
            ModelCapabilityInfo {
                id: "openai/gpt-4o-transcribe".to_string(),
                label: "GPT-4o Transcribe".to_string(),
                mode: TranscriptionMode::Segmented,
                description: None,
                pricing: std::collections::BTreeMap::new(),
            },
            ModelCapabilityInfo {
                id: "openai/gpt-realtime-whisper".to_string(),
                label: "gpt-realtime-whisper".to_string(),
                mode: TranscriptionMode::Streaming,
                description: None,
                pricing: std::collections::BTreeMap::new(),
            },
        ],
    }]
}

fn install_catalogue(controller: &mut SessionController) {
    controller.set_capabilities(catalogue());
}

/// A Live-mode controller past the form and `Ready`.
fn complete_draft() -> SessionController {
    let mut controller = SessionController::new();
    install_catalogue(&mut controller);
    assert!(!controller.choose_mode(SessionMode::Live));
    controller.cycle_provider();
    controller.cycle_model();
    assert!(controller.create_session());
    controller
}

fn duplex_controller() -> (SessionController, RecordingSink) {
    let controller = complete_draft();
    let sink = RecordingSink::default();
    let mut controller = controller;
    controller.set_sink(Box::new(sink.clone()));
    (controller, sink)
}

fn settings_with_key() -> Settings {
    let mut settings = Settings {
        provider: "vercel-ai-gateway".into(),
        model: "openai/gpt-realtime-whisper".into(),
        auto_paste: false,
        ..Settings::default()
    };
    settings.set_api_key("vercel-ai-gateway", "sk-e2e-secret");
    settings
}

/// Mixed case and digits: a single changed character is a different key.
const MIXED_CASE_KEY: &str = "vck_AbCdEfGhIjKlMnOpQrStUvWxYzH0123456789";

fn settings_with_mixed_case_key() -> Settings {
    let mut settings = Settings {
        provider: "vercel-ai-gateway".into(),
        model: "openai/gpt-realtime-whisper".into(),
        auto_paste: false,
        ..Settings::default()
    };
    settings.set_api_key("vercel-ai-gateway", MIXED_CASE_KEY);
    settings
}

fn settings_with_auto_paste() -> Settings {
    Settings {
        auto_paste: true,
        ..settings_with_key()
    }
}

fn finish_with_transcript(controller: &mut SessionController, sink: &RecordingSink) {
    assert!(controller.start_recording());
    controller.handle_stt_event(SttEvent::Partial {
        session_id: "take-1".into(),
        text: "audio".into(),
    });
    controller.handle_stt_event(SttEvent::Delta {
        session_id: "take-1".into(),
        delta: " capture".into(),
    });
    assert!(controller.pause_recording());
    assert!(controller.finish_take());
    let _ = sink.take();
}

// ----- T9.2: controller integration -----------------------------------

#[test]
fn audio_frames_reach_the_backend_only_while_recording() {
    let (mut controller, sink) = duplex_controller();
    assert!(controller.start_recording());
    controller.push_audio_chunk(&[1, 2], Arc::<[f32]>::from([0.4]));
    assert!(controller.pause_recording());
    controller.push_audio_chunk(&[3, 4], Arc::<[f32]>::from([0.9]));
    assert!(controller.resume_recording());
    controller.push_audio_chunk(&[5, 6], Arc::<[f32]>::from([0.2]));
    assert!(controller.finish_take());

    let commands = sink.take();
    let audio: Vec<_> = commands
        .iter()
        .filter(|command| matches!(command, SttCommand::Audio { .. }))
        .collect();
    assert_eq!(audio.len(), 2, "{commands:?}");
    assert_eq!(audio[0], &SttCommand::audio_from_pcm16(&[1, 2]));
    assert_eq!(audio[1], &SttCommand::audio_from_pcm16(&[5, 6]));
}

#[test]
fn partial_delta_and_committed_text_survive_finish_and_stopped_refines_it() {
    let (mut controller, _sink) = duplex_controller();
    assert!(controller.start_recording());

    controller.handle_stt_event(SttEvent::Partial {
        session_id: "take-1".into(),
        text: "audio".into(),
    });
    controller.handle_stt_event(SttEvent::Delta {
        session_id: "take-1".into(),
        delta: " capture".into(),
    });
    assert_eq!(controller.transcript_live(), "audio capture");

    controller.handle_stt_event(SttEvent::Committed {
        session_id: "take-1".into(),
        text: "audio capture layer.".into(),
    });
    controller.handle_stt_event(SttEvent::Partial {
        session_id: "take-1".into(),
        text: "next words".into(),
    });
    assert!(controller.finish_take());

    let take = controller.finished_take().expect("finished take");
    assert_eq!(take.committed().to_string(), "audio capture layer.");
    assert_eq!(take.live().to_string(), "next words");

    controller.handle_stt_event(SttEvent::Stopped {
        session_id: "take-1".into(),
        full_text: "audio capture layer. next words.".into(),
    });
    let take = controller.finished_take().expect("refined take");
    assert_eq!(
        take.committed().to_string(),
        "audio capture layer. next words."
    );
    assert_eq!(take.live().to_string(), "");
}

#[test]
fn stale_session_events_are_ignored() {
    let (mut controller, _sink) = duplex_controller();
    assert!(controller.start_recording());
    controller.handle_stt_event(SttEvent::Partial {
        session_id: "take-1".into(),
        text: "first".into(),
    });
    assert!(controller.pause_recording());
    assert!(controller.finish_take());
    controller.new_take();

    assert!(!controller.choose_mode(SessionMode::Live));
    controller.cycle_provider();
    controller.cycle_model();
    assert!(controller.create_session());
    assert!(controller.start_recording());
    assert_eq!(controller.session_label(), "take-2");

    controller.handle_stt_event(SttEvent::Partial {
        session_id: "take-1".into(),
        text: "stale".into(),
    });
    controller.handle_stt_event(SttEvent::Stopped {
        session_id: "take-1".into(),
        full_text: "stale flush".into(),
    });
    assert_eq!(controller.transcript_live(), "");
    assert_eq!(controller.transcript_committed(), "");
}

#[test]
fn hotkey_actions_match_the_button_guards() {
    let (mut controller, _sink) = duplex_controller();

    assert!(controller.apply_action(SessionAction::StartRecording));
    assert_eq!(
        controller.session_state(),
        speechtt_core::session::SessionState::Recording
    );
    // Stop is inert while recording, exactly like the disabled button.
    assert!(!controller.apply_action(SessionAction::StopRecording));
    assert!(controller.apply_action(SessionAction::PauseRecording));
    assert!(controller.apply_action(SessionAction::StopRecording));
    assert!(controller.apply_action(SessionAction::NewTake));
    assert!(!controller.apply_action(SessionAction::NewTake));

    assert!(controller.apply_action(SessionAction::ToggleHelp));
    assert_eq!(controller.active_dialog(), Some(Dialog::Help));
    controller.close_dialog();
    assert!(controller.apply_action(SessionAction::ToggleSettings));
    assert_eq!(controller.active_dialog(), Some(Dialog::Settings));
}

#[test]
fn clock_drives_elapsed_and_resets_for_each_take() {
    let (mut controller, _sink) = duplex_controller();
    let mut clock = ManualClock::new();

    let start = clock.now();
    assert!(controller.start_recording_at(start));
    assert!(controller.refresh_elapsed(clock.advance(Duration::from_millis(3_400))));
    assert_eq!(controller.elapsed_seconds(), 3);

    let paused = clock.advance(Duration::from_millis(1_600));
    assert!(controller.pause_recording_at(paused));
    assert_eq!(controller.elapsed_seconds(), 5);
    // While paused the clock keeps moving, but the read-out does not.
    assert!(!controller.refresh_elapsed(clock.advance(Duration::from_secs(10))));
    assert_eq!(controller.elapsed_seconds(), 5);

    let resumed = clock.advance(Duration::from_secs(10));
    assert!(controller.resume_recording_at(resumed));
    assert!(controller.refresh_elapsed(clock.advance(Duration::from_millis(2_200))));
    assert_eq!(controller.elapsed_seconds(), 7);

    assert!(controller.finish_take());
    controller.new_take();
    assert!(!controller.choose_mode(SessionMode::Live));
    controller.cycle_provider();
    controller.cycle_model();
    assert!(controller.create_session());

    assert!(controller.start_recording_at(clock.advance(Duration::from_secs(30))));
    assert_eq!(controller.elapsed_seconds(), 0);
    assert!(controller.refresh_elapsed(clock.advance(Duration::from_secs(1))));
    assert_eq!(controller.elapsed_seconds(), 1);
}

// ----- T9.3: output flow ----------------------------------------------

#[test]
fn copy_sends_the_exact_transcript_to_the_clipboard() {
    let (mut controller, sink) = duplex_controller();
    finish_with_transcript(&mut controller, &sink);

    let mut clipboard = RecordingClipboard::default();
    controller
        .copy_finished_transcript(&mut clipboard)
        .expect("copy succeeds");

    assert_eq!(
        clipboard.copies.lock().expect("clipboard lock").clone(),
        vec!["audio capture".to_owned()]
    );
    assert_eq!(controller.output_status(), Some("Copied"));
}

#[test]
fn save_writes_markdown_and_keeps_the_transcript() {
    let (mut controller, sink) = duplex_controller();
    finish_with_transcript(&mut controller, &sink);
    let directory = TempDir::new("save");
    let target = directory.path().join("meeting notes.md");

    let saved = controller
        .save_finished_transcript_to(&target)
        .expect("save succeeds");

    assert_eq!(saved.path, target);
    assert_eq!(
        std::fs::read_to_string(&saved.path).expect("saved file"),
        "# meeting notes\n\naudio capture\n"
    );
    assert_eq!(controller.written_path(), Some(saved.path.as_path()));
    assert_eq!(controller.transcript_full_text(), "audio capture");
    assert_eq!(controller.output_status(), Some("Saved"));
}

#[test]
fn a_save_failure_is_typed_and_keeps_the_transcript() {
    let (mut controller, sink) = duplex_controller();
    finish_with_transcript(&mut controller, &sink);

    let directory = TempDir::new("save-failure");
    let blocked = directory.path().join("blocked");
    std::fs::write(&blocked, b"not a directory").expect("blocking file");
    // The parent of the target is a regular file: the save must fail cleanly.
    let target = blocked.join("notes.md");

    let error = controller
        .save_finished_transcript_to(&target)
        .expect_err("save must fail");

    assert!(matches!(error, AppError::SaveFailed(_)));
    assert_eq!(controller.app_error(), Some(&error));
    assert_eq!(controller.transcript_full_text(), "audio capture");
    assert!(controller.written_path().is_none());
}

#[test]
fn a_file_take_is_saved_under_its_source_stem() {
    let directory = TempDir::new("file-take");
    let source = directory.path().join("standup.mp4");
    std::fs::write(&source, b"RIFF0000WAVE").expect("source fixture");
    let target = directory.path().join("standup.md");
    let mut controller = SessionController::new();
    install_catalogue(&mut controller);
    controller.choose_mode(SessionMode::File);
    controller.cycle_provider();
    controller.cycle_model();
    assert!(controller.create_session());
    controller
        .start_file_session(&source)
        .expect("file take starts");
    controller.handle_stt_event(SttEvent::Committed {
        session_id: "take-1".into(),
        text: "audio capture".into(),
    });
    assert!(controller.finish_take());

    assert_eq!(controller.suggested_save_name().as_deref(), Some("standup"));
    let saved = controller
        .save_finished_transcript_to(&target)
        .expect("file take saves");

    assert_eq!(saved.path, target);
    assert_eq!(
        std::fs::read_to_string(&saved.path).expect("saved file"),
        "# standup\n\naudio capture\n"
    );
    assert_eq!(controller.written_path(), Some(saved.path.as_path()));

    let mut opener = RecordingOpener::default();
    controller
        .open_saved_file(&mut opener)
        .expect("written file opens");
    assert_eq!(
        opener.opened.lock().expect("opener lock").clone(),
        vec![saved.path]
    );
}

#[test]
fn open_calls_the_opener_with_the_written_path() {
    let (mut controller, sink) = duplex_controller();
    finish_with_transcript(&mut controller, &sink);
    let directory = TempDir::new("open");
    let target = directory.path().join("meeting notes.md");
    let saved = controller
        .save_finished_transcript_to(&target)
        .expect("save succeeds");

    let mut opener = RecordingOpener::default();
    controller
        .open_saved_file(&mut opener)
        .expect("open succeeds");

    assert_eq!(
        opener.opened.lock().expect("opener lock").clone(),
        vec![saved.path]
    );
    assert_eq!(controller.output_status(), Some("Opened"));
}

#[test]
fn auto_paste_runs_only_when_enabled() {
    // Disabled by default (no settings applied).
    let (mut controller, sink) = duplex_controller();
    finish_with_transcript(&mut controller, &sink);
    let mut clipboard = RecordingClipboard::default();
    let mut paste = RecordingPaste::default();
    assert!(
        controller
            .auto_paste_after_finish(&mut clipboard, &mut paste)
            .is_none()
    );
    assert_eq!(*paste.pastes.lock().expect("paste lock"), 0);

    // Enabled through persisted settings with a model and key.
    let mut enabled = complete_draft();
    let sink = RecordingSink::default();
    enabled.set_sink(Box::new(sink.clone()));
    enabled.apply_settings(settings_with_auto_paste());
    finish_with_transcript(&mut enabled, &sink);

    let mut clipboard = RecordingClipboard::default();
    let mut paste = RecordingPaste::default();
    let result = enabled
        .auto_paste_after_finish(&mut clipboard, &mut paste)
        .expect("auto-paste runs");
    assert!(result.is_ok());
    assert_eq!(
        clipboard.copies.lock().expect("clipboard lock").clone(),
        vec!["audio capture".to_owned()]
    );
    assert_eq!(*paste.pastes.lock().expect("paste lock"), 1);
}

#[test]
fn clipboard_failure_is_typed_and_does_not_paste() {
    let (mut controller, sink) = duplex_controller();
    finish_with_transcript(&mut controller, &sink);
    controller.apply_settings(settings_with_auto_paste());

    let mut clipboard = FailingClipboard;
    let mut paste = RecordingPaste::default();
    let result = controller
        .auto_paste_after_finish(&mut clipboard, &mut paste)
        .expect("auto-paste runs");

    assert_eq!(result, Err(AppError::ClipboardFailed));
    assert_eq!(*paste.pastes.lock().expect("paste lock"), 0);
}

// ----- T9.4: settings and capabilities --------------------------------

#[test]
fn capabilities_populate_options_and_connection_state() {
    let mut controller = SessionController::new();
    controller.set_capabilities(vec![ProviderCapability {
        id: "vercel-ai-gateway".into(),
        label: "Vercel AI Gateway".into(),
        available: false,
        key_env_vars: Vec::new(),
        models: vec![speechtt_core::protocol::ModelCapabilityInfo {
            id: "openai/gpt-realtime-whisper".into(),
            label: "GPT Realtime Whisper".into(),
            mode: speechtt_core::protocol::TranscriptionMode::Streaming,
            description: None,
            pricing: std::collections::BTreeMap::new(),
        }],
    }]);

    controller.open_settings();
    assert!(controller.set_settings_provider("vercel-ai-gateway"));
    assert!(controller.set_settings_model("openai/gpt-realtime-whisper"));
    assert_eq!(
        controller.settings_provider_label(),
        Some("Vercel AI Gateway")
    );
    assert_eq!(
        controller.settings_model_label(),
        Some("GPT Realtime Whisper")
    );
    assert_eq!(
        controller.provider_available("vercel-ai-gateway"),
        Some(false)
    );

    controller.set_settings_api_key("sk-capability-test");
    assert!(
        !controller.settings_connected(),
        "an unavailable provider is not connected even with a key"
    );
    assert!(
        controller.settings_draft_valid(),
        "the key can still be saved so the next handshake can connect it"
    );

    // A second handshake reporting the provider as available flips the row to
    // CONNECTED without re-typing the key.
    controller.set_capabilities(vec![ProviderCapability {
        id: "vercel-ai-gateway".into(),
        label: "Vercel AI Gateway".into(),
        available: true,
        key_env_vars: vec!["AI_GATEWAY_API_KEY".into()],
        models: vec![speechtt_core::protocol::ModelCapabilityInfo {
            id: "openai/gpt-realtime-whisper".into(),
            label: "GPT Realtime Whisper".into(),
            mode: speechtt_core::protocol::TranscriptionMode::Streaming,
            description: None,
            pricing: std::collections::BTreeMap::new(),
        }],
    }]);
    assert!(controller.settings_connected());
    assert!(controller.settings_draft_valid());
}

#[test]
fn choosing_live_prefills_the_persisted_default_provider_and_model() {
    let mut controller = SessionController::new();
    install_catalogue(&mut controller);
    controller.apply_settings(settings_with_key());

    assert!(
        controller.choose_mode(SessionMode::Live),
        "the settings already resolve, so Live lands on Ready"
    );

    assert_eq!(
        controller.session_state(),
        speechtt_core::session::SessionState::Ready
    );
    assert_eq!(controller.session_missing(), Vec::new());
    assert_eq!(controller.mode(), SessionMode::Live);

    let sink = RecordingSink::default();
    controller.set_sink(Box::new(sink.clone()));
    assert!(controller.start_recording());
    match sink
        .commands()
        .into_iter()
        .find(|command| matches!(command, SttCommand::Start { .. }))
        .expect("start command")
    {
        SttCommand::Start {
            provider,
            model,
            api_key,
            ..
        } => {
            assert_eq!(provider, "vercel-ai-gateway");
            assert_eq!(model, "openai/gpt-realtime-whisper");
            assert_eq!(api_key, Some(SecretString::new("sk-e2e-secret")));
        }
        other => panic!("expected start, got {other:?}"),
    }
}

#[test]
fn choosing_live_opens_the_form_without_a_configured_model() {
    let mut controller = SessionController::new();
    // A provider-only/default settings file has no model to pre-fill.
    let mut settings = Settings {
        provider: "vercel-ai-gateway".into(),
        ..Settings::default()
    };
    settings.set_api_key("vercel-ai-gateway", "sk-provider-only");
    controller.apply_settings(settings);

    assert!(!controller.choose_mode(SessionMode::Live));

    assert!(!controller.has_provider());
    assert!(!controller.has_model());
    assert_eq!(controller.provider_label(), None);
    assert_eq!(controller.model_label(), None);
}

#[test]
fn new_session_form_cycles_the_capability_catalogue() {
    let mut controller = SessionController::new();
    controller.set_capabilities(vec![
        ProviderCapability {
            id: "custom".into(),
            label: "Custom STT".into(),
            available: true,
            key_env_vars: Vec::new(),
            models: vec![
                speechtt_core::protocol::ModelCapabilityInfo {
                    id: "custom-large".into(),
                    label: "Custom Large".into(),
                    mode: speechtt_core::protocol::TranscriptionMode::Streaming,
                    description: None,
                    pricing: std::collections::BTreeMap::new(),
                },
                speechtt_core::protocol::ModelCapabilityInfo {
                    id: "custom-small".into(),
                    label: "Custom Small".into(),
                    mode: speechtt_core::protocol::TranscriptionMode::Segmented,
                    description: None,
                    pricing: std::collections::BTreeMap::new(),
                },
            ],
        },
        ProviderCapability {
            id: "other".into(),
            label: "Other STT".into(),
            available: true,
            key_env_vars: Vec::new(),
            models: vec![speechtt_core::protocol::ModelCapabilityInfo {
                id: "other-one".into(),
                label: "Other One".into(),
                mode: speechtt_core::protocol::TranscriptionMode::Streaming,
                description: None,
                pricing: std::collections::BTreeMap::new(),
            }],
        },
    ]);

    controller.choose_mode(SessionMode::Live);
    controller.cycle_provider();
    assert_eq!(controller.provider_label(), Some("Custom STT"));
    controller.cycle_model();
    assert_eq!(controller.model_label(), Some("Custom Large"));

    // A second model cycle moves inside the same capability provider.
    controller.cycle_model();
    assert_eq!(controller.model_label(), Some("Custom Small"));

    assert!(controller.create_session());
    let sink = RecordingSink::default();
    controller.set_sink(Box::new(sink.clone()));
    assert!(controller.start_recording());

    match sink
        .commands()
        .into_iter()
        .find(|command| matches!(command, SttCommand::Start { .. }))
        .expect("start command")
    {
        SttCommand::Start {
            provider, model, ..
        } => {
            assert_eq!(provider, "custom");
            assert_eq!(model, "custom-small");
        }
        other => panic!("expected start, got {other:?}"),
    }
}

#[test]
fn start_uses_the_session_choice_and_the_settings_key_for_that_provider() {
    // No persisted model: the chooser opens the form and the user picks one.
    let mut settings = Settings {
        provider: "vercel-ai-gateway".into(),
        ..Settings::default()
    };
    settings.set_api_key("vercel-ai-gateway", "sk-vercel");
    let mut controller = SessionController::new();
    install_catalogue(&mut controller);
    controller.apply_settings(settings);

    assert!(!controller.choose_mode(SessionMode::Live));
    controller.cycle_provider();
    controller.cycle_model();
    assert_eq!(
        controller.model_label(),
        Some("GPT-4o Transcribe"),
        "cycle_model picks the first model in label order"
    );
    assert!(controller.create_session());

    let sink = RecordingSink::default();
    controller.set_sink(Box::new(sink.clone()));
    assert!(controller.start_recording());
    match sink
        .commands()
        .into_iter()
        .find(|command| matches!(command, SttCommand::Start { .. }))
        .expect("start command")
    {
        SttCommand::Start {
            provider,
            model,
            api_key,
            ..
        } => {
            assert_eq!(provider, "vercel-ai-gateway");
            assert_eq!(model, "openai/gpt-4o-transcribe");
            assert_eq!(api_key, Some(SecretString::new("sk-vercel")));
        }
        other => panic!("expected start, got {other:?}"),
    }
}

#[test]
fn settings_save_round_trips_to_the_store_and_start_carries_the_key() {
    let store = MemoryStore::new(Settings::default());
    let mut controller = SessionController::new();
    install_catalogue(&mut controller);
    controller.open_settings();
    assert!(controller.set_settings_provider("vercel-ai-gateway"));
    assert!(controller.set_settings_model("openai/gpt-realtime-whisper"));
    controller.set_settings_api_key("sk-round-trip");

    let change = controller.save_settings().expect("valid draft");
    let mut settings = store.get();
    settings.provider = change.provider.clone();
    settings.model = change.model.clone();
    if let Some(key) = &change.api_key {
        settings.set_api_key(&change.provider, key.clone());
    }
    store.save(&settings).expect("save to store");
    assert_eq!(
        store.get().api_key("vercel-ai-gateway"),
        Some("sk-round-trip")
    );

    // Start uses the persisted provider/model/key that the chooser pre-fills.
    controller.apply_settings(store.get());
    assert!(controller.choose_mode(SessionMode::Live));
    assert_eq!(
        controller.session_state(),
        speechtt_core::session::SessionState::Ready
    );
    let sink = RecordingSink::default();
    controller.set_sink(Box::new(sink.clone()));
    assert!(controller.start_recording());

    let start = sink
        .commands()
        .into_iter()
        .find(|command| matches!(command, SttCommand::Start { .. }))
        .expect("start command");
    match start {
        SttCommand::Start {
            provider,
            model,
            api_key,
            ..
        } => {
            assert_eq!(provider, "vercel-ai-gateway");
            assert_eq!(model, "openai/gpt-realtime-whisper");
            assert_eq!(api_key, Some(SecretString::new("sk-round-trip")));
        }
        other => panic!("expected start, got {other:?}"),
    }
}

#[test]
fn a_missing_key_blocks_recording_with_a_settings_shortcut() {
    let mut settings = Settings {
        provider: "vercel-ai-gateway".into(),
        model: "openai/gpt-realtime-whisper".into(),
        ..Settings::default()
    };
    settings.remove_api_key("vercel");
    assert!(settings.api_key("vercel").is_none());

    let mut controller = complete_draft();
    controller.apply_settings(settings);
    let sink = RecordingSink::default();
    controller.set_sink(Box::new(sink.clone()));

    assert!(!controller.start_recording());
    assert_eq!(
        controller.app_error(),
        Some(&AppError::MissingApiKey {
            provider: "vercel-ai-gateway".into()
        })
    );
    assert_eq!(
        controller.error_action(),
        Some(speechtt_ui::controller::ErrorAction::OpenSettings)
    );
    assert!(
        sink.take()
            .iter()
            .all(|command| !matches!(command, SttCommand::Start { .. })),
        "no start command is sent without a key"
    );
}

#[test]
fn retry_reissues_start_with_the_same_session_id() {
    let (mut controller, sink) = duplex_controller();
    assert!(controller.start_recording());
    let _ = sink.take();

    controller.report_error(AppError::NetworkLost);
    assert!(controller.retry_start());

    let commands = sink.take();
    assert_eq!(commands.len(), 1);
    match &commands[0] {
        SttCommand::Start { session_id, .. } => assert_eq!(session_id, "take-1"),
        other => panic!("expected start, got {other:?}"),
    }
    assert!(controller.app_error().is_none());
}

// ----- T9.5: end-to-end app wiring ------------------------------------

mod common;

use gpui_kit::AppContext as _;
use gpui_kit::test::TestWindowExt;

struct FakeApp {
    audio: Box<dyn AudioSource>,
    sink: RecordingSink,
    events: speechtt_core::backend::SttEventReceiver,
    store: MemoryStore,
    clipboard: RecordingClipboard,
    paste: RecordingPaste,
    opener: RecordingOpener,
    listener: RecordingListener,
    clock: ManualClock,
}

impl Default for FakeApp {
    fn default() -> Self {
        let (_sender, events) = speechtt_core::async_channel::unbounded();
        Self {
            audio: Box::new(SilentAudio),
            sink: RecordingSink::default(),
            events,
            store: MemoryStore::new(Settings::default()),
            clipboard: RecordingClipboard::default(),
            paste: RecordingPaste::default(),
            opener: RecordingOpener::default(),
            listener: RecordingListener::default(),
            clock: ManualClock::new(),
        }
    }
}

impl FakeApp {
    fn services(self) -> AppServices {
        AppServices {
            audio: self.audio,
            analyzer: SpectrumAnalyzer::new(SAMPLE_RATE),
            sink: Box::new(self.sink),
            events: self.events,
            settings: Box::new(self.store),
            clipboard: Box::new(self.clipboard),
            paste: Box::new(self.paste),
            opener: Box::new(self.opener),
            listener: Box::new(self.listener),
            clock: Box::new(self.clock),
        }
    }
}

#[gpui_kit::test]
fn the_app_constructs_from_fakes_and_persists_settings(cx: &mut gpui_kit::TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        speechtt_ui::theme::apply(cx);
    });

    let store = MemoryStore::new(Settings::default());
    let store_handle = store.clone();
    let listener = RecordingListener::default();
    let listener_handle = listener.clone();
    let fake = FakeApp {
        store,
        listener,
        ..FakeApp::default()
    };

    let (window, view) = common::open_window(cx, None, |window, cx| {
        cx.new(|cx| SpeechTtApp::with_services(fake.services(), window, cx))
    });

    cx.update(|cx| {
        view.update(cx, |app, cx| app.apply_capabilities(catalogue(), cx));
    });

    assert_eq!(cx.update(|cx| view.read(cx).view_state()), ViewState::Mode);
    assert!(
        !listener_handle
            .registrations
            .lock()
            .expect("listener lock")
            .is_empty(),
        "default bindings are registered"
    );

    cx.update_window(window.into(), |_, window, cx| {
        view.update(cx, |app, cx| app.open_settings(window, cx));
        window.render_frame(cx);
        view.update(cx, |app, cx| {
            app.cycle_settings_provider(cx);
            app.cycle_settings_model(cx);
            app.set_settings_api_key_value("sk-app-round-trip", window, cx);
            assert!(app.save_settings(cx));
        });
        window.render_frame(cx);
    })
    .unwrap();

    let saved = store_handle.get();
    assert_eq!(saved.provider, "vercel-ai-gateway");
    assert!(!saved.model.is_empty());
    assert_eq!(saved.api_key("vercel"), Some("sk-app-round-trip"));
    assert_eq!(cx.update(|cx| view.read(cx).active_dialog()), None);

    // Re-opening Settings shows the persisted provider/model/key status
    // instead of resetting to "Choose provider" / NOT CONNECTED.
    cx.update_window(window.into(), |_, window, cx| {
        view.update(cx, |app, cx| app.open_settings(window, cx));
        window.render_frame(cx);
        let app = view.read(cx);
        let draft = app.settings_draft();
        assert_eq!(draft.provider.as_deref(), Some("vercel-ai-gateway"));
        assert_eq!(draft.model.as_deref(), Some(saved.model.as_str()));
        assert!(draft.api_key_unchanged, "the stored key is not re-typed");
        assert!(app.settings_connected(), "the stored key shows CONNECTED");
    })
    .unwrap();
}

#[gpui_kit::test]
fn the_shared_test_window_does_not_write_the_real_settings_file(cx: &mut gpui_kit::TestAppContext) {
    let real_path = speechtt_core::settings::JsonFileStore::default_path();
    let before = std::fs::read(&real_path).ok();

    let (window, view) = common::open_speechtt_window(cx);
    cx.update(|cx| {
        view.update(cx, |app, cx| app.apply_capabilities(catalogue(), cx));
    });
    cx.update_window(window.into(), |_, window, cx| {
        view.update(cx, |app, cx| app.open_settings(window, cx));
        view.update(cx, |app, cx| {
            app.cycle_settings_provider(cx);
            app.cycle_settings_model(cx);
            app.set_settings_api_key_value("sk-hermetic", window, cx);
            assert!(app.save_settings(cx));
        });
        window.render_frame(cx);
    })
    .unwrap();

    assert_eq!(
        std::fs::read(&real_path).ok(),
        before,
        "the shared test window must never touch the real settings file"
    );
}

#[gpui_kit::test]
fn the_app_event_pump_updates_the_caption_and_copy_uses_the_clipboard(
    cx: &mut gpui_kit::TestAppContext,
) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        speechtt_ui::theme::apply(cx);
    });

    let sink = RecordingSink::default();
    let sink_handle = sink.clone();
    let clipboard = RecordingClipboard::default();
    let clipboard_handle = clipboard.clone();
    let (events_sender, events) = speechtt_core::async_channel::unbounded();
    let fake = FakeApp {
        audio: Box::new(ScriptedAudio::with_chunks([vec![1_i16; 640]])),
        sink,
        events,
        store: MemoryStore::new(settings_with_key()),
        clipboard,
        ..FakeApp::default()
    };

    let (window, view) = common::open_window(cx, None, |window, cx| {
        cx.new(|cx| SpeechTtApp::with_services(fake.services(), window, cx))
    });

    cx.update(|cx| {
        view.update(cx, |app, cx| app.apply_capabilities(catalogue(), cx));
    });

    cx.update_window(window.into(), |_, _window, cx| {
        view.update(cx, |app, cx| {
            // Persisted settings resolve, so Live lands on Ready directly.
            app.choose_mode(SessionMode::Live, cx);
            assert_eq!(app.view_state(), ViewState::Ready);
        });
    })
    .unwrap();
    view.update(cx, |app, cx| app.start_recording(cx));

    events_sender
        .send_blocking(Ok(SttEvent::Partial {
            session_id: "take-1".into(),
            text: "hello".into(),
        }))
        .expect("event sent");
    events_sender
        .send_blocking(Ok(SttEvent::Delta {
            session_id: "take-1".into(),
            delta: " world".into(),
        }))
        .expect("event sent");

    cx.run_until_parked();
    assert_eq!(
        cx.update(|cx| view.read(cx).transcript_live().to_owned()),
        "hello world"
    );

    view.update(cx, |app, cx| {
        app.pause_recording(cx);
        app.finish_take(cx);
        assert!(app.copy_transcript(cx));
    });

    assert_eq!(
        clipboard_handle
            .copies
            .lock()
            .expect("clipboard lock")
            .clone(),
        vec!["hello world".to_owned()]
    );
    assert_eq!(
        cx.update(|cx| view.read(cx).view_state()),
        ViewState::Stopped
    );
    // The composed Start came from persisted settings, not the form labels.
    let commands = sink_handle.commands();
    assert!(commands.iter().any(|command| matches!(
        command,
        SttCommand::Start { provider, .. } if provider == "vercel-ai-gateway"
    )));
}

// ----- T10.2–T10.4: key integrity and stored-key reveal UX -------------

#[gpui_kit::test]
fn a_mixed_case_settings_key_is_saved_byte_for_byte(cx: &mut gpui_kit::TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        speechtt_ui::theme::apply(cx);
    });

    let store = MemoryStore::new(Settings::default());
    let store_handle = store.clone();
    let fake = FakeApp {
        store,
        ..FakeApp::default()
    };
    let (window, view) = common::open_window(cx, None, |window, cx| {
        cx.new(|cx| SpeechTtApp::with_services(fake.services(), window, cx))
    });

    cx.update(|cx| {
        view.update(cx, |app, cx| app.apply_capabilities(catalogue(), cx));
    });

    cx.update_window(window.into(), |_, window, cx| {
        view.update(cx, |app, cx| {
            app.open_settings(window, cx);
            app.cycle_settings_provider(cx);
            app.cycle_settings_model(cx);
            app.set_settings_api_key_value(MIXED_CASE_KEY, window, cx);

            assert!(
                app.settings_api_key_value(cx) == MIXED_CASE_KEY,
                "the input value was altered"
            );

            // Masking is presentation only: toggling Reveal must not edit the
            // draft or the input value.
            app.toggle_settings_key_reveal(window, cx);
            assert!(
                app.settings_api_key_value(cx) == MIXED_CASE_KEY,
                "revealing the draft altered the input value"
            );
            app.toggle_settings_key_reveal(window, cx);
            assert!(
                app.settings_api_key_value(cx) == MIXED_CASE_KEY,
                "masking the draft altered the input value"
            );

            assert!(app.save_settings(cx));
        });
        window.render_frame(cx);
    })
    .unwrap();

    let saved = store_handle.get();
    assert!(
        saved.api_key("vercel") == Some(MIXED_CASE_KEY),
        "the store received a different key"
    );
    assert!(
        saved
            .api_key("vercel")
            .is_some_and(|stored| stored.as_bytes() == MIXED_CASE_KEY.as_bytes()),
        "case and every other byte must reach the store unchanged"
    );
}

#[gpui_kit::test]
fn a_pasted_mixed_case_key_reaches_the_store_unchanged(cx: &mut gpui_kit::TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        speechtt_ui::theme::apply(cx);
    });

    let store = MemoryStore::new(Settings::default());
    let store_handle = store.clone();
    let fake = FakeApp {
        store,
        ..FakeApp::default()
    };
    let (window, view) = common::open_window(cx, None, |window, cx| {
        cx.new(|cx| SpeechTtApp::with_services(fake.services(), window, cx))
    });

    cx.update(|cx| {
        view.update(cx, |app, cx| app.apply_capabilities(catalogue(), cx));
    });

    cx.update_window(window.into(), |_, window, cx| {
        view.update(cx, |app, cx| {
            app.open_settings(window, cx);
            app.cycle_settings_provider(cx);
            app.cycle_settings_model(cx);
        });
        // A paste is a whole-value input update, the same path
        // `set_settings_api_key_value` drives through `InputState::set_value`.
        view.update(cx, |app, cx| {
            app.set_settings_api_key_value(MIXED_CASE_KEY, window, cx);
            assert!(
                app.settings_api_key_value(cx) == MIXED_CASE_KEY,
                "the pasted value was altered"
            );
            assert!(app.save_settings(cx));
        });
        window.render_frame(cx);
    })
    .unwrap();

    assert!(
        store_handle.get().api_key("vercel") == Some(MIXED_CASE_KEY),
        "the store received a different key"
    );
}

#[gpui_kit::test]
fn a_stored_key_shows_only_its_mask_and_reveal_stays_disabled(cx: &mut gpui_kit::TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        speechtt_ui::theme::apply(cx);
    });

    let fake = FakeApp {
        store: MemoryStore::new(settings_with_mixed_case_key()),
        ..FakeApp::default()
    };
    let (window, view) = common::open_window(cx, None, |window, cx| {
        cx.new(|cx| SpeechTtApp::with_services(fake.services(), window, cx))
    });

    cx.update(|cx| {
        view.update(cx, |app, cx| app.apply_capabilities(catalogue(), cx));
    });

    cx.update_window(window.into(), |_, window, cx| {
        view.update(cx, |app, cx| app.open_settings(window, cx));
        window.render_frame(cx);

        assert_eq!(
            view.read(cx).settings_stored_key_label().as_deref(),
            Some("Stored key: vck…89")
        );
        assert!(!view.read(cx).settings_key_reveal_enabled());
        assert!(view.read(cx).settings_draft().api_key_unchanged);
        assert!(view.read(cx).settings_api_key_value(cx).is_empty());

        // Reveal cannot pretend to show a stored key.
        view.update(cx, |app, cx| app.toggle_settings_key_reveal(window, cx));
        assert!(!view.read(cx).settings_key_revealed());
        assert!(view.read(cx).settings_api_key_value(cx).is_empty());

        // A replacement draft value re-enables Reveal and hides the stored
        // line; it does not change the stored key by itself.
        view.update(cx, |app, cx| {
            app.set_settings_api_key_value(MIXED_CASE_KEY, window, cx)
        });
        window.render_frame(cx);
        assert!(view.read(cx).settings_key_reveal_enabled());
        assert_eq!(view.read(cx).settings_stored_key_label(), None);
    })
    .unwrap();
}

// ----- T13: remembered window geometry ---------------------------------

#[gpui_kit::test]
fn a_stable_window_geometry_is_persisted_after_the_quiet_period(cx: &mut gpui_kit::TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        speechtt_ui::theme::apply(cx);
    });

    let store = MemoryStore::new(Settings::default());
    let store_handle = store.clone();
    let fake = FakeApp {
        store,
        ..FakeApp::default()
    };
    let (_window, view) = common::open_window(cx, None, |window, cx| {
        cx.new(|cx| SpeechTtApp::with_services(fake.services(), window, cx))
    });

    let start = Instant::now();
    let layout = WindowLayout::new(700, 600, 120, 80);

    cx.update(|cx| {
        view.update(cx, |app, _| {
            app.remember_window_layout(layout, start);
            assert!(
                !app.flush_pending_window_layout(start + Duration::from_millis(599)),
                "the debounce must keep the drag from writing on every frame"
            );
            assert!(app.flush_pending_window_layout(start + Duration::from_millis(600)));
            assert!(
                !app.flush_pending_window_layout(start + Duration::from_secs(2)),
                "nothing left to persist"
            );
        });
    });

    assert_eq!(store_handle.get().window, Some(layout));
}

#[gpui_kit::test]
fn saving_settings_keeps_the_remembered_geometry(cx: &mut gpui_kit::TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        speechtt_ui::theme::apply(cx);
    });

    let layout = WindowLayout::new(640, 560, 20, 30);
    let mut settings = settings_with_key();
    settings.window = Some(layout);
    let store = MemoryStore::new(settings);
    let store_handle = store.clone();
    let fake = FakeApp {
        store,
        ..FakeApp::default()
    };
    let (_window, view) = common::open_window(cx, None, |window, cx| {
        cx.new(|cx| SpeechTtApp::with_services(fake.services(), window, cx))
    });

    cx.update(|cx| {
        view.update(cx, |app, _| {
            app.apply_settings_change(SettingsChange {
                provider: "vercel-ai-gateway".to_string(),
                model: "openai/gpt-4o-transcribe".to_string(),
                api_key: Some("sk-rotated".to_string()),
            });
        });
    });

    let saved = store_handle.get();
    assert_eq!(saved.window, Some(layout));
    assert_eq!(saved.model, "openai/gpt-4o-transcribe");
}

// ----- T15.8: file transcription wiring -------------------------------

/// Backend event sender the file-transcription tests push into.
type EventSender = speechtt_core::async_channel::Sender<Result<SttEvent, BackendError>>;

/// Ready app whose backend events come from a channel the test owns.
fn ready_app_for_files(
    cx: &mut gpui_kit::TestAppContext,
) -> (
    gpui_kit::WindowHandle<gpui_kit::base::Root>,
    gpui_kit::Entity<SpeechTtApp>,
    EventSender,
    RecordingSink,
) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        speechtt_ui::theme::apply(cx);
    });

    let (sender, events) = speechtt_core::async_channel::unbounded();
    let fake = FakeApp {
        events,
        store: MemoryStore::new(settings_with_key()),
        ..FakeApp::default()
    };
    let sink = fake.sink.clone();

    let (window, view) = common::open_window(cx, None, |window, cx| {
        cx.new(|cx| SpeechTtApp::with_services(fake.services(), window, cx))
    });

    cx.update(|cx| {
        view.update(cx, |app, cx| app.apply_capabilities(catalogue(), cx));
    });

    cx.update_window(window.into(), |_, window, cx| {
        view.update(cx, |app, cx| {
            // The file card opens the native picker; this helper walks the form
            // from its headless seams instead, so the pending prompt is ignored
            // (T0023: picker -> form -> take).
            app.choose_mode(SessionMode::File, cx);
            assert_eq!(app.view_state(), ViewState::Mode);
            app.cycle_provider(cx);
            app.cycle_model(cx);
            app.create_session(cx);
            assert_eq!(app.view_state(), ViewState::Form);
        });
        window.render_frame(cx);
    })
    .unwrap();

    (window, view, sender, sink)
}

#[gpui_kit::test]
fn a_file_path_starts_a_take_without_audio(cx: &mut gpui_kit::TestAppContext) {
    let dir = TempDir::new("file-take");
    let media = dir.path().join("interview.mp4");
    std::fs::write(&media, b"RIFF0000WAVE").expect("temp media");

    let (window, view, _sender, sink) = ready_app_for_files(cx);
    // The construction handshake already sent a capabilities request.
    let _ = sink.take();

    cx.update_window(window.into(), |_, window, cx| {
        view.update(cx, |app, cx| app.start_file_from_path(&media, cx));
        window.render_frame(cx);
    })
    .unwrap();

    assert_eq!(
        cx.update(|cx| view.read(cx).view_state()),
        ViewState::Recording
    );

    let commands = sink.take();
    assert_eq!(commands.len(), 1, "{commands:?}");
    match &commands[0] {
        SttCommand::TranscribeFile {
            session_id,
            path,
            api_key,
            ..
        } => {
            assert_eq!(session_id, "take-1");
            assert_eq!(path, &media.to_string_lossy().into_owned());
            assert_eq!(
                api_key.as_ref().map(SecretString::expose_secret),
                Some("sk-e2e-secret")
            );
        }
        other => panic!("expected transcribeFile, got {other:?}"),
    }

    // Capture frames must never reach a file take.
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
    })
    .unwrap();
    assert!(sink.take().is_empty());
}

#[gpui_kit::test]
fn a_progress_event_drives_the_elapsed_read_out(cx: &mut gpui_kit::TestAppContext) {
    let dir = TempDir::new("file-progress");
    let media = dir.path().join("lecture.m4a");
    std::fs::write(&media, b"RIFF0000WAVE").expect("temp media");

    let (window, view, sender, _sink) = ready_app_for_files(cx);

    cx.update_window(window.into(), |_, window, cx| {
        view.update(cx, |app, cx| app.start_file_from_path(&media, cx));
        window.render_frame(cx);
    })
    .unwrap();

    sender
        .send_blocking(Ok(SttEvent::Progress {
            session_id: "take-1".into(),
            processed_seconds: 12.6,
            total_seconds: Some(340.25),
        }))
        .expect("send progress");
    cx.run_until_parked();

    assert_eq!(cx.update(|cx| view.read(cx).elapsed_seconds()), 12);
}

#[gpui_kit::test]
fn a_missing_file_shows_a_typed_error_and_keeps_the_ready_screen(
    cx: &mut gpui_kit::TestAppContext,
) {
    let (window, view, _sender, sink) = ready_app_for_files(cx);
    let _ = sink.take();
    let missing = std::env::temp_dir().join("speechtt-missing-e2e.mov");

    cx.update_window(window.into(), |_, window, cx| {
        view.update(cx, |app, cx| app.start_file_from_path(&missing, cx));
        window.render_frame(cx);
    })
    .unwrap();

    // File mode has no holding screen; the form stays up (T0023).
    assert_eq!(cx.update(|cx| view.read(cx).view_state()), ViewState::Form);
    assert_eq!(
        cx.update(|cx| view.read(cx).app_error().cloned()),
        Some(AppError::FileNotFound)
    );
    assert!(sink.take().is_empty());
}
