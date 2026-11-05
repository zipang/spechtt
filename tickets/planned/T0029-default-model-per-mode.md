# T0029 — One default model per transcription mode

- **Track**: Rust core (settings) + Rust UI (controller + views + components) / design system
- **Wave**: 1 (independent)
- **Depends on**: T0028 (the dropdown menus these defaults feed)
- **Blocks**: nothing
- **References**:
  - `AGENTS.md` §2 (component/view rules), §5 (gates), §6 (official documentation)
  - `DESIGN.md` § "Session setup form" and § "Settings panel"
  - gpui-kit `Select` guide: <https://gpui-kit.com/component/select.md>
  - `SelectState` API: <https://docs.rs/gpui-component/latest/gpui_component/select/struct.SelectState.html>
  - gpui-kit llms index: <https://gpui-kit.com/llms.txt>

## Context & Goal

Settings currently stores a single default model. That cannot serve both take
modes at once: a live take needs a `Streaming` model and a file take needs a
`Segmented` (batch) one, so the saved default is wrong half the time and the
setup form opens without a usable selection.

This ticket splits the single default into **two** defaults, one per mode:

- `model_streaming` — the default for live takes.
- `model_batch` — the default for file takes.

Done means: Settings shows and edits both defaults, both are required to save,
and the Session setup form opens **already seeded** with the default that
matches the chosen mode (streaming for Live, batch for File). An existing
settings file with a single `model` is migrated into the slot its catalogue mode
belongs to, on the first catalogue that resolves it, and the next save writes
only the two mode-specific keys.

## Scope Boundary

Allowed to create or modify:

- `crates/speechtt-core/src/settings.rs`
- `crates/speechtt-ui/src/controller.rs`
- `crates/speechtt-ui/src/app.rs`
- `crates/speechtt-ui/src/components/model_select.rs`
- `crates/speechtt-ui/src/views/dialogs/settings.rs`
- `crates/speechtt-ui/src/views/dialogs/window.rs`
- `crates/speechtt-ui/tests/common/mod.rs`
- `crates/speechtt-ui/tests/{widget,dialogs,end_to_end,session_flow}.rs`
- `DESIGN.md` (§ "Settings panel", and the default-seeding sentence in
  § "Session setup form")
- `tickets/README.md` and this ticket

Out of scope — do **not** touch:

- `src/stt/**` and `crates/speechtt-core/src/protocol.rs`: the worker and the
  wire protocol are unchanged; `start` and `transcribeFile` keep one `model`.
- `crates/speechtt-ui/src/views/session_setup.rs`: the form already renders the
  draft; only the seeding behind it changes.
- `crates/speechtt-ui/src/components/setting_row.rs`, `tokens.rs`,
  `design-tokens.css`, `tests/design_system.rs`.
- `crates/speechtt-ui/src/views/dialogs/help.rs`.

## Contract

### 1. Core settings: two defaults plus a migration-only legacy field

```rust
pub struct Settings {
    pub provider: String,
    /// Default model for live takes; empty until chosen.
    pub model_streaming: String,
    /// Default model for file takes; empty until chosen.
    pub model_batch: String,
    /// Single default from older settings files. Read for migration only: it is
    /// never written once `model_streaming`/`model_batch` exist, and
    /// `skip_serializing_if` drops the key on the next save.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub model: String,
    pub api_keys: BTreeMap<String, SecretString>,
    pub shortcuts: BTreeMap<String, String>,
    pub auto_paste: bool,
    pub window: Option<WindowLayout>,
}
```

- JSON keys stay snake_case (`model_streaming`, `model_batch`, `model`), matching
  the rest of the struct (`#[serde(default, deny_unknown_fields)]`, no
  `rename_all`).
- `Settings::default()` sets `model_streaming`, `model_batch` and `model` all
  empty.
- `Debug` reports `model_streaming`, `model_batch` and a `legacy_model_present`
  boolean rather than the legacy string, so no model id of a removed file leaks
  into a debug line. API keys stay `[REDACTED]`.
- `validate()` requires a non-empty `provider` **and** both
  `model_streaming` and `model_batch` non-empty; the error message names the
  missing field (`"model_streaming must not be empty"` /
  `"model_batch must not be empty"`). Unknown provider ids stay allowed.

Loading a file that only has `model` must succeed (no `deny_unknown_fields`
error) and leave the two new fields empty; that is the migration input.

### 2. Controller: draft, defaults, seeding and migration

`SettingsDraft` and `SettingsChange` each carry both models:

