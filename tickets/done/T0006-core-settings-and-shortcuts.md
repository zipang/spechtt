# T0006 — Core: Settings Store and Global Hotkeys

- **Track**: Rust core (`crates/speechtt-core`)
- **Wave**: 1 (independent)
- **Depends on**: nothing; reuses `protocol::SecretString` (T0002)
- **Blocks**: T0009 (end-to-end wiring)
- **PRODUCT.md**: Phases 4 and 7 (settings-store part)
- **Owner**: coding sub-agent (deepseek-v4.1-flash), supervised by CTO
- **Started**: 2026-10-27

## Context & Goal

Provider/model choice, API keys and shortcut bindings must persist across
launches, and global shortcuts must map to session actions without the UI
knowing about OS APIs. This ticket adds two pure, injected-dependency modules
to `speechtt-core`:

1. `settings.rs` — typed settings model, JSON persistence with safe file
   permissions, masking and round-trip guarantees.
2. `hotkeys.rs` — canonical shortcut parsing/formatting, default bindings,
   conflict detection, and a trait-based listener that maps OS key events to
   `SessionAction`s.

No test may touch the real home directory, XDG dirs, or a real display server.

## Scope Boundary

Only touch:

```text
crates/speechtt-core/Cargo.toml     # global-hotkey (append-only)
crates/speechtt-core/src/lib.rs     # `pub mod settings; pub mod hotkeys;` (append-only)
crates/speechtt-core/src/settings.rs # new
crates/speechtt-core/src/hotkeys.rs  # new
```

Do **not** touch `crates/speechtt-ui/` or `src/stt/`.

> `Cargo.toml` / `lib.rs` are shared with T0005/T0007; append only, never
> rewrite existing lines. `protocol::SecretString` is read-only here.

## 1. Settings store (`settings.rs`)

### Model

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub provider: String,
    pub model: String,
    /// Provider id -> key. Serialized, but never exposed by `Debug`.
    pub api_keys: BTreeMap<String, SecretString>,
    pub shortcuts: BTreeMap<String, String>, // action name -> "Ctrl+Shift+R"
    pub auto_paste: bool,
}

impl Default for Settings { /* provider = "vercel", model = "", no keys */ }
```

- `Settings::default()` must be non-panicking and usable before first run.
- API keys are addressed by provider id: `api_key(&self, provider) ->
  Option<&str>`, `set_api_key(&mut self, provider, key)`,
  `remove_api_key(&mut self, provider)`.
- `masked_api_key(&self, provider) -> Option<String>` renders
  `"sk-…"`-style masks (first 3 + `…` + last 2 for keys ≥ 8 chars, otherwise
  a fixed `"••••"`). Empty keys are treated as absent.
- `Debug for Settings` prints `api_keys: "[REDACTED]"` or per-key
  `"[REDACTED]"` — never key material.
- `validate(&self) -> Result<(), SettingsError>`: non-empty provider/model,
  known `auto_paste`-independent constraints are not required here; unknown
  provider ids are allowed at the core layer (the Bun registry is the source
  of truth).

### Persistence

```rust
pub trait SettingsStore: Send {
    fn load(&self) -> Result<Settings, SettingsError>;
    fn save(&self, settings: &Settings) -> Result<(), SettingsError>;
    fn path(&self) -> &Path;
}

pub struct JsonFileStore { /* path injected */ }

