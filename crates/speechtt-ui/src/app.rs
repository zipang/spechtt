//! The SpeechTT widget window view.
//!
//! Phase 2 shell: header + body views derived from the session controller.
//!
//! ```text
//! Mode → (setup form when needed) → Live take (idle ⇄ recording ⇄ paused)
//!   ↑                                    │
//!   │                                 Session end ← Stop
//!   │
//!   └── (file) → Transcription (decoding → Continue) → Session end
//! ```
//!
//! This module owns the window shell, the workflow state, the window-level
//! commands and the public command/query surface. Screen composition lives in
//! [`crate::views`] and reusable controls in [`crate::components`].
//!
//! # Who uses this
//!
//! `lib.rs::run` opens the widget window with [`SpeechTtApp::new`], the headless
//! fixtures build the same view through [`SpeechTtApp::with_services`], and
//! every screen under [`crate::views`] reads it through the query and command
//! methods below. The transcript's context menu
//! ([`crate::components::CaptionWell`]) dispatches [`CopySelection`], which this
//! module binds and handles.
//!
//! # Commands
//!
//! Every keyboard command is declared here as one GPUI action, so the key
//! ([`crate::shortcuts`]), the button on a screen and — for the two dialogs —
//! the dialog window reach the same handler. A command exists only in the state
//! where it can act: the window root declares the state fact from the session
//! machine every frame, and each binding names it in its key-context predicate.
//! Handlers run [`SpeechTtApp::apply_action`] and call `cx.propagate()` when the
//! command does not apply in the current state, which lets action dispatch
//! continue to the next candidate for that keystroke.
//!
//! Inside the widget window a focused control keeps its own Space and Enter
//! through binding precedence, not through propagation: a component declares a
//! key context deeper on the focused path, and the documented context-depth
//! rule ranks its binding ahead of the window's. That is what leaves a focused
//! control's activation and the editor's own keys untouched there.
//!
//! In a dialog window nothing is outranked, because the take commands are not
//! bound in that window at all: its root declares [`SPEECHTT_DIALOG_CONTEXT`]
//! and no `SpeechTT` fact, so the take predicates cannot match, and
//! `DialogWindow` registers no take handler. Space and Enter are the API-key
//! field's and the buttons' own keys there, not a window command that yields.

use gpui_kit::assets::IconName;
use gpui_kit::base::{TextSelection, h_flex, v_flex};
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::button::ButtonVariants as _;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::select::{SearchableVec, SelectEvent};
use gpui_kit::component::{Sizable as _, button::Button};
use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, AnyWindowHandle, App, Bounds, Context, Entity, FocusHandle, PathPromptOptions,
    Pixels, Point, ScrollHandle, Size, Subscription, Task, Window, WindowBounds, WindowId,
    WindowKind, WindowOptions, actions, div, px,
};
use speechtt_core::actions::SessionAction;
use speechtt_core::audio::{AudioError, AudioSource, CpalAudioSource, PcmChunk};
use speechtt_core::backend::{
    BackendError, BunSubprocess, SttEventReceiver, SttSessionSink, bun_candidates, resolve_bun_in,
    which_on_path,
};
use speechtt_core::fft::SpectrumAnalyzer;
use speechtt_core::output::clipboard::{ClipboardSink, SystemClipboard};
use speechtt_core::output::file::{FileOpener, SystemOpener, suggested_transcript_name};
use speechtt_core::output::paste::{PasteSink, SystemPaste};
use speechtt_core::protocol::{SttEvent, TranscriptionMode};
use speechtt_core::session::{SessionField, SessionState};
use speechtt_core::settings::{JsonFileStore, Settings, SettingsStore, WindowLayout};

use crate::components::{
    AppHeader, IconButton, Label, LiveModelField, ModelOption, ProviderOption, Ripple, RippleRing,
    SelectFields, SelectSnapshot,
};
use crate::controller::{
    AppError, BackendFailure, CleanStatus, CleanToggle, Clock, ConnectionState, Dialog,
    ErrorAction, FinishedTake, HelpTab, SAMPLE_RATE, SessionController, SessionMode,
    SettingsChange, SettingsDraft, StartupSettingsAction, SystemClock, ViewState,
};
use crate::dialogs;
use crate::dialogs::{DIALOG_WIDTH, DialogWindow, HELP_DIALOG_HEIGHT, SETTINGS_DIALOG_HEIGHT};
use crate::tokens::color;
use crate::views;

/// Application title shown in the header strip.
pub const APP_TITLE: &str = "SpeechTT";

// The widget's window-scoped commands.
//
// Declared once each so the keybinding, the screen button and — for the two
// dialogs — the dialog window dispatch the same GPUI action and reach the same
// handler on [`SpeechTtApp`]. `actions!` generates unit structs whose
// documentation rustdoc does not render, so each command is documented on its
// binding ([`crate::shortcuts::BINDINGS`]) and on the handler that owns it.
actions!(
    speechtt,
    [
        CopySelection,
        ToggleRecording,
        StopRecording,
        ToggleHelp,
        ToggleSettings,
        SaveTranscript,
    ]
);

/// Key context of the widget window, declared on the window view's root
/// element.
///
/// A context has to be a named fact on the focused dispatch path for a
/// contextual binding to match; the copy shortcut is bound under this context
/// (in [`crate::shortcuts::bind_app_keys`]) so it outranks the generic binding
/// of the window root by the documented context-depth precedence.
///
/// On the live take and the finished transcript the root declares this context
/// together with the screen's state fact ([`SPEECHTT_TAKE_CONTEXT`],
/// [`SPEECHTT_SESSION_END_CONTEXT`]), so the `SpeechTT` fact stays on the
/// dispatch path there too.
pub const SPEECHTT_CONTEXT: &str = "SpeechTT";

/// Key context of the live take screen: the window fact plus the `Take` state
/// fact.
///
/// A key context holds a *set* of whitespace-separated identifiers, so one
/// declaration carries both facts and the keymap tests them separately as
/// `SpeechTT && Take`. This constant is the declaration string the window root
/// uses; [`crate::shortcuts::BINDINGS`] spells the predicate. Keeping the two
/// state facts named separately from [`SPEECHTT_CONTEXT`] lets the binding
/// tests compare every term of a predicate with the facts the root declares,
/// instead of matching a substring.
pub const SPEECHTT_TAKE_CONTEXT: &str = "SpeechTT Take";

/// Key context of the finished transcript screen: the window fact plus the
/// `SessionEnd` state fact.
///
/// Separate from [`SPEECHTT_CONTEXT`] for the same reason as
/// [`SPEECHTT_TAKE_CONTEXT`]: the save command is bound under
/// `SpeechTT && SessionEnd`, so the declaration and the predicate are checked
/// against each other term by term.
pub const SPEECHTT_SESSION_END_CONTEXT: &str = "SpeechTT SessionEnd";

/// Key context of a Help or Settings window, declared on its root element.
///
/// The two dialog toggles are bound under `SpeechTT || SpeechTTDialog`, so
/// **F1** and **F2** work while a dialog owns focus; without this context on the
/// dialog's root they would go dead as soon as a dialog opened. No take command
/// names it, so a dialog keeps its own Space and Enter.
pub const SPEECHTT_DIALOG_CONTEXT: &str = "SpeechTTDialog";

/// The part of a window selection that may replace the clipboard.
///
/// `None` for an empty selection and for a selection made only of whitespace,
/// so a copy with nothing selected keeps the clipboard's previous content.
/// A usable selection is returned exactly as the selection layer reported it —
/// untrimmed — because the copied text must match the highlighted run.
fn copyable_selection(selected: &str) -> Option<&str> {
    if selected.trim().is_empty() {
        None
    } else {
        Some(selected)
    }
}

/// What the platform save dialog answered.
///
/// The three outcomes are distinct on purpose: a dismissed dialog is the user's
/// decision and stays silent, while a dialog that could not open is a failure
/// the widget must report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveAnswer {
    /// The path the user chose.
    Chosen(PathBuf),
    /// The user dismissed the dialog.
    Cancelled,
    /// The platform could not open the dialog.
    Failed,
}

/// Opens the platform save dialog for the finished take.
///
/// One of the injected side effects, so the `Ctrl+S` command is headless-
/// testable: a test answers with a temporary path instead of driving an OS
/// dialog.
pub trait SavePrompt: Send {
    /// What the user did with the dialog.
    fn pick_save_path(&mut self, cx: &mut App, suggested: &str) -> Task<SaveAnswer>;
}

/// The platform save dialog, suggested inside `$HOME/Documents`.
pub struct GpuiSavePrompt;

impl SavePrompt for GpuiSavePrompt {
    fn pick_save_path(&mut self, cx: &mut App, suggested: &str) -> Task<SaveAnswer> {
        let selection = cx.prompt_for_new_path(&documents_dir(), Some(suggested));

        cx.spawn(async move |_cx| match selection.await {
            Ok(Ok(Some(path))) => SaveAnswer::Chosen(path),
            Ok(Ok(None)) => SaveAnswer::Cancelled,
            // The platform refused to open the picker: the user never got to
            // choose, so this is a failure rather than a cancellation.
            Ok(Err(_)) | Err(_) => SaveAnswer::Failed,
        })
    }
}

/// Every side effect the widget needs, injected as a single composition root.
///
/// Production is built by [`SpeechTtApp::new`]; headless tests build the same
/// struct with fakes so no microphone, display server, clipboard, editor,
/// filesystem, save dialog or live provider is required.
pub struct AppServices {
    pub audio: Box<dyn AudioSource>,
    pub analyzer: SpectrumAnalyzer,
    pub sink: Box<dyn SttSessionSink>,
    pub events: SttEventReceiver,
    pub settings: Box<dyn SettingsStore>,
    pub clipboard: Box<dyn ClipboardSink>,
    pub paste: Box<dyn PasteSink>,
    pub opener: Box<dyn FileOpener>,
    pub save_prompt: Box<dyn SavePrompt>,
    pub clock: Box<dyn Clock>,
}

type AudioFactory = Box<dyn FnMut() -> Result<Box<dyn AudioSource>, AudioError> + Send>;

/// How the widget obtains a worker process.
///
/// The production value is [`spawn_backend`], which resolves `bun` and starts
/// the real subprocess. The headless fixtures replace it with a closure that
/// returns an in-memory sink or a chosen failure, so the save → respawn path in
/// [`SpeechTtApp::save_settings`] is reachable without a `bun` installation and
/// without starting a process.
type BackendSpawn = Box<dyn Fn(&Settings) -> Result<Box<dyn SttSessionSink>, BackendFailure>>;

