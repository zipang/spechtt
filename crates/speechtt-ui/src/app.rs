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
//! This module owns the window shell, the workflow state and the public
//! command/query surface. Screen composition lives in [`crate::views`] and
//! reusable controls in [`crate::components`].

use gpui_kit::assets::IconName;
use gpui_kit::base::{h_flex, v_flex};
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::button::ButtonVariants as _;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::{Sizable as _, button::Button};
use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, AnyWindowHandle, App, Bounds, Context, Entity, PathPromptOptions, Pixels, Point,
    Size, Subscription, Task, Window, WindowBounds, WindowId, WindowKind, WindowOptions, div, px,
};
use speechtt_core::audio::{AudioError, AudioSource, CpalAudioSource, PcmChunk};
use speechtt_core::backend::{BackendError, BunSubprocess, SttEventReceiver, SttSessionSink};
use speechtt_core::fft::SpectrumAnalyzer;
use speechtt_core::hotkeys::{
    GlobalHotkeyListener, HotkeyError, SessionAction, ShortcutListener, bindings_from_settings,
};
use speechtt_core::output::clipboard::{ClipboardSink, SystemClipboard};
use speechtt_core::output::file::{FileOpener, SystemOpener, suggested_transcript_name};
use speechtt_core::output::paste::{PasteSink, SystemPaste};
use speechtt_core::session::{SessionField, SessionState};
use speechtt_core::settings::{JsonFileStore, Settings, SettingsStore, WindowLayout};

use crate::components::{AppHeader, IconButton, Label, Ripple, RippleRing};
use crate::controller::{
    AppError, Clock, Dialog, ErrorAction, FinishedTake, HelpTab, SAMPLE_RATE, SessionController,
    SessionMode, SettingsChange, SettingsDraft, SystemClock, ViewState,
};
use crate::tokens::color;
use crate::views;
use crate::views::dialogs::{
    DIALOG_WIDTH, DialogWindow, HELP_DIALOG_HEIGHT, SETTINGS_DIALOG_HEIGHT,
};

/// Application title shown in the header strip.
pub const APP_TITLE: &str = "SpeechTT";

/// Every side effect the widget needs, injected as a single composition root.
///
/// Production is built by [`SpeechTtApp::new`]; headless tests build the same
/// struct with fakes so no microphone, display server, clipboard, editor,
/// filesystem or live provider is required.
pub struct AppServices {
    pub audio: Box<dyn AudioSource>,
    pub analyzer: SpectrumAnalyzer,
    pub sink: Box<dyn SttSessionSink>,
    pub events: SttEventReceiver,
    pub settings: Box<dyn SettingsStore>,
    pub clipboard: Box<dyn ClipboardSink>,
    pub paste: Box<dyn PasteSink>,
    pub opener: Box<dyn FileOpener>,
    pub listener: Box<dyn ShortcutListener>,
    pub clock: Box<dyn Clock>,
}

type AudioFactory = Box<dyn FnMut() -> Result<Box<dyn AudioSource>, AudioError> + Send>;

/// Root view of the widget window.
pub struct SpeechTtApp {
    controller: SessionController,
    /// Monotonic click counters for round-button ripples, keyed by element id.
    /// `u64` because the view only needs "this changed", not a timestamp.
    /// The press ripple, drawn at the window root so it survives navigation.
    ripple: Option<Ripple>,
    ripple_seq: u64,
    settings_api_key: Entity<InputState>,
    settings_key_revealed: bool,
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
    listener: Box<dyn ShortcutListener>,
    clock: Box<dyn Clock>,
    stored_settings: Settings,
    /// Geometry seen on screen, waiting for the quiet period before persisting.
    pending_window_layout: Option<(WindowLayout, Instant)>,
}

impl SpeechTtApp {
    /// Production composition root: real capture, duplex Bun backend,
    /// settings file, clipboard/paste/opener and global hotkeys.
    ///
    /// A missing Bun binary or hotkey backend never prevents the widget from
    /// opening; the failure is surfaced as a typed error strip instead.
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (sink, events, backend_failed) = match BunSubprocess::spawn_default() {
            Ok(backend) => {
                let events = backend.events();
                (Box::new(backend) as Box<dyn SttSessionSink>, events, false)
            }
            Err(_) => {
                let (_sender, receiver) = speechtt_core::async_channel::unbounded();
                (
                    Box::new(UnavailableSink) as Box<dyn SttSessionSink>,
                    receiver,
                    true,
                )
            }
        };

        let listener: Box<dyn ShortcutListener> = match GlobalHotkeyListener::new() {
            Ok(listener) => Box::new(listener),
            Err(_) => Box::new(NoopShortcutListener),
        };

