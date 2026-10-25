//! Pure view-model for the widget: session workflow, setup draft and screen
//! selection, with no GPUI `Window`/`Context` dependency.
//!
//! Keeping this logic free of rendering concerns means the whole session flow
//! can be exercised by ordinary `#[test]` unit tests. `app::SpeechTtApp` owns a
//! [`SessionController`] and only translates its state into elements.

use gpui_kit::SharedString;
use speechtt_core::session::{
    FinishedSession, OutputTarget, SessionError, SessionField, SessionMachine, SessionState,
};

/// Mock audio source until device enumeration lands (Phase 3).
pub const DEFAULT_AUDIO_SOURCE: &str = "System default microphone";

/// Mock feed until the core (Phase 3/8) is wired: elapsed time, FFT-shaped
/// bars and transcript lines mirroring `recording.png` / `paused.png`.
pub const MOCK_ELAPSED_SECONDS: u32 = 42;
pub const MOCK_BARS: [f32; 36] = [
    0.28, 0.55, 0.34, 0.60, 0.78, 0.41, 0.52, 0.33, 0.72, 0.95, 0.85, 0.48, 0.30, 0.42, 0.66, 0.87,
    0.94, 0.58, 0.36, 0.26, 0.47, 0.70, 0.89, 0.81, 0.44, 0.32, 0.56, 0.75, 0.90, 0.63, 0.39, 0.29,
    0.50, 0.68, 0.46, 0.24,
];
pub const MOCK_COMMITTED: &str =
    "...so I think the best approach here is to split the work into two phases, starting with the";
pub const MOCK_LIVE: &str = "audio capture layer";

/// Mock provider registry until the TypeScript/provider layer is wired
/// (Phases 6/8). The form cycles through these; the real Select control and
/// capability detection replace it later.
struct Provider {
    name: &'static str,
    models: &'static [&'static str],
}

const PROVIDERS: &[Provider] = &[
    Provider {
        name: "Vercel AI Gateway",
        models: &["voxtral-mini-transcribe-3", "whisper-large-v3"],
    },
    Provider {
        name: "Mistral AI",
        models: &["voxtral-mini-latest", "voxtral-small-latest"],
    },
];

/// Which setup screen the body shows while the machine is `Blank`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Screen {
    Prompt,
    Form,
}

/// What the widget is currently displaying. Derived from the machine, the
/// setup screen and the finished-take overlay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewState {
    Prompt,
    Form,
    Ready,
    Recording,
    Paused,
    Stopped,
}

/// Form values not yet committed to the machine.
#[derive(Debug, Default)]
struct SessionDraft {
    provider: Option<usize>,
    model: Option<usize>,
    output: Option<OutputTarget>,
    session_name: String,
}

/// The finished take shown in the `Stopped` screen. The machine hands the
/// session identity back on `stop`; the transcript itself is mocked here until
/// the core streams it in Phase 8.
#[derive(Debug)]
pub struct FinishedTake {
    session: FinishedSession,
    committed: SharedString,
    live: SharedString,
}

impl FinishedTake {
    pub fn session(&self) -> &FinishedSession {
        &self.session
    }

    pub fn committed(&self) -> &SharedString {
        &self.committed
    }

    pub fn live(&self) -> &SharedString {
        &self.live
    }
}

/// Owns the session machine, the setup draft and the stopped-screen result.
#[derive(Debug)]
pub struct SessionController {
    machine: SessionMachine,
    screen: Screen,
    draft: SessionDraft,
    result: Option<FinishedTake>,
}

impl Default for SessionController {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionController {
    pub fn new() -> Self {
        let mut machine = SessionMachine::new();
        // The mock "idle" frame assumes a usable default mic: no device row
        // on the setup form.
        machine
            .set_audio_source(DEFAULT_AUDIO_SOURCE)
            .expect("blank machine accepts setup");

        Self {
            machine,
            screen: Screen::Prompt,
            draft: SessionDraft::default(),
            result: None,
        }
    }

    // ----- Queries -----------------------------------------------------

    pub fn view_state(&self) -> ViewState {
        if self.result.is_some() {
            return ViewState::Stopped;
        }
        match (self.machine.state(), self.screen) {
            (SessionState::Blank, Screen::Prompt) => ViewState::Prompt,
            (SessionState::Blank, Screen::Form) => ViewState::Form,
            (SessionState::Ready, _) => ViewState::Ready,
            (SessionState::Recording, _) => ViewState::Recording,
            (SessionState::Paused, _) => ViewState::Paused,
        }
    }

