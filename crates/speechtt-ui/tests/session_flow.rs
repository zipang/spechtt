//! Session setup and transport flow — pure view-model tests.
//!
//! These tests drive [`SessionController`] directly. They never open a window,
//! click elements or assert what the UI renders, so the widget screens remain
//! free to be redesigned (elements moved, merged, replaced) without breaking
//! the workflow contract. Rendering is covered only by the smoke test in
//! `tests/widget.rs`.

use speechtt_core::session::{OutputTarget, SessionField, SessionState};
use speechtt_ui::controller::{SessionController, ViewState};

use std::sync::{Arc, Mutex};

use speechtt_core::backend::{BackendError, SttSessionSink};
use speechtt_core::protocol::{
    ModelCapabilityInfo, ProviderCapability, SttCommand, SttEvent, TranscriptionMode,
};
use speechtt_ui::controller::SAMPLE_RATE;

/// Records every command the controller emits, shared with the test.
#[derive(Clone, Default)]
struct RecordingSink {
    commands: Arc<Mutex<Vec<SttCommand>>>,
}

impl RecordingSink {
    fn take(&self) -> Vec<SttCommand> {
        std::mem::take(&mut *self.commands.lock().expect("sink lock"))
    }
}

impl SttSessionSink for RecordingSink {
    fn send(&mut self, cmd: SttCommand) -> Result<(), BackendError> {
        self.commands.lock().expect("sink lock").push(cmd);
        Ok(())
    }
}

/// A ready-to-record controller wired to a recording sink.
fn duplex_controller() -> (SessionController, RecordingSink) {
    let mut controller = complete_draft(OutputTarget::Memory);
    assert!(controller.create_session());
    let sink = RecordingSink::default();
    controller.set_sink(Box::new(sink.clone()));
    (controller, sink)
}

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

/// A controller whose draft is complete except for `create_session`.
fn complete_draft(output: OutputTarget) -> SessionController {
    let mut controller = SessionController::new();
    install_catalogue(&mut controller);
    controller.open_form();
    controller.cycle_provider();
    controller.cycle_model();
    controller.set_draft_output(output);
    controller.set_draft_name("meeting notes");
    controller
}

#[test]
fn blank_state_shows_the_new_session_prompt() {
    let controller = SessionController::new();

    assert_eq!(controller.view_state(), ViewState::Prompt);
    assert_eq!(controller.session_state(), SessionState::Blank);
    assert_eq!(
        controller.session_missing(),
        vec![
            SessionField::Provider,
            SessionField::Model,
            SessionField::SessionName,
            SessionField::Output,
        ]
    );
}

#[test]
fn opening_the_form_switches_the_view_and_starts_an_empty_draft() {
    let mut controller = SessionController::new();
    install_catalogue(&mut controller);
    controller.open_form();

    assert_eq!(controller.view_state(), ViewState::Form);
    assert!(!controller.is_draft_complete());
    assert_eq!(controller.session_state(), SessionState::Blank);
}

#[test]
fn cancelling_the_form_returns_to_the_prompt_and_discards_the_draft() {
    let mut controller = SessionController::new();
    install_catalogue(&mut controller);
    controller.open_form();
    controller.cycle_provider();
    controller.set_draft_name("discarded");
    assert!(controller.has_provider());

    controller.cancel_form();

    assert_eq!(controller.view_state(), ViewState::Prompt);
    assert!(!controller.has_provider());
    assert_eq!(controller.session_state(), SessionState::Blank);
}

#[test]
fn create_is_inert_until_every_required_field_is_set() {
    let mut controller = SessionController::new();
    install_catalogue(&mut controller);
    controller.open_form();

    // Nothing chosen yet.
    assert!(!controller.create_session());
    assert_eq!(controller.view_state(), ViewState::Form);

    // Partial setup (provider only) is still not enough.
    controller.cycle_provider();
    assert!(!controller.create_session());
    assert_eq!(controller.view_state(), ViewState::Form);

    // Everything but a name.
    controller.cycle_model();
    controller.set_draft_output(OutputTarget::Memory);
    assert!(!controller.create_session());
    assert_eq!(controller.view_state(), ViewState::Form);

    // Whitespace is not a name.
    controller.set_draft_name("   ");
    assert!(!controller.is_draft_complete());
    assert!(!controller.create_session());
    assert_eq!(controller.view_state(), ViewState::Form);
}

