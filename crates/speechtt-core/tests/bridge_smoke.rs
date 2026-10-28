//! Live cross-language smoke test: Rust [`BunSubprocess`] ↔ `src/stt/worker.ts`.
//!
//! Unlike the unit tests, these spawn a real Bun process. They never contact a
//! provider and never need an API key: the child inherits a scrubbed
//! environment, so every `available` flag is deterministic.
//!
//! The tests skip cleanly (returning early with a note on stderr) when `bun` is
//! not on `PATH` or the worker script is missing, so CI without Bun stays
//! green. Every read has a hard timeout; a stuck worker fails instead of
//! hanging the suite.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, Stdio};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use async_channel::TryRecvError;
use speechtt_core::backend::{BackendError, BunSubprocess, SttEventReceiver, WORKER_SCRIPT};
use speechtt_core::protocol::{ProviderCapability, SttCommand, SttEvent, TranscriptionMode};

/// Every variable the registry consults. They are cleared before spawning so
/// the availability flags do not depend on the developer's shell.
const PROVIDER_ENV_VARS: [&str; 6] = [
    "AI_GATEWAY_API_KEY",
    "VERCEL_AI_GATEWAY_API_KEY",
    "MISTRAL_API_KEY",
    "OPENAI_API_KEY",
    "XAI_API_KEY",
    "ELEVENLABS_API_KEY",
];

/// The environment is process-global: serialize the smoke tests so one test's
/// scrub cannot race the other.
static ENV_LOCK: Mutex<()> = Mutex::new(());

/// No single read may take longer than this. A worker that stops responding
/// must fail the test rather than hang CI.
const READ_TIMEOUT: Duration = Duration::from_secs(10);

