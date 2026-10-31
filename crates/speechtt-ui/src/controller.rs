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
use speechtt_core::media::is_media_path;
use speechtt_core::output::OutputError;
use speechtt_core::output::clipboard::{ClipboardSink, CopySummary, copy_transcript};
use speechtt_core::output::file::{FileOpener, SavedFile, TranscriptFormat, save_transcript_to};
use speechtt_core::output::paste::PasteSink;
use std::collections::BTreeMap;

use speechtt_core::protocol::{
    CapabilitySource, ModelCapabilityInfo, ProviderCapability, SecretString, SttCommand, SttEvent,
    TranscriptionMode,
};
use speechtt_core::session::{FinishedSession, SessionField, SessionMachine, SessionState};
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
    MissingApiKey {
        provider: String,
    },
    BackendUnavailable,
    BackendTimeout,
    ProviderAuthFailed,
    NetworkLost,
    SaveFailed(String),
    ClipboardFailed,
    PasteFailed,
    ProtocolError(String),
    /// Worker reported a failure that is not one of the canonical buckets.
    BackendFailed {
        code: String,
        detail: String,
    },
    /// Copy/save was requested for an empty take; informational, not an error.
    NothingRecorded,
    /// FileModelNotSupported: a streaming-only model was chosen for a file take.
    FileModelNotSupported,
    /// The picked path is not an audio or video file.
    UnsupportedFileType,
    /// The file picked for transcription no longer exists.
    FileNotFound,
    /// The picked path is a directory, unreadable, or not decodable.
    FileUnreadable,
    /// `ffmpeg`/`ffprobe` are not installed on the worker's host.
    FfmpegMissing,
    /// The native file picker could not be opened.
    FileDialogFailed,
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
    /// text. Dynamic causes are sanitized and the whole line stays within the
    /// 160-character strip budget.
    #[must_use]
    pub fn message(&self) -> String {
        let message = match self {
            Self::MicPermissionDenied => {
                "Microphone access is denied. Open your system sound settings to allow it."
                    .to_owned()
            }
            Self::NoInputDevice => "No microphone is available.".to_owned(),
            Self::MissingApiKey { .. } => {
                "Add an API key for this provider in Settings.".to_owned()
            }
            Self::BackendUnavailable => "The transcription backend is unavailable.".to_owned(),
            Self::BackendTimeout => "The transcription backend did not respond in time.".to_owned(),
            Self::ProviderAuthFailed => {
                "The provider rejected the API key. Update it in Settings.".to_owned()
            }
            Self::NetworkLost => "The network connection was lost.".to_owned(),
            Self::SaveFailed(cause) => {
                format!("Could not save the transcript: {}.", sentence(cause))
            }
            Self::ClipboardFailed => "Could not copy the transcript.".to_owned(),
            Self::PasteFailed => "Could not paste the transcript.".to_owned(),
            Self::ProtocolError(detail) => format!(
                "The transcription backend sent an unexpected message: {}.",
                sentence(detail)
            ),
            Self::BackendFailed { detail, .. } => {
                format!("The transcription backend failed: {}.", sentence(detail))
            }
            Self::NothingRecorded => "Nothing was recorded.".to_owned(),
            Self::FileModelNotSupported => {
                "File transcription needs a non-streaming model.".to_owned()
            }
            Self::UnsupportedFileType => "That is not an audio or video file.".to_owned(),
            Self::FileNotFound => "That file no longer exists. Pick another one.".to_owned(),
            Self::FileUnreadable => {
                "That file could not be read. Pick an audio or video file.".to_owned()
            }
            Self::FfmpegMissing => {
                "Transcribing files needs ffmpeg. Install it, then try again.".to_owned()
            }
            Self::FileDialogFailed => "Could not open the file picker.".to_owned(),
        };
        clamp_message(message, 160)
    }

    /// The primary action the strip should offer.
    #[must_use]
    pub fn action(&self) -> ErrorAction {
        match self {
            Self::MissingApiKey { .. } | Self::ProviderAuthFailed | Self::FileModelNotSupported => {
                ErrorAction::OpenSettings
            }
            Self::BackendUnavailable | Self::BackendTimeout | Self::NetworkLost => {
                ErrorAction::Retry
            }
            Self::MicPermissionDenied => ErrorAction::SettingsHint,
            Self::NoInputDevice
            | Self::SaveFailed(_)
            | Self::ClipboardFailed
            | Self::PasteFailed
            | Self::ProtocolError(_)
            | Self::BackendFailed { .. }
            | Self::NothingRecorded
            | Self::UnsupportedFileType
            | Self::FileNotFound
            | Self::FileUnreadable
            | Self::FfmpegMissing
            | Self::FileDialogFailed => ErrorAction::Dismiss,
        }
    }

    /// Optional diagnostic detail for logs; never rendered unsanitized.
    #[must_use]
    pub fn detail(&self) -> Option<String> {
        match self {
            Self::SaveFailed(detail) | Self::ProtocolError(detail) => {
                Some(non_empty_detail(detail))
            }
            Self::BackendFailed { detail, .. } => Some(non_empty_detail(detail)),
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
                Self::ProtocolError(non_empty_detail(&error.to_string()))
            }
            // `BackendError` is `#[non_exhaustive]`: future transport
            // failures stay retryable until they get a specific mapping.
            _ => Self::BackendUnavailable,
        }
    }

    /// Map one `SttEvent::Error` payload. Codes are canonical (see
    /// `src/stt/protocol.ts`); the sanitized worker message is retained as the
    /// diagnostic detail so an unknown failure still names its cause.
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
            "file_not_found" => Self::FileNotFound,
            "unsupported_file" | "decode_failed" => Self::FileUnreadable,
            "unsupported_file_model" => Self::FileModelNotSupported,
            "ffmpeg_missing" => Self::FfmpegMissing,
            "invalid_protocol" => Self::ProtocolError(non_empty_detail(message)),
            "stream_failed" => {
                let lowered = message.to_ascii_lowercase();
                if contains_any(&lowered, &["auth", "api key", "unauthorized", "forbidden"]) {
                    Self::ProviderAuthFailed
                } else if contains_any(
                    &lowered,
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
                    Self::BackendFailed {
                        code: code.to_owned(),
                        detail: non_empty_detail(message),
                    }
                }
            }
            _ => Self::BackendFailed {
                code: code.to_owned(),
                detail: non_empty_detail(message),
            },
        }
    }

    /// Map a settings store failure.
    #[must_use]
    pub fn from_settings(error: &SettingsError) -> Self {
        match error {
            SettingsError::Io { .. } | SettingsError::Encode(_) => {
                Self::SaveFailed(save_cause(&error.to_string()))
            }
            SettingsError::Corrupt { .. } | SettingsError::Invalid(_) => {
                Self::ProtocolError(non_empty_detail(&error.to_string()))
            }
            _ => Self::SaveFailed(save_cause(&error.to_string())),
        }
    }

    /// Map an output adapter failure.
    #[must_use]
    pub fn from_output(error: OutputError) -> Self {
        match error {
            OutputError::Clipboard(_) => Self::ClipboardFailed,
            OutputError::Paste(_) => Self::PasteFailed,
            // An empty take is a normal outcome, not a save failure.
            OutputError::EmptyTranscript => Self::NothingRecorded,
            OutputError::UnsupportedFormat(_) => Self::SaveFailed("unsupported format".to_owned()),
            OutputError::Write(detail) | OutputError::OpenFile(detail) => {
                Self::SaveFailed(save_cause(&detail))
            }
        }
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message())
    }
}

