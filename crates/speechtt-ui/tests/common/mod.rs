//! Test fixture: open a production view in a headless window with
//! deterministic test bounds. Mirrors gpui-kit's own `tests/common`.
//!
//! The SpeechTT fixtures inject hermetic [`AppServices`]: an in-memory
//! settings store, silent audio, a no-op backend/clipboard/paste/opener and, by
//! default, a [`ScriptedSavePrompt`] that cancels every save request, so no
//! fixture opens the platform save dialog unless a test asks for it. A test
//! that exercises `Ctrl+S` supplies its own prompt through
//! [`open_speechtt_window_with_save_prompt`]; only
//! [`open_speechtt_window_with_events`] keeps the production adapter, for the
//! one test that answers the platform dialog with
//! `simulate_new_path_selection`. No test reads or writes the user's real
//! config, spawns Bun, or opens audio/display hardware.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use gpui_kit::{
    App, AppContext, Bounds, Entity, Pixels, Point, Render, Size, Task, TestAppContext, Window,
    WindowBounds, WindowHandle, base::Root, px, size,
};
use speechtt_core::audio::{AudioError, AudioSource, PcmChunk};
use speechtt_core::backend::{BackendError, SttSessionSink};
use speechtt_core::fft::SpectrumAnalyzer;
use speechtt_core::output::OutputError;
use speechtt_core::output::clipboard::ClipboardSink;
use speechtt_core::output::file::FileOpener;
use speechtt_core::output::paste::PasteSink;
use speechtt_core::protocol::{
    ModelCapabilityInfo, ProviderCapability, SttCommand, TranscriptionMode,
};
use speechtt_core::settings::{Settings, SettingsError, SettingsStore};
use speechtt_ui::app::{AppServices, GpuiSavePrompt, SaveAnswer, SavePrompt, SpeechTtApp};
use speechtt_ui::controller::{SAMPLE_RATE, SystemClock};

/// In-memory settings store. The initial key makes the widget's recording
/// flow passes the missing-key guard without touching `$HOME`.
#[derive(Clone)]
struct TestSettingsStore {
    settings: Arc<Mutex<Settings>>,
    path: PathBuf,
}

impl TestSettingsStore {
    fn new() -> Self {
        let mut settings = Settings {
            provider: "vercel".to_owned(),
            model: String::new(),
            auto_paste: false,
            ..Settings::default()
        };
        settings.set_api_key("vercel", "sk-hermetic-test");
        Self {
            settings: Arc::new(Mutex::new(settings)),
            path: PathBuf::from("/tmp/speechtt-headless-settings.json"),
        }
    }
}

impl SettingsStore for TestSettingsStore {
    fn load(&self) -> Result<Settings, SettingsError> {
        Ok(self.settings.lock().expect("test settings lock").clone())
    }