impl JsonFileStore {
    pub fn new(path: impl Into<PathBuf>) -> Self;
    /// `$XDG_CONFIG_HOME/speechtt/settings.json` or
    /// `$HOME/.config/speechtt/settings.json`; falls back to the OS temp
    /// directory only when neither variable is set.
    pub fn default_path() -> PathBuf;
}
```

- `load()` on a missing file returns `Settings::default()` (not an error).
- Corrupt JSON returns `SettingsError::Corrupt` with the path, never a panic
  and never a partial `Settings`.
- `save()` writes atomically: create parent dirs → write
  `<file>.tmp` → `sync_all` → rename over the target. On Unix set mode
  `0o600` before rename. A failed write must leave the previous file intact.
- File format is pretty-printed JSON with a stable key order
  (`BTreeMap` + serde field order) so diffs are reviewable.

## 2. Hotkeys (`hotkeys.rs`)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SessionAction {
    StartRecording,
    PauseRecording,
    ResumeRecording,
    StopRecording,
    NewTake,
    ToggleHelp,
    ToggleSettings,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Shortcut {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub meta: bool,
    pub key: String, // canonical, upper-case single char or "F1".."F12", "Space", ...
}
```

- `Shortcut::parse(&str)` accepts `Ctrl+Shift+R`, `ctrl + alt + space`,
  `Super+/`, `F9`; returns `Err(ShortcutError::...)` with a precise reason
  for empty input, unknown modifier, missing key, or duplicate modifiers.
- `Display for Shortcut` emits the canonical `Ctrl+Alt+Shift+Super+R` order.
- `default_bindings() -> BTreeMap<SessionAction, Shortcut>`:
  `StartRecording = Ctrl+Shift+R`, `PauseRecording = Ctrl+Shift+P`,
  `ResumeRecording = Ctrl+Shift+P` is **not** allowed — use
  `ResumeRecording = Ctrl+Shift+O` to avoid an intentional conflict, or model
  Pause/Resume as one `TogglePause` action mapped to `Ctrl+Shift+Space`.
  Document the chosen mapping in the module docs.
- `resolve(bindings, &Shortcut) -> Option<SessionAction>` — exact modifier
  match, case-insensitive key comparison.
- `conflicts(bindings) -> Vec<(SessionAction, SessionAction, Shortcut)>` —
  detects two actions bound to the same shortcut.
- `bind(bindings, action, shortcut) -> Result<(), ShortcutError>` — refuses a
  duplicate binding unless it replaces the same action.
- `ShortcutListener` trait:

```rust
pub trait ShortcutListener: Send {
    fn register(&mut self, shortcuts: &[(SessionAction, Shortcut)]) -> Result<(), HotkeyError>;
    fn unregister_all(&mut self);
}
```

- `GlobalHotkeyListener` (crate `global-hotkey`) implements the trait; it is
  constructed only on the main thread and is **not** instantiated by tests.
- Parsing `Settings.shortcuts` is explicit: `bindings_from_settings(&Settings)
  -> Result<BTreeMap<SessionAction, Shortcut>, ShortcutError>`, so bad
  persisted strings surface as errors instead of silently disabling hotkeys.

## 3. TDD Tasks

- [x] **T6.1 — Settings model tests**: default is valid-ish and serializable;
  set/remove/mask keys; empty key == absent; round-trip through serde
  preserves all fields; `Debug` output contains no key material.
- [x] **T6.2 — Store tests** using `tempfile`-style unique temp dirs:
  - missing file → `Settings::default()`;
  - save → load round-trip equality;
  - corrupt file → `SettingsError::Corrupt` (original untouched);
  - atomicity: a save failure (target parent is a file / injected failing
    writer) leaves the previous JSON readable;
  - Unix-only: file mode is `0o600` (gate with `#[cfg(unix)]`).
- [x] **T6.3 — Shortcut parsing tests**: valid forms, canonical `Display`
  round-trip, each error variant, duplicate-modifier rejection.
- [x] **T6.4 — Binding tests**: defaults resolve; unknown shortcut →
  `None`; conflict detection; rebinding one action; duplicate binding
  rejected; `bindings_from_settings` round-trip.
- [x] **T6.5 — Listener contract test**: a `MockShortcutListener` records
  register/unregister calls; `unregister_all` on drop is asserted.
- [x] **T6.6 — Implementation** of `settings.rs` + `hotkeys.rs`, add the
  `global-hotkey` dependency, register modules.
