//! Pure view-model for the widget: session workflow, setup draft, screen
//! selection and live take telemetry, with no GPUI `Window`/`Context`
//! dependency.
//!
//! Keeping this logic free of rendering concerns means the whole session flow
//! can be exercised by ordinary `#[test]` unit tests. `app::SpeechTtApp` owns a
//! [`SessionController`] and only translates its state into elements.
//!
//! The controller is also the duplex boundary: it emits [`SttCommand`]s to an
//! optional [`SttSessionSink`] and folds inbound [`SttEvent`]s into a
//! [`TranscriptBuffer`], so `CaptionWell`, `Waveform` and `ElapsedTimer` read
//! live state rather than mock constants.

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use gpui_kit::SharedString;
use speechtt_core::audio::AudioError;
use speechtt_core::backend::{BackendError, SttEventReceiver, SttSessionSink};
use speechtt_core::hotkeys::SessionAction;
use speechtt_core::output::OutputError;
use speechtt_core::output::clipboard::{ClipboardSink, CopySummary, copy_transcript};
use speechtt_core::output::file::{FileOpener, SavedFile, TranscriptFormat, save_transcript_at};
use speechtt_core::output::paste::PasteSink;
use speechtt_core::protocol::{
    CapabilitySource, ProviderCapability, SecretString, SttCommand, SttEvent, TranscriptionMode,
};
use speechtt_core::session::{
    FinishedSession, OutputTarget, SessionError, SessionField, SessionMachine, SessionState,
};
use speechtt_core::settings::{Settings, SettingsError};
use speechtt_core::transcript::TranscriptBuffer;

/// Mock audio source until device enumeration lands (Phase 3).
pub const DEFAULT_AUDIO_SOURCE: &str = "System default microphone";

/// PCM sample rate sent to the STT backend (16 kHz mono, see `PRODUCT.md` §5).
pub const SAMPLE_RATE: u32 = 16_000;

/// Bars in the idle waveform row (matches the 480×280 mockups).
pub const WAVEFORM_BARS: usize = 36;

/// Reference elapsed read-out used by the static mockups.
pub const MOCK_ELAPSED_SECONDS: u32 = 42;
/// Reference FFT frame used by the static mockups.
pub const MOCK_BARS: [f32; 36] = [
    0.28, 0.55, 0.34, 0.60, 0.78, 0.41, 0.52, 0.33, 0.72, 0.95, 0.85, 0.48, 0.30, 0.42, 0.66, 0.87,
    0.94, 0.58, 0.36, 0.26, 0.47, 0.70, 0.89, 0.81, 0.44, 0.32, 0.56, 0.75, 0.90, 0.63, 0.39, 0.29,
    0.50, 0.68, 0.46, 0.24,
];
/// Offline demo transcript shown on the stopped screen when no STT backend is
/// connected (Phases 3–6 replace it with the live [`TranscriptBuffer`]).
pub const MOCK_COMMITTED: &str =
    "...so I think the best approach here is to split the work into two phases, starting with the";
pub const MOCK_LIVE: &str = "audio capture layer";

/// Time source injected through [`crate::app::AppServices`].
///
/// Production uses [`SystemClock`]; tests use a manual clock so the elapsed
/// read-out and saved-file timestamps never depend on wall time.
pub trait Clock: Send {
    /// Monotonic instant used for elapsed-time accounting.
    fn now(&self) -> Instant;
    /// Wall-clock time used for saved-file names.
    fn system_now(&self) -> SystemTime;
}

/// The real clock.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }

    fn system_now(&self) -> SystemTime {
        SystemTime::now()
    }
}

/// One typed failure surface for every layer (`PRODUCT.md` Phase 9).
///
/// Variants carry only user-safe data: never a transcript or key material.
/// Backend messages are reduced to a deterministic variant (plus a diagnostic
/// detail when it is safe) so raw provider text cannot reach the screen or a
/// log line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppError {
    MicPermissionDenied,
    NoInputDevice,
    MissingApiKey { provider: String },
    BackendUnavailable,
    BackendTimeout,
    ProviderAuthFailed,
    NetworkLost,
    SaveFailed(String),
    ClipboardFailed,
    PasteFailed,
    ProtocolError(String),
}

/// What the error strip offers next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorAction {
    /// Opens the Settings dialog.
    OpenSettings,
    /// Restarts/re-issues `Start` with the same session id.
    Retry,
    /// Plain "Open settings" prose (OS-level microphone permission).
    SettingsHint,
    /// Dismisses the strip.
    Dismiss,
}

impl AppError {
    /// Short, user-facing message. Never contains key material or transcript
    /// text.
    #[must_use]
    pub fn message(&self) -> &'static str {
        match self {
            Self::MicPermissionDenied => {
                "Microphone access is denied. Open your system sound settings to allow it."
            }
            Self::NoInputDevice => "No microphone is available.",
            Self::MissingApiKey { .. } => "Add an API key for this provider in Settings.",
            Self::BackendUnavailable => "The transcription backend is unavailable.",
            Self::BackendTimeout => "The transcription backend did not respond in time.",
            Self::ProviderAuthFailed => "The provider rejected the API key. Update it in Settings.",
            Self::NetworkLost => "The network connection was lost.",
            Self::SaveFailed(_) => "Could not save the transcript.",
            Self::ClipboardFailed => "Could not copy the transcript.",
            Self::PasteFailed => "Could not paste the transcript.",
            Self::ProtocolError(_) => "The transcription backend sent an unexpected message.",
        }
    }

    /// The primary action the strip should offer.
    #[must_use]
    pub fn action(&self) -> ErrorAction {
        match self {
            Self::MissingApiKey { .. } | Self::ProviderAuthFailed => ErrorAction::OpenSettings,
            Self::BackendUnavailable | Self::BackendTimeout | Self::NetworkLost => {
                ErrorAction::Retry
            }
            Self::MicPermissionDenied => ErrorAction::SettingsHint,
            Self::NoInputDevice
            | Self::SaveFailed(_)
            | Self::ClipboardFailed
            | Self::PasteFailed
            | Self::ProtocolError(_) => ErrorAction::Dismiss,
        }
    }

    /// Optional diagnostic detail for logs; never rendered.
    #[must_use]
    pub fn detail(&self) -> Option<&str> {
        match self {
            Self::SaveFailed(detail) | Self::ProtocolError(detail) => Some(detail),
            _ => None,
        }
    }

    /// Map a capture failure. `UnsupportedConfig` and generic backend failures
    /// are treated as "no usable input", because there is no device-level
    /// recovery the user can trigger from the strip.
    #[must_use]
    pub fn from_audio(error: &AudioError) -> Self {
        match error {
            AudioError::PermissionDenied => Self::MicPermissionDenied,
            AudioError::NoDevice | AudioError::UnsupportedConfig(_) | AudioError::Backend(_) => {
                Self::NoInputDevice
            }
        }
    }

    /// Map a transport failure.
    #[must_use]
    pub fn from_backend(error: &BackendError) -> Self {
        match error {
            BackendError::Io(_) | BackendError::Disconnected => Self::BackendUnavailable,
            BackendError::Spawn { .. } => Self::BackendUnavailable,
            BackendError::Encode(_) | BackendError::Decode(_) => {
                Self::ProtocolError(error.to_string())
            }
            // `BackendError` is `#[non_exhaustive]`: future transport
            // failures stay retryable until they get a specific mapping.
            _ => Self::BackendUnavailable,
        }
    }

    /// Map one `SttEvent::Error` payload. Codes are canonical (see
    /// `src/stt/protocol.ts`); the message is only inspected for the generic
    /// `stream_failed` bucket, and is never copied into the error.
    #[must_use]
    pub fn from_stt_error(code: &str, message: &str, provider: &str) -> Self {
        match code {
            "missing_api_key" => Self::MissingApiKey {
                provider: provider.to_owned(),
            },
            "auth_failed" | "provider_auth_failed" | "unauthorized" | "invalid_api_key" => {
                Self::ProviderAuthFailed
            }
            "network_lost" | "network_error" | "connection_lost" | "connection_failed" => {
                Self::NetworkLost
            }
            "timeout" | "backend_timeout" => Self::BackendTimeout,
            "stream_failed" => {
                let message = message.to_ascii_lowercase();
                if contains_any(&message, &["auth", "api key", "unauthorized", "forbidden"]) {
                    Self::ProviderAuthFailed
                } else if contains_any(
                    &message,
                    &[
                        "network",
                        "socket",
                        "connection",
                        "timed out",
                        "timeout",
                        "offline",
                    ],
                ) {
                    Self::NetworkLost
                } else {
                    Self::ProtocolError(code.to_owned())
                }
            }
            _ => Self::ProtocolError(code.to_owned()),
        }
    }

    /// Map a settings store failure.
    #[must_use]
    pub fn from_settings(error: &SettingsError) -> Self {
        match error {
            SettingsError::Io { .. } | SettingsError::Encode(_) => {
                Self::SaveFailed(error.to_string())
            }
            SettingsError::Corrupt { .. } | SettingsError::Invalid(_) => {
                Self::ProtocolError(error.to_string())
            }
            _ => Self::SaveFailed(error.to_string()),
        }
    }

    /// Map an output adapter failure.
    #[must_use]
    pub fn from_output(error: OutputError) -> Self {
        match error {
            OutputError::Clipboard(_) => Self::ClipboardFailed,
            OutputError::Paste(_) => Self::PasteFailed,
            OutputError::EmptyTranscript => Self::ProtocolError("transcript is empty".to_owned()),
            OutputError::UnsupportedFormat(detail) | OutputError::Write(detail) => {
                Self::SaveFailed(detail)
            }
            OutputError::OpenFile(detail) => {
                Self::SaveFailed(format!("could not open file: {detail}"))
            }
        }
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.message())
    }
}

