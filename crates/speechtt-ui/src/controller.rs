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
    CapabilitySource, LanguageModelInfo, LanguageOption, ModelCapabilityInfo, ProviderCapability,
    SecretString, SttCommand, SttEvent, TranscriptionMode,
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
/// Offline demo transcript shown on the result screen when no STT backend is
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
    /// The language-model cleanup pass failed. The transcript is untouched.
    CleanupFailed,
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
            Self::CleanupFailed => {
                "The cleanup pass failed. The transcript is unchanged.".to_owned()
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
            | Self::CleanupFailed
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
/// A complete live configuration shows `Live` directly; idle, recording and
/// paused are states of that one screen, told apart by the machine and rendered
/// by its badge and waveform. A file take has its own `Transcription` screen and
/// stays there after the decode ends until the user continues.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewState {
    Mode,
    Form,
    Live,
    Transcription,
    SessionEnd,
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
/// It is the dialog's own copy: [`SettingsChange`] is what later reaches the
/// persisted store.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct SettingsDraft {
    pub provider: Option<String>,
    /// Default model for live takes.
    pub model_streaming: Option<String>,
    /// Default model for file takes.
    pub model_batch: Option<String>,
    /// ISO-639-1 code of the dictation language; `None` is Auto.
    pub language: Option<String>,
    /// Cleanup model id; `None` means "use the worker's default".
    pub language_model: Option<String>,
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
            .field("model_streaming", &self.model_streaming)
            .field("model_batch", &self.model_batch)
            .field("language", &self.language)
            .field("language_model", &self.language_model)
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

/// The value committed by `Save changes`, consumed by the settings store.
/// `api_key = None` keeps the stored key, `Some("")` clears it.
#[derive(Clone, PartialEq, Eq)]
pub struct SettingsChange {
    pub provider: String,
    pub model_streaming: String,
    pub model_batch: String,
    /// ISO-639-1 dictation language; `None` is Auto.
    pub language: Option<String>,
    /// Cleanup model id; `None` keeps the worker's default.
    pub language_model: Option<String>,
    pub api_key: Option<String>,
}

impl fmt::Debug for SettingsChange {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SettingsChange")
            .field("provider", &self.provider)
            .field("model_streaming", &self.model_streaming)
            .field("model_batch", &self.model_batch)
            .field("language", &self.language)
            .field("language_model", &self.language_model)
            .field("api_key", &self.api_key.as_ref().map(|_| "[REDACTED]"))
            .finish()
    }
}

/// One entry offered by a provider or model dropdown.
///
/// Built from the capability catalogue so the menus never carry a hardcoded
/// list: `id` is the value a selection commits, `label` is what the row shows,
/// and `mode` is the catalogue mode for model entries (`None` for providers).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectOption {
    pub id: String,
    pub label: String,
    /// `None` for provider entries; the catalogue mode for model entries.
    pub mode: Option<TranscriptionMode>,
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

/// The finished take shown in the `SessionEnd` screen. The machine hands the
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

/// The cached result of one repair pass.
///
/// Both halves are kept so the single cleanup button can flip between them with
/// no request: `original` is the pair the take had before the repair, `cleaned`
/// the text the model returned. A take is repaired at most once, so every later
/// flip is local and instant.
#[derive(Debug, Clone)]
struct CleanSnapshot {
    original: (SharedString, SharedString),
    cleaned: String,
}

/// Cleanup state of the finished take.
///
/// Read through [`SessionController::clean_state`]. It says whether a repair is
/// possible and which half of a cached pass the screen shows; the button's own
/// [`CleanStatus`] adds the in-flight state on top.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CleanState {
    /// No cleanup model resolves: neither a stored id nor a worker default.
    Unavailable,
    /// A repaired transcript is on screen.
    Cleaned,
    /// The pre-repair text is on screen and the repaired text is still cached.
    ShowingOriginal,
    /// Idle, with a cleanup model available and no cached pass.
    Ready,
}

/// What the single cleanup button should look like and offer next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CleanStatus {
    /// No cleanup model resolves, so there is nothing to call.
    Unavailable,
    /// A pass can be started now.
    Ready,
    /// A request is outstanding: the button is disabled until it answers.
    Running,
    /// The repaired text is on screen: the button offers the original.
    Cleaned,
    /// The original is on screen: the button offers the cached repaired text.
    ShowingOriginal,
}

/// What the single cleanup button does next.
///
/// `Start` is the only variant that costs a request; the two flips read the
/// cached [`CleanSnapshot`], so a user can compare both texts for free.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CleanToggle {
    /// Repair the transcript: the caller sends `CleanTranscript`.
    Start,
    /// Show the cached repaired text. No request.
    ShowCleaned,
    /// Show the pre-repair text. No request.
    ShowOriginal,
    /// Nothing to do: no take, no model, or a request already in flight.
    Inert,
}

/// Label of the "no declared language" entry of the Settings picker.
///
/// The entry's value is the empty string, which is what an unset
/// [`Settings::language`] means.
pub const AUTO_LANGUAGE_LABEL: &str = "Auto (detect)";

/// Dictation languages offered when the worker reports none.
///
/// Pairs of ISO-639-1 code and English label, sorted by label, covering the
/// languages the supported providers transcribe. The worker's own
/// `LANGUAGE_OPTIONS` is the authority whenever it arrives; this list keeps an
/// older worker usable rather than leaving the picker with Auto alone.
pub const FALLBACK_LANGUAGES: &[(&str, &str)] = &[
    ("ar", "Arabic"),
    ("zh", "Chinese"),
    ("da", "Danish"),
    ("nl", "Dutch"),
    ("en", "English"),
    ("fi", "Finnish"),
    ("fr", "French"),
    ("de", "German"),
    ("hi", "Hindi"),
    ("it", "Italian"),
    ("ja", "Japanese"),
    ("ko", "Korean"),
    ("no", "Norwegian"),
    ("pl", "Polish"),
    ("pt", "Portuguese"),
    ("ru", "Russian"),
    ("es", "Spanish"),
    ("sv", "Swedish"),
    ("tr", "Turkish"),
];

