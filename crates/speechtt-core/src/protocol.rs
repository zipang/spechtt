//! NDJSON wire protocol shared with the Bun STT worker (`specs/T0003.md`).
//!
//! Every message is one JSON object terminated by `\n`. Rust writes
//! [`SttCommand`]s to the worker's `stdin` and reads [`SttEvent`]s from its
//! `stdout`; `stderr` is diagnostics only.
//!
//! Secrets travel as [`SecretString`], whose `Debug`/`Display` output is always
//! `"[REDACTED]"`, so a stray `dbg!` or log line cannot leak an API key.

use std::fmt;

use base64::Engine as _;
use serde::{Deserialize, Serialize};

/// An API key that never reveals itself in `Debug` or `Display` output.
///
/// Serialization is transparent: the worker receives the real key.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SecretString(String);

impl SecretString {
    pub fn new(secret: impl Into<String>) -> Self {
        Self(secret.into())
    }

    /// The real value. Call this only where the secret is actually needed
    /// (process environment, HTTP headers); never pass it to a formatter.
    #[must_use]
    pub fn expose_secret(&self) -> &str {
        &self.0
    }
}

impl From<String> for SecretString {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for SecretString {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

impl fmt::Debug for SecretString {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("SecretString")
            .field(&"[REDACTED]")
            .finish()
    }
}

impl fmt::Display for SecretString {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("[REDACTED]")
    }
}

/// A command sent Rust → Bun over the worker's `stdin`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
#[non_exhaustive]
pub enum SttCommand {
    /// Open a streaming transcription session.
    #[serde(rename_all = "camelCase")]
    Start {
        session_id: String,
        provider: String,
        model: String,
        sample_rate: u32,
        #[serde(skip_serializing_if = "Option::is_none")]
        api_key: Option<SecretString>,
    },
    /// One ~40–50 ms chunk of 16-bit little-endian mono PCM, base64 encoded.
    #[serde(rename_all = "camelCase")]
    Audio {
        pcm16_base64: String,
    },
    Pause,
    Resume,
    Stop,
}

impl SttCommand {
    /// Base64-encode raw PCM16 samples as an [`SttCommand::Audio`] chunk.
    #[must_use]
    pub fn audio_from_pcm16(samples: &[i16]) -> Self {
        let mut bytes = Vec::with_capacity(samples.len() * 2);
        for sample in samples {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        Self::Audio {
            pcm16_base64: base64::engine::general_purpose::STANDARD.encode(bytes),
        }
    }

    /// Convert normalized `-1.0..=1.0` float samples to PCM16 and encode them.
    #[must_use]
    pub fn audio_from_f32(samples: &[f32]) -> Self {
        let pcm: Vec<i16> = samples.iter().copied().map(f32_to_pcm16).collect();
        Self::audio_from_pcm16(&pcm)
    }
}

/// A streaming event sent Bun → Rust over the worker's `stdout`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
#[non_exhaustive]
pub enum SttEvent {
    /// The worker finished booting and can accept commands.
    Ready,
    #[serde(rename_all = "camelCase")]
    Started { session_id: String },
    /// Uncommitted live hypothesis (replaces the live tail).
    #[serde(rename_all = "camelCase")]
    Partial { session_id: String, text: String },
    /// Incremental token delta (appends to the live tail).
    #[serde(rename_all = "camelCase")]
    Delta { session_id: String, delta: String },
    /// A finalized segment, moved from the live tail into the transcript.
    #[serde(rename_all = "camelCase")]
    Committed { session_id: String, text: String },
    /// Final flush emitted after `stop` completes.
    #[serde(rename_all = "camelCase")]
    Stopped {
        session_id: String,
        full_text: String,
    },
    #[serde(rename_all = "camelCase")]
    Error {
        #[serde(default)]
        session_id: Option<String>,
        code: String,
        message: String,
    },
}

impl SttEvent {
    /// The session this event belongs to, when it carries one.
    ///
    /// [`SttEvent::Ready`] is session-agnostic; an [`SttEvent::Error`] may be
    /// reported before a session exists.
    #[must_use]
    pub fn session_id(&self) -> Option<&str> {
        match self {
            SttEvent::Ready => None,
            SttEvent::Started { session_id }
            | SttEvent::Partial { session_id, .. }
            | SttEvent::Delta { session_id, .. }
            | SttEvent::Committed { session_id, .. }
            | SttEvent::Stopped { session_id, .. } => Some(session_id),
            SttEvent::Error { session_id, .. } => session_id.as_deref(),
        }
    }
}

/// Map one float sample to PCM16, clamping out-of-range input.
fn f32_to_pcm16(sample: f32) -> i16 {
    // `i16::MAX` (not `MIN.abs()`) keeps the range symmetric and matches what
    // capture backends emit. NaN casts to 0, which is a safe silence frame.
    (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)).round() as i16
}

#[cfg(test)]
mod tests {
    use super::*;

    fn start(api_key: Option<SecretString>) -> SttCommand {
        SttCommand::Start {
            session_id: "take-1".to_string(),
            provider: "vercel".to_string(),
            model: "whisper-large-v3".to_string(),
            sample_rate: 16_000,
            api_key,
        }
    }

    #[test]
    fn start_serializes_as_camel_case_and_omits_an_absent_key() {
        let json = serde_json::to_string(&start(None)).expect("serializes");

        assert_eq!(
            json,
            r#"{"type":"start","sessionId":"take-1","provider":"vercel","model":"whisper-large-v3","sampleRate":16000}"#
        );
    }

