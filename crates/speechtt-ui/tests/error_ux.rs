//! T9.1 — the single typed error surface (`PRODUCT.md` Phase 9).
//!
//! Every source error maps to one deterministic `AppError` variant, and no
//! variant can render key material or transcript text.

use speechtt_core::audio::AudioError;
use speechtt_core::backend::{BackendError, SttSessionSink};
use speechtt_core::output::OutputError;
use speechtt_core::protocol::{SttCommand, SttEvent};
use speechtt_core::serde_json;
use speechtt_core::settings::SettingsError;
use speechtt_ui::controller::{AppError, ErrorAction, SessionController, SettingsChange};

#[test]
fn audio_errors_map_to_microphone_variants() {
    assert_eq!(
        AppError::from_audio(&AudioError::PermissionDenied),
        AppError::MicPermissionDenied
    );
    assert_eq!(
        AppError::from_audio(&AudioError::NoDevice),
        AppError::NoInputDevice
    );
    assert_eq!(
        AppError::from_audio(&AudioError::UnsupportedConfig("bad".into())),
        AppError::NoInputDevice
    );
    assert_eq!(
        AppError::from_audio(&AudioError::Backend("stream died".into())),
        AppError::NoInputDevice
    );
}

#[test]
fn backend_errors_map_to_transport_variants() {
    assert_eq!(
        AppError::from_backend(&BackendError::Disconnected),
        AppError::BackendUnavailable
    );
    assert_eq!(
        AppError::from_backend(&BackendError::Spawn {
            program: "bun".into(),
            source: std::io::Error::new(std::io::ErrorKind::NotFound, "missing"),
        }),
        AppError::BackendUnavailable
    );

    let decode: serde_json::Error =
        serde_json::from_str::<SttEvent>("not json").expect_err("fixture is malformed");
    assert!(matches!(
        AppError::from_backend(&BackendError::Decode(decode)),
        AppError::ProtocolError(_)
    ));
}

#[test]
fn stt_error_codes_map_deterministically() {
    assert_eq!(
        AppError::from_stt_error("missing_api_key", "set a key", "mistral"),
        AppError::MissingApiKey {
            provider: "mistral".into()
        }
    );
    for code in [
        "auth_failed",
        "provider_auth_failed",
        "unauthorized",
        "invalid_api_key",
    ] {
        assert_eq!(
            AppError::from_stt_error(code, "rejected", "vercel"),
            AppError::ProviderAuthFailed,
            "code {code}"
        );
    }
    for code in ["network_lost", "network_error", "connection_lost"] {
        assert_eq!(
            AppError::from_stt_error(code, "offline", "vercel"),
            AppError::NetworkLost,
            "code {code}"
        );
    }
    assert_eq!(
        AppError::from_stt_error("timeout", "slow", "vercel"),
        AppError::BackendTimeout
    );
    assert_eq!(
        AppError::from_stt_error("stream_failed", "socket closed", "vercel"),
        AppError::NetworkLost
    );
    assert_eq!(
        AppError::from_stt_error("stream_failed", "api key rejected", "vercel"),
        AppError::ProviderAuthFailed
    );
    assert_eq!(
        AppError::from_stt_error("stream_failed", "unexpected", "vercel"),
        AppError::BackendFailed {
            code: "stream_failed".into(),
            detail: "unexpected".into(),
        }
    );
    assert_eq!(
        AppError::from_stt_error("invalid_protocol", "bad line", "vercel"),
        AppError::ProtocolError("bad line".into())
    );
    assert_eq!(
        AppError::from_stt_error(
            "unsupported_file_model",
            "gpt-realtime cannot transcribe files",
            "vercel"
        ),
        AppError::FileModelNotSupported
    );
    assert_eq!(
        AppError::from_stt_error("new_worker_code", "provider said no", "vercel"),
        AppError::BackendFailed {
            code: "new_worker_code".into(),
            detail: "provider said no".into(),
        }
    );
    assert_eq!(
        AppError::from_stt_error("new_worker_code", "", "vercel"),
        AppError::BackendFailed {
            code: "new_worker_code".into(),
            detail: "no detail provided".into(),
        }
    );
}

#[test]
fn settings_errors_map_to_save_or_protocol_variants() {
    let io = SettingsError::Io {
        path: "/tmp/settings.json".into(),
        source: std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied"),
    };
    assert!(matches!(
        AppError::from_settings(&io),
        AppError::SaveFailed(_)
    ));

    let corrupt = SettingsError::Corrupt {
        path: "/tmp/settings.json".into(),
        source: serde_json::from_str::<serde_json::Value>("{bad")
            .expect_err("fixture is malformed"),
    };
    assert!(matches!(
        AppError::from_settings(&corrupt),
        AppError::ProtocolError(_)
    ));

    let encode = SettingsError::Encode(
        serde_json::from_str::<serde_json::Value>("{bad").expect_err("fixture is malformed"),
    );
    assert!(matches!(
        AppError::from_settings(&encode),
        AppError::SaveFailed(_)
    ));

    assert!(matches!(
        AppError::from_settings(&SettingsError::Invalid("empty".into())),
        AppError::ProtocolError(_)
    ));
}

