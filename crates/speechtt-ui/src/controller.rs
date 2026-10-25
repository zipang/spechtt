//! Pure view-model for the widget: session workflow, setup draft, screen
//! selection and live take telemetry, with no GPUI `Window`/`Context`
//! dependency.
//!
//! Keeping this logic free of rendering concerns means the whole session flow
//! can be exercised by ordinary `#[test]` unit tests. `app::SpeechTtApp` owns a
//! [`SessionController`] and only translates its state into elements.
//!
//! The controller is also the duplex boundary: it emits [`SttCommand`]s to an
//! optional [`SttSessionSink`] and folds inbound [`SttEvent`]s into a
//! [`TranscriptBuffer`], so `CaptionWell`, `Waveform` and `ElapsedTimer` read
//! live state rather than mock constants.

use std::fmt;
use std::sync::Arc;

use gpui_kit::SharedString;
use speechtt_core::backend::{SttEventReceiver, SttSessionSink};
use speechtt_core::protocol::{SttCommand, SttEvent};
use speechtt_core::session::{
    FinishedSession, OutputTarget, SessionError, SessionField, SessionMachine, SessionState,
};
use speechtt_core::transcript::TranscriptBuffer;

/// Mock audio source until device enumeration lands (Phase 3).
pub const DEFAULT_AUDIO_SOURCE: &str = "System default microphone";

/// PCM sample rate sent to the STT backend (16 kHz mono, see `PRODUCT.md` §5).
pub const SAMPLE_RATE: u32 = 16_000;

/// Bars in the idle waveform row (matches the 480×280 mockups).
pub const WAVEFORM_BARS: usize = 36;

/// Reference elapsed read-out used by the static mockups.
pub const MOCK_ELAPSED_SECONDS: u32 = 42;
/// Reference FFT frame used by the static mockups.
pub const MOCK_BARS: [f32; 36] = [
    0.28, 0.55, 0.34, 0.60, 0.78, 0.41, 0.52, 0.33, 0.72, 0.95, 0.85, 0.48, 0.30, 0.42, 0.66, 0.87,
    0.94, 0.58, 0.36, 0.26, 0.47, 0.70, 0.89, 0.81, 0.44, 0.32, 0.56, 0.75, 0.90, 0.63, 0.39, 0.29,
    0.50, 0.68, 0.46, 0.24,
];
/// Offline demo transcript shown on the stopped screen when no STT backend is
/// connected (Phases 3–6 replace it with the live [`TranscriptBuffer`]).
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

/// A draft whose required fields all resolved to real values. Only
/// constructible through [`SessionDraft::validate`], so committing a session
/// cannot hit a partially-filled draft.
#[derive(Debug)]
struct ValidatedDraft {
    provider: &'static str,
    model: &'static str,
    session_name: String,
    output: OutputTarget,
}

impl SessionDraft {
    fn validate(&self) -> Option<ValidatedDraft> {
        let provider = PROVIDERS.get(self.provider?)?;
        let model = *provider.models.get(self.model?)?;
        let output = self.output.clone()?;
        let session_name = self.session_name.trim().to_string();
        if session_name.is_empty() {
            return None;
        }
        Some(ValidatedDraft {
            provider: provider.name,
            model,
            session_name,
            output,
        })
    }
}

/// The finished take shown in the `Stopped` screen. The machine hands the
/// session identity back on `stop`; the transcript is the live buffer at that
/// moment and is refined by the final `stopped` event.
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

/// Owns the session machine, the setup draft, the stopped-screen result and
/// the live take telemetry.
pub struct SessionController {
    machine: SessionMachine,
    screen: Screen,
    draft: SessionDraft,
    result: Option<FinishedTake>,
    /// Monotonic take counter; the wire id is `take-{session_id}`.
    session_id: u64,
    transcript: TranscriptBuffer,
    waveform_bars: Arc<[f32]>,
    elapsed_seconds: u32,
    last_error: Option<SharedString>,
    sink: Option<Box<dyn SttSessionSink>>,
    events: Option<SttEventReceiver>,
}