    fn save(&self, settings: &Settings) -> Result<(), SettingsError> {
        *self.settings.lock().expect("test settings lock") = settings.clone();
        Ok(())
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

struct SilentAudio;

impl AudioSource for SilentAudio {
    fn next_chunk(&mut self) -> Result<Option<PcmChunk>, AudioError> {
        Ok(None)
    }
}

struct NoopSink;

impl SttSessionSink for NoopSink {
    fn send(&mut self, _cmd: SttCommand) -> Result<(), BackendError> {
        Ok(())
    }
}

struct NoopClipboard;

impl ClipboardSink for NoopClipboard {
    fn copy(&mut self, _text: &str) -> Result<(), OutputError> {
        Ok(())
    }
}

/// A clipboard that keeps every string the app copies.
///
/// Handed back by [`open_speechtt_window_with_channels`] so a test can assert
/// the payload of Copy and of the automatic paste.
#[derive(Clone, Default)]
pub struct RecordingClipboard {
    copies: Arc<Mutex<Vec<String>>>,
}

impl RecordingClipboard {
    /// Every string copied so far, in order.
    pub fn copies(&self) -> Vec<String> {
        self.copies.lock().expect("clipboard lock").clone()
    }
}

impl ClipboardSink for RecordingClipboard {
    fn copy(&mut self, text: &str) -> Result<(), OutputError> {
        self.copies
            .lock()
            .expect("clipboard lock")
            .push(text.to_owned());
        Ok(())
    }
}

/// A backend sink that keeps every command the app sends.
///
/// Handed back by [`open_speechtt_window_with_channels`], so a window test can
/// assert what reached the worker without spawning one.
#[derive(Clone, Default)]
pub struct RecordingSink {
    commands: Arc<Mutex<Vec<SttCommand>>>,
}

impl RecordingSink {
    /// Every command sent so far, in order.
    pub fn commands(&self) -> Vec<SttCommand> {
        self.commands.lock().expect("sink lock").clone()
    }
}

impl SttSessionSink for RecordingSink {
    fn send(&mut self, cmd: SttCommand) -> Result<(), BackendError> {
        self.commands.lock().expect("sink lock").push(cmd);
        Ok(())
    }
}

struct NoopPaste;

impl PasteSink for NoopPaste {
    fn paste(&mut self) -> Result<(), OutputError> {
        Ok(())
    }
}

struct NoopOpener;

impl FileOpener for NoopOpener {
    fn open(&mut self, _path: &Path) -> Result<(), OutputError> {
        Ok(())
    }
}

/// A save prompt that answers every request with one scripted [`SaveAnswer`].
///
/// It records every suggested name it was asked for, so a test can prove that a
/// second keystroke did not stack a dialog. The answer resolves on the next
/// executor poll, which is what keeps the prompt "open" for the rest of the
/// frame that asked.
#[derive(Clone)]
pub struct ScriptedSavePrompt {
    answer: SaveAnswer,
    asks: Arc<Mutex<Vec<String>>>,
}

impl Default for ScriptedSavePrompt {
    fn default() -> Self {
        Self::cancelled()
    }
}

impl ScriptedSavePrompt {
    /// A prompt that answers every request with `path`.
    pub fn answering(path: impl Into<PathBuf>) -> Self {
        Self::chosen(path)
    }

    /// A prompt the user picks `path` in.
    pub fn chosen(path: impl Into<PathBuf>) -> Self {
        Self::with_answer(SaveAnswer::Chosen(path.into()))
    }

    /// A prompt the user dismisses.
    pub fn cancelled() -> Self {
        Self::with_answer(SaveAnswer::Cancelled)
    }

    /// A prompt whose platform dialog could not open.
    pub fn failed() -> Self {
        Self::with_answer(SaveAnswer::Failed)
    }

    fn with_answer(answer: SaveAnswer) -> Self {
        Self {
            answer,
            asks: Arc::default(),
        }
    }

    /// The suggested names asked for so far, in order.
    pub fn asks(&self) -> Vec<String> {
        self.asks.lock().expect("save prompt lock").clone()
    }
}

impl SavePrompt for ScriptedSavePrompt {
    fn pick_save_path(&mut self, _cx: &mut App, suggested: &str) -> Task<SaveAnswer> {
        self.asks
            .lock()
            .expect("save prompt lock")
            .push(suggested.to_owned());
        Task::ready(self.answer.clone())
    }
}

fn test_services() -> AppServices {
    let (sender, events) = speechtt_core::async_channel::unbounded();
    // Keep the event stream open for the window's lifetime: a closed stream
    // would surface `BackendUnavailable` in every headless render test.
    std::mem::forget(sender);

    services_with_events(events)
}

/// Injected services whose backend event stream is owned by the caller.
fn services_with_events(events: speechtt_core::backend::SttEventReceiver) -> AppServices {
    services_with_events_and_prompt(events, Box::new(ScriptedSavePrompt::cancelled()))
}

/// Injected services with a caller-supplied save dialog.
fn services_with_events_and_prompt(
    events: speechtt_core::backend::SttEventReceiver,
    save_prompt: Box<dyn SavePrompt>,
) -> AppServices {
    AppServices {
        audio: Box::new(SilentAudio),
        analyzer: SpectrumAnalyzer::new(SAMPLE_RATE),
        sink: Box::new(NoopSink),
        events,
        settings: Box::new(TestSettingsStore::new()),
        clipboard: Box::new(NoopClipboard),
        paste: Box::new(NoopPaste),
        opener: Box::new(NoopOpener),
        save_prompt,
        clock: Box::new(SystemClock),
    }
}

/// Runs the production startup seam on a test context: the gpui-kit
/// framework, the application keymap and the SpeechTT theme, in the order
/// `startup::init_app` installs them.
///
/// Calls [`speechtt_ui::startup::init_app`] rather than repeating its steps, so a
/// step added to production is installed by every UI test that opens the widget.
/// It does not load the persisted window layout: that read touches the user's
/// config directory, and tests stay hermetic.
pub fn init_kit(cx: &mut App) {
    speechtt_ui::startup::init_app(cx);
}

/// Opens the production `gpui_kit::open_window` entry point with the
/// SpeechTT widget bounds by default, wrapped in Base's `Root`.
///
/// The window options are the production contract
/// ([`speechtt_ui::startup::window_options`]) with deterministic test bounds
/// substituted for the restored layout, so a field added to the production
/// contract reaches every UI test without a second list to maintain. An
/// explicit `window_size` also drops the production minimum size:
/// `WindowOptions::window_min_size` is documented only as "Window minimum size"
/// with nothing about creation clamping the requested bounds, so the adaptive
/// smoke test that asks for a size below production's floor gets exactly the
/// bounds it asked for.
pub fn open_window<V: Render>(
    cx: &mut TestAppContext,
    window_size: Option<Size<Pixels>>,
    build: impl FnOnce(&mut Window, &mut App) -> Entity<V>,
) -> (WindowHandle<Root>, Entity<V>) {
    cx.update(|cx| {
        let bounds = window_size
            .map(|window_size| Bounds {
                origin: Point::default(),
                size: window_size,
            })
            .unwrap_or_else(|| Bounds {
                origin: Point::default(),
                size: size(
                    px(speechtt_ui::tokens::WIDGET_WIDTH),
                    px(speechtt_ui::tokens::WIDGET_HEIGHT),
                ),
            });
        let mut options = speechtt_ui::startup::window_options(None, &[]);
        options.window_bounds = Some(WindowBounds::Windowed(bounds));
        if window_size.is_some() {
            // Deliberately below production's minimum: see the doc above.
            options.window_min_size = None;
        }
        let (window, content) =
            gpui_kit::open_window(options, cx, build).expect("open test window");
        (window.downcast::<Root>().expect("Base Root"), content)
    })
}

/// Deterministic catalogue the fake worker would return after a handshake.
///
/// Installed by [`open_speechtt_window`] so Settings and the setup form can be
/// exercised without a live Bun worker.
pub fn test_catalogue() -> Vec<ProviderCapability> {
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
                description: Some("Live gateway transcription.".to_string()),
                pricing: std::collections::BTreeMap::from([(
                    "transcription_duration_cost_per_second".to_string(),
                    "0.000031".to_string(),
                )]),
            },
        ],
    }]
}

