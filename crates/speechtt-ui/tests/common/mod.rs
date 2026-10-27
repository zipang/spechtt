//! Test fixture: open a production view in a headless window with
//! deterministic test bounds. Mirrors gpui-kit's own `tests/common`.
//!
//! The SpeechTT fixtures inject hermetic [`AppServices`]: an in-memory
//! settings store, silent audio, a no-op backend/clipboard/paste/opener and a
//! recording-free shortcut listener. No test reads or writes the user's real
//! config, spawns Bun, or opens audio/display hardware.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use gpui_kit::{
    App, AppContext, Bounds, Entity, Pixels, Point, Render, Size, TestAppContext, Window,
    WindowBounds, WindowHandle, WindowOptions, base::Root, px, size,
};
use speechtt_core::audio::{AudioError, AudioSource, PcmChunk};
use speechtt_core::backend::{BackendError, SttSessionSink};
use speechtt_core::fft::SpectrumAnalyzer;
use speechtt_core::hotkeys::{HotkeyError, SessionAction, Shortcut, ShortcutListener};
use speechtt_core::output::OutputError;
use speechtt_core::output::clipboard::ClipboardSink;
use speechtt_core::output::file::FileOpener;
use speechtt_core::output::paste::PasteSink;
use speechtt_core::protocol::SttCommand;
use speechtt_core::settings::{Settings, SettingsError, SettingsStore};
use speechtt_ui::app::{AppServices, SpeechTtApp};
use speechtt_ui::controller::{SAMPLE_RATE, SystemClock};

/// In-memory settings store. The initial key makes the widget's recording
/// flow pass the T0009 missing-key guard without touching `$HOME`.
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

struct NoopListener;

impl ShortcutListener for NoopListener {
    fn register(&mut self, _shortcuts: &[(SessionAction, Shortcut)]) -> Result<(), HotkeyError> {
        Ok(())
    }

    fn unregister_all(&mut self) {}
}

fn test_services() -> AppServices {
    let (_sender, events) = speechtt_core::async_channel::unbounded();
    // Keep the event stream open for the window's lifetime: a closed stream
    // would surface `BackendUnavailable` in every headless render test.
    std::mem::forget(_sender);

    AppServices {
        audio: Box::new(SilentAudio),
        analyzer: SpectrumAnalyzer::new(SAMPLE_RATE),
        sink: Box::new(NoopSink),
        events,
        settings: Box::new(TestSettingsStore::new()),
        clipboard: Box::new(NoopClipboard),
        paste: Box::new(NoopPaste),
        opener: Box::new(NoopOpener),
        listener: Box::new(NoopListener),
        clock: Box::new(SystemClock),
    }
}

/// Opens the production `gpui_kit::open_window` entry point with the
/// SpeechTT widget bounds by default, wrapped in Base's `Root`.
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
        let (window, content) = gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..WindowOptions::default()
            },
            cx,
            build,
        )
        .expect("open test window");
        (window.downcast::<Root>().expect("Base Root"), content)
    })
}

/// Opens the real SpeechTT widget window (theme applied, production view,
/// hermetic services).
pub fn open_speechtt_window(
    cx: &mut TestAppContext,
) -> (WindowHandle<Root>, Entity<speechtt_ui::app::SpeechTtApp>) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        speechtt_ui::theme::apply(cx);
    });
    open_window(cx, None, |window, cx| {
        window.set_window_title(speechtt_ui::app::APP_TITLE);
        cx.new(|cx| SpeechTtApp::with_services(test_services(), window, cx))
    })
}

/// Opens the real SpeechTT widget window at an explicit size, for adaptive
/// layout smoke tests.
pub fn open_speechtt_window_at(
    cx: &mut TestAppContext,
    width: f32,
    height: f32,
) -> (WindowHandle<Root>, Entity<speechtt_ui::app::SpeechTtApp>) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        speechtt_ui::theme::apply(cx);
    });
    open_window(cx, Some(size(px(width), px(height))), |window, cx| {
        window.set_window_title(speechtt_ui::app::APP_TITLE);
        cx.new(|cx| SpeechTtApp::with_services(test_services(), window, cx))
    })
}
