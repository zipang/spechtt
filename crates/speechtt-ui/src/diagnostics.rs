//! Redacted, single-line diagnostics for typed application errors.
//!
//! The status strip only shows a short user-facing sentence; this module
//! preserves the cause on `stderr` so a failure can be reconstructed after the
//! strip is dismissed. Log lines never contain transcript text or secrets.
//!
//! Read by `controller::SessionController::report_error` and by the event pump
//! in `controller::SessionController::handle_stt_event`; nothing else writes a
//! diagnostic line.

use crate::controller::{AppError, sanitize_detail};
use speechtt_core::protocol::SttEvent;

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
