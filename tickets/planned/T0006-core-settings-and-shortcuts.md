# T0006 — Core: Settings Store and Global Hotkeys

- **Track**: Rust core (`crates/speechtt-core`)
- **Wave**: 1 (independent)
- **Depends on**: nothing; reuses `protocol::SecretString` (T0002)
- **Blocks**: T0009 (end-to-end wiring)
- **PRODUCT.md**: Phases 4 and 7 (settings-store part)

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

- [ ] **T6.1 — Settings model tests**: default is valid-ish and serializable;
  set/remove/mask keys; empty key == absent; round-trip through serde
  preserves all fields; `Debug` output contains no key material.
- [ ] **T6.2 — Store tests** using `tempfile`-style unique temp dirs:
  - missing file → `Settings::default()`;
  - save → load round-trip equality;
  - corrupt file → `SettingsError::Corrupt` (original untouched);
  - atomicity: a save failure (target parent is a file / injected failing
    writer) leaves the previous JSON readable;
  - Unix-only: file mode is `0o600` (gate with `#[cfg(unix)]`).
- [ ] **T6.3 — Shortcut parsing tests**: valid forms, canonical `Display`
  round-trip, each error variant, duplicate-modifier rejection.
- [ ] **T6.4 — Binding tests**: defaults resolve; unknown shortcut →
  `None`; conflict detection; rebinding one action; duplicate binding
  rejected; `bindings_from_settings` round-trip.
- [ ] **T6.5 — Listener contract test**: a `MockShortcutListener` records
  register/unregister calls; `unregister_all` on drop is asserted.
- [ ] **T6.6 — Implementation** of `settings.rs` + `hotkeys.rs`, add the
  `global-hotkey` dependency, register modules.
- [ ] **T6.7 — Gates**: `cargo fmt -- --check`,
  `cargo clippy --all-targets -- -D warnings`, `cargo test` clean; no test
  reads/writes the real user config directory.

## Acceptance Criteria

- Settings persist across process restarts and are written atomically with
  `0600` permissions on Unix.
- Secrets never appear in `Debug`, error messages, or logs.
- Every default shortcut resolves to the intended `SessionAction`, and
  conflicts are rejected at bind time.
- Tests are hermetic: injected temp paths, mock listeners, no display server.
