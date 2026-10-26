# T0004 — UI: Help / Shortcuts and Settings Dialogs

- **Track**: Rust UI (`crates/speechtt-ui`)
- **Wave**: 1 (independent)
- **Depends on**: T0001 (layout + components)
- **Blocks**: T0009 (end-to-end wiring)
- **Visual ground truth**: `mockups/dialogs/help.png`, `mockups/dialogs/settings.png`

## Context & Goal

The widget shell already renders a header with a `?` help button and a cog
settings button, but both are inert stubs (`help_button()` /
`settings_button()` in `crates/speechtt-ui/src/app.rs`). Phase 2 of
`PRODUCT.md` is therefore incomplete.

This ticket implements the two dialogs **as pure, testable view-model state
plus presentational views**, with no dependency on the future settings store
(T0006). Interaction is tested through controller commands — never through
element-tree assertions or pixel geometry.

## Scope Boundary

Only touch:

```text
crates/speechtt-ui/src/app.rs
crates/speechtt-ui/src/controller.rs            # add dialog state + commands
crates/speechtt-ui/src/components/mod.rs        # export KeyCap
crates/speechtt-ui/src/components/key_cap.rs    # new
crates/speechtt-ui/src/views/mod.rs             # register dialogs module
crates/speechtt-ui/src/views/dialogs/           # new (mod.rs, help.rs, settings.rs)
crates/speechtt-ui/tests/dialogs.rs             # new
```

Do **not** touch `crates/speechtt-core`, `Cargo.toml`, `src/stt/`, or
`PRODUCT.md`.

## 1. View-model contract (`controller.rs`)

