//! Redacted, single-line diagnostics for typed application errors.
//!
//! The status strip only shows a short user-facing sentence; this module
//! preserves the cause on `stderr` so a failure can be reconstructed after the
//! strip is dismissed. Log lines never contain transcript text or secrets.
//!
//! Read by `controller::SessionController::report_error` and by the event pump
//! in `controller::SessionController::handle_stt_event`; nothing else writes an
//! error line. `lib.rs::run` writes the launch handshake ([`log_launched`])
//! through the same channel, and `tests/launch.rs` waits for it to judge a real
//! launch. `app::SpeechTtApp` writes the two startup markers — the Settings
//! self-check ([`log_settings_recovery`]) and the first catalogue
//! ([`log_catalogue_loaded`] / [`log_catalogue_respawned`]) — here as well, so
//! every `[speechtt]` line has one writer and one format.

use crate::controller::{AppError, sanitize_detail};
use speechtt_core::protocol::SttEvent;

/// Status token of the launch handshake: the widget's window is open.
const LAUNCHED_STATUS: &str = "window-open";

/// The line the widget writes once its window is open and its view is built.
///
/// `run()` writes it exactly once, through [`log_launched`], after
/// `gpui_kit::open_window` returns. It says the window exists and
/// `SpeechTtApp::new` has returned; the first frame's render follows it, which
/// is why a launcher waits for the process to stay alive afterwards as well.
///
/// Carries no path, no settings value and no credential: it is a handshake, not
/// a boot log. `tests/launch.rs` waits for this exact line and takes the text
/// from this function rather than restating it, so the gate cannot drift from
/// the binary it launches.
#[must_use]
pub fn launched_line() -> String {
    format!("[speechtt] startup status={LAUNCHED_STATUS}")
}

/// Write the launch line [`launched_line`] describes.
///
/// Called by `lib.rs::run` once the window is open; nothing else writes it.
pub fn log_launched() {
    eprintln!("{}", launched_line());
}

/// The line the startup self-check writes when it opens Settings:
/// `[speechtt] startup settings=open reason=<reason>`.
///
/// `reason` is a fixed vocabulary (`bun-missing`, `backend-unavailable`,
/// `api-key-missing`) and carries no user data. A headless launch cannot show
/// the Settings window, so this line is how the self-check is observed from
/// outside the process.
#[must_use]
pub fn settings_recovery_line(reason: &str) -> String {
    format!("[speechtt] startup settings=open reason={reason}")
}

/// Write the line [`settings_recovery_line`] describes.
///
/// Called by `app::SpeechTtApp` when it opens Settings at startup; nothing else
/// writes it.
pub fn log_settings_recovery(reason: &str) {
    eprintln!("{}", settings_recovery_line(reason));
}

/// The line the first catalogue of the startup worker writes:
/// `[speechtt] startup catalogue=loaded providers=<n>`.
///
/// The first catalogue proves the worker answered the handshake, which a
/// headless run cannot show any other way.
#[must_use]
pub fn catalogue_loaded_line(providers: usize) -> String {
    format!("[speechtt] startup catalogue=loaded providers={providers}")
}

/// Write the line [`catalogue_loaded_line`] describes.
///
/// Called by `app::SpeechTtApp`'s event pump for the first catalogue of the
/// worker the startup spawn attached.
pub fn log_catalogue_loaded(providers: usize) {
    eprintln!("{}", catalogue_loaded_line(providers));
}

/// The line for a first catalogue that arrived after a save respawned the
/// worker: `[speechtt] catalogue=loaded providers=<n>`.
///
/// It deliberately omits the `startup` prefix: the catalogue is the evidence
/// that a save recovered the backend, not that the launch was healthy.
#[must_use]
pub fn catalogue_respawned_line(providers: usize) -> String {
    format!("[speechtt] catalogue=loaded providers={providers}")
}

/// Write the line [`catalogue_respawned_line`] describes.
///
/// Called by `app::SpeechTtApp`'s event pump when the catalogue belongs to a
/// worker the save-triggered respawn attached.
pub fn log_catalogue_respawned(providers: usize) {
    eprintln!("{}", catalogue_respawned_line(providers));
}

/// Write exactly one redacted diagnostic line for `error`.
///
/// [`AppError::NothingRecorded`] is informational, so it produces no error
/// line at all.
pub fn log_error(context: &str, error: &AppError) {
    if let Some(line) = loggable_line(context, error) {
        eprintln!("{line}");
    }
}

