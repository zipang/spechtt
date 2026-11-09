//! Startup gate: the production startup seam runs headlessly, `run()` calls it,
//! and the UI test fixture calls it too.
//!
//! `lib.rs::run` hands its closure body to `gpui_kit::application().run`, so no
//! test can call it: a startup step that lived only there would be parsed for
//! the first time on the user's machine. Three checks close that hole:
//!
//! 1. [`init_app`] is executed on a headless context, so every registration it
//!    performs — the keymap above all — is parsed by `cargo test`.
//! 2. [`window_options`] carries the window contract: a minimum size and a
//!    windowed bound. Behaviour only; nothing here asserts rendered geometry.
//! 3. `src/lib.rs` still calls the seam and contains no startup step of its own,
//!    and the UI test fixture calls the seam instead of copying it.
//!
//! [`init_app`]: speechtt_ui::startup::init_app
//! [`window_options`]: speechtt_ui::startup::window_options

use std::path::{Path, PathBuf};

use gpui_kit::{Bounds, Size, TestAppContext, WindowBounds};
use speechtt_core::settings::WindowLayout;
use speechtt_ui::startup;

fn manifest_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(relative: &str) -> String {
    let path = manifest_root().join(relative);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
}

/// The code lines of `source`, without the comment lines.
///
/// A comment may name a startup step without performing one, so the gates below
/// look at code only — the same handling `tests/design_system.rs` uses. Only
/// `//` lines are stripped: block comments are not, so startup prose stays in
/// `//` lines. A false positive is the safe direction.
fn code_lines(source: &str) -> Vec<(usize, &str)> {
    source
        .lines()
        .enumerate()
        .map(|(index, line)| (index + 1, line))
        .filter(|(_, line)| {
            let code = line.trim_start();
            !code.starts_with("//")
        })
        .collect()
}

/// Every `needle` found in `source`'s code, as `file:line: source` strings.
fn occurrences(relative: &str, source: &str, needles: &[&str]) -> Vec<String> {
    let mut found = Vec::new();
    for (number, line) in code_lines(source) {
        for needle in needles {
            if line.contains(needle) {
                found.push(format!("{relative}:{number}: {}", line.trim()));
            }
        }
    }
    found
}

/// The ways an unparsable key string can enter the tree.
///
/// `KeyBinding::new` and `KeyBinding::load` are the two keybinding constructors,
/// and `Keystroke::parse` / `KeyContext::parse` are the documented free parsers
/// behind them. A startup step that calls one of these outside the seam is a
/// registration the headless run never sees.
const KEY_STRING_PATHS: [&str; 9] = [
    "gpui_kit::init",
    "cx.bind_keys",
    "KeyBinding::new",
    "KeyBinding::load",
    "Keystroke::parse",
    "KeyContext::parse",
    "theme::apply",
    "bind_app_keys",
    "bind_keys",
];

/// Runs the production startup on a headless context.
///
/// `KeyBinding::new` panics on a keystroke or predicate it cannot parse, so a
/// green run is the proof that every registration [`startup::init_app`] performs
/// parses. A startup step added to the seam is covered here even when it belongs
/// to no table; `shortcuts`' own tests pin the table on both platforms.
#[gpui_kit::test]
fn init_app_installs_a_parseable_keymap_and_theme(cx: &mut TestAppContext) {
    cx.update(startup::init_app);
}

/// The window contract: a minimum size, a windowed bound, and a size that never
/// exceeds the display it restores onto.
#[test]
fn window_options_are_the_window_contract() {
    let options = startup::window_options(None, &[]);

    assert_eq!(
        options.window_min_size,
        Some(speechtt_ui::minimum_window_size()),
        "the window must carry the one minimum size the runtime clamp also uses"
    );
    assert!(
        matches!(options.window_bounds, Some(WindowBounds::Windowed(_))),
        "the widget opens as a windowed window"
    );

    let Some(WindowBounds::Windowed(bounds)) = options.window_bounds else {
        unreachable!("asserted above");
    };
    let minimum = speechtt_ui::minimum_window_size();
    assert!(
        bounds.size.width >= minimum.width && bounds.size.height >= minimum.height,
        "with no persisted layout and no display list the bounds still meet the \
         minimum, got {:?}",
        bounds.size
    );

    // A persisted layout larger than the display it restores onto is shrunk to
    // the display, which is where the restore contract actually bites. Only the
    // size is asserted: origins belong to the restored-bounds contract pinned in
    // `tests/widget.rs`, and no rendered geometry is involved.
    let display = Bounds {
        origin: Default::default(),
        size: Size {
            width: gpui_kit::px(1920.),
            height: gpui_kit::px(1080.),
        },
    };
    let oversized = WindowLayout::new(4000, 3000, 0, 0);
    let clamped = startup::window_options(Some(oversized), &[display]);
    let Some(WindowBounds::Windowed(bounds)) = clamped.window_bounds else {
        panic!("the widget opens as a windowed window");
    };
    assert!(
        bounds.size.width <= display.size.width && bounds.size.height <= display.size.height,
        "a persisted layout larger than the display is restored no larger than the \
         display, got {:?}",
        bounds.size
    );
}