#[test]
fn completing_the_form_moves_the_machine_to_ready() {
    let mut controller = complete_draft(OutputTarget::Memory);

    assert!(controller.is_draft_complete());
    assert!(controller.create_session());

    assert_eq!(controller.view_state(), ViewState::Ready);
    assert_eq!(controller.session_state(), SessionState::Ready);
    assert!(controller.session_missing().is_empty());
}

#[test]
fn changing_the_provider_resets_the_dependent_model_choice() {
    let mut controller = SessionController::new();
    install_catalogue(&mut controller);
    controller.open_form();
    controller.cycle_provider();
    controller.cycle_model();
    assert!(controller.has_model());

    controller.cycle_provider();

    assert!(!controller.has_model());
    assert!(!controller.is_draft_complete());
}

#[test]
fn recording_pauses_resumes_and_finishes_a_memory_take() {
    let mut controller = complete_draft(OutputTarget::Memory);
    assert!(controller.create_session());

    assert!(controller.start_recording());
    assert_eq!(controller.view_state(), ViewState::Recording);

    assert!(controller.pause_recording());
    assert_eq!(controller.view_state(), ViewState::Paused);

    assert!(controller.resume_recording());
    assert_eq!(controller.view_state(), ViewState::Recording);

    assert!(controller.pause_recording());
    assert!(controller.finish_take());

    // The machine is Blank again, but the view still shows the finished take.
    assert_eq!(controller.session_state(), SessionState::Blank);
    assert_eq!(controller.view_state(), ViewState::Stopped);

    let take = controller
        .finished_take()
        .expect("a finished take is shown");
    assert_eq!(take.session().session_name, "meeting notes");
    assert_eq!(take.session().output, OutputTarget::Memory);
    assert!(!take.committed().is_empty());
    assert!(!take.live().is_empty());
}

#[test]
fn transport_actions_are_refused_in_the_wrong_view_state() {
    let mut controller = SessionController::new();
    assert!(!controller.start_recording());
    assert!(!controller.pause_recording());
    assert!(!controller.resume_recording());
    assert!(!controller.finish_take());

    let mut ready = complete_draft(OutputTarget::Memory);
    ready.create_session();
    assert!(!ready.pause_recording());
    assert!(!ready.resume_recording());
    assert!(!ready.finish_take());
}

#[test]
fn a_file_take_records_the_default_path_for_its_name() {
    let mut controller = complete_draft(OutputTarget::File(String::new()));
    assert!(controller.create_session());

    controller.start_recording();
    controller.pause_recording();
    assert!(controller.finish_take());

    let take = controller
        .finished_take()
        .expect("a finished take is shown");
    assert_eq!(
        take.session().output,
        OutputTarget::File("~/Documents/meeting notes.md".into())
    );
}

#[test]
fn completeness_can_be_checked_against_the_live_input_name() {
    let mut controller = SessionController::new();
    install_catalogue(&mut controller);
    controller.open_form();
    controller.cycle_provider();
    controller.cycle_model();
    controller.set_draft_output(OutputTarget::Memory);

    // The view reads the editable input directly, so the controller must be
    // able to judge a name it has not received a change event for yet.
    assert!(!controller.is_draft_complete_with_name("   "));
    assert!(controller.is_draft_complete_with_name("standup"));
    assert!(!controller.is_draft_complete());
}

#[test]
fn a_new_take_returns_to_the_prompt_and_requires_a_name_and_output() {
    let mut controller = complete_draft(OutputTarget::Memory);
    controller.create_session();
    controller.start_recording();
    controller.pause_recording();
    controller.finish_take();

    controller.new_take();

    assert_eq!(controller.view_state(), ViewState::Prompt);
    assert!(controller.finished_take().is_none());
    assert_eq!(controller.session_state(), SessionState::Blank);
    // Source, provider and model persist; the next take must be named.
    assert_eq!(
        controller.session_missing(),
        vec![SessionField::SessionName, SessionField::Output]
    );
}

#[test]
fn setup_changes_are_frozen_while_a_take_is_active() {
    let mut controller = complete_draft(OutputTarget::Memory);
    controller.create_session();
    controller.start_recording();

    // The form is unreachable from the transport screens; the controller's
    // draft is independent of the frozen machine configuration.
    controller.cycle_provider();
    controller.set_draft_name("renamed");
    assert_eq!(controller.view_state(), ViewState::Recording);
    assert_eq!(controller.session_missing(), Vec::<SessionField>::new());
}