impl std::error::Error for AppError {}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
}

/// Which setup screen the body shows while the machine is `Blank`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Screen {
    Prompt,
    Form,
}

/// What the widget is currently displaying. Derived from the machine, the
/// setup screen and the finished-take overlay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewState {
    Prompt,
    Form,
    Ready,
    Recording,
    Paused,
    Stopped,
}

/// The focused overlay currently open, if any. Only one dialog at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialog {
    Help,
    Settings,
}

/// Hint shown under the API key row when a stored key cannot be revealed.
pub const STORED_KEY_REVEAL_HINT: &str =
    "Stored keys are never displayed; paste a new key to replace it.";

/// Editable copy of the settings shown in the Settings dialog.
///
/// It is deliberately UI-only: T0006 supplies the persisted store and T0009
/// maps [`SettingsChange`] into it.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct SettingsDraft {
    pub provider: Option<String>,
    pub model: Option<String>,
    /// Raw user input. Never printed by `Debug` or written to logs.
    pub api_key: String,
    /// `true` when a key was already stored and the field is untouched.
    pub api_key_unchanged: bool,
}

impl fmt::Debug for SettingsDraft {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SettingsDraft")
            .field("provider", &self.provider)
            .field("model", &self.model)
            .field(
                "api_key",
                &if self.api_key.is_empty() {
                    "[EMPTY]"
                } else {
                    "[REDACTED]"
                },
            )
            .field("api_key_unchanged", &self.api_key_unchanged)
            .finish()
    }
}

/// The value committed by `Save changes`; consumed by the settings store in
/// T0009. `api_key = None` keeps the stored key, `Some("")` clears it.
#[derive(Clone, PartialEq, Eq)]
pub struct SettingsChange {
    pub provider: String,
    pub model: String,
    pub api_key: Option<String>,
}

impl fmt::Debug for SettingsChange {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SettingsChange")
            .field("provider", &self.provider)
            .field("model", &self.model)
            .field("api_key", &self.api_key.as_ref().map(|_| "[REDACTED]"))
            .finish()
    }
}

/// Form values not yet committed to the machine.
///
/// Provider and model are catalogue ids (capabilities when available, else
/// [`provider_options`]), so the form stays in step with the worker registry
/// and with persisted [`Settings`].
#[derive(Debug, Default)]
struct SessionDraft {
    provider: Option<String>,
    model: Option<String>,
    output: Option<OutputTarget>,
    session_name: String,
}

/// A draft whose required fields all resolved to real catalogue entries.
#[derive(Debug)]
struct ValidatedDraft {
    provider: String,
    model: String,
    session_name: String,
    output: OutputTarget,
}

/// The finished take shown in the `Stopped` screen. The machine hands the
/// session identity back on `stop`; the transcript is the live buffer at that
/// moment and is refined by the final `stopped` event.
#[derive(Debug)]
pub struct FinishedTake {
    session: FinishedSession,
    committed: SharedString,
    live: SharedString,
}

impl FinishedTake {
    pub fn session(&self) -> &FinishedSession {
        &self.session
    }

    pub fn committed(&self) -> &SharedString {
        &self.committed
    }

    pub fn live(&self) -> &SharedString {
        &self.live
    }
}

/// Owns the session machine, the setup draft, the stopped-screen result and
/// the live take telemetry.
pub struct SessionController {
    machine: SessionMachine,
    screen: Screen,
    draft: SessionDraft,
    result: Option<FinishedTake>,
    /// Monotonic take counter; the wire id is `take-{session_id}`.
    session_id: u64,
    transcript: TranscriptBuffer,
    waveform_bars: Arc<[f32]>,
    elapsed_seconds: u32,
    /// Elapsed time accumulated before the current segment.
    elapsed_base: Duration,
    /// Start of the current recording segment (paused takes have none).
    segment_started: Option<Instant>,
    last_error: Option<SharedString>,
    /// Typed Phase 9 error surface (the strip).
    app_error: Option<AppError>,
    /// Transient output feedback ("Copied", "Saved", …).
    output_status: Option<SharedString>,
    /// Path produced by the last successful save/open action.
    written_path: Option<PathBuf>,
    active_dialog: Option<Dialog>,
    settings_draft: SettingsDraft,
    /// Persisted settings once the composition root has loaded them.
    settings: Option<Settings>,
    /// Latest provider catalogue from the Bun capability handshake.
    capabilities: Option<Vec<ProviderCapability>>,
    /// Where that catalogue came from (`builtin` fallback or live discovery).
    capability_source: Option<CapabilitySource>,
    /// The last `Start` command, kept so Retry can reuse the session id.
    last_start: Option<SttCommand>,
    sink: Option<Box<dyn SttSessionSink>>,
    events: Option<SttEventReceiver>,
}

impl fmt::Debug for SessionController {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SessionController")
            .field("machine", &self.machine)
            .field("screen", &self.screen)
            .field("draft", &self.draft)
            .field("result", &self.result)
            .field("session_id", &self.session_id)
            .field("transcript", &self.transcript)
            .field("waveform_bars", &self.waveform_bars)
            .field("elapsed_seconds", &self.elapsed_seconds)
            .field("last_error", &self.last_error)
            .field("app_error", &self.app_error)
            .field("output_status", &self.output_status)
            .field("written_path", &self.written_path)
            .field("active_dialog", &self.active_dialog)
            .field("settings_draft", &self.settings_draft)
            .field("settings", &self.settings)
            .field(
                "capability_count",
                &self.capabilities.as_ref().map(Vec::len),
            )
            .field("has_sink", &self.sink.is_some())
            .field("has_event_stream", &self.events.is_some())
            .finish()
    }
}

impl Default for SessionController {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionController {
    pub fn new() -> Self {
        let mut machine = SessionMachine::new();
        // The mock "idle" frame assumes a usable default mic: no device row
        // on the setup form.
        machine
            .set_audio_source(DEFAULT_AUDIO_SOURCE)
            .expect("blank machine accepts setup");

        Self {
            machine,
            screen: Screen::Prompt,
            draft: SessionDraft::default(),
            result: None,
            session_id: 0,
            transcript: TranscriptBuffer::default(),
            waveform_bars: idle_waveform(),
            elapsed_seconds: 0,
            elapsed_base: Duration::ZERO,
            segment_started: None,
            last_error: None,
            app_error: None,
            output_status: None,
            written_path: None,
            active_dialog: None,
            settings_draft: SettingsDraft::default(),
            settings: None,
            capabilities: None,
            capability_source: None,
            last_start: None,
            sink: None,
            events: None,
        }
    }

    // ----- Queries -----------------------------------------------------

    pub fn view_state(&self) -> ViewState {
        if self.result.is_some() {
            return ViewState::Stopped;
        }
        match (self.machine.state(), self.screen) {
            (SessionState::Blank, Screen::Prompt) => ViewState::Prompt,
            (SessionState::Blank, Screen::Form) => ViewState::Form,
            (SessionState::Ready, _) => ViewState::Ready,
            (SessionState::Recording, _) => ViewState::Recording,
            (SessionState::Paused, _) => ViewState::Paused,
        }
    }