/// Root view of the widget window.
pub struct SpeechTtApp {
    /// The window's keyboard target.
    ///
    /// The root tracks this handle, so a pointer press anywhere in the widget —
    /// including the drag that selects a transcript range — puts the root on the
    /// focused dispatch path. That is what makes the window's key context match,
    /// and therefore what makes a window-scoped command reach its handler.
    focus: FocusHandle,
    /// The widget window itself.
    ///
    /// Dialog windows are placed beside it, and a command that arrives from a
    /// dialog runs against it: the dialog that forwarded the command may be
    /// removing itself in the same keystroke.
    shell_window: AnyWindowHandle,
    controller: SessionController,
    /// Monotonic click counters for round-button ripples, keyed by element id.
    /// `u64` because the view only needs "this changed", not a timestamp.
    /// The press ripple, drawn at the window root so it survives navigation.
    ripple: Option<Ripple>,
    ripple_seq: u64,
    settings_api_key: Entity<InputState>,
    /// The Settings dialog's `Bun path` field. Its value mirrors
    /// [`SettingsDraft::bun_executable`].
    settings_bun_path: Entity<InputState>,
    settings_key_revealed: bool,
    /// The provider and model dropdowns of the Session setup form.
    draft_selects: SelectFields,
    /// The model dropdown of the live take screen's header.
    live_model_field: LiveModelField,
    /// The open dialog window, if any. Dialogs are separate floating windows.
    dialog_window: Option<(Dialog, AnyWindowHandle)>,
    /// Mirrors the dialog window id for the window-closed observer, which runs
    /// outside this entity and must not re-enter it for our own closes.
    dialog_window_id: Rc<Cell<Option<WindowId>>>,
    pending_settings: Option<SettingsChange>,
    /// Keeps the input subscriptions alive; dropped with the view.
    _subscriptions: Vec<Subscription>,
    /// Backend event pump; dropped (and cancelled) with the view.
    _event_task: Option<Task<()>>,
    /// The process spawn behind startup and the save-triggered respawn.
    /// See [`BackendSpawn`]; [`SpeechTtApp::set_backend_spawn`] replaces it in
    /// the headless fixtures.
    backend_spawn: BackendSpawn,
    /// Whether the first catalogue can only have come from the startup spawn.
    ///
    /// Set when the startup spawn attaches a worker and cleared when a save
    /// respawns one, so the event pump does not report a recovered backend as a
    /// healthy launch.
    catalogue_from_startup: bool,
    /// Clock-driven elapsed read-out.
    _clock_task: Option<Task<()>>,
    /// Capture pump; dropped (and cancelled) with the view.
    _audio_task: Option<Task<()>>,
    audio_thread: Option<std::thread::JoinHandle<()>>,
    audio_stop: Option<Arc<AtomicBool>>,
    audio: Option<Box<dyn AudioSource>>,
    audio_factory: Option<AudioFactory>,
    analyzer: SpectrumAnalyzer,
    settings_store: Box<dyn SettingsStore>,
    clipboard: Box<dyn ClipboardSink>,
    paste: Box<dyn PasteSink>,
    opener: Box<dyn FileOpener>,
    save_prompt: Box<dyn SavePrompt>,
    /// Whether the save dialog is up. See [`SpeechTtApp::save_prompt_open`].
    save_prompt_open: bool,
    clock: Box<dyn Clock>,
    stored_settings: Settings,
    /// Geometry seen on screen, waiting for the quiet period before persisting.
    pending_window_layout: Option<(WindowLayout, Instant)>,
    /// Scroll position of the live caption well. Owned by the view so the
    /// well's follow rule survives every repaint of the take screen.
    caption_scroll: ScrollHandle,
}

impl SpeechTtApp {
    /// Production composition root: real capture, duplex Bun backend,
    /// settings file, clipboard/paste/opener and the platform save dialog.
    ///
    /// The runtime is resolved from the persisted `bun_executable` override,
    /// `PATH` and the well-known install locations, in that order. A missing
    /// `bun` never prevents the widget from opening: the failure is recorded so
    /// Settings can name it, and the dialog opens by itself when the widget
    /// cannot work without it.
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let factory: AudioFactory =
            Box::new(|| Ok(Box::new(CpalAudioSource::open_default()?) as Box<dyn AudioSource>));
        // The composition root swaps this stand-in for a real worker once the
        // settings store has answered; until then no commands leave the view.
        let (_sender, events) = speechtt_core::async_channel::unbounded();
        let services = AppServices {
            audio: Box::new(LazyCpalAudioSource::default()),
            analyzer: SpectrumAnalyzer::new(SAMPLE_RATE),
            sink: Box::new(UnavailableSink),
            events,
            settings: Box::new(JsonFileStore::new(JsonFileStore::default_path())),
            clipboard: Box::new(SystemClipboard::new()),
            paste: Box::new(SystemPaste::new()),
            opener: Box::new(SystemOpener::new()),
            save_prompt: Box::new(GpuiSavePrompt),
            clock: Box::new(SystemClock),
        };