/// Owns the session machine, the setup draft, the finished-take result and
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
    /// The cleanup request in flight, if any: its id is what a `Cleaned` or a
    /// correlated `Error` must name to be accepted.
    clean_pending: Option<String>,
    /// Monotonic cleanup counter, reset by [`Self::forget_clean`]. The request
    /// id is `clean-<take>-<n>`, so an id issued for an earlier take can never
    /// match a later one even though the counter restarts.
    clean_seq: u64,
    /// The repaired text and the pair it replaced, cached for the life of the
    /// take. `Some` once a pass has landed, which is what the flips and the
    /// `Cleaned` / `ShowingOriginal` states read.
    clean_snapshot: Option<CleanSnapshot>,
    /// Text models the worker offers for the cleanup pass.
    language_models: Option<Vec<LanguageModelInfo>>,
    /// The worker's own default cleanup model (`DEFAULT_LANGUAGE_MODEL`, else
    /// its fallback), used when no id is stored.
    default_language_model: Option<String>,
    /// Dictation languages the worker offers. `None` when it reported none,
    /// in which case [`FALLBACK_LANGUAGES`] is offered.
    languages: Option<Vec<LanguageOption>>,
    /// Monotonic take counter; the wire id is `take-{session_id}`.
    session_id: u64,
    transcript: TranscriptBuffer,
    waveform_bars: Arc<[f32]>,
    elapsed_seconds: u32,
    /// Decoded position of the current file take: `(processed, total)` seconds.
    progress: Option<(f32, Option<f32>)>,
    /// Display name of the file the current file take transcribes.
    ///
    /// Held for the whole take: the session machine drops its configuration when
    /// the take ends, but the transcription screen keeps naming the file while
    /// decoding and once the result is ready.
    file_display_name: Option<String>,
    /// True while the current take transcribes a file rather than live audio.
    file_take: bool,
    /// A file take has been finalized and is waiting for the user to open the
    /// result: the transcription screen then offers Continue instead of Stop.
    awaiting_continue: bool,
    /// A finalized file take was ended by the user rather than by the worker
    /// reaching the end of the file. The transcription screen reports it as
    /// stopped, not as done.
    file_take_stopped_early: bool,
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
    /// A finished take asked for the automatic copy/paste gesture and is still
    /// waiting for it.
    pending_auto_paste: bool,
    /// `Stop` was sent and the matching `Stopped` has not arrived yet. The
    /// transcript is still going to be refined, so the pending copy/paste must
    /// wait.
    final_transcript_pending: bool,
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
            .field("clean_pending", &self.clean_pending)
            .field("clean_seq", &self.clean_seq)
            .field("clean_snapshot", &self.clean_snapshot.is_some())
            .field(
                "language_model_count",
                &self.languages.as_ref().map(Vec::len),
            )
            .field("session_id", &self.session_id)
            .field("transcript", &self.transcript)
            .field("waveform_bars", &self.waveform_bars)
            .field("elapsed_seconds", &self.elapsed_seconds)
            .field("last_error", &self.last_error)
            .field("app_error", &self.app_error)
            .field("output_status", &self.output_status)
            .field("written_path", &self.written_path)
            .field("pending_auto_paste", &self.pending_auto_paste)
            .field("final_transcript_pending", &self.final_transcript_pending)
            .field("finished_file_stem", &self.finished_file_stem)
            .field("file_display_name", &self.file_display_name)
            .field("file_take_stopped_early", &self.file_take_stopped_early)
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
            clean_pending: None,
            clean_seq: 0,
            clean_snapshot: None,
            language_models: None,
            default_language_model: None,
            languages: None,
            session_id: 0,
            transcript: TranscriptBuffer::default(),
            waveform_bars: idle_waveform(),
            elapsed_seconds: 0,
            progress: None,
            file_display_name: None,
            file_take: false,
            awaiting_continue: false,
            file_take_stopped_early: false,
            elapsed_base: Duration::ZERO,
            segment_started: None,
            last_error: None,
            app_error: None,
            output_status: None,
            written_path: None,
            pending_auto_paste: false,
            final_transcript_pending: false,
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
        // A file take owns its screen from the first decoded frame until the
        // user continues into the result.
        if self.file_take || self.awaiting_continue {
            return ViewState::Transcription;
        }
        if self.result.is_some() {
            return ViewState::SessionEnd;
        }
        match (self.machine.state(), self.screen) {
            (SessionState::Blank, Screen::Mode) => ViewState::Mode,
            (SessionState::Blank, Screen::Form) => ViewState::Form,
            // File mode has no holding screen: with no take running, the setup
            // form is the only thing to show.
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

    /// Whether a file take is decoding right now.
    #[must_use]
    pub fn is_file_take(&self) -> bool {
        self.file_take
    }

    /// Name of the file the current file take transcribes, if any.
    ///
    /// Available from the moment the take starts until it is discarded, so the
    /// transcription screen names the file while decoding and once the result is
    /// ready.
    #[must_use]
    pub fn file_display_name(&self) -> Option<&str> {
        self.file_display_name.as_deref()
    }

    /// Whether the finished file take was ended by the user before the end.
    #[must_use]
    pub fn file_take_stopped_early(&self) -> bool {
        self.file_take_stopped_early
    }

    /// Whether a file take has finished and its result waits behind Continue.
    ///
    /// While this is true the transcription screen shows the completed take:
    /// Stop is inert, Continue opens the result.
    #[must_use]
    pub fn is_awaiting_continue(&self) -> bool {
        self.awaiting_continue
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

    /// Catalogue id of the provider chosen for the next take.
    #[must_use]
    pub fn draft_provider(&self) -> Option<&str> {
        self.draft.provider.as_deref()
    }

    /// Catalogue id of the model chosen for the next take.
    #[must_use]
    pub fn draft_model(&self) -> Option<&str> {
        self.draft.model.as_deref()
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

    fn capability_provider_options(&self) -> Vec<SelectOption> {
        self.capabilities
            .as_ref()
            .map_or_else(Vec::new, |providers| {
                providers
                    .iter()
                    .map(|option| SelectOption {
                        id: option.id.clone(),
                        label: option.label.clone(),
                        mode: None,
                    })
                    .collect()
            })
    }

    /// A provider's models as dropdown options.
    ///
    /// `mode` keeps only the models of that mode; `None` lists every model the
    /// provider offers. The mode is carried on each option so the menu row can
    /// draw its badge.
    fn capability_model_options(
        &self,
        provider: &str,
        mode: Option<TranscriptionMode>,
    ) -> Vec<SelectOption> {
        self.capabilities
            .as_ref()
            .and_then(|providers| providers.iter().find(|option| option.id == provider))
            .map_or_else(Vec::new, |option| {
                option
                    .models
                    .iter()
                    .filter(|model| mode.is_none_or(|mode| model.mode == mode))
                    .map(|model| SelectOption {
                        id: model.id.clone(),
                        label: model.label.clone(),
                        mode: Some(model.mode),
                    })
                    .collect()
            })
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

    /// Label of the streaming default selected in the Settings dialog.
    #[must_use]
    pub fn settings_streaming_model_label(&self) -> Option<&str> {
        self.model_label_for(
            self.settings_draft.provider.as_deref()?,
            self.settings_draft.model_streaming.as_deref()?,
        )
    }

    /// Label of the batch default selected in the Settings dialog.
    #[must_use]
    pub fn settings_batch_model_label(&self) -> Option<&str> {
        self.model_label_for(
            self.settings_draft.provider.as_deref()?,
            self.settings_draft.model_batch.as_deref()?,
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

    /// Every provider in the capability catalogue, in catalogue order.
    ///
    /// Empty before the handshake arrives, so the Session setup provider menu
    /// has nothing to show rather than a stale list.
    #[must_use]
    pub fn draft_provider_options(&self) -> Vec<SelectOption> {
        self.capability_provider_options()
    }

    /// The models the setup form may offer for the chosen provider.
    ///
    /// A file take is decoded in batches, so file mode lists only
    /// [`TranscriptionMode::Segmented`] models; live mode lists every model.
    /// Empty when no provider is chosen.
    #[must_use]
    pub fn draft_model_options(&self) -> Vec<SelectOption> {
        let Some(provider) = self.draft.provider.as_deref() else {
            return Vec::new();
        };
        let mode = (self.mode == SessionMode::File).then_some(TranscriptionMode::Segmented);
        self.capability_model_options(provider, mode)
    }

    /// Every provider in the capability catalogue, for the Settings menu.
    #[must_use]
    pub fn settings_provider_options(&self) -> Vec<SelectOption> {
        self.capability_provider_options()
    }

    /// Streaming models the Settings provider offers.
    ///
    /// Empty until a provider is chosen.
    #[must_use]
    pub fn settings_streaming_model_options(&self) -> Vec<SelectOption> {
        let Some(provider) = self.settings_draft.provider.as_deref() else {
            return Vec::new();
        };
        self.capability_model_options(provider, Some(TranscriptionMode::Streaming))
    }

    /// Batch (segmented) models the Settings provider offers.
    ///
    /// Empty until a provider is chosen.
    #[must_use]
    pub fn settings_batch_model_options(&self) -> Vec<SelectOption> {
        let Some(provider) = self.settings_draft.provider.as_deref() else {
            return Vec::new();
        };
        self.capability_model_options(provider, Some(TranscriptionMode::Segmented))
    }

    /// Mode of the streaming default selected in the Settings dialog.
    #[must_use]
    pub fn settings_streaming_model_mode(&self) -> Option<TranscriptionMode> {
        self.model_mode(
            self.settings_draft.provider.as_deref()?,
            self.settings_draft.model_streaming.as_deref()?,
        )
    }

    /// Mode of the batch default selected in the Settings dialog.
    #[must_use]
    pub fn settings_batch_model_mode(&self) -> Option<TranscriptionMode> {
        self.model_mode(
            self.settings_draft.provider.as_deref()?,
            self.settings_draft.model_batch.as_deref()?,
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

    /// Provider the live take uses, or would use.
    ///
    /// The machine's own choice wins while a take exists, then the persisted
    /// settings, then the catalogue's first entry: the take screen must name a
    /// model even on an install whose settings were never saved.
    fn live_provider(&self) -> String {
        let provider = self.active_provider();
        if !provider.trim().is_empty() {
            return provider;
        }
        self.capabilities
            .as_ref()
            .and_then(|providers| providers.first())
            .map(|option| option.id.clone())
            .unwrap_or_default()
    }

    /// Catalogue id of the model the live take uses, or would use.
    ///
    /// The active take's model wins, because the machine freezes it when
    /// recording starts; before that, the default the settings resolve for Live.
    /// `None` when nothing resolves.
    #[must_use]
    pub fn live_model_id(&self) -> Option<&str> {
        let provider = self.live_provider();
        if let Some(model) = self
            .machine
            .config()
            .model
            .as_deref()
            .filter(|model| !model.trim().is_empty())
            && self.model_exists(&provider, model)
        {
            return Some(model);
        }
        self.default_model_for(SessionMode::Live)
    }

    /// Catalogue label of the model the live take uses, or would use.
    #[must_use]
    pub fn live_model_label(&self) -> Option<&str> {
        let provider = self.live_provider();
        self.model_label_for(&provider, self.live_model_id()?)
    }

    /// Models the live picker may offer for the take being prepared.
    ///
    /// Every model of the live provider, not only the streaming ones: a live
    /// take may use a segmented model, which batches the microphone audio.
    /// Empty when no catalogue has arrived or no provider resolves.
    #[must_use]
    pub fn live_model_options(&self) -> Vec<SelectOption> {
        let provider = self.live_provider();
        self.capability_model_options(&provider, None)
    }

    /// Chooses the model for the take being prepared.
    ///
    /// Returns `false` — changing nothing — when the catalogue does not know the
    /// id, when the take mode is not Live, or when the session is already
    /// active: the machine freezes its configuration from the first recorded
    /// frame.
    ///
    /// The choice is take-scoped. It does not rewrite the Settings default,
    /// because a live take may legitimately use a segmented model and
    /// `model_streaming` accepts only a streaming one.
    pub fn set_live_model(&mut self, model: &str) -> bool {
        if self.mode != SessionMode::Live || self.machine.state().is_active() {
            return false;
        }
        let provider = self.live_provider();
        if !self.model_exists(&provider, model) {
            return false;
        }
        self.machine.set_model(model).is_ok()
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

    /// A complete provider + one default per mode + usable key is required
    /// before saving.
    ///
    /// Availability is deliberately not required: saving a key is how an
    /// unavailable provider becomes connected on the next handshake.
    #[must_use]
    pub fn settings_draft_valid(&self) -> bool {
        self.settings_provider_known()
            && self.settings_draft.model_streaming.is_some()
            && self.settings_draft.model_batch.is_some()
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
        // The catalogue is what reveals the mode of a legacy single default,
        // so the migration retries here for files written by older releases.
        self.migrate_legacy_default();
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
        self.migrate_legacy_default();
        if self.screen == Screen::Form {
            self.seed_draft_from_settings();
        }
    }

    /// The persisted settings, if the composition root loaded them.
    #[must_use]
    pub fn settings(&self) -> Option<&Settings> {
        self.settings.as_ref()
    }

    // ----- Cleanup -----------------------------------------------------

    /// Whether a repair can run at all, and whether one is on screen.
    ///
    /// This answers the model question only; whether the take on screen is
    /// worth repairing is [`Self::can_clean_transcript`]'s question.
    #[must_use]
    pub fn clean_state(&self) -> CleanState {
        if let Some(snapshot) = &self.clean_snapshot {
            let showing_cleaned = self
                .result
                .as_ref()
                .is_some_and(|result| result.committed.as_ref() == snapshot.cleaned);
            return if showing_cleaned {
                CleanState::Cleaned
            } else {
                CleanState::ShowingOriginal
            };
        }
        if self.cleanup_model().is_some() {
            CleanState::Ready
        } else {
            CleanState::Unavailable
        }
    }

    /// What the cleanup button should look like and offer, including the
    /// in-flight state.
    #[must_use]
    pub fn clean_status(&self) -> CleanStatus {
        if self.clean_is_running() {
            return CleanStatus::Running;
        }
        match self.clean_state() {
            CleanState::Unavailable => CleanStatus::Unavailable,
            CleanState::Cleaned => CleanStatus::Cleaned,
            CleanState::ShowingOriginal => CleanStatus::ShowingOriginal,
            CleanState::Ready => CleanStatus::Ready,
        }
    }

    /// What the single cleanup button does next.
    ///
    /// `Start` costs a request; the two flips read the cached pass. A request in
    /// flight is always `Inert`, so a double click cannot issue two of them.
    #[must_use]
    pub fn clean_toggle(&self) -> CleanToggle {
        if self.clean_is_running() {
            return CleanToggle::Inert;
        }
        match self.clean_state() {
            CleanState::Cleaned => CleanToggle::ShowOriginal,
            CleanState::ShowingOriginal => CleanToggle::ShowCleaned,
            CleanState::Ready => CleanToggle::Start,
            CleanState::Unavailable => CleanToggle::Inert,
        }
    }

    /// The model a repair would call: the stored id, else the worker default.
    ///
    /// `None` when neither resolves, which is what disables the cleanup action.
    #[must_use]
    pub fn cleanup_model(&self) -> Option<&str> {
        let stored = self
            .settings
            .as_ref()
            .map(|settings| settings.language_model.trim())
            .filter(|model| !model.is_empty());
        stored.or_else(|| self.default_cleanup_model())
    }

    /// The model the worker would use when no id is stored.
    ///
    /// The Settings dialog shows it so the row reads as configured instead of
    /// blank, even on an install that never chose a model.
    #[must_use]
    pub fn default_cleanup_model(&self) -> Option<&str> {
        self.default_language_model
            .as_deref()
            .map(str::trim)
            .filter(|model| !model.is_empty())
    }

    /// The text models the worker offers for the cleanup pass.
    ///
    /// Empty when the worker reported none (an older worker, or discovery
    /// that found no language models).
    #[must_use]
    pub fn cleanup_models(&self) -> &[LanguageModelInfo] {
        self.language_models.as_deref().unwrap_or(&[])
    }

    /// `true` when the cleanup button is reachable at all.
    ///
    /// A cached pass stays flippable even after the model stops resolving, so
    /// the two halves of a repair can always be compared once it has run.
    #[must_use]
    pub fn can_clean_transcript(&self) -> bool {
        !matches!(self.clean_toggle(), CleanToggle::Inert) && !self.finished_take_is_empty()
    }

    /// `true` while a repair request is outstanding.
    #[must_use]
    pub fn clean_is_running(&self) -> bool {
        self.clean_pending.is_some()
    }

    /// The finished take's text exactly as the screen shows it.
    ///
    /// The cleanup request is built from this, and Copy, Save and the automatic
    /// paste read the same pair, so a repaired transcript is what leaves the
    /// widget.
    #[must_use]
    pub fn displayed_transcript(&self) -> String {
        self.result
            .as_ref()
            .map(|result| joined_text(&result.committed, &result.live))
            .unwrap_or_default()
    }

    /// The dictation language every new take declares, as an ISO-639-1 code.
    ///
    /// `None` for Auto: the command then omits the key and the provider
    /// detects the language itself.
    #[must_use]
    pub fn dictation_language(&self) -> Option<String> {
        self.settings.as_ref()?.language()
    }

    /// Records the cleanup catalogue and the offered dictation languages.
    ///
    /// A worker that reports no languages leaves [`FALLBACK_LANGUAGES`] in
    /// place, so the Settings pickers keep working against an older worker; an
    /// empty list is treated as "reported none" for the same reason.
    pub fn set_language_catalogue(
        &mut self,
        languages: Option<Vec<LanguageOption>>,
        language_models: Option<Vec<LanguageModelInfo>>,
        default_language_model: Option<String>,
    ) {
        self.languages = languages.filter(|languages| !languages.is_empty());
        self.language_models = language_models;
        self.default_language_model = default_language_model;
    }

    /// The dictation languages the Settings picker offers, Auto first.
    ///
    /// Auto is the empty id — what an unset [`Settings::language`] means — and
    /// the rest come from the worker, or from [`FALLBACK_LANGUAGES`] when it
    /// reports none.
    #[must_use]
    pub fn settings_language_options(&self) -> Vec<SelectOption> {
        let mut options = vec![SelectOption {
            id: String::new(),
            label: AUTO_LANGUAGE_LABEL.to_owned(),
            mode: None,
        }];
        options.extend(
            self.offered_languages()
                .into_iter()
                .map(|language| SelectOption {
                    id: language.code,
                    label: language.label,
                    mode: None,
                }),
        );
        options
    }

    /// The languages the worker published, else the built-in list.
    fn offered_languages(&self) -> Vec<LanguageOption> {
        self.languages.clone().unwrap_or_else(|| {
            FALLBACK_LANGUAGES
                .iter()
                .map(|(code, label)| LanguageOption {
                    code: (*code).to_owned(),
                    label: (*label).to_owned(),
                })
                .collect()
        })
    }

    /// The cleanup-model ids the Settings picker offers, in catalogue order.
    #[must_use]
    pub fn settings_cleanup_model_options(&self) -> Vec<SelectOption> {
        self.cleanup_models()
            .iter()
            .map(|model| SelectOption {
                id: model.id.clone(),
                label: model.label.clone(),
                mode: None,
            })
            .collect()
    }

    /// Selects the dictation language in the Settings draft.
    ///
    /// The empty code is Auto. Any other code must be one the picker offers, so
    /// the dialog cannot store a language no provider can be told about.
    pub fn set_settings_language(&mut self, code: &str) -> bool {
        let code = code.trim().to_ascii_lowercase();
        if code.is_empty() {
            self.settings_draft.language = None;
            return true;
        }
        if !self
            .offered_languages()
            .iter()
            .any(|language| language.code == code)
        {
            return false;
        }
        self.settings_draft.language = Some(code);
        true
    }

    /// Selects the cleanup model in the Settings draft.
    ///
    /// The empty id means "use the worker's default". When the catalogue lists
    /// language models the id must be one of them; a worker that lists none
    /// accepts any non-empty id, which is what the free-text fallback needs.
    pub fn set_settings_cleanup_model(&mut self, model: &str) -> bool {
        let model = model.trim();
        if model.is_empty() {
            self.settings_draft.language_model = None;
            return true;
        }
        let offered = self.cleanup_models();
        if !offered.is_empty() && !offered.iter().any(|option| option.id == model) {
            return false;
        }
        self.settings_draft.language_model = Some(model.to_owned());
        true
    }

    /// Starts one repair request and returns the id to send, or `None` when no
    /// pass can be taken.
    ///
    /// The id embeds the take counter, so a late answer issued for a previous
    /// take can never match [`Self::apply_cleaned`].
    pub fn begin_clean_transcript(&mut self) -> Option<String> {
        if !self.can_clean_transcript() {
            return None;
        }
        self.clean_seq += 1;
        let request_id = format!("clean-{}-{}", self.session_id, self.clean_seq);
        self.clean_pending = Some(request_id.clone());
        Some(request_id)
    }

    /// The `CleanTranscript` command answering `request_id`.
    ///
    /// Provider and key are resolved exactly as `Start` resolves them, so the
    /// repair reaches the provider the take used. `None` — having changed
    /// nothing — when no model resolves, the displayed text is blank, or the
    /// provider has no stored key; the caller reports the failure and drops the
    /// request.
    #[must_use]
    pub fn clean_transcript_command(&self, request_id: &str) -> Option<SttCommand> {
        let provider = self.active_provider();
        let model = self.cleanup_model()?.to_owned();
        let text = self.displayed_transcript();
        if text.trim().is_empty() {
            return None;
        }
        let api_key = self
            .settings
            .as_ref()
            .and_then(|settings| settings.api_key(&provider))
            .map(SecretString::new);
        api_key.as_ref()?;
        Some(SttCommand::CleanTranscript {
            request_id: request_id.to_owned(),
            provider,
            model,
            text,
            language: self.dictation_language(),
            api_key,
        })
    }

    /// Folds a successful repair: caches both halves and makes
    /// the repaired text the whole displayed take.
    ///
    /// Returns `false` — after settling the request — for an id that is not the
    /// outstanding one, and for an answer with no usable text: a blank answer
    /// must never erase the transcript already on screen.
    pub fn apply_cleaned(&mut self, request_id: &str, text: &str) -> bool {
        if self.clean_pending.as_deref() != Some(request_id) {
            return false;
        }
        self.clean_pending = None;

        let text = text.trim();
        if text.is_empty() {
            return false;
        }
        let Some(result) = self.result.as_mut() else {
            return false;
        };
        // Both halves are cached, so the button can flip between them for the
        // rest of the take without asking the model again.
        self.clean_snapshot = Some(CleanSnapshot {
            original: (result.committed.clone(), result.live.clone()),
            cleaned: text.to_owned(),
        });
        result.committed = text.into();
        result.live = SharedString::default();
        self.output_status = Some("Cleaned".into());
        true
    }

    /// Folds a failed repair: drops the outstanding request and reports the
    /// typed error, leaving the transcript untouched.
    ///
    /// Returns `false` — reporting nothing — for an id that is not the
    /// outstanding one, so a stale failure cannot raise the strip.
    pub fn fail_clean_transcript(&mut self, request_id: &str, error: AppError) -> bool {
        if self.clean_pending.as_deref() != Some(request_id) {
            return false;
        }
        self.clean_pending = None;
        self.report_error(error);
        true
    }

    /// Flips the screen between the cached repaired text and its original.
    ///
    /// Local and instant: the pass ran once, and every later click reads
    /// [`CleanSnapshot`]. Returns `false` — changing nothing — when no pass is
    /// cached or a request is in flight, i.e. for every [`CleanToggle`] that is
    /// not a flip.
    pub fn flip_clean_transcript(&mut self) -> bool {
        if self.clean_is_running() {
            return false;
        }
        let Some(snapshot) = self.clean_snapshot.clone() else {
            return false;
        };
        let show_original = self.clean_state() == CleanState::Cleaned;
        let Some(result) = self.result.as_mut() else {
            return false;
        };
        if show_original {
            result.committed = snapshot.original.0;
            result.live = snapshot.original.1;
            self.output_status = Some("Original restored".into());
        } else {
            result.committed = snapshot.cleaned.into();
            result.live = SharedString::default();
            self.output_status = Some("Cleaned".into());
        }
        true
    }

    /// Drops every trace of the cleanup of a finished take: no request in
    /// flight, nothing cached to flip, and a request counter that restarts.
    fn forget_clean(&mut self) {
        self.clean_pending = None;
        self.clean_seq = 0;
        self.clean_snapshot = None;
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
    /// the current one. `Ready` and session-less errors are global. A `Cleaned`
    /// answer and an error carrying a `request_id` are routed to the cleanup
    /// state machine instead of the transcript; a cleanup error whose id is not
    /// outstanding falls through, so an unexpected failure still reaches the
    /// strip.
    pub fn handle_stt_event(&mut self, event: SttEvent) {
        let accepted = self.accepts_session(event.session_id());
        if crate::diagnostics::transcript_trace_enabled() {
            crate::diagnostics::log_stt_event(&event, accepted);
        }
        if !accepted {
            return;
        }

        if let SttEvent::Cleaned { request_id, text } = &event {
            self.apply_cleaned(request_id, text);
            return;
        }

        // A failed repair answers the action that asked for it; an id that is
        // not outstanding falls through, so an unexpected failure still reaches
        // the strip.
        if let SttEvent::Error {
            request_id: Some(request_id),
            code,
            message,
            ..
        } = &event
        {
            let provider = self.active_provider();
            // A cleanup names its own failure: the take and its transcript are
            // intact, so a backend-flavoured message would mislead.
            let error = if code == "cleanup_failed" {
                AppError::CleanupFailed
            } else {
                AppError::from_stt_error(code, message, &provider)
            };
            if self.fail_clean_transcript(request_id, error) {
                return;
            }
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
            providers,
            source,
            languages,
            language_models,
            default_language_model,
            ..
        } = &event
        {
            self.apply_capabilities(providers.clone(), *source);
            self.set_language_catalogue(
                languages.clone(),
                language_models.clone(),
                default_language_model.clone(),
            );
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

        if let SttEvent::Stopped { session_id, .. } = &event
            && self.accepts_session(Some(session_id))
        {
            self.final_transcript_pending = false;
        }

        if matches!(event, SttEvent::Stopped { .. }) {
            // A live take that is already finished keeps refining in place; a
            // running file take is finalized here without asking the worker for
            // a second stop.
            if self.finalize_file_take() {
                return;
            }
            if let Some(result) = &mut self.result {
                // The authoritative final refines the take in place; the take
                // is still finished, so nothing stays in the preview half.
                let text = self.transcript.full_text();
                result.committed = text.into();
                result.live = SharedString::default();
            }
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

    /// Store a decoded position. Progress is monotonic in the worker, so the bar
    /// never runs backwards; a stale take is filtered upstream.
    ///
    /// The decoded position drives the progress bar only. The elapsed read-out
    /// stays the wall-clock time since the take started, exactly as it is for a
    /// live take.
    fn apply_progress(&mut self, processed_seconds: f64, total_seconds: Option<f64>) {
        if !self.file_take {
            return;
        }
        let processed = processed_seconds.max(0.0);
        self.progress = Some((processed as f32, total_seconds.map(|total| total as f32)));
    }

    fn accrue_elapsed(&mut self, now: Instant) {
        if let Some(started) = self.segment_started.take() {
            self.elapsed_base += now.saturating_duration_since(started);
        }
        self.elapsed_seconds = self.elapsed_base.as_secs().min(u64::from(u32::MAX)) as u32;
    }

    /// Provider the current take runs on: the session's frozen configuration,
    /// the finished take that is on screen, then the persisted default.
    ///
    /// The cleanup request and error attribution both read it, so a repair
    /// always names the provider its take used — the machine forgets its
    /// configuration when the take ends, but the finished take keeps the
    /// provider it ran on.
    #[must_use]
    pub fn active_provider(&self) -> String {
        if let Some(provider) = self
            .machine
            .config()
            .provider
            .clone()
            .filter(|provider| !provider.trim().is_empty())
        {
            return provider;
        }
        if let Some(provider) = self
            .result
            .as_ref()
            .map(|result| result.session().provider.trim().to_owned())
            .filter(|provider| !provider.is_empty())
        {
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
        self.open_settings_with(None, None, None, false);
    }

    /// Seed the Settings draft from persisted values.
    ///
    /// Unknown provider/model ids are ignored so corrupt state cannot make the
    /// dialog panic. `api_key_unchanged` means "a key is already stored";
    /// the field itself stays empty because the raw key never leaves the store.
    pub fn open_settings_with(
        &mut self,
        provider: Option<&str>,
        model_streaming: Option<&str>,
        model_batch: Option<&str>,
        api_key_unchanged: bool,
    ) {
        self.settings_draft = SettingsDraft {
            provider: None,
            model_streaming: None,
            model_batch: None,
            language: None,
            language_model: None,
            api_key: String::new(),
            api_key_unchanged,
        };
        if let Some(provider) = provider
            && self.set_settings_provider(provider)
        {
            if let Some(model) = model_streaming {
                self.set_settings_streaming_model(model);
            }
            if let Some(model) = model_batch {
                self.set_settings_batch_model(model);
            }
        }
        // The dictation language and the cleanup model are settings-level
        // choices with no per-take counterpart, so the draft starts from what is
        // stored: Auto and "use the worker default" when nothing is.
        let stored = self.settings.clone();
        self.settings_draft.language = stored.as_ref().and_then(Settings::language);
        self.settings_draft.language_model = stored
            .as_ref()
            .map(|settings| settings.language_model.trim().to_owned())
            .filter(|model| !model.is_empty());
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
            self.settings_draft.model_streaming = None;
            self.settings_draft.model_batch = None;
        }
        true
    }

    /// Selects the live-take default model.
    ///
    /// Returns `true` when `model` belongs to the settings provider and the
    /// catalogue reports it as [`TranscriptionMode::Streaming`]. The batch slot
    /// is untouched: the two defaults are chosen independently.
    pub fn set_settings_streaming_model(&mut self, model: &str) -> bool {
        self.select_settings_model(model, TranscriptionMode::Streaming)
    }

    /// Selects the file-take default model.
    ///
    /// Returns `true` when `model` belongs to the settings provider and the
    /// catalogue reports it as [`TranscriptionMode::Segmented`]. The streaming
    /// slot is untouched.
    pub fn set_settings_batch_model(&mut self, model: &str) -> bool {
        self.select_settings_model(model, TranscriptionMode::Segmented)
    }

    /// Shared body of the two default setters: the id must exist for the
    /// settings provider and carry exactly `mode`.
    fn select_settings_model(&mut self, model: &str, mode: TranscriptionMode) -> bool {
        let Some(provider) = self.settings_draft.provider.clone() else {
            return false;
        };
        if self.model_mode(&provider, model) != Some(mode) {
            return false;
        }
        match mode {
            TranscriptionMode::Streaming => {
                self.settings_draft.model_streaming = Some(model.to_string());
            }
            TranscriptionMode::Segmented => {
                self.settings_draft.model_batch = Some(model.to_string());
            }
        }
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

    /// Commits the Settings draft when valid and closes the dialog.
    ///
    /// Returns `None` and leaves the dialog open otherwise.
    pub fn save_settings(&mut self) -> Option<SettingsChange> {
        if !self.settings_draft_valid() {
            return None;
        }
        let change = SettingsChange {
            provider: self.settings_draft.provider.clone()?,
            model_streaming: self.settings_draft.model_streaming.clone()?,
            model_batch: self.settings_draft.model_batch.clone()?,
            language: self.settings_draft.language.clone(),
            language_model: self.settings_draft.language_model.clone(),
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
    /// file has been chosen.
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
        self.forget_clean();

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
    /// path is not an audio or video file.
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
        self.forget_clean();
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
        self.file_display_name = None;
        self.file_take = false;
        self.awaiting_continue = false;
        self.file_take_stopped_early = false;
        self.progress = None;
        self.output_status = None;
        self.written_path = None;
        self.forget_clean();
        true
    }

    /// The one back action every screen footer renders.
    ///
    /// Back always means "one step towards the mode chooser": the setup form
    /// discards its draft, a finished take is cleared, and the take screen
    /// discards an open take and returns home. Whichever path runs, the step
    /// leaves no stale error strip behind.
    pub fn go_back(&mut self) -> bool {
        let stepped = match self.view_state() {
            ViewState::Form => {
                self.cancel_form();
                true
            }
            ViewState::SessionEnd => {
                self.new_take();
                true
            }
            // Leaving a take screen discards the take.
            ViewState::Mode | ViewState::Live | ViewState::Transcription => self.cancel_take(),
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
    /// right list.
    /// The stored default for `mode`, when the catalogue resolves it.
    ///
    /// A live take uses the streaming default and a file take the batch one.
    /// `None` when no settings were loaded, the provider is unknown, the stored
    /// id is not in the catalogue, or its catalogue mode does not match the
    /// take's mode.
    #[must_use]
    pub fn default_model_for(&self, mode: SessionMode) -> Option<&str> {
        let settings = self.settings.as_ref()?;
        let stored = match mode {
            SessionMode::Live => settings.model_streaming.as_str(),
            SessionMode::File => settings.model_batch.as_str(),
        };
        if stored.trim().is_empty() {
            return None;
        }
        if !self.model_exists(&settings.provider, stored) {
            return None;
        }
        let expected = match mode {
            SessionMode::Live => TranscriptionMode::Streaming,
            SessionMode::File => TranscriptionMode::Segmented,
        };
        if self.model_mode(&settings.provider, stored) != Some(expected) {
            return None;
        }
        Some(stored)
    }

    /// Opens the form with the defaults its mode should start from.
    ///
    /// Both fields are prefilled so a take can start without touching the
    /// menus: provider from the stored settings, model from
    /// [`Self::default_model_for`] for the current mode. A choice the user
    /// already made in this draft is never overwritten.
    fn seed_draft_from_settings(&mut self) {
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
        self.draft.model = self.default_model_for(self.mode).map(str::to_string);
    }

    /// Moves a single default written by an older release into its mode slot.
    ///
    /// Older settings files hold one `model`. The catalogue is the only thing
    /// that knows whether it streams or batches, so the migration runs after
    /// the handshake as well as after loading. It applies only while both new
    /// slots are empty, never overwrites a chosen slot, and clears the legacy
    /// value so the next save writes the two mode-specific keys.
    fn migrate_legacy_default(&mut self) {
        let Some(settings) = self.settings.as_ref() else {
            return;
        };
        let legacy = settings.model.trim().to_owned();
        if legacy.is_empty() {
            return;
        }
        if !settings.model_streaming.is_empty() || !settings.model_batch.is_empty() {
            return;
        }
        let provider = settings.provider.clone();
        let slot = match self.model_mode(&provider, &legacy) {
            Some(TranscriptionMode::Streaming) => {
                let mut settings = settings.clone();
                settings.model_streaming = legacy.clone();
                settings.model.clear();
                settings
            }
            Some(TranscriptionMode::Segmented) => {
                let mut settings = settings.clone();
                settings.model_batch = legacy.clone();
                settings.model.clear();
                settings
            }
            None => return,
        };
        self.settings = Some(slot);
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
        self.forget_clean();
    }

    /// Selects the draft provider by catalogue id.
    ///
    /// Returns `true` when `provider` exists in the catalogue and was applied.
    /// Choosing a different provider clears the draft model, because a model
    /// belongs to its provider. Re-selecting the current provider keeps it.
    pub fn set_draft_provider(&mut self, provider: &str) -> bool {
        if !self.provider_exists(provider) {
            return false;
        }
        if self.draft.provider.as_deref() != Some(provider) {
            self.draft.provider = Some(provider.to_string());
            self.draft.model = None;
        }
        true
    }

    /// Selects the draft model by catalogue id.
    ///
    /// Returns `true` when `model` belongs to the draft provider and, in file
    /// mode, is a [`TranscriptionMode::Segmented`] model. Streaming models are
    /// rejected for file takes because the worker only batches files.
    pub fn set_draft_model(&mut self, model: &str) -> bool {
        let Some(provider) = self.draft.provider.clone() else {
            return false;
        };
        if !self.model_exists(&provider, model) {
            return false;
        }
        if self.mode == SessionMode::File
            && self.model_mode(&provider, model) != Some(TranscriptionMode::Segmented)
        {
            return false;
        }
        self.draft.model = Some(model.to_string());
        true
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
    /// offers [`TranscriptionMode::Segmented`] models. Live mode offers the
    /// provider's whole list.
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
        self.file_display_name = None;
        self.file_take_stopped_early = false;
        self.forget_clean();

        let start = SttCommand::Start {
            session_id: self.session_label(),
            provider,
            model,
            sample_rate: SAMPLE_RATE,
            language: self.dictation_language(),
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
        // type rather than "that file no longer exists".
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
        // model, and this method is reachable from other callers.
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
        let _ = self.machine.set_audio_source(file_name.clone());
        self.mode = SessionMode::File;
        self.finished_file_stem = Some(stem);
        self.file_display_name = Some(file_name);
        self.file_take_stopped_early = false;
        self.forget_clean();

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
        self.awaiting_continue = false;
        // The elapsed read-out times the transcription itself, so the clock
        // starts with the take even though no audio reaches the host.
        self.segment_started = Some(Instant::now());
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
            language: self.dictation_language(),
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
    ///
    /// A file take cannot pause: the worker decodes and transcribes it in one
    /// sequential batch run, so only Stop ends it.
    pub fn pause_recording_at(&mut self, now: Instant) -> bool {
        if self.file_take || self.machine.pause().is_err() {
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
    ///
    /// Inert for a file take, which has no paused state to resume from.
    pub fn resume_recording_at(&mut self, now: Instant) -> bool {
        if self.file_take || self.machine.resume().is_err() {
            return false;
        }
        self.segment_started = Some(now);
        self.send_command(SttCommand::Resume);
        true
    }

    /// Ends the take, asks the backend to flush, and remembers the transcript
    /// for the result screen.
    ///
    /// A live take goes straight to `SessionEnd`. A file take stays on
    /// `Transcription`: the result is built and held until the user presses
    /// Continue, so the screen never leaves on its own.
    pub fn finish_take(&mut self) -> bool {
        let was_file_take = self.file_take;
        let Ok(session) = self.machine.stop() else {
            return false;
        };
        self.accrue_elapsed(Instant::now());
        self.segment_started = None;
        self.file_take = false;
        self.progress = None;
        self.send_command(SttCommand::Stop);
        // The worker answers `Stop` with a `Stopped` event whose text is the
        // authoritative final transcript, so the automatic copy/paste waits for
        // it instead of pasting a version the next event is about to replace.
        self.final_transcript_pending = true;
        self.pending_auto_paste = self.auto_paste_enabled();

        self.forget_clean();
        let (committed, live) = self.finished_transcript();
        self.result = Some(FinishedTake {
            session,
            committed: committed.into(),
            live: live.into(),
        });
        self.awaiting_continue = was_file_take;
        self.file_take_stopped_early = was_file_take;
        self.written_path = None;
        self.output_status = None;
        true
    }

    /// Finalize a file take the worker ended by itself.
    ///
    /// Called on the worker's `Stopped` event for a take that is still running:
    /// the machine returns to `Ready` and the result is built from the text
    /// received so far, but **no** `SttCommand::Stop` is sent — the worker
    /// already finished. The screen stays on `Transcription` with Continue
    /// enabled; the user decides when to open the transcript.
    ///
    /// Returns `false` (changing nothing) when no file take is running, so a
    /// `Stopped` event can never finalize a live take.
    pub fn finalize_file_take(&mut self) -> bool {
        if !self.file_take {
            return false;
        }
        let Ok(session) = self.machine.stop() else {
            return false;
        };
        self.accrue_elapsed(Instant::now());
        self.segment_started = None;
        self.file_take = false;
        self.forget_clean();
        let (committed, live) = self.finished_transcript();
        self.result = Some(FinishedTake {
            session,
            committed: committed.into(),
            live: live.into(),
        });
        self.awaiting_continue = true;
        self.written_path = None;
        self.output_status = None;
        true
    }

    /// Reveal the result of a finished file take: the transcription screen's
    /// Continue.
    ///
    /// Returns `false` when no file take is waiting, so Continue is inert in
    /// every other state.
    pub fn continue_to_end(&mut self) -> bool {
        if !self.awaiting_continue || self.result.is_none() {
            return false;
        }
        self.awaiting_continue = false;
        true
    }

    /// Abandon the open take and stay on the take screen, ready to record
    /// again: the footer's Reset.
    ///
    /// Sends exactly one `Stop` so the worker tears the session down, then
    /// invalidates the discarded session so any late event of it is ignored.
    /// The configuration is kept (`SessionState::Ready`), so the next mic press
    /// opens a new session with a new id. Nothing reaches Copy/Save: the
    /// finished result stays `None`.
    ///
    /// Returns `false` — changing nothing, sending nothing — when no take is
    /// open or when the take decodes a file (the worker owns that decode).
    pub fn reset_take(&mut self) -> bool {
        if self.file_take || !self.machine.state().is_active() {
            return false;
        }
        if self.machine.discard_take().is_err() {
            return false;
        }
        self.send_command(SttCommand::Stop);
        self.forget_take();
        true
    }

    /// Abandon the open take and return to the mode chooser: the footer's
    /// Cancel.
    ///
    /// Works mid-take, from Recording or Paused. A file take also aborts the
    /// worker's decode. Whatever the state, the result is the mode chooser with
    /// no finished take to copy or save.
    pub fn cancel_take(&mut self) -> bool {
        if self.machine.state().is_active() {
            if self.machine.discard_take().is_err() {
                return false;
            }
            self.send_command(SttCommand::Stop);
        }
        self.change_mode()
    }

    /// Drop every trace of the take that was just discarded, keeping the
    /// configuration. The session counter moves on so late events of the
    /// abandoned session fail [`Self::accepts_session`].
    fn forget_take(&mut self) {
        self.session_id += 1;
        self.transcript.clear();
        self.waveform_bars = idle_waveform();
        self.elapsed_seconds = 0;
        self.elapsed_base = Duration::ZERO;
        self.segment_started = None;
        self.progress = None;
        self.file_take = false;
        self.awaiting_continue = false;
        self.file_take_stopped_early = false;
        self.finished_file_stem = None;
        self.file_display_name = None;
        self.result = None;
        self.last_error = None;
        self.app_error = None;
        self.output_status = None;
        self.written_path = None;
        self.pending_auto_paste = false;
        self.final_transcript_pending = false;
        self.forget_clean();
    }

    /// Leave the result screen and start over from the mode chooser.
    pub fn new_take(&mut self) {
        self.result = None;
        self.awaiting_continue = false;
        self.file_take_stopped_early = false;
        self.mode = SessionMode::default();
        self.screen = Screen::Mode;
        self.draft = SessionDraft::default();
        self.pending_file = None;
        self.finished_file_stem = None;
        self.file_display_name = None;
        self.transcript.clear();
        self.waveform_bars = idle_waveform();
        self.elapsed_seconds = 0;
        self.elapsed_base = Duration::ZERO;
        self.segment_started = None;
        self.last_error = None;
        self.app_error = None;
        self.output_status = None;
        self.written_path = None;
        self.pending_auto_paste = false;
        self.final_transcript_pending = false;
        self.forget_clean();
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
            // Stop needs no pause first: the microphone owns pause/resume, so
            // Stop is the only way to end the take and is always live.
            SessionAction::StopRecording => {
                if self.machine.state().is_active() {
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

    /// Takes the automatic copy/paste request of a finished take.
    ///
    /// Returns `true` exactly once per take that asked for the gesture, and
    /// only after the worker's final `Stopped` event has landed: the finished
    /// transcript is refined in place by that event, so copying earlier would
    /// paste text the screen is about to replace.
    pub fn take_auto_paste_request(&mut self) -> bool {
        if !self.pending_auto_paste || self.final_transcript_pending {
            return false;
        }
        self.pending_auto_paste = false;
        true
    }

    /// Whether a finished take is still waiting for its automatic copy/paste.
    #[must_use]
    pub fn auto_paste_is_pending(&self) -> bool {
        self.pending_auto_paste
    }

    /// Whether the persisted settings ask for automatic paste.
    ///
    /// Paste needs a configured default for the current take mode: without one
    /// the take cannot start, and the worker never produces a transcript.
    #[must_use]
    pub fn auto_paste_enabled(&self) -> bool {
        self.settings
            .as_ref()
            .is_some_and(|settings| settings.auto_paste)
            && self.default_model_for(self.mode).is_some()
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
            .or_else(|| self.default_model_for(self.mode).map(str::to_string))
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
        // A file take is batched: only a segmented model can serve it.
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

    /// Sends one command on the attached sink.
    ///
    /// A send failure is reported as a typed error and changes nothing else;
    /// with no sink attached the command is dropped, which is how the headless
    /// fixtures run without a worker.
    pub fn send_command(&mut self, command: SttCommand) {
        let Some(sink) = self.sink.as_mut() else {
            return;
        };
        if let Err(error) = sink.send(command) {
            self.report_error(AppError::from_backend(&error));
        }
    }

    /// Transcript for the result screen. With no backend attached the widget
    /// is an offline demo and shows its built-in sample take.
    fn finished_transcript(&self) -> (String, String) {
        if self.sink.is_none() && self.transcript.full_text().is_empty() {
            (MOCK_COMMITTED.to_string(), MOCK_LIVE.to_string())
        } else {
            // A finished take has no preview left: the live tail was the
            // hypothesis for what was being spoken, and the take is over. The
            // displayed text is settled into the committed half so Copy, Save
            // and the automatic paste carry exactly what the screen shows.
            (self.transcript.full_text(), String::new())
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

    /// Records the commands a controller sends, shared with the test.
    #[derive(Clone, Default)]
    struct CommandLog {
        commands: Arc<std::sync::Mutex<Vec<SttCommand>>>,
    }

    impl CommandLog {
        fn take(&self) -> Vec<SttCommand> {
            std::mem::take(&mut *self.commands.lock().expect("command log"))
        }
    }

    impl SttSessionSink for CommandLog {
        fn send(&mut self, command: SttCommand) -> Result<(), BackendError> {
            self.commands.lock().expect("command log").push(command);
            Ok(())
        }
    }

    /// A live take that has already started, with a sink attached.
    fn recording_live_take() -> (SessionController, CommandLog) {
        let mut controller = SessionController::new();
        install_catalogue(&mut controller);
        let mut settings = Settings {
            provider: "vercel-ai-gateway".to_string(),
            model_streaming: "openai/gpt-realtime-whisper".to_string(),
            model_batch: "openai/gpt-4o-transcribe".to_string(),
            ..Settings::default()
        };
        settings.set_api_key("vercel-ai-gateway", "sk-test-key");
        controller.apply_settings(settings);
        let sink = CommandLog::default();
        controller.set_sink(Box::new(sink.clone()));
        assert!(controller.choose_mode(SessionMode::Live));
        assert!(controller.start_recording_at(Instant::now()));
        let _ = sink.take();
        (controller, sink)
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
        controller.set_settings_streaming_model("openai/gpt-realtime-whisper");
        controller.set_settings_batch_model("openai/gpt-4o-transcribe");
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
        assert!(controller.set_settings_streaming_model("openai/gpt-realtime-whisper"));
        assert!(
            !controller.settings_draft_valid(),
            "the batch default is still missing"
        );
        assert!(controller.set_settings_batch_model("openai/gpt-4o-transcribe"));
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
            Some("openai/gpt-realtime-whisper"),
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
        assert!(controller.set_settings_streaming_model("openai/gpt-realtime-whisper"));
        assert!(controller.set_settings_batch_model("openai/gpt-4o-transcribe"));

        // Only the gateway is offered: an unknown provider is rejected and the
        // current choice survives, while cycling a single provider keeps it.
        assert!(!controller.set_settings_provider("mistral"));
        controller.cycle_settings_provider();
        assert_eq!(
            controller.settings_draft().provider.as_deref(),
            Some("vercel-ai-gateway")
        );
        assert_eq!(
            controller.settings_draft().model_streaming.as_deref(),
            Some("openai/gpt-realtime-whisper")
        );
        assert_eq!(
            controller.settings_draft().model_batch.as_deref(),
            Some("openai/gpt-4o-transcribe")
        );
        assert!(!controller.set_settings_streaming_model("not-a-model"));
        assert!(!controller.set_settings_batch_model("not-a-model"));
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
                model_streaming: "openai/gpt-realtime-whisper".to_string(),
                model_batch: "openai/gpt-4o-transcribe".to_string(),
                language: None,
                language_model: None,
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
            Some("openai/gpt-realtime-whisper"),
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
        controller.set_settings_streaming_model("openai/gpt-realtime-whisper");
        controller.set_settings_batch_model("openai/gpt-4o-transcribe");
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

        assert!(controller.set_settings_streaming_model("openai/gpt-realtime-whisper"));
        assert_eq!(
            controller.settings_streaming_model_mode(),
            Some(TranscriptionMode::Streaming)
        );

        assert!(controller.set_settings_batch_model("openai/gpt-4o-transcribe"));
        assert_eq!(
            controller.settings_batch_model_mode(),
            Some(TranscriptionMode::Segmented)
        );
        assert_eq!(controller.session_model_mode(), None, "no take yet");
    }

    /// Stored settings with the given per-mode defaults and no API key.
    fn stored_settings(streaming: &str, batch: &str) -> Settings {
        Settings {
            provider: "vercel-ai-gateway".to_string(),
            model_streaming: streaming.to_string(),
            model_batch: batch.to_string(),
            ..Settings::default()
        }
    }

    #[test]
    fn a_live_take_starts_from_the_streaming_default() {
        let mut controller = SessionController::new();
        install_catalogue(&mut controller);
        controller.apply_settings(stored_settings(
            "openai/gpt-realtime-whisper",
            "openai/gpt-4o-transcribe",
        ));

        // Both defaults resolve, so the form is skipped entirely and the live
        // take is configured from the streaming slot.
        assert!(controller.choose_mode(SessionMode::Live));

        let config = controller.machine.config();
        assert_eq!(config.provider.as_deref(), Some("vercel-ai-gateway"));
        assert_eq!(
            config.model.as_deref(),
            Some("openai/gpt-realtime-whisper"),
            "a live take starts from the streaming default"
        );
    }

    #[test]
    fn a_live_form_seeds_the_streaming_default() {
        let mut controller = SessionController::new();
        install_catalogue(&mut controller);
        // A retired streaming default leaves the draft incomplete, so the form
        // stays on screen with the provider already chosen.
        controller.apply_settings(Settings {
            model_streaming: "retired/model".to_string(),
            model_batch: "openai/gpt-4o-transcribe".to_string(),
            ..stored_settings("", "")
        });

        assert!(!controller.choose_mode(SessionMode::Live));
        assert_eq!(controller.draft_provider(), Some("vercel-ai-gateway"));
        assert_eq!(controller.draft_model(), None);

        // Choosing the streaming model is enough to continue.
        assert!(controller.set_draft_model("openai/gpt-realtime-whisper"));
        assert!(controller.create_session());
    }

    #[test]
    fn a_file_take_seeds_the_batch_default() {
        let mut controller = SessionController::new();
        install_catalogue(&mut controller);
        controller.apply_settings(stored_settings(
            "openai/gpt-realtime-whisper",
            "openai/gpt-4o-transcribe",
        ));
        assert!(!controller.choose_mode(SessionMode::File));

        controller
            .set_pending_file(PathBuf::from("/tmp/interview.mp4"))
            .expect("a media file opens the form");

        assert_eq!(
            controller.draft_model(),
            Some("openai/gpt-4o-transcribe"),
            "a file take starts from the batch default"
        );
        assert_eq!(
            controller.draft_model_mode(),
            Some(TranscriptionMode::Segmented)
        );
        assert!(controller.is_draft_complete(), "the form is already usable");
    }

    #[test]
    fn a_default_the_catalogue_does_not_know_leaves_the_draft_empty() {
        let mut controller = SessionController::new();
        install_catalogue(&mut controller);
        controller.apply_settings(stored_settings(
            "some/retired-model",
            "some/retired-batch-model",
        ));

        assert!(!controller.choose_mode(SessionMode::Live));

        assert_eq!(controller.draft_provider(), Some("vercel-ai-gateway"));
        assert_eq!(controller.draft_model(), None);
    }

    #[test]
    fn default_model_for_reads_the_slot_of_its_mode() {
        let mut controller = SessionController::new();
        install_catalogue(&mut controller);
        controller.apply_settings(stored_settings(
            "openai/gpt-realtime-whisper",
            "openai/gpt-4o-transcribe",
        ));

        assert_eq!(
            controller.default_model_for(SessionMode::Live),
            Some("openai/gpt-realtime-whisper")
        );
        assert_eq!(
            controller.default_model_for(SessionMode::File),
            Some("openai/gpt-4o-transcribe")
        );
    }

    #[test]
    fn a_legacy_streaming_default_migrates_into_the_streaming_slot() {
        let mut controller = SessionController::new();
        controller.apply_settings(Settings {
            model: "openai/gpt-realtime-whisper".to_string(),
            ..stored_settings("", "")
        });
        assert!(
            controller
                .settings()
                .expect("settings")
                .model_streaming
                .is_empty(),
            "the catalogue has not revealed the mode yet"
        );

        install_catalogue(&mut controller);

        let settings = controller.settings().expect("settings");
        assert_eq!(settings.model_streaming, "openai/gpt-realtime-whisper");
        assert!(settings.model_batch.is_empty());
        assert!(settings.model.is_empty(), "the legacy key is consumed");
    }

    #[test]
    fn a_legacy_batch_default_migrates_into_the_batch_slot() {
        let mut controller = SessionController::new();
        install_catalogue(&mut controller);
        controller.apply_settings(Settings {
            model: "openai/gpt-4o-transcribe".to_string(),
            ..stored_settings("", "")
        });

        let settings = controller.settings().expect("settings");
        assert_eq!(settings.model_batch, "openai/gpt-4o-transcribe");
        assert!(settings.model_streaming.is_empty());
    }

    #[test]
    fn an_unknown_legacy_default_is_left_in_place() {
        let mut controller = SessionController::new();
        install_catalogue(&mut controller);
        controller.apply_settings(Settings {
            model: "retired/model".to_string(),
            ..stored_settings("", "")
        });

        let settings = controller.settings().expect("settings");
        assert!(settings.model_streaming.is_empty());
        assert!(settings.model_batch.is_empty());
        assert_eq!(settings.model, "retired/model");
    }

    #[test]
    fn the_legacy_migration_never_overwrites_a_chosen_slot() {
        let mut controller = SessionController::new();
        install_catalogue(&mut controller);
        controller.apply_settings(Settings {
            model: "openai/gpt-realtime-whisper".to_string(),
            ..stored_settings("", "openai/gpt-4o-transcribe")
        });

        let settings = controller.settings().expect("settings");
        assert_eq!(settings.model_batch, "openai/gpt-4o-transcribe");
        assert!(
            settings.model_streaming.is_empty(),
            "an existing default wins over the legacy one"
        );
    }

    #[test]
    fn the_legacy_migration_is_idempotent() {
        let mut controller = SessionController::new();
        install_catalogue(&mut controller);
        controller.apply_settings(Settings {
            model: "openai/gpt-realtime-whisper".to_string(),
            ..stored_settings("", "")
        });
        let once = controller.settings().expect("settings").clone();

        install_catalogue(&mut controller);

        assert_eq!(controller.settings().expect("settings"), &once);
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

    // ----- The live take's model and clock ------------------------------

    #[test]
    fn the_live_take_names_the_model_the_settings_resolve() {
        let mut controller = SessionController::new();
        install_catalogue(&mut controller);
        controller.apply_settings(stored_settings(
            "openai/gpt-realtime-whisper",
            "openai/gpt-4o-transcribe",
        ));

        assert_eq!(
            controller.live_model_id(),
            Some("openai/gpt-realtime-whisper"),
            "the streaming default is what a live take uses"
        );
        assert_eq!(controller.live_model_label(), Some("GPT Realtime Whisper"));
    }

    #[test]
    fn the_live_take_names_the_model_the_machine_froze() {
        let (mut controller, _sink) = recording_live_take();

        // The take froze its model; the settings may say something else.
        controller.apply_settings(
            SessionController::default()
                .settings()
                .cloned()
                .unwrap_or_default(),
        );
        controller.apply_settings(Settings {
            provider: "vercel-ai-gateway".to_string(),
            model_streaming: "openai/gpt-4o-transcribe".to_string(),
            model_batch: "openai/gpt-4o-transcribe".to_string(),
            ..Settings::default()
        });

        assert_eq!(
            controller.live_model_id(),
            Some("openai/gpt-realtime-whisper"),
            "the frozen configuration wins while a take exists"
        );
    }

    #[test]
    fn a_live_take_with_no_catalogue_names_no_model() {
        let controller = SessionController::new();

        assert_eq!(controller.live_model_id(), None);
        assert_eq!(controller.live_model_label(), None);
        assert!(controller.live_model_options().is_empty());
    }

    #[test]
    fn the_live_picker_offers_every_model_of_the_provider() {
        let controller = live_form();
        let ids: Vec<String> = controller
            .live_model_options()
            .into_iter()
            .map(|option| option.id)
            .collect();

        assert_eq!(
            ids,
            [
                "openai/gpt-4o-transcribe",
                "openai/gpt-realtime-whisper",
                "fish-audio/transcribe-1",
            ],
            "a live take may batch the microphone, so segmented models are offered"
        );
    }

    #[test]
    fn choosing_a_live_model_changes_the_take_but_not_the_default() {
        let mut controller = SessionController::new();
        install_catalogue(&mut controller);
        let mut settings =
            stored_settings("openai/gpt-realtime-whisper", "openai/gpt-4o-transcribe");
        settings.set_api_key("vercel-ai-gateway", "sk-test-key");
        controller.apply_settings(settings);
        assert!(controller.choose_mode(SessionMode::Live));

        assert!(controller.set_live_model("openai/gpt-4o-transcribe"));
        assert_eq!(controller.live_model_id(), Some("openai/gpt-4o-transcribe"));
        assert_eq!(
            controller
                .settings()
                .map(|settings| settings.model_streaming.as_str()),
            Some("openai/gpt-realtime-whisper"),
            "the take-scoped choice never rewrites the Settings default"
        );

        // The next Start carries the picker's model.
        let sink = CommandLog::default();
        controller.set_sink(Box::new(sink.clone()));
        assert!(controller.start_recording_at(Instant::now()));
        match sink.take().first() {
            Some(SttCommand::Start { model, .. }) => {
                assert_eq!(model, "openai/gpt-4o-transcribe");
            }
            other => panic!("expected a start command, got {other:?}"),
        }
    }

    #[test]
    fn a_live_model_cannot_change_once_the_take_is_active() {
        let (mut controller, _sink) = recording_live_take();

        assert!(!controller.set_live_model("openai/gpt-4o-transcribe"));
        assert!(
            !controller.set_live_model("not-a-model"),
            "the catalogue still has the last word"
        );
    }

    #[test]
    fn a_file_take_never_accepts_the_live_picker_choice() {
        let mut controller = file_form();

        assert!(!controller.set_live_model("openai/gpt-4o-transcribe"));
    }

    fn live_form() -> SessionController {
        let mut controller = SessionController::new();
        install_catalogue(&mut controller);
        assert!(!controller.choose_mode(SessionMode::Live));
        controller
    }

    fn file_form() -> SessionController {
        let mut controller = SessionController::new();
        install_catalogue(&mut controller);
        assert!(!controller.choose_mode(SessionMode::File));
        controller
            .set_pending_file(PathBuf::from("/tmp/interview.mp4"))
            .expect("a media file opens the form");
        controller
    }

    #[test]
    fn provider_options_preserve_catalogue_order_and_labels() {
        let controller = live_form();
        let options = controller.draft_provider_options();

        assert_eq!(options.len(), 1);
        assert_eq!(options[0].id, "vercel-ai-gateway");
        assert_eq!(options[0].label, "Vercel AI Gateway");
        assert_eq!(options[0].mode, None, "providers have no model mode");
    }

    #[test]
    fn provider_options_are_empty_before_the_catalogue_arrives() {
        let controller = SessionController::new();
        assert!(controller.draft_provider_options().is_empty());
        assert!(controller.settings_provider_options().is_empty());
    }

    #[test]
    fn live_model_options_offer_every_model_with_its_mode() {
        let mut controller = live_form();
        assert!(controller.set_draft_provider("vercel-ai-gateway"));

        let options = controller.draft_model_options();
        let ids: Vec<_> = options.iter().map(|option| option.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "openai/gpt-4o-transcribe",
                "openai/gpt-realtime-whisper",
                "fish-audio/transcribe-1",
            ]
        );
        assert_eq!(options[0].mode, Some(TranscriptionMode::Segmented));
        assert_eq!(options[1].mode, Some(TranscriptionMode::Streaming));
    }

    #[test]
    fn file_model_options_offer_only_batch_models() {
        let mut controller = file_form();
        assert!(controller.set_draft_provider("vercel-ai-gateway"));

        let options = controller.draft_model_options();
        let ids: Vec<_> = options.iter().map(|option| option.id.as_str()).collect();
        assert_eq!(ids, ["openai/gpt-4o-transcribe", "fish-audio/transcribe-1"]);
        assert!(
            options
                .iter()
                .all(|option| option.mode == Some(TranscriptionMode::Segmented))
        );
    }

    #[test]
    fn model_options_are_empty_without_a_provider() {
        let controller = live_form();
        assert!(controller.draft_model_options().is_empty());
    }

    #[test]
    fn settings_options_follow_the_settings_draft() {
        let mut controller = SessionController::new();
        install_catalogue(&mut controller);
        controller.open_settings();

        let providers = controller.settings_provider_options();
        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0].id, "vercel-ai-gateway");
        assert!(controller.settings_streaming_model_options().is_empty());
        assert!(controller.settings_batch_model_options().is_empty());

        assert!(controller.set_settings_provider("vercel-ai-gateway"));

        let streaming = controller.settings_streaming_model_options();
        let streaming_ids: Vec<_> = streaming.iter().map(|option| option.id.as_str()).collect();
        assert_eq!(
            streaming_ids,
            ["openai/gpt-realtime-whisper"],
            "the streaming menu offers only streaming models"
        );
        assert_eq!(streaming[0].mode, Some(TranscriptionMode::Streaming));

        let batch = controller.settings_batch_model_options();
        let batch_ids: Vec<_> = batch.iter().map(|option| option.id.as_str()).collect();
        assert_eq!(
            batch_ids,
            ["openai/gpt-4o-transcribe", "fish-audio/transcribe-1"],
            "the batch menu offers only segmented models"
        );
        assert!(
            batch
                .iter()
                .all(|option| option.mode == Some(TranscriptionMode::Segmented))
        );
    }

    #[test]
    fn selecting_a_provider_clears_the_draft_model() {
        let mut controller = live_form();
        assert!(controller.set_draft_provider("vercel-ai-gateway"));
        assert!(controller.set_draft_model("openai/gpt-4o-transcribe"));
        assert_eq!(controller.draft_model(), Some("openai/gpt-4o-transcribe"));

        assert!(controller.set_draft_provider("vercel-ai-gateway"));
        assert_eq!(
            controller.draft_model(),
            Some("openai/gpt-4o-transcribe"),
            "re-selecting the same provider keeps its model"
        );

        assert!(
            !controller.set_draft_provider("mistral"),
            "unknown providers are rejected"
        );
        assert_eq!(controller.draft_provider(), Some("vercel-ai-gateway"));
        assert_eq!(controller.draft_model(), Some("openai/gpt-4o-transcribe"));
    }

    #[test]
    fn changing_the_provider_clears_the_draft_model() {
        let mut controller = SessionController::new();
        install_catalogue(&mut controller);
        controller.set_capabilities(vec![
            ProviderCapability {
                id: "first".to_string(),
                label: "First".to_string(),
                available: true,
                key_env_vars: Vec::new(),
                models: vec![ModelCapabilityInfo {
                    id: "first/model".to_string(),
                    label: "First Model".to_string(),
                    mode: TranscriptionMode::Segmented,
                    description: None,
                    pricing: BTreeMap::new(),
                }],
            },
            ProviderCapability {
                id: "second".to_string(),
                label: "Second".to_string(),
                available: true,
                key_env_vars: Vec::new(),
                models: Vec::new(),
            },
        ]);
        assert!(!controller.choose_mode(SessionMode::Live));

        assert!(controller.set_draft_provider("first"));
        assert!(controller.set_draft_model("first/model"));

        assert!(controller.set_draft_provider("second"));
        assert_eq!(controller.draft_model(), None);
    }

    #[test]
    fn set_draft_model_rejects_unknown_models() {
        let mut controller = live_form();
        assert!(controller.set_draft_provider("vercel-ai-gateway"));

        assert!(!controller.set_draft_model("not-a-model"));
        assert_eq!(controller.draft_model(), None);
    }

    #[test]
    fn set_draft_model_rejects_streaming_models_for_files() {
        let mut controller = file_form();
        assert!(controller.set_draft_provider("vercel-ai-gateway"));

        assert!(!controller.set_draft_model("openai/gpt-realtime-whisper"));
        assert_eq!(controller.draft_model(), None);
        assert!(controller.set_draft_model("openai/gpt-4o-transcribe"));
        assert_eq!(controller.draft_model(), Some("openai/gpt-4o-transcribe"));
    }

    #[test]
    fn set_draft_model_accepts_streaming_models_in_live_mode() {
        let mut controller = live_form();
        assert!(controller.set_draft_provider("vercel-ai-gateway"));

        assert!(controller.set_draft_model("openai/gpt-realtime-whisper"));
        assert_eq!(
            controller.draft_model_mode(),
            Some(TranscriptionMode::Streaming)
        );
    }

    #[test]
    fn set_draft_model_requires_a_provider() {
        let mut controller = live_form();
        assert!(!controller.set_draft_model("openai/gpt-4o-transcribe"));
        assert_eq!(controller.draft_model(), None);
    }

    #[test]
    fn cycling_the_provider_keeps_both_default_slots_empty() {
        let mut controller = SessionController::new();
        install_catalogue(&mut controller);
        controller.open_settings();

        assert!(controller.cycle_settings_provider());
        assert_eq!(
            controller.settings_draft().provider.as_deref(),
            Some("vercel-ai-gateway")
        );
        assert_eq!(controller.settings_draft().model_streaming, None);
        assert_eq!(controller.settings_draft().model_batch, None);
    }

    #[test]
    fn each_default_slot_rejects_the_other_modes_model() {
        let mut controller = SessionController::new();
        install_catalogue(&mut controller);
        controller.open_settings();
        assert!(controller.set_settings_provider("vercel-ai-gateway"));

        assert!(
            !controller.set_settings_streaming_model("openai/gpt-4o-transcribe"),
            "a batch model cannot be the live default"
        );
        assert!(
            !controller.set_settings_batch_model("openai/gpt-realtime-whisper"),
            "a streaming model cannot be the file default"
        );
        assert_eq!(controller.settings_draft().model_streaming, None);
        assert_eq!(controller.settings_draft().model_batch, None);

        assert!(controller.set_settings_streaming_model("openai/gpt-realtime-whisper"));
        assert!(controller.set_settings_batch_model("openai/gpt-4o-transcribe"));
    }

    #[test]
    fn changing_the_settings_provider_clears_both_defaults() {
        let mut controller = SessionController::new();
        install_catalogue(&mut controller);
        controller.set_capabilities(vec![
            ProviderCapability {
                id: "vercel-ai-gateway".to_string(),
                label: "Vercel AI Gateway".to_string(),
                available: true,
                key_env_vars: vec!["AI_GATEWAY_API_KEY".to_string()],
                models: test_capabilities()[0].models.clone(),
            },
            ProviderCapability {
                id: "other-provider".to_string(),
                label: "Other".to_string(),
                available: true,
                key_env_vars: vec!["OTHER_KEY".to_string()],
                models: test_capabilities()[0].models.clone(),
            },
        ]);
        controller.open_settings();
        assert!(controller.set_settings_provider("vercel-ai-gateway"));
        assert!(controller.set_settings_streaming_model("openai/gpt-realtime-whisper"));
        assert!(controller.set_settings_batch_model("openai/gpt-4o-transcribe"));

        assert!(controller.set_settings_provider("other-provider"));
        assert_eq!(controller.settings_draft().model_streaming, None);
        assert_eq!(controller.settings_draft().model_batch, None);
    }
}