    pub fn session_state(&self) -> SessionState {
        self.machine.state()
    }

    pub fn session_missing(&self) -> Vec<SessionField> {
        self.machine.missing()
    }

    /// The current (or most recent) take counter.
    #[must_use]
    pub fn session_id(&self) -> u64 {
        self.session_id
    }

    /// The wire-level session id, e.g. `"take-1"`. Empty before the first take.
    #[must_use]
    pub fn session_label(&self) -> String {
        if self.session_id == 0 {
            String::new()
        } else {
            format!("take-{}", self.session_id)
        }
    }

    /// Finalized transcript text, read by `CaptionWell`.
    #[must_use]
    pub fn transcript_committed(&self) -> &str {
        self.transcript.committed()
    }

    /// In-progress live tail, read by `CaptionWell`.
    #[must_use]
    pub fn transcript_live(&self) -> &str {
        self.transcript.live()
    }

    /// Committed plus live text, for copy/save actions.
    #[must_use]
    pub fn transcript_full_text(&self) -> String {
        self.transcript.full_text()
    }

    /// The latest FFT frame, shared with the renderer without copying.
    #[must_use]
    pub fn waveform_bars(&self) -> Arc<[f32]> {
        Arc::clone(&self.waveform_bars)
    }

    /// Seconds elapsed in the current (or most recent) take.
    #[must_use]
    pub fn elapsed_seconds(&self) -> u32 {
        self.elapsed_seconds
    }

    /// The last backend/protocol failure, shown until the next take.
    #[must_use]
    pub fn last_error(&self) -> Option<&SharedString> {
        self.last_error.as_ref()
    }

    /// The typed failure shown by the status strip.
    #[must_use]
    pub fn app_error(&self) -> Option<&AppError> {
        self.app_error.as_ref()
    }

    /// The action the strip should offer, if any.
    #[must_use]
    pub fn error_action(&self) -> Option<ErrorAction> {
        self.app_error.as_ref().map(AppError::action)
    }

    /// Record a typed failure for the strip.
    pub fn report_error(&mut self, error: AppError) {
        self.last_error = Some(error.message().into());
        self.app_error = Some(error);
    }

    /// Dismiss the current strip without touching the transcript.
    pub fn dismiss_error(&mut self) {
        self.app_error = None;
        self.last_error = None;
    }

    /// Transient confirmation for Copy/Save/Paste ("Copied", "Saved", …).
    #[must_use]
    pub fn output_status(&self) -> Option<&str> {
        self.output_status.as_deref()
    }

    /// Path written or opened by the last successful output action.
    #[must_use]
    pub fn written_path(&self) -> Option<&Path> {
        self.written_path.as_deref()
    }

    pub fn is_draft_complete(&self) -> bool {
        self.is_draft_complete_with_name(&self.draft.session_name)
    }

    /// Completeness check against a name supplied by the view's input entity.
    /// The UI reads the editable input directly; the controller may not have
    /// received a change event yet.
    pub fn is_draft_complete_with_name(&self, session_name: &str) -> bool {
        let (Some(provider), Some(model)) = (&self.draft.provider, &self.draft.model) else {
            return false;
        };
        self.provider_exists(provider)
            && self.model_exists(provider, model)
            && self.draft.output.is_some()
            && !session_name.trim().is_empty()
    }

    pub fn finished_take(&self) -> Option<&FinishedTake> {
        self.result.as_ref()
    }

    pub fn has_provider(&self) -> bool {
        self.draft.provider.is_some()
    }

    pub fn has_model(&self) -> bool {
        self.draft.model.is_some()
    }

    pub fn provider_label(&self) -> Option<&str> {
        self.provider_label_for(self.draft.provider.as_deref()?)
    }

    pub fn model_label(&self) -> Option<&str> {
        self.model_label_for(
            self.draft.provider.as_deref()?,
            self.draft.model.as_deref()?,
        )
    }

    pub fn selected_output(&self) -> Option<&OutputTarget> {
        self.draft.output.as_ref()
    }

    /// Catalogue label for a provider id.
    fn provider_label_for(&self, provider: &str) -> Option<&str> {
        self.capabilities
            .as_ref()
            .and_then(|providers| providers.iter().find(|option| option.id == provider))
            .map(|option| option.label.as_str())
    }

    /// Catalogue label for a model id inside a provider.
    fn model_label_for(&self, provider: &str, model: &str) -> Option<&str> {
        self.capabilities
            .as_ref()
            .and_then(|providers| providers.iter().find(|option| option.id == provider))
            .and_then(|option| option.models.iter().find(|option| option.id == model))
            .map(|option| option.label.as_str())
    }

    fn provider_exists(&self, provider: &str) -> bool {
        self.capabilities
            .as_ref()
            .is_some_and(|providers| providers.iter().any(|option| option.id == provider))
    }

    fn model_exists(&self, provider: &str, model: &str) -> bool {
        self.capabilities.as_ref().is_some_and(|providers| {
            providers
                .iter()
                .find(|option| option.id == provider)
                .is_some_and(|option| option.models.iter().any(|option| option.id == model))
        })
    }

    /// Mode of a model in the latest catalogue.
    fn model_mode(&self, provider: &str, model: &str) -> Option<TranscriptionMode> {
        self.capabilities
            .as_ref()?
            .iter()
            .find(|option| option.id == provider)?
            .models
            .iter()
            .find(|option| option.id == model)
            .map(|option| option.mode)
    }

    fn provider_ids(&self) -> Vec<String> {
        self.capabilities
            .as_ref()
            .map_or_else(Vec::new, |providers| {
                providers.iter().map(|option| option.id.clone()).collect()
            })
    }

    fn model_ids(&self, provider: &str) -> Vec<String> {
        self.capabilities
            .as_ref()
            .and_then(|providers| providers.iter().find(|option| option.id == provider))
            .map(|option| option.models.iter().map(|model| model.id.clone()).collect())
            .unwrap_or_default()
    }

    // ----- Dialogs -----------------------------------------------------

    /// The overlay currently open, if any.
    #[must_use]
    pub fn active_dialog(&self) -> Option<Dialog> {
        self.active_dialog
    }

    /// The Settings dialog's editable copy.
    #[must_use]
    pub fn settings_draft(&self) -> &SettingsDraft {
        &self.settings_draft
    }

    /// Label of the provider selected in the Settings dialog.
    #[must_use]
    pub fn settings_provider_label(&self) -> Option<&str> {
        self.provider_label_for(self.settings_draft.provider.as_deref()?)
    }

    /// Label of the model selected in the Settings dialog.
    #[must_use]
    pub fn settings_model_label(&self) -> Option<&str> {
        self.model_label_for(
            self.settings_draft.provider.as_deref()?,
            self.settings_draft.model.as_deref()?,
        )
    }

    /// Environment variable the worker reads for the selected provider.
    ///
    /// Comes from the capability handshake so the UI never maintains its own
    /// provider catalogue.
    #[must_use]
    pub fn settings_api_key_env(&self) -> Option<&str> {
        let provider = self.settings_draft.provider.as_deref()?;
        self.capabilities
            .as_ref()?
            .iter()
            .find(|option| option.id == provider)?
            .key_env_vars
            .first()
            .map(String::as_str)
    }

    /// Mode of the model chosen for the next take (New Session form).
    #[must_use]
    pub fn draft_model_mode(&self) -> Option<TranscriptionMode> {
        self.model_mode(
            self.draft.provider.as_deref()?,
            self.draft.model.as_deref()?,
        )
    }

    /// Mode of the model selected in the Settings dialog.
    #[must_use]
    pub fn settings_model_mode(&self) -> Option<TranscriptionMode> {
        self.model_mode(
            self.settings_draft.provider.as_deref()?,
            self.settings_draft.model.as_deref()?,
        )
    }

    /// Mode of the model used by the active take.
    ///
    /// `None` before a take starts and when the catalogue does not know the
    /// model (an older worker, or a catalogue that has not arrived yet).
    #[must_use]
    pub fn session_model_mode(&self) -> Option<TranscriptionMode> {
        if !self.machine.state().is_active() {
            return None;
        }
        let SttCommand::Start {
            provider, model, ..
        } = self.last_start.as_ref()?
        else {
            return None;
        };
        self.model_mode(provider, model)
    }

