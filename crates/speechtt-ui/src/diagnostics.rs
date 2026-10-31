//! Redacted, single-line diagnostics for typed application errors.
//!
//! The status strip only shows a short user-facing sentence; this module
//! preserves the cause on `stderr` so a failure can be reconstructed after the
//! strip is dismissed. Log lines never contain transcript text or secrets.

use crate::controller::{AppError, sanitize_detail};

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
        AppError::FileModelNotSupported => "FileModelNotSupported",
        AppError::UnsupportedFileType => "UnsupportedFileType",
        AppError::NothingRecorded => "NothingRecorded",
        AppError::FileNotFound => "FileNotFound",
        AppError::FileUnreadable => "FileUnreadable",
        AppError::FfmpegMissing => "FfmpegMissing",
        AppError::FileDialogFailed => "FileDialogFailed",
    }
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
}
