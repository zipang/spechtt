# T0037 — A gate that launches the built binary

- **Track**: Rust UI (test infrastructure)
- **Wave**: 1 (independent)
- **Depends on**: T0036
- **Blocks**: nothing
- **References**:
    - `AGENTS.md` §5 (quality gates), §6 (hermetic testing, no GUI runs)
    - `tickets/done/T0036-gpui-kit-0-9-migration.md` — the failure this gate exists
      to catch, and the honest limit its startup gate declares
    - `https://docs.rs/gpui-kit/0.9.1/gpui_kit/fn.application.html`
      (`gpui_kit::application() -> Application`, `.run(cx)`)

## Context & Goal

T0036 shipped with every gate green and the application panicking before its
first frame: `KeyBinding::new("secondary-c", …)` is rejected by
`gpui-pre 0.3.12`. The fix added a startup seam (`startup::init_app`) and a
source/headless gate over it, which closes every _registration_ the application
performs. It does not close the loop, by its own admission:

> `gpui_kit::application().run()`'s event loop is never executed headlessly, so
> what the gate covers is […] not _the application starts_.

A defect that only appears once the framework's loop drives a real window — a
panic in `run()`'s closure below the seam, a failed window creation, a GPU
surface/backend failure — is invisible to `cargo test` and to the source gate.

**Goal.** One test that starts the compiled `speechtt-ui` binary and fails if it
dies before it is running, so "compiles and tests green but cannot launch" stops
being a reachable state.

## Scope Boundary

May change:

- `crates/speechtt-ui/tests/launch.rs` (new).
- `crates/speechtt-ui/Cargo.toml` — dev-dependencies only, if the test needs one.
- `crates/speechtt-ui/src/**` — **only** if a positive launch signal is added
  (see `Contract` §C); nothing else, and no behaviour change.
- `AGENTS.md` §5 (name the gate and its environment requirement), §6 if a rule
  follows from it.
- `tickets/README.md`, this ticket.

Do **not** touch: the worker (`src/stt/**`), the design tokens, the session
logic, or any `tickets/done/**`.

## Contract

### A. The launch assertion

