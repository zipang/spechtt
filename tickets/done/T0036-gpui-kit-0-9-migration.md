# T0036 — Migrate the UI to gpui-kit 0.9.1

- **Track**: Rust UI (framework bump) — cross-cutting only in that it touches the
  documentation registry
- **Wave**: 1 (independent)
- **Depends on**: T0035 (the keyboard work is what made two framework gaps
  visible; it must be closed first so its behaviour is the baseline)
- **Blocks**: nothing
- **Owner**: coding sub-agent, supervised by CTO
- **Started**: 2026-11-09
- **References**:
    - crates.io API: `<https://crates.io/api/v1/crates/gpui-kit>` (version +
      publish dates), `<https://crates.io/api/v1/crates/gpui-kit/0.9.1/dependencies>`
      (exact framework pins)
    - `gpui-kit` docs for the pinned target: `<https://docs.rs/gpui-kit/0.9.1>`
    - Markdown guide index: `<https://gpui-kit.com/llms.txt>` (already documents
      0.9.x, so the repository is behind its own reference documentation)
    - KeyBinding / Action / Testing pages:
      `<https://gpui-kit.com/docs/keybinding.md>`,
      `<https://gpui-kit.com/docs/action.md>`,
      `<https://gpui-kit.com/docs/test.md>`
    - `AGENTS.md` §6 "Official documentation sources — the registry"

## Context & Goal

The workspace pins `gpui-kit = "0.8"`, which resolves to **0.8.0** (published
2026-10-19) and drags `gpui-pre 0.3.9`. The framework has since published
**0.8.1** (2026-10-26), **0.9.0** (2026-11-02) and **0.9.1** (2026-11-09), so the
repository is two minor versions behind the crate _and_ behind the
`gpui-kit.com` documentation it is instructed to treat as authoritative
(`AGENTS.md` §6). Every guide page fetched today describes 0.9.x, which means an
implementing agent following the documented API can write code that does not
compile — that is exactly what happened twice during T0035.

**Goal.** Move the workspace to `gpui-kit 0.9.1`, recover the two capabilities
that were worked around under 0.8.0, and re-run every gate.

## What 0.9.1 changes underneath

`gpui-kit 0.9.1` pins its layers exactly:

| Crate             | Now (0.8.0) | Target (0.9.1)          |
| ----------------- | ----------- | ----------------------- |
| `gpui-kit`        | 0.8.0       | 0.9.1                   |
| `gpui-base`       | 0.8.0       | 0.9.1                   |
| `gpui-component`  | 0.8.0       | 0.9.1                   |
| `gpui-kit-assets` | 0.8.0       | 0.9.1                   |
| `gpui-pre`        | 0.3.9       | **=0.3.12** (exact pin) |
| `gpui-fast`       | 0.1.10      | ^0.1.14                 |

`gpui-pre` is pre-1.0 and pinned exactly, so a patch bump is a real API surface
change, not a bugfix. Everything the UI re-exports through `gpui_kit` moves with
it.

## Two capabilities the bump recovers

Both were discovered while implementing T0035 and are recorded there as gaps.

1. **`App::platform_info()` exists in 0.9.1.**
   `https://docs.rs/gpui-kit/0.9.1/gpui_kit/struct.App.html`:

    > `pub fn platform_info(self) -> PlatformInfo` — _Returns the host OS and
    > application target reported by the backend._

    `shortcuts::is_mac()` currently answers `cfg!(target_os = "macos")` because
    neither `App` nor the `Platform` trait exposed the host OS at 0.3.9. The
    documented snippet in `keybinding.md` (`cx.platform_info().is_mac()`) can
    finally be followed. Behaviour today is correct for a native build; the point
    is to stop guessing and to match the documentation.

