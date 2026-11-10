# T0040 — Find `bun`, and never leave Settings as a dead end

- **Track**: Rust core (backend resolution + settings) + Rust UI (Settings dialog, startup) + docs
- **Wave**: 2 (integration — touches the production spawn path, the settings file format and the startup flow)
- **Depends on**: T0038 (the executable-relative worker resolver and the AppImage this ticket's failure was found in), T0016 (the dependency vocabulary the README already uses)
- **Blocks**: nothing
- **Owner**: coding sub-agent, supervised by CTO
- **Started**: 2026-11-10
- **References**:
    - `AGENTS.md` §2 (module boundaries, the "no ad-hoc styles" rule), §3 (documentation in code), §5 (quality gates), §6 (Do/Don't), §9 (secret safety)
    - `DESIGN.md` §"Settings panel" — the row list this ticket extends
    - `crates/speechtt-core/src/backend.rs` — `worker_candidates`, `resolve_worker_in`, `spawn_with`, `BackendError`
    - `crates/speechtt-core/src/settings.rs` — `Settings`, `SettingsStore`, `JsonFileStore`
    - `crates/speechtt-ui/src/app.rs` — `SpeechTtApp::new` (the spawn attempt), `open_settings`, `apply_settings_change`, `UnavailableSink`
    - `crates/speechtt-ui/src/controller.rs` — `AppError`, `settings_connected`, `catalogue_loaded`, `request_capabilities`
    - `crates/speechtt-ui/src/views/dialogs/settings.rs` — the provider row's placeholder and the API-key row
    - `crates/speechtt-ui/src/components/` — `SettingRow`, `Input`, `InfoNote`, the components this ticket reuses

## Context & Goal

A user installed the T0038 AppImage and launched it from the desktop menu. The
Settings dialog opened on **"Loading providers…"**, disabled, with the API-key
field showing a **CONNECTED** badge and a masked value. Nothing was clickable,
`Save changes` stayed grey, and no error explained anything. The user could not
enter an API key to get out of the state they were in.

### The cause, established by evidence

The app was launched from the Xfce Whisker menu, whose environment carries

```text
PATH=/usr/local/bin:/usr/bin:/bin:/usr/local/sbin:/usr/bin/site_perl:…:/home/zipang/.photon/bin
```

`~/.bun/bin` is added to `PATH` by `~/.bashrc` (line 124), which only a login
shell reads. A desktop launcher never sees it, so `bun` was not on `PATH`.
Reproduced exactly: running the packaged binary with that `PATH` yields
`context=controller kind=BackendUnavailable`, and the process has **no `bun`
child** — from the original run and from the repro, while a run with `bun`
resolvable spawns `bun run …/share/speechtt/worker.js` normally.

The chain from there:

1. `BunSubprocess::spawn_default()` cannot start `bun` → `BackendError::Spawn`.
2. `SpeechTtApp::new` catches it, attaches `UnavailableSink` over an empty,
   already-closed channel, and reports `AppError::BackendUnavailable`.
3. The worker never runs, so no `capabilities` event is ever emitted.
4. `catalogue_loaded()` — `capabilities.is_some()` — stays `false` forever, so
   the provider menu renders disabled with the `"Loading providers…"`
   placeholder (settings.rs:84-89). No provider means no model means the
   draft can never be valid, so `Save changes` stays disabled.
5. `settings_connected()` returns `true` anyway, because it only requires a
   stored key and "not reported unavailable"; with no catalogue,
   `provider_available()` is `None`, and `None == Some(false)` is false. The
   badge lies.

So two defects compound. The first is a resolution gap: the app assumes `bun` is
on `PATH`, which is false for the launch path the artifact is designed for. The
second is worse, because it is the one that traps the user: when the backend is
dead, Settings — the only place the situation could be repaired — becomes a
silent dead end.

### What done means

- The app finds `bun` without the user touching their environment: a configured
  path in Settings, then `PATH`, then the well-known install locations.
- The path is editable in Settings, so a user whose `bun` is elsewhere can fix
  it from inside the widget.
- When the app starts without a usable `bun` or without an API key, Settings
  opens by itself, focused on the missing piece, and **says what is missing**.
- Neither the provider row nor the key badge claims a state the app has not
  verified.

## Scope Boundary

May create or modify:

- `crates/speechtt-core/src/backend.rs` — **modified**: `bun_candidates`,
  `resolve_bun_in`, `BackendError::BunMissing`, and a spawn entry point that
  takes the resolved program instead of the literal `"bun"`.
- `crates/speechtt-core/tests/bun_resolution.rs` — **new**: hermetic tests for
  the resolution rules, with injected `which`/`exists` predicates. No directory
  is created and no file is read.
- `crates/speechtt-core/src/settings.rs` — **modified**: the `bun_executable`
  field, its default, its normalization and its validation.
- `crates/speechtt-ui/src/app.rs` — **modified**: pass the resolved program to
  the spawn, and respawn the backend after a settings save that changed it.
- `crates/speechtt-ui/src/controller.rs` — **modified**: `AppError::BunMissing`
  (or the closest existing variant, extended), `SettingsDraft::bun_executable`,
  `SettingsChange::bun_executable`, and an accessor for "why the backend is
  unavailable" that the dialog can render.
- `crates/speechtt-ui/src/views/dialogs/settings.rs` — **modified**: the
  `Bun path` row, the explicit failure note replacing the eternal placeholder,
  and the honest badge.
- `crates/speechtt-ui/src/views/dialogs/mod.rs` / `window.rs` — only if the new
  row needs a dropdown-free field the existing `SettingsSelectFields` does not
  carry; state the choice in `## Implementation Notes`.
- `crates/speechtt-ui/src/startup.rs` — only if the auto-open needs a step
  behind the existing seam; `run()` must keep registering nothing itself
  (AGENTS.md §5, `tests/startup.rs`).
- `DESIGN.md` — the Settings panel row list, which currently says "Six rows".
- `README.md` — the desktop-launch `PATH` note and the recovery path.
- `tickets/README.md` — board row.

Out of scope — do **not** touch anything else, in particular:

- No change to the worker, the protocol, the STT providers or the model
  catalogue.
- No bundling of `bun` into the AppImage: T0038's decision stands and the
  artifact still requires a system `bun`.
- No new visual component, token, size, weight or colour. This ticket adds a
  **row** built from existing components — see Contract §6. Introducing a new
  style here is a design-system gate failure.
- No change to `scripts/package.ts`, `scripts/setup.*`, the release pipeline or
  T0039's macOS/Windows work.
- No general "settings schema version" mechanism: one optional string field does
  not justify one.

## Contract

### §1 — `bun` resolution (Rust, `speechtt-core`)

The literal `"bun"` stops being the only program the backend tries. Add to
`crates/speechtt-core/src/backend.rs`:

```rust
/// Candidate `bun` executables, most specific first.
///
/// A single-component path means "resolve through `PATH`", which is how
/// `Command` treats a bare program name. The well-known locations are listed
/// because a desktop launcher's environment does not contain the directory a
/// shell profile added to `PATH`.
pub fn bun_candidates(configured: Option<&Path>, home: Option<&Path>) -> Vec<PathBuf>;

/// Picks the first candidate the injected lookups accept.
///
/// `which` resolves a bare name through `PATH`; `exists` accepts a path. Both
/// are parameters, not `Command`'s own lookup, so the rule is testable without
/// a filesystem, a `PATH`, or a `bun` installation. Returns `None` when nothing
/// resolves; it never panics and never guesses.
pub fn resolve_bun_in(
    configured: Option<&Path>,
    home: Option<&Path>,
    which: impl Fn(&Path) -> Option<PathBuf>,
    exists: impl Fn(&Path) -> bool,
) -> Option<PathBuf>;
```

`bun_candidates` returns, in this order:

1. `configured` — the `bun_executable` setting, when non-empty. This wins over
   everything: it is the user's explicit answer to exactly this problem.
2. `bun` — a bare name, resolved through `PATH`.
3. `<home>/.bun/bin/bun` — the location `bun.sh/install` writes, and the one a
   desktop launch does not have on `PATH`.
4. `/usr/local/bin/bun`
5. `/opt/bun/bin/bun`

`resolve_bun_in` walks that list: a single-component candidate goes to `which`,
anything else to `exists`, first hit wins. `None` when the list is exhausted.

Add `BunMissing` to `BackendError`, carrying the list it searched so the UI can
name it:

```rust
#[error("could not find the bun runtime; looked for: {searched:?}")]
BunMissing { searched: Vec<PathBuf> },
```

The spawn entry point stops hardcoding the program:

```rust
/// Spawn `program run <script>` for the worker found next to this executable.
///
/// `program` is the resolved `bun`, from [`resolve_bun_in`]. The worker lookup
/// is [`worker_candidates`]; a missing worker is [`BackendError::WorkerMissing`]
/// and a missing runtime is [`BackendError::BunMissing`].
pub fn spawn_default_with(
    program: impl Into<String>,
    worker_exists: impl Fn(&Path) -> bool,
) -> Result<Self, BackendError>;
```

`spawn_default()` keeps its current meaning for existing callers and tests: the
repository worker, program `bun`. It must not gain a settings dependency —
`speechtt-core` stays framework-free and knows nothing about the settings file.

### §2 — The settings field

`Settings` gains one field:

```rust
/// Absolute path to the `bun` executable, or empty to resolve it
/// automatically.
///
/// Empty is the normal state: the app resolves `bun` through `PATH` and the
/// well-known install locations. A value here is the user's explicit override,
/// needed when `bun` is installed somewhere neither lookup finds — a desktop
/// launcher, for instance, does not read the shell profile that puts
/// `~/.bun/bin` on `PATH`.
#[serde(default, skip_serializing_if = "String::is_empty")]
pub bun_executable: String,
```

- `Default` is the empty string, so an existing file loads unchanged.
- `normalize` trims it. A value that is only whitespace becomes empty, so
  "auto" has exactly one representation.
- Wire name is `bun_executable`, matching the file's existing snake_case keys.
- `validate` does **not** require it: an empty value is valid and runs the
  automatic resolution.
- The whole settings struct keeps `deny_unknown_fields`; adding the field is
  what makes the older file still load (`#[serde(default)]` at struct level).

**The trap this ticket must not fall into**: `apply_settings_change`
(`app.rs:1549`) rebuilds a full `Settings` literal from a `SettingsChange`. A
new field that `SettingsChange` does not carry, and that literal does not copy,
is silently dropped on every save. Both must be extended together, and a test
must pin that a saved `bun_executable` survives a load.

### §3 — Recovery after a save

`save_settings` currently re-runs the handshake (`request_capabilities`), which
does nothing when no backend is attached. Saving a usable `bun_executable` must
therefore attempt a **new spawn**:

- If no backend is attached and the settings now resolve a `bun`, the app
  spawns the worker, attaches it and re-runs the handshake — the same path
  `SpeechTtApp::new` takes at startup, factored so both call it.
- If a backend is already attached, saving does not respawn it: the existing
  handshake refresh stands. Respawning a running worker would drop a live take.
- A failed respawn keeps the old error and the dialog open, so the user can
  correct the path again.

### §4 — Startup: open Settings when the app cannot work

When the widget starts and either condition holds, Settings opens by itself:

- the backend could not be spawned (`BunMissing`/`BackendUnavailable`), or
- no API key is stored for the configured provider.

The check runs once, after the settings load and the first spawn attempt, and
must not fight the user: it opens Settings at most once per launch, never
reopens it after the user dismisses it, and never opens on a launch where the
backend is healthy and a key exists.

The dialog opens on the **Action** that is missing, not merely on the first row.
"Focused on what is missing" means the row that blocks the app is the one the
user can act on, and the failure note (below) names the cause.

### §5 — The dialog says what is wrong

When no catalogue has arrived **and** the backend is unavailable, the provider
row must not render `"Loading providers…"` indefinitely. Instead:

- The provider row's placeholder states the situation instead of implying
  progress.
- An `InfoNote` states the cause, in the terms the user can act on: `bun` was
  not found, and where it was looked for; or the backend is unavailable for
  another reason.
- The note carries no secret and no stack detail. Paths come from
  `BackendError::BunMissing`'s own list.

`catalogue_loaded` stays the provider menu's enable condition: an unavailable
backend still means no menu, because there is nothing to offer. What changes is
that the user is told why, and has a field they can fill.

### §6 — The badge stops lying

`settings_connected()` must require a loaded catalogue:

- With no catalogue, the badge must not read `CONNECTED`. It reads as
  not-yet-known rather than as success.
- With a catalogue, the existing rule stands: a stored key and a provider not
  reported unavailable.

No new visual element: the badge keeps `StatusBadge`/`StatusDot` and the same
tokens; only the condition changes.

### §7 — No new visual component

The `Bun path` row is a `SettingRow` with an `Input` — the same pair the API-key
row already uses (settings.rs:186-191). The failure note is `InfoNote`, already
used in this dialog (settings.rs:251). `DESIGN.md`'s Settings panel section is
updated from "Six rows" to "Seven rows" naming the new one, in the same change.
Introducing a new component, size, weight, colour or spacing here is a
design-system gate failure and a review blocker.

### §8 — README

The `## Download` runtime notes explain the launch-path gap: a desktop launcher
does not read the shell profile that puts `~/.bun/bin` on `PATH`, so the app may
not see `bun` even though a terminal does. The recovery path is named — the
`Bun path` row in Settings — alongside the manual alternative of exporting
`PATH` from `~/.config/environment.d/`.

## TDD Tasks

- [x] **T40.1 — Resolution specs first**: write `crates/speechtt-core/tests/bun_resolution.rs`
      red. No `tempfile`, no filesystem, no `PATH`: every case injects its own
      `which` and `exists`. Cover: the configured path wins over everything,
      including over a `PATH` hit; an empty configured value falls through to
      `PATH`; `PATH` resolves before `$HOME/.bun/bin/bun`; the `$HOME` location
      resolves when `PATH` misses; `/usr/local/bin/bun` and `/opt/bun/bin/bun`
      resolve in that order; nothing resolving yields `None`; a `home` of `None`
      skips the `$HOME` candidate without panicking; a whitespace-only
      configured value is treated as empty; the candidate list is ordered and
      its first entry is the configured path.
- [x] **T40.2 — Resolution implementation**: `bun_candidates`, `resolve_bun_in`,
      `BackendError::BunMissing`, `spawn_default_with`, per Contract §1.
      `spawn_default` keeps its current behaviour and its existing tests pass
      untouched.
- [x] **T40.3 — Settings specs first**: red tests for the new field — default is
      empty; a file without the key loads; a file with it round-trips;
      `normalize` trims it to empty; `validate` accepts an empty value and a
      populated one; a serialized default omits the key.
- [x] **T40.4 — Settings implementation**: the field, its `Default`, `normalize`
      and the doc comment per §2. Confirm no other `Settings` literal in the
      workspace fails to compile — that is the trap from §2 surfacing, and every
      site it names must be fixed, including `apply_settings_change`.
- [x] **T40.5 — Save/respawn specs first**: red tests for §3 — saving with no
      backend attached and a resolvable `bun` spawns one; saving with a backend
      already attached does not replace it; a failed respawn keeps the error and
      the draft. `SettingsChange` must carry the field, and a test must prove
      the stored value survives a save.
- [x] **T40.6 — Dialog specs first**: red tests for §5 and §6 —
      `settings_connected()` is false with no catalogue and true after one
      arrives with a key; the failure note is present exactly when the backend
      is unavailable and no catalogue has arrived; the provider placeholder is
      not the loading text in that state. These are controller-level assertions,
      never layout or position assertions (AGENTS.md §6).
- [x] **T40.7 — Startup specs first**: red tests for §4 — the auto-open fires
      when `bun` is missing, fires when no key is stored, does not fire when
      both are satisfied, and does not fire twice after the user dismisses it.
- [x] **T40.8 — Implementation**: §3–§7 — the respawn, the startup check, the
      dialog row and note, the badge condition, the `DESIGN.md` row list.
- [x] **T40.9 — Docs**: `README.md` per §8, `tickets/README.md` board row.
- [x] **T40.10 — Gates**: `cargo fmt -- --check`,
      `cargo clippy --all-targets -- -D warnings`, `cargo test`, `bun test`,
      `bun check src/stt`, `bun run design:validate`. Zero warnings; nothing
      skipped or `#[ignore]`d.
- [x] **T40.11 — Real recovery**: reproduce the original failure and prove the
      fix. Build the AppImage with `bun run package`, run it with the desktop
      `PATH` (no `~/.bun/bin`) in a throwaway `HOME`, and confirm: Settings
      opens by itself, the note names the missing runtime, entering the real
      `~/.bun/bin/bun` path and saving spawns the worker and loads the
      catalogue. Record the exact commands and observed output.

## Acceptance Criteria

1. The app resolves `bun` from the configured settings path first, then `PATH`,
   then `$HOME/.bun/bin/bun`, `/usr/local/bin/bun` and `/opt/bun/bin/bun`, in
   that order — proven by hermetic tests that touch no filesystem and run no
   process.
2. `Settings` persists `bun_executable`, an older settings file still loads, and
   a saved value survives a save/load round trip — the `apply_settings_change`
   literal included.
3. On a machine where `bun` is not on the launch `PATH`, the widget opens
   Settings by itself and states that the runtime was not found, instead of
   showing an indeterminate provider list.
4. Filling the `Bun path` row with a valid path and saving starts the worker,
   loads the catalogue and enables the dialog — without restarting the app.
5. With no catalogue loaded, the API-key badge does not read `CONNECTED`.
6. The provider row never renders the loading placeholder while no catalogue can
   arrive.
7. A healthy install — `bun` resolvable and a key stored — opens no dialog at
   startup and behaves exactly as before.
8. No new visual component or style is introduced; `DESIGN.md`'s Settings panel
   documents the new row; `bun run design:validate` and the design-system gate
   stay green.
9. Every gate in T40.10 is green with zero warnings, and no test asserts layout,
   position, size or geometry.
10. The T40.11 reproduction shows the recovery end to end from a packaged
    artifact, with the observed output recorded in the ticket.

## Implementation Notes

### What landed

- **Core resolution** — `bun_candidates`, `resolve_bun_in`, `which_on_path`,
  `BackendError::BunMissing { searched }` and `BunSubprocess::spawn_default_with`
  in `crates/speechtt-core/src/backend.rs`. `spawn_default()` is untouched and
  still resolves the repository worker with program `bun`. `spawn_default_with`
  converts a `Spawn` whose kind is `NotFound` into `BunMissing`, so a caller that
  hands over a path still gets the typed failure; the composition root resolves
  first and builds `BunMissing` with the full candidate list.
- **Settings** — `bun_executable: String` (empty default, serde default, skipped
  when empty), trimmed by `normalize`, optional in `validate`, shown in `Debug`.
- **Recovery** — `SpeechTtApp::new` resolves and spawns after the settings load,
  records a typed `BackendFailure`, and opens Settings once when the runtime or
  the key is missing. `save_settings` builds the change with
  `SessionController::settings_change` (which leaves the dialog open), persists
  it, and respawns while `SessionController::backend_unavailable()` holds — that
  is, only when no worker was spawned, or the one that was has died with its
  sink still attached. A healthy backend is never replaced; a failed
  save/respawn returns `false`, so `DialogWindow` keeps the window and the
  draft.
- **Spawn seam** — `SpeechTtApp` holds the spawn as
  `Box<dyn Fn(&Settings) -> Result<Box<dyn SttSessionSink>, BackendFailure>>`.
  Production installs `spawn_backend` wrapped into a sink; the headless
  fixtures replace it through `SpeechTtApp::set_backend_spawn`, so both branches
  of the save → respawn rule are exercised without a `bun` installation and
  without starting a process. `attach_backend` now takes `Box<dyn SttSessionSink>`
  and attaches through the existing `connect_backend`.
- **Dialog** — a seventh `SettingRow` with an `Input` for `Bun path`, the
  controller-owned provider placeholder, an `InfoNote` naming the failure and
  the searched paths, and a three-state badge (`CONNECTED`, `NOT CONNECTED`,
  `NOT VERIFIED`).
- **Docs** — `DESIGN.md` Settings panel (seven rows, the row's resolution order,
  the badge and note states), `README.md` Download notes (desktop-launch `PATH`
  gap, the `Bun path` recovery, `~/.config/environment.d/` alternative).

### Design decisions

- **`BackendFailure` instead of a new `AppError` variant.** `AppError` is the
  status strip's vocabulary and stays unchanged; the typed reason lives in
  `BackendFailure` in `controller.rs`, which carries the paths the strip does
  not need and the dialog does. The strip keeps reporting `BackendUnavailable`.
  This is the "closest existing variant, extended" option in the controller
  scope.
- **The dialog must be able to save the fix.** `settings_draft_valid()` remains
  the catalogue-validated rule. Without a worker no catalogue can arrive, so
  Save stayed disabled and the dialog was still a dead end. `settings_can_save()`
  adds the recovery case: while the backend is unavailable and no catalogue has
  arrived, a provider and a key are enough, and `open_settings_from_stored`
  seeds the persisted provider/defaults without catalogue validation. A fresh
  install can therefore save `Bun path` plus a typed key; its model defaults
  stay empty until the catalogue arrives. The negative edges are pinned too: a
  catalogue that did arrive still validates the defaults, and a healthy backend
  means the catalogue is still coming — neither relaxes the rule.
- **Failure taxonomy.** `BackendFailure` distinguishes what the user can fix:
  `BunMissing` names the runtime candidates, `NotExecutable` names a configured
  path that exists without the execute bit, `WorkerMissing` names the worker
  locations beside the executable, and `Unavailable` is the catch-all with copy
  that makes no claim about `bun`. `BackendError::WorkerMissing` therefore has
  its own mapping arm instead of being folded into `Unavailable`.
- **Startup observability, single writer.** `diagnostics.rs` is the one module
  that writes `[speechtt]` lines, so the three startup markers live there beside
  `log_launched`: `log_settings_recovery` (unchanged format
  `[speechtt] startup settings=open reason=<…>`), `log_catalogue_loaded`
  (`[speechtt] startup catalogue=loaded providers=<n>`, the startup spawn only)
  and `log_catalogue_respawned` (`[speechtt] catalogue=loaded providers=<n>`,
  with no `startup=` claim, for a catalogue a save respawn produced). They carry
  no user data and are the only external evidence for the two transitions in
  T40.11.
- **Save on a successful recovery closes Settings**, which is the dialog's
  normal save semantics; reopening it shows the loaded catalogue and an enabled
  form. The contract only requires a failed respawn to keep the dialog open.

### Deviations from the contract

1. **`AppError::BunMissing` is not a new variant** (see above); the typed reason
   lives in `BackendFailure`.
2. **Three integration-test files gained one line each** —
   `crates/speechtt-ui/tests/error_ux.rs`, `dialogs.rs`, `end_to_end.rs`: they
   build `SettingsChange` with a full literal, so the contract-mandated field is
   a compile error there. Only `bun_executable: String::new(),` was added; no
   assertion changed. No other out-of-scope file was touched.
3. **`tickets/README.md` needed no edit**: T0040 was already listed under
   In Progress by the previous board update.
4. **No programmatic focus of the missing row.** The documented `Input` surface
   has `set_value`, not a documented focus entry point for a `SettingRow`, and
   adding row-focus would be a component change. "Focused on the missing action"
   is implemented as: the dialog opens by itself, the note names the cause, and
   the only editable row that can fix it is `Bun path` / the API key. This needs
   the CEO's visual review.
5. **`which_on_path` is an addition to `backend.rs`** beyond the two signatures
   in the contract. `resolve_bun_in` takes `which` as a parameter to stay
   hermetic; this function is the production `PATH` lookup the app passes in.
6. **`SETTINGS_DIALOG_HEIGHT` 640 → 700** so the seventh row does not start below
   the scroll fold. Same constant, no token or component change.
7. **`crates/speechtt-ui/src/diagnostics.rs` is a deliberate scope extension**
   (review finding Q4). The three startup markers began as `eprintln!` calls in
   `app.rs`, which contradicted the module's documented role as the single
   writer of `[speechtt]` lines. They now live in `diagnostics.rs` beside
   `log_launched`, with the same output format. A respawned catalogue uses the
   neutral `[speechtt] catalogue=loaded …` prefix rather than
   `startup catalogue=…`.
8. **Test-support files added by the review round**: one fixture
   (`open_speechtt_window_without_catalogue`) in
   `crates/speechtt-ui/tests/common/mod.rs`, and three integration tests in
   `crates/speechtt-ui/tests/dialogs.rs`. Both are test code only.
9. **Two public app setters** — `set_backend_spawn` (the spawn seam the review
   asked for) and `set_settings_bun_executable` (mirrors a typed `Bun path` into
   the controller and its input, the same pair the API-key setter mirrors).
   `set_backend_spawn` exists so the two respawn branches are testable without
   `bun`; it defaults to the production `spawn_backend`.
10. **`configured_path_problem` is a new pure helper** beside `spawn_backend`, so
    the "exists but is not executable" rule (Q6) is testable with injected
    lookups and no filesystem.

### Settings literal sites updated (the §2 trap)

- `crates/speechtt-ui/src/app.rs` — the rebuild lives in the new pure
  `settings_after_change`, covered by
  `a_committed_change_keeps_the_bun_executable_and_the_untouched_fields`.
- `crates/speechtt-core/src/settings.rs` — `sample_settings()` fixture.
- `crates/speechtt-ui/src/controller.rs` — one `SettingsChange` expectation and
  the new `SettingsDraft` initializer.
- The three forced test literals listed under deviations.

### Red → green evidence

- **T40.1/T40.2** — `cargo test -p speechtt-core --test bun_resolution` first
  failed with `unresolved imports speechtt_core::backend::bun_candidates,
resolve_bun_in` and `no variant named BunMissing` (plus one test-helper
  `FnMut` bug in my own fixture, fixed before implementing). After the
  implementation: `11 passed; 0 failed`.
- **T40.3/T40.4** — `cargo test -p speechtt-core --lib settings::tests` failed
  with `no field named bun_executable` at every new assertion. After the field,
  `normalize` and `Default`: all settings tests pass (the suite reports 51 at
  that point).
- **T40.5–T40.7** — `cargo test -p speechtt-ui --lib` failed with
  `no method named check_startup_settings`, `cannot find type BackendFailure`,
  and the missing `SettingsDraft`/`SettingsChange` fields. After the controller
  work, the 13 new controller tests pass (saving carries the path, the change keeps the
  dialog open, reopening seeds the stored path, the badge is `Unknown` without a
  catalogue, the note/placeholder state, the four startup cases and the one-shot
  dismissal guard). **Correction (peer review, B1):** T40.5's three respawn
  tests were not written in that round, so the save → respawn branch was
  uncovered. They exist now and are listed below.
- **T40.8** — `cargo check -p speechtt-ui --all-targets` then reported exactly
  the three out-of-scope `SettingsChange` literals; adding the field made the
  workspace compile again.
- **T40.10** — all gates green (below).

### Peer-review round: findings and fixes

Each finding from the review, with what changed. Every fix is covered by a
deterministic, hermetic test; none needs a display server, a filesystem, a live
`bun`, or the user's configuration.

- **B1 — the save → respawn path had no test, and the notes claimed it did.**
  Fixed with a spawn seam: `SpeechTtApp` holds
  `backend_spawn: BackendSpawn` (a closure from `&Settings` to
  `Result<Box<dyn SttSessionSink>, BackendFailure>`), production initialises it
  to `spawn_backend`, and `SpeechTtApp::set_backend_spawn` replaces it in a
  fixture. `attach_backend` now takes a sink and goes through
  `connect_backend`. The false coverage sentence is gone. New integration tests
  in `crates/speechtt-ui/tests/dialogs.rs`, using the new
  `open_speechtt_window_without_catalogue` fixture:
    - `saving_a_bun_path_respawns_and_attaches_the_worker` — a dead backend plus a
      path correction spawns exactly once, attaches the returned sink (it receives
      `Capabilities`), clears the error and closes Settings.
    - `saving_with_a_healthy_backend_does_not_spawn_a_worker` — the injected spawn
      is never called.
    - `a_failed_respawn_keeps_settings_open_with_the_error_and_the_draft` —
      `save_settings` returns `false`, `active_dialog` stays `Settings`, the error
      stays, the correction stays in the draft, and a second corrected save
      succeeds.
- **Q1 — negative edges of `settings_can_save`.** New unit tests in
  `controller.rs`: `a_loaded_catalogue_does_not_relax_the_save_rule_when_the_backend_is_down`
  and `a_healthy_backend_without_a_catalogue_does_not_relax_the_save_rule`.
- **Q2 — docs described a rule the code no longer has.** `settings_draft_valid`'s
  doc now states the catalogue-validated draft rule and names
  `settings_can_save` as the gate. `save_settings` and `respawn_backend` now say
  a _healthy_ backend is never replaced while a dead one (still holding its
  sink) is.
- **Q4 — diagnostics ownership.** The three `eprintln!` helpers moved to
  `crates/speechtt-ui/src/diagnostics.rs` beside `log_launched`:
  `log_settings_recovery`, `log_catalogue_loaded` and `log_catalogue_respawned`.
  The formats are unchanged except that a respawn catalogue no longer claims
  `startup=`. `diagnostics.rs` is a recorded, deliberate scope extension (see
  Deviations 7). `app.rs` calls them; the pump picks the marker from
  `catalogue_from_startup`, which `attach_backend` sets per spawn origin.
- **Q5 — `bun` was blamed for every failure.** `BackendFailure` gained
  `WorkerMissing { searched }` and `NotExecutable { path }`;
  `from_backend_error` maps `BackendError::WorkerMissing` to its own arm. The
  worker note names the searched worker paths, the not-executable note names the
  configured path, and the generic `Unavailable` copy no longer mentions `bun`.
  Tests: `a_missing_worker_names_its_searched_paths_instead_of_bun`,
  `a_configured_path_without_the_execute_bit_is_named`,
  `the_generic_backend_note_does_not_blame_bun`,
  `the_backend_error_mapping_keeps_the_paths_it_can_name`.
- **Q6 — the `Bun path` row is the headline affordance.** `configured_bun_path`
  expands a leading `~` (and `~` alone) against `$HOME` before the path is
  tested, and falls back to automatic resolution when there is no home to expand
  against. `configured_path_problem` returns `NotExecutable` when the configured
  path exists without the execute bit instead of silently falling through to
  `PATH`; a path that does not exist still falls through, and
  `which_on_path`/`exists` semantics are unchanged. Tests:
  `a_configured_path_expands_a_leading_tilde`,
  `a_configured_path_without_a_tilde_is_used_as_written`,
  `a_configured_path_is_reported_when_it_exists_but_is_not_executable`.
- **Q3 — not raised as a separate finding in this round.** Nothing beyond Q2's
  doc corrections was required; the `Bun path` row and its note already state
  the real rule after Q5/Q6.

### T40.10 gate output (final run, after the peer-review round)

```text
$ cargo fmt -- --check
fmt ok

$ cargo clippy --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.39s

$ cargo test
speechtt-core lib            237 passed
speechtt-core bridge_smoke     5 passed
speechtt-core bun_resolution  11 passed
speechtt-core session_machine 14 passed
speechtt-core transcript_captures 5 passed
speechtt-core worker_resolution 11 passed
speechtt-ui  lib             152 passed
speechtt-ui  main              0 passed
speechtt-ui  design_system     3 passed
speechtt-ui  dialogs          16 passed
speechtt-ui  end_to_end       37 passed
speechtt-ui  error_ux         14 passed
speechtt-ui  launch            5 passed
speechtt-ui  session_flow     96 passed
speechtt-ui  shortcuts        23 passed
speechtt-ui  startup           5 passed
speechtt-ui  theme_contract    4 passed
speechtt-ui  widget           35 passed
… every suite 0 failed; doc-tests 0

$ bun test
 466 pass
 0 fail
 1554 expect() calls
Ran 466 tests across 18 files. [534.00ms]

$ bun check src/stt
✓ No type errors in 32 files [62.44ms]

$ bun run design:validate
✓ Design contract valid — 66 tokens, 0 errors.
```

Nothing is `#[ignore]`d or skipped. The startup and launch gates
(`tests/startup.rs`, `tests/launch.rs`) are in the `cargo test` run above.

### T40.11 reproduction

Artifact: `bun run package` →
`dist/SpeechTT-0.5.0-x86_64.AppImage` (sha256 companion written).

Throwaway state:

```sh
rm -rf /tmp/opencode/t40 && mkdir -p /tmp/opencode/t40/home/.config/speechtt
# settings.json: provider vercel-ai-gateway, both defaults, a stored key, no bun_executable
```

Run with the desktop-like `PATH` (no `~/.bun/bin`), no display:

```sh
setsid env -i \
  HOME=/tmp/opencode/t40/home \
  XDG_CONFIG_HOME=/tmp/opencode/t40/home/.config \
  PATH=/usr/local/bin:/usr/bin:/bin:/usr/local/sbin \
  APPIMAGE_EXTRACT_AND_RUN=1 \
  ./dist/SpeechTT-0.5.0-x86_64.AppImage --appimage-extract-and-run
```

Observed (scenario 1, original failure reproduced):

```text
[speechtt] context=controller kind=BackendUnavailable code=- detail=""
[speechtt] startup settings=open reason=bun-missing
[speechtt] startup status=window-open

$ ps -eo pid,ppid,args | grep -E "SpeechTT-0.5.0|appimage_extracted_.*speechtt-ui|bun run .*worker.js"
210047  210045 ./dist/SpeechTT-0.5.0-x86_64.AppImage --appimage-extract-and-run
210049  210047 /tmp/appimage_extracted_73bb4b74…/usr/bin/speechtt-ui
```

No `bun` child, the failure is logged, and the self-check opened Settings and
named the missing runtime. `bun_executable` was then set to
`/home/zipang/.bun/bin/bun` in the throwaway file and the same command re-run:

```text
[speechtt] startup status=window-open
[speechtt] startup catalogue=loaded providers=1

210106  210045 ./dist/SpeechTT-0.5.0-x86_64.AppImage --appimage-extract-and-run
210108  210106 /tmp/appimage_extracted_73bb4b74…/usr/bin/speechtt-ui
210134  210108 /home/zipang/.bun/bin/bun run /tmp/appimage_extracted_73bb4b74…/usr/bin/../share/speechtt/worker.js
```

The worker spawned with the resolved runtime, the first catalogue arrived, and
no `settings=open` line was written: a healthy, keyed launch opens no dialog.

**How the UI save path is covered:** no display server is available here, so the
dialog's Save button could not be clicked in the packaged artifact. The
save → respawn wiring itself is now pinned by three headless integration tests
in `tests/dialogs.rs` (a dead backend respawns once and attaches the sink; a
healthy one is not replaced; a failed respawn keeps the dialog, the error and
the draft) — see the peer-review round above. The file-based proof is what the
artifact shows for the startup half of the flow. The CEO should still click it
once on a real desktop to confirm the visual flow.

### What the CEO should look at manually

- The Settings dialog at the new 700px height: seven rows, the `Bun path`
  `Input`, and the `NOT VERIFIED` badge before a catalogue arrives.
- The failure note's length with five searched paths (long but bounded).
- A real desktop launch from a menu with `~/.bun/bin` absent, then the
  fill-and-save recovery through the UI.