#[test]
fn output_errors_map_to_their_action_variants() {
    assert_eq!(
        AppError::from_output(OutputError::Clipboard("no display".into())),
        AppError::ClipboardFailed
    );
    assert_eq!(
        AppError::from_output(OutputError::Paste("no display".into())),
        AppError::PasteFailed
    );
    assert!(matches!(
        AppError::from_output(OutputError::Write("disk full".into())),
        AppError::SaveFailed(_)
    ));
    assert!(matches!(
        AppError::from_output(OutputError::UnsupportedFormat("pdf".into())),
        AppError::SaveFailed(_)
    ));
    assert_eq!(
        AppError::from_output(OutputError::EmptyTranscript),
        AppError::NothingRecorded
    );
    assert!(matches!(
        AppError::from_output(OutputError::OpenFile("missing".into())),
        AppError::SaveFailed(_)
    ));
}

#[test]
fn every_error_has_a_short_message_and_one_primary_action() {
    let cases = [
        (AppError::MicPermissionDenied, ErrorAction::SettingsHint),
        (AppError::NoInputDevice, ErrorAction::Dismiss),
        (
            AppError::MissingApiKey {
                provider: "vercel".into(),
            },
            ErrorAction::OpenSettings,
        ),
        (AppError::BackendUnavailable, ErrorAction::Retry),
        (AppError::BackendTimeout, ErrorAction::Retry),
        (AppError::ProviderAuthFailed, ErrorAction::OpenSettings),
        (AppError::NetworkLost, ErrorAction::Retry),
        (AppError::SaveFailed("disk".into()), ErrorAction::Dismiss),
        (AppError::ClipboardFailed, ErrorAction::Dismiss),
        (AppError::PasteFailed, ErrorAction::Dismiss),
        (AppError::ProtocolError("bad".into()), ErrorAction::Dismiss),
        (
            AppError::BackendFailed {
                code: "stream_failed".into(),
                detail: "provider down".into(),
            },
            ErrorAction::Dismiss,
        ),
        (AppError::NothingRecorded, ErrorAction::Dismiss),
        (AppError::FileModelNotSupported, ErrorAction::OpenSettings),
        (AppError::FileNotFound, ErrorAction::Dismiss),
        (AppError::FileUnreadable, ErrorAction::Dismiss),
        (AppError::FfmpegMissing, ErrorAction::Dismiss),
        (AppError::FileDialogFailed, ErrorAction::Dismiss),
    ];

    for (error, action) in cases {
        assert_eq!(error.action(), action, "{error:?}");
        let message = error.message();
        assert!(!message.is_empty(), "{error:?}");
        assert!(
            message.chars().count() <= 160,
            "message is too long: {message}"
        );
    }
}

#[test]
fn dynamic_messages_name_the_cause_and_stay_within_the_strip_budget() {
    let backend = AppError::BackendFailed {
        code: "stream_failed".into(),
        detail: "provider down".into(),
    };
    assert_eq!(
        backend.message(),
        "The transcription backend failed: provider down."
    );
    assert_eq!(backend.detail().as_deref(), Some("provider down"));

    let protocol = AppError::ProtocolError("bad line".into());
    assert_eq!(
        protocol.message(),
        "The transcription backend sent an unexpected message: bad line."
    );

    let save = AppError::SaveFailed("permission denied".into());
    assert_eq!(
        save.message(),
        "Could not save the transcript: permission denied."
    );

    let long = AppError::BackendFailed {
        code: "stream_failed".into(),
        detail: "x".repeat(500),
    };
    assert!(long.message().chars().count() <= 160, "{}", long.message());

    // A provider message that already ends a sentence never doubles the stop.
    let trailing = AppError::BackendFailed {
        code: "stream_failed".into(),
        detail: "Nothing was recorded.".into(),
    };
    assert_eq!(
        trailing.message(),
        "The transcription backend failed: Nothing was recorded."
    );
}

#[test]
fn sanitize_detail_collapses_controls_and_redacts_credentials() {
    let sanitized = speechtt_ui::controller::sanitize_detail(
        "  boom\n\tAuthorization: Bearer sk-live-secret api_key=sk-other key=plain  ",
    );
    for secret in ["sk-live-secret", "sk-other", "plain", "Authorization"] {
        assert!(!sanitized.contains(secret), "{sanitized}");
    }
    assert!(!sanitized.contains('\n'), "{sanitized}");
    assert!(sanitized.contains("[REDACTED]"), "{sanitized}");

    let long = speechtt_ui::controller::sanitize_detail(&"a".repeat(500));
    assert_eq!(long.chars().count(), 120, "{long}");
    assert!(long.ends_with('…'));

    assert_eq!(speechtt_ui::controller::sanitize_detail(""), "");
    assert!(!speechtt_ui::controller::sanitize_detail("   \n\t").is_empty());
}