#[test]
fn transport_commands_are_emitted_in_order_and_paused_audio_is_dropped() {
    let (mut controller, sink) = duplex_controller();

    // Nothing is emitted before a take starts.
    controller.push_audio_chunk(&[9, 9], Arc::<[f32]>::from([0.9]));
    assert!(sink.take().is_empty());

    assert!(controller.start_recording());
    assert_eq!(controller.session_label(), "take-1");

    let active_bars = Arc::<[f32]>::from([0.1, 0.2]);
    controller.push_audio_chunk(&[0, 1], Arc::clone(&active_bars));
    assert_eq!(&*controller.waveform_bars(), &[0.1, 0.2]);

    assert!(controller.pause_recording());
    // Audio arriving while paused must not reach the backend nor move the bars.
    controller.push_audio_chunk(&[2, 3], Arc::<[f32]>::from([0.9, 0.9]));
    assert_eq!(&*controller.waveform_bars(), &[0.1, 0.2]);

    assert!(controller.resume_recording());
    controller.push_audio_chunk(&[-1, 0], Arc::<[f32]>::from([0.3]));

    assert!(controller.finish_take());

    let commands = sink.take();
    assert_eq!(commands.len(), 6, "{commands:?}");

    match &commands[0] {
        SttCommand::Start {
            session_id,
            provider,
            model,
            sample_rate,
            api_key,
        } => {
            assert_eq!(session_id, "take-1");
            assert_eq!(provider, "vercel-ai-gateway");
            assert_eq!(model, "openai/gpt-4o-transcribe");
            assert_eq!(*sample_rate, SAMPLE_RATE);
            assert!(api_key.is_none());
        }
        other => panic!("expected a start command, got {other:?}"),
    }
    assert_eq!(commands[1], SttCommand::audio_from_pcm16(&[0, 1]));
    assert_eq!(commands[2], SttCommand::Pause);
    assert_eq!(commands[3], SttCommand::Resume);
    assert_eq!(commands[4], SttCommand::audio_from_pcm16(&[-1, 0]));
    assert_eq!(commands[5], SttCommand::Stop);
}

#[test]
fn streamed_events_update_the_caption_well_in_real_time() {
    let (mut controller, _sink) = duplex_controller();
    assert!(controller.start_recording());

    controller.handle_stt_event(SttEvent::Partial {
        session_id: "take-1".into(),
        text: "audio".into(),
    });
    assert_eq!(controller.transcript_committed(), "");
    assert_eq!(controller.transcript_live(), "audio");

    controller.handle_stt_event(SttEvent::Delta {
        session_id: "take-1".into(),
        delta: " capture".into(),
    });
    assert_eq!(controller.transcript_live(), "audio capture");

    controller.handle_stt_event(SttEvent::Committed {
        session_id: "take-1".into(),
        text: "audio capture layer.".into(),
    });
    assert_eq!(controller.transcript_committed(), "audio capture layer.");
    assert_eq!(controller.transcript_live(), "");

    controller.handle_stt_event(SttEvent::Partial {
        session_id: "take-1".into(),
        text: "next words".into(),
    });
    controller.handle_stt_event(SttEvent::Delta {
        session_id: "take-1".into(),
        delta: " now".into(),
    });
    assert_eq!(controller.transcript_live(), "next words now");

    assert!(controller.pause_recording());
    assert!(controller.finish_take());

    let take = controller.finished_take().expect("shows the finished take");
    assert_eq!(take.committed().to_string(), "audio capture layer.");
    assert_eq!(take.live().to_string(), "next words now");

    // The final `stopped` flush replaces what the stopped screen showed.
    controller.handle_stt_event(SttEvent::Stopped {
        session_id: "take-1".into(),
        full_text: "audio capture layer. next words now.".into(),
    });

    let take = controller.finished_take().expect("still shows the take");
    assert_eq!(
        take.committed().to_string(),
        "audio capture layer. next words now."
    );
    assert_eq!(take.live().to_string(), "");
}

#[test]
fn backend_errors_are_surfaced_and_cleared_by_the_next_take() {
    let (mut controller, _sink) = duplex_controller();
    assert!(controller.start_recording());

    controller.handle_stt_event(SttEvent::Error {
        session_id: Some("take-1".into()),
        code: "missing_api_key".into(),
        message: "set MISTRAL_API_KEY".into(),
    });
    assert_eq!(
        controller.last_error().map(ToString::to_string).as_deref(),
        Some("missing_api_key: set MISTRAL_API_KEY")
    );

    // A stale error cannot overwrite the current take's status.
    controller.handle_stt_event(SttEvent::Error {
        session_id: Some("take-0".into()),
        code: "stream_failed".into(),
        message: "old take".into(),
    });
    assert_eq!(
        controller.last_error().map(ToString::to_string).as_deref(),
        Some("missing_api_key: set MISTRAL_API_KEY")
    );

    assert!(controller.pause_recording());
    assert!(controller.finish_take());
    controller.new_take();
    controller.open_form();
    controller.cycle_provider();
    controller.cycle_model();
    controller.set_draft_output(OutputTarget::Memory);
    controller.set_draft_name("second take");
    assert!(controller.create_session());
    assert!(controller.start_recording());

    assert!(controller.last_error().is_none());
}

