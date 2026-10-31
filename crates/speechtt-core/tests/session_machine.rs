//! Session state machine contract (TDD red phase).
//!
//! Rules locked in with the product owner:
//! - Three required session parameters: audio source, provider, model. All must
//!   be present for `Ready`. There is no session name and no output target: the
//!   file the user saves gives the take its name.
//! - Setup changes are frozen while Recording/Paused.
//! - Stop returns the finished session to the caller, then clears the whole
//!   configuration and returns to `Blank` (the next take starts from the mode
//!   chooser, which re-derives everything).
//! - `reset()` backs "change mode": idle only, clears the configuration.

use speechtt_core::session::{
    FinishedSession, SessionError, SessionField, SessionMachine, SessionState,
};

fn full_config(machine: &mut SessionMachine) {
    machine.set_audio_source("Built-in Microphone").unwrap();
    machine.set_provider("mistral").unwrap();
    machine.set_model("voxtral-mini-latest").unwrap();
}

#[test]
fn starts_blank_with_nothing_configured() {
    let machine = SessionMachine::new();

    assert_eq!(machine.state(), SessionState::Blank);
    assert_eq!(
        machine.missing(),
        vec![
            SessionField::AudioSource,
            SessionField::Provider,
            SessionField::Model,
        ]
    );
}

#[test]
fn each_parameter_alone_leaves_the_machine_blank() {
    let mut machine = SessionMachine::new();

    machine.set_audio_source("Built-in Microphone").unwrap();
    assert_eq!(machine.state(), SessionState::Blank);
    assert_eq!(machine.missing().len(), 2);

    machine.set_provider("mistral").unwrap();
    assert_eq!(machine.state(), SessionState::Blank);
    assert_eq!(machine.missing().len(), 1);

    machine.set_model("voxtral-mini-latest").unwrap();
    assert_eq!(machine.state(), SessionState::Ready);
    assert!(machine.missing().is_empty());
}

#[test]
fn whitespace_only_values_do_not_count_as_set() {
    let mut machine = SessionMachine::new();
    full_config(&mut machine);
    assert_eq!(machine.state(), SessionState::Ready);

    machine.set_provider("   ").unwrap();
    assert_eq!(machine.state(), SessionState::Blank);
    assert_eq!(machine.missing(), vec![SessionField::Provider]);
}

#[test]
fn removing_a_parameter_returns_ready_to_blank() {
    let mut machine = SessionMachine::new();
    full_config(&mut machine);
    assert_eq!(machine.state(), SessionState::Ready);

    machine.clear(SessionField::Provider).unwrap();
    assert_eq!(machine.state(), SessionState::Blank);
    assert_eq!(machine.missing(), vec![SessionField::Provider]);
}

#[test]
fn start_is_rejected_while_blank_and_reports_missing_fields() {
    let mut machine = SessionMachine::new();
    machine.set_provider("mistral").unwrap();

    let error = machine.start().unwrap_err();
    match error {
        SessionError::NotReady { missing } => {
            assert_eq!(
                missing,
                vec![SessionField::AudioSource, SessionField::Model]
            );
        }
        other => panic!("expected NotReady, got {other:?}"),
    }
    assert_eq!(machine.state(), SessionState::Blank);
}

#[test]
fn ready_start_moves_to_recording_then_pause_and_resume() {
    let mut machine = SessionMachine::new();
    full_config(&mut machine);

    machine.start().unwrap();
    assert_eq!(machine.state(), SessionState::Recording);

    machine.pause().unwrap();
    assert_eq!(machine.state(), SessionState::Paused);

    machine.resume().unwrap();
    assert_eq!(machine.state(), SessionState::Recording);
}

#[test]
fn transport_actions_are_rejected_in_the_wrong_state() {
    let mut machine = SessionMachine::new();

    // Blank: nothing can start/pause/resume/stop.
    assert!(machine.pause().is_err());
    assert!(machine.resume().is_err());
    assert!(machine.stop().is_err());

    full_config(&mut machine);
    // Ready: pause/resume/stop rejected.
    assert!(machine.pause().is_err());
    assert!(machine.resume().is_err());
    assert!(machine.stop().is_err());

    machine.start().unwrap();
    // Recording: start/resume rejected.
    assert!(machine.start().is_err());
    assert!(machine.resume().is_err());

    machine.pause().unwrap();
    // Paused: start/pause rejected.
    assert!(machine.start().is_err());
    assert!(machine.pause().is_err());
    assert_eq!(machine.state(), SessionState::Paused);
}

#[test]
fn setup_changes_are_frozen_while_recording_or_paused() {
    let mut machine = SessionMachine::new();
    full_config(&mut machine);
    machine.start().unwrap();

    assert!(matches!(
        machine.set_provider("vercel"),
        Err(SessionError::Frozen { .. })
    ));
    assert!(matches!(
        machine.clear(SessionField::Model),
        Err(SessionError::Frozen { .. })
    ));
    // Frozen means untouched, not partially applied.
    assert_eq!(machine.config().provider, Some("mistral".to_string()));

    machine.pause().unwrap();
    assert!(matches!(
        machine.set_model("whisper"),
        Err(SessionError::Frozen { .. })
    ));
    assert!(matches!(
        machine.reset(),
        Err(SessionError::InvalidTransition { .. })
    ));
}

