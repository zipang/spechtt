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
source/headless gate over it, which closes every *registration* the application
performs. It does not close the loop, by its own admission:

> `gpui_kit::application().run()`'s event loop is never executed headlessly, so
> what the gate covers is […] not *the application starts*.

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
status:

1. Spawn with `stdin` null, `stdout` and `stderr` piped.
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

### C. Signal, not just a timeout (preferred if clean)

A pure timeout cannot distinguish "still initialising" from "running". If a small,
honest positive signal exists — one line the binary emits once its window is open,
on `stderr`, through the existing `diagnostics` module — add it and let the test
wait for that line instead of for a fixed window, with the timeout as a backstop.
This is a *diagnostic*, must not leak secrets or paths, and must not change what
the widget does. If no clean signal is possible without touching behaviour, say so
in the notes and ship the liveness window alone.

### D. When there is no display

The suite must not fail on a machine with no display server (`gpui` opens a real
window). Detect the absence of both `DISPLAY` and `WAYLAND_DISPLAY` and **skip
with an explicit `eprintln!` reason**, never a silent pass and never an
`#[ignore]` that hides the gate by default. Record in the notes which machine
state runs it and which skips it.

## TDD Tasks

- [x] **T1 — Red first.** Write the test against the current tree; prove it
      passes on a healthy binary. Then reintroduce a startup panic (temporarily,
      e.g. the `secondary-c` string) and prove the test **fails** with the panic
      text in its message. Revert. Record both outputs.
- [x] **T2 — Liveness loop, hermeticity, cleanup.** §A and §B, including the
      exit-0 case and the kill-on-every-path requirement.
- [x] **T3 — Optional launch signal.** §C, only if it is clean.
- [x] **T4 — Display-less behaviour.** §D.
- [x] **T5 — Documentation.** `AGENTS.md` §5 names the gate and its display
      requirement; §6 gets a rule if one follows.
- [x] **T6 — Gates.** All six green; the new test counted.

## Acceptance Criteria

- `cargo test` fails when the built binary panics before opening its window, and
  passes when it launches — demonstrated, not asserted.
- The test never reads or writes the user's real config directory, and kills its
  child on every path.
- A machine with no `DISPLAY` and no `WAYLAND_DISPLAY` skips with a visible
  reason; it never reports a silent success.
- All six gates green with zero warnings and no test weakened, skipped or
  `#[ignore]`d; `AGENTS.md` names the gate.

## Implementation Notes

### Shape of the gate

`crates/speechtt-ui/tests/launch.rs`, three tests, all run by plain `cargo test`:

1. `the_built_binary_reaches_a_running_window` — the launch itself. Spawns
   `env!("CARGO_BIN_EXE_speechtt-ui")` with `stdin` null, `stdout`/`stderr`
   piped, `HOME`/`XDG_CONFIG_HOME` pointed at a fresh temp dir. Waits for the
   positive signal (below) and then requires the process to stay alive for a
   settle window. An exit at any point fails, and status 0 gets its own message.
2. `the_launch_signal_is_one_path_free_line` — pins the handshake: one physical
   line, `[speechtt] ` prefixed, no `/`, no `\`, and none of the inherited
   `HOME`/`XDG_CONFIG_HOME`/`DISPLAY` values echoed into it.
3. `an_exit_inside_the_window_fails_the_launch` — pins the verdict wording for
   exit 0 versus a non-zero exit, so the "clean exit means no window" judgement
   cannot silently regress into "an exit is fine".

### Timeouts, and why

- `LAUNCH_TIMEOUT = 20 s` — the backstop for the signal, not the assertion. A
  debug binary cold-loads the GPU backend, fonts and the theme; 10 s was the
  starting point but a cold CI runner is well within an order of magnitude of it,
  and a false red on a slow machine would push someone to weaken the gate. 20 s
  costs nothing on a healthy run, which reaches the signal in well under a
  second.
- `SETTLE_WINDOW = 3 s` — the window must survive a first-frame render. That is
  the failure mode that lands *after* `open_window` returns, and it is invisible
  to the signal alone.
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
*after* it, which is exactly why the settle window exists.

### §D behaviour, and which machines run it

With neither `DISPLAY` nor `WAYLAND_DISPLAY` set and non-empty, the launch test
prints
`[launch] skipped: neither DISPLAY nor WAYLAND_DISPLAY is set, so gpui has no display server to open the widget's window on; this gate needs a graphical session to run`
and returns — a visible reason, never a silent pass, never `#[ignore]`. The two
signal tests still run, because they need no display.