        let mut app = Self::with_services_and_factory(
            services,
            Some(factory),
            // The startup spawn below attaches the real pump; a pump over the
            // stand-in's closed channel would report a second, less specific
            // failure over the recorded one.
            false,
            window,
            cx,
        );
        app.attach_backend_on_startup(cx);
        app.open_settings_if_startup_requires(window, cx);
        app
    }

    /// Build the widget with injected services (the test seam).
    pub fn with_services(
        services: AppServices,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::with_services_and_factory(services, None, true, window, cx)
    }

    /// Replaces the process-spawning half of the backend seam.
    ///
    /// Production installs [`spawn_backend`]. A headless fixture installs a
    /// closure that returns a sink it can inspect, or the failure the test
    /// wants to pin, so both branches of the save → respawn rule are reachable
    /// without a `bun` installation and without starting a process.
    pub fn set_backend_spawn(
        &mut self,
        spawn: impl Fn(&Settings) -> Result<Box<dyn SttSessionSink>, BackendFailure> + 'static,
    ) {
        self.backend_spawn = Box::new(spawn);
    }

    /// Shared constructor. `start_events` is `false` only for the production
    /// path, which replaces the stand-in backend before it attaches the real
    /// event pump.
    fn with_services_and_factory(
        services: AppServices,
        audio_factory: Option<AudioFactory>,
        start_events: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let settings_api_key = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Paste your provider API key")
                .masked(true)
        });
        let settings_subscription =
            cx.subscribe(&settings_api_key, |this, input, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    this.controller
                        .set_settings_api_key(input.read(cx).value().to_string());
                    cx.notify();
                }
            });

        let settings_bun_path =
            cx.new(|cx| InputState::new(window, cx).placeholder("Auto: resolved from PATH"));
        let bun_path_subscription =
            cx.subscribe(&settings_bun_path, |this, input, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    this.controller
                        .set_settings_bun_executable(input.read(cx).value().to_string());
                    cx.notify();
                }
            });

        let draft_selects = SelectFields::new(window, cx);
        let provider_subscription = cx.subscribe(
            draft_selects.provider(),
            |this, _, event: &SelectEvent<SearchableVec<ProviderOption>>, cx| {
                if let SelectEvent::Confirm(Some(id)) = event {
                    this.set_draft_provider(id.as_ref(), cx);
                }
            },
        );
        let model_subscription = cx.subscribe(
            draft_selects.model(),
            |this, _, event: &SelectEvent<SearchableVec<ModelOption>>, cx| {
                if let SelectEvent::Confirm(Some(id)) = event {
                    this.set_draft_model(id.as_ref(), cx);
                }
            },
        );

        let live_model_field = LiveModelField::new(window, cx);
        let live_model_subscription = cx.subscribe(
            live_model_field.state(),
            |this, _, event: &SelectEvent<SearchableVec<ModelOption>>, cx| {
                if let SelectEvent::Confirm(Some(id)) = event {
                    this.set_live_model(id.as_ref(), cx);
                }
            },
        );

        // An OS-level close (window manager X button) must clear the dialog
        // state just like the in-window Cancel button does. Our own closes
        // clear the tracked id first, so this observer never re-enters the
        // entity while it is being updated.
        let dialog_window_id = Rc::new(Cell::new(None));
        let weak_app = cx.entity().downgrade();
        let tracked_id = dialog_window_id.clone();
        let window_closed = cx.on_window_closed(move |cx, window_id| {
            if tracked_id.get() != Some(window_id) {
                return;
            }
            if let Some(app) = weak_app.upgrade() {
                app.update(cx, |app, cx| app.handle_window_closed(window_id, cx));
            }
        });

        let (stored_settings, settings_error) = match services.settings.load() {
            Ok(settings) => (settings, None),
            Err(error) => (Settings::default(), Some(error)),
        };
        let mut controller = SessionController::new();
        controller.set_sink(services.sink);
        // Persisted settings always reach the controller: the New Session
        // form and `Start` both read the provider/model/key from here.
        controller.apply_settings(stored_settings.clone());

        let mut app = Self {
            focus: cx.focus_handle(),
            shell_window: window.window_handle(),
            controller,
            ripple: None,
            ripple_seq: 0,
            settings_api_key,
            settings_bun_path,
            settings_key_revealed: false,
            draft_selects,
            live_model_field,
            dialog_window: None,
            dialog_window_id,
            pending_settings: None,
            _subscriptions: vec![
                settings_subscription,
                bun_path_subscription,
                provider_subscription,
                model_subscription,
                live_model_subscription,
                window_closed,
            ],
            _event_task: None,
            backend_spawn: Box::new(|settings| {
                spawn_backend(settings).map(|backend| Box::new(backend) as Box<dyn SttSessionSink>)
            }),
            catalogue_from_startup: false,
            _clock_task: None,
            _audio_task: None,
            audio_thread: None,
            audio_stop: None,
            audio: Some(services.audio),
            audio_factory,
            analyzer: services.analyzer,
            settings_store: services.settings,
            clipboard: services.clipboard,
            paste: services.paste,
            opener: services.opener,
            save_prompt: services.save_prompt,
            save_prompt_open: false,
            clock: services.clock,
            stored_settings,
            pending_window_layout: None,
            caption_scroll: ScrollHandle::new(),
        };

        if let Some(error) = settings_error {
            app.controller.report_error(AppError::from_settings(&error));
        }
        if start_events {
            app.start_event_loop(services.events, cx);
            // The test seam has its sink already attached; the production
            // path asks for the catalogue after it spawned the real worker, so
            // a healthy launch never sends a command to the stand-in.
            app.controller.request_capabilities();
        }
        app.start_clock_loop(cx);
        // The widget root holds focus until a control inside it takes over, so
        // the window's key context is on the dispatch path from the first frame.
        app.focus.focus(window, cx);
        app
    }

    fn start_clock_loop(&mut self, cx: &mut Context<Self>) {
        self._clock_task = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(200))
                    .await;
                let updated = this.update(cx, |this, cx| {
                    let now = this.clock.now();
                    this.flush_pending_window_layout(now);
                    if this.controller.session_state().is_active()
                        && this.controller.refresh_elapsed(now)
                    {
                        cx.notify();
                    }
                });
                if updated.is_err() {
                    break;
                }
            }
        }));
    }

    // ----- Queries -----------------------------------------------------

    /// What screen the widget currently shows (test surface, no window needed
    /// once the view exists — the controller alone answers this).
    pub fn view_state(&self) -> ViewState {
        self.controller.view_state()
    }

    pub fn session_state(&self) -> SessionState {
        self.controller.session_state()
    }

    pub fn session_missing(&self) -> Vec<SessionField> {
        self.controller.session_missing()
    }

    pub fn is_draft_complete(&self) -> bool {
        self.controller.is_draft_complete()
    }

    pub fn finished_take(&self) -> Option<&FinishedTake> {
        self.controller.finished_take()
    }

    /// Decoded position of the current file take, in seconds:
    /// `(processed, total)`, where `total` is absent when the probe failed.
    pub fn file_progress(&self) -> Option<(f32, Option<f32>)> {
        self.controller.progress()
    }

    /// Whether a file take is decoding right now.
    pub fn is_file_take(&self) -> bool {
        self.controller.is_file_take()
    }

    /// Whether the finished file take was ended by the user before the end.
    pub fn file_take_stopped_early(&self) -> bool {
        self.controller.file_take_stopped_early()
    }

    /// Whether a finished file take waits behind Continue.
    pub fn is_awaiting_continue(&self) -> bool {
        self.controller.is_awaiting_continue()
    }

    pub fn transcript_committed(&self) -> &str {
        self.controller.transcript_committed()
    }

    pub fn transcript_live(&self) -> &str {
        self.controller.transcript_live()
    }

    pub fn transcript_full_text(&self) -> String {
        self.controller.transcript_full_text()
    }

    /// The finished take's text exactly as the screen shows it.
    pub fn displayed_transcript(&self) -> String {
        self.controller.displayed_transcript()
    }

    /// What the cleanup action should do next (Ready, Running, Cleaned, …).
    pub fn clean_status(&self) -> CleanStatus {
        self.controller.clean_status()
    }

    /// Whether a repair pass can be started right now.
    pub fn can_clean_transcript(&self) -> bool {
        self.controller.can_clean_transcript()
    }

    /// What the cleanup button does next.
    pub fn clean_toggle(&self) -> CleanToggle {
        self.controller.clean_toggle()
    }

    /// The cleanup model the next pass would call.
    pub fn cleanup_model(&self) -> Option<&str> {
        self.controller.cleanup_model()
    }

    pub fn elapsed_seconds(&self) -> u32 {
        self.controller.elapsed_seconds()
    }

    pub fn app_error(&self) -> Option<&AppError> {
        self.controller.app_error()
    }

    pub fn written_path(&self) -> Option<&std::path::Path> {
        self.controller.written_path()
    }

    /// Whether a save dialog is open. A second Ctrl+S while the OS dialog is up
    /// must not stack another one.
    pub fn save_prompt_open(&self) -> bool {
        self.save_prompt_open
    }

    pub fn active_dialog(&self) -> Option<Dialog> {
        self.controller.active_dialog()
    }

    pub fn settings_draft(&self) -> &SettingsDraft {
        self.controller.settings_draft()
    }

    /// The gateway reading both dialog footers draw; see
    /// [`SessionController::gateway_state`].
    pub fn gateway_state(&self) -> ConnectionState {
        self.controller.gateway_state()
    }

    /// The masked stored-key line for the Settings API-key row, if any.
    pub fn settings_stored_key_label(&self) -> Option<String> {
        self.controller.settings_stored_key_label()
    }

    /// Hint shown while Reveal is disabled for a stored key.
    pub fn settings_stored_key_hint(&self) -> Option<&'static str> {
        self.controller.settings_stored_key_hint()
    }

    /// Whether Reveal applies to a typed draft value.
    pub fn settings_key_reveal_enabled(&self) -> bool {
        self.controller.settings_key_reveal_enabled()
    }

    /// Current text of the Settings API-key field (query surface for tests).
    pub fn settings_api_key_value(&self, cx: &App) -> String {
        self.settings_api_key.read(cx).value().to_string()
    }

    /// The change staged by `Save changes`, consumed by the settings store.
    pub fn pending_settings_change(&self) -> Option<&SettingsChange> {
        self.pending_settings.as_ref()
    }

    /// Install a provider catalogue from a capability handshake.
    ///
    /// The worker's event loop normally reaches this through
    /// `SessionController::handle_stt_event`; the headless window fixture calls
    /// it directly so Settings can be exercised without a live Bun worker.
    /// Help page currently shown (query surface for tests and the view).
    pub fn help_tab(&self) -> HelpTab {
        self.controller.help_tab()
    }

    /// Models listed by the Help dialog.
    pub fn help_models(&self) -> &[speechtt_core::protocol::ModelCapabilityInfo] {
        self.controller.help_models()
    }

    /// Switches the Help dialog page.
    pub fn set_help_tab(&mut self, tab: HelpTab, cx: &mut Context<Self>) {
        self.controller.set_help_tab(tab);
        cx.notify();
    }

    pub fn apply_capabilities(
        &mut self,
        providers: Vec<speechtt_core::protocol::ProviderCapability>,
        cx: &mut Context<Self>,
    ) {
        self.controller.apply_capabilities(
            providers,
            speechtt_core::protocol::CapabilitySource::Builtin,
        );
        cx.notify();
    }

    /// Whether a dialog currently has its own window open.
    pub fn has_dialog_window(&self) -> bool {
        self.dialog_window.is_some()
    }

    pub(crate) fn settings_api_key_input(&self) -> &Entity<InputState> {
        &self.settings_api_key
    }

    /// The `Bun path` field of the Settings dialog.
    pub(crate) fn settings_bun_path_input(&self) -> &Entity<InputState> {
        &self.settings_bun_path
    }

    pub fn settings_key_revealed(&self) -> bool {
        self.settings_key_revealed
    }

    /// Read access used by the screen renderers.
    pub(crate) fn controller(&self) -> &SessionController {
        &self.controller
    }

    /// The provider and model dropdowns of the Session setup form.
    pub(crate) fn draft_selects(&self) -> &SelectFields {
        &self.draft_selects
    }

    /// The model dropdown of the live take screen's header.
    pub(crate) fn live_model_field(&self) -> &LiveModelField {
        &self.live_model_field
    }

    /// Catalogue label of the model the live take uses, or would use.
    pub fn live_model_label(&self) -> Option<&str> {
        self.controller.live_model_label()
    }

    /// Chooses the model for the take being prepared.
    ///
    /// Take-scoped: it changes the model of the take about to be recorded and
    /// leaves the Settings default alone.
    pub fn set_live_model(&mut self, model: &str, cx: &mut Context<Self>) -> bool {
        if !self.controller.set_live_model(model) {
            return false;
        }
        cx.notify();
        true
    }

    /// Catalogue id of the provider chosen for the next take.
    pub fn draft_provider(&self) -> Option<&str> {
        self.controller.draft_provider()
    }

    /// Catalogue id of the model chosen for the next take.
    pub fn draft_model(&self) -> Option<&str> {
        self.controller.draft_model()
    }

    /// Mode of the model chosen for the next take.
    pub fn draft_model_mode(&self) -> Option<TranscriptionMode> {
        self.controller.draft_model_mode()
    }

    /// Scroll position of the live caption well, passed to
    /// [`crate::components::CaptionWell::follow`] by the take screen.
    pub(crate) fn caption_scroll(&self) -> &ScrollHandle {
        &self.caption_scroll
    }

    /// Puts the live caption well back at the top for a take that is about to
    /// start.
    ///
    /// The handle outlives a take, so an offset the reader scrolled to in the
    /// previous one would otherwise open the next take already scrolled up,
    /// with the follow rule inactive until they scroll back to the bottom.
    fn rewind_caption(&mut self) {
        self.caption_scroll.set_offset(Point::new(px(0.0), px(0.0)));
    }

    // ----- Commands ----------------------------------------------------

    /// Enter the workflow for `mode`, then route: live goes to Ready, file
    /// opens the picker straight away (the form follows the picked file).
    pub fn choose_mode(&mut self, mode: SessionMode, cx: &mut Context<Self>) {
        self.controller.choose_mode(mode);
        if mode == SessionMode::File {
            self.pick_transcription_file(cx);
        }
        cx.notify();
    }

    /// Back to the mode chooser (the file flow's exit, and the command under
    /// the shared footer's back action).
    pub fn change_mode(&mut self, cx: &mut Context<Self>) {
        self.stop_audio_capture();
        self.controller.change_mode();
        cx.notify();
    }

    /// The one back action every screen footer renders.
    ///
    /// Back always means "one step towards the mode chooser": the setup form
    /// discards its draft, a finished take is cleared, and the take screen
    /// returns home. Transient state (error strip, pending file, output status,
    /// finished take) is dropped exactly as the underlying controller method
    /// does it, so no stale screen state survives the step.
    pub fn go_back(&mut self, cx: &mut Context<Self>) {
        self.stop_audio_capture();
        self.controller.go_back();
        cx.notify();
    }

    pub fn cancel_form(&mut self, cx: &mut Context<Self>) {
        self.controller.cancel_form();
        cx.notify();
    }

    /// Commits the dropdown's provider choice into the setup draft.
    pub fn set_draft_provider(&mut self, provider: &str, cx: &mut Context<Self>) -> bool {
        if !self.controller.set_draft_provider(provider) {
            return false;
        }
        cx.notify();
        true
    }

    /// Commits the dropdown's model choice into the setup draft.
    pub fn set_draft_model(&mut self, model: &str, cx: &mut Context<Self>) -> bool {
        if !self.controller.set_draft_model(model) {
            return false;
        }
        cx.notify();
        true
    }

    pub fn create_session(&mut self, cx: &mut Context<Self>) {
        // A file take starts from the path already picked, so it is captured
        // before the commit consumes it.
        let pending = self.controller.pending_file().map(Path::to_path_buf);
        if self.controller.create_session()
            && let Some(path) = pending
        {
            self.start_file_from_path(&path, cx);
        }
        cx.notify();
    }

    /// Starts the press ripple of round button `id` and repaints.
    ///
    /// The wave is concentric with the button, not with the pointer, so the
    /// anchor recorded while the button prepainted is the origin. The ring is
    /// owned by the window rather than by the pressed button: every round
    /// action navigates, so a ring drawn inside the pressed screen would vanish
    /// on release. Recording it here lets it finish after the screen changes.
    pub fn ripple(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(anchor) = crate::components::button_anchor(id, cx) else {
            return;
        };
        self.ripple_seq += 1;
        self.ripple = Some(Ripple::new(
            self.ripple_seq,
            f32::from(anchor.center.x),
            f32::from(anchor.center.y),
            anchor.tone,
        ));
        cx.notify();
    }

    /// The ripple currently animating, if any.
    #[must_use]
    pub fn current_ripple(&self) -> Option<Ripple> {
        self.ripple
    }

    pub fn start_recording(&mut self, cx: &mut Context<Self>) {
        if self.controller.start_recording_at(self.clock.now()) {
            self.rewind_caption();
            self.start_audio_capture(cx);
        }
        cx.notify();
    }

    /// Starts, pauses or resumes the live take — the record button's command.
    ///
    /// One method for the button and for **Space**, so the two cannot disagree
    /// about what a press means. The session decides which of the three steps
    /// applies (see [`SessionController::toggle_recording_at`]); a start also
    /// rewinds the caption well and opens the capture, exactly as
    /// [`Self::start_recording`] does. Returns `false`, having changed nothing,
    /// when no take can be toggled — on the mode chooser, on the transcript
    /// screen, or for a file take the worker decodes.
    pub fn toggle_recording(&mut self, cx: &mut Context<Self>) -> bool {
        let starting = self.controller.session_state() == SessionState::Ready;
        if !self.controller.toggle_recording_at(self.clock.now()) {
            cx.notify();
            return false;
        }
        if starting {
            self.rewind_caption();
            self.start_audio_capture(cx);
        }
        cx.notify();
        true
    }

    /// Opens the native file picker, then shows the setup form for the chosen
    /// file.
    ///
    /// The picker is the only platform-specific part: the worker decodes the
    /// file itself, so no media crosses the bridge. Cancelling the dialog is a
    /// full no-op on the flow — the user stays on the welcome screen.
    pub fn pick_transcription_file(&mut self, cx: &mut Context<Self>) {
        let selection = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Transcribe".into()),
        });

        cx.spawn(async move |this, cx| {
            match selection.await {
                Ok(Ok(Some(paths))) => {
                    if let Some(path) = paths.into_iter().next() {
                        let _ = this.update(cx, |this, cx| {
                            this.accept_picked_file(path, cx);
                        });
                    }
                }
                // The user closed the dialog: back to the chooser, no trace.
                Ok(Ok(None)) => {
                    let _ = this.update(cx, |this, cx| {
                        this.controller.change_mode();
                        cx.notify();
                    });
                }
                Ok(Err(_)) | Err(_) => {
                    let _ = this.update(cx, |this, cx| {
                        this.controller.report_error(AppError::FileDialogFailed);
                        cx.notify();
                    });
                }
            }
        })
        .detach();
    }

    /// Accept (or refuse) the file chosen in the picker.
    ///
    /// Shared by the picker closure and the headless tests. A non-media path is
    /// refused before any take exists: the strip explains it and the user stays
    /// on the welcome screen. Returns `true` when the setup form opened.
    pub fn accept_picked_file(&mut self, path: PathBuf, cx: &mut Context<Self>) -> bool {
        let accepted = self.controller.set_pending_file(path).is_ok();
        if !accepted {
            self.controller.change_mode();
        }
        cx.notify();
        accepted
    }

    /// Starts a file take for `path`. Headless-testable seam behind the picker.
    ///
    /// Validation errors are already recorded by the controller, so the strip
    /// shows them without any extra plumbing here.
    pub fn start_file_from_path(&mut self, path: &Path, cx: &mut Context<Self>) {
        if self.controller.start_file_session(path).is_ok() {
            // The worker decodes on its own host: never start a mic capture.
            self.stop_audio_capture();
        }
        cx.notify();
    }

    pub fn pause_recording(&mut self, cx: &mut Context<Self>) {
        self.controller.pause_recording_at(self.clock.now());
        cx.notify();
    }

    pub fn resume_recording(&mut self, cx: &mut Context<Self>) {
        self.controller.resume_recording_at(self.clock.now());
        cx.notify();
    }

    pub fn finish_take(&mut self, cx: &mut Context<Self>) {
        if !self.controller.finish_take() {
            return;
        }
        self.stop_audio_capture();

        // Nothing is written automatically: Save is an explicit action for both
        // modes, so the user names the transcript. The automatic copy/paste is
        // deferred to the worker's `Stopped` event, which carries the final
        // text; see [`Self::flush_auto_paste`].
        cx.notify();
    }

    /// Runs the automatic copy/paste once the final transcript has landed.
    ///
    /// The finished take is refined in place by the worker's `Stopped` event,
    /// so pasting when `Stop` is sent would copy the pre-final text while the
    /// screen ends up showing something else.
    fn flush_auto_paste(&mut self) {
        if !self.controller.take_auto_paste_request() {
            return;
        }
        let _ = self
            .controller
            .auto_paste_after_finish(&mut *self.clipboard, &mut *self.paste);
    }

    /// Whether a capture thread is running (test surface for device release).
    ///
    /// Reset, Cancel and Stop release the microphone, so this must be `false`
    /// after any of them.
    #[must_use]
    pub fn is_capturing_audio(&self) -> bool {
        self.audio_stop.is_some()
    }

    /// Abandon the open take and stay on the take screen (the footer's Reset).
    ///
    /// Capture stops, the worker tears the session down, the transcript,
    /// waveform and timer are cleared, and the configuration survives so the
    /// mic starts a fresh take immediately.
    pub fn reset_take(&mut self, cx: &mut Context<Self>) -> bool {
        let reset = self.controller.reset_take();
        if reset {
            self.rewind_caption();
            self.stop_audio_capture();
        }
        cx.notify();
        reset
    }

    /// Open the finished file take's transcript (the transcription screen's
    /// Continue).
    pub fn continue_to_end(&mut self, cx: &mut Context<Self>) -> bool {
        let continued = self.controller.continue_to_end();
        cx.notify();
        continued
    }

    /// Leave the take screen for the mode chooser, discarding an open take
    /// (the footer's Cancel). Capture stops first so the device is released.
    pub fn cancel_take(&mut self, cx: &mut Context<Self>) -> bool {
        self.stop_audio_capture();
        let cancelled = self.controller.cancel_take();
        cx.notify();
        cancelled
    }

    pub fn new_take(&mut self, cx: &mut Context<Self>) {
        self.stop_audio_capture();
        self.controller.new_take();
        cx.notify();
    }

    /// Runs a keyboard command through the same guards as its button.
    ///
    /// The binding's key-context predicate is the first gate and this state
    /// guard is the second, so a command that matched in a rare race still
    /// declines. Returns `false` — having changed nothing — when the command is
    /// inert in the current state; every key handler turns that into
    /// [`Context::propagate`], which lets action dispatch continue to the next
    /// candidate for the same keystroke. The two dialog toggles always apply:
    /// they either open the dialog or close the one that is already open.
    pub fn apply_action(
        &mut self,
        action: SessionAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        match action {
            SessionAction::ToggleRecording => self.toggle_recording(cx),
            SessionAction::StopRecording => {
                // Stop is valid from the two active states, so `finish_take`
                // cannot decline once this guard passes.
                if !self.controller.session_state().is_active() {
                    return false;
                }
                self.finish_take(cx);
                true
            }
            SessionAction::ToggleHelp => {
                self.toggle_dialog(Dialog::Help, window, cx);
                true
            }
            SessionAction::ToggleSettings => {
                self.toggle_dialog(Dialog::Settings, window, cx);
                true
            }
            SessionAction::SaveTranscript => self.save_transcript(cx),
        }
    }

    /// Opens `dialog`, or closes it when it is the one already open.
    fn toggle_dialog(&mut self, dialog: Dialog, window: &mut Window, cx: &mut Context<Self>) {
        if self.controller.active_dialog() == Some(dialog) {
            self.close_dialog(window, cx);
            return;
        }
        match dialog {
            Dialog::Help => self.open_help(window, cx),
            Dialog::Settings => self.open_settings(window, cx),
        }
    }

    /// The `Space` handler: starts, pauses or resumes the live take.
    fn on_toggle_recording(
        &mut self,
        _: &ToggleRecording,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.apply_action(SessionAction::ToggleRecording, window, cx) {
            cx.propagate();
        }
    }

    /// The `Enter` handler: ends the take and shows its transcript.
    fn on_stop_recording(
        &mut self,
        _: &StopRecording,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.apply_action(SessionAction::StopRecording, window, cx) {
            cx.propagate();
        }
    }

    /// The `F1` handler: opens Help, or closes it when it is already open.
    fn on_toggle_help(&mut self, _: &ToggleHelp, window: &mut Window, cx: &mut Context<Self>) {
        if !self.apply_action(SessionAction::ToggleHelp, window, cx) {
            cx.propagate();
        }
    }

    /// The `F2` handler: opens Settings, or closes it when it is already open.
    fn on_toggle_settings(
        &mut self,
        _: &ToggleSettings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.apply_action(SessionAction::ToggleSettings, window, cx) {
            cx.propagate();
        }
    }

    /// The `Ctrl+S` handler: asks where to write the finished take.
    fn on_save_transcript(
        &mut self,
        _: &SaveTranscript,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.apply_action(SessionAction::SaveTranscript, window, cx) {
            cx.propagate();
        }
    }

    /// Copy the finished take through the injected clipboard adapter.
    ///
    /// This is the result screen's whole-take Copy action, separate from
    /// [`Self::on_copy_selection`], which copies only the range the pointer
    /// selected.
    pub fn copy_transcript(&mut self, cx: &mut Context<Self>) -> bool {
        let copied = self
            .controller
            .copy_finished_transcript(&mut *self.clipboard)
            .is_ok();
        cx.notify();
        copied
    }

    /// The footer's single cleanup button: run the pass, or flip a cached one.
    ///
    /// Only [`CleanToggle::Start`] costs a request. The two flips read the
    /// controller's cached pass, so a user can compare the repaired and original
    /// texts as often as they like without calling the model again. Returns
    /// `false` — having changed nothing — when the button is inert.
    pub fn toggle_clean_transcript(&mut self, cx: &mut Context<Self>) -> bool {
        let toggle = self.controller.clean_toggle();
        let changed = match toggle {
            CleanToggle::ShowCleaned | CleanToggle::ShowOriginal => {
                self.controller.flip_clean_transcript()
            }
            CleanToggle::Start => self.start_clean_transcript(),
            CleanToggle::Inert => false,
        };
        cx.notify();
        changed
    }

    /// Sends one cleanup request. Called only from
    /// [`Self::toggle_clean_transcript`], which owns the toggle decision.
    fn start_clean_transcript(&mut self) -> bool {
        let Some(request_id) = self.controller.begin_clean_transcript() else {
            return false;
        };
        let Some(command) = self.controller.clean_transcript_command(&request_id) else {
            // The provider has no key: say so and re-arm the action rather
            // than leaving a request nothing will ever answer.
            let provider = self.controller.active_provider();
            self.controller
                .fail_clean_transcript(&request_id, AppError::MissingApiKey { provider });
            return false;
        };
        self.controller.send_command(command);
        true
    }

    /// Put the selected transcript range on the system clipboard.
    ///
    /// [`TextSelection::selected_text`] is the only source of the copied string:
    /// it resolves the highlighted run across the committed and the live part of
    /// the transcript, so this handler never invents its own notion of "the
    /// current transcript". An empty or whitespace-only selection is not written
    /// at all (see [`copyable_selection`]), which is how the clipboard keeps its
    /// previous content after a stray keystroke.
    ///
    /// The action is consumed either way: the window has no other selection for a
    /// fallback handler to copy, and writing twice would be a second clipboard
    /// change per keystroke. Nothing is reported to the user and nothing is
    /// logged, because a copy is silent by convention and transcript text is the
    /// user's own content.
    fn on_copy_selection(
        &mut self,
        _: &CopySelection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let selected = TextSelection::selected_text(window, cx);
        if let Some(text) = copyable_selection(&selected) {
            cx.write_to_clipboard(text.to_owned().into());
        }
    }

    /// Ask where to write the finished take, with a suggested name: the source
    /// file's stem for a file take, a timestamped `speechtt-…` for dictation.
    ///
    /// Returns `false`, having asked nothing, when there is no transcript to
    /// write or while the dialog is already up — a second `Ctrl+S` must not
    /// stack another dialog. The three [`SaveAnswer`]s are handled in one place:
    /// a chosen path is written, a dismissed dialog changes nothing, and a
    /// dialog that could not open reports [`AppError::FileDialogFailed`].
    pub fn save_transcript(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.controller.can_save_transcript() || self.save_prompt_open {
            return false;
        }
        let prefix = self
            .controller
            .suggested_save_name()
            .unwrap_or_else(|| "speechtt".to_owned());
        let suggested = suggested_transcript_name(&prefix, self.clock.system_now());

        self.save_prompt_open = true;
        let selection = self.save_prompt.pick_save_path(cx, &suggested);
        cx.spawn(async move |this, cx| {
            let answer = selection.await;
            let _ = this.update(cx, |this, cx| {
                this.save_prompt_open = false;
                match answer {
                    SaveAnswer::Chosen(path) => {
                        this.save_transcript_to(&path, cx);
                    }
                    SaveAnswer::Cancelled => {}
                    SaveAnswer::Failed => {
                        this.controller.report_error(AppError::FileDialogFailed);
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
        true
    }

    /// Write the finished take to `path`. Headless-testable seam behind the
    /// save dialog.
    pub fn save_transcript_to(&mut self, path: &Path, cx: &mut Context<Self>) -> bool {
        let saved = self.controller.save_finished_transcript_to(path).is_ok();
        cx.notify();
        saved
    }

    /// Open the saved transcript with the injected opener.
    pub fn open_saved_file(&mut self, cx: &mut Context<Self>) -> bool {
        let opened = self.controller.open_saved_file(&mut *self.opener).is_ok();
        cx.notify();
        opened
    }

    /// Dismiss the status strip.
    pub fn dismiss_error(&mut self, cx: &mut Context<Self>) {
        self.controller.dismiss_error();
        cx.notify();
    }

    /// Retry the current take after a retryable backend/network failure.
    pub fn retry_backend(&mut self, cx: &mut Context<Self>) {
        self.controller.retry_start();
        cx.notify();
    }

    // ----- Dialogs -----------------------------------------------------

    /// Opens Help in its own floating window beside the widget.
    pub fn open_help(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.release_dialog_window(window, cx);
        self.controller.open_help();
        self.request_dialog_window(Dialog::Help, cx);
        cx.notify();
    }

    /// Opens Settings in its own floating window beside the widget.
    ///
    /// A configured install opens on its stored values, so the dialog shows
    /// what will be used rather than an empty form. A fresh install opens empty
    /// and asks for a provider first.
    pub fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.release_dialog_window(window, cx);
        // The controller holds the migrated view of the settings: a file
        // written before the two defaults existed only shows its single model
        // there, once the catalogue has resolved its mode.
        let stored = self
            .controller
            .settings()
            .cloned()
            .unwrap_or_else(|| self.stored_settings.clone());
        let provider = stored.provider.clone();
        let streaming = stored.model_streaming.clone();
        let batch = stored.model_batch.clone();
        if !self.controller.catalogue_loaded() {
            // No catalogue can validate the stored ids, and none may arrive
            // while the backend is down; show the persisted values so the
            // `Bun path` correction is savable.
            self.controller.open_settings_from_stored();
        } else if streaming.trim().is_empty() && batch.trim().is_empty() {
            self.controller.open_settings();
        } else {
            let has_key = stored.api_key(&stored.provider).is_some();
            self.controller.open_settings_with(
                Some(&provider),
                (!streaming.trim().is_empty()).then_some(streaming.as_str()),
                (!batch.trim().is_empty()).then_some(batch.as_str()),
                has_key,
            );
        }
        // Refresh the provider/model catalogue each time the dialog opens so
        // a key exported after startup appears as connected. With no worker
        // attached the command has nowhere to go, and the dialog shows the
        // recorded failure instead.
        if !self.controller.backend_unavailable() {
            self.controller.request_capabilities();
        }
        self.settings_key_revealed = false;
        self.settings_api_key.update(cx, |state, cx| {
            state.set_value("", window, cx);
            state.set_masked(true, window, cx);
        });
        let bun_path = self.controller.settings_draft().bun_executable.clone();
        self.settings_bun_path
            .update(cx, |state, cx| state.set_value(bun_path, window, cx));
        self.request_dialog_window(Dialog::Settings, cx);
        cx.notify();
    }

    /// Cancel semantics: discard the draft. The dialog window closes itself.
    pub fn close_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.controller.close_dialog();
        self.clear_settings_key(window, cx);
        self.release_dialog_window(window, cx);
        cx.notify();
    }

    /// Escape path — discards the draft and closes the dialog window.
    pub fn dismiss_dialog_with_escape(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.controller.dismiss_dialog_with_escape();
        self.clear_settings_key(window, cx);
        self.close_dialog_window(cx);
        cx.notify();
    }

    /// Schedules the dialog window beside the widget, on the side with room.
    ///
    /// Opening is deferred because a window draws as soon as it is created.
    /// The Settings root reads this entity, so creating it inside an entity
    /// update would violate GPUI's borrow rules. The deferred pass checks the
    /// request is still current, opens outside any entity borrow, then records
    /// the handle. `WindowKind::Floating` keeps it above the widget on the
    /// platforms that support transient windows.
    ///
    /// The new window is placed against the widget window, never against the
    /// dialog that forwarded the command: that dialog may be closing itself in
    /// the same keystroke, and its handle must not be updated from here.
    fn request_dialog_window(&mut self, dialog: Dialog, cx: &mut Context<Self>) {
        let app = cx.entity();
        let shell_window = self.shell_window;
        cx.defer(move |cx| {
            if app.update(cx, |app, _| app.active_dialog()) != Some(dialog) {
                return;
            }

            let title = dialogs::window_title(dialog);
            let app_for_window = app.clone();
            let opened = shell_window.update(cx, |_, window, cx| {
                let bounds = dialog_window_bounds(dialog, window, cx);
                let options = WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    kind: WindowKind::Floating,
                    focus: true,
                    show: true,
                    is_resizable: false,
                    is_minimizable: false,
                    ..WindowOptions::default()
                };
                gpui_kit::open_window(options, cx, move |window, cx| {
                    window.set_window_title(&title);
                    cx.new(|cx| DialogWindow::new(dialog, app_for_window, window, cx))
                })
            });

            match opened {
                Ok(Ok((handle, _))) => {
                    app.update(cx, |app, cx| app.attach_dialog_window(dialog, handle, cx));
                }
                Ok(Err(_)) | Err(_) => {
                    app.update(cx, |app, cx| app.abandon_dialog(dialog, cx));
                }
            }
        });
    }

    /// Records a freshly opened dialog window, or closes it when the request
    /// was superseded while it was being created.
    fn attach_dialog_window(
        &mut self,
        dialog: Dialog,
        handle: AnyWindowHandle,
        cx: &mut Context<Self>,
    ) {
        if self.controller.active_dialog() == Some(dialog) {
            self.dialog_window_id.set(Some(handle.window_id()));
            self.dialog_window = Some((dialog, handle));
        } else {
            let _ = handle.update(cx, |_, window, _| window.remove_window());
        }
    }

    /// Keeps the controller consistent when the platform refuses a window.
    fn abandon_dialog(&mut self, dialog: Dialog, cx: &mut Context<Self>) {
        if self.controller.active_dialog() == Some(dialog) {
            self.controller.close_dialog();
            self.settings_key_revealed = false;
            cx.notify();
        }
    }

    fn close_dialog_window(&mut self, cx: &mut Context<Self>) {
        // Clear first: removing the window synchronously notifies the
        // window-closed observer, which must not re-enter this entity.
        self.dialog_window_id.set(None);
        if let Some((_, handle)) = self.dialog_window.take() {
            let _ = handle.update(cx, |_, window, _| window.remove_window());
        }
    }

    /// Releases the open dialog window for a command that arrived from
    /// `window`.
    ///
    /// When `window` is the dialog's own window, the dialog removes itself
    /// after forwarding the command, so the handle is only forgotten: reaching
    /// into that window through its handle while it is dispatching the
    /// keystroke would re-enter a window that is already borrowed. Any other
    /// window closes the dialog from here.
    fn release_dialog_window(&mut self, window: &Window, cx: &mut Context<Self>) {
        let Some((_, handle)) = self.dialog_window else {
            return;
        };
        if handle.window_id() == window.window_handle().window_id() {
            // Also clears the id the observer compares against, so the close
            // that follows cannot dismiss a replacement dialog.
            self.dialog_window_id.set(None);
            self.dialog_window = None;
        } else {
            self.close_dialog_window(cx);
        }
    }

    fn handle_window_closed(&mut self, window_id: WindowId, cx: &mut Context<Self>) {
        let Some((_, handle)) = self.dialog_window else {
            return;
        };
        if handle.window_id() != window_id {
            return;
        }
        self.dialog_window_id.set(None);
        self.dialog_window = None;
        if self.controller.active_dialog().is_some() {
            self.controller.close_dialog();
            self.settings_key_revealed = false;
            cx.notify();
        }
    }

    /// Commits the Settings dropdown's provider choice into the draft.
    pub fn set_settings_provider(&mut self, provider: &str, cx: &mut Context<Self>) -> bool {
        if !self.controller.set_settings_provider(provider) {
            return false;
        }
        cx.notify();
        true
    }

    /// Commits the Settings dialog's streaming default into the draft.
    pub fn set_settings_streaming_model(&mut self, model: &str, cx: &mut Context<Self>) -> bool {
        if !self.controller.set_settings_streaming_model(model) {
            return false;
        }
        cx.notify();
        true
    }

    /// Commits the Settings dialog's batch default into the draft.
    pub fn set_settings_batch_model(&mut self, model: &str, cx: &mut Context<Self>) -> bool {
        if !self.controller.set_settings_batch_model(model) {
            return false;
        }
        cx.notify();
        true
    }

    /// Commits the Settings dialog's dictation language into the draft.
    ///
    /// The empty code selects Auto, which stores no language at all.
    pub fn set_settings_language(&mut self, code: &str, cx: &mut Context<Self>) -> bool {
        if !self.controller.set_settings_language(code) {
            return false;
        }
        cx.notify();
        true
    }

    /// Commits the Settings dialog's cleanup model into the draft.
    pub fn set_settings_cleanup_model(&mut self, model: &str, cx: &mut Context<Self>) -> bool {
        if !self.controller.set_settings_cleanup_model(model) {
            return false;
        }
        cx.notify();
        true
    }

    /// Mirrors a typed `Bun path` into both the controller and its input.
    ///
    /// The same pair the API-key setter mirrors: the draft is what the save
    /// reads, the input is what the dialog shows, and they must not diverge.
    pub fn set_settings_bun_executable(
        &mut self,
        value: impl Into<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let value = value.into();
        self.controller.set_settings_bun_executable(value.clone());
        self.settings_bun_path
            .update(cx, |state, cx| state.set_value(value, window, cx));
        cx.notify();
    }

    /// Mirrors a typed key into both the controller and the masked input.
    pub fn set_settings_api_key_value(
        &mut self,
        value: impl Into<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let value = value.into();
        self.controller.set_settings_api_key(value.clone());
        self.settings_api_key
            .update(cx, |state, cx| state.set_value(value, window, cx));
        cx.notify();
    }

    pub fn toggle_settings_key_reveal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // A stored key is never held in the field, so there is nothing to
        // reveal until the user types or pastes a replacement.
        if !self.controller.settings_key_reveal_enabled() {
            return;
        }
        self.settings_key_revealed = !self.settings_key_revealed;
        let masked = !self.settings_key_revealed;
        self.settings_api_key
            .update(cx, |state, cx| state.set_masked(masked, window, cx));
        cx.notify();
    }

    /// Commits the Settings draft, spawning a worker when the dialog was the
    /// last chance to reach one.
    ///
    /// Returns `false` — with the dialog and the draft still on screen — when
    /// the draft is incomplete, the store rejects the save, or the respawn the
    /// save made possible still fails. A *healthy* backend is never replaced:
    /// only an unattached one, or one whose worker has died while its sink was
    /// still attached, is respawned. When the backend is healthy the existing
    /// handshake refresh stands.
    pub fn save_settings(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(change) = self.controller.settings_change() else {
            return false;
        };
        if !self.apply_settings_change(change) {
            cx.notify();
            return false;
        }

        if self.controller.backend_unavailable() && !self.respawn_backend(cx) {
            cx.notify();
            return false;
        }

        self.controller.close_dialog();
        cx.notify();
        true
    }

    /// Persist a committed Settings draft and make it current for the next
    /// take. The draft key is written only to the store; it never appears in
    /// `pending_settings` debug output because `SettingsChange` redacts it.
    ///
    /// Returns whether the store accepted the value. Attaching a backend is
    /// [`SpeechTtApp::save_settings`]'s job because it needs the window context.
    pub fn apply_settings_change(&mut self, change: SettingsChange) -> bool {
        let next = settings_after_change(&change, &self.stored_settings);

        self.pending_settings = Some(change);

        match self.settings_store.save(&next) {
            Ok(()) => {
                self.stored_settings = next.clone();
                self.controller.apply_settings(next);
                self.controller.dismiss_error();
                // Re-run the handshake so the newly saved key flips the
                // provider to CONNECTED without reopening the app. When no
                // backend is attached, `save_settings` respawns it and that
                // path asks for the catalogue instead.
                if !self.controller.backend_unavailable() {
                    self.controller.request_capabilities();
                }
                true
            }
            Err(error) => {
                self.controller
                    .report_error(AppError::from_settings(&error));
                false
            }
        }
    }

    fn clear_settings_key(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.settings_api_key.update(cx, |state, cx| {
            state.set_value("", window, cx);
            state.set_masked(true, window, cx);
        });
    }

    /// Attach the duplex STT backend and pump its events into the controller.
    ///
    /// Each received event updates live take state and re-renders once, so the
    /// caption well grows token by token while the take is running.
    pub fn connect_backend(&mut self, sink: Box<dyn SttSessionSink>, cx: &mut Context<Self>) {
        let events = sink.events();
        self.controller.set_sink(sink);
        if let Some(events) = events {
            self.start_event_loop(events, cx);
        }
        cx.notify();
    }

    /// Spawns the worker the persisted settings resolve, and attaches it.
    ///
    /// The startup path: the settings store has answered, so the runtime
    /// override, `PATH` and the well-known locations are all available. A
    /// failure is recorded so Settings can name it; it never prevents the
    /// widget from opening.
    fn attach_backend_on_startup(&mut self, cx: &mut Context<Self>) {
        match (self.backend_spawn)(&self.stored_settings) {
            Ok(backend) => self.attach_backend(backend, true, cx),
            Err(failure) => self.record_backend_failure(failure),
        }
    }

    /// Retries the spawn after a save that supplied a usable runtime.
    ///
    /// Called only while [`SessionController::backend_unavailable`] holds: no
    /// worker is attached, or the attached one has died. A dead worker is
    /// replaced because it can never answer again; a healthy one is not,
    /// because replacing it would drop the take it is running. A failed retry
    /// records the new failure and leaves the dialog, and the user's
    /// correction, on screen.
    fn respawn_backend(&mut self, cx: &mut Context<Self>) -> bool {
        match (self.backend_spawn)(&self.stored_settings) {
            Ok(backend) => {
                self.attach_backend(backend, false, cx);
                true
            }
            Err(failure) => {
                self.record_backend_failure(failure);
                false
            }
        }
    }

    /// Makes `backend` the controller's sink and asks it for the catalogue.
    ///
    /// `from_startup` decides how the first catalogue is reported: the startup
    /// spawn owns the healthy-launch marker, a save respawn must not claim one.
    fn attach_backend(
        &mut self,
        backend: Box<dyn SttSessionSink>,
        from_startup: bool,
        cx: &mut Context<Self>,
    ) {
        self.catalogue_from_startup = from_startup;
        // Exactly one pump owns the event stream: detach the previous one even
        // when the replacement has no inbound events.
        self._event_task = None;
        self.connect_backend(backend, cx);
        self.controller.clear_backend_failure();
        self.controller.request_capabilities();
    }

    fn record_backend_failure(&mut self, failure: BackendFailure) {
        self.controller.note_backend_failure(failure);
        self.controller.report_error(AppError::BackendUnavailable);
    }

    /// Opens Settings once per launch when the widget cannot work.
    ///
    /// The controller owns the one-shot decision and names the action that is
    /// missing; the dialog seeds its rows from the stored settings, and its
    /// failure note says what is wrong.
    fn open_settings_if_startup_requires(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(action) = self.controller.check_startup_settings() else {
            return;
        };
        crate::diagnostics::log_settings_recovery(match action {
            StartupSettingsAction::BunPath => "bun-missing",
            StartupSettingsAction::Backend => "backend-unavailable",
            StartupSettingsAction::ApiKey => "api-key-missing",
        });
        self.open_settings(window, cx);
    }

    /// Own the backend event receiver. The task lives as long as the view;
    /// dropping it cancels the pump.
    fn start_event_loop(&mut self, events: SttEventReceiver, cx: &mut Context<Self>) {
        // Exactly one pump owns the event stream; replacing the backend cancels
        // the pump attached to the previous one.
        self._event_task = None;
        self._event_task = Some(cx.spawn(async move |this, cx| {
            while let Ok(message) = events.recv().await {
                let updated = this.update(cx, |this, cx| {
                    match message {
                        Ok(event) => {
                            if !this.controller.catalogue_loaded()
                                && let SttEvent::Capabilities { providers, .. } = &event
                            {
                                if this.catalogue_from_startup {
                                    crate::diagnostics::log_catalogue_loaded(providers.len());
                                } else {
                                    crate::diagnostics::log_catalogue_respawned(providers.len());
                                }
                            }
                            this.controller.handle_stt_event(event);
                        }
                        Err(error) => this.controller.report_error(AppError::from_backend(&error)),
                    }
                    // A `Stopped` event refines the finished take, so the
                    // automatic copy/paste of a stopped take runs here.
                    this.flush_auto_paste();
                    cx.notify();
                });
                if updated.is_err() {
                    break;
                }
            }
            // A closed stream means the worker exited (or never started).
            // Surfacing it keeps a dead backend actionable instead of silent.
            let _ = this.update(cx, |this, cx| {
                // A dead worker will never send the final `Stopped`: the take
                // that asked for a copy/paste still gets the text there is.
                this.flush_auto_paste();
                this.controller
                    .note_backend_failure(BackendFailure::Unavailable);
                this.controller.report_error(AppError::BackendUnavailable);
                cx.notify();
            });
        }));
    }

    /// Start the capture thread and its UI pump. Pause keeps the stream alive
    /// and relies on the controller dropping frames; finish/new take stop and
    /// drop the source so the device is released.
    fn start_audio_capture(&mut self, cx: &mut Context<Self>) {
        if self.audio_stop.is_some() {
            return;
        }

        let source = match self.audio.take() {
            Some(source) => Some(source),
            None => match self.audio_factory.as_mut() {
                Some(factory) => match factory() {
                    Ok(source) => Some(source),
                    Err(error) => {
                        self.controller.report_error(AppError::from_audio(&error));
                        None
                    }
                },
                None => None,
            },
        };
        let Some(mut source) = source else {
            return;
        };

        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let (sender, receiver) =
            speechtt_core::async_channel::unbounded::<Result<PcmChunk, AudioError>>();
        self.audio_thread = std::thread::Builder::new()
            .name("speechtt-audio".to_owned())
            .spawn(move || {
                while !thread_stop.load(Ordering::Relaxed) {
                    match source.next_chunk() {
                        Ok(Some(chunk)) => {
                            if sender.send_blocking(Ok(chunk)).is_err() {
                                break;
                            }
                        }
                        Ok(None) => break,
                        Err(error) => {
                            let _ = sender.send_blocking(Err(error));
                            break;
                        }
                    }
                }
            })
            .ok();
        self.audio_stop = Some(stop);

        self._audio_task = Some(cx.spawn(async move |this, cx| {
            while let Ok(message) = receiver.recv().await {
                let updated = this.update(cx, |this, cx| {
                    match message {
                        Ok(chunk) => {
                            let samples: Vec<f32> = chunk
                                .samples
                                .iter()
                                .map(|sample| f32::from(*sample) / f32::from(i16::MAX))
                                .collect();
                            let bars = this.analyzer.bars(&samples);
                            this.controller.push_audio_chunk(&chunk.samples, bars);
                            this.controller.refresh_elapsed(this.clock.now());
                        }
                        Err(error) => this.controller.report_error(AppError::from_audio(&error)),
                    }
                    cx.notify();
                });
                if updated.is_err() {
                    break;
                }
            }
        }));
    }

    fn stop_audio_capture(&mut self) {
        if let Some(stop) = self.audio_stop.take() {
            stop.store(true, Ordering::Relaxed);
        }
        self._audio_task = None;
        // The capture call returns after at most one chunk (~40 ms) on a live
        // device, so joining here cannot stall the UI for long.
        if let Some(handle) = self.audio_thread.take() {
            let _ = handle.join();
        }
    }

    // ----- Window geometry ---------------------------------------------

    /// Default geometry used when nothing usable has been persisted.
    fn default_window_layout() -> WindowLayout {
        WindowLayout::new(
            crate::tokens::WIDGET_WIDTH as u32,
            crate::tokens::WIDGET_HEIGHT as u32,
            0,
            0,
        )
    }

    /// Records the live window geometry, debounced through the clock loop.
    ///
    /// Maximized/fullscreen windows are ignored: restoring a maximized flag is
    /// out of scope, and the windowed rectangle stays the useful one.
    fn observe_window_bounds(&mut self, window: &mut Window) {
        let WindowBounds::Windowed(bounds) = window.window_bounds() else {
            return;
        };

        // Not every window manager honours the min-size hint, and the window
        // must never be smaller than the content it holds (the chooser's two
        // cards would truncate and collide with the description). Push the
        // window back up to the floor and record the clamped size, so the
        // persisted geometry can never fall below it either.
        let minimum = crate::minimum_window_size();
        let size = Size {
            width: bounds.size.width.max(minimum.width),
            height: bounds.size.height.max(minimum.height),
        };
        if size != bounds.size {
            window.resize(size);
        }

        let layout = WindowLayout::new(
            f32::from(size.width).round() as u32,
            f32::from(size.height).round() as u32,
            f32::from(bounds.origin.x).round() as i32,
            f32::from(bounds.origin.y).round() as i32,
        );
        self.remember_window_layout(layout, self.clock.now());
    }

    /// Marks `layout` as pending; it is written once stable for 600 ms.
    ///
    /// A geometry identical to the stored one clears the pending change, and
    /// the untouched default is not persisted at all, so a first launch never
    /// creates a settings file just because the window was rendered.
    pub fn remember_window_layout(&mut self, layout: WindowLayout, now: Instant) {
        let default = Self::default_window_layout();
        if self.stored_settings.window == Some(layout)
            || (self.stored_settings.window.is_none() && layout == default)
        {
            self.pending_window_layout = None;
            return;
        }
        if self.pending_window_layout.map(|(pending, _)| pending) != Some(layout) {
            self.pending_window_layout = Some((layout, now));
        }
    }

    /// Persists the pending geometry once it has been stable for 600 ms.
    ///
    /// Returns `true` when the store was written. Failures are silent: a
    /// remembered window position must never surface an error strip.
    pub fn flush_pending_window_layout(&mut self, now: Instant) -> bool {
        const QUIET: Duration = Duration::from_millis(600);
        let Some((layout, since)) = self.pending_window_layout else {
            return false;
        };
        if now.checked_duration_since(since).unwrap_or_default() < QUIET {
            return false;
        }

        let mut next = self.stored_settings.clone();
        next.window = Some(layout);
        match self.settings_store.save(&next) {
            Ok(()) => {
                self.stored_settings = next;
                self.pending_window_layout = None;
                true
            }
            Err(_) => false,
        }
    }
}

impl gpui_kit::Render for SpeechTtApp {
    fn render(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl gpui_kit::IntoElement {
        // The setup form is the only screen that shows these menus, so the
        // catalogue snapshot is rebuilt only while it is on screen.
        if self.view_state() == ViewState::Form {
            let snapshot = SelectSnapshot {
                providers: self.controller.draft_provider_options(),
                selected_provider: self.controller.draft_provider().map(str::to_string),
                models: self.controller.draft_model_options(),
                selected_model: self.controller.draft_model().map(str::to_string),
            };
            self.draft_selects.sync(snapshot, window, cx);
        }

        // The take screen owns the model picker, so its snapshot is rebuilt only
        // while that screen is on show.
        if self.view_state() == ViewState::Live {
            let options = self.controller.live_model_options();
            let selected = self.controller.live_model_id().map(str::to_owned);
            self.live_model_field
                .sync(options, selected.as_deref(), window, cx);
        }

        // The root declares the window fact plus exactly one state fact, rebuilt
        // from the controller every frame. The keymap registers each command
        // under the predicate that names the state where it can act, so a
        // keystroke only matches on the screen that can honour it: Space and
        // Enter on the live take, Ctrl+S on a finished transcript. The facts are
        // mutually exclusive because one `match` chooses them.
        let key_context = match self.view_state() {
            ViewState::Live => SPEECHTT_TAKE_CONTEXT,
            ViewState::SessionEnd => SPEECHTT_SESSION_END_CONTEXT,
            _ => SPEECHTT_CONTEXT,
        };

        let mut root = v_flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .track_focus(&self.focus)
            .key_context(key_context)
            .on_action(cx.listener(Self::on_copy_selection))
            .on_action(cx.listener(Self::on_toggle_recording))
            .on_action(cx.listener(Self::on_stop_recording))
            .on_action(cx.listener(Self::on_toggle_help))
            .on_action(cx.listener(Self::on_toggle_settings))
            .on_action(cx.listener(Self::on_save_transcript));

        root = root.child(
            AppHeader::new(APP_TITLE)
                .version(concat!("V", env!("CARGO_PKG_VERSION")))
                .action(
                    IconButton::new("btn-help", IconName::CircleQuestionMark)
                        .label("Open help and shortcuts")
                        .on_click(
                            cx.listener(|this, _event, window, cx| this.open_help(window, cx)),
                        ),
                )
                .action(
                    IconButton::new("btn-settings", IconName::Settings)
                        .label("Open settings")
                        .on_click(
                            cx.listener(|this, _event, window, cx| this.open_settings(window, cx)),
                        ),
                ),
        );

        // The error strip sits above the body and below the header, so it
        // never replaces or moves the transport controls.
        if let Some(strip) = self.render_error_strip(cx) {
            root = root.child(strip);
        }

        let state = self.view_state();
        let body = v_flex().flex_1().min_h_0().w_full().overflow_hidden();

        // Every screen fills the window body and owns its own header/body
        // inset through `ScreenShell`; footers are part of the screen so they
        // can run to the window edge.
        root = root.child(body.child(match state {
            ViewState::Mode => views::welcome::render(self, cx),
            ViewState::Form => views::session_setup::render(self, cx),
            ViewState::Live => views::recording::render(self, cx),
            ViewState::Transcription => views::session_transcription::render(self, cx),
            ViewState::SessionEnd => views::session_end::render(self, cx),
        }));

        // The press wave is painted last, above the screen. It is the button's
        // own colour, so the part over the button is invisible and only the
        // sliver that grows past the button edge reads — a wave emerging from
        // behind the button face. It belongs to the window rather than to the
        // pressed screen so it also outlives the navigation its click starts.
        if let Some(ripple) = self.ripple {
            root = root.child(RippleRing::new(ripple));
        }

        self.observe_window_bounds(window);
        root
    }
}

impl SpeechTtApp {
    fn render_error_strip(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let error = self.controller.app_error()?.clone();
        let action = error.action();

        let mut row = h_flex()
            .id("app-error-strip")
            .w_full()
            .flex_shrink_0()
            .items_center()
            .gap_2()
            .px_5()
            .py_1p5()
            .bg(cx.theme().secondary)
            .border_t_1()
            .border_b_1()
            .border_color(gpui_kit::rgb(color::DANGER))
            .child(
                div()
                    .size(px(6.))
                    .rounded(px(3.))
                    .bg(gpui_kit::rgb(color::DANGER)),
            )
            .child(div().flex_1().min_w_0().child(Label::new(error.message())));

        row = match action {
            ErrorAction::OpenSettings => row.child(
                Button::new("btn-error-settings")
                    .ghost()
                    .small()
                    .label("Open Settings")
                    .on_click(
                        cx.listener(|this, _event, window, cx| this.open_settings(window, cx)),
                    ),
            ),
            ErrorAction::Retry => row.child(
                Button::new("btn-error-retry")
                    .ghost()
                    .small()
                    .label("Retry")
                    .on_click(cx.listener(|this, _event, _window, cx| this.retry_backend(cx))),
            ),
            ErrorAction::SettingsHint => row.child(Label::new("Open settings").muted()),
            ErrorAction::Dismiss => row,
        };

        Some(
            row.child(
                Button::new("btn-error-dismiss")
                    .ghost()
                    .small()
                    .label("Dismiss")
                    .on_click(cx.listener(|this, _event, _window, cx| this.dismiss_error(cx))),
            )
            .into_any_element(),
        )
    }
}

/// The dialog sits beside the widget, preferring the right edge and falling
/// back to the left when the display runs out of room there.
fn dialog_window_bounds(dialog: Dialog, window: &mut Window, cx: &App) -> Bounds<Pixels> {
    const GAP: f32 = 16.;
    let width = px(DIALOG_WIDTH);
    let height = px(match dialog {
        Dialog::Help => HELP_DIALOG_HEIGHT,
        Dialog::Settings => SETTINGS_DIALOG_HEIGHT,
    });
    let main = window.bounds();
    let mut origin = Point {
        x: main.origin.x + main.size.width + px(GAP),
        y: main.origin.y,
    };

    if let Some(display) = window.display(cx) {
        let display = display.visible_bounds();
        if origin.x + width > display.origin.x + display.size.width {
            origin.x = main.origin.x - width - px(GAP);
        }
        origin.x = origin.x.max(display.origin.x);
        let max_y = display.origin.y + display.size.height - height;
        origin.y = origin.y.min(max_y).max(display.origin.y);
    }

    Bounds {
        origin,
        size: Size { width, height },
    }
}

impl Drop for SpeechTtApp {
    fn drop(&mut self) {
        if let Some(stop) = self.audio_stop.take() {
            stop.store(true, Ordering::Relaxed);
        }
    }
}

/// `$HOME/Documents`, falling back to the temp directory in hermetic builds.
fn documents_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(|home| PathBuf::from(home).join("Documents"))
        .unwrap_or_else(std::env::temp_dir)
}

/// Opens the default device on the first chunk, so constructing the widget
/// never touches audio hardware (the pump only runs during a take).
#[derive(Default)]
struct LazyCpalAudioSource {
    inner: Option<CpalAudioSource>,
}

impl AudioSource for LazyCpalAudioSource {
    fn next_chunk(&mut self) -> Result<Option<PcmChunk>, AudioError> {
        if self.inner.is_none() {
            self.inner = Some(CpalAudioSource::open_default()?);
        }
        let source = self
            .inner
            .as_mut()
            .ok_or_else(|| AudioError::Backend("capture source missing".to_owned()))?;
        source.next_chunk()
    }
}

/// The Settings a committed change produces from the stored file.
///
/// Every field of [`SettingsChange`] is copied here; a field the change carries
/// but this literal omits would be dropped on every save. Fields the dialog
/// does not edit (API keys other than the current provider's, the legacy
/// shortcut map, auto-paste, window geometry) are carried over from `stored`.
fn settings_after_change(change: &SettingsChange, stored: &Settings) -> Settings {
    let mut next = Settings {
        provider: change.provider.clone(),
        model_streaming: change.model_streaming.clone(),
        model_batch: change.model_batch.clone(),
        model: String::new(),
        language_model: change.language_model.clone().unwrap_or_default(),
        language: change.language.clone().unwrap_or_default(),
        api_keys: stored.api_keys.clone(),
        bun_executable: change.bun_executable.trim().to_owned(),
        shortcuts: stored.shortcuts.clone(),
        auto_paste: stored.auto_paste,
        window: stored.window,
    };
    if let Some(key) = &change.api_key {
        next.set_api_key(&change.provider, key.clone());
    }
    next
}

/// Resolves the configured `bun` and spawns the worker beside this executable.
///
/// Resolution order is [`bun_candidates`]'s: the persisted override (with a
/// leading `~` expanded), `PATH`, `$HOME/.bun/bin/bun`, then the system
/// locations. A configured path that exists without the execute bit is
/// [`BackendFailure::NotExecutable`] rather than a silent fall-through to
/// `PATH`: the user's explicit answer is wrong in a way only they can fix.
/// Called by [`SpeechTtApp::new`] at startup and by
/// [`SpeechTtApp::save_settings`] when a save supplied the missing runtime. A
/// missing runtime is reported with every candidate it searched.
fn spawn_backend(settings: &Settings) -> Result<BunSubprocess, BackendFailure> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let configured = configured_bun_path(&settings.bun_executable, home.as_deref());
    if let Some(path) = &configured
        && let Some(problem) = configured_path_problem(path, |path| path.is_file(), is_executable)
    {
        return Err(problem);
    }

    let searched = bun_candidates(configured.as_deref(), home.as_deref());
    let program = resolve_bun_in(
        configured.as_deref(),
        home.as_deref(),
        which_on_path,
        |path| path.is_file(),
    )
    .ok_or(BackendFailure::BunMissing { searched })?;

    BunSubprocess::spawn_default_with(program.to_string_lossy().into_owned(), |path| {
        path.is_file()
    })
    .map_err(|error| BackendFailure::from_backend_error(&error))
}