impl fmt::Debug for SessionController {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SessionController")
            .field("machine", &self.machine)
            .field("screen", &self.screen)
            .field("draft", &self.draft)
            .field("result", &self.result)
            .field("session_id", &self.session_id)
            .field("transcript", &self.transcript)
            .field("waveform_bars", &self.waveform_bars)
            .field("elapsed_seconds", &self.elapsed_seconds)
            .field("last_error", &self.last_error)
            .field("has_sink", &self.sink.is_some())
            .field("has_event_stream", &self.events.is_some())
            .finish()
    }
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
            session_id: 0,
            transcript: TranscriptBuffer::default(),
            waveform_bars: idle_waveform(),
            elapsed_seconds: 0,
            last_error: None,
            sink: None,
            events: None,
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

    /// The current (or most recent) take counter.
    #[must_use]
    pub fn session_id(&self) -> u64 {
        self.session_id
    }

    /// The wire-level session id, e.g. `"take-1"`. Empty before the first take.
    #[must_use]
    pub fn session_label(&self) -> String {
        if self.session_id == 0 {
            String::new()
        } else {
            format!("take-{}", self.session_id)
        }
    }

    /// Finalized transcript text, read by `CaptionWell`.
    #[must_use]
    pub fn transcript_committed(&self) -> &str {
        self.transcript.committed()
    }

    /// In-progress live tail, read by `CaptionWell`.
    #[must_use]
    pub fn transcript_live(&self) -> &str {
        self.transcript.live()
    }

    /// Committed plus live text, for copy/save actions.
    #[must_use]
    pub fn transcript_full_text(&self) -> String {
        self.transcript.full_text()
    }

    /// The latest FFT frame, shared with the renderer without copying.
    #[must_use]
    pub fn waveform_bars(&self) -> Arc<[f32]> {
        Arc::clone(&self.waveform_bars)
    }

    /// Seconds elapsed in the current (or most recent) take.
    #[must_use]
    pub fn elapsed_seconds(&self) -> u32 {
        self.elapsed_seconds
    }

    /// The last backend/protocol failure, shown until the next take.
    #[must_use]
    pub fn last_error(&self) -> Option<&SharedString> {
        self.last_error.as_ref()
    }

    pub fn is_draft_complete(&self) -> bool {
        self.is_draft_complete_with_name(&self.draft.session_name)
    }

    /// Completeness check against a name supplied by the view's input entity.
    /// The UI reads the editable input directly; the controller may not have
    /// received a change event yet.
    pub fn is_draft_complete_with_name(&self, session_name: &str) -> bool {
        self.draft.provider.is_some()
            && self.draft.model.is_some()
            && self.draft.output.is_some()
            && !session_name.trim().is_empty()
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

    // ----- Backend wiring ----------------------------------------------

    /// Attach the duplex backend. Its event stream (if any) is exposed by
    /// [`SessionController::take_event_stream`] for the view's receiver loop.
    pub fn set_sink(&mut self, sink: Box<dyn SttSessionSink>) {
        self.events = sink.events();
        self.sink = Some(sink);
    }

    /// Take ownership of the backend event stream, if one is attached.
    pub fn take_event_stream(&mut self) -> Option<SttEventReceiver> {
        self.events.take()
    }

    /// Record a transport failure (e.g. the worker exited) for the UI.
    pub fn report_backend_error(&mut self, error: impl fmt::Display) {
        self.last_error = Some(error.to_string().into());
    }

    /// Fold one inbound backend event into live take state.
    ///
    /// Events from a previous take are ignored so a late packet cannot corrupt
    /// the current one. `Ready` and session-less errors are global.
    pub fn handle_stt_event(&mut self, event: SttEvent) {
        if !self.accepts_session(event.session_id()) {
            return;
        }

        if let SttEvent::Error { code, message, .. } = &event {
            self.last_error = Some(format!("{code}: {message}").into());
            return;
        }

        self.transcript.apply(&event);

        // A `stopped` flush usually lands just after the stopped screen is
        // already visible: refine it in place.
        if matches!(event, SttEvent::Stopped { .. })
            && let Some(result) = &mut self.result
        {
            result.committed = self.transcript.committed().into();
            result.live = self.transcript.live().into();
        }
    }

    /// Drive the take clock from the capture pipeline.
    pub fn set_elapsed_seconds(&mut self, seconds: u32) {
        if self.machine.state().is_active() {
            self.elapsed_seconds = seconds;
        }
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
        let Some(draft) = self.draft.validate() else {
            return false;
        };

        // No file picker yet: the default location follows the title.
        let output = match draft.output {
            OutputTarget::Memory => OutputTarget::Memory,
            OutputTarget::File(_) => {
                OutputTarget::File(format!("~/Documents/{}.md", draft.session_name))
            }
        };

        let setup = (|| -> Result<(), SessionError> {
            self.machine.set_provider(draft.provider)?;
            self.machine.set_model(draft.model)?;
            self.machine.set_session_name(draft.session_name)?;
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

    /// Begins a take: reset telemetry, mint the session id and tell the
    /// backend to open a stream.
    pub fn start_recording(&mut self) -> bool {
        if self.machine.start().is_err() {
            return false;
        }

        self.session_id += 1;
        self.transcript.clear();
        self.waveform_bars = idle_waveform();
        self.elapsed_seconds = 0;
        self.last_error = None;

        let config = self.machine.config();
        self.send_command(SttCommand::Start {
            session_id: self.session_label(),
            provider: config.provider.clone().unwrap_or_default(),
            model: config.model.clone().unwrap_or_default(),
            sample_rate: SAMPLE_RATE,
            // No settings/key store yet (Phase 5); the worker falls back to
            // provider environment variables when this is absent.
            api_key: None,
        });

        true
    }

    /// Forward one capture chunk and its FFT frame. Ignored unless the take is
    /// actively recording, so paused audio never reaches the provider.
    pub fn push_audio_chunk(&mut self, pcm16: &[i16], bars: Arc<[f32]>) {
        if self.machine.state() != SessionState::Recording {
            return;
        }
        self.waveform_bars = bars;
        self.send_command(SttCommand::audio_from_pcm16(pcm16));
    }

    pub fn pause_recording(&mut self) -> bool {
        if self.machine.pause().is_err() {
            return false;
        }
        self.send_command(SttCommand::Pause);
        true
    }

    pub fn resume_recording(&mut self) -> bool {
        if self.machine.resume().is_err() {
            return false;
        }
        self.send_command(SttCommand::Resume);
        true
    }

    /// Ends the take, asks the backend to flush, and remembers the transcript
    /// for the `Stopped` screen.
    pub fn finish_take(&mut self) -> bool {
        let Ok(session) = self.machine.stop() else {
            return false;
        };
        self.send_command(SttCommand::Stop);

        let (committed, live) = self.finished_transcript();
        self.result = Some(FinishedTake {
            session,
            committed: committed.into(),
            live: live.into(),
        });
        true
    }

    /// Leave the `Stopped` screen and start over from `Blank`.
    pub fn new_take(&mut self) {
        self.result = None;
        self.screen = Screen::Prompt;
        self.draft = SessionDraft::default();
        self.transcript.clear();
        self.waveform_bars = idle_waveform();
        self.elapsed_seconds = 0;
        self.last_error = None;
    }

    // ----- Internals ---------------------------------------------------

    fn accepts_session(&self, session_id: Option<&str>) -> bool {
        match session_id {
            None => true,
            Some(id) => self.session_id > 0 && id == self.session_label(),
        }
    }

    fn send_command(&mut self, command: SttCommand) {
        let Some(sink) = self.sink.as_mut() else {
            return;
        };
        if let Err(error) = sink.send(command) {
            self.last_error = Some(error.to_string().into());
        }
    }

    /// Transcript for the stopped screen. With no backend attached the widget
    /// is an offline demo, so it keeps the mock take it showed before T0002.
    fn finished_transcript(&self) -> (String, String) {
        if self.sink.is_none() && self.transcript.full_text().is_empty() {
            (MOCK_COMMITTED.to_string(), MOCK_LIVE.to_string())
        } else {
            (
                self.transcript.committed().to_string(),
                self.transcript.live().to_string(),
            )
        }
    }
}

fn idle_waveform() -> Arc<[f32]> {
    vec![0.0; WAVEFORM_BARS].into()
}
