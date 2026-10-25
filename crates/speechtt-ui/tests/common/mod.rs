//! Test fixture: open a production view in a headless window with
//! deterministic test bounds. Mirrors gpui-kit's own `tests/common`.

#![allow(dead_code)]

use gpui_kit::{
    App, AppContext, Bounds, Entity, Pixels, Point, Render, Size, TestAppContext, Window,
    WindowBounds, WindowHandle, WindowOptions, base::Root, px, size,
};

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

/// Opens the real SpeechTT widget window (theme applied, production view).
pub fn open_speechtt_window(
    cx: &mut TestAppContext,
) -> (WindowHandle<Root>, Entity<speechtt_ui::app::SpeechTtApp>) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        speechtt_ui::theme::apply(cx);
    });
    open_window(cx, None, |window, cx| {
        window.set_window_title(speechtt_ui::app::APP_TITLE);
        cx.new(|cx| speechtt_ui::app::SpeechTtApp::new(window, cx))
    })
}