Add to `SessionController` (or a sibling `DialogState` struct owned by it):

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialog {
    Help,
    Settings,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsDraft {
    pub provider: Option<String>,
    pub model: Option<String>,
    /// Raw user input; never logged, never rendered unmasked.
    pub api_key: String,
    /// `true` when a key was already stored and the field is untouched.
    pub api_key_unchanged: bool,
}
```

Commands (all return `bool`/`Option` so tests can assert effects):

- `open_help()` / `open_settings()` — set `active_dialog`.
- `close_dialog()` — clear it.
- `dismiss_dialog_with_escape()` — same as `close_dialog`, exists so the
  keybinding intent is explicit and testable.
- `settings_draft()` / `active_dialog()` readers.
- `set_settings_provider`, `set_settings_model`, `set_settings_api_key`,
  `clear_settings_api_key`.
- `settings_draft_valid() -> bool` — provider + model selected; an empty key
  is valid only when `api_key_unchanged` is true.
- `save_settings() -> Option<SettingsChange>` — returns the pending change
  and closes the dialog; `None` (and leaves the dialog open) when invalid.

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsChange {
    pub provider: String,
    pub model: String,
    /// `None` = keep the stored key; `Some("")` = clear it.
    pub api_key: Option<String>,
}
```

- Dialog state must **not** be persisted and must survive ordinary screen
  state transitions while open (opening Settings during Recording is allowed
  and must not mutate the session).
- `SettingsChange` must implement `Debug` with the key redacted.

Provider/model options come from a small UI-owned catalogue for now. Add
`pub fn provider_options() -> &'static [ProviderOption]` returning at least
`Vercel AI Gateway`, `Mistral AI`, `OpenAI`, `xAI`, `ElevenLabs`, so T0009
can later replace the catalogue with the Bun capability report.

## 2. Components

### `components/key_cap.rs`

```rust
pub struct KeyCap { keys: SmallVec<[SharedString; 3]> }
impl KeyCap { pub fn new(keys: impl IntoIterator<Item = impl Into<SharedString>>) -> Self; }
```

Renders one bordered rounded box per key with token colors/typography, joined
by `+`. Exported from `components/mod.rs`. Used only by the Help dialog.

### Dialog shell

`views/dialogs/mod.rs` exposes `render_dialog(app: &SpeechTtApp, cx) ->
Option<AnyElement>` that dispatches on `active_dialog()`. When no dialog is
active the main body renders unchanged.

- Dimmed backdrop over the 480×280 frame, centered surface panel using
  existing tokens, `Escape` binding wired through the app key context.
- Buttons carry stable `ElementId`s **only** for access keys/accessibility:
  `btn-help-close`, `btn-settings-cancel`, `btn-settings-save`,
  `field-settings-api-key`. Tests must not depend on them.

### Help dialog (`views/dialogs/help.rs`)

Per `mockups/dialogs/help.png`: title, keycap rows for the global shortcuts,
a short scope note ("Global shortcuts work while SpeechTT is running"), the
app version from `CARGO_PKG_VERSION`, and a **Got it** button. Shortcuts shown
must be read from one constant table (`HELP_SHORTCUTS`) so T0006 can later
drive it from user bindings.

### Settings dialog (`views/dialogs/settings.rs`)

Per `mockups/dialogs/settings.png`: provider select, model select (choices
derived from the selected provider), masked API key input
(`type = password` or equivalent), connection badge (`CONNECTED` / `NOT
CONNECTED`), local fallback note, **Cancel** and **Save**. Save is disabled
while `settings_draft_valid()` is false. The API key value must never be
echoed into logs or `Debug`.

## 3. App wiring (`app.rs`)

- Replace the `help_button()` / `settings_button()` stubs with buttons whose
  `on_click` calls `app.open_help()` / `app.open_settings()` and then
  `cx.notify()`.
- Render `views::dialogs::render_dialog(...)` as an overlay above the body.
- Register an `Escape` keybinding that calls `dismiss_dialog_with_escape()`.
- `save_settings()` result is forwarded to a single hook
  `apply_settings_change(SettingsChange)` that, for now, stores the pending
  change on the app and emits a diagnostic-free no-op. T0009 replaces the
  hook body with the real settings store.

## 4. TDD Tasks

- [ ] **T4.1 — Pure dialog state tests** (`controller.rs` unit tests):
  - open/close Help and Settings; only one dialog active at a time.
  - Settings draft starts invalid; selecting provider + model makes it valid
    only when a key is present or `api_key_unchanged`.
  - `save_settings()` returns the expected `SettingsChange`, closes the
    dialog, and preserves the key verbatim (`Some("sk-…")`).
  - `Debug` output of `SettingsChange` and `SettingsDraft` never contains the
    key value.
  - Opening Settings during Recording leaves `session_state()` unchanged.
- [ ] **T4.2 — Behaviour tests** (`tests/dialogs.rs`, controller-driven, no
  `window.try_find`):
  - `open_help` → `active_dialog() == Some(Dialog::Help)`;
    `dismiss_dialog_with_escape` → `None`.
  - `open_settings` → choose provider + model + key → `save_settings()`
    returns the change and closes.
  - Invalid draft (no provider) → `save_settings()` returns `None` and leaves
    `Some(Dialog::Settings)` active.
  - Cancel path discards the draft (reopening shows a fresh/default draft).
- [ ] **T4.3 — `KeyCap` + dialog render smoke**:
  - One `#[gpui_kit::test]` renders a frame with Help open and another with
    Settings open; asserts no panic and, optionally, that the window has a
    non-empty root. No placement assertions.
- [ ] **T4.4 — Implementation**: `Dialog`/`SettingsDraft`/commands,
  `KeyCap`, `views/dialogs/*`, overlay + Escape + header wiring.
- [ ] **T4.5 — Visual QA (manual)**: side-by-side against
  `mockups/dialogs/help.png` and `settings.png`; fix spacing/typography only.
- [ ] **T4.6 — Gates**: `cargo fmt -- --check`,
  `cargo clippy --all-targets -- -D warnings`, `cargo test` all clean.

## Acceptance Criteria

- `?` opens Help; `Got it` and `Escape` close it.
- Cog opens Settings; `Cancel` and `Escape` discard, `Save` emits exactly one
  `SettingsChange` when the draft is valid.
- No behavioral test asserts element placement, geometry, or labels.
- No API key appears in `Debug`, test output, or logs.
- Settings/Help open without disturbing an in-flight recording.