/// The line [`log_error`] writes, or `None` for informational outcomes.
fn loggable_line(context: &str, error: &AppError) -> Option<String> {
    if matches!(error, AppError::NothingRecorded) {
        return None;
    }
    Some(format_error_line(context, error))
}

/// Format one physical diagnostic line:
/// `[speechtt] context=<context> kind=<Variant> code=<code|-> detail="<detail>"`.
#[must_use]
pub fn format_error_line(context: &str, error: &AppError) -> String {
    let code = match error {
        AppError::BackendFailed { code, .. } => code.as_str(),
        _ => "-",
    };
    let detail = error.detail().unwrap_or_default();
    format!(
        "[speechtt] context={} kind={} code={} detail=\"{}\"",
        sanitize_detail(context),
        variant_name(error),
        code,
        detail
    )
}

/// Stable variant name used by the `kind=` field.
fn variant_name(error: &AppError) -> &'static str {
    match error {
        AppError::MicPermissionDenied => "MicPermissionDenied",
        AppError::NoInputDevice => "NoInputDevice",
        AppError::MissingApiKey { .. } => "MissingApiKey",
        AppError::BackendUnavailable => "BackendUnavailable",
        AppError::BackendTimeout => "BackendTimeout",
        AppError::ProviderAuthFailed => "ProviderAuthFailed",
        AppError::NetworkLost => "NetworkLost",
        AppError::SaveFailed(_) => "SaveFailed",
        AppError::ClipboardFailed => "ClipboardFailed",
        AppError::PasteFailed => "PasteFailed",
        AppError::ProtocolError(_) => "ProtocolError",
        AppError::BackendFailed { .. } => "BackendFailed",
        AppError::CleanupFailed => "CleanupFailed",
        AppError::FileModelNotSupported => "FileModelNotSupported",
        AppError::UnsupportedFileType => "UnsupportedFileType",
        AppError::NothingRecorded => "NothingRecorded",
        AppError::FileNotFound => "FileNotFound",
        AppError::FileUnreadable => "FileUnreadable",
        AppError::FfmpegMissing => "FfmpegMissing",
        AppError::FileDialogFailed => "FileDialogFailed",
    }
}

/// Environment variable that switches the transcript trace on.
///
/// The trace is a diagnosis tool, not part of the running app's output: it is
/// off unless the variable is exactly `1`, so a normal run writes nothing.
pub const TRACE_ENV: &str = "SPEECHTT_STT_TRACE";

/// Whether the transcript trace is enabled, i.e. `SPEECHTT_STT_TRACE=1`.
#[must_use]
pub fn transcript_trace_enabled() -> bool {
    std::env::var_os(TRACE_ENV).is_some_and(|value| value == "1")
}

/// Write one trace line for `event`, when the trace is enabled.
///
/// `accepted` says whether the controller took the event or dropped it as a
/// stale session, which is the half of a missing transcript a user cannot see.
pub fn log_stt_event(event: &SttEvent, accepted: bool) {
    if let Some(line) = format_event_line(event, accepted) {
        eprintln!("{line}");
    }
}