impl std::error::Error for AppError {}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
}

/// Reduce an untrusted detail to one safe, bounded line.
///
/// Whitespace and control characters collapse, credential patterns become
/// `[REDACTED]`, and the result never exceeds 120 characters.
#[must_use]
pub fn sanitize_detail(raw: &str) -> String {
    if raw.is_empty() {
        return String::new();
    }

    let mut collapsed = String::with_capacity(raw.len());
    let mut previous_was_space = false;
    for character in raw.chars() {
        if character.is_control() || character.is_whitespace() {
            if !previous_was_space {
                collapsed.push(' ');
                previous_was_space = true;
            }
        } else {
            collapsed.push(character);
            previous_was_space = false;
        }
    }

    let redacted = redact_secrets(collapsed.trim());
    if redacted.is_empty() {
        return "no detail provided".to_owned();
    }
    truncate_chars(&redacted, 120)
}

/// A sanitized detail without trailing terminal punctuation, so composing a
/// message sentence never doubles the full stop.
fn sentence(raw: &str) -> String {
    let detail = non_empty_detail(raw);
    detail
        .trim_end_matches(|character: char| character == '.' || character.is_whitespace())
        .to_owned()
}

/// A detail that was empty or fully redacted still names its absence.
fn non_empty_detail(raw: &str) -> String {
    let sanitized = sanitize_detail(raw);
    if sanitized.is_empty() {
        "no detail provided".to_owned()
    } else {
        sanitized
    }
}

/// Short cause for a save failure, so a raw OS dump never reaches the strip.
fn save_cause(raw: &str) -> String {
    let lowered = raw.to_ascii_lowercase();
    let cause = if contains_any(&lowered, &["permission denied", "os error 13"]) {
        "permission denied"
    } else if contains_any(&lowered, &["no space left", "os error 28"]) {
        "no space left"
    } else if contains_any(&lowered, &["read-only file system", "os error 30"]) {
        "read-only file system"
    } else if contains_any(&lowered, &["is a directory", "os error 21"]) {
        "not a file"
    } else {
        return non_empty_detail(raw);
    };
    cause.to_owned()
}

/// Replace common credential shapes with `[REDACTED]`.
fn redact_secrets(input: &str) -> String {
    let lowered = input.to_ascii_lowercase();
    let mut output = String::with_capacity(input.len());
    let mut index = 0;

    while index < input.len() {
        let rest = &lowered[index..];
        if rest.starts_with("authorization:") {
            output.push_str("[REDACTED]");
            index = input.len();
        } else if rest.starts_with("bearer ") {
            output.push_str("[REDACTED]");
            index = skip_secret_value(input, index + "bearer ".len());
        } else if rest.starts_with("api_key=") {
            output.push_str("[REDACTED]");
            index = skip_secret_value(input, index + "api_key=".len());
        } else if rest.starts_with("key=") {
            output.push_str("[REDACTED]");
            index = skip_secret_value(input, index + "key=".len());
        } else if rest.starts_with("sk-") {
            output.push_str("[REDACTED]");
            index = skip_secret_value(input, index);
        } else {
            let character = input[index..].chars().next().expect("non-empty tail");
            output.push(character);
            index += character.len_utf8();
        }
    }

    output
}