    /// Where the current catalogue came from.
    #[must_use]
    pub fn catalogue_source(&self) -> Option<CapabilitySource> {
        self.capability_source
    }

    /// Whether a catalogue has arrived at least once.
    #[must_use]
    pub fn catalogue_loaded(&self) -> bool {
        self.capabilities.is_some()
    }

    /// The stored key mask for the provider selected in the dialog.
    ///
    /// Only [`Settings::masked_api_key`] output is exposed; the raw key never
    /// leaves the settings store.
    #[must_use]
    pub fn settings_stored_key_mask(&self) -> Option<String> {
        let provider = self.settings_draft.provider.as_deref()?;
        self.settings.as_ref()?.masked_api_key(provider)
    }

    /// Whether the draft still refers to the stored key and the field is empty.
    #[must_use]
    pub fn settings_key_is_stored_unchanged(&self) -> bool {
        self.settings_draft.api_key_unchanged && self.settings_draft.api_key.is_empty()
    }

    /// The secondary line shown under the API key row for a stored key.
    ///
    /// `None` once the user types or pastes a replacement value.
    #[must_use]
    pub fn settings_stored_key_label(&self) -> Option<String> {
        if !self.settings_key_is_stored_unchanged() {
            return None;
        }
        self.settings_stored_key_mask()
            .map(|mask| format!("Stored key: {mask}"))
    }

    /// Hint shown while Reveal is disabled for a stored key.
    #[must_use]
    pub fn settings_stored_key_hint(&self) -> Option<&'static str> {
        self.settings_stored_key_label()
            .map(|_| STORED_KEY_REVEAL_HINT)
    }

    /// Whether Reveal applies to a typed draft value.
    ///
    /// A stored key cannot be revealed because its raw value never leaves the
    /// settings store.
    #[must_use]
    pub fn settings_key_reveal_enabled(&self) -> bool {
        !self.settings_draft.api_key.is_empty()
    }

    /// Whether the selected provider exists in the capability catalogue.
    #[must_use]
    pub fn settings_provider_known(&self) -> bool {
        self.settings_draft
            .provider
            .as_deref()
            .is_some_and(|provider| self.provider_exists(provider))
    }

    /// Capability-reported availability for a provider. `None` when the
    /// provider is unknown or no handshake has arrived.
    #[must_use]
    pub fn provider_available(&self, provider: &str) -> Option<bool> {
        self.capabilities
            .as_ref()?
            .iter()
            .find(|option| option.id == provider)
            .map(|option| option.available)
    }

    /// Whether a key is typed or already stored.
    #[must_use]
    pub fn settings_has_key(&self) -> bool {
        !self.settings_draft.api_key.is_empty() || self.settings_draft.api_key_unchanged
    }

    /// Whether the dialog shows a connected badge: a key is available and the
    /// provider is not reported unavailable.
    #[must_use]
    pub fn settings_connected(&self) -> bool {
        let unavailable = self
            .settings_draft
            .provider
            .as_deref()
            .and_then(|provider| self.provider_available(provider))
            == Some(false);
        self.settings_has_key() && !unavailable
    }

    /// A complete provider + model + usable key is required before saving.
    /// Availability is deliberately not required: saving a key is how an
    /// unavailable provider becomes connected on the next handshake.
    #[must_use]
    pub fn settings_draft_valid(&self) -> bool {
        self.settings_provider_known()
            && self.settings_draft.model.is_some()
            && self.settings_has_key()
    }

    /// The provider catalogue from the latest handshake, if any.
    #[must_use]
    pub fn capabilities(&self) -> Option<&[ProviderCapability]> {
        self.capabilities.as_deref()
    }

    /// Replace the provider catalogue from a `capabilities` event.
    pub fn set_capabilities(&mut self, providers: Vec<ProviderCapability>) {
        self.apply_capabilities(providers, CapabilitySource::Builtin);
    }

    /// Replace the provider catalogue and record where it came from.
    ///
    /// The handshake answers twice (builtin, then discovered); the later event
    /// simply replaces the earlier one. A first catalogue also seeds the new
    /// session form from the stored settings.
    pub fn apply_capabilities(
        &mut self,
        providers: Vec<ProviderCapability>,
        source: CapabilitySource,
    ) {
        self.capabilities = Some(providers);
        self.capability_source = Some(source);
        if self.screen == Screen::Form {
            self.seed_draft_from_settings();
        }
    }

    /// Ask the worker for the current provider/model catalogue.
    ///
    /// A stored key is forwarded so a provider configured only in the settings
    /// file (not in `process.env`) still reports as available. The worker
    /// never echoes it back, and `Debug` redacts it.
    pub fn request_capabilities(&mut self) {
        let api_key = self
            .settings
            .as_ref()
            .and_then(|settings| settings.api_key(&settings.provider))
            .map(SecretString::new);
        self.send_command(SttCommand::Capabilities {
            request_id: None,
            api_key,
        });
    }

    /// Apply persisted settings (provider/model/key/auto-paste).
    pub fn apply_settings(&mut self, settings: Settings) {
        self.settings = Some(settings);
        if self.screen == Screen::Form {
            self.seed_draft_from_settings();
        }
    }

    /// The persisted settings, if the composition root loaded them.
    #[must_use]
    pub fn settings(&self) -> Option<&Settings> {
        self.settings.as_ref()
    }

    // ----- Backend wiring ----------------------------------------------

    /// Attach the duplex backend. Its event stream (if any) is exposed by
    /// [`SessionController::take_event_stream`] for the view's receiver loop.
    pub fn set_sink(&mut self, sink: Box<dyn SttSessionSink>) {
        self.events = sink.events();
        self.sink = Some(sink);
    }

    /// Take ownership of the backend event stream, if one is attached.
    pub fn take_event_stream(&mut self) -> Option<SttEventReceiver> {
        self.events.take()
    }

    /// Record a transport failure (e.g. the worker exited) for the UI.
    pub fn report_backend_error(&mut self, error: impl fmt::Display) {
        self.last_error = Some(error.to_string().into());
    }

    /// Fold one inbound backend event into live take state.
    ///
    /// Events from a previous take are ignored so a late packet cannot corrupt
    /// the current one. `Ready` and session-less errors are global.
    pub fn handle_stt_event(&mut self, event: SttEvent) {
        if !self.accepts_session(event.session_id()) {
            return;
        }

        if let SttEvent::Error { code, message, .. } = &event {
            let provider = self.active_provider();
            self.last_error = Some(format!("{code}: {message}").into());
            self.app_error = Some(AppError::from_stt_error(code, message, &provider));
            return;
        }

        if let SttEvent::Capabilities {
            providers, source, ..
        } = &event
        {
            self.apply_capabilities(providers.clone(), *source);
            return;
        }

        self.transcript.apply(&event);

        // A `stopped` flush usually lands just after the stopped screen is
        // already visible: refine it in place.
        if matches!(event, SttEvent::Stopped { .. })
            && let Some(result) = &mut self.result
        {
            result.committed = self.transcript.committed().into();
            result.live = self.transcript.live().into();
        }
    }

    /// Drive the take clock from the capture pipeline.
    pub fn set_elapsed_seconds(&mut self, seconds: u32) {
        if self.machine.state().is_active() {
            self.elapsed_seconds = seconds;
            self.elapsed_base = Duration::from_secs(u64::from(seconds));
            self.segment_started = Some(Instant::now());
        }
    }

    /// Recompute the elapsed read-out from the injected clock.
    pub fn refresh_elapsed(&mut self, now: Instant) -> bool {
        if self.machine.state() != SessionState::Recording {
            return false;
        }
        let Some(started) = self.segment_started else {
            return false;
        };
        let total = self.elapsed_base + now.saturating_duration_since(started);
        let seconds = total.as_secs().min(u64::from(u32::MAX)) as u32;
        if seconds == self.elapsed_seconds {
            return false;
        }
        self.elapsed_seconds = seconds;
        true
    }

    fn accrue_elapsed(&mut self, now: Instant) {
        if let Some(started) = self.segment_started.take() {
            self.elapsed_base += now.saturating_duration_since(started);
        }
        self.elapsed_seconds = self.elapsed_base.as_secs().min(u64::from(u32::MAX)) as u32;
    }

