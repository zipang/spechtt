//! Session state machine contract (TDD red phase).
//!
//! Rules locked in with the product owner:
//! - Five required session parameters: audio source, provider, model,
//!   session name, output target. All must be present for `Ready`.
//! - Setup changes are frozen while Recording/Paused.
//! - Stop returns the finished session to the caller, then resets to
//!   `Blank`, keeping audio source / provider / model, clearing session
//!   name and output (the next take requires naming again).

use speechtt_core::session::{
    FinishedSession, OutputTarget, SessionError, SessionField, SessionMachine, SessionState,
};

fn full_config(machine: &mut SessionMachine) {
    machine.set_audio_source("Built-in Microphone").unwrap();
    machine.set_provider("mistral").unwrap();
    machine.set_model("voxtral-mini-latest").unwrap();
    machine.set_session_name("meeting-notes").unwrap();
    machine.set_output(OutputTarget::Memory).unwrap();
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
            SessionField::SessionName,
            SessionField::Output,
        ]
    );
}

#[test]
fn each_parameter_alone_leaves_the_machine_blank() {
    let mut machine = SessionMachine::new();

    machine.set_audio_source("Built-in Microphone").unwrap();
    assert_eq!(machine.state(), SessionState::Blank);
    assert_eq!(machine.missing().len(), 4);

    machine.set_provider("mistral").unwrap();
    assert_eq!(machine.state(), SessionState::Blank);

    machine.set_model("voxtral-mini-latest").unwrap();
    assert_eq!(machine.state(), SessionState::Blank);

    machine.set_session_name("meeting-notes").unwrap();
    assert_eq!(machine.state(), SessionState::Blank);

    machine.set_output(OutputTarget::Memory).unwrap();
    assert_eq!(machine.state(), SessionState::Ready);
    assert!(machine.missing().is_empty());
}

#[test]
fn an_empty_session_name_does_not_count_as_set() {
    let mut machine = SessionMachine::new();
    full_config(&mut machine);
    assert_eq!(machine.state(), SessionState::Ready);

    machine.set_session_name("   ").unwrap();
    assert_eq!(machine.state(), SessionState::Blank);
    assert_eq!(machine.missing(), vec![SessionField::SessionName]);
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
                vec![
                    SessionField::AudioSource,
                    SessionField::Model,
                    SessionField::SessionName,
                    SessionField::Output,
                ]
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
        machine.set_output(OutputTarget::File("/tmp/take.md".into())),
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
        machine.set_session_name("renamed"),
        Err(SessionError::Frozen { .. })
    ));
}

#[test]
fn stopping_returns_the_finished_session_and_resets_to_blank() {
    let mut machine = SessionMachine::new();
    full_config(&mut machine);
    machine
        .set_output(OutputTarget::File("/tmp/meeting-notes.md".into()))
        .unwrap();
    machine.start().unwrap();
    machine.pause().unwrap();

    let finished: FinishedSession = machine.stop().unwrap();

    // The caller receives everything it needs to save / paste / label.
    assert_eq!(finished.audio_source, "Built-in Microphone");
    assert_eq!(finished.provider, "mistral");
    assert_eq!(finished.model, "voxtral-mini-latest");
    assert_eq!(finished.session_name, "meeting-notes");
    assert_eq!(
        finished.output,
        OutputTarget::File("/tmp/meeting-notes.md".into())
    );

    // Next take: identity is remembered, name and output are not.
    assert_eq!(machine.state(), SessionState::Blank);
    assert_eq!(
        machine.config().audio_source.as_deref(),
        Some("Built-in Microphone")
    );
    assert_eq!(machine.config().provider.as_deref(), Some("mistral"));
    assert_eq!(
        machine.config().model.as_deref(),
        Some("voxtral-mini-latest")
    );
    assert_eq!(machine.config().session_name, None);
    assert_eq!(machine.config().output, None);
    assert_eq!(
        machine.missing(),
        vec![SessionField::SessionName, SessionField::Output]
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
    assert_eq!(finished.session_name, "meeting-notes");
    assert_eq!(machine.state(), SessionState::Blank);

    // Retake needs only name + output: provider/model/source persisted.
    machine.set_session_name("meeting-notes-2").unwrap();
    assert_eq!(machine.state(), SessionState::Blank);
    machine.set_output(OutputTarget::Memory).unwrap();
    assert_eq!(machine.state(), SessionState::Ready);
}