#[test]
fn stale_events_from_a_finished_take_are_ignored() {
    let (mut controller, _sink) = duplex_controller();
    assert!(controller.start_recording());
    controller.handle_stt_event(SttEvent::Partial {
        session_id: "take-1".into(),
        text: "first take".into(),
    });
    assert_eq!(controller.transcript_live(), "first take");
    assert!(controller.finish_take());

    controller.new_take();
    controller.open_form();
    controller.cycle_provider();
    controller.cycle_model();
    controller.set_draft_output(OutputTarget::Memory);
    controller.set_draft_name("second take");
    assert!(controller.create_session());
    assert!(controller.start_recording());
    assert_eq!(controller.session_label(), "take-2");

    controller.handle_stt_event(SttEvent::Partial {
        session_id: "take-1".into(),
        text: "stale words".into(),
    });
    controller.handle_stt_event(SttEvent::Stopped {
        session_id: "take-1".into(),
        full_text: "stale flush".into(),
    });
    assert_eq!(controller.transcript_live(), "");
    assert_eq!(controller.transcript_committed(), "");

    controller.handle_stt_event(SttEvent::Partial {
        session_id: "take-2".into(),
        text: "fresh words".into(),
    });
    assert_eq!(controller.transcript_live(), "fresh words");
}

#[test]
fn the_elapsed_clock_resets_when_a_new_take_starts() {
    let (mut controller, _sink) = duplex_controller();
    assert!(controller.start_recording());
    controller.set_elapsed_seconds(42);
    assert_eq!(controller.elapsed_seconds(), 42);
    assert!(controller.pause_recording());
    assert!(controller.finish_take());

    controller.new_take();
    controller.open_form();
    controller.cycle_provider();
    controller.cycle_model();
    controller.set_draft_output(OutputTarget::Memory);
    controller.set_draft_name("next take");
    assert!(controller.create_session());
    assert!(controller.start_recording());

    assert_eq!(controller.elapsed_seconds(), 0);
}

// ----- File transcription (T0015) ------------------------------------

use speechtt_ui::controller::AppError;

/// A real file that is removed when the test ends.
struct TempFile {
    path: std::path::PathBuf,
}