    /// Provider used for error attribution: the active session wins, then the
    /// persisted default.
    fn active_provider(&self) -> String {
        if let Some(provider) = self.machine.config().provider.clone() {
            return provider;
        }
        self.settings
            .as_ref()
            .map(|settings| settings.provider.clone())
            .unwrap_or_default()
    }

    // ----- Commands ----------------------------------------------------

    pub fn open_help(&mut self) {
        self.active_dialog = Some(Dialog::Help);
    }

    /// Opens Settings with an empty draft.
    ///
    /// [`Self::open_settings_with`] seeds a draft explicitly (for example from
    /// a future "edit current provider" entry point). The dialog deliberately
    /// starts from "Choose provider" so changing a provider always starts from
    /// the first catalogue entry, independent of any previously stored file.
    pub fn open_settings(&mut self) {
        self.open_settings_with(None, None, false);
    }

    /// Seed the Settings draft from persisted values.
    ///
    /// Unknown provider/model ids are ignored so corrupt state cannot make the
    /// dialog panic. `api_key_unchanged` means "a key is already stored";
    /// the field itself stays empty because the raw key never leaves the store.
    pub fn open_settings_with(
        &mut self,
        provider: Option<&str>,
        model: Option<&str>,
        api_key_unchanged: bool,
    ) {
        self.settings_draft = SettingsDraft {
            provider: None,
            model: None,
            api_key: String::new(),
            api_key_unchanged,
        };
        if let Some(provider) = provider
            && self.set_settings_provider(provider)
            && let Some(model) = model
        {
            self.set_settings_model(model);
        }
        self.active_dialog = Some(Dialog::Settings);
    }

    /// Dismisses the open dialog and discards its draft (Cancel semantics).
    pub fn close_dialog(&mut self) {
        self.active_dialog = None;
        self.settings_draft = SettingsDraft::default();
    }

    /// Same effect as Cancel, named for the Escape key path.
    pub fn dismiss_dialog_with_escape(&mut self) {
        self.close_dialog();
    }

    /// Returns `true` when the provider exists and was applied. Changing the
    /// provider clears a model that belongs to the previous one.
    pub fn set_settings_provider(&mut self, provider: &str) -> bool {
        if !self.provider_exists(provider) {
            return false;
        }
        if self.settings_draft.provider.as_deref() != Some(provider) {
            self.settings_draft.provider = Some(provider.to_string());
            self.settings_draft.model = None;
        }
        true
    }

    /// Returns `true` when the model belongs to the selected provider.
    pub fn set_settings_model(&mut self, model: &str) -> bool {
        let Some(provider) = self.settings_draft.provider.clone() else {
            return false;
        };
        if !self.model_exists(&provider, model) {
            return false;
        }
        self.settings_draft.model = Some(model.to_string());
        true
    }

    /// Mirrors typed input; clearing the field marks the draft invalid.
    pub fn set_settings_api_key(&mut self, value: impl Into<String>) {
        self.settings_draft.api_key = value.into();
        self.settings_draft.api_key_unchanged = false;
    }

    pub fn clear_settings_api_key(&mut self) {
        self.settings_draft.api_key.clear();
        self.settings_draft.api_key_unchanged = false;
    }

    /// Cycles the provider catalogue (the compact select control).
    pub fn cycle_settings_provider(&mut self) -> bool {
        let ids = self.provider_ids();
        let Some(next) = next_id(&ids, self.settings_draft.provider.as_deref()) else {
            return false;
        };
        self.set_settings_provider(&next)
    }

    /// Cycles the selected provider's models.
    pub fn cycle_settings_model(&mut self) -> bool {
        let Some(provider) = self.settings_draft.provider.clone() else {
            return false;
        };
        let ids = self.model_ids(&provider);
        let Some(next) = next_id(&ids, self.settings_draft.model.as_deref()) else {
            return false;
        };
        self.set_settings_model(&next)
    }

    /// Commits the Settings draft when valid and closes the dialog.
    ///
    /// Returns `None` and leaves the dialog open otherwise.
    pub fn save_settings(&mut self) -> Option<SettingsChange> {
        if !self.settings_draft_valid() {
            return None;
        }
        let change = SettingsChange {
            provider: self.settings_draft.provider.clone()?,
            model: self.settings_draft.model.clone()?,
            api_key: if self.settings_draft.api_key_unchanged {
                None
            } else {
                Some(self.settings_draft.api_key.clone())
            },
        };
        self.close_dialog();
        Some(change)
    }

    pub fn open_form(&mut self) {
        self.screen = Screen::Form;
        self.draft = SessionDraft::default();
        self.seed_draft_from_settings();
    }

    /// Pre-fill the New Session form from the persisted default provider and
    /// model, but only when both exist in the current catalogue. A fresh
    /// controller (no settings, or no model configured) starts empty.
    fn seed_draft_from_settings(&mut self) {
        // Never overwrite a choice the user already made in this draft.
        if self.draft.provider.is_some() {
            return;
        }
        let Some(settings) = self.settings.clone() else {
            return;
        };
        if settings.model.trim().is_empty() {
            return;
        }
        if self.provider_exists(&settings.provider)
            && self.model_exists(&settings.provider, &settings.model)
        {
            self.draft.provider = Some(settings.provider);
            self.draft.model = Some(settings.model);
        }
    }

    pub fn cancel_form(&mut self) {
        self.screen = Screen::Prompt;
        self.draft = SessionDraft::default();
    }

    pub fn cycle_provider(&mut self) {
        let ids = self.provider_ids();
        let Some(next) = next_id(&ids, self.draft.provider.as_deref()) else {
            return;
        };
        self.draft.provider = Some(next);
        // A model belongs to its provider: reset the dependent choice.
        self.draft.model = None;
    }

    pub fn cycle_model(&mut self) {
        let Some(provider) = self.draft.provider.clone() else {
            return;
        };
        let ids = self.model_ids(&provider);
        let Some(next) = next_id(&ids, self.draft.model.as_deref()) else {
            return;
        };
        self.draft.model = Some(next);
    }

    pub fn set_draft_name(&mut self, value: impl Into<String>) {
        self.draft.session_name = value.into();
    }

    pub fn set_draft_output(&mut self, output: OutputTarget) {
        self.draft.output = Some(output);
    }

    /// Commit the draft to the machine. Returns `false` (and changes nothing)
    /// while the draft is incomplete or the machine refuses a setup change.
    pub fn create_session(&mut self) -> bool {
        let Some(draft) = self.validate_draft() else {
            return false;
        };

        // An explicit file target (tests, a future picker) is kept; the
        // default location still follows the title.
        let output = match draft.output {
            OutputTarget::Memory => OutputTarget::Memory,
            OutputTarget::File(path) if path.trim().is_empty() => {
                OutputTarget::File(format!("~/Documents/{}.md", draft.session_name))
            }
            OutputTarget::File(path) => OutputTarget::File(path),
        };

        let setup = (|| -> Result<(), SessionError> {
            self.machine.set_provider(draft.provider)?;
            self.machine.set_model(draft.model)?;
            self.machine.set_session_name(draft.session_name)?;
            self.machine.set_output(output)?;
            Ok(())
        })();
        if setup.is_err() {
            return false;
        }

        self.screen = Screen::Prompt;
        self.draft = SessionDraft::default();
        true
    }

    /// Begins a take: reset telemetry, mint the session id and tell the
    /// backend to open a stream.
    pub fn start_recording(&mut self) -> bool {
        self.start_recording_at(Instant::now())
    }

    /// [`Self::start_recording`] with an injected clock reading.
    pub fn start_recording_at(&mut self, now: Instant) -> bool {
        if self.machine.state() != SessionState::Ready {
            return false;
        }
        let (provider, model, api_key) = match self.start_configuration() {
            Ok(configuration) => configuration,
            Err(provider) => {
                self.report_error(AppError::MissingApiKey { provider });
                return false;
            }
        };

        if self.machine.start().is_err() {
            return false;
        }

        self.session_id += 1;
        self.transcript.clear();
        self.waveform_bars = idle_waveform();
        self.elapsed_seconds = 0;
        self.elapsed_base = Duration::ZERO;
        self.segment_started = Some(now);
        self.last_error = None;
        self.app_error = None;
        self.output_status = None;
        self.written_path = None;

        let start = SttCommand::Start {
            session_id: self.session_label(),
            provider,
            model,
            sample_rate: SAMPLE_RATE,
            api_key,
        };
        self.last_start = Some(start.clone());
        self.send_command(start);

        true
    }