- **Runs it**: the CEO's session (`DISPLAY=:0.0` here — see the gate output
  below, which printed `[launch] running the built widget with DISPLAY set`).
- **Skips it**: a headless box with both variables unset. Verified: with
  `env -u DISPLAY -u WAYLAND_DISPLAY cargo test -p speechtt-ui --test launch`
  the suite reports `3 passed`, and the skip line above is printed.

### Hermeticity

The child's `HOME` and `XDG_CONFIG_HOME` are created under `env::temp_dir()` by
a `Sandbox` guard whose `Drop` removes the tree. `JsonFileStore::default_path()`
resolves through `XDG_CONFIG_HOME` first, so the persisted window layout the
child reads and writes stays inside the sandbox. The test additionally snapshots
the *real* settings file's metadata (size + mtime, never its contents) before the
spawn and asserts it is unchanged after — a leakage of the user's config fails
the test rather than passing quietly. Verified after the full run: no
`/tmp/speechtt-launch-*` directory remains.

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
`"secondary-c"` for Linux, the exact defect the ticket cites:

```
running 2 tests
test the_launch_signal_is_one_path_free_line ... ok
test the_built_binary_reaches_a_running_window ... FAILED

---- the_built_binary_reaches_a_running_window stdout ----
[launch] running the built widget with DISPLAY set

thread 'the_built_binary_reaches_a_running_window' panicked at crates/speechtt-ui/tests/launch.rs:122:9:
the widget exited (exit status: 101) before reporting that its window is open: a panic while starting up looks like this
--- widget stderr ---

thread 'main' (227795) panicked at .../gpui-pre-0.3.12/src/keymap/binding.rs:47:10:
called `Result::unwrap()` on an `Err` value: InvalidKeystrokeError { keystroke: "secondary-c" }
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace

test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.10s
```

The failure carries the child's own panic text, and it takes 0.10 s instead of
waiting out the timeout, because the exit is detected rather than inferred.

**Reverted** — `shortcuts.rs` restored (`"ctrl-c"` on Linux), test green again:

```
running 2 tests
test the_launch_signal_is_one_path_free_line ... ok
test the_built_binary_reaches_a_running_window ... ok

test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 3.75s
```

### Documentation (T5)

- `AGENTS.md` §5 (Rust) names the **launch gate**: what it launches, the signal
  it waits for, the settle requirement, and that it is the only gate needing a
  graphical session plus its skip behaviour.
- `AGENTS.md` §6 (Do) gains the rule that startup keeps the launch handshake:
  `run()` writes `diagnostics::log_launched()` once `open_window` returned, a
  window opened by another route keeps it, and a display-less machine skips
  with a printed reason rather than `#[ignore]`.
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

`cargo test` — **613 passed, 0 failed, 0 ignored**, up from 610: the launch file
contributes 3 new tests (its line is `running 3 tests … 3 passed; 3.61s`).

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
running   3 tests	test result: ok.   3 passed; 0 failed; 0 ignored   (launch.rs)
running  96 tests	test result: ok.  96 passed; 0 failed; 0 ignored
running  23 tests	test result: ok.  23 passed; 0 failed; 0 ignored
running   5 tests	test result: ok.   5 passed; 0 failed; 0 ignored
running   4 tests	test result: ok.   4 passed; 0 failed; 0 ignored
running  35 tests	test result: ok.  35 passed; 0 failed; 0 ignored
running   0 tests	test result: ok.   0 passed; 0 failed; 0 ignored
running   0 tests	test result: ok.   0 passed; 0 failed; 0 ignored

totals: passed=613 failed=0 ignored=0
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