/// The trace line for `event`, or `None` for an event that never carries
/// transcript text.
///
/// One physical line, in one of two shapes:
///
/// - `[speechtt] stt type=<type> session=<id> accepted=<bool> segment=<identity|->
///   len=<chars>` for the take events;
/// - `[speechtt] stt type=cleaned session=- accepted=<bool> request=<id> len=<chars>`
///   for a repaired transcript, which has no session and is correlated by its
///   cleanup request instead.
///
/// Both carry the text's length and an identity, never the text itself, so a
/// take can be reconstructed without transcribing anything to a log.
#[must_use]
pub fn format_event_line(event: &SttEvent, accepted: bool) -> Option<String> {
    let (kind, session_id, segment, length) = match event {
        SttEvent::Partial {
            session_id,
            text,
            segment,
        } => (
            "partial",
            session_id,
            segment.as_deref(),
            text.chars().count(),
        ),
        SttEvent::Delta {
            session_id,
            delta,
            segment,
        } => (
            "delta",
            session_id,
            segment.as_deref(),
            delta.chars().count(),
        ),
        SttEvent::Committed {
            session_id,
            text,
            segment,
        } => (
            "committed",
            session_id,
            segment.as_deref(),
            text.chars().count(),
        ),
        SttEvent::Stopped {
            session_id,
            full_text,
        } => ("stopped", session_id, None, full_text.chars().count()),
        SttEvent::Cleaned { request_id, text } => {
            return Some(format!(
                "[speechtt] stt type=cleaned session=- accepted={accepted} request={} len={}",
                sanitize_detail(request_id),
                text.chars().count()
            ));
        }
        _ => return None,
    };

    Some(format!(
        "[speechtt] stt type={kind} session={} accepted={accepted} segment={} len={length}",
        sanitize_detail(session_id),
        segment.map_or_else(|| "-".to_string(), sanitize_detail)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_exactly_one_redacted_line() {
        let line = format_error_line(
            "stt_event",
            &AppError::BackendFailed {
                code: "stream_failed".into(),
                detail: "provider rejected sk-live-secret".into(),
            },
        );

        assert_eq!(
            line,
            "[speechtt] context=stt_event kind=BackendFailed code=stream_failed detail=\"provider rejected [REDACTED]\""
        );
        assert_eq!(line.lines().count(), 1);
    }

    #[test]
    fn omits_the_code_when_the_variant_has_none() {
        let line = format_error_line("controller", &AppError::NetworkLost);

        assert_eq!(
            line,
            "[speechtt] context=controller kind=NetworkLost code=- detail=\"\""
        );
    }

    #[test]
    fn nothing_recorded_produces_no_loggable_line() {
        assert!(loggable_line("controller", &AppError::NothingRecorded).is_none());
        assert!(loggable_line("controller", &AppError::BackendUnavailable).is_some());
    }

    /// The startup markers keep the shape T40.11's artifact check observes, and
    /// a respawned catalogue is never labelled as a startup one.
    #[test]
    fn the_startup_markers_keep_their_command_line_shape() {
        assert_eq!(
            settings_recovery_line("bun-missing"),
            "[speechtt] startup settings=open reason=bun-missing"
        );
        assert_eq!(
            catalogue_loaded_line(1),
            "[speechtt] startup catalogue=loaded providers=1"
        );
        assert_eq!(
            catalogue_respawned_line(2),
            "[speechtt] catalogue=loaded providers=2"
        );
        assert!(
            !catalogue_respawned_line(2).contains("startup"),
            "a save respawn is not a startup event"
        );
        assert_eq!(catalogue_respawned_line(2).lines().count(), 1);
    }

    /// The trace carries the shape of a text event — type, identity, accepted,
    /// length — so a take can be reconstructed, and never the words.
    #[test]
    fn a_text_event_traces_its_shape_without_its_text() {
        let partial = SttEvent::Partial {
            session_id: "take-1".into(),
            text: "private words".into(),
            segment: Some("take-1:0".into()),
        };
        let line = format_event_line(&partial, true).expect("a text event is traced");
        assert_eq!(
            line,
            "[speechtt] stt type=partial session=take-1 accepted=true segment=take-1:0 len=13"
        );
        assert!(
            !line.contains("private"),
            "no transcript text in a trace line"
        );
    }

    #[test]
    fn a_delta_and_a_final_trace_their_own_type() {
        let delta = SttEvent::Delta {
            session_id: "take-1".into(),
            delta: " more".into(),
            segment: None,
        };
        let committed = SttEvent::Committed {
            session_id: "take-1".into(),
            text: "words.".into(),
            segment: None,
        };
        let stopped = SttEvent::Stopped {
            session_id: "take-1".into(),
            full_text: "words.".into(),
        };

        assert_eq!(
            format_event_line(&delta, true).expect("traced"),
            "[speechtt] stt type=delta session=take-1 accepted=true segment=- len=5"
        );
        assert_eq!(
            format_event_line(&committed, false).expect("traced"),
            "[speechtt] stt type=committed session=take-1 accepted=false segment=- len=6"
        );
        assert_eq!(
            format_event_line(&stopped, true).expect("traced"),
            "[speechtt] stt type=stopped session=take-1 accepted=true segment=- len=6"
        );
    }

    #[test]
    fn a_repaired_transcript_traces_its_request_and_length_without_the_words() {
        let line = format_event_line(
            &SttEvent::Cleaned {
                request_id: "clean-1-2".into(),
                text: "d'une dictée".into(),
            },
            true,
        )
        .expect("a cleanup answer is traced");

        assert_eq!(
            line,
            "[speechtt] stt type=cleaned session=- accepted=true request=clean-1-2 len=12"
        );
        assert!(
            !line.contains("dictée"),
            "no transcript text in a trace line"
        );
        assert_eq!(line.lines().count(), 1);
    }

    #[test]
    fn a_non_text_event_is_not_traced() {
        assert!(format_event_line(&SttEvent::Ready, true).is_none());
        assert!(
            format_event_line(
                &SttEvent::Started {
                    session_id: "take-1".into()
                },
                true
            )
            .is_none()
        );
    }
}