    /// Forward one capture chunk and its FFT frame. Ignored unless the take is
    /// actively recording, so paused audio never reaches the provider.
    pub fn push_audio_chunk(&mut self, pcm16: &[i16], bars: Arc<[f32]>) {
        if self.machine.state() != SessionState::Recording {
            return;
        }
        self.waveform_bars = bars;
        self.send_command(SttCommand::audio_from_pcm16(pcm16));
    }

    pub fn pause_recording(&mut self) -> bool {
        self.pause_recording_at(Instant::now())
    }

    /// [`Self::pause_recording`] with an injected clock reading.
    pub fn pause_recording_at(&mut self, now: Instant) -> bool {
        if self.machine.pause().is_err() {
            return false;
        }
        self.accrue_elapsed(now);
        self.segment_started = None;
        self.send_command(SttCommand::Pause);
        true
    }

    pub fn resume_recording(&mut self) -> bool {
        self.resume_recording_at(Instant::now())
    }

    /// [`Self::resume_recording`] with an injected clock reading.
    pub fn resume_recording_at(&mut self, now: Instant) -> bool {
        if self.machine.resume().is_err() {
            return false;
        }
        self.segment_started = Some(now);
        self.send_command(SttCommand::Resume);
        true
    }

    /// Ends the take, asks the backend to flush, and remembers the transcript
    /// for the `Stopped` screen.
    pub fn finish_take(&mut self) -> bool {
        let Ok(session) = self.machine.stop() else {
            return false;
        };
        self.accrue_elapsed(Instant::now());
        self.segment_started = None;
        self.send_command(SttCommand::Stop);

        let (committed, live) = self.finished_transcript();
        self.result = Some(FinishedTake {
            session,
            committed: committed.into(),
            live: live.into(),
        });
        self.written_path = None;
        self.output_status = None;
        true
    }

    /// Leave the `Stopped` screen and start over from `Blank`.
    pub fn new_take(&mut self) {
        self.result = None;
        self.screen = Screen::Prompt;
        self.draft = SessionDraft::default();
        self.transcript.clear();
        self.waveform_bars = idle_waveform();
        self.elapsed_seconds = 0;
        self.elapsed_base = Duration::ZERO;
        self.segment_started = None;
        self.last_error = None;
        self.app_error = None;
        self.output_status = None;
        self.written_path = None;
    }

    /// Apply a hotkey action through the same guards as its button.
    ///
    /// Returns `true` only when the action actually changed state. Invalid
    /// actions (for example Stop while Recording) are inert.
    pub fn apply_action(&mut self, action: SessionAction) -> bool {
        match action {
            SessionAction::StartRecording => self.start_recording(),
            SessionAction::PauseRecording => self.pause_recording(),
            SessionAction::ResumeRecording => self.resume_recording(),
            SessionAction::StopRecording => {
                if self.machine.state() == SessionState::Paused {
                    self.finish_take()
                } else {
                    false
                }
            }
            SessionAction::NewTake => {
                if self.result.is_some() {
                    self.new_take();
                    true
                } else {
                    false
                }
            }
            SessionAction::ToggleHelp => {
                self.open_help();
                true
            }
            SessionAction::ToggleSettings => {
                self.open_settings();
                true
            }
        }
    }

    /// Re-issue `Start` for the current take after a retryable failure.
    pub fn retry_start(&mut self) -> bool {
        if !self.machine.state().is_active() {
            return false;
        }
        let Some(start) = self.last_start.clone() else {
            return false;
        };
        self.dismiss_error();
        self.send_command(start);
        true
    }

    // ----- Output actions ----------------------------------------------

    /// Copy the finished take exactly as shown and record "Copied".
    pub fn copy_finished_transcript(
        &mut self,
        clipboard: &mut dyn ClipboardSink,
    ) -> Result<CopySummary, AppError> {
        let Some(result) = self.result.as_ref() else {
            return Err(self.fail(AppError::ClipboardFailed));
        };
        let committed = result.committed.to_string();
        let live = result.live.to_string();

        match copy_transcript(clipboard, &committed, &live) {
            Ok(summary) => {
                self.output_status = Some("Copied".into());
                Ok(summary)
            }
            Err(error) => Err(self.fail(AppError::from_output(error))),
        }
    }

    /// Save a memory take as Markdown in `directory`.
    pub fn save_finished_transcript(
        &mut self,
        directory: &Path,
        now: SystemTime,
    ) -> Result<SavedFile, AppError> {
        let Some(result) = self.result.as_ref() else {
            return Err(self.fail(AppError::SaveFailed("no finished take".to_owned())));
        };
        let session_name = result.session.session_name.clone();
        let text = joined_text(&result.committed, &result.live);

        match save_transcript_at(
            directory,
            &session_name,
            TranscriptFormat::Markdown,
            &text,
            now,
        ) {
            Ok(saved) => {
                self.written_path = Some(saved.path.clone());
                self.output_status = Some("Saved".into());
                Ok(saved)
            }
            Err(error) => Err(self.fail(AppError::from_output(error))),
        }
    }

    /// Write a file take according to its `OutputTarget::File` directory and
    /// extension. `~` is expanded against `$HOME`; the UI only calls this in
    /// production.
    pub fn save_finished_file(&mut self, now: SystemTime) -> Result<SavedFile, AppError> {
        let Some(result) = self.result.as_ref() else {
            return Err(self.fail(AppError::SaveFailed("no finished take".to_owned())));
        };
        let OutputTarget::File(target) = &result.session.output else {
            return Err(self.fail(AppError::SaveFailed(
                "take does not target a file".to_owned(),
            )));
        };
        let target = expand_tilde(target);
        let extension = target
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or("md");
        let Some(format) = TranscriptFormat::from_extension(extension) else {
            return Err(self.fail(AppError::SaveFailed(format!(
                "unsupported transcript format: {extension}"
            ))));
        };
        let directory = target
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let session_name = result.session.session_name.clone();
        let text = joined_text(&result.committed, &result.live);

        match save_transcript_at(directory, &session_name, format, &text, now) {
            Ok(saved) => {
                self.written_path = Some(saved.path.clone());
                self.output_status = Some("Saved".into());
                Ok(saved)
            }
            Err(error) => Err(self.fail(AppError::from_output(error))),
        }
    }

    /// Open the last written file (or the file take's target).
    pub fn open_saved_file(&mut self, opener: &mut dyn FileOpener) -> Result<(), AppError> {
        let path = self.written_path.clone().or_else(|| {
            let result = self.result.as_ref()?;
            match &result.session.output {
                OutputTarget::File(path) => Some(expand_tilde(path)),
                OutputTarget::Memory => None,
            }
        });
        let Some(path) = path else {
            return Err(self.fail(AppError::SaveFailed("no saved file".to_owned())));
        };

        match opener.open(&path) {
            Ok(()) => {
                self.output_status = Some("Opened".into());
                Ok(())
            }
            Err(error) => Err(self.fail(AppError::from_output(error))),
        }
    }

    /// Copy and paste the finished take when `auto_paste` is enabled.
    ///
    /// Returns `None` when auto-paste is off; failures are recorded but are
    /// never fatal to the finished take.
    pub fn auto_paste_after_finish(
        &mut self,
        clipboard: &mut dyn ClipboardSink,
        paste: &mut dyn PasteSink,
    ) -> Option<Result<(), AppError>> {
        if !self.auto_paste_enabled() {
            return None;
        }
        self.result.as_ref()?;

        Some(match self.copy_finished_transcript(clipboard) {
            Ok(_) => match paste.paste() {
                Ok(()) => {
                    self.output_status = Some("Pasted".into());
                    Ok(())
                }
                Err(error) => Err(self.fail(AppError::from_output(error))),
            },
            Err(error) => Err(error),
        })
    }

    /// Whether the persisted settings ask for automatic paste.
    #[must_use]
    pub fn auto_paste_enabled(&self) -> bool {
        self.settings
            .as_ref()
            .is_some_and(|settings| settings.auto_paste && !settings.model.trim().is_empty())
    }

    // ----- Internals ---------------------------------------------------