```rust
pub struct SettingsDraft {
    pub provider: Option<String>,
    pub model_streaming: Option<String>,
    pub model_batch: Option<String>,
    pub api_key: String,
    pub api_key_unchanged: bool,
}

pub struct SettingsChange {
    pub provider: String,
    pub model_streaming: String,
    pub model_batch: String,
    pub api_key: Option<String>,
}
```

Controller API changes (old single-model names removed):

```rust
/// Default model the given take mode should preselect, if the catalogue knows
/// the stored default and it has the right mode.
pub fn default_model_for(&self, mode: SessionMode) -> Option<&str>;

pub fn set_settings_streaming_model(&mut self, model: &str) -> bool;
pub fn set_settings_batch_model(&mut self, model: &str) -> bool;
pub fn settings_streaming_model_mode(&self) -> Option<TranscriptionMode>;
pub fn settings_batch_model_mode(&self) -> Option<TranscriptionMode>;
pub fn settings_streaming_model_options(&self) -> Vec<SelectOption>;  // == provider models, streaming only
pub fn settings_batch_model_options(&self) -> Vec<SelectOption>;      // == provider models, segmented only
```

Rules:

- `set_settings_streaming_model` accepts a model of the settings provider whose
  catalogue mode is `Streaming`; `set_settings_batch_model` accepts one whose
  mode is `Segmented`. Both return `false` for an unknown id, an unknown
  provider or the wrong mode.
- `set_settings_provider` clearing the draft clears **both** model slots (a
  model belongs to its provider).
- `settings_draft_valid()` = provider known && both a streaming and a batch
  model chosen && a usable key. The streaming/batch selections are independent:
  neither implies the other.
- `settings_provider_label` / `settings_model_label` are replaced by
  `settings_streaming_model_label` / `settings_batch_model_label` (same
  catalogue-label lookup); any remaining caller is updated.
- `open_settings_with(provider, streaming, batch, api_key_unchanged)` seeds
  both slots; the current `open_settings` opens with neither.
- **Seeding**: `seed_draft_from_settings` sets the provider, then the model from
  `default_model_for(self.mode)`. It no longer refuses the file mode default: a
  batch default is `Segmented` by construction, so it is valid for a file take.
- **Migration**: `migrate_legacy_default(&mut self)` runs from `apply_settings`
  and `apply_capabilities` (the catalogue is what reveals a legacy model's
  mode). It applies when the stored `model` is non-empty, both new slots are
  empty, and the catalogue knows the id for the stored provider: the id is
  written into the slot matching its catalogue mode, on `self.settings` and, if
  the form is showing, into the draft via the usual seeding. It is idempotent
  and a no-op once both slots exist or the catalogue does not know the id.
- `apply_settings_change` writes `model_streaming`/`model_batch` and leaves the
  legacy `model` empty, so a save drops the old key.

### 3. Component: a three-state field set for Settings

`components/model_select.rs` keeps `SelectFields` (provider + one model) for the
Session setup form and adds a Settings-specific trio:

```rust
/// Provider + streaming default + batch default, one `SelectState` each.
pub struct SettingsSelectFields { /* provider, streaming, batch, applied */ }

/// The choices and selections the Settings dialog should show.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SettingsSnapshot {
    pub providers: Vec<SelectOption>,
    pub selected_provider: Option<String>,
    pub streaming_models: Vec<SelectOption>,
    pub selected_streaming_model: Option<String>,
    pub batch_models: Vec<SelectOption>,
    pub selected_batch_model: Option<String>,
}

impl SettingsSelectFields {
    pub fn new(window: &mut Window, cx: &mut App) -> Self;
    pub fn provider(&self) -> &Entity<SelectState<SearchableVec<ProviderOption>>>;
    pub fn streaming(&self) -> &Entity<SelectState<SearchableVec<ModelOption>>>;
    pub fn batch(&self) -> &Entity<SelectState<SearchableVec<ModelOption>>>;
    /// Same rebuild rule as [`SelectFields::sync`]: only when the snapshot
    /// changed, and a selection no longer offered is cleared.
    pub fn sync(&mut self, snapshot: SettingsSnapshot, window: &mut Window, cx: &mut App) -> bool;
}
```

Both model states are searchable, like the setup form's model menu. The
provider state is not.

### 4. Settings dialog: three rows

- **Transcription Service** — `ProviderSelect` (id `field-settings-provider`).
- **Default Streaming Model** — `ModelSelect` (id `field-settings-streaming`)
  listing only `Streaming` models, with the `TranscriptionModeBadge` beside it.
