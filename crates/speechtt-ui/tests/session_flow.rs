//! Session setup and transport flow — pure view-model tests.
//!
//! These tests drive [`SessionController`] directly. They never open a window,
//! click elements or assert what the UI renders, so the widget screens remain
//! free to be redesigned (elements moved, merged, replaced) without breaking
//! the workflow contract. Rendering is covered only by the smoke test in
//! `tests/widget.rs`.
//!
//! The session contract is nameless: a take is a mode (live or file), a
//! provider and a model. The file the user saves names it.

use speechtt_core::session::{SessionField, SessionState};
use speechtt_core::settings::Settings;
use speechtt_ui::controller::{SessionController, SessionMode, ViewState};

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

/// A controller with the worker's catalogue installed.
fn catalogued() -> SessionController {
    let mut controller = SessionController::new();
    controller.set_capabilities(catalogue());
    controller
}

/// A ready-to-record controller wired to a recording sink.
fn duplex_controller() -> (SessionController, RecordingSink) {
    let mut controller = ready_live();
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

/// Settings that resolve against [`catalogue`].
fn configured_settings(model: &str) -> Settings {
    Settings {
        provider: "vercel-ai-gateway".to_string(),
        model: model.to_string(),
        ..Settings::default()
    }
}

/// A controller in Live mode, past the form and `Ready`.
fn ready_live() -> SessionController {
    let mut controller = catalogued();
    assert!(
        !controller.choose_mode(SessionMode::Live),
        "no settings yet: the form is required"
    );
    controller.cycle_provider();
    controller.cycle_model();
    assert!(controller.create_session());
    controller
}

/// A controller in File mode, past the form and `Ready`.
fn ready_file() -> SessionController {
    let mut controller = catalogued();
    assert!(!controller.choose_mode(SessionMode::File));
    controller.cycle_provider();
    controller.cycle_model();
    assert!(controller.create_session());
    controller
}

// ----- Mode chooser ---------------------------------------------------

#[test]
fn blank_state_shows_the_mode_chooser() {
    let controller = SessionController::new();

    assert_eq!(controller.view_state(), ViewState::Mode);
    assert_eq!(controller.session_state(), SessionState::Blank);
    assert_eq!(controller.mode(), SessionMode::Live);
    // The default microphone is assumed; only the service and model are left.
    assert_eq!(
        controller.session_missing(),
        vec![SessionField::Provider, SessionField::Model]
    );
}

#[test]
fn choosing_a_mode_without_settings_opens_the_two_field_form() {
    let mut controller = catalogued();

    assert!(!controller.choose_mode(SessionMode::Live));
    assert_eq!(controller.view_state(), ViewState::Form);
    assert_eq!(controller.mode(), SessionMode::Live);
    assert!(!controller.is_draft_complete());

    assert!(!controller.choose_mode(SessionMode::File));
    assert_eq!(controller.view_state(), ViewState::Form);
    assert_eq!(controller.mode(), SessionMode::File);
}

#[test]
fn choosing_live_with_configured_settings_goes_straight_to_ready() {
    let mut controller = catalogued();
    controller.apply_settings(configured_settings("openai/gpt-4o-transcribe"));

    assert!(controller.choose_mode(SessionMode::Live));

    assert_eq!(controller.view_state(), ViewState::Ready);
    assert_eq!(controller.mode(), SessionMode::Live);
    assert_eq!(controller.session_state(), SessionState::Ready);
    assert!(controller.session_missing().is_empty());
}

#[test]
fn choosing_file_with_configured_settings_goes_straight_to_ready() {
    let mut controller = catalogued();
    controller.apply_settings(configured_settings("openai/gpt-4o-transcribe"));

    assert!(controller.choose_mode(SessionMode::File));

    assert_eq!(controller.view_state(), ViewState::Ready);
    assert_eq!(controller.mode(), SessionMode::File);
}

#[test]
fn a_stale_configured_model_falls_back_to_the_form() {
    let mut controller = catalogued();
    controller.apply_settings(configured_settings("retired/model"));

    assert!(!controller.choose_mode(SessionMode::Live));

    assert_eq!(controller.view_state(), ViewState::Form);
    assert!(!controller.is_draft_complete());
}

#[test]
fn create_session_commits_provider_and_model_and_keeps_the_mode() {
    let controller = ready_file();

    assert!(!controller.is_draft_complete(), "the draft is consumed");
    assert_eq!(controller.view_state(), ViewState::Ready);
    assert_eq!(controller.mode(), SessionMode::File);
    assert_eq!(controller.session_state(), SessionState::Ready);
    assert!(controller.session_missing().is_empty());
}

#[test]
fn create_is_inert_until_the_two_fields_are_set() {
    let mut controller = catalogued();
    controller.choose_mode(SessionMode::Live);

    // Nothing chosen yet.
    assert!(!controller.create_session());
    assert_eq!(controller.view_state(), ViewState::Form);

    // Provider only is not enough.
    controller.cycle_provider();
    assert!(!controller.create_session());
    assert_eq!(controller.view_state(), ViewState::Form);

    // Model completes the form.
    controller.cycle_model();
    assert!(controller.create_session());
    assert_eq!(controller.view_state(), ViewState::Ready);
}

#[test]
fn changing_the_provider_resets_the_dependent_model_choice() {
    let mut controller = catalogued();
    controller.choose_mode(SessionMode::Live);
    controller.cycle_provider();
    controller.cycle_model();
    assert!(controller.has_model());

    controller.cycle_provider();

    assert!(!controller.has_model());
    assert!(!controller.is_draft_complete());
}

#[test]
fn cancelling_the_form_returns_to_the_mode_chooser_and_discards_the_draft() {
    let mut controller = catalogued();
    controller.choose_mode(SessionMode::File);
    controller.cycle_provider();
    assert!(controller.has_provider());

    controller.cancel_form();

    assert_eq!(controller.view_state(), ViewState::Mode);
    assert!(!controller.has_provider());
    assert_eq!(controller.session_state(), SessionState::Blank);
}

#[test]
fn change_mode_clears_the_machine_and_returns_to_the_chooser() {
    let mut controller = ready_live();
    assert_eq!(controller.session_state(), SessionState::Ready);

    assert!(controller.change_mode());

    assert_eq!(controller.view_state(), ViewState::Mode);
    assert_eq!(controller.session_state(), SessionState::Blank);
    assert_eq!(
        controller.session_missing(),
        vec![
            SessionField::AudioSource,
            SessionField::Provider,
            SessionField::Model
        ]
    );
}

#[test]
fn change_mode_is_inert_while_a_take_is_active() {
    let mut controller = ready_live();
    assert!(controller.start_recording());

    assert!(!controller.change_mode());

    assert_eq!(controller.view_state(), ViewState::Recording);
    assert_eq!(controller.session_state(), SessionState::Recording);
}

// ----- Transport ------------------------------------------------------

#[test]
fn start_recording_is_refused_in_file_mode() {
    let mut controller = ready_file();

    assert!(!controller.start_recording());
    assert_eq!(controller.view_state(), ViewState::Ready);
    assert_eq!(controller.session_state(), SessionState::Ready);
}

#[test]
fn recording_pauses_resumes_and_finishes_a_take() {
    let mut controller = ready_live();

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
    assert_eq!(take.session().provider, "vercel-ai-gateway");
    assert_eq!(take.session().model, "openai/gpt-4o-transcribe");
    assert!(!take.session().audio_source.is_empty());
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

    let mut ready = ready_live();
    assert!(!ready.pause_recording());
    assert!(!ready.resume_recording());
    assert!(!ready.finish_take());
}

#[test]
fn a_new_take_returns_to_the_mode_chooser() {
    let mut controller = ready_live();
    controller.start_recording();
    controller.pause_recording();
    controller.finish_take();

    controller.new_take();

    assert_eq!(controller.view_state(), ViewState::Mode);
    assert!(controller.finished_take().is_none());
    assert_eq!(controller.session_state(), SessionState::Blank);
    assert_eq!(
        controller.session_missing(),
        vec![
            SessionField::AudioSource,
            SessionField::Provider,
            SessionField::Model
        ]
    );
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
    controller.choose_mode(SessionMode::Live);
    controller.cycle_provider();
    controller.cycle_model();
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
    controller.choose_mode(SessionMode::Live);
    controller.cycle_provider();
    controller.cycle_model();
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
    controller.choose_mode(SessionMode::Live);
    controller.cycle_provider();
    controller.cycle_model();
    assert!(controller.create_session());
    assert!(controller.start_recording());

    assert_eq!(controller.elapsed_seconds(), 0);
}

// ----- File transcription --------------------------------------------

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

#[test]
fn file_session_rejects_a_missing_path() {
    let mut controller = ready_file();
    let sink = RecordingSink::default();
    controller.set_sink(Box::new(sink.clone()));
    let missing = std::env::temp_dir().join("speechtt-definitely-missing-42.mp4");

    let result = controller.start_file_session(&missing);

    assert_eq!(result, Err(AppError::FileNotFound));
    assert_eq!(controller.session_state(), SessionState::Ready);
    assert!(sink.take().is_empty());
}

#[test]
fn file_session_rejects_a_directory() {
    let mut controller = ready_file();
    let sink = RecordingSink::default();
    controller.set_sink(Box::new(sink.clone()));
    let directory = std::env::temp_dir();

    let result = controller.start_file_session(&directory);

    assert_eq!(result, Err(AppError::FileUnreadable));
    assert_eq!(controller.session_state(), SessionState::Ready);
    assert!(sink.take().is_empty());
}

#[test]
fn file_session_rejects_an_empty_or_nul_path() {
    let mut controller = ready_file();

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
    let mut controller = ready_file();
    let sink = RecordingSink::default();
    controller.set_sink(Box::new(sink.clone()));

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
    let mut controller = ready_file();

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
fn file_session_never_sends_audio_even_if_the_capture_pushes() {
    let file = TempFile::new("no-audio.wav");
    let mut controller = ready_file();
    let sink = RecordingSink::default();
    controller.set_sink(Box::new(sink.clone()));
    controller.start_file_session(&file.path).expect("starts");
    sink.take();

    controller.push_audio_chunk(&[1, 2, 3, 4], Arc::from(vec![0.1_f32; 8]));

    assert!(sink.take().is_empty());
}

#[test]
fn file_session_reports_progress_as_the_decoded_position() {
    let file = TempFile::new("progress.mp3");
    let mut controller = ready_file();
    let sink = RecordingSink::default();
    controller.set_sink(Box::new(sink.clone()));
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
    let mut controller = ready_file();
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
    let mut controller = ready_file();
    let sink = RecordingSink::default();
    controller.set_sink(Box::new(sink.clone()));
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

// ----- Nameless saves -------------------------------------------------

/// A unique directory under the system temp dir, removed on drop.
struct TempDir(std::path::PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let mut path = std::env::temp_dir();
        path.push(format!("speechtt-flow-{}-{label}", std::process::id()));
        std::fs::create_dir_all(&path).expect("temp dir");
        Self(path)
    }

    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn finished_live_with(text: &str) -> SessionController {
    let (mut controller, _sink) = duplex_controller();
    assert!(controller.start_recording());
    controller.handle_stt_event(SttEvent::Committed {
        session_id: "take-1".into(),
        text: text.into(),
    });
    assert!(controller.pause_recording());
    assert!(controller.finish_take());
    controller
}

fn finished_file_with(stem: &str, text: &str) -> (SessionController, TempFile) {
    let file = TempFile::new(&format!("{stem}.mp4"));
    let mut controller = ready_file();
    controller.start_file_session(&file.path).expect("starts");
    controller.handle_stt_event(SttEvent::Committed {
        session_id: "take-1".into(),
        text: text.into(),
    });
    assert!(controller.finish_take());
    (controller, file)
}

#[test]
fn suggested_save_name_is_none_for_a_live_take() {
    let controller = finished_live_with("hello there");

    assert_eq!(controller.suggested_save_name(), None);
}

#[test]
fn suggested_save_name_uses_the_source_file_stem() {
    let (controller, file) = finished_file_with("weekly-sync", "hello");

    let stem = file
        .path
        .file_stem()
        .expect("file stem")
        .to_string_lossy()
        .into_owned();
    assert_eq!(controller.suggested_save_name(), Some(stem));
}

#[test]
fn saving_writes_the_chosen_path_and_titles_it_with_its_stem() {
    let directory = TempDir::new("save");
    let target = directory.path().join("Board Meeting.md");
    let mut controller = finished_live_with("hello\nworld");

    let saved = controller
        .save_finished_transcript_to(&target)
        .expect("save succeeds");

    assert_eq!(saved.path, target);
    assert_eq!(controller.written_path(), Some(target.as_path()));
    assert_eq!(controller.output_status(), Some("Saved"));
    assert_eq!(
        std::fs::read_to_string(&target).expect("saved file"),
        "# Board Meeting\n\nhello\nworld\n"
    );
}

#[test]
fn saving_honours_a_text_extension() {
    let directory = TempDir::new("save-txt");
    let target = directory.path().join("notes.txt");
    let mut controller = finished_live_with("hello");

    controller
        .save_finished_transcript_to(&target)
        .expect("save succeeds");

    assert_eq!(
        std::fs::read_to_string(&target).expect("saved file"),
        "hello\n"
    );
}

#[test]
fn saving_without_a_finished_take_reports_an_error() {
    let directory = TempDir::new("save-none");
    let target = directory.path().join("notes.md");
    let mut controller = SessionController::new();

    let result = controller.save_finished_transcript_to(&target);

    assert!(matches!(result, Err(AppError::SaveFailed(_))));
    assert!(!target.exists());
}

#[test]
fn saving_an_empty_transcript_reports_an_error() {
    let directory = TempDir::new("save-empty");
    let target = directory.path().join("notes.md");
    // A real sink keeps the mock transcript out of the finished take.
    let (mut controller, _sink) = duplex_controller();
    assert!(controller.start_recording());
    controller.handle_stt_event(SttEvent::Committed {
        session_id: "take-1".into(),
        text: "   ".into(),
    });
    assert!(controller.pause_recording());
    assert!(controller.finish_take());

    let result = controller.save_finished_transcript_to(&target);

    assert!(matches!(result, Err(AppError::SaveFailed(_))));
    assert!(!target.exists());
}

#[test]
fn open_saved_file_uses_the_written_path() {
    use speechtt_core::output::OutputError;
    use speechtt_core::output::file::FileOpener;

    #[derive(Default)]
    struct RecordingOpener {
        opened: Vec<std::path::PathBuf>,
    }

    impl FileOpener for RecordingOpener {
        fn open(&mut self, path: &std::path::Path) -> Result<(), OutputError> {
            self.opened.push(path.to_path_buf());
            Ok(())
        }
    }

    let directory = TempDir::new("open");
    let target = directory.path().join("notes.md");
    let mut controller = finished_live_with("hello");
    controller
        .save_finished_transcript_to(&target)
        .expect("save succeeds");

    let mut opener = RecordingOpener::default();
    controller.open_saved_file(&mut opener).expect("opens");

    assert_eq!(opener.opened, vec![target]);
    assert_eq!(controller.output_status(), Some("Opened"));
}

#[test]
fn opening_without_a_saved_file_reports_an_error() {
    use speechtt_core::output::OutputError;
    use speechtt_core::output::file::{FileOpener, SystemOpener};

    struct NoOpOpener;

    impl FileOpener for NoOpOpener {
        fn open(&mut self, _path: &std::path::Path) -> Result<(), OutputError> {
            Ok(())
        }
    }

    let mut controller = finished_live_with("hello");
    let mut opener = NoOpOpener;
    assert!(matches!(
        controller.open_saved_file(&mut opener),
        Err(AppError::SaveFailed(_))
    ));
    // SystemOpener is only referenced to keep the production type in scope.
    let _ = std::mem::size_of::<SystemOpener>();
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
