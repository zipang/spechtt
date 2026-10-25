//! Session setup and transport flow — pure view-model tests.
//!
//! These tests drive [`SessionController`] directly. They never open a window,
//! click elements or assert what the UI renders, so the widget screens remain
//! free to be redesigned (elements moved, merged, replaced) without breaking
//! the workflow contract. Rendering is covered only by the smoke test in
//! `tests/widget.rs`.

use speechtt_core::session::{OutputTarget, SessionField, SessionState};
use speechtt_ui::controller::{SessionController, ViewState};

/// A controller whose draft is complete except for `create_session`.
fn complete_draft(output: OutputTarget) -> SessionController {
    let mut controller = SessionController::new();
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
    controller.open_form();

    assert_eq!(controller.view_state(), ViewState::Form);
    assert!(!controller.is_draft_complete());
    assert_eq!(controller.session_state(), SessionState::Blank);
}

#[test]
fn cancelling_the_form_returns_to_the_prompt_and_discards_the_draft() {
    let mut controller = SessionController::new();
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