- **Default Batch Model** — `ModelSelect` (id `field-settings-batch`) listing
  only `Segmented` models, with the badge beside it.
- Descriptions: "Used for live dictation" and "Used for audio and video files".
- Provider disabled until the catalogue arrives; each model select disabled
  until a provider is chosen. Save stays disabled until the draft is valid.
- `DialogWindow` owns `SettingsSelectFields`, subscribes each state to its
  setter (`set_settings_provider`, `set_settings_streaming_model`,
  `set_settings_batch_model`), and still observes `SpeechTtApp` so an arriving
  catalogue refreshes the menus.

### 5. Behaviour the tests must pin

- Settings JSON round-trips both keys, and loading a legacy single-`model` file
  yields empty new fields.
- Saving writes `model_streaming` and `model_batch` and no `model` key.
- `validate()` requires both defaults and names the missing one.
- Setting a streaming id in the batch slot (and vice versa) is rejected.
- Changing the settings provider clears both default slots.
- The legacy id migrates into the slot matching its catalogue mode, once, and
  only when the catalogue knows it.
- Seeding picks the streaming default for Live and the batch default for File;
  the form opens complete (Continue enabled) for both modes when both defaults
  and a key are stored.
- Headless: the Settings dialog's three menus commit provider, streaming and
  batch into the draft; the setup form's model menu is prefilled per mode.

## TDD Tasks

- [ ] **T29.1 — Red: core settings.** Failing tests in `settings.rs` for the
  two fields, the legacy key load, `validate()` naming each missing default,
  and the JSON shape (both keys written, legacy key dropped). Then implement.
- [ ] **T29.2 — Red: controller defaults and validation.** Failing tests for
  `default_model_for` (live/file), `set_settings_streaming_model` /
  `set_settings_batch_model` (wrong mode rejected, provider change clears both),
  `settings_draft_valid` requiring both, and `SettingsChange` carrying both.
  Then implement.
- [ ] **T29.3 — Red: migration.** Failing tests: legacy `model` equal to a
  streaming id lands in `model_streaming`; equal to a segmented id lands in
  `model_batch`; unknown id or absent catalogue leaves both empty; running it
  twice changes nothing; it never overwrites an already-chosen slot. Then
  implement `migrate_legacy_default`.
- [ ] **T29.4 — Red: seeding.** Failing tests: `set_pending_file` opens the form
  with the batch default already selected; `choose_mode(Live)` opens it with the
  streaming default; a stored default the catalogue does not know leaves the
  slot empty. Then implement in `seed_draft_from_settings`.
- [ ] **T29.5 — Red: component.** Failing tests for `SettingsSnapshot`
  equality and the "selection no longer offered is cleared" rule (the pure part
  is testable without a window). Then implement
  `SettingsSelectFields` + `sync`.
- [ ] **T29.6 — Green: wire the dialog.** Extend `DialogWindow`, its
  subscriptions and `views/dialogs/settings.rs` to three rows, update
  `apply_settings_change` and the `set_settings_*` app wrappers, and make the
  headless widget test drive all three menus.
- [ ] **T29.7 — Fixtures and docs.** Update `tests/common/mod.rs` and every test
  that builds a `Settings` to the two fields; refresh `DESIGN.md` § "Settings
  panel" and the seeding sentence of § "Session setup form"; update the module
  and item docs (`AGENTS.md` §3: present tense, callers named, no ticket IDs).
- [ ] **T29.8 — Gates.** `cargo fmt -- --check`,
  `cargo clippy --all-targets -- -D warnings`, `cargo test`, `bun test`,
  `bun check src/stt`, `bun run design:validate`. Record exact counts in
  `## Implementation Notes`.

## Acceptance Criteria

- Settings shows **Transcription Service**, **Default Streaming Model** and
  **Default Batch Model** as three `SettingRow`s, each a real dropdown; the
  streaming menu lists only Stream models and the batch menu only Batch models,
  each row carrying its mode badge.
- Saving Settings is impossible until a provider, a streaming default, a batch
  default and a usable key are all present.
- A settings file written before this change keeps working: its single `model`
  becomes the default of its own mode, and the next save writes the two new
  keys without the old one.
- The Session setup form opens with the default model of the chosen mode already
  selected (Live → streaming default, File → batch default), so Continue is
  enabled without touching the fields when a key is stored.
- Switching provider in either dialog empties the dependent model selections.
- No view contains styling literals; the three menus use the existing
  components and badges.
- All six gates are green with zero warnings and no skipped tests.
- Final visual review is left to the user.