/// End of a credential token: whitespace or common separator punctuation.
fn skip_secret_value(input: &str, start: usize) -> usize {
    let mut index = start;
    for character in input[start..].chars() {
        if character.is_whitespace()
            || character.is_control()
            || matches!(character, ',' | ';' | '&' | '\'' | '"' | ')' | ']')
        {
            break;
        }
        index += character.len_utf8();
    }
    index
}

/// Keep at most `max` characters, ending with `…` when truncation happened.
fn truncate_chars(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let mut truncated: String = text.chars().take(max.saturating_sub(1)).collect();
    truncated.push('…');
    truncated
}

fn clamp_message(message: String, max: usize) -> String {
    if message.chars().count() <= max {
        message
    } else {
        truncate_chars(&message, max)
    }
}

/// Which setup screen the body shows while the machine is `Blank`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Screen {
    /// The mode chooser: live dictation or from a file.
    Mode,
    /// Provider + model, shown only when the settings do not resolve.
    Form,
}

/// How a take gets its audio: the microphone or an existing file.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SessionMode {
    #[default]
    Live,
    File,
}

/// What the widget is currently displaying. Derived from the machine, the
/// setup screen and the finished-take overlay.
///
/// There is deliberately no `Ready` and no `Paused` screen: a complete
/// configuration shows the live take screen directly, and pause/resume are
/// states of that one screen (T0021/T0022).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewState {
    Mode,
    Form,
    Live,
    Stopped,
}

/// The focused overlay currently open, if any. Only one dialog at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialog {
    Help,
    Settings,
}

/// Human-readable price lines for one model, in a stable order.
///
/// The gateway publishes per-token prices for some models and a per-second
/// audio price for others, each as a string; the UI owns the formatting.
#[must_use]
pub fn pricing_lines(pricing: &BTreeMap<String, String>) -> Vec<String> {
    if pricing.is_empty() {
        return vec!["Price not published".to_owned()];
    }

    let mut lines = Vec::new();
    let mut saw_known = false;
    let mut any_nonzero = false;

    if let Some(raw) = pricing.get("transcription_duration_cost_per_second") {
        saw_known = true;
        match raw.trim().parse::<f64>() {
            Ok(0.0) => {}
            Ok(value) => {
                any_nonzero = true;
                lines.push(format!("${} / second of audio", format_seconds(value)));
            }
            Err(_) => {
                any_nonzero = true;
                lines.push(format!("${raw} / second of audio"));
            }
        }
    }

    for (key, unit) in [("input", "input"), ("output", "output")] {
        let Some(raw) = pricing.get(key) else {
            continue;
        };
        saw_known = true;
        match raw.trim().parse::<f64>() {
            Ok(0.0) => {}
            Ok(value) => {
                any_nonzero = true;
                lines.push(format!("${:.2} / M {unit} tokens", value * 1_000_000.0));
            }
            Err(_) => {
                any_nonzero = true;
                lines.push(format!("${raw} / M {unit} tokens"));
            }
        }
    }

    for (key, raw) in pricing {
        if matches!(
            key.as_str(),
            "transcription_duration_cost_per_second" | "input" | "output"
        ) {
            continue;
        }
        lines.push(format!("{}: {raw}", humanize_price_key(key)));
    }

    if saw_known && !any_nonzero && lines.is_empty() {
        return vec!["Free".to_owned()];
    }
    lines
}

/// Six decimals is the gateway's precision for per-second audio prices;
/// trailing zeros are trimmed so `0.000030556` renders as `$0.000031`.
fn format_seconds(value: f64) -> String {
    let mut text = format!("{value:.6}");
    while text.ends_with('0') && !text.ends_with(".00") {
        text.pop();
    }
    text
}

/// `transcription_duration_cost_per_second` -> `Transcription duration cost per second`.
fn humanize_price_key(key: &str) -> String {
    let mut chars = key.replace('_', " ").chars().collect::<Vec<_>>();
    if let Some(first) = chars.first_mut() {
        *first = first.to_uppercase().next().unwrap_or(*first);
    }
    chars.into_iter().collect()
}

/// Which page the Help dialog shows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum HelpTab {
    /// Keyboard shortcuts (the historical content).
    #[default]
    Shortcuts,
    /// The discovered model catalogue with description and pricing.
    Models,
}

impl HelpTab {
    /// Stable tab index used by the UI's `TabBar`.
    #[must_use]
    pub fn index(self) -> usize {
        match self {
            Self::Shortcuts => 0,
            Self::Models => 1,
        }
    }

    /// Inverse of [`Self::index`]; unknown indices fall back to Shortcuts.
    #[must_use]
    pub fn from_index(index: usize) -> Self {
        match index {
            1 => Self::Models,
            _ => Self::Shortcuts,
        }
    }
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
}

