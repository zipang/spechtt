//! `speechtt-ui` — the SpeechTT floating dictation widget, built with
//! gpui-kit (native, GPU-accelerated) over a custom theme generated from
//! `design-tokens.css`. See `PRODUCT.md` §5 / Phase 2.
//!
//! Layout:
//! - [`tokens`] — the Rust mirror of the stylesheet token contract.
//! - [`theme`] — that token set mapped onto gpui-kit's Theme.
//! - [`app`] — the widget window views (shell now; states + dialogs next).

pub mod app;
pub mod theme;
pub mod tokens;

use gpui_kit::{AppContext, Bounds, Point, Size, WindowBounds, WindowOptions, px};

// SpeechTT's own icon set — embedded as borrowed static bytes, nothing more.
// Components that need their own internals (spinners, carets, close buttons)
// still get them via the gpui-kit default `Assets` through [`AppAssets`].
gpui_kit::assets::icon_assets!(AppIcons, [CircleQuestionMark, Settings]);

/// Composed asset source: SpeechTT's curated icons first, then the
/// gpui-kit component defaults as fallback. Keeps the binary slim while
/// styled components keep working.
struct AppAssets;

impl gpui_kit::AssetSource for AppAssets {
    fn load(
        &self,
        path: &str,
    ) -> gpui_kit::Result<Option<std::borrow::Cow<'static, [u8]>>> {
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

/// Window dimensions of the widget (mockups: 480×280).
pub fn widget_window_options() -> WindowOptions {
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds {
            origin: Point::default(),
            size: Size {
                width: px(tokens::WIDGET_WIDTH),
                height: px(tokens::WIDGET_HEIGHT),
            },
        })),
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

            gpui_kit::open_window(widget_window_options(), cx, |window, cx| {
                window.set_window_title(app::APP_TITLE);
                cx.new(|_| app::SpeechTtApp::new())
            })
            .expect("failed to open SpeechTT widget window");
        });
}