/// Opens the real SpeechTT widget window (theme applied, production view,
/// hermetic services).
pub fn open_speechtt_window(
    cx: &mut TestAppContext,
) -> (WindowHandle<Root>, Entity<speechtt_ui::app::SpeechTtApp>) {
    cx.update(init_kit);
    open_window(cx, None, |window, cx| {
        window.set_window_title(speechtt_ui::app::APP_TITLE);
        let view = cx.new(|cx| SpeechTtApp::with_services(test_services(), window, cx));
        view.update(cx, |app, cx| app.apply_capabilities(test_catalogue(), cx));
        view
    })
}

/// Opens the widget window with a backend event stream the test drives.
///
/// Lets a headless test feed a transcript before finishing the take, so the
/// stopped screen has real text to save. This fixture keeps the production save
/// adapter, because `tests/widget.rs` answers its dialog with
/// `TestAppContext::simulate_new_path_selection`; every fixture that does not
/// supply a prompt cancels a save request instead.
pub fn open_speechtt_window_with_events(
    cx: &mut TestAppContext,
) -> (
    WindowHandle<Root>,
    Entity<speechtt_ui::app::SpeechTtApp>,
    speechtt_core::async_channel::Sender<
        Result<speechtt_core::protocol::SttEvent, speechtt_core::backend::BackendError>,
    >,
) {
    cx.update(init_kit);
    let (sender, events) = speechtt_core::async_channel::unbounded();
    let (window, view) = open_window(cx, None, |window, cx| {
        window.set_window_title(speechtt_ui::app::APP_TITLE);
        let view = cx.new(|cx| {
            SpeechTtApp::with_services(
                services_with_events_and_prompt(events, Box::new(GpuiSavePrompt)),
                window,
                cx,
            )
        });
        view.update(cx, |app, cx| app.apply_capabilities(test_catalogue(), cx));
        view
    });
    (window, view, sender)
}

