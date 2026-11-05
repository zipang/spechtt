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
use speechtt_ui::controller::{
    AppError, CleanState, CleanStatus, CleanToggle, SessionController, SessionMode, ViewState,
};

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use speechtt_core::backend::{BackendError, SttSessionSink};
use speechtt_core::protocol::{
    CapabilitySource, LanguageModelInfo, LanguageOption, ModelCapabilityInfo, ProviderCapability,
    SecretString, SttCommand, SttEvent, TranscriptionMode,
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

/// The same provider, but every model is streaming — what a catalogue refresh
/// can do underneath an already-committed file draft.
fn reclassified_catalogue() -> Vec<ProviderCapability> {
    vec![ProviderCapability {
        id: "vercel-ai-gateway".to_string(),
        label: "Vercel AI Gateway".to_string(),
        available: true,
        key_env_vars: vec!["AI_GATEWAY_API_KEY".to_string()],
        models: vec![
            ModelCapabilityInfo {
                id: "openai/gpt-4o-transcribe".to_string(),
                label: "GPT-4o Transcribe".to_string(),
                mode: TranscriptionMode::Streaming,
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

/// Settings that resolve against [`catalogue`]: one default per take mode.
fn configured_settings(streaming: &str, batch: &str) -> Settings {
    Settings {
        provider: "vercel-ai-gateway".to_string(),
        model_streaming: streaming.to_string(),
        model_batch: batch.to_string(),
        ..Settings::default()
    }
}

/// A pair of known defaults, streaming first.
const DEFAULTS: (&str, &str) = ("openai/gpt-realtime-whisper", "openai/gpt-4o-transcribe");

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
    // The picker comes first in file mode: the form only exists once a
    // file has been chosen.
    controller
        .set_pending_file(PathBuf::from("/tmp/speechtt-ready.mp4"))
        .expect("a media path");
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

    // File mode waits for the picker on the welcome screen.
    assert!(!controller.choose_mode(SessionMode::File));
    assert_eq!(controller.view_state(), ViewState::Mode);
    assert_eq!(controller.mode(), SessionMode::File);
}

#[test]
fn choosing_live_with_configured_settings_opens_the_take_screen_directly() {
    let mut controller = catalogued();
    controller.apply_settings(configured_settings(DEFAULTS.0, DEFAULTS.1));
    let sink = RecordingSink::default();
    controller.set_sink(Box::new(sink.clone()));

    assert!(controller.choose_mode(SessionMode::Live));

    // No Ready holding screen: the configuration is complete, so the
    // take screen is what the user sees. Capture is started by its own
    // control, never by the mode choice.
    assert_eq!(controller.view_state(), ViewState::Live);
    assert_eq!(controller.mode(), SessionMode::Live);
    assert_eq!(controller.session_state(), SessionState::Ready);
    assert!(controller.session_missing().is_empty());
    assert!(sink.take().is_empty(), "no command may start the take");
}

#[test]
fn choosing_file_waits_for_the_picker_on_the_welcome_screen() {
    let mut controller = catalogued();
    controller.apply_settings(configured_settings(DEFAULTS.0, DEFAULTS.1));

    assert!(!controller.choose_mode(SessionMode::File));

    // The native picker is open: the welcome screen stays up, and cancelling
    // the dialog must be a no-op on the flow.
    assert_eq!(controller.view_state(), ViewState::Mode);
    assert_eq!(controller.mode(), SessionMode::File);
    assert_eq!(controller.pending_file(), None);
    assert!(
        !controller.has_provider(),
        "no draft before a file is picked"
    );
    assert_eq!(controller.session_state(), SessionState::Blank);

    // Cancelling the picker is what the app does: change back to the chooser.
    assert!(controller.change_mode());
    assert_eq!(controller.view_state(), ViewState::Mode);
    assert_eq!(controller.pending_file(), None);
    assert!(!controller.has_provider());
}

#[test]
fn a_pending_file_opens_the_form_and_seeds_a_segmented_model() {
    let mut controller = catalogued();
    controller.apply_settings(configured_settings(DEFAULTS.0, DEFAULTS.1));
    assert!(!controller.choose_mode(SessionMode::File));

    controller
        .set_pending_file(PathBuf::from("/tmp/interview.mp4"))
        .expect("a media path");

    assert_eq!(controller.view_state(), ViewState::Form);
    assert_eq!(
        controller.pending_file(),
        Some(Path::new("/tmp/interview.mp4"))
    );
    assert!(controller.has_provider());
    assert!(
        controller.is_draft_complete(),
        "segmented default is seeded"
    );
    assert_eq!(
        controller.draft_model_mode(),
        Some(TranscriptionMode::Segmented)
    );
}

#[test]
fn a_pending_file_does_not_seed_a_streaming_model() {
    let mut controller = catalogued();
    controller.apply_settings(configured_settings(DEFAULTS.0, ""));
    assert!(!controller.choose_mode(SessionMode::File));

    controller
        .set_pending_file(PathBuf::from("/tmp/interview.mp4"))
        .expect("a media path");

    assert_eq!(controller.view_state(), ViewState::Form);
    assert!(controller.has_provider(), "the provider is still seeded");
    assert!(!controller.has_model(), "a streaming model is not offered");
    assert!(!controller.is_draft_complete());
}

#[test]
fn the_pending_file_is_cleared_by_every_way_out() {
    let mut controller = catalogued();
    controller.choose_mode(SessionMode::File);
    controller
        .set_pending_file(PathBuf::from("/tmp/a.mp4"))
        .expect("a media path");
    controller.cancel_form();
    assert_eq!(controller.pending_file(), None);

    controller.choose_mode(SessionMode::File);
    controller
        .set_pending_file(PathBuf::from("/tmp/b.mp4"))
        .expect("a media path");
    controller.change_mode();
    assert_eq!(controller.pending_file(), None);

    // A finished take's "New take" also drops it.
    controller.choose_mode(SessionMode::File);
    controller
        .set_pending_file(PathBuf::from("/tmp/c.mp4"))
        .expect("a media path");
    controller.cycle_provider();
    controller.cycle_model();
    assert!(controller.create_session());
    controller.new_take();
    assert_eq!(controller.pending_file(), None);
}

#[test]
fn the_file_model_list_only_offers_segmented_models() {
    let mut controller = catalogued();
    controller.apply_settings(configured_settings(DEFAULTS.0, DEFAULTS.1));
    controller.choose_mode(SessionMode::File);
    controller
        .set_pending_file(PathBuf::from("/tmp/a.mp4"))
        .expect("a media path");

    assert_eq!(
        controller.selectable_model_ids("vercel-ai-gateway"),
        vec!["openai/gpt-4o-transcribe".to_string()]
    );
    assert!(
        controller
            .selectable_model_ids("unknown/provider")
            .is_empty(),
        "an unknown provider offers nothing"
    );

    // Live mode keeps the whole list.
    let mut live = catalogued();
    live.choose_mode(SessionMode::Live);
    assert_eq!(
        live.selectable_model_ids("vercel-ai-gateway"),
        vec![
            "openai/gpt-4o-transcribe".to_string(),
            "openai/gpt-realtime-whisper".to_string()
        ]
    );
}

#[test]
fn file_mode_never_cycles_onto_a_streaming_model() {
    let mut controller = catalogued();
    controller.apply_settings(configured_settings(DEFAULTS.0, ""));
    controller.choose_mode(SessionMode::File);
    controller
        .set_pending_file(PathBuf::from("/tmp/a.mp4"))
        .expect("a media path");
    controller.cycle_provider();

    // Walk a full cycle: no visited model may be streaming.
    let mut seen = Vec::new();
    for _ in 0..4 {
        controller.cycle_model();
        assert_eq!(
            controller.draft_model_mode(),
            Some(TranscriptionMode::Segmented),
            "cycle landed on a streaming model"
        );
        seen.push(
            controller
                .model_label()
                .expect("a selectable model")
                .to_string(),
        );
    }
    assert!(
        seen.iter().all(|label| label == "GPT-4o Transcribe"),
        "{seen:?}"
    );
}

#[test]
fn a_streaming_model_cannot_complete_a_file_draft() {
    let mut controller = catalogued();
    controller.apply_settings(configured_settings(DEFAULTS.0, DEFAULTS.1));
    controller.choose_mode(SessionMode::File);
    controller
        .set_pending_file(PathBuf::from("/tmp/a.mp4"))
        .expect("a media path");
    assert!(controller.is_draft_complete(), "segmented to begin with");

    // The catalogue reclassifies the model as streaming underneath the draft.
    controller.set_capabilities(reclassified_catalogue());

    assert!(!controller.is_draft_complete());
    assert!(!controller.create_session());
    assert_eq!(controller.view_state(), ViewState::Form);
}

#[test]
fn a_file_take_refuses_a_streaming_model_before_reaching_the_worker() {
    let mut controller = ready_file();
    controller.set_capabilities(reclassified_catalogue());

    let sink = RecordingSink::default();
    controller.set_sink(Box::new(sink.clone()));
    let file = TempFile::new("refused.mp4");

    let result = controller.start_file_session(&file.path);

    assert_eq!(result, Err(AppError::FileModelNotSupported));
    assert!(sink.take().is_empty(), "no command may be sent");
    assert_eq!(controller.session_state(), SessionState::Ready);
}

#[test]
fn a_stale_configured_model_falls_back_to_the_form() {
    let mut controller = catalogued();
    controller.apply_settings(configured_settings("retired/model", "retired/model"));

    assert!(!controller.choose_mode(SessionMode::Live));

    assert_eq!(controller.view_state(), ViewState::Form);
    assert!(!controller.is_draft_complete());
}

#[test]
fn create_session_commits_provider_and_model_and_keeps_the_mode() {
    let controller = ready_file();

    assert!(!controller.is_draft_complete(), "the draft is consumed");
    // File mode has no holding screen: with no take running the setup form is
    // the only thing to show.
    assert_eq!(controller.view_state(), ViewState::Form);
    assert_eq!(controller.mode(), SessionMode::File);
    assert_eq!(controller.session_state(), SessionState::Ready);
    assert!(controller.session_missing().is_empty());
    assert_eq!(
        controller.pending_file(),
        None,
        "Continue consumes the path"
    );
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
    assert_eq!(controller.view_state(), ViewState::Live);
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

    assert!(controller.go_back());

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

    assert_eq!(controller.view_state(), ViewState::Live);
    assert_eq!(controller.session_state(), SessionState::Recording);
}

// ----- Take screen: reset and cancel --------------------------------

#[test]
fn reset_clears_the_take_and_keeps_the_configuration() {
    let (mut controller, sink) = duplex_controller();
    controller.report_error(AppError::PasteFailed);
    assert!(controller.start_recording());
    controller.handle_stt_event(SttEvent::Committed {
        session_id: "take-1".into(),
        text: "a microphone check".into(),
        segment: None,
    });
    let _ = sink.take();

    assert!(controller.reset_take());

    // Still on the take screen, still configured, with nothing left over.
    assert_eq!(controller.view_state(), ViewState::Live);
    assert_eq!(controller.session_state(), SessionState::Ready);
    assert_eq!(controller.transcript_committed(), "");
    assert_eq!(controller.transcript_live(), "");
    assert_eq!(controller.elapsed_seconds(), 0);
    assert!(controller.progress().is_none());
    assert!(!controller.is_file_take());
    assert!(controller.finished_take().is_none(), "no result to copy");
    assert!(controller.output_status().is_none());
    assert!(controller.app_error().is_none(), "a reset is not a failure");
    assert_eq!(
        sink.take(),
        vec![SttCommand::Stop],
        "one Stop tears the session down"
    );

    // The next press opens a brand-new session.
    assert!(controller.start_recording());
    assert_eq!(controller.session_state(), SessionState::Recording);
    assert_eq!(controller.session_label(), "take-3");
}

#[test]
fn a_reset_take_ignores_late_events_of_the_discarded_session() {
    let (mut controller, _sink) = duplex_controller();
    assert!(controller.start_recording());
    assert!(controller.reset_take());

    controller.handle_stt_event(SttEvent::Committed {
        session_id: "take-1".into(),
        text: "text from the discarded take".into(),
        segment: None,
    });

    assert_eq!(
        controller.transcript_committed(),
        "",
        "the abandoned session cannot write into the fresh one"
    );
}

#[test]
fn reset_is_refused_when_idle_and_for_a_file_take() {
    let (mut controller, sink) = duplex_controller();
    // Idle live screen: there is nothing to reset.
    assert!(!controller.reset_take());
    assert!(sink.take().is_empty());

    // A file take decodes on the worker: Reset must not fake a local reset.
    let file = TempFile::new("meeting.mp4");
    let mut file_controller = ready_file();
    let file_sink = RecordingSink::default();
    file_controller.set_sink(Box::new(file_sink.clone()));
    file_controller
        .start_file_session(&file.path)
        .expect("the file take starts");
    assert!(file_controller.is_file_take());
    let _ = file_sink.take();

    assert!(!file_controller.reset_take());

    assert!(file_sink.take().is_empty(), "no command may be sent");
    assert_eq!(file_controller.session_state(), SessionState::Recording);
    assert_eq!(file_controller.view_state(), ViewState::Transcription);
}

#[test]
fn cancel_ends_the_take_from_recording_and_from_paused() {
    for paused in [false, true] {
        let (mut controller, sink) = duplex_controller();
        assert!(controller.start_recording());
        if paused {
            assert!(controller.pause_recording());
        }
        let _ = sink.take();

        assert!(controller.cancel_take());

        assert_eq!(controller.view_state(), ViewState::Mode, "paused={paused}");
        assert_eq!(controller.session_state(), SessionState::Blank);
        assert!(controller.finished_take().is_none(), "nothing to save");
        assert!(
            controller.app_error().is_none(),
            "cancelling is not a failure"
        );
        assert_eq!(sink.take(), vec![SttCommand::Stop], "paused={paused}");
    }
}

#[test]
fn cancel_from_the_idle_take_screen_only_goes_home() {
    let (mut controller, sink) = duplex_controller();
    let _ = sink.take();

    assert!(controller.cancel_take());

    assert_eq!(controller.view_state(), ViewState::Mode);
    assert!(sink.take().is_empty(), "no session to stop");
}

#[test]
fn stop_ends_a_running_take_without_pausing_first() {
    let (mut controller, _sink) = duplex_controller();
    assert!(controller.start_recording());
    controller.handle_stt_event(SttEvent::Committed {
        session_id: "take-1".into(),
        text: "recorded while recording".into(),
        segment: None,
    });

    assert!(controller.finish_take());

    assert_eq!(controller.view_state(), ViewState::SessionEnd);
    let take = controller.finished_take().expect("a finished take");
    assert_eq!(take.committed(), "recorded while recording");
}

// ----- File transcription screen ---------------------------------------

#[test]
fn a_running_and_a_finished_file_take_both_stay_on_the_transcription_screen() {
    let file = TempFile::new("routing.wav");
    let mut controller = ready_file();
    let sink = RecordingSink::default();
    controller.set_sink(Box::new(sink.clone()));

    controller.start_file_session(&file.path).expect("starts");
    assert_eq!(controller.view_state(), ViewState::Transcription);

    // The worker reaches the end of the file by itself.
    controller.handle_stt_event(SttEvent::Stopped {
        session_id: "take-1".into(),
        full_text: String::new(),
    });
    assert_eq!(
        controller.view_state(),
        ViewState::Transcription,
        "the screen never leaves on its own"
    );

    assert!(controller.continue_to_end());
    assert_eq!(controller.view_state(), ViewState::SessionEnd);
}

#[test]
fn a_finished_file_take_is_not_navigated_by_the_app() {
    let file = TempFile::new("natural.wav");
    let mut controller = ready_file();
    let sink = RecordingSink::default();
    controller.set_sink(Box::new(sink.clone()));
    controller.start_file_session(&file.path).expect("starts");
    controller.handle_stt_event(SttEvent::Committed {
        session_id: "take-1".into(),
        text: "transcribed by the worker".into(),
        segment: None,
    });
    let _ = sink.take();

    controller.handle_stt_event(SttEvent::Stopped {
        session_id: "take-1".into(),
        full_text: String::new(),
    });

    // Finalized from the accumulated transcript, without asking the worker for
    // a second stop, and held behind Continue.
    assert!(
        sink.take().is_empty(),
        "natural completion sends no Stop command"
    );
    assert!(!controller.is_file_take());
    assert!(controller.is_awaiting_continue());
    assert_eq!(controller.session_state(), SessionState::Blank);
    assert_eq!(
        controller
            .finished_take()
            .expect("the take is finalized")
            .committed()
            .to_string(),
        "transcribed by the worker"
    );
    assert_eq!(controller.view_state(), ViewState::Transcription);
}

#[test]
fn a_late_stopped_event_only_refines_the_finished_take() {
    let file = TempFile::new("late.wav");
    let mut controller = ready_file();
    let sink = RecordingSink::default();
    controller.set_sink(Box::new(sink.clone()));
    controller.start_file_session(&file.path).expect("starts");
    controller.handle_stt_event(SttEvent::Stopped {
        session_id: "take-1".into(),
        full_text: String::new(),
    });
    assert!(controller.is_awaiting_continue());
    let _ = sink.take();

    controller.handle_stt_event(SttEvent::Committed {
        session_id: "take-1".into(),
        text: "words that arrived after the flush".into(),
        segment: None,
    });
    controller.handle_stt_event(SttEvent::Stopped {
        session_id: "take-1".into(),
        full_text: String::new(),
    });

    assert!(sink.take().is_empty(), "no command for a refined take");
    assert_eq!(controller.view_state(), ViewState::Transcription);
    assert_eq!(
        controller
            .finished_take()
            .expect("still one take")
            .committed()
            .to_string(),
        "words that arrived after the flush"
    );
}

#[test]
fn a_stopped_event_never_finalizes_a_live_take() {
    let (mut controller, sink) = duplex_controller();
    assert!(controller.start_recording());
    let _ = sink.take();

    controller.handle_stt_event(SttEvent::Stopped {
        session_id: "take-1".into(),
        full_text: String::new(),
    });

    assert_eq!(controller.view_state(), ViewState::Live);
    assert_eq!(controller.session_state(), SessionState::Recording);
    assert!(controller.finished_take().is_none());
}

#[test]
fn continue_is_refused_before_a_file_take_has_finished() {
    let file = TempFile::new("not-yet.wav");
    let mut controller = ready_file();
    controller.set_sink(Box::new(RecordingSink::default()));
    controller.start_file_session(&file.path).expect("starts");

    assert!(!controller.continue_to_end());
    assert_eq!(controller.view_state(), ViewState::Transcription);

    // And it is refused outright for a live take.
    let (mut live, _sink) = duplex_controller();
    assert!(live.start_recording());
    assert!(!live.continue_to_end());
    assert_eq!(live.view_state(), ViewState::Live);
}

#[test]
fn back_from_the_transcription_screen_discards_the_take() {
    for finished in [false, true] {
        let name = if finished {
            "back-done.wav"
        } else {
            "back-run.wav"
        };
        let file = TempFile::new(name);
        let mut controller = ready_file();
        let sink = RecordingSink::default();
        controller.set_sink(Box::new(sink.clone()));
        controller.start_file_session(&file.path).expect("starts");
        if finished {
            controller.handle_stt_event(SttEvent::Stopped {
                session_id: "take-1".into(),
                full_text: String::new(),
            });
        }
        let _ = sink.take();

        assert!(controller.cancel_take(), "finished={finished}");

        assert_eq!(controller.view_state(), ViewState::Mode, "{finished}");
        assert!(controller.finished_take().is_none(), "{finished}");
        assert!(!controller.is_awaiting_continue(), "{finished}");
        assert!(!controller.is_file_take(), "{finished}");
        assert_eq!(controller.transcript_committed(), "", "{finished}");
        assert!(controller.progress().is_none(), "{finished}");
        assert!(
            controller.app_error().is_none(),
            "cancelling is not a failure"
        );
    }
}

#[test]
fn pause_and_resume_are_inert_for_a_file_take() {
    let file = TempFile::new("pause.wav");
    let mut controller = ready_file();
    let sink = RecordingSink::default();
    controller.set_sink(Box::new(sink.clone()));
    controller.start_file_session(&file.path).expect("starts");
    let _ = sink.take();

    assert!(!controller.pause_recording());
    assert!(!controller.resume_recording());

    assert!(sink.take().is_empty(), "no command may be sent");
    assert_eq!(controller.session_state(), SessionState::Recording);
    assert_eq!(controller.view_state(), ViewState::Transcription);

    // A live take still pauses and resumes.
    let (mut live, _live_sink) = duplex_controller();
    assert!(live.start_recording());
    assert!(live.pause_recording());
    assert!(live.resume_recording());
}

#[test]
fn progress_events_drive_the_file_take_position() {
    let file = TempFile::new("progress.wav");
    let mut controller = ready_file();
    controller.set_sink(Box::new(RecordingSink::default()));
    controller.start_file_session(&file.path).expect("starts");

    controller.handle_stt_event(SttEvent::Progress {
        session_id: "take-1".into(),
        processed_seconds: 42.7,
        total_seconds: Some(120.0),
    });

    assert_eq!(controller.progress(), Some((42.7, Some(120.0))));
    assert_eq!(
        controller.elapsed_seconds(),
        0,
        "the corner clock times the take, not the position in the file"
    );

    // The position is cleared once the take is discarded.
    assert!(controller.cancel_take());
    assert_eq!(controller.progress(), None);
}

#[test]
fn a_file_take_times_its_own_transcription() {
    use std::time::{Duration, Instant};

    let file = TempFile::new("clock.wav");
    let mut controller = ready_file();
    controller.set_sink(Box::new(RecordingSink::default()));
    let started = Instant::now();
    controller.start_file_session(&file.path).expect("starts");

    assert_eq!(controller.elapsed_seconds(), 0, "fresh take");

    // The clock runs while the worker decodes, exactly like a live take.
    assert!(controller.refresh_elapsed(started + Duration::from_millis(3500)));
    assert_eq!(controller.elapsed_seconds(), 3);
    assert!(controller.refresh_elapsed(started + Duration::from_millis(8500)));
    assert_eq!(controller.elapsed_seconds(), 8);

    // A decoded position does not disturb it.
    controller.handle_stt_event(SttEvent::Progress {
        session_id: "take-1".into(),
        processed_seconds: 41.0,
        total_seconds: Some(90.0),
    });
    assert_eq!(controller.elapsed_seconds(), 8);
    assert_eq!(controller.progress(), Some((41.0, Some(90.0))));

    // The clock stops when the worker finishes: the report is final, and it is
    // the transcription time, never the decoded index.
    controller.handle_stt_event(SttEvent::Stopped {
        session_id: "take-1".into(),
        full_text: String::new(),
    });
    assert!(controller.is_awaiting_continue());
    assert!(!controller.refresh_elapsed(started + Duration::from_secs(30)));
    let frozen = controller.elapsed_seconds();
    assert!(frozen < 30, "the frozen clock is the take's own time");
    assert_ne!(frozen, 41, "and never the position in the file");
}

#[test]
fn stopping_a_file_take_early_reports_it_as_stopped() {
    let file = TempFile::new("interrupted.wav");
    let mut controller = ready_file();
    controller.set_sink(Box::new(RecordingSink::default()));
    controller.start_file_session(&file.path).expect("starts");
    assert!(!controller.file_take_stopped_early(), "still decoding");

    // The user ends the decode before the worker reaches the end of the file.
    assert!(controller.finish_take());

    assert!(controller.is_awaiting_continue());
    assert!(
        controller.file_take_stopped_early(),
        "an interrupted take is reported as stopped, not done"
    );

    // The report survives into the result screen…
    assert!(controller.continue_to_end());
    assert!(controller.file_take_stopped_early());
    // …and is dropped with the take.
    controller.new_take();
    assert!(!controller.file_take_stopped_early());
}

#[test]
fn a_file_take_the_worker_finished_is_not_reported_as_stopped() {
    let file = TempFile::new("completed.wav");
    let mut controller = ready_file();
    controller.set_sink(Box::new(RecordingSink::default()));
    controller.start_file_session(&file.path).expect("starts");

    controller.handle_stt_event(SttEvent::Stopped {
        session_id: "take-1".into(),
        full_text: String::new(),
    });

    assert!(controller.is_awaiting_continue());
    assert!(
        !controller.file_take_stopped_early(),
        "a natural completion is done, not stopped"
    );

    // A live take never reports a file-take stop either.
    let (mut live, _sink) = duplex_controller();
    assert!(live.start_recording());
    assert!(live.finish_take());
    assert!(!live.file_take_stopped_early());
}

// ----- Back contract ---------------------------------------------------

#[test]
fn back_from_the_setup_form_returns_to_the_chooser_with_nothing_left() {
    let mut controller = catalogued();
    controller.choose_mode(SessionMode::File);
    controller
        .set_pending_file(PathBuf::from("/tmp/interview.mp4"))
        .expect("a media path");
    controller.cycle_provider();
    controller.report_error(AppError::FileDialogFailed);

    // Back on the footer: one step towards the chooser.
    assert!(controller.go_back());

    assert_eq!(controller.view_state(), ViewState::Mode);
    assert_eq!(controller.session_state(), SessionState::Blank);
    assert_eq!(controller.pending_file(), None, "the file is forgotten");
    assert!(!controller.has_provider(), "the draft is discarded");
    assert!(controller.app_error().is_none(), "no stale error strip");
    assert!(controller.finished_take().is_none());
    assert!(controller.output_status().is_none());
}

#[test]
fn back_from_the_idle_live_screen_returns_to_the_chooser() {
    let mut controller = ready_live();
    controller.report_error(AppError::BackendUnavailable);

    assert!(controller.go_back());

    assert_eq!(controller.view_state(), ViewState::Mode);
    assert_eq!(controller.session_state(), SessionState::Blank);
    assert!(controller.app_error().is_none(), "no stale error strip");
    assert!(controller.finished_take().is_none());
    assert!(controller.pending_file().is_none());
}

#[test]
fn back_from_the_stopped_screen_clears_the_finished_take() {
    let mut controller = ready_live();
    assert!(controller.start_recording());
    controller.handle_stt_event(SttEvent::Committed {
        session_id: "take-1".into(),
        text: "a finished line".into(),
        segment: None,
    });
    assert!(controller.pause_recording());
    assert!(controller.finish_take());
    controller.report_error(AppError::PasteFailed);
    assert_eq!(controller.view_state(), ViewState::SessionEnd);

    // Back on the stopped footer clears the finished take.
    assert!(controller.go_back());

    assert_eq!(controller.view_state(), ViewState::Mode);
    assert_eq!(controller.session_state(), SessionState::Blank);
    assert!(controller.finished_take().is_none(), "the take is gone");
    assert!(controller.app_error().is_none(), "no stale error strip");
    assert!(controller.output_status().is_none());
    assert_eq!(controller.transcript_committed(), "");
}

// ----- Transport ------------------------------------------------------

#[test]
fn start_recording_is_refused_in_file_mode() {
    let mut controller = ready_file();

    assert!(!controller.start_recording());
    // File mode has no holding screen: the form is what the user sees.
    assert_eq!(controller.view_state(), ViewState::Form);
    assert_eq!(controller.session_state(), SessionState::Ready);
}

#[test]
fn recording_pauses_resumes_and_finishes_a_take() {
    let mut controller = ready_live();

    assert!(controller.start_recording());
    assert_eq!(controller.view_state(), ViewState::Live);

    assert!(controller.pause_recording());
    assert_eq!(controller.view_state(), ViewState::Live);

    assert!(controller.resume_recording());
    assert_eq!(controller.view_state(), ViewState::Live);

    assert!(controller.pause_recording());
    assert!(controller.finish_take());

    // The machine is Blank again, but the view still shows the finished take.
    assert_eq!(controller.session_state(), SessionState::Blank);
    assert_eq!(controller.view_state(), ViewState::SessionEnd);

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
            language,
            api_key,
        } => {
            assert_eq!(session_id, "take-1");
            assert_eq!(provider, "vercel-ai-gateway");
            assert_eq!(model, "openai/gpt-4o-transcribe");
            assert_eq!(*sample_rate, SAMPLE_RATE);
            assert!(language.is_none(), "no dictation language is configured");
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
        segment: None,
    });
    assert_eq!(controller.transcript_committed(), "");
    assert_eq!(controller.transcript_live(), "audio");

    controller.handle_stt_event(SttEvent::Delta {
        session_id: "take-1".into(),
        delta: " capture".into(),
        segment: None,
    });
    assert_eq!(controller.transcript_live(), "audio capture");

    controller.handle_stt_event(SttEvent::Committed {
        session_id: "take-1".into(),
        text: "audio capture layer.".into(),
        segment: None,
    });
    assert_eq!(controller.transcript_committed(), "audio capture layer.");
    assert_eq!(controller.transcript_live(), "");

    controller.handle_stt_event(SttEvent::Partial {
        session_id: "take-1".into(),
        text: "next words".into(),
        segment: None,
    });
    controller.handle_stt_event(SttEvent::Delta {
        session_id: "take-1".into(),
        delta: " now".into(),
        segment: None,
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
fn a_segmented_live_take_commits_every_word_once_and_never_shrinks() {
    let (mut controller, _sink) = duplex_controller();
    assert!(controller.start_recording());

    // Three utterances: each is revised inside its own segment, then finalized.
    let events = [
        SttEvent::Partial {
            session_id: "take-1".into(),
            text: "one".into(),
            segment: Some("seg-0".into()),
        },
        SttEvent::Delta {
            session_id: "take-1".into(),
            delta: " two".into(),
            segment: Some("seg-0".into()),
        },
        SttEvent::Committed {
            session_id: "take-1".into(),
            text: "one two.".into(),
            segment: Some("seg-0".into()),
        },
        SttEvent::Partial {
            session_id: "take-1".into(),
            text: "three".into(),
            segment: Some("seg-1".into()),
        },
        SttEvent::Committed {
            session_id: "take-1".into(),
            text: "three four.".into(),
            segment: Some("seg-1".into()),
        },
        SttEvent::Partial {
            session_id: "take-1".into(),
            text: "five".into(),
            segment: Some("seg-2".into()),
        },
        // A final belonging to the *next* utterance settles the unfinished
        // one first: "five" is never dropped, and never repeated.
        SttEvent::Committed {
            session_id: "take-1".into(),
            text: "six.".into(),
            segment: Some("seg-3".into()),
        },
    ];

    let mut previous = String::new();
    for event in &events {
        controller.handle_stt_event(event.clone());
        let committed = controller.transcript_committed().to_owned();
        assert!(
            committed.starts_with(&previous),
            "committed shrank after {event:?}: {previous:?} -> {committed:?}"
        );
        previous = committed;
    }

    assert_eq!(
        controller.transcript_committed(),
        "one two. three four. five six."
    );
    assert_eq!(controller.transcript_live(), "");
    assert_eq!(controller.view_state(), ViewState::Live);

    let words: Vec<String> = controller
        .transcript_full_text()
        .split_whitespace()
        .map(ToOwned::to_owned)
        .collect();
    assert_eq!(
        words,
        ["one", "two.", "three", "four.", "five", "six."],
        "every word the backend produced appears exactly once"
    );

    // The authoritative flush still ends the take with one clean transcript.
    assert!(controller.finish_take());
    controller.handle_stt_event(SttEvent::Stopped {
        session_id: "take-1".into(),
        full_text: "One two. Three four. Five six.".into(),
    });
    let take = controller.finished_take().expect("shows the finished take");
    assert_eq!(
        take.committed().to_string(),
        "One two. Three four. Five six."
    );
    assert_eq!(take.live().to_string(), "");
}

#[test]
fn backend_errors_are_surfaced_and_cleared_by_the_next_take() {
    let (mut controller, _sink) = duplex_controller();
    assert!(controller.start_recording());

    controller.handle_stt_event(SttEvent::Error {
        session_id: Some("take-1".into()),
        request_id: None,
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
        request_id: None,
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
        segment: None,
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
        segment: None,
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
        segment: None,
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

// ----- Media allowlist -------------------------------------------------

#[test]
fn the_pending_file_refuses_a_non_media_path() {
    let mut controller = catalogued();
    controller.choose_mode(SessionMode::File);

    let result = controller.set_pending_file(PathBuf::from("/tmp/report.pdf"));

    assert_eq!(result, Err(AppError::UnsupportedFileType));
    assert_eq!(controller.pending_file(), None, "nothing is stored");
    assert_eq!(
        controller.view_state(),
        ViewState::Mode,
        "the user stays on the welcome screen"
    );
    assert_eq!(controller.session_state(), SessionState::Blank);
}

#[test]
fn the_pending_file_accepts_every_media_kind() {
    for path in [
        "/tmp/a.wav",
        "/tmp/b.MP4",
        "/tmp/c.mkv",
        "/tmp/d.flac",
        "/tmp/e.webm",
    ] {
        let mut controller = catalogued();
        controller.choose_mode(SessionMode::File);
        controller
            .set_pending_file(PathBuf::from(path))
            .unwrap_or_else(|error| panic!("{path}: {error}"));
        assert_eq!(controller.pending_file(), Some(Path::new(path)), "{path}");
        assert_eq!(controller.view_state(), ViewState::Form, "{path}");
    }
}

#[test]
fn a_file_take_refuses_a_non_media_path_before_checking_existence() {
    let mut controller = ready_file();
    let sink = RecordingSink::default();
    controller.set_sink(Box::new(sink.clone()));
    // Missing *and* not media: the type wins, because it is the real problem.
    let path = std::env::temp_dir().join("speechtt-definitely-missing-42.pdf");

    let result = controller.start_file_session(&path);

    assert_eq!(result, Err(AppError::UnsupportedFileType));
    assert!(sink.take().is_empty());
    assert_eq!(controller.session_state(), SessionState::Ready);
}

#[test]
fn a_file_take_accepts_an_extensionless_media_container() {
    // `.tar.gz`-style stacks are not media, but a bare name is still refused
    // even when the bytes would decode: the extension is the contract.
    let mut controller = ready_file();
    let result = controller.start_file_session(Path::new("/tmp/recording"));
    assert_eq!(result, Err(AppError::UnsupportedFileType));
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

    // A directory without a media extension is refused as the wrong file type,
    // which is the more useful message.
    assert_eq!(
        controller.start_file_session(&std::env::temp_dir()),
        Err(AppError::UnsupportedFileType)
    );

    // A directory that *looks* media-shaped gets past the extension gate and
    // fails on the file check instead.
    let mut shaped = std::env::temp_dir();
    shaped.push(format!("speechtt-dir-{}", std::process::id()));
    shaped.push("fake.mp4");
    std::fs::create_dir_all(&shaped).expect("temp dir");
    let result = controller.start_file_session(&shaped);
    let _ = std::fs::remove_dir_all(shaped.parent().expect("parent"));

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

    assert_eq!(controller.view_state(), ViewState::Transcription);
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
            language: None,
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

    assert_eq!(controller.progress(), Some((12.5, Some(340.25))));
    assert_eq!(
        controller.elapsed_seconds(),
        0,
        "the decoded index leaves the elapsed clock alone"
    );
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
fn file_take_stops_early_and_waits_for_continue() {
    let file = TempFile::new("finish.wav");
    let mut controller = ready_file();
    let sink = RecordingSink::default();
    controller.set_sink(Box::new(sink.clone()));
    controller.start_file_session(&file.path).expect("starts");
    controller.handle_stt_event(SttEvent::Committed {
        session_id: "take-1".into(),
        text: "hello file".into(),
        segment: None,
    });
    sink.take();

    assert!(controller.finish_take());

    // Stopping sends exactly one Stop, keeps the partial text, and leaves the
    // user on the transcription screen with Continue available.
    assert_eq!(sink.take(), vec![SttCommand::Stop]);
    assert_eq!(controller.view_state(), ViewState::Transcription);
    assert!(!controller.is_file_take());
    assert!(controller.is_awaiting_continue());
    assert_eq!(
        controller
            .finished_take()
            .expect("take")
            .committed()
            .to_string(),
        "hello file"
    );

    assert!(controller.continue_to_end());
    assert_eq!(controller.view_state(), ViewState::SessionEnd);
    assert!(!controller.continue_to_end(), "already revealed");
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
        segment: None,
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
        segment: None,
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
fn saving_without_a_finished_take_reports_nothing_recorded() {
    let directory = TempDir::new("save-none");
    let target = directory.path().join("notes.md");
    let mut controller = SessionController::new();

    let result = controller.save_finished_transcript_to(&target);

    assert_eq!(result, Err(AppError::NothingRecorded));
    assert!(!target.exists());
    assert!(controller.app_error().is_none());
}

#[test]
fn saving_an_empty_transcript_is_informational_and_writes_nothing() {
    use speechtt_core::output::clipboard::ClipboardSink;
    use speechtt_core::output::paste::PasteSink;

    #[derive(Default)]
    struct RecordingClipboard(Arc<Mutex<Vec<String>>>);

    impl ClipboardSink for RecordingClipboard {
        fn copy(&mut self, text: &str) -> Result<(), speechtt_core::output::OutputError> {
            self.0.lock().expect("clipboard lock").push(text.to_owned());
            Ok(())
        }
    }

    struct NeverPaste;

    impl PasteSink for NeverPaste {
        fn paste(&mut self) -> Result<(), speechtt_core::output::OutputError> {
            panic!("an empty take must never paste");
        }
    }

    let directory = TempDir::new("save-empty");
    let target = directory.path().join("notes.md");
    // A real sink keeps the mock transcript out of the finished take.
    let (mut controller, _sink) = duplex_controller();
    assert!(controller.start_recording());
    controller.handle_stt_event(SttEvent::Committed {
        session_id: "take-1".into(),
        text: "   ".into(),
        segment: None,
    });
    assert!(controller.pause_recording());
    assert!(controller.finish_take());

    assert!(controller.finished_take_is_empty());
    assert!(
        controller.app_error().is_none(),
        "no strip for an empty take"
    );

    let result = controller.save_finished_transcript_to(&target);
    assert_eq!(result, Err(AppError::NothingRecorded));
    assert!(!target.exists());
    assert!(
        controller.app_error().is_none(),
        "no strip for an empty take"
    );
    assert!(controller.output_status().is_none());

    let mut clipboard = RecordingClipboard::default();
    assert_eq!(
        controller.copy_finished_transcript(&mut clipboard),
        Err(AppError::NothingRecorded)
    );
    assert!(clipboard.0.lock().expect("clipboard lock").is_empty());
    assert!(
        controller.app_error().is_none(),
        "no strip for an empty take"
    );
    assert!(controller.output_status().is_none());

    assert!(
        controller
            .auto_paste_after_finish(&mut clipboard, &mut NeverPaste)
            .is_none()
    );
}

#[test]
fn a_take_that_recorded_nothing_finishes_without_an_error() {
    let (mut controller, _sink) = duplex_controller();
    assert!(controller.start_recording());
    assert!(controller.pause_recording());
    assert!(controller.finish_take());

    assert!(controller.finished_take_is_empty());
    assert!(controller.app_error().is_none());
    assert_eq!(controller.view_state(), ViewState::SessionEnd);

    // A late empty flush must not introduce an error either.
    controller.handle_stt_event(SttEvent::Stopped {
        session_id: "take-1".into(),
        full_text: String::new(),
    });
    assert!(controller.finished_take_is_empty());
    assert!(controller.app_error().is_none());
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

#[test]
fn a_file_take_names_its_file_while_decoding_and_after() {
    let file = TempFile::new("budget-review.m4a");
    let mut controller = ready_file();
    controller.set_sink(Box::new(RecordingSink::default()));
    controller.start_file_session(&file.path).expect("starts");

    let expected = format!("speechtt-test-{}-budget-review.m4a", std::process::id());
    assert_eq!(controller.file_display_name(), Some(expected.as_str()));

    // The machine drops its configuration when the take ends; the name stays,
    // so the screen never falls back to a placeholder.
    controller.handle_stt_event(SttEvent::Stopped {
        session_id: "take-1".into(),
        full_text: String::new(),
    });
    assert!(controller.is_awaiting_continue());
    assert_eq!(
        controller.file_display_name(),
        Some(expected.as_str()),
        "the name survives the end of the decode"
    );
    assert_eq!(controller.audio_source(), None, "the machine is reset");

    assert!(controller.continue_to_end());
    assert_eq!(controller.view_state(), ViewState::SessionEnd);
    let expected = format!("speechtt-test-{}-budget-review.m4a", std::process::id());
    assert_eq!(controller.file_display_name(), Some(expected.as_str()));

    // Discarding the take drops the name with it.
    controller.new_take();
    assert_eq!(controller.file_display_name(), None);
}

#[test]
fn a_live_take_never_inherits_a_file_name() {
    let file = TempFile::new("inherited.wav");
    let mut controller = ready_file();
    controller.set_sink(Box::new(RecordingSink::default()));
    controller.start_file_session(&file.path).expect("starts");
    assert!(controller.file_display_name().is_some());

    let mut live = duplex_controller().0;
    assert!(live.start_recording());
    assert_eq!(live.file_display_name(), None, "a live take names no file");
}

// ----- T30.6: cleaning the finished transcript -------------------------

/// The two take defaults plus a key, so a take can actually start.
fn keyed_settings(streaming: &str, batch: &str) -> Settings {
    let mut settings = configured_settings(streaming, batch);
    settings.set_api_key("vercel-ai-gateway", "sk-clean-test");
    settings
}

/// Settings with a cleanup model and the two take defaults.
fn cleanable_settings(language_model: &str) -> Settings {
    Settings {
        language_model: language_model.to_owned(),
        auto_paste: false,
        ..keyed_settings(DEFAULTS.0, DEFAULTS.1)
    }
}

/// A finished live take whose controller has a cleanup model to call.
fn cleanable_take() -> SessionController {
    let mut controller = finished_live_with("d une dict ée");
    controller.apply_settings(cleanable_settings("openai/gpt-4.1-mini"));
    controller
}

/// A finished live take that asked for the automatic copy/paste, with a
/// cleanup model available.
fn cleanable_take_with_auto_paste() -> SessionController {
    let (mut controller, _sink) = duplex_controller();
    controller.apply_settings(Settings {
        auto_paste: true,
        language_model: "openai/gpt-4.1-mini".to_owned(),
        ..keyed_settings(DEFAULTS.0, DEFAULTS.1)
    });
    assert!(controller.start_recording());
    controller.handle_stt_event(SttEvent::Committed {
        session_id: "take-1".into(),
        text: "d une dict ée".into(),
        segment: None,
    });
    assert!(controller.pause_recording());
    assert!(controller.finish_take());
    controller.handle_stt_event(SttEvent::Stopped {
        session_id: "take-1".into(),
        full_text: "d une dict ée".into(),
    });
    controller
}

/// Records every copy the controller makes.
#[derive(Clone, Default)]
struct RecordingClipboard {
    copies: Arc<Mutex<Vec<String>>>,
}

impl RecordingClipboard {
    fn copies(&self) -> Vec<String> {
        self.copies.lock().expect("clipboard lock").clone()
    }
}

impl speechtt_core::output::clipboard::ClipboardSink for RecordingClipboard {
    fn copy(&mut self, text: &str) -> Result<(), speechtt_core::output::OutputError> {
        self.copies
            .lock()
            .expect("clipboard lock")
            .push(text.to_owned());
        Ok(())
    }
}

/// Records every automatic paste the controller makes.
#[derive(Clone, Default)]
struct RecordingPaste {
    pastes: Arc<Mutex<u32>>,
}

impl speechtt_core::output::paste::PasteSink for RecordingPaste {
    fn paste(&mut self) -> Result<(), speechtt_core::output::OutputError> {
        *self.pastes.lock().expect("paste lock") += 1;
        Ok(())
    }
}

#[test]
fn the_cleanup_walks_unavailable_ready_running_cleaned_and_back() {
    // No cleanup model resolves: the wand has nothing to call.
    let mut controller = finished_live_with("d une dict ée");
    assert_eq!(controller.clean_status(), CleanStatus::Unavailable);
    assert_eq!(controller.clean_state(), CleanState::Unavailable);
    assert_eq!(controller.cleanup_model(), None);
    assert!(!controller.can_clean_transcript());
    assert_eq!(controller.begin_clean_transcript(), None);

    // A stored model makes it ready.
    let mut controller = cleanable_take();
    assert_eq!(controller.clean_status(), CleanStatus::Ready);
    assert_eq!(controller.clean_state(), CleanState::Ready);
    assert_eq!(controller.cleanup_model(), Some("openai/gpt-4.1-mini"));
    assert!(controller.can_clean_transcript());

    let request = controller.begin_clean_transcript().expect("a request id");
    assert_eq!(controller.clean_status(), CleanStatus::Running);
    assert!(controller.clean_is_running());
    assert!(!controller.can_clean_transcript(), "one pass at a time");
    assert_eq!(controller.begin_clean_transcript(), None);

    assert!(controller.apply_cleaned(&request, "d'une dictée"));
    assert_eq!(controller.clean_status(), CleanStatus::Cleaned);
    assert_eq!(controller.clean_state(), CleanState::Cleaned);
    assert!(!controller.clean_is_running());

    // The toggle now flips between the two halves, so the button stays live.
    assert!(controller.can_clean_transcript());
    assert_eq!(controller.clean_toggle(), CleanToggle::ShowOriginal);

    assert!(controller.flip_clean_transcript());
    assert_eq!(controller.clean_status(), CleanStatus::ShowingOriginal);
    assert_eq!(controller.clean_state(), CleanState::ShowingOriginal);
    assert_eq!(controller.clean_toggle(), CleanToggle::ShowCleaned);
    assert_eq!(controller.displayed_transcript(), "d une dict ée");
    assert_eq!(controller.output_status(), Some("Original restored"));

    assert!(controller.flip_clean_transcript());
    assert_eq!(controller.clean_status(), CleanStatus::Cleaned);
    assert_eq!(controller.displayed_transcript(), "d'une dictée");
    assert_eq!(controller.output_status(), Some("Cleaned"));
}

#[test]
fn the_toggle_starts_once_and_then_only_flips() {
    let mut controller = cleanable_take();
    assert_eq!(controller.clean_toggle(), CleanToggle::Start);

    let request = controller.begin_clean_transcript().expect("a request id");
    assert_eq!(
        controller.clean_toggle(),
        CleanToggle::Inert,
        "a request in flight cannot be started twice"
    );

    assert!(controller.apply_cleaned(&request, "d'une dictée"));
    assert_eq!(controller.clean_toggle(), CleanToggle::ShowOriginal);
    assert!(controller.flip_clean_transcript());
    assert_eq!(controller.clean_toggle(), CleanToggle::ShowCleaned);
    assert!(controller.flip_clean_transcript());
    assert_eq!(controller.clean_toggle(), CleanToggle::ShowOriginal);
}

#[test]
fn a_cached_pass_stays_flippable_after_the_model_disappears() {
    let mut controller = cleanable_take();
    let request = controller.begin_clean_transcript().expect("a request id");
    assert!(controller.apply_cleaned(&request, "d'une dictée"));

    // Neither a stored model nor a worker default resolves any more: the button
    // must still let the user compare the two halves it already holds.
    controller.apply_settings(speechtt_core::settings::Settings::default());
    controller.set_capabilities(Vec::new());
    assert_eq!(controller.cleanup_model(), None);
    assert_eq!(controller.clean_status(), CleanStatus::Cleaned);
    assert!(controller.can_clean_transcript());
    assert!(controller.flip_clean_transcript());
    assert_eq!(controller.displayed_transcript(), "d une dict ée");
}

#[test]
fn a_flip_is_inert_without_a_cached_pass() {
    let mut controller = cleanable_take();
    assert!(!controller.flip_clean_transcript());
    assert_eq!(controller.displayed_transcript(), "d une dict ée");

    let request = controller.begin_clean_transcript().expect("a request id");
    assert!(
        !controller.flip_clean_transcript(),
        "a request in flight cannot be flipped"
    );
    assert!(controller.clean_is_running());
    let _ = request;
}

#[test]
fn a_cleanup_is_a_single_pass_over_a_non_empty_take() {
    let (mut controller, _sink) = duplex_controller();
    controller.apply_settings(cleanable_settings("openai/gpt-4.1-mini"));
    assert!(controller.start_recording());
    assert!(controller.pause_recording());
    assert!(controller.finish_take());

    assert!(controller.finished_take_is_empty());
    assert_eq!(
        controller.clean_status(),
        CleanStatus::Ready,
        "the model is known; only the take is empty"
    );
    assert!(!controller.can_clean_transcript());
    assert_eq!(controller.begin_clean_transcript(), None);
}

#[test]
fn a_worker_default_cleanup_model_is_enough_to_clean() {
    let mut controller = finished_live_with("d une dict ée");
    controller.apply_settings(cleanable_settings(""));
    assert_eq!(controller.cleanup_model(), None);
    assert_eq!(controller.clean_status(), CleanStatus::Unavailable);

    controller.set_language_catalogue(
        None,
        Some(vec![LanguageModelInfo {
            id: "openai/gpt-4.1-mini".to_owned(),
            label: "GPT-4.1 mini".to_owned(),
        }]),
        Some("openai/gpt-4.1-mini".to_owned()),
    );

    assert_eq!(controller.cleanup_model(), Some("openai/gpt-4.1-mini"));
    assert_eq!(controller.clean_status(), CleanStatus::Ready);

    // A stored id wins over the worker default.
    controller.apply_settings(cleanable_settings("openai/gpt-5-mini"));
    assert_eq!(controller.cleanup_model(), Some("openai/gpt-5-mini"));
    assert_eq!(
        controller.default_cleanup_model(),
        Some("openai/gpt-4.1-mini"),
        "the dialog still knows what the worker would use"
    );
}

#[test]
fn undo_restores_the_original_transcript_exactly() {
    let mut controller = cleanable_take();
    let original = controller.displayed_transcript();
    assert_eq!(original, "d une dict ée");

    let request = controller.begin_clean_transcript().expect("a request id");
    assert!(controller.apply_cleaned(&request, "d'une dictée"));

    assert_eq!(controller.displayed_transcript(), "d'une dictée");
    let take = controller.finished_take().expect("a finished take");
    assert_eq!(take.committed().to_string(), "d'une dictée");
    assert_eq!(take.live().to_string(), "");
    assert_eq!(controller.output_status(), Some("Cleaned"));

    assert!(controller.flip_clean_transcript());
    assert_eq!(controller.displayed_transcript(), original);
    let take = controller.finished_take().expect("a finished take");
    assert_eq!(take.committed().to_string(), "d une dict ée");
    assert_eq!(take.live().to_string(), "");
}

#[test]
fn an_empty_cleaned_text_never_blanks_the_transcript() {
    let mut controller = cleanable_take();
    let request = controller.begin_clean_transcript().expect("a request id");

    assert!(!controller.apply_cleaned(&request, "   "));
    assert_eq!(controller.displayed_transcript(), "d une dict ée");
    assert!(!controller.clean_is_running(), "the request is settled");
    assert_eq!(controller.clean_status(), CleanStatus::Ready);
}

#[test]
fn a_cleanup_answer_for_another_request_is_ignored() {
    let mut controller = cleanable_take();
    let first = controller.begin_clean_transcript().expect("first request");
    assert!(controller.apply_cleaned(&first, "d'une dictée"));
    assert!(controller.flip_clean_transcript());

    let second = controller.begin_clean_transcript().expect("second request");
    assert_ne!(first, second);
    assert!(
        !controller.apply_cleaned(&first, "a stale answer"),
        "an answer to a superseded request is dropped"
    );
    assert!(controller.clean_is_running());
    assert_eq!(controller.displayed_transcript(), "d une dict ée");

    assert!(
        !controller.apply_cleaned("clean-99-99", "invented"),
        "a request id the controller never issued is dropped"
    );
    assert!(controller.apply_cleaned(&second, "d'une dictée"));
}

#[test]
fn a_failed_cleanup_keeps_the_transcript_and_reports_a_typed_error() {
    let mut controller = cleanable_take();
    let request = controller.begin_clean_transcript().expect("a request id");

    assert!(controller.fail_clean_transcript(&request, AppError::CleanupFailed));

    assert!(!controller.clean_is_running());
    assert_eq!(controller.clean_status(), CleanStatus::Ready);
    assert_eq!(controller.displayed_transcript(), "d une dict ée");
    assert!(
        !controller.flip_clean_transcript(),
        "nothing cached to flip"
    );
    assert!(matches!(
        controller.app_error(),
        Some(AppError::CleanupFailed)
    ));
    assert_eq!(
        controller.app_error().map(AppError::message).as_deref(),
        Some("The cleanup pass failed. The transcript is unchanged."),
        "a cleanup failure names itself instead of blaming the backend"
    );

    let message = controller
        .app_error()
        .map(AppError::message)
        .unwrap_or_default();
    assert!(
        !message.contains("dict"),
        "the transcript never reaches an error message: {message}"
    );

    assert!(
        !controller.fail_clean_transcript(&request, AppError::NetworkLost),
        "a request settles exactly once"
    );
    assert_eq!(
        controller.clean_status(),
        CleanStatus::Ready,
        "the second failure changed nothing"
    );
}

#[test]
fn copy_and_save_follow_the_displayed_text() {
    let mut controller = cleanable_take();
    let request = controller.begin_clean_transcript().expect("a request id");
    assert!(controller.apply_cleaned(&request, "d'une dictée"));

    let mut clipboard = RecordingClipboard::default();
    controller
        .copy_finished_transcript(&mut clipboard)
        .expect("copy succeeds");
    assert_eq!(clipboard.copies(), vec!["d'une dictée".to_owned()]);

    let directory = TempDir::new("clean-save");
    let target = directory.path().join("cleaned.md");
    controller
        .save_finished_transcript_to(&target)
        .expect("save succeeds");
    assert_eq!(
        std::fs::read_to_string(&target).expect("saved file"),
        "# cleaned\n\nd'une dictée\n"
    );

    assert!(controller.flip_clean_transcript());
    let mut clipboard = RecordingClipboard::default();
    controller
        .copy_finished_transcript(&mut clipboard)
        .expect("copy succeeds");
    assert_eq!(clipboard.copies(), vec!["d une dict ée".to_owned()]);
}

#[test]
fn the_automatic_paste_copies_the_displayed_text() {
    let mut controller = cleanable_take_with_auto_paste();
    assert!(
        controller.take_auto_paste_request(),
        "the gesture is pending"
    );

    let request = controller.begin_clean_transcript().expect("a request id");
    assert!(controller.apply_cleaned(&request, "d'une dictée"));

    let mut clipboard = RecordingClipboard::default();
    let mut paste = RecordingPaste::default();
    assert!(
        controller
            .auto_paste_after_finish(&mut clipboard, &mut paste)
            .is_some(),
        "auto-paste still runs after a cleanup"
    );
    assert_eq!(clipboard.copies(), vec!["d'une dictée".to_owned()]);
    assert_eq!(*paste.pastes.lock().expect("paste lock"), 1);
}

#[test]
fn a_new_take_forgets_the_cleanup_state() {
    let mut controller = cleanable_take();
    let request = controller.begin_clean_transcript().expect("a request id");
    assert!(controller.apply_cleaned(&request, "d'une dictée"));

    controller.new_take();

    assert!(!controller.flip_clean_transcript());
    assert!(!controller.clean_is_running());
    assert!(
        !controller.apply_cleaned(&request, "an answer to the discarded take"),
        "an id issued for the previous take is ignored"
    );
    assert!(controller.finished_take().is_none());
}

#[test]
fn a_take_that_starts_again_forgets_the_cleaned_text() {
    let mut controller = cleanable_take();
    let request = controller.begin_clean_transcript().expect("a request id");
    assert!(controller.apply_cleaned(&request, "d'une dictée"));

    controller.new_take();
    assert!(controller.choose_mode(SessionMode::Live));
    assert!(controller.start_recording());

    assert!(!controller.clean_is_running());
    assert!(controller.finished_take().is_none());
    controller.handle_stt_event(SttEvent::Committed {
        session_id: "take-2".into(),
        text: "a new take".into(),
        segment: None,
    });
    assert!(controller.pause_recording());
    assert!(controller.finish_take());
    assert_eq!(controller.displayed_transcript(), "a new take");
    assert_eq!(controller.clean_status(), CleanStatus::Ready);
}

#[test]
fn the_capabilities_event_installs_the_cleanup_catalogue_and_languages() {
    let mut controller = SessionController::new();
    controller.handle_stt_event(SttEvent::Capabilities {
        request_id: None,
        source: CapabilitySource::Builtin,
        providers: catalogue(),
        languages: Some(vec![LanguageOption {
            code: "fr".to_owned(),
            label: "French".to_owned(),
        }]),
        language_models: Some(vec![LanguageModelInfo {
            id: "openai/gpt-4.1-mini".to_owned(),
            label: "GPT-4.1 mini".to_owned(),
        }]),
        default_language_model: Some("openai/gpt-4.1-mini".to_owned()),
    });

    assert_eq!(controller.cleanup_models().len(), 1);
    assert_eq!(
        controller.default_cleanup_model(),
        Some("openai/gpt-4.1-mini")
    );

    let options = controller.settings_language_options();
    assert_eq!(options[0].id, "", "Auto is always the first entry");
    assert_eq!(options[0].label, "Auto (detect)");
    assert!(
        options.iter().any(|option| option.id == "fr"),
        "the worker's languages are offered: {options:?}"
    );
}

#[test]
fn an_older_worker_still_gets_a_language_menu() {
    let controller = catalogued();

    let options = controller.settings_language_options();

    assert_eq!(options[0].id, "");
    assert!(
        options
            .iter()
            .any(|option| option.id == "fr" && option.label == "French"),
        "the built-in list backs an older worker: {options:?}"
    );
    assert_eq!(controller.cleanup_models().len(), 0);
    assert_eq!(controller.default_cleanup_model(), None);
}

#[test]
fn the_settings_draft_carries_the_language_and_the_cleanup_model() {
    let mut controller = catalogued();
    controller.set_language_catalogue(
        Some(vec![
            LanguageOption {
                code: "fr".to_owned(),
                label: "French".to_owned(),
            },
            LanguageOption {
                code: "en".to_owned(),
                label: "English".to_owned(),
            },
        ]),
        Some(vec![LanguageModelInfo {
            id: "openai/gpt-4.1-mini".to_owned(),
            label: "GPT-4.1 mini".to_owned(),
        }]),
        Some("openai/gpt-4.1-mini".to_owned()),
    );
    controller.apply_settings(configured_settings(DEFAULTS.0, DEFAULTS.1));
    controller.open_settings();

    // An unset language is Auto, not a blank field.
    assert_eq!(controller.settings_draft().language, None);
    assert!(controller.set_settings_language("FR"));
    assert_eq!(
        controller.settings_draft().language.as_deref(),
        Some("fr"),
        "the stored code is normalised"
    );
    assert!(
        !controller.set_settings_language("xx"),
        "a language the picker does not offer is refused"
    );
    assert!(controller.set_settings_language(""));
    assert_eq!(
        controller.settings_draft().language,
        None,
        "Auto is the empty language"
    );

    assert_eq!(controller.settings_draft().language_model, None);
    assert!(controller.set_settings_cleanup_model("openai/gpt-4.1-mini"));
    assert_eq!(
        controller.settings_draft().language_model.as_deref(),
        Some("openai/gpt-4.1-mini")
    );
    assert!(
        !controller.set_settings_cleanup_model("unknown/model"),
        "a model the catalogue does not offer is refused"
    );
    assert!(controller.set_settings_cleanup_model(""));
    assert_eq!(controller.settings_draft().language_model, None);
}

#[test]
fn a_take_carries_the_persisted_dictation_language() {
    let mut controller = catalogued();
    controller.apply_settings(Settings {
        language: " FR ".into(),
        ..keyed_settings(DEFAULTS.0, DEFAULTS.1)
    });
    let sink = RecordingSink::default();
    controller.set_sink(Box::new(sink.clone()));

    assert!(controller.choose_mode(SessionMode::Live));
    assert!(controller.start_recording());

    let commands = sink.take();
    match &commands[0] {
        SttCommand::Start { language, .. } => {
            assert_eq!(
                language.as_deref(),
                Some("fr"),
                "the normalized code reaches the worker"
            );
        }
        other => panic!("expected a start command, got {other:?}"),
    }
}

#[test]
fn auto_sends_no_language_at_all() {
    let mut controller = catalogued();
    controller.apply_settings(keyed_settings(DEFAULTS.0, DEFAULTS.1));
    let sink = RecordingSink::default();
    controller.set_sink(Box::new(sink.clone()));

    assert!(controller.choose_mode(SessionMode::Live));
    assert!(controller.start_recording());

    let commands = sink.take();
    match &commands[0] {
        SttCommand::Start { language, .. } => assert_eq!(*language, None),
        other => panic!("expected a start command, got {other:?}"),
    }
}

#[test]
fn a_file_take_forwards_the_dictation_language_too() {
    let file = TempFile::new("dictated-french.mp4");
    let mut controller = catalogued();
    controller.apply_settings(Settings {
        language: "fr".into(),
        ..keyed_settings(DEFAULTS.0, DEFAULTS.1)
    });
    let sink = RecordingSink::default();
    controller.set_sink(Box::new(sink.clone()));

    assert!(!controller.choose_mode(SessionMode::File));
    controller
        .set_pending_file(file.path.clone())
        .expect("a media path");
    assert!(controller.create_session());
    controller.start_file_session(&file.path).expect("starts");

    let commands = sink.take();
    match &commands[0] {
        SttCommand::TranscribeFile { language, .. } => {
            assert_eq!(language.as_deref(), Some("fr"));
        }
        other => panic!("expected a transcribeFile command, got {other:?}"),
    }
}

#[test]
fn the_clean_transcript_command_names_the_take_provider_model_and_text() {
    let mut controller = finished_live_with("d une dict ée");
    controller.apply_settings(Settings {
        language_model: "openai/gpt-4.1-mini".to_owned(),
        language: "fr".to_owned(),
        auto_paste: false,
        ..keyed_settings(DEFAULTS.0, DEFAULTS.1)
    });

    let request_id = controller.begin_clean_transcript().expect("a request id");
    let command = controller
        .clean_transcript_command(&request_id)
        .expect("a command");

    match command {
        SttCommand::CleanTranscript {
            request_id: id,
            provider,
            model,
            text,
            language,
            api_key,
        } => {
            assert_eq!(id, request_id);
            assert_eq!(provider, "vercel-ai-gateway");
            assert_eq!(model, "openai/gpt-4.1-mini");
            assert_eq!(text, "d une dict ée", "the text on screen is what travels");
            assert_eq!(language.as_deref(), Some("fr"));
            assert_eq!(
                api_key.as_ref().map(SecretString::expose_secret),
                Some("sk-clean-test")
            );
        }
        other => panic!("expected a cleanTranscript command, got {other:?}"),
    }
}

#[test]
fn a_cleanup_without_a_provider_key_sends_nothing() {
    let mut controller = finished_live_with("d une dict ée");
    controller.apply_settings(Settings {
        language_model: "openai/gpt-4.1-mini".to_owned(),
        ..configured_settings(DEFAULTS.0, DEFAULTS.1)
    });

    let request_id = controller.begin_clean_transcript().expect("a request id");

    assert!(
        controller.clean_transcript_command(&request_id).is_none(),
        "a request the provider would reject is never sent"
    );
    assert!(controller.clean_is_running());
    assert!(controller.fail_clean_transcript(
        &request_id,
        AppError::MissingApiKey {
            provider: "vercel-ai-gateway".into()
        }
    ));
    assert_eq!(controller.clean_status(), CleanStatus::Ready);
}