/// Startup steps belong to the seam. `run()` wires and never registers, the
/// fixture calls the seam, and both call it rather than re-listing its steps.
#[test]
fn run_wires_and_the_fixture_calls_the_seam() {
    let run_shell = read("src/lib.rs");

    let offenders = occurrences("src/lib.rs", &run_shell, &KEY_STRING_PATHS);
    assert!(
        offenders.is_empty(),
        "startup steps belong in `startup::init_app`; `run()` only wires — move each \
         of these into the seam ({} offender(s), AGENTS.md §6 startup rule):\n{}",
        offenders.len(),
        offenders.join("\n")
    );

    assert!(
        run_shell.contains("startup::init_app(cx)"),
        "`run()` must install the startup seam: call `startup::init_app(cx)` in its \
         application closure, or the widget opens with no application keymap and the \
         default theme (AGENTS.md §6 startup rule)"
    );
    assert!(
        run_shell.contains("startup::window_options("),
        "`run()` must take the window contract from the seam: build its options with \
         `startup::window_options(layout, &displays)` rather than assembling them inline \
         (AGENTS.md §6 startup rule)"
    );

    let fixture = read("tests/common/mod.rs");

    let offenders = occurrences("tests/common/mod.rs", &fixture, &KEY_STRING_PATHS);
    assert!(
        offenders.is_empty(),
        "startup steps belong in `startup::init_app`; the fixture must call the seam, \
         never copy it — replace each of these with a call to \
         `startup::init_app` ({} offender(s), AGENTS.md §6 startup rule):\n{}",
        offenders.len(),
        offenders.join("\n")
    );
    assert!(
        fixture.contains("speechtt_ui::startup::init_app"),
        "the fixture must call the seam: `init_kit` has to call \
         `speechtt_ui::startup::init_app(cx)` so the UI tests install the startup the \
         binary does (AGENTS.md §6 startup rule)"
    );
}

/// A usable persisted layout reaches the window unchanged.
///
/// The seam receives the layout as an argument, so a regression that forwards
/// `None` — or drops the argument while assembling the options — would ship a
/// widget that forgets the user's size and position on every start. Only the
/// size is asserted; origins belong to the restored-bounds contract in
/// `tests/widget.rs`.
#[test]
fn window_options_keep_a_usable_persisted_layout() {
    let display = Bounds {
        origin: Default::default(),
        size: Size {
            width: gpui_kit::px(1920.),
            height: gpui_kit::px(1080.),
        },
    };
    let layout = WindowLayout::new(640, 560, 20, 30);
    let options = startup::window_options(Some(layout), &[display]);

    let Some(WindowBounds::Windowed(bounds)) = options.window_bounds else {
        panic!("the widget opens as a windowed window");
    };
    assert_eq!(
        bounds.size,
        Size {
            width: gpui_kit::px(640.),
            height: gpui_kit::px(560.),
        },
        "a persisted layout that fits the display is restored as persisted"
    );
}

/// Guards the gate itself: `read` must fail loudly on a path typo rather than
/// comparing against an empty string, which would let every check pass.
#[test]
fn the_gate_reads_real_sources() {
    for relative in ["src/lib.rs", "tests/common/mod.rs", "src/startup.rs"] {
        let path = manifest_root().join(relative);
        assert!(
            Path::new(&path).is_file(),
            "{} is missing, so the startup gate would be checking nothing",
            path.display()
        );
        assert!(!read(relative).is_empty(), "{relative} is empty");
    }
}