    #[test]
    fn start_serializes_an_api_key_but_redacts_it_in_debug() {
        let command = start(Some(SecretString::new("sk-live-secret")));

        let json = serde_json::to_string(&command).expect("serializes");
        assert!(json.contains(r#""apiKey":"sk-live-secret""#), "{json}");

        let debug = format!("{command:?}");
        assert!(debug.contains("[REDACTED]"), "{debug}");
        assert!(!debug.contains("sk-live-secret"), "{debug}");
    }

    #[test]
    fn secret_string_never_exposes_its_value_through_debug_or_display() {
        let secret = SecretString::new("sk-live-secret");

        assert_eq!(format!("{secret:?}"), r#"SecretString("[REDACTED]")"#);
        assert!(!format!("{secret}").contains("sk-live-secret"));
        assert_eq!(secret.expose_secret(), "sk-live-secret");
    }

    #[test]
    fn lifecycle_commands_serialize_as_single_tagged_lines() {
        let cases = [
            (SttCommand::Pause, r#"{"type":"pause"}"#),
            (SttCommand::Resume, r#"{"type":"resume"}"#),
            (SttCommand::Stop, r#"{"type":"stop"}"#),
        ];

        for (command, expected) in cases {
            assert_eq!(
                serde_json::to_string(&command).expect("serializes"),
                expected
            );
            assert_eq!(
                serde_json::from_str::<SttCommand>(expected).expect("deserializes"),
                command
            );
        }
    }

    #[test]
    fn pcm16_helpers_encode_little_endian_samples_as_base64() {
        let command = SttCommand::audio_from_pcm16(&[-1, 0, 1]);

        let SttCommand::Audio { pcm16_base64 } = &command else {
            panic!("expected an audio command, got {command:?}");
        };
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(pcm16_base64)
            .expect("valid base64");
        assert_eq!(bytes, vec![0xff, 0xff, 0x00, 0x00, 0x01, 0x00]);

        let json = serde_json::to_string(&command).expect("serializes");
        assert_eq!(
            json,
            format!(r#"{{"type":"audio","pcm16Base64":"{pcm16_base64}"}}"#)
        );
        assert_eq!(
            serde_json::from_str::<SttCommand>(&json).expect("round-trips"),
            command
        );
    }

    #[test]
    fn f32_samples_are_clamped_and_rounded_to_pcm16() {
        let command = SttCommand::audio_from_f32(&[0.0, 1.0, -1.0, 0.5, 2.0, -2.0]);

        let SttCommand::Audio { pcm16_base64 } = &command else {
            panic!("expected an audio command, got {command:?}");
        };
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(pcm16_base64)
            .expect("valid base64");
        let (pairs, remainder) = bytes.as_chunks::<2>();
        assert!(remainder.is_empty(), "PCM16 is an even number of bytes");
        let samples: Vec<i16> = pairs.iter().map(|pair| i16::from_le_bytes(*pair)).collect();

        assert_eq!(samples, vec![0, 32_767, -32_767, 16_384, 32_767, -32_767]);
    }

    #[test]
    fn every_event_serializes_as_camel_case_ndjson() {
        let cases = [
            (SttEvent::Ready, r#"{"type":"ready"}"#),
            (
                SttEvent::Started {
                    session_id: "take-1".into(),
                },
                r#"{"type":"started","sessionId":"take-1"}"#,
            ),
            (
                SttEvent::Partial {
                    session_id: "take-1".into(),
                    text: "audio".into(),
                },
                r#"{"type":"partial","sessionId":"take-1","text":"audio"}"#,
            ),
            (
                SttEvent::Delta {
                    session_id: "take-1".into(),
                    delta: " capture".into(),
                },
                r#"{"type":"delta","sessionId":"take-1","delta":" capture"}"#,
            ),
            (
                SttEvent::Committed {
                    session_id: "take-1".into(),
                    text: "audio capture layer.".into(),
                },
                r#"{"type":"committed","sessionId":"take-1","text":"audio capture layer."}"#,
            ),
            (
                SttEvent::Stopped {
                    session_id: "take-1".into(),
                    full_text: "audio capture layer.".into(),
                },
                r#"{"type":"stopped","sessionId":"take-1","fullText":"audio capture layer."}"#,
            ),
            (
                SttEvent::Error {
                    session_id: Some("take-1".into()),
                    code: "stream_failed".into(),
                    message: "socket closed".into(),
                },
                r#"{"type":"error","sessionId":"take-1","code":"stream_failed","message":"socket closed"}"#,
            ),
        ];

        for (event, expected) in cases {
            assert_eq!(serde_json::to_string(&event).expect("serializes"), expected);
            assert_eq!(
                serde_json::from_str::<SttEvent>(expected).expect("deserializes"),
                event
            );
        }
    }

    #[test]
    fn an_error_without_a_session_id_defaults_to_none() {
        let event: SttEvent =
            serde_json::from_str(r#"{"type":"error","code":"missing_api_key","message":"no key"}"#)
                .expect("deserializes");

        assert_eq!(
            event,
            SttEvent::Error {
                session_id: None,
                code: "missing_api_key".into(),
                message: "no key".into(),
            }
        );
    }

    #[test]
    fn event_session_ids_are_available_for_stale_packet_filtering() {
        assert_eq!(SttEvent::Ready.session_id(), None);
        assert_eq!(
            SttEvent::Partial {
                session_id: "take-2".into(),
                text: String::new(),
            }
            .session_id(),
            Some("take-2")
        );
        assert_eq!(
            SttEvent::Error {
                session_id: None,
                code: "aborted".into(),
                message: String::new(),
            }
            .session_id(),
            None
        );
    }

    #[test]
    fn unknown_message_types_are_rejected_rather_than_guessed() {
        assert!(serde_json::from_str::<SttCommand>(r#"{"type":"bogus"}"#).is_err());
        assert!(serde_json::from_str::<SttEvent>(r#"{"type":"bogus"}"#).is_err());
    }
}
