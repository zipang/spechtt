//! `speechtt-ui` — the SpeechTT floating dictation widget, built with
//! gpui-kit (native, GPU-accelerated) over a custom theme generated from
//! `design-tokens.css`. See `PRODUCT.md` §5 / Phase 2.
//!
//! Layout:
//! - [`tokens`] — the Rust mirror of the stylesheet token contract.
//! - [`theme`] — that token set mapped onto gpui-kit's Theme.
//! - [`controller`] — the pure session/view-model state (no window needed).
//! - [`components`] — SpeechTT's `RenderOnce` visual pieces.
//! - [`app`] — the widget window view (shell now; dialogs next).

pub mod app;
pub mod components;
pub mod controller;
pub mod diagnostics;
pub mod layout;
pub mod theme;
pub mod tokens;

mod views;

use gpui_kit::{AppContext, Bounds, Pixels, Point, Size, WindowBounds, WindowOptions, point, px};
use speechtt_core::settings::{JsonFileStore, SettingsStore, WindowLayout};

// SpeechTT's own icon set — embedded as borrowed static bytes, nothing more.
// Components that need their own internals (spinners, carets, close buttons)
// still get them via the gpui-kit default `Assets` through [`AppAssets`].
gpui_kit::assets::icon_assets!(
    AppIcons,
    [
        CircleQuestionMark,
        Settings,
        Mic,
        Square,
        Pause,
        Play,
        Copy,
        HardDriveDownload,
        FileText,
        FileUp,
        Zap,
        Scissors
    ]
);

/// Composed asset source: SpeechTT's curated icons first, then the
/// gpui-kit component defaults as fallback. Keeps the binary slim while
/// styled components keep working.
struct AppAssets;

impl gpui_kit::AssetSource for AppAssets {
    fn load(&self, path: &str) -> gpui_kit::Result<Option<std::borrow::Cow<'static, [u8]>>> {
        match AppIcons.load(path) {
            Ok(Some(data)) => Ok(Some(data)),
            Ok(None) => gpui_kit::assets::Assets.load(path),
            Err(err) => Err(err),
        }
    }

    fn list(&self, path: &str) -> gpui_kit::Result<Vec<gpui_kit::SharedString>> {
        let mut listed = AppIcons.list(path)?;
        listed.extend(gpui_kit::assets::Assets.list(path)?);
        Ok(listed)
    }
}

/// At least this much of the window stays inside the visible display area.
const MIN_VISIBLE_EDGE: f32 = 60.;

/// Default widget bounds when no usable geometry was persisted.
#[must_use]
pub fn default_window_bounds() -> Bounds<Pixels> {
    Bounds {
        origin: Point::default(),
        size: Size {
            width: px(tokens::WIDGET_WIDTH),
            height: px(tokens::WIDGET_HEIGHT),
        },
    }
}

/// Restores a persisted layout, clamping it into the first visible display.
///
/// Sizes below [`WindowLayout`]'s minimum are rejected (default bounds), a
/// remembered size below the widget minimum is raised to it, and a position
/// that would leave the widget off-screen is pulled back so at least
/// [`MIN_VISIBLE_EDGE`] logical pixels remain visible.
#[must_use]
pub fn restored_window_bounds(
    layout: Option<WindowLayout>,
    displays: &[Bounds<Pixels>],
) -> Bounds<Pixels> {
    let Some(layout) = layout.filter(WindowLayout::is_usable) else {
        return default_window_bounds();
    };

    let mut width = px(layout.width as f32).max(px(tokens::MIN_WINDOW_WIDTH));
    let mut height = px(layout.height as f32).max(px(tokens::MIN_WINDOW_HEIGHT));
    if let Some(display) = displays.first() {
        width = width.min(display.size.width);
        height = height.min(display.size.height);
    }

    let mut origin = point(px(layout.x as f32), px(layout.y as f32));
    if let Some(display) = displays.first() {
        let edge = px(MIN_VISIBLE_EDGE);
        let min_x = display.origin.x - width + edge;
        let max_x = display.origin.x + display.size.width - edge;
        let min_y = display.origin.y;
        let max_y = display.origin.y + display.size.height - edge;
        origin.x = origin.x.clamp(min_x.min(max_x), max_x);
        origin.y = origin.y.clamp(min_y.min(max_y), max_y);
    }

    Bounds {
        origin,
        size: Size { width, height },
    }
}

/// The widget's minimum window size: the mode chooser must never be squeezed.
///
/// This is the single source of truth for the floor, shared by the window
/// options and by the runtime clamp in [`app::SpeechTtApp`].
#[must_use]
pub fn minimum_window_size() -> Size<Pixels> {
    Size {
        width: px(tokens::MIN_WINDOW_WIDTH),
        height: px(tokens::MIN_WINDOW_HEIGHT),
    }
}

/// Window options for the widget, restoring a persisted layout when present.
#[must_use]
pub fn widget_window_options(layout: Option<WindowLayout>) -> WindowOptions {
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(restored_window_bounds(layout, &[]))),
        window_min_size: Some(minimum_window_size()),
        ..WindowOptions::default()
    }
}

/// Runs the SpeechTT widget application (single process — PRODUCT.md §7).
/// Used by `main.rs`; headless tests drive the same pieces directly.
pub fn run() {
    gpui_kit::application()
        // Composed asset source (SpeechTT icons + component defaults) —
        // see `AppAssets` above. No full-catalog embed.
        .with_assets(AppAssets)
        .run(move |cx| {
            // MUST be first: registers component state, icons, theme keys.
            gpui_kit::init(cx);

            // SpeechTT Sensor token mapping, so the first frame is ours.
            theme::apply(cx);

            // Restore the last size/position before the window exists, so the
            // first frame is already at the right geometry.
            let layout = JsonFileStore::new(JsonFileStore::default_path())
                .load()
                .ok()
                .and_then(|settings| settings.window);
            let displays: Vec<Bounds<Pixels>> = cx
                .displays()
                .iter()
                .map(|display| display.visible_bounds())
                .collect();
            let bounds = restored_window_bounds(layout, &displays);
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(minimum_window_size()),
                ..WindowOptions::default()
            };

            gpui_kit::open_window(options, cx, |window, cx| {
                window.set_window_title(app::APP_TITLE);
                cx.new(|cx| app::SpeechTtApp::new(window, cx))
            })
            .expect("failed to open SpeechTT widget window");
        });
}