/// Opens the widget window with no catalogue and a backend event stream the
/// test owns.
///
/// No catalogue is applied, so the Settings dialog opens on the persisted
/// values. Closing the returned sender is how a test reaches the dead-backend
/// state: the pump records the failure, which is the condition
/// [`speechtt_ui::app::SpeechTtApp::save_settings`] reacts to.
pub fn open_speechtt_window_without_catalogue(
    cx: &mut TestAppContext,
) -> (
    WindowHandle<Root>,
    Entity<speechtt_ui::app::SpeechTtApp>,
    EventSender,
) {
    cx.update(init_kit);
    let (sender, events) = speechtt_core::async_channel::unbounded();
    let (window, view) = open_window(cx, None, |window, cx| {
        window.set_window_title(speechtt_ui::app::APP_TITLE);
        cx.new(|cx| SpeechTtApp::with_services(services_with_events(events), window, cx))
    });
    (window, view, sender)
}

/// Opens the widget window with a save dialog the test answers.
pub fn open_speechtt_window_with_save_prompt(
    cx: &mut TestAppContext,
    save_prompt: Box<dyn SavePrompt>,
) -> (
    WindowHandle<Root>,
    Entity<speechtt_ui::app::SpeechTtApp>,
    EventSender,
) {
    cx.update(init_kit);
    let (sender, events) = speechtt_core::async_channel::unbounded();
    let (window, view) = open_window(cx, None, |window, cx| {
        window.set_window_title(speechtt_ui::app::APP_TITLE);
        let view = cx.new(|cx| {
            SpeechTtApp::with_services(
                services_with_events_and_prompt(events, save_prompt),
                window,
                cx,
            )
        });
        view.update(cx, |app, cx| app.apply_capabilities(test_catalogue(), cx));
        view
    });
    (window, view, sender)
}

/// Opens the real SpeechTT widget window at an explicit size, for adaptive
/// layout smoke tests.
pub fn open_speechtt_window_at(
    cx: &mut TestAppContext,
    width: f32,
    height: f32,
) -> (WindowHandle<Root>, Entity<speechtt_ui::app::SpeechTtApp>) {
    cx.update(init_kit);
    open_window(cx, Some(size(px(width), px(height))), |window, cx| {
        window.set_window_title(speechtt_ui::app::APP_TITLE);
        let view = cx.new(|cx| SpeechTtApp::with_services(test_services(), window, cx));
        view.update(cx, |app, cx| app.apply_capabilities(test_catalogue(), cx));
        view
    })
}

/// The worker side of the event pump: a test feeds events through it.
pub type EventSender =
    speechtt_core::async_channel::Sender<Result<speechtt_core::protocol::SttEvent, BackendError>>;

/// Opens the widget window with a backend the test can inspect and drive.
///
/// The sink records every command the app sends and the clipboard every string
/// it copies, so a headless test can assert what reached the worker without
/// spawning one. `events` is the worker side of the pump: the test feeds
/// [`speechtt_core::protocol::SttEvent`]s through it.
pub fn open_speechtt_window_with_channels(
    cx: &mut TestAppContext,
) -> (
    WindowHandle<Root>,
    Entity<speechtt_ui::app::SpeechTtApp>,
    RecordingSink,
    RecordingClipboard,
    EventSender,
) {
    cx.update(init_kit);
    let (sender, events) = speechtt_core::async_channel::unbounded();
    let sink = RecordingSink::default();
    let clipboard = RecordingClipboard::default();
    let services = AppServices {
        audio: Box::new(SilentAudio),
        analyzer: SpectrumAnalyzer::new(SAMPLE_RATE),
        sink: Box::new(sink.clone()),
        events,
        settings: Box::new(TestSettingsStore::new()),
        clipboard: Box::new(clipboard.clone()),
        paste: Box::new(NoopPaste),
        opener: Box::new(NoopOpener),
        save_prompt: Box::new(ScriptedSavePrompt::cancelled()),
        clock: Box::new(SystemClock),
    };
    let (window, view) = open_window(cx, None, |window, cx| {
        window.set_window_title(speechtt_ui::app::APP_TITLE);
        let view = cx.new(|cx| SpeechTtApp::with_services(services, window, cx));
        view.update(cx, |app, cx| app.apply_capabilities(test_catalogue(), cx));
        view
    });
    (window, view, sink, clipboard, sender)
}
