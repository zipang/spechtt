# T0040 — Find `bun`, and never leave Settings as a dead end

- **Track**: Rust core (backend resolution + settings) + Rust UI (Settings dialog, startup) + docs
- **Wave**: 2 (integration — touches the production spawn path, the settings file format and the startup flow)
- **Depends on**: T0038 (the executable-relative worker resolver and the AppImage this ticket's failure was found in), T0016 (the dependency vocabulary the README already uses)
- **Blocks**: nothing
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

- [ ] **T40.1 — Resolution specs first**: write `crates/speechtt-core/tests/bun_resolution.rs`
      red. No `tempfile`, no filesystem, no `PATH`: every case injects its own
      `which` and `exists`. Cover: the configured path wins over everything,
      including over a `PATH` hit; an empty configured value falls through to
      `PATH`; `PATH` resolves before `$HOME/.bun/bin/bun`; the `$HOME` location
      resolves when `PATH` misses; `/usr/local/bin/bun` and `/opt/bun/bin/bun`
      resolve in that order; nothing resolving yields `None`; a `home` of `None`
      skips the `$HOME` candidate without panicking; a whitespace-only
      configured value is treated as empty; the candidate list is ordered and
      its first entry is the configured path.
- [ ] **T40.2 — Resolution implementation**: `bun_candidates`, `resolve_bun_in`,
      `BackendError::BunMissing`, `spawn_default_with`, per Contract §1.
      `spawn_default` keeps its current behaviour and its existing tests pass
      untouched.
- [ ] **T40.3 — Settings specs first**: red tests for the new field — default is
      empty; a file without the key loads; a file with it round-trips;
      `normalize` trims it to empty; `validate` accepts an empty value and a
      populated one; a serialized default omits the key.
- [ ] **T40.4 — Settings implementation**: the field, its `Default`, `normalize`
      and the doc comment per §2. Confirm no other `Settings` literal in the
      workspace fails to compile — that is the trap from §2 surfacing, and every
      site it names must be fixed, including `apply_settings_change`.
- [ ] **T40.5 — Save/respawn specs first**: red tests for §3 — saving with no
      backend attached and a resolvable `bun` spawns one; saving with a backend
      already attached does not replace it; a failed respawn keeps the error and
      the draft. `SettingsChange` must carry the field, and a test must prove
      the stored value survives a save.
- [ ] **T40.6 — Dialog specs first**: red tests for §5 and §6 —
      `settings_connected()` is false with no catalogue and true after one
      arrives with a key; the failure note is present exactly when the backend
      is unavailable and no catalogue has arrived; the provider placeholder is
      not the loading text in that state. These are controller-level assertions,
      never layout or position assertions (AGENTS.md §6).
- [ ] **T40.7 — Startup specs first**: red tests for §4 — the auto-open fires
      when `bun` is missing, fires when no key is stored, does not fire when
      both are satisfied, and does not fire twice after the user dismisses it.
- [ ] **T40.8 — Implementation**: §3–§7 — the respawn, the startup check, the
      dialog row and note, the badge condition, the `DESIGN.md` row list.
- [ ] **T40.9 — Docs**: `README.md` per §8, `tickets/README.md` board row.
- [ ] **T40.10 — Gates**: `cargo fmt -- --check`,
      `cargo clippy --all-targets -- -D warnings`, `cargo test`, `bun test`,
      `bun check src/stt`, `bun run design:validate`. Zero warnings; nothing
      skipped or `#[ignore]`d.
- [ ] **T40.11 — Real recovery**: reproduce the original failure and prove the
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

_(added during work: design decisions, deviations from the contract, the exact
gate output, and the T40.11 reproduction.)_
