//! The widget's startup seam: everything the application installs before its
//! first frame, in one callable place.
//!
//! `lib.rs::run` builds the gpui-kit application and hands its closure to the
//! framework, so nothing can call that closure body from a test. This module is
//! the part that can be called: [`init_app`] installs the framework, keymap and
//! theme, and [`window_options`] assembles the window contract.
//!
//! - `lib.rs::run` calls [`init_app`] and [`window_options`]; it only wires.
//! - Every UI integration test that opens the widget calls [`init_app`] through
//!   `tests/common::init_kit`, so the tests install the same startup the binary
//!   does instead of a hand-maintained copy of it. `tests/theme_contract.rs`
//!   installs the framework and theme directly, because it isolates the token
//!   mapping.
//! - `tests/startup.rs` runs [`init_app`] headlessly and pins
//!   [`window_options`]' behaviour, and it reads `lib.rs` to keep startup steps
//!   out of the wiring shell.
//!
//! Neither function reads the filesystem or the environment: the persisted
//! window layout is loaded by the caller, which keeps [`window_options`] a pure
//! function of its arguments and the tests hermetic.

use gpui_kit::{App, Bounds, Pixels, WindowBounds, WindowOptions};
use speechtt_core::settings::WindowLayout;

/// Installs everything the widget needs before its first frame.
///
/// In order: the gpui-kit framework (`gpui_kit::init` — component state, icons
/// and theme keys, and the component key bindings the rest of the keymap is
/// layered over), then the application keymap
/// ([`crate::shortcuts::bind_app_keys`]), then the SpeechTT theme
/// ([`crate::theme::apply`]). The order matters: the keymap and the theme both
/// address state that `gpui_kit::init` registers.
///
/// Every registration the application performs belongs here. A step added at a
/// call site instead is parsed for the first time on the user's machine.
pub fn init_app(cx: &mut App) {
    gpui_kit::init(cx);
    crate::shortcuts::bind_app_keys(cx);
    crate::theme::apply(cx);
}

/// The window contract the widget opens with.
///
/// `layout` is the persisted geometry the caller loaded (or `None`), and
/// `displays` the visible display rectangles the caller read, used to pull an
/// off-screen window back into view. Passing `&[]` keeps the persisted size but
/// skips the display clamp, which is what a headless test wants.
///
/// Pure: the same arguments always yield the same options, and nothing is read
/// from disk or from the environment.
#[must_use]
pub fn window_options(layout: Option<WindowLayout>, displays: &[Bounds<Pixels>]) -> WindowOptions {
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(crate::restored_window_bounds(
            layout, displays,
        ))),
        window_min_size: Some(crate::minimum_window_size()),
        ..WindowOptions::default()
    }
}