    fn accepts_session(&self, session_id: Option<&str>) -> bool {
        match session_id {
            None => true,
            Some(id) => self.session_id > 0 && id == self.session_label(),
        }
    }

    /// Provider/model/key for `Start`, or the provider that is missing a key.
    ///
    /// The session's frozen machine configuration wins; persisted settings
    /// fill any gap. When no settings were ever loaded (unit tests), the key
    /// stays `None` and the worker falls back to its environment.
    fn start_configuration(&self) -> Result<(String, String, Option<SecretString>), String> {
        let config = self.machine.config();
        let provider = config
            .provider
            .clone()
            .filter(|provider| !provider.trim().is_empty())
            .or_else(|| {
                self.settings
                    .as_ref()
                    .map(|settings| settings.provider.clone())
            })
            .unwrap_or_default();
        let model = config
            .model
            .clone()
            .filter(|model| !model.trim().is_empty())
            .or_else(|| {
                self.settings
                    .as_ref()
                    .map(|settings| settings.model.clone())
            })
            .unwrap_or_default();

        let Some(settings) = &self.settings else {
            return Ok((provider, model, None));
        };
        let Some(key) = settings.api_key(&provider) else {
            return Err(provider);
        };
        Ok((provider, model, Some(SecretString::new(key))))
    }

    /// Resolve the draft against the active catalogue.
    fn validate_draft(&self) -> Option<ValidatedDraft> {
        let provider = self.draft.provider.clone()?;
        let model = self.draft.model.clone()?;
        if !self.provider_exists(&provider) || !self.model_exists(&provider, &model) {
            return None;
        }
        let output = self.draft.output.clone()?;
        let session_name = self.draft.session_name.trim().to_string();
        if session_name.is_empty() {
            return None;
        }
        Some(ValidatedDraft {
            provider,
            model,
            session_name,
            output,
        })
    }

    fn fail(&mut self, error: AppError) -> AppError {
        self.report_error(error.clone());
        error
    }

    fn send_command(&mut self, command: SttCommand) {
        let Some(sink) = self.sink.as_mut() else {
            return;
        };
        if let Err(error) = sink.send(command) {
            self.report_error(AppError::from_backend(&error));
        }
    }

    /// Transcript for the stopped screen. With no backend attached the widget
    /// is an offline demo, so it keeps the mock take it showed before T0002.
    fn finished_transcript(&self) -> (String, String) {
        if self.sink.is_none() && self.transcript.full_text().is_empty() {
            (MOCK_COMMITTED.to_string(), MOCK_LIVE.to_string())
        } else {
            (
                self.transcript.committed().to_string(),
                self.transcript.live().to_string(),
            )
        }
    }
}

/// Join the two transcript parts the same way the clipboard adapter does.
fn joined_text(committed: &str, live: &str) -> String {
    let committed = committed.trim();
    let live = live.trim();
    match (committed.is_empty(), live.is_empty()) {
        (true, true) => String::new(),
        (true, false) => live.to_owned(),
        (false, true) => committed.to_owned(),
        (false, false) => format!("{committed} {live}"),
    }
}

/// Next catalogue id after `current`, wrapping to the first entry.
fn next_id(ids: &[String], current: Option<&str>) -> Option<String> {
    if ids.is_empty() {
        return None;
    }
    let index = current
        .and_then(|current| ids.iter().position(|id| id == current))
        .map_or(0, |index| (index + 1) % ids.len());
    Some(ids[index].clone())
}

/// Expand a leading `~` to `$HOME`. Paths without it are returned unchanged.
fn expand_tilde(path: &str) -> PathBuf {
    let Some(rest) = path.strip_prefix("~/") else {
        return PathBuf::from(path);
    };
    match std::env::var_os("HOME") {
        Some(home) => PathBuf::from(home).join(rest),
        None => PathBuf::from(path),
    }
}