- [x] **T6.7 — Gates**: `cargo fmt -- --check`,
  `cargo clippy --all-targets -- -D warnings`, `cargo test` clean; no test
  reads/writes the real user config directory.

## Acceptance Criteria

- Settings persist across process restarts and are written atomically with
  `0600` permissions on Unix.
- Secrets never appear in `Debug`, error messages, or logs.
- Every default shortcut resolves to the intended `SessionAction`, and
  conflicts are rejected at bind time.
- Tests are hermetic: injected temp paths, mock listeners, no display server.

## Implementation Notes

### `settings.rs`

- **Model.** `Settings` keeps the ticket's field set and semantics:
  `provider` defaults to `"vercel"`, `model` to `""`, both maps empty, and
  `auto_paste` to `true` (the product's primary dictation flow; the open
  question in `PRODUCT.md` §7 is about automatic paste *behaviour*, not the
  stored default). `#[serde(default, deny_unknown_fields)]` gives forward
  compatibility for missing fields while turning stale/typo'd keys into
  `SettingsError::Corrupt` instead of silently dropping configuration.
- **Secret handling.** `api_keys` uses `protocol::SecretString`. `Debug` is
  hand-written and prints `api_keys: "[REDACTED]"`; no key material is ever
  formatted. Setters/removers treat an empty value as "no key": `set_api_key`
  with `""` removes the entry, and `api_key` filters empty values for
  defensively written files. `masked_api_key` renders `sk-…56` (first 3,
  U+2026, last 2) for keys of at least 8 characters, `••••` otherwise, and
  `None` when absent. Character-based slicing avoids panics on non-ASCII
  input.
- **Validation.** `validate` requires non-empty (after `trim`) provider and
  model only; unknown provider ids pass because the Bun registry is the
  source of truth. `Settings::default()` is therefore intentionally *not*
  valid until a model is chosen; the test documents this.
- **Persistence.** `JsonFileStore::load` maps `NotFound` to
  `Settings::default()`, deserializes the whole file before returning
  anything (no partial `Settings`), and reports malformed/unknown-field JSON
  as `SettingsError::Corrupt { path, source }`.
- **Atomic save.** `save` serializes pretty JSON (struct field order, plus a
  trailing newline), creates parent directories, creates
  `<file>.tmp` with `OpenOptions::mode(0o600)` and re-applies `0o600` (for a
  stale temp file), writes, `sync_all`s, then renames over the target. Every
  failure path removes the temp file, so the previous target is never
  replaced by partial output. The tests inject failures with a directory at
  `settings.json.tmp` and with a regular file as the target's parent.
- **Default path.** `default_path` is a thin wrapper over the pure
  `resolve_default_path(xdg, home, temp)` helper: non-empty `XDG_CONFIG_HOME`
  → `$XDG/speechtt/settings.json`, else non-empty `HOME` →
  `$HOME/.config/speechtt/settings.json`, else `<temp>/speechtt/settings.json`.
  Tests exercise the helper with explicit values, so no test mutates process
  environment variables.

### `hotkeys.rs`

- **Actions and notation.** `SessionAction` keeps camelCase serde names and
  gains `PartialOrd, Ord` (required for `BTreeMap` keys), `Display` and
  `FromStr` using the same camelCase names, plus `ALL`. `Shortcut::parse`
  treats the last `+`-separated token as the key and every preceding token as
  a modifier, which makes errors precise: `Empty`, `UnknownModifier`,
  `DuplicateModifier`, `MissingKey` (including a trailing modifier such as
  `Ctrl+Shift`), `UnknownKey`. Keys are canonicalised to upper-case single
  characters, `F1`..`F12`, or named keys (`Space`, `Tab`, `Enter`, `Escape`,
  `Backspace`, `Delete`, `Insert`, `Home`, `End`, `PageUp`, `PageDown`, `Up`,
  `Down`, `Left`, `Right`); aliases (`ctrl`/`control`, `super`/`meta`/`cmd`,
  `esc`, `pgdn`, ...) are accepted. `Display` always emits
  `Ctrl+Alt+Shift+Super+Key` order and round-trips through `parse`.