    pub fn session_state(&self) -> SessionState {
        self.machine.state()
    }

    pub fn session_missing(&self) -> Vec<SessionField> {
        self.machine.missing()
    }

    pub fn is_draft_complete(&self) -> bool {
        self.draft.provider.is_some()
            && self.draft.model.is_some()
            && self.draft.output.is_some()
            && !self.draft.session_name.trim().is_empty()
    }

    pub fn finished_take(&self) -> Option<&FinishedTake> {
        self.result.as_ref()
    }

    pub fn has_provider(&self) -> bool {
        self.draft.provider.is_some()
    }

    pub fn has_model(&self) -> bool {
        self.draft.model.is_some()
    }

    pub fn provider_label(&self) -> Option<&'static str> {
        self.draft.provider.map(|index| PROVIDERS[index].name)
    }

    pub fn model_label(&self) -> Option<&'static str> {
        let provider = self.draft.provider?;
        let model = self.draft.model?;
        PROVIDERS[provider].models.get(model).copied()
    }

    pub fn selected_output(&self) -> Option<&OutputTarget> {
        self.draft.output.as_ref()
    }

    // ----- Commands ----------------------------------------------------

    pub fn open_form(&mut self) {
        self.screen = Screen::Form;
        self.draft = SessionDraft::default();
    }

    pub fn cancel_form(&mut self) {
        self.screen = Screen::Prompt;
        self.draft = SessionDraft::default();
    }

    pub fn cycle_provider(&mut self) {
        let next = self
            .draft
            .provider
            .map_or(0, |index| (index + 1) % PROVIDERS.len());
        self.draft.provider = Some(next);
        // A model belongs to its provider: reset the dependent choice.
        self.draft.model = None;
    }

    pub fn cycle_model(&mut self) {
        let Some(provider) = self.draft.provider else {
            return;
        };
        let models = PROVIDERS[provider].models;
        self.draft.model = Some(
            self.draft
                .model
                .map_or(0, |index| (index + 1) % models.len()),
        );
    }

    pub fn set_draft_name(&mut self, value: impl Into<String>) {
        self.draft.session_name = value.into();
    }

    pub fn set_draft_output(&mut self, output: OutputTarget) {
        self.draft.output = Some(output);
    }

    /// Commit the draft to the machine. Returns `false` (and changes nothing)
    /// while the draft is incomplete or the machine refuses a setup change.
    pub fn create_session(&mut self) -> bool {
        let Some(provider_ix) = self.draft.provider else {
            return false;
        };
        let Some(model_ix) = self.draft.model else {
            return false;
        };
        let Some(output) = self.draft.output.clone() else {
            return false;
        };
        let name = self.draft.session_name.trim().to_string();
        if name.is_empty() {
            return false;
        }

        let provider = &PROVIDERS[provider_ix];
        let Some(model) = provider.models.get(model_ix) else {
            return false;
        };
        // No file picker yet: the default location follows the title.
        let output = match output {
            OutputTarget::Memory => OutputTarget::Memory,
            OutputTarget::File(_) => OutputTarget::File(format!("~/Documents/{name}.md")),
        };

        let setup = (|| -> Result<(), SessionError> {
            self.machine.set_provider(provider.name)?;
            self.machine.set_model(*model)?;
            self.machine.set_session_name(name)?;
            self.machine.set_output(output)?;
            Ok(())
        })();
        if setup.is_err() {
            return false;
        }

        self.screen = Screen::Prompt;
        self.draft = SessionDraft::default();
        true
    }

    pub fn start_recording(&mut self) -> bool {
        self.machine.start().is_ok()
    }

    pub fn pause_recording(&mut self) -> bool {
        self.machine.pause().is_ok()
    }

    pub fn resume_recording(&mut self) -> bool {
        self.machine.resume().is_ok()
    }

    /// Ends the take and remembers it for the `Stopped` screen.
    pub fn finish_take(&mut self) -> bool {
        let Ok(session) = self.machine.stop() else {
            return false;
        };
        self.result = Some(FinishedTake {
            session,
            committed: MOCK_COMMITTED.into(),
            live: MOCK_LIVE.into(),
        });
        true
    }

    /// Leave the `Stopped` screen and start over from `Blank`.
    pub fn new_take(&mut self) {
        self.result = None;
        self.screen = Screen::Prompt;
        self.draft = SessionDraft::default();
    }
}