/// Clears provider keys for the duration of a test and restores them after.
struct ScrubbedEnv {
    _lock: MutexGuard<'static, ()>,
    saved: Vec<(&'static str, Option<OsString>)>,
}

impl ScrubbedEnv {
    fn new() -> Self {
        let lock = ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let saved = PROVIDER_ENV_VARS
            .iter()
            .map(|name| (*name, std::env::var_os(name)))
            .collect();
        for name in PROVIDER_ENV_VARS {
            // SAFETY: this test binary owns the environment while `ENV_LOCK`
            // is held; no other thread reads or writes it during this window.
            unsafe { std::env::remove_var(name) };
        }
        Self { _lock: lock, saved }
    }
}

impl Drop for ScrubbedEnv {
    fn drop(&mut self) {
        for (name, value) in self.saved.drain(..) {
            // SAFETY: same contract as `ScrubbedEnv::new`; the lock is still held.
            unsafe {
                match value {
                    Some(value) => std::env::set_var(name, value),
                    None => std::env::remove_var(name),
                }
            }
        }
    }
}

/// Outcome of one bounded read from the worker's event stream.
#[derive(Debug)]
enum ReadOutcome {
    Event(SttEvent),
    /// The worker exited and closed `stdout` without a protocol error.
    Closed,
    /// The transport or JSON decoding failed.
    Failed(BackendError),
    TimedOut,
}

fn next_event(events: &SttEventReceiver, timeout: Duration) -> ReadOutcome {
    let deadline = Instant::now() + timeout;
    loop {
        match events.try_recv() {
            Ok(Ok(event)) => return ReadOutcome::Event(event),
            Ok(Err(error)) => return ReadOutcome::Failed(error),
            Err(TryRecvError::Closed) => return ReadOutcome::Closed,
            Err(TryRecvError::Empty) => {
                if Instant::now() >= deadline {
                    return ReadOutcome::TimedOut;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }
}

fn expect_event(events: &SttEventReceiver) -> SttEvent {
    match next_event(events, READ_TIMEOUT) {
        ReadOutcome::Event(event) => event,
        ReadOutcome::Failed(error) => panic!("worker transport failed: {error}"),
        other => panic!("expected a worker event within {READ_TIMEOUT:?}, got {other:?}"),
    }
}

fn worker_script() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join(WORKER_SCRIPT)
}

fn bun_is_available() -> bool {
    ProcessCommand::new("bun")
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// The worker script path, or `None` when the scenario must be skipped.
fn smoke_target() -> Option<PathBuf> {
    let script = worker_script();
    if !script.is_file() {
        eprintln!(
            "skipping live bridge smoke test: worker script {} is missing",
            script.display()
        );
        return None;
    }
    if !bun_is_available() {
        eprintln!("skipping live bridge smoke test: `bun` is not on PATH");
        return None;
    }
    Some(script)
}

/// Asserts the catalogue invariants the Settings dialog relies on.
fn assert_provider_catalogue(providers: &[ProviderCapability]) {
    assert!(
        providers.len() >= 5,
        "expected at least five providers, got {}",
        providers.len()
    );

    let mut previous_provider: Option<&str> = None;
    for provider in providers {
        assert!(!provider.id.is_empty(), "provider id must not be empty");
        assert!(
            !provider.label.is_empty(),
            "provider label must not be empty"
        );
        assert!(
            !provider.available,
            "{} must be unavailable without an API key",
            provider.id
        );
        assert!(
            !provider.models.is_empty(),
            "{} must list its models even when unavailable",
            provider.id
        );

        if let Some(previous) = previous_provider {
            assert!(
                previous <= provider.label.as_str(),
                "providers must be sorted by label: {previous:?} > {:?}",
                provider.label
            );
        }
        previous_provider = Some(&provider.label);

        let mut previous_model: Option<&str> = None;
        for model in &provider.models {
            assert!(!model.id.is_empty(), "model id must not be empty");
            assert!(!model.label.is_empty(), "model label must not be empty");
            assert!(
                matches!(
                    model.mode,
                    TranscriptionMode::Streaming | TranscriptionMode::Segmented
                ),
                "{} has an invalid mode",
                model.id
            );

            if let Some(previous) = previous_model {
                assert!(
                    previous <= model.label.as_str(),
                    "models of {} must be sorted by label: {previous:?} > {:?}",
                    provider.id,
                    model.label
                );
            }
            previous_model = Some(&model.label);
        }
    }
}

#[test]
fn live_worker_answers_a_capabilities_round_trip() {
    let _env = ScrubbedEnv::new();
    let Some(script) = smoke_target() else {
        return;
    };

    let mut worker = BunSubprocess::spawn(&script).expect("spawns the Bun worker");
    let events = worker.events();

    assert_eq!(expect_event(&events), SttEvent::Ready);

    worker
        .send(&SttCommand::Capabilities {
            request_id: Some("smoke-1".to_string()),
            api_key: None,
        })
        .expect("sends a capabilities request");

    let SttEvent::Capabilities {
        request_id,
        providers,
        ..
    } = expect_event(&events)
    else {
        panic!("expected a capabilities event");
    };
    assert_eq!(request_id.as_deref(), Some("smoke-1"));
    assert_provider_catalogue(&providers);

    worker.send(&SttCommand::Stop).expect("sends stop");
    drop(worker);

    assert!(
        matches!(next_event(&events, READ_TIMEOUT), ReadOutcome::Closed),
        "the worker must close stdout after stdin EOF"
    );
}

#[test]
fn live_worker_recovers_from_a_malformed_line() {
    let _env = ScrubbedEnv::new();
    let Some(script) = smoke_target() else {
        return;
    };

    let mut worker = BunSubprocess::spawn(&script).expect("spawns the Bun worker");
    let events = worker.events();

    assert_eq!(expect_event(&events), SttEvent::Ready);

    // Written untyped on purpose: even malformed JSON must not echo the line,
    // because a truncated request may contain an API key.
    worker
        .send_raw_line(r#"{"type":"capabilities","apiKey":"sk-smoke-secret""#)
        .expect("writes a malformed line");

    match expect_event(&events) {
        SttEvent::Error {
            session_id,
            code,
            message,
        } => {
            assert_eq!(session_id, None, "protocol errors are session-agnostic");
            assert_eq!(code, "invalid_protocol");
            assert!(
                !message.contains("sk-smoke-secret"),
                "diagnostics must not echo the raw line: {message}"
            );
        }
        other => panic!("expected an invalid_protocol error, got {other:?}"),
    }

    // The loop must stay alive for the next request.
    worker
        .send(&SttCommand::Capabilities {
            request_id: Some("smoke-2".to_string()),
            api_key: None,
        })
        .expect("sends a capabilities request after the bad line");

    let SttEvent::Capabilities {
        request_id,
        providers,
        ..
    } = expect_event(&events)
    else {
        panic!("expected a capabilities event after recovering");
    };
    assert_eq!(request_id.as_deref(), Some("smoke-2"));
    assert_provider_catalogue(&providers);

    drop(worker);
    assert!(
        matches!(next_event(&events, READ_TIMEOUT), ReadOutcome::Closed),
        "the worker must close stdout after stdin EOF"
    );
}