/// A draft whose required fields all resolved to real catalogue entries.
#[derive(Debug)]
struct ValidatedDraft {
    provider: String,
    model: String,
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
    /// How the current (or next) take gets its audio.
    mode: SessionMode,
    screen: Screen,
    draft: SessionDraft,
    /// File chosen in the picker, waiting for the setup form's Continue.
    /// Cleared when the take starts or the flow is abandoned.
    pending_file: Option<PathBuf>,
    /// Source-file stem of the finished take, when it came from a file. Titles
    /// the suggested save name; `None` for a live take.
    finished_file_stem: Option<String>,
    result: Option<FinishedTake>,
    /// Monotonic take counter; the wire id is `take-{session_id}`.
    session_id: u64,
    transcript: TranscriptBuffer,
    waveform_bars: Arc<[f32]>,
    elapsed_seconds: u32,
    /// Decoded position of the current file take: `(processed, total)` seconds.
    progress: Option<(f32, Option<f32>)>,
    /// True while the current take transcribes a file rather than live audio.
    file_take: bool,
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
    /// Page shown by the Help dialog.
    help_tab: HelpTab,
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
            .field("mode", &self.mode)
            .field("screen", &self.screen)
            .field("draft", &self.draft)
            .field("pending_file", &self.pending_file)
            .field("result", &self.result)
            .field("session_id", &self.session_id)
            .field("transcript", &self.transcript)
            .field("waveform_bars", &self.waveform_bars)
            .field("elapsed_seconds", &self.elapsed_seconds)
            .field("last_error", &self.last_error)
            .field("app_error", &self.app_error)
            .field("output_status", &self.output_status)
            .field("written_path", &self.written_path)
            .field("finished_file_stem", &self.finished_file_stem)
            .field("active_dialog", &self.active_dialog)
            .field("help_tab", &self.help_tab)
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
            mode: SessionMode::default(),
            screen: Screen::Mode,
            draft: SessionDraft::default(),
            pending_file: None,
            finished_file_stem: None,
            result: None,
            session_id: 0,
            transcript: TranscriptBuffer::default(),
            waveform_bars: idle_waveform(),
            elapsed_seconds: 0,
            progress: None,
            file_take: false,
            elapsed_base: Duration::ZERO,
            segment_started: None,
            last_error: None,
            app_error: None,
            output_status: None,
            written_path: None,
            active_dialog: None,
            help_tab: HelpTab::default(),
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
            (SessionState::Blank, Screen::Mode) => ViewState::Mode,
            (SessionState::Blank, Screen::Form) => ViewState::Form,
            // File mode has no holding screen (T0023): with no take running,
            // the setup form is the only thing to show. A running file take
            // borrows the live take screen until T0025 gives it its own.
            (SessionState::Ready, _) if self.mode == SessionMode::File => ViewState::Form,
            (SessionState::Ready | SessionState::Recording | SessionState::Paused, _) => {
                ViewState::Live
            }
        }
    }

    pub fn session_state(&self) -> SessionState {
        self.machine.state()
    }

    /// How the current (or next) take gets its audio.
    #[must_use]
    pub fn mode(&self) -> SessionMode {
        self.mode
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

    /// Audio source of the current (or most recent) take: the device name for
    /// live takes, the picked file's display name for file takes.
    #[must_use]
    pub fn audio_source(&self) -> Option<&str> {
        self.machine.config().audio_source.as_deref()
    }

    /// Decoded position of the current file take, in seconds:
    /// `(processed, total)`, where `total` is absent when ffprobe failed.
    #[must_use]
    pub fn progress(&self) -> Option<(f32, Option<f32>)> {
        self.progress
    }

    /// Whether the current take transcribes a file rather than live audio.
    #[must_use]
    pub fn is_file_take(&self) -> bool {
        self.file_take
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

    /// Record a typed failure for the strip and write one redacted log line.
    pub fn report_error(&mut self, error: AppError) {
        crate::diagnostics::log_error("controller", &error);
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

    /// True when the draft names a provider and model that both exist in the
    /// current catalogue. That is the whole setup: a take has no other field.
    pub fn is_draft_complete(&self) -> bool {
        self.validate_draft().is_some()
    }

    pub fn finished_take(&self) -> Option<&FinishedTake> {
        self.result.as_ref()
    }

    /// True when there is no finished take, or its joined text is blank.
    #[must_use]
    pub fn finished_take_is_empty(&self) -> bool {
        let Some(result) = self.result.as_ref() else {
            return true;
        };
        joined_text(&result.committed, &result.live)
            .trim()
            .is_empty()
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
            // Log the worker-provided cause before mapping, so an unknown code
            // reduced to a generic variant is still recoverable from stderr.
            crate::diagnostics::log_error(
                "stt_event",
                &AppError::BackendFailed {
                    code: code.clone(),
                    detail: sanitize_detail(message),
                },
            );
            self.last_error = Some(format!("{code}: {}", sanitize_detail(message)).into());
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

        if let SttEvent::Progress {
            processed_seconds,
            total_seconds,
            ..
        } = &event
        {
            self.apply_progress(*processed_seconds, *total_seconds);
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
    ///
    /// A file take has no live clock: its position comes from `progress`
    /// events emitted by the decoder.
    pub fn refresh_elapsed(&mut self, now: Instant) -> bool {
        if self.file_take || self.machine.state() != SessionState::Recording {
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

    /// Store a decoded position. Progress is monotonic in the worker, so the
    /// read-out never runs backwards; a stale take is filtered upstream.
    fn apply_progress(&mut self, processed_seconds: f64, total_seconds: Option<f64>) {
        if !self.file_take {
            return;
        }
        let processed = processed_seconds.max(0.0);
        let seconds = processed.floor().min(f64::from(u32::MAX)) as u32;
        if seconds >= self.elapsed_seconds {
            self.elapsed_seconds = seconds;
            self.elapsed_base = Duration::from_secs(u64::from(seconds));
        }
        self.progress = Some((processed as f32, total_seconds.map(|total| total as f32)));
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
        self.help_tab = HelpTab::default();
    }

    /// The page currently shown by the Help dialog.
    #[must_use]
    pub fn help_tab(&self) -> HelpTab {
        self.help_tab
    }

    /// Switches the Help dialog page.
    pub fn set_help_tab(&mut self, tab: HelpTab) {
        self.help_tab = tab;
    }

    /// The provider whose models the Help dialog lists.
    ///
    /// The Settings provider wins; before a catalogue has arrived (or when
    /// that provider is unknown) the first listed provider is used, which is
    /// the only one while direct providers stay dormant.
    #[must_use]
    pub fn help_provider(&self) -> Option<&ProviderCapability> {
        let providers = self.capabilities.as_deref()?;
        self.settings
            .as_ref()
            .and_then(|settings| {
                providers
                    .iter()
                    .find(|provider| provider.id == settings.provider)
            })
            .or_else(|| providers.first())
    }

    /// Label of the provider listed by the Help dialog.
    #[must_use]
    pub fn help_provider_label(&self) -> Option<&str> {
        self.help_provider().map(|provider| provider.label.as_str())
    }

    /// Models listed by the Help dialog, in catalogue order.
    #[must_use]
    pub fn help_models(&self) -> &[ModelCapabilityInfo] {
        self.help_provider()
            .map_or(&[], |provider| &provider.models)
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

    /// Enter the workflow for `mode`.
    ///
    /// Live mode returns `true` when the persisted settings already resolve
    /// (the machine goes straight to `Ready`) and `false` when the two-field
    /// form is required.
    ///
    /// File mode always returns `false` and stays on the welcome screen: the
    /// picker runs first, and [`Self::set_pending_file`] opens the form once a
    /// file has been chosen (T0023).
    ///
    /// Refuses while a take is active: the setup is frozen then.
    pub fn choose_mode(&mut self, mode: SessionMode) -> bool {
        if self.machine.state().is_active() {
            return false;
        }
        // A new take starts from scratch: drop whichever mode was configured.
        let _ = self.machine.reset();
        self.mode = mode;
        self.draft = SessionDraft::default();
        self.pending_file = None;
        self.finished_file_stem = None;

        if mode == SessionMode::File {
            // The native picker is open; cancelling it must leave no trace.
            self.screen = Screen::Mode;
            return false;
        }

        self.screen = Screen::Form;
        self.seed_draft_from_settings();
        self.create_session()
    }

    /// Remember the file chosen in the picker and open the setup form.
    ///
    /// Nothing is transcribed yet: the take starts when [`Self::create_session`]
    /// commits the form, so the user picks a model before any decoding happens.
    ///
    /// Returns [`AppError::UnsupportedFileType`] — storing nothing — when the
    /// path is not an audio or video file (T0024).
    pub fn set_pending_file(&mut self, path: PathBuf) -> Result<(), AppError> {
        if !is_media_path(&path) {
            return Err(self.fail(AppError::UnsupportedFileType));
        }
        self.pending_file = Some(path);
        self.screen = Screen::Form;
        self.draft = SessionDraft::default();
        self.finished_file_stem = None;
        self.result = None;
        self.output_status = None;
        self.written_path = None;
        self.seed_draft_from_settings();
        Ok(())
    }

    /// The file waiting in the setup form, if any.
    #[must_use]
    pub fn pending_file(&self) -> Option<&Path> {
        self.pending_file.as_deref()
    }

    /// Drop the pending file without starting a take.
    pub fn clear_pending_file(&mut self) {
        self.pending_file = None;
    }

    /// Back to the mode chooser, clearing the machine configuration.
    /// Refuses while a take is active.
    pub fn change_mode(&mut self) -> bool {
        if self.machine.reset().is_err() {
            return false;
        }
        self.mode = SessionMode::default();
        self.screen = Screen::Mode;
        self.draft = SessionDraft::default();
        self.pending_file = None;
        self.result = None;
        self.finished_file_stem = None;
        self.output_status = None;
        self.written_path = None;
        true
    }

    /// The one back action every screen footer renders (T0021).
    ///
    /// Back always means "one step towards the mode chooser": the setup form
    /// discards its draft, a finished take is cleared, and the take screen
    /// returns home. Whichever path runs, the step leaves no stale error strip
    /// behind. Sending a take back to the chooser is refused while it is still
    /// active; T0022 makes that step discard the take instead.
    pub fn go_back(&mut self) -> bool {
        let stepped = match self.view_state() {
            ViewState::Form => {
                self.cancel_form();
                true
            }
            ViewState::Stopped => {
                self.new_take();
                true
            }
            ViewState::Mode | ViewState::Live => self.change_mode(),
        };
        if stepped {
            self.dismiss_error();
        }
        stepped
    }

    /// Pre-fill the New Session form from the persisted default provider and
    /// model, but only when both exist in the current catalogue. A fresh
    /// controller (no settings, or no model configured) starts empty.
    ///
    /// A file take can only be batched, so file mode seeds the model only when
    /// it is segmented; the provider is still seeded so the user picks from the
    /// right list (T0023).
    fn seed_draft_from_settings(&mut self) {
        // Never overwrite a choice the user already made in this draft.
        if self.draft.provider.is_some() {
            return;
        }
        let Some(settings) = self.settings.clone() else {
            return;
        };
        if !self.provider_exists(&settings.provider) {
            return;
        }
        self.draft.provider = Some(settings.provider.clone());

        if settings.model.trim().is_empty()
            || !self.model_exists(&settings.provider, &settings.model)
        {
            return;
        }
        if self.mode == SessionMode::File
            && self.model_mode(&settings.provider, &settings.model)
                != Some(TranscriptionMode::Segmented)
        {
            // Streaming models cannot transcribe files: force a choice.
            return;
        }
        self.draft.model = Some(settings.model);
    }

    /// Leave the setup form for the mode chooser, discarding the draft, the
    /// pending file, any finished take and the error strip.
    pub fn cancel_form(&mut self) {
        self.screen = Screen::Mode;
        self.draft = SessionDraft::default();
        self.pending_file = None;
        self.result = None;
        self.last_error = None;
        self.app_error = None;
        self.output_status = None;
        self.written_path = None;
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
        let ids = self.selectable_model_ids(&provider);
        let Some(next) = next_id(&ids, self.draft.model.as_deref()) else {
            return;
        };
        self.draft.model = Some(next);
    }

    /// Model ids the setup form may offer for `provider` in the current mode.
    ///
    /// A file take is decoded and sent in batches, so file mode only ever
    /// offers [`TranscriptionMode::Segmented`] models (T0023). Live mode offers
    /// the provider's whole list.
    #[must_use]
    pub fn selectable_model_ids(&self, provider: &str) -> Vec<String> {
        self.capabilities
            .as_ref()
            .and_then(|providers| providers.iter().find(|option| option.id == provider))
            .map(|option| {
                option
                    .models
                    .iter()
                    .filter(|model| {
                        self.mode != SessionMode::File || model.mode == TranscriptionMode::Segmented
                    })
                    .map(|model| model.id.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Commit the draft to the machine. Returns `false` (and changes nothing)
    /// while the draft is incomplete or the machine refuses a setup change.
    pub fn create_session(&mut self) -> bool {
        if self.machine.state().is_active() {
            return false;
        }
        let Some(draft) = self.validate_draft() else {
            return false;
        };

        // Both setters only accept an idle machine, so a partial failure can
        // never leave the configuration frozen mid-take.
        if self.machine.set_provider(draft.provider).is_err() {
            return false;
        }
        if self.machine.set_model(draft.model).is_err() {
            return false;
        }
        // Live takes always use the default microphone; file takes replace it
        // with the picked file's name once `start_file_session` validates it.
        if self.machine.config().audio_source.is_none()
            && self.machine.set_audio_source(DEFAULT_AUDIO_SOURCE).is_err()
        {
            return false;
        }

        self.screen = Screen::Mode;
        self.draft = SessionDraft::default();
        // The chosen path is consumed by the take that is about to start.
        self.pending_file = None;
        true
    }

    /// Begins a take: reset telemetry, mint the session id and tell the
    /// backend to open a stream.
    pub fn start_recording(&mut self) -> bool {
        self.start_recording_at(Instant::now())
    }

    /// [`Self::start_recording`] with an injected clock reading.
    pub fn start_recording_at(&mut self, now: Instant) -> bool {
        // A file take never opens the microphone; the picker starts those.
        if self.mode != SessionMode::Live || self.machine.state() != SessionState::Ready {
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
        self.progress = None;
        self.file_take = false;
        self.elapsed_base = Duration::ZERO;
        self.segment_started = Some(now);
        self.last_error = None;
        self.app_error = None;
        self.output_status = None;
        self.written_path = None;
        self.finished_file_stem = None;

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

    /// Begins a take that transcribes a file already on the worker's host.
    ///
    /// Only the path crosses the bridge: the Bun worker decodes the file with
    /// ffmpeg, so the host validates the path but never reads the media. The
    /// take then follows the normal stop → result → copy/paste/save flow.
    ///
    /// Returns [`AppError::FileNotFound`] for a missing path and
    /// [`AppError::FileUnreadable`] for an empty, NUL-bearing, directory or
    /// unreadable path. When no take can start (machine not `Ready`) the call
    /// is a no-op: the Prompt screen only offers the action in that state.
    pub fn start_file_session(&mut self, path: &Path) -> Result<(), AppError> {
        let display = path.to_string_lossy().into_owned();
        if display.trim().is_empty() || display.contains('\0') {
            return Err(self.fail(AppError::FileUnreadable));
        }
        // The extension is checked before existence, so a `.pdf` reports its
        // type rather than "that file no longer exists" (T0024).
        if !is_media_path(path) {
            return Err(self.fail(AppError::UnsupportedFileType));
        }

        let metadata = match std::fs::metadata(path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(self.fail(AppError::FileNotFound));
            }
            Err(_) => return Err(self.fail(AppError::FileUnreadable)),
        };
        if !metadata.is_file() {
            return Err(self.fail(AppError::FileUnreadable));
        }

        if self.machine.state() != SessionState::Ready {
            return Ok(());
        }

        let (provider, model, api_key) = match self.start_configuration() {
            Ok(configuration) => configuration,
            Err(provider) => {
                return Err(self.fail(AppError::MissingApiKey { provider }));
            }
        };

        // A file take is batched. Defensive check: `is_draft_complete` already
        // refuses this, but a catalogue refresh can reclassify a committed
        // model, and this method is reachable from other callers (T0023).
        if self.model_mode(&provider, &model) == Some(TranscriptionMode::Streaming) {
            return Err(self.fail(AppError::FileModelNotSupported));
        }

        let file_name = path.file_name().map_or_else(
            || display.clone(),
            |name| name.to_string_lossy().into_owned(),
        );
        let stem = path.file_stem().map_or_else(
            || file_name.clone(),
            |stem| stem.to_string_lossy().into_owned(),
        );

        // The source file names the take; the save dialog turns its stem into
        // the transcript title.
        let _ = self.machine.set_audio_source(file_name);
        self.mode = SessionMode::File;
        self.finished_file_stem = Some(stem);

        if self.machine.start().is_err() {
            return Ok(());
        }

        self.session_id += 1;
        self.transcript.clear();
        self.waveform_bars = idle_waveform();
        self.elapsed_seconds = 0;
        self.elapsed_base = Duration::ZERO;
        self.progress = None;
        self.file_take = true;
        // No live clock: the decoded position arrives as `progress` events.
        self.segment_started = None;
        self.last_error = None;
        self.app_error = None;
        self.output_status = None;
        self.written_path = None;
        // Retry re-issues a `Start`, which does not apply to a file take.
        self.last_start = None;

        self.send_command(SttCommand::TranscribeFile {
            session_id: self.session_label(),
            provider,
            model,
            path: display,
            api_key,
        });

        Ok(())
    }

    /// Forward one capture chunk and its FFT frame. Ignored unless the take is
    /// actively recording, so paused audio never reaches the provider.
    pub fn push_audio_chunk(&mut self, pcm16: &[i16], bars: Arc<[f32]>) {
        // A file take decodes on the worker host: live audio must never reach
        // it, even if capture happens to be running.
        if self.file_take || self.machine.state() != SessionState::Recording {
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
        self.file_take = false;
        self.progress = None;
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

    /// Leave the `Stopped` screen and start over from the mode chooser.
    pub fn new_take(&mut self) {
        self.result = None;
        self.mode = SessionMode::default();
        self.screen = Screen::Mode;
        self.draft = SessionDraft::default();
        self.pending_file = None;
        self.finished_file_stem = None;
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
        if self.finished_take_is_empty() {
            return Err(AppError::NothingRecorded);
        }
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

    /// Suggested name for the native save dialog: the source file's stem for a
    /// file take, `None` for dictation (the UI suggests a timestamped name).
    #[must_use]
    pub fn suggested_save_name(&self) -> Option<String> {
        self.result.as_ref()?;
        self.finished_file_stem.clone()
    }

    /// Write the finished take to exactly `path`, which the OS save dialog
    /// already confirmed. The file's stem titles the document and names the
    /// take; its extension picks Markdown or plain text.
    pub fn save_finished_transcript_to(&mut self, path: &Path) -> Result<SavedFile, AppError> {
        if self.finished_take_is_empty() {
            return Err(AppError::NothingRecorded);
        }
        let Some(result) = self.result.as_ref() else {
            return Err(self.fail(AppError::SaveFailed("no finished take".to_owned())));
        };
        let title = path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default();
        let format = path
            .extension()
            .and_then(|extension| extension.to_str())
            .and_then(TranscriptFormat::from_extension)
            .unwrap_or(TranscriptFormat::Markdown);
        let text = joined_text(&result.committed, &result.live);

        match save_transcript_to(path, &title, format, &text) {
            Ok(saved) => {
                self.written_path = Some(saved.path.clone());
                self.output_status = Some("Saved".into());
                Ok(saved)
            }
            Err(error) => Err(self.fail(AppError::from_output(error))),
        }
    }

    /// Open the file written by the last save.
    pub fn open_saved_file(&mut self, opener: &mut dyn FileOpener) -> Result<(), AppError> {
        let Some(path) = self.written_path.clone() else {
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
        if !self.auto_paste_enabled() || self.finished_take_is_empty() {
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
        // A file take is batched: only a segmented model can serve it (T0023).
        if self.mode == SessionMode::File
            && self.model_mode(&provider, &model) != Some(TranscriptionMode::Segmented)
        {
            return None;
        }
        Some(ValidatedDraft { provider, model })
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
        vec![ProviderCapability {
            id: "vercel-ai-gateway".to_string(),
            label: "Vercel AI Gateway".to_string(),
            available: false,
            key_env_vars: vec![
                "AI_GATEWAY_API_KEY".to_string(),
                "VERCEL_AI_GATEWAY_API_KEY".to_string(),
            ],
            // Label order, like `listCapabilities`.
            models: vec![
                ModelCapabilityInfo {
                    id: "openai/gpt-4o-transcribe".to_string(),
                    label: "GPT-4o Transcribe".to_string(),
                    mode: TranscriptionMode::Segmented,
                    description: None,
                    pricing: BTreeMap::new(),
                },
                ModelCapabilityInfo {
                    id: "openai/gpt-realtime-whisper".to_string(),
                    label: "GPT Realtime Whisper".to_string(),
                    mode: TranscriptionMode::Streaming,
                    description: Some("Live gateway transcription.".to_string()),
                    pricing: BTreeMap::from([(
                        "transcription_duration_cost_per_second".to_string(),
                        "0.000031".to_string(),
                    )]),
                },
                ModelCapabilityInfo {
                    id: "fish-audio/transcribe-1".to_string(),
                    label: "Transcribe-1".to_string(),
                    mode: TranscriptionMode::Segmented,
                    description: None,
                    pricing: BTreeMap::new(),
                },
            ],
        }]
    }

    /// Installs the catalogue the worker would send.
    fn install_catalogue(controller: &mut SessionController) {
        controller.set_capabilities(test_capabilities());
    }

    fn complete_controller() -> SessionController {
        let mut controller = SessionController::new();
        install_catalogue(&mut controller);
        assert!(!controller.choose_mode(SessionMode::Live));
        controller.cycle_provider();
        controller.cycle_model();
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
        controller.open_settings_with(
            Some("vercel-ai-gateway"),
            Some("openai/gpt-4o-transcribe"),
            true,
        );
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

        // Only the gateway is offered: an unknown provider is rejected and the
        // current choice survives, while cycling a single provider keeps it.
        assert!(!controller.set_settings_provider("mistral"));
        controller.cycle_settings_provider();
        assert_eq!(
            controller.settings_draft().provider.as_deref(),
            Some("vercel-ai-gateway")
        );
        assert_eq!(
            controller.settings_draft().model.as_deref(),
            Some("openai/gpt-realtime-whisper")
        );
        assert!(!controller.set_settings_model("not-a-model"));
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
        controller.choose_mode(SessionMode::Live);

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
    fn help_opens_on_shortcuts_and_switches_tabs() {
        let mut controller = SessionController::new();
        install_catalogue(&mut controller);

        controller.open_help();
        assert_eq!(controller.help_tab(), HelpTab::Shortcuts);
        assert_eq!(controller.help_tab().index(), 0);

        controller.set_help_tab(HelpTab::Models);
        assert_eq!(controller.help_tab(), HelpTab::Models);
        assert_eq!(controller.help_tab().index(), 1);
        assert_eq!(HelpTab::from_index(1), HelpTab::Models);
        assert_eq!(HelpTab::from_index(9), HelpTab::Shortcuts);

        controller.close_dialog();
        controller.open_help();
        assert_eq!(
            controller.help_tab(),
            HelpTab::Shortcuts,
            "reopening the dialog resets the page"
        );
    }

    #[test]
    fn help_lists_the_providers_models_with_metadata() {
        let mut controller = SessionController::new();
        install_catalogue(&mut controller);
        controller.open_help();

        assert_eq!(controller.help_provider_label(), Some("Vercel AI Gateway"));
        let models = controller.help_models();
        assert_eq!(models.len(), 3);

        let streaming = models
            .iter()
            .find(|model| model.id == "openai/gpt-realtime-whisper")
            .expect("streaming model listed");
        assert_eq!(streaming.mode, TranscriptionMode::Streaming);
        assert_eq!(
            streaming.description.as_deref(),
            Some("Live gateway transcription.")
        );
        assert_eq!(
            pricing_lines(&streaming.pricing),
            vec!["$0.000031 / second of audio"]
        );

        let batch = models
            .iter()
            .find(|model| model.id == "openai/gpt-4o-transcribe")
            .expect("batch model listed");
        assert_eq!(batch.description, None);
        assert_eq!(pricing_lines(&batch.pricing), vec!["Price not published"]);
    }

    #[test]
    fn help_models_are_empty_before_the_catalogue_arrives() {
        let mut controller = SessionController::new();
        controller.open_help();

        assert!(controller.help_models().is_empty());
        assert_eq!(controller.help_provider_label(), None);
    }

    #[test]
    fn pricing_lines_cover_the_gateway_shapes() {
        assert_eq!(pricing_lines(&BTreeMap::new()), vec!["Price not published"]);

        let tokens = BTreeMap::from([
            ("input".to_string(), "0.000002".to_string()),
            ("output".to_string(), "0.000012".to_string()),
        ]);
        assert_eq!(
            pricing_lines(&tokens),
            vec!["$2.00 / M input tokens", "$12.00 / M output tokens"]
        );

        let seconds = BTreeMap::from([(
            "transcription_duration_cost_per_second".to_string(),
            "0.000030556".to_string(),
        )]);
        assert_eq!(pricing_lines(&seconds), vec!["$0.000031 / second of audio"]);

        let free = BTreeMap::from([
            ("input".to_string(), "0".to_string()),
            ("output".to_string(), "0".to_string()),
        ]);
        assert_eq!(pricing_lines(&free), vec!["Free"]);

        let mixed = BTreeMap::from([
            ("zzz_key".to_string(), "2".to_string()),
            ("input".to_string(), "0.000003".to_string()),
            ("aaa_key".to_string(), "1".to_string()),
        ]);
        assert_eq!(
            pricing_lines(&mixed),
            vec!["$3.00 / M input tokens", "Aaa key: 1", "Zzz key: 2"]
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
        assert_eq!(ids, ["vercel-ai-gateway"]);
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

        assert!(!controller.set_settings_provider("mistral"));
        assert_eq!(
            controller.settings_api_key_env(),
            Some("AI_GATEWAY_API_KEY")
        );
    }

    #[test]
    fn cycling_selects_a_provider_and_then_its_first_model() {
        let mut controller = SessionController::new();
        install_catalogue(&mut controller);
        controller.open_settings();

        assert!(controller.cycle_settings_provider());
        assert_eq!(
            controller.settings_draft().provider.as_deref(),
            Some("vercel-ai-gateway")
        );
        assert_eq!(controller.settings_draft().model, None);

        assert!(controller.cycle_settings_model());
        assert_eq!(
            controller.settings_draft().model.as_deref(),
            Some("openai/gpt-4o-transcribe")
        );
    }
}