`crates/speechtt-ui/tests/launch.rs` spawns the built binary through Cargo's
`env!("CARGO_BIN_EXE_speechtt-ui")` and judges it by **liveness**, not by exit
status. The child runs **headless**: `DISPLAY` and `WAYLAND_DISPLAY` are removed
from its environment, so `gpui` starts the application with no display
connection and `cargo test` opens no window on the developer's desktop.
`Application::with_windowing` is the documented basis ("On Linux, the default
environment is the process's own, and the app starts headless if that names no
allowed display server"):
<https://docs.rs/gpui-kit/0.9.1/gpui_kit/struct.Application.html>

1. Spawn with `stdin` null, `stdout` and `stderr` piped, and the two display
   variables removed.
2. Poll the child every 100 ms up to a bounded window (start at 10 s; justify
   whatever you land on).
3. **Fail** if the child exits inside the window, with the captured `stderr` in
   the panic message — a panic in `run()` is exactly this shape.
4. **Fail** if the child exits with status `0` inside the window: the widget has
   no quit path at startup, so a clean exit means it never opened its window.
5. Otherwise kill the child, reap it, and pass.

The child must not outlive the test: kill and `wait` on every path, including the
panic paths.

### B. Hermeticity

The binary reads its persisted window layout from `JsonFileStore::default_path()`,
which resolves under the user's config directory. The test must point the child at
a throwaway directory (`$HOME` and `$XDG_CONFIG_HOME`) so it never reads or writes
the real one. Nothing else about the child's environment is inherited specially.

The gate asserts **the redirection, not the absence of another process's writes**:
the property is "the command this file builds spawns the child with `HOME` and
`XDG_CONFIG_HOME` pointing into a temporary directory that exists and lives under
`env::temp_dir()`, and with `DISPLAY`/`WAYLAND_DISPLAY` removed". It is true or
false regardless of what else is running on the machine, and it still catches the
regression it exists for — a dropped or mistyped `.env(..)`/`.env_remove(..)`.
Observing the user's real settings file instead would be a side-effect
observation: a second running instance legitimately saves it
(`SpeechTtApp::flush_pending_window_layout` auto-saves 600 ms after a window move
or resize; the Settings dialog saves on user action), so the gate would fail for
a reason outside its subject.

### C. Signal, not just a timeout (preferred if clean)

A pure timeout cannot distinguish "still initialising" from "running". If a small,
honest positive signal exists — one line the binary emits once its window is open,
on `stderr`, through the existing `diagnostics` module — add it and let the test
wait for that line instead of for a fixed window, with the timeout as a backstop.
This is a _diagnostic_, must not leak secrets or paths, and must not change what
the widget does. If no clean signal is possible without touching behaviour, say so
in the notes and ship the liveness window alone.

### D. Displayless machines

There is **no skip and no `#[ignore]`**. The gate spawns the child without
`DISPLAY` and `WAYLAND_DISPLAY`, so it runs the real event loop with the
documented headless windowing mode and needs no display server, no Xvfb, no audio
server and no user configuration. It therefore runs on every machine, a headless
CI box included, and its verdict does not depend on whether the developer has a
graphical session.

Running it headless is also the reason it opens no window: `cargo test` must
never flash the widget on someone's desktop. The gate asserts the two
`env_remove` calls on the command it builds, so a regression that reintroduces a
display connection fails deterministically.

## TDD Tasks

- [x] **T1 — Red first.** Write the test against the current tree; prove it
      passes on a healthy binary. Then reintroduce a startup panic (temporarily,
      e.g. the `secondary-c` string) and prove the test **fails** with the panic
      text in its message. Revert. Record both outputs.
- [x] **T2 — Liveness loop, hermeticity, cleanup.** §A and §B, including the
      exit-0 case and the kill-on-every-path requirement.
- [x] **T3 — Optional launch signal.** §C, only if it is clean.
- [x] **T4 — Display-less behaviour.** §D — the gate runs headless on every
      machine, with no skip.
- [x] **T5 — Documentation.** `AGENTS.md` §5 names the gate and its display
      requirement; §6 gets a rule if one follows.
- [x] **T6 — Gates.** All six green; the new test counted.

## Acceptance Criteria

- `cargo test` fails when the built binary panics before opening its window, and
  passes when it launches — demonstrated, not asserted.
- The test never reads or writes the user's real config directory, and kills its
  child on every path.
- The gate runs headless on every machine and CI: it opens no window and needs no
  display server, and it is never skipped or `#[ignore]`d.
- All six gates green with zero warnings and no test weakened, skipped or
  `#[ignore]`d; `AGENTS.md` names the gate.

## Implementation Notes

### Shape of the gate

`crates/speechtt-ui/tests/launch.rs`, four tests, all run by plain `cargo test`:

1. `the_built_binary_reaches_a_running_window` — the launch itself. Spawns the
   binary through `widget_command`, which sets `stdin` null, `stdout`/`stderr`
   piped, `HOME`/`XDG_CONFIG_HOME` at a fresh temp dir, and removes
   `DISPLAY`/`WAYLAND_DISPLAY` so the child runs headless. Waits for the
   positive signal (below) and then requires the process to stay alive for a
   settle window. An exit at any point fails, and status 0 gets its own message.
2. `the_launch_signal_is_one_path_free_line` — pins the handshake: one physical
   line, `[speechtt] ` prefixed, no `/`, no `\`, and none of the inherited
   `HOME`/`XDG_CONFIG_HOME` values echoed into it.
3. `an_exit_inside_the_window_fails_the_launch` — pins the verdict wording for
   exit 0 versus a non-zero exit, so the "clean exit means no window" judgement
   cannot silently regress into "an exit is fine".
4. `the_child_resolves_its_settings_inside_the_sandbox` — pins the redirection
   itself: `HOME` and `XDG_CONFIG_HOME` on the built command are exactly the
   sandbox's, the config root is under `env::temp_dir()`, and both directories
   exist. Deterministic and display-free (see below).

### Timeouts, and why

- `LAUNCH_TIMEOUT = 20 s` — the backstop for the signal, not the assertion. A
  debug binary cold-loads the fonts, the theme and the worker process; 10 s was the
  starting point but a cold CI runner is well within an order of magnitude of it,
  and a false red on a slow machine would push someone to weaken the gate. 20 s
  costs nothing on a healthy run, which reaches the signal in well under a
  second.
- `SETTLE_WINDOW = 3 s` — the window must survive the first frame's layout. That
  is the failure mode that lands _after_ `open_window` returns, and it is
  invisible to the signal alone.
- `POLL_INTERVAL = 100 ms` — the sampling rate for the child's status and its
  captured stderr, per §A.

### §C verdict: the positive signal was added

`crates/speechtt-ui/src/diagnostics.rs` gained `launched_line()` (returns
`[speechtt] startup status=window-open`) and `log_launched()` (writes it on
`stderr`). `lib.rs::run` calls `log_launched()` after `open_window` returns, so
the line means "the window exists and `SpeechTtApp::new` returned". It is a pure
diagnostic: one fixed token, no path, no settings value, no credential, no
behaviour change. The test takes the string from `launched_line()` rather than
restating it, so the gate cannot drift from the binary it launches, and
`the_launch_signal_is_one_path_free_line` pins that it stays path-free.

The signal alone is not treated as success: a panic in the first frame happens
_after_ it, which is exactly why the settle window exists.

### §D behaviour: headless, no skip

There is no skip branch and no `#[ignore]`. `widget_command` removes `DISPLAY`
and `WAYLAND_DISPLAY` from the child's environment, and `gpui` then starts the
application in its documented headless windowing mode:
`Application::with_windowing` — "On Linux, the default environment is the
process's own, and the app starts headless if that names no allowed display
server" (<https://docs.rs/gpui-kit/0.9.1/gpui_kit/struct.Application.html>);
`WindowingRequest::Headless` — "No display server. Windows lay out and handle
input but draw nothing"
(<https://docs.rs/gpui-kit/0.9.1/gpui_kit/enum.WindowingRequest.html>).

The gate therefore runs on every machine, CI included, with no graphical
session and no Xvfb, and `cargo test` never opens a window on the developer's
desktop. Verified two ways:

- `env -u DISPLAY -u WAYLAND_DISPLAY cargo test -p speechtt-ui --test launch` —
  the same environment a CI box has — reports `4 passed`, including the launch
  test itself.
- While the gate ran with the CEO session's `DISPLAY=:0.0` present, the spawned
  `target/debug/speechtt-ui` process (`pid=318115`) had **zero**
  `DISPLAY`/`WAYLAND_DISPLAY` entries in `/proc/<pid>/environ`, read live. The
  other processes that matched the pgrep pattern and did carry `DISPLAY` were the
  shell and `cargo` running the test, not the child.

`the_child_resolves_its_settings_inside_the_sandbox` pins the two `env_remove`
calls, so a change that lets the child reach a display server fails
deterministically.

### Hermeticity

The child's `HOME` and `XDG_CONFIG_HOME` are created under `env::temp_dir()` by
a `Sandbox` guard whose `Drop` removes the tree. `JsonFileStore::default_path()`
resolves through `XDG_CONFIG_HOME` first, so the persisted window layout the
child reads and writes stays inside the sandbox. `widget_command` is the single
place that builds the spawn, and `the_child_resolves_its_settings_inside_the_sandbox`
asserts both overrides resolve to the sandbox's own `home`/`config`, that
`config` starts with `env::temp_dir()`, that both directories exist, and that
both display variables are removed — a deterministic property of the command,
with no dependence on any other process. Verified after the full run: no
`/tmp/speechtt-launch-*` directory remains.

### Corrections recorded during review

**1 — the gate must not perturb the desktop.** The first version spawned the
binary windowed, so a real window flashed on the user's screen on every
`cargo test`. The child now runs headless through the two `env_remove` calls
above; the handshake, settle window, exit-status judgement, kill/reap and
timeout are unchanged, and a re-run Red check proves the gate still fails on a
startup panic.

**2 — the settings-file side-effect assertion is gone.** The first version
additionally snapshotted the **real** settings file's metadata before the spawn
and asserted it unchanged afterwards. That is an external side-effect
observation, not a property of the code under test, and it fails whenever
another `speechtt-ui` instance writes that file — which one legitimately does.
`SpeechTtApp::flush_pending_window_layout` (`crates/speechtt-ui/src/app.rs:1804`)
auto-saves 600 ms after any window move or resize, and the Settings dialog saves
on user action (`app.rs:1570`), so dragging the widget during the gate window
produced a false red. A gate that fails for a reason outside its subject is worse
than no gate.

Replaced by `widget_command(sandbox) -> Command` (the one builder for the spawn)
plus `the_child_resolves_its_settings_inside_the_sandbox`, which pins the
redirection and the display removal. `file_state` and the `JsonFileStore`/`Path`
imports it needed are deleted.

- **Red check (correction 1, headless):** with `secondary-c` restored in
  `shortcuts.rs`, the headless gate fails with the child's own panic text in
  0.20 s:
    ```
    test the_built_binary_reaches_a_running_window ... FAILED

    thread 'the_built_binary_reaches_a_running_window' panicked at crates/speechtt-ui/tests/launch.rs:110:9:
    the widget exited (exit status: 101) before reporting that its window is open: a panic while starting up looks like this
    --- widget stderr ---

    thread 'main' (316747) panicked at .../gpui-pre-0.3.12/src/keymap/binding.rs:47:10:
    called `Result::unwrap()` on an `Err` value: InvalidKeystrokeError { keystroke: "secondary-c" }
    note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace

    test result: FAILED. 3 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.20s
    ```
    Reverted → `4 passed; 0 failed`, `3.31s`.
- **Red check (correction 2, redirection):** with
  `.env("XDG_CONFIG_HOME", sandbox.config())` temporarily removed from
  `widget_command`, only the deterministic test fails:
    ```
    test the_child_resolves_its_settings_inside_the_sandbox ... FAILED

    thread 'the_child_resolves_its_settings_inside_the_sandbox' panicked at crates/speechtt-ui/tests/launch.rs:142:9:
    assertion `left == right` failed: the child must be spawned with XDG_CONFIG_HOME pointing into its throwaway settings root, got None instead of "/tmp/speechtt-launch-304402-1794235480636484726/config"
      left: None
     right: Some("/tmp/speechtt-launch-304402-1794235480636484726/config")

    test result: FAILED. 3 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 3.82s
    ```
    Reverted → `4 passed; 0 failed`.
- **Racy failure is gone**: two concurrent `cargo test -p speechtt-ui --test launch`
  runs both passed — `A: 4 passed … 4.22s`, `B: 4 passed … 4.22s` — and a real
  `target/debug/speechtt-ui` (PID 290232) was running the whole time, which is the
  exact condition that used to produce the false red.
- Rust test count: **614** (610 before the ticket; `launch.rs` reports
  `running 4 tests`, and the deterministic test replaced no test because
  `file_state` was a helper).

### Cleanup on every path

`child.kill()` and `child.wait()` run before any assertion can panic, and both
reader threads are joined. `try_wait` is used for the status, so the pass path
kills and reaps a live process rather than leaving it. Verified: after a full
`cargo test` run, `pgrep -af speechtt-ui` found no orphan.

### Red evidence (T1)

**Healthy tree — passes** (`cargo test -p speechtt-ui --test launch`):

```
running 2 tests
test the_launch_signal_is_one_path_free_line ... ok
test the_built_binary_reaches_a_running_window ... ok

test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 3.61s
```

An intermediate honest red was also captured: with `launched_line()` defined but
`log_launched()` not yet called from `run()`, the gate failed rather than
silently passing, because the child stayed alive but never reached its window:

```
the widget did not report `[speechtt] startup status=window-open` within 20s:
it is alive but never reached its first window
```

**Startup panic reintroduced** — `shortcuts.rs`'s copy key set back to
`"secondary-c"` for Linux, the exact defect the ticket cites. Run again after the
headless correction, so this is the current gate's behaviour:

```
running 4 tests
test an_exit_inside_the_window_fails_the_launch ... ok
test the_launch_signal_is_one_path_free_line ... ok
test the_child_resolves_its_settings_inside_the_sandbox ... ok
test the_built_binary_reaches_a_running_window ... FAILED

---- the_built_binary_reaches_a_running_window stdout ----

thread 'the_built_binary_reaches_a_running_window' (316744) panicked at crates/speechtt-ui/tests/launch.rs:110:9:
the widget exited (exit status: 101) before reporting that its window is open: a panic while starting up looks like this
--- widget stderr ---

thread 'main' (316747) panicked at .../gpui-pre-0.3.12/src/keymap/binding.rs:47:10:
called `Result::unwrap()` on an `Err` value: InvalidKeystrokeError { keystroke: "secondary-c" }
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace

test result: FAILED. 3 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.20s
```

The failure carries the child's own panic text, and it takes 0.20 s instead of
waiting out the timeout, because the exit is detected rather than inferred.

**Reverted** — `shortcuts.rs` restored (`"ctrl-c"` on Linux), gate green again:

```
running 4 tests
test the_child_resolves_its_settings_inside_the_sandbox ... ok
test an_exit_inside_the_window_fails_the_launch ... ok
test the_launch_signal_is_one_path_free_line ... ok
test the_built_binary_reaches_a_running_window ... ok

test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 3.31s
```

### Documentation (T5)

- `AGENTS.md` §5 (Rust) names the **launch gate**: it runs the built binary
  headless (both display variables removed, so no window), the signal it waits
  for, the settle requirement, and that it needs no display server, audio server
  or user configuration, so it runs on every machine and in CI.
- `AGENTS.md` §6 (Do) gains the rule that startup keeps the launch handshake:
  `run()` writes `diagnostics::log_launched()` once `open_window` returned, a
  window opened by another route keeps it, and the gate must never open a window
  on the developer's desktop — it removes `DISPLAY`/`WAYLAND_DISPLAY` and is
  never `#[ignore]`d or made conditional on a display server.
- `diagnostics.rs`'s module header and `lib.rs::run`'s doc name the new
  responsibility and its caller (§3: present tense, no history, no ticket IDs).

### Gates (T6)

`cargo fmt -- --check`:

```
$ cargo fmt -- --check
FMT=0
```

`cargo clippy --all-targets -- -D warnings`:

```
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.35s
CLIPPY=0
```

`cargo test` — **614 passed, 0 failed, 0 ignored**, up from 610: the launch file
contributes 4 tests (its line is `running 4 tests … 4 passed; 3.31s`, headless,
no window).

```
running 229 tests	test result: ok. 229 passed; 0 failed; 0 ignored
running   5 tests	test result: ok.   5 passed; 0 failed; 0 ignored
running  14 tests	test result: ok.  14 passed; 0 failed; 0 ignored
running   5 tests	test result: ok.   5 passed; 0 failed; 0 ignored
running 128 tests	test result: ok. 128 passed; 0 failed; 0 ignored
running   0 tests	test result: ok.   0 passed; 0 failed; 0 ignored
running   3 tests	test result: ok.   3 passed; 0 failed; 0 ignored
running  12 tests	test result: ok.  12 passed; 0 failed; 0 ignored
running  37 tests	test result: ok.  37 passed; 0 failed; 0 ignored
running  14 tests	test result: ok.  14 passed; 0 failed; 0 ignored
running   4 tests	test result: ok.   4 passed; 0 failed; 0 ignored   (launch.rs)
running  96 tests	test result: ok.  96 passed; 0 failed; 0 ignored
running  23 tests	test result: ok.  23 passed; 0 failed; 0 ignored
running   5 tests	test result: ok.   5 passed; 0 failed; 0 ignored
running   4 tests	test result: ok.   4 passed; 0 failed; 0 ignored
running  35 tests	test result: ok.  35 passed; 0 failed; 0 ignored
running   0 tests	test result: ok.   0 passed; 0 failed; 0 ignored
running   0 tests	test result: ok.   0 passed; 0 failed; 0 ignored

totals: passed=614 failed=0 ignored=0
```

`bun test`:

```
 348 pass
 0 fail
 1269 expect() calls
Ran 348 tests across 16 files. [849.00ms]
```

`bun check src/stt`:

```
$ bun check src/stt
✓ No type errors in 32 files [106.65ms]
```

`bun run design:validate`:

```
$ bun scripts/validate-design.ts
✓ Design contract valid — 66 tokens, 0 errors.
```

`RUSTDOCFLAGS="-D warnings" cargo doc --no-deps -p speechtt-ui` — exactly the 11
pre-existing `^error` lines, nothing new (no hit for `diagnostics` or
`launched`):

```
error: public documentation for `app` links to private item `crate::views`
error: public documentation for `app` links to private item `crate::views`
error: unresolved link to `Context::propagate`
error: public documentation for `copy_transcript` links to private item `Self::on_copy_selection`
error: public documentation for `bar_height` links to private item `PROGRESS_OSCILLATIONS`
error: unresolved link to `Card`
error: public documentation for `CleanToggle` links to private item `CleanSnapshot`
error: public documentation for `flip_clean_transcript` links to private item `CleanSnapshot`
error: unresolved link to `gpui::Hsla`
error: public documentation for `restored_window_bounds` links to private item `MIN_VISIBLE_EDGE`
error: could not document `speechtt-ui`
```

### Validation

Validated by the CEO/PM on 2026-11-09, after the visual pass and after the
gate was made headless:

- `cargo test` runs the whole suite without opening a window on the desktop;
  the spawned widget carries the sandboxed `HOME`/`XDG_CONFIG_HOME` and neither
  `DISPLAY` nor `WAYLAND_DISPLAY`.
- The suite leaves no `speechtt-ui` process and no `/tmp/speechtt-launch-*`
  directory behind.
- Gates at close: `cargo fmt`, `cargo clippy -D warnings`, **614** Rust tests,
  **348** Bun tests, `bun check src/stt`, `bun run design:validate` — all green,
  zero warnings.