#[test]
fn stopping_returns_the_finished_session_and_clears_the_configuration() {
    let mut machine = SessionMachine::new();
    full_config(&mut machine);
    machine.start().unwrap();
    machine.pause().unwrap();

    let finished: FinishedSession = machine.stop().unwrap();

    // The caller receives everything it needs to save / paste the take.
    assert_eq!(finished.audio_source, "Built-in Microphone");
    assert_eq!(finished.provider, "mistral");
    assert_eq!(finished.model, "voxtral-mini-latest");

    // Next take starts from the mode chooser: nothing is remembered.
    assert_eq!(machine.state(), SessionState::Blank);
    assert_eq!(machine.config().audio_source, None);
    assert_eq!(machine.config().provider, None);
    assert_eq!(machine.config().model, None);
    assert_eq!(
        machine.missing(),
        vec![
            SessionField::AudioSource,
            SessionField::Provider,
            SessionField::Model,
        ]
    );
}

#[test]
fn reset_clears_an_idle_machine_and_refuses_while_active() {
    let mut machine = SessionMachine::new();
    full_config(&mut machine);
    assert_eq!(machine.state(), SessionState::Ready);

    machine.reset().unwrap();
    assert_eq!(machine.state(), SessionState::Blank);
    assert_eq!(machine.config().provider, None);

    // Resetting a blank machine is a no-op, not an error.
    machine.reset().unwrap();
    assert_eq!(machine.state(), SessionState::Blank);

    full_config(&mut machine);
    machine.start().unwrap();
    assert!(matches!(
        machine.reset(),
        Err(SessionError::InvalidTransition {
            from: SessionState::Recording
        })
    ));
    // The refusal left the take untouched.
    assert_eq!(machine.state(), SessionState::Recording);
    assert_eq!(
        machine.config().model.as_deref(),
        Some("voxtral-mini-latest")
    );

    machine.pause().unwrap();
    assert!(matches!(
        machine.reset(),
        Err(SessionError::InvalidTransition {
            from: SessionState::Paused
        })
    ));
}

#[test]
fn session_errors_render_a_message_and_implement_std_error() {
    let mut machine = SessionMachine::new();
    let error = machine.start().unwrap_err();
    assert!(error.to_string().contains("not ready"));
    let source = std::error::Error::source(&error);
    assert!(source.is_none());

    machine.set_audio_source("Built-in Microphone").unwrap();
    assert!(
        machine
            .pause()
            .unwrap_err()
            .to_string()
            .contains("not valid")
    );
}

#[test]
fn full_happy_path_blank_to_ready_to_recording_to_blank_to_ready() {
    let mut machine = SessionMachine::new();
    assert_eq!(machine.state(), SessionState::Blank);

    full_config(&mut machine);
    assert_eq!(machine.state(), SessionState::Ready);

    machine.start().unwrap();
    assert_eq!(machine.state(), SessionState::Recording);
    machine.pause().unwrap();
    assert_eq!(machine.state(), SessionState::Paused);
    machine.resume().unwrap();
    assert_eq!(machine.state(), SessionState::Recording);

    let finished = machine.stop().unwrap();
    assert_eq!(finished.provider, "mistral");
    assert_eq!(machine.state(), SessionState::Blank);
    assert_eq!(machine.missing().len(), 3);

    // The retake re-derives the three parameters from scratch.
    full_config(&mut machine);
    assert_eq!(machine.state(), SessionState::Ready);
}

// ----- Discarding a take ----------------------------------------------

#[test]
fn discard_take_drops_the_take_but_keeps_the_configuration() {
    let mut machine = SessionMachine::new();
    full_config(&mut machine);
    machine.start().unwrap();
    machine.pause().unwrap();

    machine.discard_take().unwrap();

    // The configuration survives: the next press of the same control starts a
    // fresh take without going through the form again.
    assert_eq!(machine.state(), SessionState::Ready);
    assert_eq!(
        machine.config().provider.as_deref(),
        Some("mistral"),
        "provider kept"
    );
    assert_eq!(
        machine.config().model.as_deref(),
        Some("voxtral-mini-latest"),
        "model kept"
    );
    assert_eq!(
        machine.config().audio_source.as_deref(),
        Some("Built-in Microphone"),
        "audio source kept"
    );
    assert!(machine.missing().is_empty());

    // And it records again straight away.
    machine.start().unwrap();
    assert_eq!(machine.state(), SessionState::Recording);
}

#[test]
fn discard_take_is_a_no_op_when_no_take_is_open() {
    let mut machine = SessionMachine::new();
    machine.discard_take().unwrap();
    assert_eq!(machine.state(), SessionState::Blank);

    full_config(&mut machine);
    assert_eq!(machine.state(), SessionState::Ready);
    machine.discard_take().unwrap();
    assert_eq!(machine.state(), SessionState::Ready, "still configured");
    assert!(machine.missing().is_empty());
}