- **Defaults.** Documented in the module docs:
  `StartRecording Ctrl+Shift+R`, `PauseRecording Ctrl+Shift+P`,
  `ResumeRecording Ctrl+Shift+O`, `StopRecording Ctrl+Shift+S`,
  `NewTake Ctrl+Shift+N`, `ToggleHelp Ctrl+Shift+H`,
  `ToggleSettings Ctrl+Shift+,`. Pause/Resume stay distinct actions;
  `Ctrl+Shift+O` avoids the collision the ticket warns about.
- **Binding operations.** `resolve` matches modifiers exactly and keys with
  `eq_ignore_ascii_case`; `conflicts` reports every pair in `BTreeMap` order;
  `bind` replaces the same action's binding but rejects another action's
  shortcut with `ShortcutError::Conflict` and leaves the map untouched.
  `bindings_from_settings` starts from `default_bindings()` (so an empty
  settings map yields the defaults), then parses action names and shortcut
  strings and applies them through `bind`; bad strings, unknown actions and
  conflicting overrides surface as errors instead of silently disabling
  hotkeys.
- **Listener seam.** `ShortcutListener` is exactly the ticket's trait.
  `ListenerGuard<L>` owns any listener and calls `unregister_all` on drop; the
  tests drive it with `MockShortcutListener` (shared `Arc<Mutex<...>>` state)
  and assert both forwarded registration and drop-time unregistration.
  `GlobalHotkeyListener` wraps `GlobalHotKeyManager`, maps each `Shortcut` to
  a `global_hotkey::HotKey` via `Code::from_str`, rolls back partial
  registrations on failure, exposes `event_receiver()` and `action_for_id()`,
  and also unregisters in its own `Drop` (idempotent with the guard).
  Tests never construct it: they only assert the trait bound at compile time
  and exercise the pure `to_hotkey`/`code_name` conversion, so no X11/display
  server is touched.

### Test coverage

- `settings::tests` — 22 tests: defaults/validation, key set/get/remove,
  empty-key handling, masking, serde round-trip, `Debug` redaction, unknown
  fields, missing/partial/corrupt files, parent creation, atomic-failure
  preservation (temp dir blocked, parent blocked), stable pretty JSON,
  `0o600` (Unix), and all default-path branches.
- `hotkeys::tests` — 29 tests: parse forms/aliases/canonical display and all
  error variants, default table completeness/conflict-freedom/canonicality,
  resolve semantics, rebinding, duplicate rejection, conflict detection,
  `bindings_from_settings` (empty/round-trip/override/bad action/bad
  shortcut/conflict), camelCase serde, listener forwarding, drop-time
  unregistration, idempotent explicit unregistration, trait-bound check for
  `GlobalHotkeyListener`, and pure shortcut → `HotKey` conversion.
- All filesystem tests use a per-test unique directory under
  `std::env::temp_dir()` (pid + nanos + counter) removed on drop; no test
  reads `$HOME`/`$XDG_CONFIG_HOME` or instantiates an OS listener.

### Gates

- `cargo fmt -- --check` — clean (after one `cargo fmt` pass).
- `cargo clippy --all-targets -- -D warnings` — `Finished dev profile
  [unoptimized + debuginfo] target(s) in 0.26s`, zero warnings.
- `cargo test` — 175 passed, 0 failed:
  `speechtt-core` lib 117, `bridge_smoke` 2, `session_machine` 11,
  `speechtt-ui` lib 14, `dialogs` 8, `session_flow` 17, `theme_contract` 4,
  `widget` 2, doc-tests 0. The 51 new tests are the 22 settings + 29 hotkeys
  modules; the remaining core tests are unchanged.
- `Cargo.lock` was updated automatically by Cargo (not hand-edited) to pin
  `global-hotkey 0.9` and its dependency tree; no other manifests changed.