/// The `bun_executable` setting as a path, with a leading `~` expanded against
/// `home`.
///
/// A user copies the value from a shell prompt, where `~` is the shell's to
/// expand, so the widget must expand it too: `~/bin/bun` is never a literal
/// directory name. `None` means "resolve automatically" — the setting is empty
/// or whitespace, or it starts with `~` and there is no home directory to
/// expand it against.
fn configured_bun_path(configured: &str, home: Option<&Path>) -> Option<PathBuf> {
    let trimmed = configured.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed == "~" {
        return home.map(Path::to_path_buf);
    }
    if let Some(rest) = trimmed.strip_prefix("~/") {
        return home.map(|home| home.join(rest));
    }
    Some(PathBuf::from(trimmed))
}

/// Whether a configured path is a problem before any fallback is tried.
///
/// `None` when the path may be used, or when it does not exist at all: a
/// configured value that is simply absent falls through to the automatic
/// lookup, exactly as an empty setting does. `Some` only when the user's
/// explicit answer is present but cannot be executed, which no fallback should
/// hide. Both lookups are parameters so the rule is testable without a
/// filesystem.
fn configured_path_problem(
    path: &Path,
    exists: impl Fn(&Path) -> bool,
    executable: impl Fn(&Path) -> bool,
) -> Option<BackendFailure> {
    (exists(path) && !executable(path)).then(|| BackendFailure::NotExecutable {
        path: path.to_path_buf(),
    })
}