#[test]
fn diagnostics_format_one_redacted_line_per_typed_error() {
    use speechtt_ui::diagnostics::format_error_line;

    let line = format_error_line(
        "stt_event",
        &AppError::BackendFailed {
            code: "stream_failed".into(),
            detail: "provider said sk-secret".into(),
        },
    );
    assert!(line.starts_with("[speechtt] "), "{line}");
    assert!(line.contains("context=stt_event"), "{line}");
    assert!(line.contains("kind=BackendFailed"), "{line}");
    assert!(line.contains("code=stream_failed"), "{line}");
    assert!(
        line.contains("detail=\"provider said [REDACTED]\""),
        "{line}"
    );
    assert!(!line.contains("sk-secret"), "{line}");
    assert_eq!(line.lines().count(), 1, "{line}");

    let info = format_error_line("controller", &AppError::NothingRecorded);
    assert!(info.contains("kind=NothingRecorded"), "{info}");
    assert!(info.contains("code=-"), "{info}");
}

#[test]
fn error_messages_never_contain_secret_material_or_transcript_text() {
    let secret = "sk-live-super-secret";
    let transcript = "the secret dictation";
    let error = AppError::from_stt_error("missing_api_key", secret, "vercel");

    let rendered = format!("{error} {error:?}");
    assert!(!rendered.contains(secret), "{rendered}");
    assert!(!rendered.contains(transcript), "{rendered}");

    let change = SettingsChange {
        provider: "vercel".into(),
        model: "openai/gpt-realtime-whisper".into(),
        api_key: Some(secret.into()),
    };
    let debug = format!("{change:?}");
    assert!(debug.contains("REDACTED"), "{debug}");
    assert!(!debug.contains(secret), "{debug}");
}

/// A sink that never receives anything; used to drive the controller's event
/// folding without a backend.
struct NullSink;

impl SttSessionSink for NullSink {
    fn send(&mut self, _cmd: SttCommand) -> Result<(), BackendError> {
        Ok(())
    }
}

#[test]
fn controller_stores_and_dismisses_the_typed_error() {
    let mut controller = SessionController::new();
    controller.set_sink(Box::new(NullSink));
    controller.handle_stt_event(SttEvent::Error {
        session_id: None,
        code: "auth_failed".into(),
        message: "401".into(),
    });

    assert_eq!(controller.app_error(), Some(&AppError::ProviderAuthFailed));
    assert_eq!(controller.error_action(), Some(ErrorAction::OpenSettings));

    controller.dismiss_error();
    assert!(controller.app_error().is_none());
    assert!(controller.last_error().is_none());
}

#[test]
fn inbound_stt_errors_name_their_code_and_sanitized_detail() {
    use speechtt_ui::controller::sanitize_detail;
    use speechtt_ui::diagnostics::format_error_line;

    let mut controller = SessionController::new();
    controller.set_sink(Box::new(NullSink));
    controller.handle_stt_event(SttEvent::Error {
        session_id: None,
        code: "stream_failed".into(),
        message: "upstream refused sk-live-secret".into(),
    });

    assert_eq!(
        controller.app_error(),
        Some(&AppError::BackendFailed {
            code: "stream_failed".into(),
            detail: "upstream refused [REDACTED]".into(),
        })
    );
    let message = controller
        .app_error()
        .map(AppError::message)
        .unwrap_or_default();
    assert!(message.contains("upstream refused"), "{message}");
    assert!(!message.contains("unexpected message"), "{message}");
    assert!(!message.contains("sk-live-secret"), "{message}");

    let line = format_error_line(
        "stt_event",
        &AppError::BackendFailed {
            code: "stream_failed".into(),
            detail: sanitize_detail("upstream refused sk-live-secret"),
        },
    );
    assert!(line.contains("code=stream_failed"), "{line}");
    assert!(line.contains("upstream refused [REDACTED]"), "{line}");
    assert!(!line.contains("sk-live-secret"), "{line}");
}

#[test]
fn stale_errors_from_another_take_do_not_replace_the_current_error() {
    let mut controller = SessionController::new();
    controller.set_sink(Box::new(NullSink));
    controller.handle_stt_event(SttEvent::Error {
        session_id: None,
        code: "invalid_protocol".into(),
        message: "bad".into(),
    });

    // No take has started, so any session-scoped error is stale.
    controller.handle_stt_event(SttEvent::Error {
        session_id: Some("take-1".into()),
        code: "auth_failed".into(),
        message: "401".into(),
    });

    assert_eq!(
        controller.app_error(),
        Some(&AppError::ProtocolError("bad".into()))
    );
}