        let factory: AudioFactory =
            Box::new(|| Ok(Box::new(CpalAudioSource::open_default()?) as Box<dyn AudioSource>));
        let services = AppServices {
            audio: Box::new(LazyCpalAudioSource::default()),
            analyzer: SpectrumAnalyzer::new(SAMPLE_RATE),
            sink,
            events,
            settings: Box::new(JsonFileStore::new(JsonFileStore::default_path())),
            clipboard: Box::new(SystemClipboard::new()),
            paste: Box::new(SystemPaste::new()),
            opener: Box::new(SystemOpener::new()),
            listener,
            clock: Box::new(SystemClock),
        };

        let mut app = Self::with_services_and_factory(services, Some(factory), window, cx);
        if backend_failed {
            app.controller.report_error(AppError::BackendUnavailable);
        }
        app
    }

    /// Build the widget with injected services (the test seam).
    pub fn with_services(
        services: AppServices,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::with_services_and_factory(services, None, window, cx)
    }

    fn with_services_and_factory(
        services: AppServices,
        audio_factory: Option<AudioFactory>,
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
            controller,
            ripple: None,
            ripple_seq: 0,
            settings_api_key,
            settings_key_revealed: false,
            dialog_window: None,
            dialog_window_id,
            pending_settings: None,
            _subscriptions: vec![settings_subscription, window_closed],
            _event_task: None,
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
            listener: services.listener,
            clock: services.clock,
            stored_settings,
            pending_window_layout: None,
        };

        if let Some(error) = settings_error {
            app.controller.report_error(AppError::from_settings(&error));
        }
        app.register_hotkeys();
        app.start_event_loop(services.events, cx);
        app.start_clock_loop(cx);
        app.controller.request_capabilities();
        app
    }

    fn register_hotkeys(&mut self) {
        let bindings = match bindings_from_settings(&self.stored_settings) {
            Ok(bindings) => bindings,
            Err(error) => {
                self.controller
                    .report_error(AppError::ProtocolError(format!(
                        "invalid shortcut settings: {error}"
                    )));
                return;
            }
        };
        let pairs: Vec<_> = bindings.into_iter().collect();
        if let Err(error) = self.listener.register(&pairs) {
            self.controller
                .report_error(AppError::ProtocolError(format!("hotkeys: {error}")));
        }
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

    pub fn elapsed_seconds(&self) -> u32 {
        self.controller.elapsed_seconds()
    }

    pub fn app_error(&self) -> Option<&AppError> {
        self.controller.app_error()
    }

    pub fn written_path(&self) -> Option<&std::path::Path> {
        self.controller.written_path()
    }

    pub fn active_dialog(&self) -> Option<Dialog> {
        self.controller.active_dialog()
    }

    pub fn settings_draft(&self) -> &SettingsDraft {
        self.controller.settings_draft()
    }

    pub fn settings_connected(&self) -> bool {
        self.controller.settings_connected()
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

    pub fn settings_key_revealed(&self) -> bool {
        self.settings_key_revealed
    }

    /// Read access used by the screen renderers.
    pub(crate) fn controller(&self) -> &SessionController {
        &self.controller
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

    pub fn cycle_provider(&mut self, cx: &mut Context<Self>) {
        self.controller.cycle_provider();
        cx.notify();
    }

    pub fn cycle_model(&mut self, cx: &mut Context<Self>) {
        self.controller.cycle_model();
        cx.notify();
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
            self.start_audio_capture(cx);
        }
        cx.notify();
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
        // modes, so the user names the transcript.
        let _ = self
            .controller
            .auto_paste_after_finish(&mut *self.clipboard, &mut *self.paste);
        cx.notify();
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

    /// Apply a hotkey action exactly like its button, including opening the
    /// dialog windows for the toggle actions.
    pub fn apply_action(
        &mut self,
        action: SessionAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        match action {
            SessionAction::ToggleHelp => {
                self.open_help(window, cx);
                true
            }
            SessionAction::ToggleSettings => {
                self.open_settings(window, cx);
                true
            }
            _ => {
                let applied = self.controller.apply_action(action);
                if applied {
                    match action {
                        SessionAction::StartRecording => self.start_audio_capture(cx),
                        SessionAction::StopRecording | SessionAction::NewTake => {
                            self.stop_audio_capture();
                        }
                        _ => {}
                    }
                }
                cx.notify();
                applied
            }
        }
    }

    /// Copy the finished take through the injected clipboard adapter.
    pub fn copy_transcript(&mut self, cx: &mut Context<Self>) -> bool {
        let copied = self
            .controller
            .copy_finished_transcript(&mut *self.clipboard)
            .is_ok();
        cx.notify();
        copied
    }

    /// Ask where to write the finished take, with a suggested name: the source
    /// file's stem for a file take, a timestamped `speechtt-…` for dictation.
    pub fn save_transcript(&mut self, cx: &mut Context<Self>) {
        let prefix = self
            .controller
            .suggested_save_name()
            .unwrap_or_else(|| "speechtt".to_owned());
        let suggested = suggested_transcript_name(&prefix, self.clock.system_now());
        let selection = cx.prompt_for_new_path(&documents_dir(), Some(&suggested));

        cx.spawn(async move |this, cx| {
            match selection.await {
                Ok(Ok(Some(path))) => {
                    let _ = this.update(cx, |this, cx| {
                        this.save_transcript_to(&path, cx);
                    });
                }
                // The user canceled the dialog.
                Ok(Ok(None)) => {}
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
        self.close_dialog_window(cx);
        self.controller.open_help();
        self.request_dialog_window(Dialog::Help, window, cx);
        cx.notify();
    }

    /// Opens Settings in its own floating window beside the widget.
    pub fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_dialog_window(cx);
        if !self.stored_settings.model.trim().is_empty() {
            let provider = self.stored_settings.provider.clone();
            let model = self.stored_settings.model.clone();
            let has_key = self
                .stored_settings
                .api_key(&self.stored_settings.provider)
                .is_some();
            self.controller
                .open_settings_with(Some(&provider), Some(&model), has_key);
        } else {
            self.controller.open_settings();
        }
        // Refresh the provider/model catalogue each time the dialog opens so
        // a key exported after startup appears as connected.
        self.controller.request_capabilities();
        self.settings_key_revealed = false;
        self.settings_api_key.update(cx, |state, cx| {
            state.set_value("", window, cx);
            state.set_masked(true, window, cx);
        });
        self.request_dialog_window(Dialog::Settings, window, cx);
        cx.notify();
    }

    /// Cancel semantics: discard the draft. The dialog window closes itself.
    pub fn close_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.controller.close_dialog();
        self.clear_settings_key(window, cx);
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
    fn request_dialog_window(
        &mut self,
        dialog: Dialog,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let app = cx.entity();
        let main_window = window.window_handle();
        cx.defer(move |cx| {
            if app.update(cx, |app, _| app.active_dialog()) != Some(dialog) {
                return;
            }

            let title = views::dialogs::window_title(dialog);
            let app_for_window = app.clone();
            let opened = main_window.update(cx, |_, window, cx| {
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

    pub fn cycle_settings_provider(&mut self, cx: &mut Context<Self>) {
        self.controller.cycle_settings_provider();
        cx.notify();
    }

    pub fn cycle_settings_model(&mut self, cx: &mut Context<Self>) {
        self.controller.cycle_settings_model();
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

    /// Commits the Settings draft. Returns `false` and leaves the dialog open
    /// when the draft is incomplete.
    pub fn save_settings(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(change) = self.controller.save_settings() else {
            return false;
        };
        self.apply_settings_change(change);
        cx.notify();
        true
    }

    /// Persist a committed Settings draft and make it current for the next
    /// take. The draft key is written only to the store; it never appears in
    /// `pending_settings` debug output because `SettingsChange` redacts it.
    pub fn apply_settings_change(&mut self, change: SettingsChange) {
        let provider = change.provider.clone();
        let next = Settings {
            provider: change.provider.clone(),
            model: change.model.clone(),
            api_keys: self.stored_settings.api_keys.clone(),
            shortcuts: self.stored_settings.shortcuts.clone(),
            auto_paste: self.stored_settings.auto_paste,
            window: self.stored_settings.window,
        };
        let mut next = next;
        if let Some(key) = &change.api_key {
            next.set_api_key(&provider, key.clone());
        }

        self.pending_settings = Some(change);

        match self.settings_store.save(&next) {
            Ok(()) => {
                self.stored_settings = next.clone();
                self.controller.apply_settings(next);
                self.controller.dismiss_error();
                // Re-run the handshake so the newly saved key flips the
                // provider to CONNECTED without reopening the app.
                self.controller.request_capabilities();
            }
            Err(error) => {
                self.controller
                    .report_error(AppError::from_settings(&error));
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
                        Ok(event) => this.controller.handle_stt_event(event),
                        Err(error) => this.controller.report_error(AppError::from_backend(&error)),
                    }
                    cx.notify();
                });
                if updated.is_err() {
                    break;
                }
            }
            // A closed stream means the worker exited (or never started).
            // Surfacing it keeps a dead backend actionable instead of silent.
            let _ = this.update(cx, |this, cx| {
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
        let mut root = v_flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground);

        root = root.child(
            AppHeader::new(APP_TITLE)
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
        self.listener.unregister_all();
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

/// Stand-in backend when the Bun worker cannot be spawned.
struct UnavailableSink;

impl SttSessionSink for UnavailableSink {
    fn send(&mut self, _cmd: speechtt_core::protocol::SttCommand) -> Result<(), BackendError> {
        Err(BackendError::Disconnected)
    }
}

/// Hotkey fallback when the OS manager is unavailable (headless sessions).
struct NoopShortcutListener;

impl ShortcutListener for NoopShortcutListener {
    fn register(
        &mut self,
        _shortcuts: &[(SessionAction, speechtt_core::hotkeys::Shortcut)],
    ) -> Result<(), HotkeyError> {
        Ok(())
    }

    fn unregister_all(&mut self) {}
}