2. **`Window::bindings_for_action` may become usable as an oracle.**
   The method exists in both versions and is documented as returning _"key
   bindings that invoke an action on the currently focused element"_. Under
   0.8.0 it returned no bindings for any action, including `ToggleHelp` and
   `CopySelection` while `f1` demonstrably dispatched and opened the Help dialog
   (re-verified independently during T0035's second review round). If 0.3.12
   fixes it, T0035's follow-up assertion can be written:
   in a dialog window, `window.bindings_for_action(&ToggleRecording).is_empty()`.

## Scope Boundary

May change:

- `Cargo.toml` (workspace dependency version), `Cargo.lock`.
- Every `crates/speechtt-ui/src/**/*.rs` and `crates/speechtt-ui/tests/**` file
  that the new API forces to change — nothing else, and only for compile or
  behaviour reasons.
- `crates/speechtt-core/**` only if a transitive type it names changes.
- `AGENTS.md` §6 (documentation registry rows for `gpui-kit` / `gpui-base` /
  `gpui-component`) and §4 (tech stack version), `README.md`, `PRODUCT.md`
  (§7 topology line), `DESIGN.md` only if a token or component signature moved.
- `tickets/README.md`, this ticket.

Do **not** touch: the transcript reducer's behaviour, the worker (`src/stt/**`),
the design tokens' values, or any `tickets/done/**`. Do not "fix" a failing test
by relaxing it — a red gate after a framework bump is information, not noise.

## Contract

### A. Dependency

```toml
# Cargo.toml
[workspace.dependencies]
gpui-kit = "0.9.1"
```

`Cargo.lock` must show `gpui-kit`, `gpui-base`, `gpui-component`,
`gpui-kit-assets` at `0.9.1` and `gpui-pre*` at `0.3.12`. No crate may resolve to
two framework versions; `cargo tree -d` must not list `gpui-*` twice.

### B. Recovering `is_mac`

`shortcuts::is_mac()` delegates to the documented accessor instead of the build
target:

```rust
/// Whether this build follows macOS keyboard conventions.
#[must_use]
pub fn is_mac(cx: &App) -> bool { cx.platform_info().is_mac() }
```

`bind_keys` already receives `&mut App`; `key_caps` callers in
`views::dialogs::help` read `App`, so the signature change stays local. Every
call site compiles unchanged in meaning.

**Documented signature used** (docs.rs 0.9.1 `struct.App.html`): `pub fn
platform_info(self) -> PlatformInfo`. If `PlatformInfo` does not expose `is_mac`
at 0.3.12, the implementation reads the OS enum it does expose — and the ticket
records the actual shape instead of assuming.

### C. Re-probing `bindings_for_action`

A throwaway probe (not committed) answers, in a widget window whose `f1` press
demonstrably opens Help:

1. `window.bindings_for_action(&ToggleHelp).len()` — expected `>= 1` if the
   method works;
2. the same for `CopySelection`;
3. in a focused Settings dialog, the same for `ToggleRecording`,
   `StopRecording`, `SaveTranscript` — expected `0` **and** `ToggleHelp` `>= 1`
   for the control.

If (1) and (2) answer non-zero, add to `tests/shortcuts.rs` the assertion
T0035 could not ship:

```rust
// The predicate, not merely the fall-through: the take commands are not bound
// on a dialog's dispatch path.
assert!(window.bindings_for_action(&ToggleRecording).is_empty());
assert!(window.bindings_for_action(&StopRecording).is_empty());
assert!(window.bindings_for_action(&SaveTranscript).is_empty());
assert!(!window.bindings_for_action(&ToggleHelp).is_empty()); // control
```

If it still answers zero, record the finding in this ticket and leave T0035 as
it is: the predicate stays pinned by the table-literal tests.

## TDD Tasks

- [x] **T1 — Baseline capture.** Before touching the version, run and record the
      six gates verbatim (they are green on `0.8.0`). Note the exact test counts;
      every later comparison is against these numbers.
- [x] **T2 — Bump.** Set `gpui-kit = "0.9.1"`; `cargo update -p gpui-kit`;
      confirm the resolved versions in §A and that `cargo tree -d` shows no
      duplicate framework crate.
- [x] **T3 — Compile errors, one group at a time.** Group the failures by API
      area (prelude re-exports, `base`/`component` paths, test-support) and fix
      each group against the 0.9.1 docs.rs page — never against
      `~/.cargo/registry/src/**`. Record every signature that changed.
- [x] **T4 — `is_mac` via `platform_info`.** Red first: a test asserting
      `shortcuts::is_mac` answers the platform's convention on the host running
      the suite. Then implement §B.
- [x] **T5 — `bindings_for_action` probe.** §C, throwaway, then either the added
      assertions or a recorded negative finding.
- [x] **T6 — Design system & theme.** `bun run design:validate`,
      `crates/speechtt-ui/tests/design_system.rs` and `theme_contract.rs` must
      pass unchanged. If a token name or role moved in 0.9.1, that is a finding
      to report, not a silent edit of `design-tokens.css`.
- [x] **T7 — Visual smoke.** No screenshots, no GUI runs (AGENTS.md §6): the
      user performs the visual pass. The ticket lists what to look at.
- [x] **T8 — Documentation.** `AGENTS.md` §4 version and §6 registry rows,
      `README.md`, `PRODUCT.md` §7; then all six gates green.

## Acceptance Criteria

- `Cargo.lock` resolves the framework to 0.9.1 / `gpui-pre 0.3.12` with no
  duplicate `gpui-*` crate.
- All six gates pass with zero warnings and **at least** the T1 test counts; no
  test was weakened, skipped or `#[ignore]`d to accommodate the bump.
- `shortcuts::is_mac()` reads the documented platform accessor and no longer
  consults `cfg!(target_os = "macos")`.
- The `bindings_for_action` question is answered with evidence: either T0035's
  discriminating assertions are now in `tests/shortcuts.rs`, or the ticket
  records that 0.3.12 still returns nothing and why it cannot be used.
- Every API whose signature changed is listed in `## Implementation Notes` with
  the URL consulted, so the next bump has a starting point.
- `AGENTS.md` names the version actually in use.

## Implementation Notes

### Runtime failure found during the CEO's visual pass

The widget panicked before opening its window. Every one of the six gates was
green at the time, so this section records the failure, the cause, the fix, and
the coverage hole that let it through.

**The panic, verbatim:**

```
$ bun run ui   # cargo run -p speechtt-ui
thread 'main' (168020) panicked at .../gpui-pre-0.3.12/src/keymap/binding.rs:47:10:
called `Result::unwrap()` on an `Err` value: InvalidKeystrokeError { keystroke: "secondary-c" }
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
error: script "ui" exited with code 101
```

**Root cause.** `lib.rs::run` registered the transcript's copy shortcut with
`KeyBinding::new("secondary-c", app::CopySelection, Some(app::SPEECHTT_CONTEXT))`.
`gpui-kit 0.9.1` / `gpui-pre 0.3.12` does not accept `secondary` in
`KeyBinding::new`; the KeyBinding page says so explicitly
(<https://gpui-kit.com/docs/keybinding.md>, "Write key strings"):

> GPUI also parses `fn`, `ctrl`, `alt`, `shift`, `cmd`, `super`, and `win` as
> modifiers. **`KeyBinding::new` requires explicit modifiers**; choose `cmd` or
> `ctrl` using `cx.platform_info().is_mac()` for a cross-platform
> Command/Control shortcut. Load dynamic strings or the `secondary` alias with
> `KeyBinding::load(..., cx)` so they resolve against the runtime operating
> system […]. `KeyBinding::new` **panics** if its key string or context
> predicate cannot be parsed.

**Fix.** `shortcuts::bind_app_keys` picks the explicit modifier from the host
report T4 exposed, keeping `KeyBinding::new` (a static string, a compile-time
action type and a `&'static str` predicate are exactly its contract):

```rust
let copy_keys = if is_mac(cx) { "cmd-c" } else { "ctrl-c" };
```

`KeyBinding::load` was not used: it needs a `Box<dyn Action>` and an
`Rc<KeyBindingContextPredicate>`, a heavier and more failure-prone shape for a
static command, and the documentation prescribes `new` plus explicit `cmd`/`ctrl`
for this case.

**Why the six gates did not catch it — the coverage hole.** `lib.rs::run`'s keymap
installation had no headless coverage. `shortcuts::tests::every_binding_parses_on_the_application_keymap`
called only `bind_keys`, which registers the `BINDINGS` table; the copy binding
lived inline in `run()`'s closure and was therefore parsed for the first time
when a real application context started the widget. `tests/common/mod.rs::init_kit`
called `bind_keys` too, so no integration test reached it either. The gates
tested the table and not the keymap, and the two had drifted.

**The seam fix.** Every registration the application performs is now behind one
function, `shortcuts::bind_app_keys(cx: &mut App)` — the `BINDINGS` table via
`bind_keys`, then the copy shortcut. `lib.rs::run` calls it, and so does the test
fixture, which makes `init_kit`'s "exactly as `lib.rs::run` does" doc true.

**Red evidence.** The regression test was written against a faithful extraction of
the seam that still carried the old string, and it failed with the CEO's exact
error before the string was fixed:

```
---- shortcuts::tests::the_full_application_keymap_parses stdout ----

thread 'shortcuts::tests::the_full_application_keymap_parses' (171111) panicked at
.../gpui-pre-0.3.12/src/keymap/binding.rs:47:10:
called `Result::unwrap()` on an `Err` value: InvalidKeystrokeError { keystroke: "secondary-c" }

test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 127 filtered out
```

After the fix the same test passes, and `cargo test` reaches **605** (baseline
603, plus `is_mac_follows_the_host_platform` and `the_full_application_keymap_parses`).

**Other startup registrations audited.** Nothing in the startup path can panic on
a parse besides the strings the keymap already carried. The remaining calls are
`gpui_kit::init(cx)`, `theme::apply(cx)`, a settings load whose error is discarded
with `.ok()`, the display list, and `gpui_kit::open_window(...).expect(...)` for
window creation. There is no other `KeyBinding::new`, no `KeyContext::parse`, and
no dialog or predicate string. The two `.key_context(...)` declarations (`app.rs`,
`views/dialogs/window.rs`) pass plain identifier constants (`SpeechTT`,
`SpeechTT Take`, `SpeechTT SessionEnd`, `SpeechTTDialog`), which parse as
identifiers, and every UI integration test renders both roots, so those strings
are parsed inside `cargo test` already. `shortcuts::caps_of` still understands a
`secondary` token, as a display formatter only; its doc says so, and no
registration emits the alias.

#### Why the fix is structural

The string fix closed the instance. Two structural facts made it possible for the
six gates to be green while `bun run ui` panicked, and both had to go:

1. **`lib.rs::run`'s closure body was untestable by construction.** Everything the
   application installed at startup lived inside the `|cx| { ... }` handed to
   `gpui_kit::application().run(...)`, and nothing can call that closure. The copy
   binding was therefore parsed for the first time when a real application context
   started the widget.
2. **The test fixture was a mirror, not a caller.** `tests/common/mod.rs::init_kit`
   re-listed the startup steps itself (`gpui_kit::init`, `theme::apply`,
   `bind_keys`). A hand-maintained copy drifts the moment one side changes, and it
   already had: production bound the table _and_ the copy shortcut, the copy bound
   only the table. Every UI test was green against a different startup than the
   real one.

**Scope extension.** The ticket's `Scope Boundary` anticipated no new module; this
work adds `crates/speechtt-ui/src/startup.rs` (with `tests/startup.rs`), because
the failure is a missing seam rather than a bad string and no in-place edit could
make the production startup callable.

**What the seam is now.** `startup::init_app(cx)` performs every install, in order:
`gpui_kit::init`, `shortcuts::bind_app_keys`, `theme::apply`. It does no I/O.
`startup::window_options(layout, displays)` is a pure function returning the
window contract; the persisted-layout read stays with `run()`, so tests stay
hermetic. `run()` only wires: build the application, load the layout, read the
display list, ask `startup` for the options, open the window. The fixture calls
`startup::init_app` and `startup::window_options`, so production and tests cannot
diverge, and the `init_kit`/`open_window` docs say so.

**What the gate forbids.** `tests/startup.rs::run_wires_and_the_fixture_calls_the_seam`
reads `src/lib.rs` and `tests/common/mod.rs` as source and enforces both directions:

- _Absence_: `run()` and the fixture may not carry a startup step. The needles are
  the ways a key string can enter the tree — `gpui_kit::init`, `cx.bind_keys`,
  `KeyBinding::new`, `KeyBinding::load`, `Keystroke::parse`, `KeyContext::parse`,
  `theme::apply`, `bind_app_keys`, `bind_keys` — with the same list for both
  files, so a copy of `bind_app_keys` or of a bare `cx.bind_keys` in the fixture
  is caught, not just `bind_keys`.
- _Presence_: `run()` must still contain `startup::init_app(cx)` and
  `startup::window_options(`, and the fixture must contain
  `speechtt_ui::startup::init_app`. Without this half, deleting the seam call
  from `run()` — a plausible refactor accident — ships a widget with no
  application keymap and the default theme, green.

Comment lines are skipped, as in `tests/design_system.rs`, so prose may name a
step; block comments are not stripped, and startup prose stays in `//` lines. Every
half was proven to bite before being left green (see the Red evidence below).

**What the gate cannot cover.** Two honest limits.

1. `gpui_kit::application().run()`'s event loop is never executed headlessly, so
   what the gate covers is _every registration `init_app` performs, the whole
   `BINDINGS` table on both platforms' key columns, and the window-option
   assembler_ — not _the application starts_. A defect that only appears once the
   framework's loop drives a real window, a real GPU surface or a real backend is
   outside `cargo test`.
2. The absence half is a needle list. A registration introduced through a **new
   helper module** — say `startup::bind_extras`, or a third crate-local function
   that calls `KeyBinding::new` — is not detected by it, and only the headless run
   of `init_app` would parse it, which it does only if `init_app` (or something it
   calls) reaches it. The list is a tripwire on the two files that matter, not a
   proof of exhaustiveness.

The gate therefore makes shipping an unparsable registration, a macOS-only
keystroke, or a dropped seam call _hard_, not impossible. It does not make the
widget's startup unconditionally safe.

**Red evidence for the gate itself.**

`init_app_installs_a_parseable_keymap_and_theme`, with `secondary-c` temporarily
restored in `startup`'s path:

```
thread 'init_app_installs_a_parseable_keymap_and_theme' (183904) panicked at
.../gpui-pre-0.3.12/src/keymap/binding.rs:47:10:
called `Result::unwrap()` on an `Err` value: InvalidKeystrokeError { keystroke: "secondary-c" }

test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 3 filtered out
```

`run_wires_and_the_fixture_calls_the_seam`, with `gpui_kit::init(cx);` temporarily
put back into `run()`:

```
thread 'run_wires_and_the_fixture_calls_the_seam' (184365) panicked at crates/speechtt-ui/tests/startup.rs:132:5:
startup steps belong in `startup::init_app`; `run()` only wires and the fixture only calls `init_app` — move each of these into the seam (1 offender(s), AGENTS.md §6 startup rule):
src/lib.rs:152: gpui_kit::init(cx);
```

Both edits were reverted; `cargo test` is green on the restored tree, which is the
proof that the fixture and the binary now share one startup path.

The fixture half of the same test was proven the same way, with `gpui_kit::init(cx);`
temporarily added to `init_kit`:

```
thread 'run_wires_and_the_fixture_calls_the_seam' (188489) panicked at crates/speechtt-ui/tests/startup.rs:145:5:
startup steps belong in `startup::init_app`; `run()` only wires and the fixture only calls `init_app` — the fixture must call the seam, never copy it (1 offender(s), AGENTS.md §6 startup rule):
tests/common/mod.rs:270: gpui_kit::init(cx);
```

#### Review round 2 — closing the remaining holes

The reviewer's finding was that the first gate closes the instance but not the
class: it asserted absence only, missed `bind_app_keys` in the fixture, never
parsed the macOS column, and the fixture still forced production's minimum size
onto windows that ask to be smaller. Each item and its evidence:

- **Positive half.** `run()` must still contain `startup::init_app(cx)` and
  `startup::window_options(`, and the fixture `speechtt_ui::startup::init_app`.
  Proven by deleting the seam call from `run()`:
    ```
    thread 'run_wires_and_the_fixture_calls_the_seam' (198877) panicked at crates/speechtt-ui/tests/startup.rs:160:5:
    `run()` must install the startup seam: call `startup::init_app(cx)` in its application closure, or the widget opens with no application keymap and the default theme (AGENTS.md §6 startup rule)
    ```
- **One needle list for both files**, now including `bind_app_keys`,
  `KeyBinding::load`, `Keystroke::parse` and `KeyContext::parse`. The
  `bind_app_keys` false negative was real (`"bind_keys"` is not a substring of
  `"bind_app_keys"`); proven by copying the call into `init_kit`:
    ```
    thread 'run_wires_and_the_fixture_calls_the_seam' (199679) panicked at crates/speechtt-ui/tests/startup.rs:176:5:
    startup steps belong in `startup::init_app`; the fixture must call the seam, never copy it — replace each of these with a call to `startup::init_app` (1 offender(s), AGENTS.md §6 startup rule):
    tests/common/mod.rs:271: speechtt_ui::shortcuts::bind_app_keys(cx);
    ```
- **Both platform columns.** `bind_keys` registers one column per host, so on
  Linux the whole `mac_keys` column was never parsed — the same mechanism as
  `secondary-c`. `shortcuts::tests::every_keystroke_of_both_platform_columns_parses`
  now builds a `KeyBinding` for every string in both columns plus `cmd-c` and
  `ctrl-c`, on the running host. `cmd` parses on every platform per the KeyBinding
  page's "Write key strings" section, so this needs no macOS host.
  `shortcuts::tests::the_full_application_keymap_parses` was deleted: the startup
  gate owns the seam check, and a third copy of the startup steps is exactly what
  drifts.
- **Fixture routing.** `tests/end_to_end.rs` (9 tests) now calls
  `common::init_kit` instead of `gpui_kit::init` + `theme::apply`, so those tests
  open the real widget with the real startup. `tests/theme_contract.rs` keeps the
  direct path and says why in one sentence: it isolates the token mapping.
- **`window_min_size` in the fixture.** `WindowOptions::window_min_size` is
  documented only as "Window minimum size"
  (<https://docs.rs/gpui-kit/0.9.1/gpui_kit/struct.WindowOptions.html>) — **the
  documentation is silent on whether window creation clamps the requested
  bounds**, which is a documentation gap, not something to infer. So
  `open_window` drops the floor when an explicit size is requested, and
  `every_screen_renders_at_extreme_window_sizes` genuinely opens 320×190.
- **Two corrections.** The `window_min_size` "clamp" assertion in
  `tests/startup.rs` compared two calls that both passed `layout = None`, so it
  could not fail; it now restores an oversized layout onto a 1920×1080 display
  and asserts the resulting window-option size does not exceed the display. And
  `window_options_keep_a_usable_persisted_layout` was added: a regression that
  forwards `None` instead of the caller's layout would otherwise ship silently.

### T1 — Baseline (captured by the CTO on `gpui-kit 0.8.0`, commit 6645443)

- `cargo fmt -- --check` → pass, 0 warnings.
- `cargo clippy --all-targets -- -D warnings` → pass, 0 warnings.
- `cargo test` → **603 passed, 0 failed, 0 ignored** across 16 test binaries;
  per binary: 229, 5, 14, 5, 126, 0, 3, 12, 37, 14, 96, 23, 4, 35, 0, 0.
- `bun test` → **348 pass, 0 fail**.
- `bun check src/stt` → `✓ No type errors in 32 files`.
- `bun run design:validate` → `✓ Design contract valid — 66 tokens, 0 errors.`

### T2 — Resolved dependency graph

`Cargo.toml` workspace dependency is `gpui-kit = "0.9.1"`. `cargo update -p
gpui-kit` moved the whole family; `Cargo.lock` now resolves:

| Crate                                     | Resolved |
| ----------------------------------------- | -------- |
| `gpui-kit`                                | 0.9.1    |
| `gpui-base`                               | 0.9.1    |
| `gpui-component`                          | 0.9.1    |
| `gpui-kit-assets`                         | 0.9.1    |
| `gpui-pre` (and every `gpui-pre-*` layer) | 0.3.12   |

`cargo tree -d 2>/dev/null | grep -E '^gpui'` prints nothing: no `gpui-*`
crate appears at more than one version. The bump also swapped
`wgpu 29 → 30`, `naga 29 → 30`, `taffy 0.13/0.14 → 0.15` and added the
`icu_segmenter` / `plotters` trees — all transitive, none named by our code.

### T3 — Compile impact

**None.** `cargo check --all-targets` was green on the first run after the
bump, for the library, the binary and all 16 test targets, with the
`test-support` feature still accepted under that name by `gpui-kit 0.9.1`.
No prelude re-export, `gpui_kit::base` / `::component` path, `RenderOnce`
builder, theme accessor or `#[gpui_kit::test]` / `TestAppContext` /
`TestWindowExt` signature had to change. Nothing was adapted, so there is no
old-shape → new-shape entry to record beyond the two in §B below.

### T4 — `is_mac` reads the platform accessor

`crates/speechtt-ui/src/shortcuts.rs`:

```rust
#[must_use]
pub fn is_mac(cx: &App) -> bool {
    cx.platform_info().is_mac()
}
```

Signatures used, both quoted in the ticket and confirmed against the docs:

- `App::platform_info(self) -> PlatformInfo` —
  <https://docs.rs/gpui-kit/0.9.1/gpui_kit/struct.App.html>
- `PlatformInfo::is_mac(self) -> bool` —
  <https://docs.rs/gpui-kit/0.9.1/gpui_kit/struct.PlatformInfo.html>
  (`PlatformInfo` also exposes `os: OperatingSystem`; `is_mac` was present, so
  the enum was not needed.)

**Red → green.** `is_mac_follows_the_host_platform` was added to the
`#[cfg(test)] mod tests` first and failed to compile against the old
zero-argument signature (`error[E0061]: this function takes 0 arguments but 1
argument was supplied`), then passed after the implementation.

The expectation is computed in the test as `cfg!(target_os = "macos")`, which is
the fact the previous implementation hard-coded. The test therefore pins the
_answer_ for the host running the suite, not the mechanism: on a native host it
cannot distinguish a framework report from a build-target guess, and the two only
separate where the compile target and the host disagree (a cross-target run). It
asserts nothing about layout.

**Doc comment.** `is_mac`'s doc names its callers and the design role of the
`bool` per AGENTS.md §3: `bind_keys` picks `Binding::mac_keys` over
`Binding::keys` on the application keymap, and `views::dialogs::help` draws the
matching keycaps, while `Binding::keystroke` and `key_caps` take the `bool`
rather than an `App` so the formatting rules stay testable without an
application context.

**Call sites changed (3, all signature-only):**

1. `shortcuts.rs::bind_keys` — `let is_mac = is_mac();` → `is_mac(cx);`
   (`bind_keys` already took `&mut App`).
2. `views/dialogs/help.rs::shortcuts` — `shortcuts::is_mac()` →
   `shortcuts::is_mac(cx)` (the function already received `&App`).
3. `shortcuts.rs::tests` — the new test calls `is_mac(cx)`.

`key_caps(binding, is_mac)` keeps its `bool` parameter: it is a pure
formatter, only the source of the bool moved.

### T5 — `bindings_for_action` probe: one accessor is empty, the other ignores the window

A throwaway `crates/speechtt-ui/tests/probe_bindings.rs` opened the production
widget, pressed `f1` (Help opened — `cx.windows().len() == 2`, asserted), and
then queried **both** accessors, in the widget window and in the dialog window.

**Window used.** §C step 3 named a _Settings_ dialog; the probe used the **Help**
dialog, because that is the window the `f1` dispatch in step 1 actually opens, so
one fixture answers both questions. Nothing in the result depends on which dialog
it is — as recorded below, the context-scoped accessor returns the same numbers
in both windows and the focused-element accessor returns zero in both.

`KeyContext` was built with the documented constructor
`KeyContext::parse(&str) -> Result<KeyContext, anyhow::Error>`
(<https://docs.rs/gpui-kit/0.9.1/gpui_kit/struct.KeyContext.html>), and the
documented `KeyContext::new_with_defaults(OperatingSystem) -> KeyContext` for the
defaults-only control. `OperatingSystem`'s macOS variant is spelled `Mac`.

**Observed numbers** (`gpui-kit 0.9.1` / `gpui-pre 0.3.12`):

```
WIDGET      focused        ToggleHelp=0  ToggleRecording=0  SaveTranscript=0
HELP-DIALOG focused        ToggleHelp=0  ToggleRecording=0  SaveTranscript=0

WIDGET      in_context     ToggleHelp/"SpeechTT"=1            ToggleHelp/"SpeechTTDialog"=1
                           ToggleRecording/"SpeechTT Take"=1   SaveTranscript/"SpeechTT SessionEnd"=1
HELP-DIALOG in_context     ToggleHelp/"SpeechTT"=1            ToggleHelp/"SpeechTTDialog"=1
                           ToggleRecording/"SpeechTT Take"=1   SaveTranscript/"SpeechTT SessionEnd"=1

WIDGET      in_context     defaults only (new_with_defaults):  ToggleHelp=0  ToggleRecording=0
WIDGET      in_context     ToggleRecording/"SpeechTT"=0  ToggleRecording/"SpeechTTDialog"=0
                           SaveTranscript/"SpeechTT"=0    ToggleHelp/"Other"=0
```

(The first round also recorded the focused-element accessor over the full action
set: `ToggleHelp=0 ToggleRecording=0 StopRecording=0 SaveTranscript=0
CopySelection=0` in both windows — including `ToggleHelp`, whose `f1` binding had
just dispatched and opened the dialog, and `CopySelection`, which does dispatch
when the caption is pressed.)

**What each accessor does.** The two are different instruments, and only one of
the earlier conclusions survives:

- `Window::bindings_for_action(&action)` — "key bindings that invoke an action on
  the currently focused element" — reports **nothing for any action in a headless
  test window**, in either the widget window or a dialog window. It is an empty
  oracle here. That is what it does; the cause is inside the framework's
  focus-path resolution and is not something this repository can pin.
- `Window::bindings_for_action_in_context(&action, context)` **does consult the
  application keymap**: it returns `1` for exactly the actions whose registered
  predicate the _supplied_ `KeyContext` satisfies, and `0` otherwise. The
  defaults-only context and the deliberately wrong contexts (`ToggleRecording`
  under `SpeechTT` or `SpeechTTDialog`, `ToggleHelp` under an unrelated
  identifier) all answer `0`, so it is not blanket-returning.

    But its answer is **identical in the widget window and in the dialog window**,
    for every action and every context. It evaluates the predicate against the
    context it is handed and never looks at the window's focus path.

**Verdict.**
`bindings_for_action_in_context` cannot serve as T0035's oracle: the assertion it
would need is "in a _dialog_ window the take commands are absent", and since the
accessor returns the same `1` in the dialog window it does not observe the
window at all. Passing the take predicate to it re-states the literal it is
supposed to check rather than testing the runtime scoping.
`bindings_for_action` would be the right instrument conceptually — it resolves
through the focused element — but it reports nothing in a headless window.

So **`tests/shortcuts.rs` gains no assertion from this**, and T0035's
table-literal tests remain the pin for the predicate; the qualified doc paragraph
above `space_typed_into_the_settings_api_key_field_stays_a_character` stands, with
its framework phrase replaced by the present-tense rule. The probe file was
deleted; nothing about it is committed.

### T6 — Design system and theme unchanged

`bun run design:validate`, `cargo test --test design_system` (3 tests) and
`cargo test --test theme_contract` (4 tests) pass with **no edit** to
`design-tokens.css`, `DESIGN.md`, `tokens.rs` or any token value. No token
name or role moved in 0.9.1.

### T7 — Visual pass for the CEO

No GUI run, no screenshots (AGENTS.md §6). The bump changed the renderer
underneath (`wgpu 29 → 30`, `taffy 0.14 → 0.15`), so the surfaces worth a look
are:

1. **Window chrome** — `AppHeader` / `AppFooter` bands and the titlebar
   spacing in the main window.
2. **Screen shell** — the three-band `layout::ScreenShell` on the mode
   chooser, setup form, live take and transcript screens.
3. **Round record button** — the pulse and the press wave of
   `ToggleRoundButton` / `RippleRing` while a take records.
4. **Caption well** — scroll behaviour and the live tail on the transcript
   screen.
5. **The two custom selects** — provider and model pickers in the setup form;
   popup placement after the layout-engine bump.
6. **Help and Settings dialogs** — their own window, the tab strip, the
   scrolling body and the keycap rows (`KeyCap`).
7. **Elapsed timer / waveform** — the live take's time readout and the
   spectrum band.

### T8 — Documentation

- `AGENTS.md` §4 — `gpui-kit 0.8` → `gpui-kit 0.9.1`.
- `AGENTS.md` §6 registry — the `gpui-kit` / `gpui-base` / `gpui-component`
  rows were already correct and are versionless by design, so they are
  unchanged. Pinning a version inside the URL would freeze the registry and
  is left as a CTO decision.
- `README.md` — contains no `0.8` mention of `gpui-kit`; unchanged.
- `PRODUCT.md` §7 — both mentions updated (`149`, `303`).
- `DESIGN.md` — untouched (no token or component signature moved).

### Review follow-up — prose that no longer stated the present truth

Peer review found three source doc comments carrying version-relative framing or
past-draft reasoning. All three are corrected; no assertion, test body or
signature changed.

1. `crates/speechtt-ui/tests/shortcuts.rs` — the paragraph above
   `space_typed_into_the_settings_api_key_field_stays_a_character` said the
   runtime absence "cannot be asserted here, because **the pinned framework's**
   `Window::bindings_for_action` returns no bindings …". The possessive tied the
   rule to whichever version happened to be pinned. It now states the rule
   directly: `Window::bindings_for_action` returns no bindings for any action in
   a headless window, empty even for the copy shortcut, which does dispatch when
   pressed. Every fact kept; the scroll paragraph after it and the test body
   untouched. This is the only edit in that file.
2. `shortcuts::is_mac`'s doc — the lead said "this build" while the body said
   runtime host, contradicting each other, and "still" contrasted against the
   discarded behaviour. Rewritten to lead on the host platform, and extended with
   the callers and the design role of the `bool` (see T4).
3. `is_mac_follows_the_host_platform`'s doc — dropped the clause reasoning about
   an implementation that never shipped, keeping the limitation: the expectation
   comes from the build target, so the assertion pins the host's answer rather
   than the mechanism.

The T5 measurement above was also re-run against both accessors instead of the
focused-element one alone.

**Scope-audit check.** `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps -p
speechtt-ui` is not one of the §5 gates; it reports 10 pre-existing broken
intra-doc links that exist on `master` independently of this diff
(`crate::views` twice in `app.rs`, `Context::propagate`, `Self::on_copy_selection`,
`PROGRESS_OSCILLATIONS`, `Card`, two `CleanSnapshot` links, `gpui::Hsla`,
`MIN_VISIBLE_EDGE`) plus the closing `could not document` line — 11 `^error`
lines. `is_mac`'s doc contributes none: `Binding::keystroke` is a private method,
so it is a plain code span rather than an intra-doc link, while
`Binding::mac_keys`, `Binding::keys` and `key_caps` stay links because they are
public. The 10 pre-existing errors are out of this ticket's scope.

### Final gates

Fresh single pass; every command below exited `0`. Timing lines vary between runs.

```
$ cargo fmt -- --check
(no output)

$ cargo clippy --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.91s

$ cargo test
     Running unittests src/lib.rs (speechtt-core)
test result: ok. 229 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
     Running tests/bridge_smoke.rs
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
     Running tests/session_machine.rs
test result: ok. 14 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
     Running tests/transcript_captures.rs
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
     Running unittests src/lib.rs (speechtt-ui)
test result: ok. 128 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
     Running unittests src/main.rs
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
     Running tests/design_system.rs
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
     Running tests/dialogs.rs
test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
     Running tests/end_to_end.rs
test result: ok. 37 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
     Running tests/error_ux.rs
test result: ok. 14 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
     Running tests/session_flow.rs
test result: ok. 96 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
     Running tests/shortcuts.rs
test result: ok. 23 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
     Running tests/startup.rs
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
     Running tests/theme_contract.rs
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
     Running tests/widget.rs
test result: ok. 35 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
     Doc-tests speechtt_core
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
     Doc-tests speechtt_ui
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out

$ bun test
 348 pass
 0 fail
 1269 expect() calls
Ran 348 tests across 16 files.

$ bun check src/stt
✓ No type errors in 32 files

$ bun run design:validate
✓ Design contract valid — 66 tokens, 0 errors.
```

Rust totals 610 passed / 0 failed / 0 ignored against the 603 baseline. The seven
new tests are `shortcuts::tests::is_mac_follows_the_host_platform`,
`shortcuts::tests::every_keystroke_of_both_platform_columns_parses`, and the five
in `tests/startup.rs` (`init_app_installs_a_parseable_keymap_and_theme`,
`window_options_are_the_window_contract`,
`window_options_keep_a_usable_persisted_layout`,
`run_wires_and_the_fixture_calls_the_seam`, `the_gate_reads_real_sources`).
`shortcuts::tests::the_full_application_keymap_parses` was removed because the
startup gate owns that check. Bun stays at 348. No test was weakened, skipped or
`#[ignore]`d.