/// Whether `path` is a file the OS can execute.
///
/// On Unix that is any execute bit; elsewhere the extension rule belongs to the
/// OS, so a regular file is the most this can prove without starting it.
#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::metadata(path)
        .map(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

/// Stand-in backend when the Bun worker cannot be spawned.
struct UnavailableSink;

impl SttSessionSink for UnavailableSink {
    fn send(&mut self, _cmd: speechtt_core::protocol::SttCommand) -> Result<(), BackendError> {
        Err(BackendError::Disconnected)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        configured_bun_path, configured_path_problem, copyable_selection, settings_after_change,
    };
    use crate::controller::{BackendFailure, SettingsChange};
    use speechtt_core::settings::Settings;
    use std::path::{Path, PathBuf};

    /// A path copied from a shell prompt starts with `~`, which the shell would
    /// have expanded. The widget must resolve it against `$HOME` itself,
    /// because no shell runs between the saved setting and the spawn.
    #[test]
    fn a_configured_path_expands_a_leading_tilde() {
        let home = Path::new("/home/ada");

        assert_eq!(
            configured_bun_path("~/bin/bun", Some(home)),
            Some(PathBuf::from("/home/ada/bin/bun"))
        );
        assert_eq!(
            configured_bun_path("  ~/.bun/bin/bun  ", Some(home)),
            Some(PathBuf::from("/home/ada/.bun/bin/bun")),
            "the setting is trimmed before the expansion"
        );
        assert_eq!(
            configured_bun_path("~", Some(home)),
            Some(PathBuf::from("/home/ada"))
        );
        assert_eq!(
            configured_bun_path("~/bin/bun", None),
            None,
            "without a home directory there is nothing to expand, so the lookup goes automatic"
        );
    }

    /// An absolute or relative path without a tilde is used verbatim; the
    /// empty setting means "resolve automatically".
    #[test]
    fn a_configured_path_without_a_tilde_is_used_as_written() {
        assert_eq!(
            configured_bun_path("/opt/bun/bin/bun", Some(Path::new("/home/ada"))),
            Some(PathBuf::from("/opt/bun/bin/bun"))
        );
        assert_eq!(
            configured_bun_path("bin/bun", None),
            Some(PathBuf::from("bin/bun"))
        );
        assert_eq!(configured_bun_path("", Some(Path::new("/home/ada"))), None);
        assert_eq!(configured_bun_path("  \t ", None), None);
    }

    /// A configured path that is present but cannot be executed is reported
    /// instead of falling through to `PATH`, because falling through would tell
    /// the user "not found" about a file they can see.
    #[test]
    fn a_configured_path_is_reported_when_it_exists_but_is_not_executable() {
        let path = Path::new("/home/ada/bin/bun");

        assert_eq!(
            configured_path_problem(path, |_| true, |_| false),
            Some(BackendFailure::NotExecutable {
                path: PathBuf::from("/home/ada/bin/bun")
            })
        );
        assert_eq!(
            configured_path_problem(path, |_| true, |_| true),
            None,
            "an executable path is usable"
        );
        assert_eq!(
            configured_path_problem(path, |_| false, |_| false),
            None,
            "an absent path falls through to the automatic lookup"
        );
    }

    /// A stray shortcut with nothing selected must not empty the clipboard.
    #[test]
    fn an_empty_selection_leaves_the_clipboard_alone() {
        assert_eq!(copyable_selection(""), None);
    }

    /// The §2 trap: a field `SettingsChange` carries but the rebuild omits is
    /// dropped on every save. The bun override must survive, trimmed, with the
    /// fields the dialog does not edit carried over.
    #[test]
    fn a_committed_change_keeps_the_bun_executable_and_the_untouched_fields() {
        let stored = Settings {
            auto_paste: false,
            language: "fr".to_owned(),
            ..Settings::default()
        };
        let change = SettingsChange {
            provider: "vercel-ai-gateway".to_owned(),
            model_streaming: "openai/gpt-realtime-whisper".to_owned(),
            model_batch: "openai/gpt-4o-transcribe".to_owned(),
            language: Some("de".to_owned()),
            language_model: None,
            bun_executable: "  /home/ada/.bun/bin/bun  ".to_owned(),
            api_key: None,
        };

        let next = settings_after_change(&change, &stored);

        assert_eq!(next.bun_executable, "/home/ada/.bun/bin/bun");
        assert_eq!(next.language, "de");
        assert!(!next.auto_paste, "a field the dialog does not edit is kept");
        assert_eq!(next.api_keys, stored.api_keys);
    }

    /// Whitespace is a selection the user cannot see, so it never replaces the
    /// clipboard either.
    #[test]
    fn a_blank_selection_leaves_the_clipboard_alone() {
        assert_eq!(copyable_selection(" "), None);
        assert_eq!(copyable_selection("\t"), None);
        assert_eq!(copyable_selection("\n"), None);
        assert_eq!(copyable_selection(" \t\r\n "), None);
    }

    /// The copy is the highlight, character for character: the whitespace a
    /// drag happens to include stays part of the copied text.
    #[test]
    fn a_selected_sentence_is_copied_exactly_as_highlighted() {
        let selection = "  Hello, world — three, two.  ";
        assert_eq!(copyable_selection(selection), Some(selection));
    }

    /// Whitespace-only is the line, and punctuation is not whitespace: a single
    /// dash is a real selection.
    #[test]
    fn punctuation_alone_is_a_real_selection() {
        assert_eq!(copyable_selection("…?!—"), Some("…?!—"));
        assert_eq!(copyable_selection("."), Some("."));
    }
}