fn idle_waveform() -> Arc<[f32]> {
    vec![0.0; WAVEFORM_BARS].into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use speechtt_core::protocol::ModelCapabilityInfo;

    /// Minimal catalogue mirroring the worker handshake, sorted by label like
    /// `listCapabilities`.
    fn test_capabilities() -> Vec<ProviderCapability> {
        let provider =
            |id: &str, label: &str, key: &str, models: Vec<(&str, &str, TranscriptionMode)>| {
                ProviderCapability {
                    id: id.to_string(),
                    label: label.to_string(),
                    available: false,
                    key_env_vars: vec![key.to_string()],
                    models: models
                        .into_iter()
                        .map(|(id, label, mode)| ModelCapabilityInfo {
                            id: id.to_string(),
                            label: label.to_string(),
                            mode,
                        })
                        .collect(),
                }
            };

        vec![
            provider(
                "elevenlabs",
                "ElevenLabs",
                "ELEVENLABS_API_KEY",
                vec![
                    ("scribe_v2", "Scribe v2", TranscriptionMode::Segmented),
                    (
                        "scribe_v2_realtime",
                        "Scribe v2 Realtime",
                        TranscriptionMode::Streaming,
                    ),
                ],
            ),
            provider(
                "mistral",
                "Mistral AI",
                "MISTRAL_API_KEY",
                vec![
                    (
                        "voxtral-mini-transcribe-3-0",
                        "Voxtral Mini Transcribe 3.0",
                        TranscriptionMode::Segmented,
                    ),
                    (
                        "voxtral-small-transcribe-3-0",
                        "Voxtral Small Transcribe 3.0",
                        TranscriptionMode::Segmented,
                    ),
                ],
            ),
            provider(
                "openai",
                "OpenAI",
                "OPENAI_API_KEY",
                vec![
                    (
                        "gpt-realtime-whisper",
                        "GPT Realtime Whisper",
                        TranscriptionMode::Streaming,
                    ),
                    (
                        "gpt-4o-transcribe",
                        "GPT-4o Transcribe",
                        TranscriptionMode::Segmented,
                    ),
                    ("whisper-1", "Whisper 1", TranscriptionMode::Segmented),
                ],
            ),
            ProviderCapability {
                id: "vercel-ai-gateway".to_string(),
                label: "Vercel AI Gateway".to_string(),
                available: false,
                key_env_vars: vec![
                    "AI_GATEWAY_API_KEY".to_string(),
                    "VERCEL_AI_GATEWAY_API_KEY".to_string(),
                ],
                models: vec![
                    ModelCapabilityInfo {
                        id: "fish-audio/transcribe-1".to_string(),
                        label: "Transcribe-1".to_string(),
                        mode: TranscriptionMode::Segmented,
                    },
                    ModelCapabilityInfo {
                        id: "openai/gpt-4o-transcribe".to_string(),
                        label: "GPT-4o Transcribe".to_string(),
                        mode: TranscriptionMode::Segmented,
                    },
                    ModelCapabilityInfo {
                        id: "openai/gpt-realtime-whisper".to_string(),
                        label: "GPT Realtime Whisper".to_string(),
                        mode: TranscriptionMode::Streaming,
                    },
                ],
            },
            provider(
                "xai",
                "xAI",
                "XAI_API_KEY",
                vec![("grok-stt", "Grok STT", TranscriptionMode::Streaming)],
            ),
        ]
    }

    /// Installs the catalogue the worker would send.
    fn install_catalogue(controller: &mut SessionController) {
        controller.set_capabilities(test_capabilities());
    }

    fn complete_controller() -> SessionController {
        let mut controller = SessionController::new();
        install_catalogue(&mut controller);
        controller.open_form();
        controller.cycle_provider();
        controller.cycle_model();
        controller.set_draft_output(OutputTarget::Memory);
        controller.set_draft_name("meeting notes");
        assert!(controller.create_session());
        controller
    }

    fn valid_settings(controller: &mut SessionController) -> bool {
        install_catalogue(controller);
        controller.open_settings();
        controller.set_settings_provider("vercel-ai-gateway");
        controller.set_settings_model("openai/gpt-realtime-whisper");
        controller.set_settings_api_key("sk-test-key");
        controller.settings_draft_valid()
    }

    #[test]
    fn only_one_dialog_can_be_open_at_a_time() {
        let mut controller = SessionController::new();
        assert_eq!(controller.active_dialog(), None);

        controller.open_help();
        assert_eq!(controller.active_dialog(), Some(Dialog::Help));

        controller.open_settings();
        assert_eq!(controller.active_dialog(), Some(Dialog::Settings));

        controller.close_dialog();
        assert_eq!(controller.active_dialog(), None);
    }

    #[test]
    fn escape_dismisses_the_open_dialog() {
        let mut controller = SessionController::new();
        controller.open_help();
        controller.dismiss_dialog_with_escape();
        assert_eq!(controller.active_dialog(), None);
    }

    #[test]
    fn settings_draft_starts_invalid_and_requires_a_key() {
        let mut controller = SessionController::new();
        install_catalogue(&mut controller);
        controller.open_settings();
        assert!(!controller.settings_draft_valid());

        assert!(controller.set_settings_provider("vercel-ai-gateway"));
        assert!(!controller.settings_draft_valid());
        assert!(controller.set_settings_model("openai/gpt-realtime-whisper"));
        assert!(!controller.settings_draft_valid(), "no key yet");

        controller.set_settings_api_key("sk-test-key");
        assert!(controller.settings_draft_valid());
    }

    #[test]
    fn an_untouched_stored_key_makes_the_draft_valid() {
        let mut controller = SessionController::new();
        install_catalogue(&mut controller);
        controller.open_settings_with(Some("mistral"), Some("voxtral-mini-transcribe-3-0"), true);
        assert!(controller.settings_draft_valid());
        assert_eq!(controller.settings_draft().api_key, "");
    }

    #[test]
    fn changing_the_provider_clears_its_model() {
        let mut controller = SessionController::new();
        install_catalogue(&mut controller);
        controller.open_settings();
        assert!(controller.set_settings_provider("vercel-ai-gateway"));
        assert!(controller.set_settings_model("openai/gpt-realtime-whisper"));

        assert!(controller.set_settings_provider("mistral"));
        assert_eq!(
            controller.settings_draft().provider.as_deref(),
            Some("mistral")
        );
        assert_eq!(controller.settings_draft().model, None);
        assert!(!controller.set_settings_model("openai/gpt-realtime-whisper"));
    }

    #[test]
    fn save_returns_the_change_and_closes_the_dialog() {
        let mut controller = SessionController::new();
        assert!(valid_settings(&mut controller));

        let change = controller.save_settings().expect("valid draft saves");
        assert_eq!(
            change,
            SettingsChange {
                provider: "vercel-ai-gateway".to_string(),
                model: "openai/gpt-realtime-whisper".to_string(),
                api_key: Some("sk-test-key".to_string()),
            }
        );
        assert_eq!(controller.active_dialog(), None);
    }

    #[test]
    fn an_unchanged_stored_key_is_sent_as_none() {
        let mut controller = SessionController::new();
        install_catalogue(&mut controller);
        controller.open_settings_with(
            Some("vercel-ai-gateway"),
            Some("openai/gpt-4o-transcribe"),
            true,
        );

        let change = controller.save_settings().expect("valid draft saves");
        assert_eq!(change.api_key, None);
    }

    #[test]
    fn an_invalid_draft_cannot_be_saved_and_keeps_the_dialog_open() {
        let mut controller = SessionController::new();
        controller.open_settings();
        assert_eq!(controller.save_settings(), None);
        assert_eq!(controller.active_dialog(), Some(Dialog::Settings));
    }

    #[test]
    fn settings_debug_output_never_contains_the_key() {
        let mut controller = SessionController::new();
        install_catalogue(&mut controller);
        controller.open_settings();
        controller.set_settings_provider("vercel-ai-gateway");
        controller.set_settings_model("openai/gpt-realtime-whisper");
        controller.set_settings_api_key("sk-super-secret-value");

        let draft = format!("{:?}", controller.settings_draft());
        assert!(!draft.contains("sk-super-secret-value"), "{draft}");
        assert!(draft.contains("REDACTED"), "{draft}");

        let change = controller.save_settings().expect("valid draft saves");
        let change = format!("{change:?}");
        assert!(!change.contains("sk-super-secret-value"), "{change}");
        assert!(change.contains("REDACTED"), "{change}");
    }

    #[test]
    fn opening_settings_during_recording_does_not_touch_the_session() {
        let mut controller = complete_controller();
        assert!(controller.start_recording());
        let before = controller.session_state();

        controller.open_settings();
        assert_eq!(controller.active_dialog(), Some(Dialog::Settings));
        assert_eq!(controller.session_state(), before);

        controller.close_dialog();
        assert_eq!(controller.session_state(), before);
    }

    #[test]
    fn model_modes_come_from_the_catalogue() {
        let mut controller = SessionController::new();
        install_catalogue(&mut controller);
        controller.open_settings();
        assert!(controller.set_settings_provider("vercel-ai-gateway"));

        assert!(controller.set_settings_model("openai/gpt-realtime-whisper"));
        assert_eq!(
            controller.settings_model_mode(),
            Some(TranscriptionMode::Streaming)
        );

        assert!(controller.set_settings_model("openai/gpt-4o-transcribe"));
        assert_eq!(
            controller.settings_model_mode(),
            Some(TranscriptionMode::Segmented)
        );
        assert_eq!(controller.session_model_mode(), None, "no take yet");
    }

    #[test]
    fn the_setup_draft_exposes_its_model_mode() {
        let mut controller = SessionController::new();
        install_catalogue(&mut controller);
        controller.open_form();

        assert_eq!(controller.draft_model_mode(), None);
        controller.cycle_provider();
        assert_eq!(controller.draft_model_mode(), None, "no model yet");

        controller.cycle_model();
        assert_eq!(
            controller.draft_model_mode(),
            Some(TranscriptionMode::Segmented)
        );

        controller.cycle_model();
        assert_eq!(
            controller.draft_model_mode(),
            Some(TranscriptionMode::Streaming)
        );
    }

    #[test]
    fn the_active_take_exposes_its_model_mode() {
        let mut controller = complete_controller();
        assert_eq!(controller.session_model_mode(), None, "no take yet");

        assert!(controller.start_recording());
        assert_eq!(
            controller.session_model_mode(),
            Some(TranscriptionMode::Segmented),
            "the first catalogue model is batch"
        );
    }

    #[test]
    fn a_discovered_catalogue_replaces_the_builtin_one() {
        let mut controller = SessionController::new();
        assert_eq!(controller.catalogue_source(), None);

        controller.apply_capabilities(test_capabilities(), CapabilitySource::Discovered);

        assert!(controller.catalogue_loaded());
        assert_eq!(
            controller.catalogue_source(),
            Some(CapabilitySource::Discovered)
        );
        assert_eq!(
            controller.settings_api_key_env(),
            None,
            "the dialog has no provider selected yet"
        );
    }

    #[test]
    fn the_catalogue_comes_from_the_handshake_only() {
        let mut controller = SessionController::new();
        assert!(!controller.catalogue_loaded());
        assert_eq!(controller.settings_provider_label(), None);

        install_catalogue(&mut controller);
        assert!(controller.catalogue_loaded());
        assert_eq!(
            controller.catalogue_source(),
            Some(CapabilitySource::Builtin)
        );
        let ids: Vec<_> = controller
            .capabilities()
            .expect("catalogue installed")
            .iter()
            .map(|option| option.id.as_str())
            .collect();
        assert_eq!(
            ids,
            [
                "elevenlabs",
                "mistral",
                "openai",
                "vercel-ai-gateway",
                "xai"
            ]
        );
    }

    #[test]
    fn the_api_key_row_names_the_selected_providers_variable() {
        let mut controller = SessionController::new();
        install_catalogue(&mut controller);
        controller.open_settings();
        assert_eq!(controller.settings_api_key_env(), None);

        assert!(controller.set_settings_provider("vercel-ai-gateway"));
        assert_eq!(
            controller.settings_api_key_env(),
            Some("AI_GATEWAY_API_KEY")
        );

        assert!(controller.set_settings_provider("mistral"));
        assert_eq!(controller.settings_api_key_env(), Some("MISTRAL_API_KEY"));
    }

    #[test]
    fn cycling_selects_a_provider_and_then_its_first_model() {
        let mut controller = SessionController::new();
        install_catalogue(&mut controller);
        controller.open_settings();

        assert!(controller.cycle_settings_provider());
        assert_eq!(
            controller.settings_draft().provider.as_deref(),
            Some("elevenlabs")
        );
        assert_eq!(controller.settings_draft().model, None);

        assert!(controller.cycle_settings_model());
        assert_eq!(
            controller.settings_draft().model.as_deref(),
            Some("scribe_v2")
        );
    }
}