impl TempFile {
    fn new(name: &str) -> Self {
        let mut path = std::env::temp_dir();
        path.push(format!("speechtt-test-{}-{name}", std::process::id()));
        std::fs::write(&path, b"RIFF0000WAVE").expect("write temp file");
        Self { path }
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn file_controller() -> (SessionController, RecordingSink) {
    duplex_controller()
}

#[test]
fn file_session_rejects_a_missing_path() {
    let (mut controller, sink) = file_controller();
    let missing = std::env::temp_dir().join("speechtt-definitely-missing-42.mp4");

    let result = controller.start_file_session(&missing);

    assert_eq!(result, Err(AppError::FileNotFound));
    assert_eq!(controller.session_state(), SessionState::Ready);
    assert!(sink.take().is_empty());
}

#[test]
fn file_session_rejects_a_directory() {
    let (mut controller, sink) = file_controller();
    let directory = std::env::temp_dir();

    let result = controller.start_file_session(&directory);

    assert_eq!(result, Err(AppError::FileUnreadable));
    assert_eq!(controller.session_state(), SessionState::Ready);
    assert!(sink.take().is_empty());
}

#[test]
fn file_session_rejects_an_empty_or_nul_path() {
    let (mut controller, _sink) = file_controller();

    assert_eq!(
        controller.start_file_session(std::path::Path::new("")),
        Err(AppError::FileUnreadable)
    );
    assert_eq!(
        controller.start_file_session(std::path::Path::new("/tmp/a\u{0}b.wav")),
        Err(AppError::FileUnreadable)
    );
}

#[test]
fn file_session_sends_a_transcribe_file_command_and_enters_recording() {
    let file = TempFile::new("meeting-2026-10-29.mp4");
    let (mut controller, sink) = file_controller();

    controller.start_file_session(&file.path).expect("starts");

    assert_eq!(controller.view_state(), ViewState::Recording);
    assert_eq!(controller.session_state(), SessionState::Recording);
    assert_eq!(controller.session_label(), "take-1");
    assert!(controller.is_file_take());

    assert_eq!(
        sink.take(),
        vec![SttCommand::TranscribeFile {
            session_id: "take-1".to_string(),
            provider: "vercel-ai-gateway".to_string(),
            model: "openai/gpt-4o-transcribe".to_string(),
            path: file.path.to_string_lossy().into_owned(),
            api_key: None,
        }]
    );
}

#[test]
fn file_session_adopts_the_file_name_as_its_audio_source() {
    let file = TempFile::new("standup-notes.m4a");
    let (mut controller, _sink) = file_controller();

    controller.start_file_session(&file.path).expect("starts");

    let name = file
        .path
        .file_name()
        .expect("file name")
        .to_string_lossy()
        .into_owned();
    assert_eq!(controller.audio_source(), Some(name.as_str()));
}

#[test]
fn file_session_keeps_an_existing_session_name() {
    let file = TempFile::new("keeps-name.mkv");
    let (mut controller, _sink) = file_controller();

    controller.start_file_session(&file.path).expect("starts");

    assert_eq!(controller.session_name(), Some("meeting notes"));
}

#[test]
fn file_session_never_sends_audio_even_if_the_capture_pushes() {
    let file = TempFile::new("no-audio.wav");
    let (mut controller, sink) = file_controller();
    controller.start_file_session(&file.path).expect("starts");
    sink.take();

    controller.push_audio_chunk(&[1, 2, 3, 4], Arc::from(vec![0.1_f32; 8]));

    assert!(sink.take().is_empty());
}

#[test]
fn file_session_reports_progress_as_the_decoded_position() {
    let file = TempFile::new("progress.mp3");
    let (mut controller, sink) = file_controller();
    controller.start_file_session(&file.path).expect("starts");
    sink.take();

    controller.handle_stt_event(SttEvent::Progress {
        session_id: "take-1".into(),
        processed_seconds: 12.5,
        total_seconds: Some(340.25),
    });

    assert_eq!(controller.elapsed_seconds(), 12);
    assert_eq!(controller.progress(), Some((12.5, Some(340.25))));
    // The decoded position never drives a live audio command.
    assert!(sink.take().is_empty());
}

#[test]
fn file_progress_ignores_a_stale_take() {
    let file = TempFile::new("stale.wav");
    let (mut controller, _sink) = file_controller();
    controller.start_file_session(&file.path).expect("starts");

    controller.handle_stt_event(SttEvent::Progress {
        session_id: "take-99".into(),
        processed_seconds: 99.0,
        total_seconds: None,
    });

    assert_eq!(controller.progress(), None);
}

#[test]
fn file_take_finishes_through_the_normal_stop_flow() {
    let file = TempFile::new("finish.wav");
    let (mut controller, sink) = file_controller();
    controller.start_file_session(&file.path).expect("starts");
    controller.handle_stt_event(SttEvent::Committed {
        session_id: "take-1".into(),
        text: "hello file".into(),
    });
    sink.take();

    assert!(controller.finish_take());

    assert_eq!(sink.take(), vec![SttCommand::Stop]);
    assert_eq!(controller.view_state(), ViewState::Stopped);
    assert!(!controller.is_file_take());
    assert_eq!(controller.progress(), None);
    assert_eq!(
        controller
            .finished_take()
            .expect("take")
            .committed()
            .to_string(),
        "hello file"
    );
}

#[test]
fn file_errors_map_to_typed_app_errors() {
    assert_eq!(
        AppError::from_stt_error("file_not_found", "gone", "p"),
        AppError::FileNotFound
    );
    assert_eq!(
        AppError::from_stt_error("unsupported_file", "no audio", "p"),
        AppError::FileUnreadable
    );
    assert_eq!(
        AppError::from_stt_error("decode_failed", "boom", "p"),
        AppError::FileUnreadable
    );
    assert_eq!(
        AppError::from_stt_error("ffmpeg_missing", "install ffmpeg", "p"),
        AppError::FfmpegMissing
    );
}

#[test]
fn file_errors_offer_a_dismiss_action_and_never_leak_details() {
    for error in [
        AppError::FileNotFound,
        AppError::FileUnreadable,
        AppError::FfmpegMissing,
        AppError::FileDialogFailed,
    ] {
        assert_eq!(
            error.action(),
            speechtt_ui::controller::ErrorAction::Dismiss
        );
        assert!(!error.message().is_empty());
        assert!(error.detail().is_none());
    }
}
