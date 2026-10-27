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
crates/speechtt-ui/src/components/mod.rs        # export the new components
crates/speechtt-ui/src/components/app_header.rs # new (shared window header)
crates/speechtt-ui/src/components/action_button.rs # new (one-size text actions)
crates/speechtt-ui/src/components/icon_button.rs   # new (one-size chrome icons)
crates/speechtt-ui/src/components/key_cap.rs    # new
crates/speechtt-ui/src/components/setting_row.rs # add prominent dialog style
crates/speechtt-ui/src/views/mod.rs             # register dialogs module
crates/speechtt-ui/src/views/dialogs/           # new (mod.rs, help.rs, settings.rs, window.rs)
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

`views/dialogs/mod.rs` exposes `DialogShell` (title row, scrollable body,
footer) and the fixed client sizes. The dialog content is rendered by the
root view of a dedicated OS window, not as an overlay on the widget.

- Each dialog gets its own `WindowKind::Floating` window, sized 600×520
  (Help and Settings) and positioned beside the widget (right edge
  preferred, left when the display has no room, clamped to the visible
  display bounds).
- The window reuses the main window's Header + Body structure through the
  shared `components::AppHeader`: bold title on the left, close button on the
  right, `title_bar` background and bottom border. Titles are
  `SpeechTT - Settings` and `SpeechTT - Help & Shortcuts`.
- Buttons carry stable `ElementId`s **only** for access keys/accessibility:
  `btn-help-close`, `btn-settings-cancel`, `btn-settings-save`,
  `field-settings-api-key`. Tests must not depend on them.

### Help dialog (`views/dialogs/help.rs`)

Per `mockups/dialogs/help.png`: title, keycap rows for the global shortcuts,
the app version from `CARGO_PKG_VERSION`, and a **Got it** button. The scope
note from the mockup was dropped: it was truncated and restated behaviour the
UI does not enforce. Shortcuts shown must be read from one constant table
(`HELP_SHORTCUTS`) so T0006 can later drive it from user bindings.

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
- Open each dialog in its own floating window (`DialogWindow` root view),
  positioned beside the widget, and track the handle so a second open
  replaces the first and Escape/close tears it down.
- Close the dialog window when the controller no longer has it active, and
  clear the controller state when the OS closes the window.
- `save_settings()` result is forwarded to a single hook
  `apply_settings_change(SettingsChange)` that, for now, stores the pending
  change on the app and emits a diagnostic-free no-op. T0009 replaces the
  hook body with the real settings store.

## 4. TDD Tasks

- [x] **T4.1 — Pure dialog state tests** (`controller.rs` unit tests):
  - open/close Help and Settings; only one dialog active at a time.
  - Settings draft starts invalid; selecting provider + model makes it valid
    only when a key is present or `api_key_unchanged`.
  - `save_settings()` returns the expected `SettingsChange`, closes the
    dialog, and preserves the key verbatim (`Some("sk-…")`).
  - `Debug` output of `SettingsChange` and `SettingsDraft` never contains the
    key value.
  - Opening Settings during Recording leaves `session_state()` unchanged.
- [x] **T4.2 — Behaviour tests** (`tests/dialogs.rs`, controller-driven, no
  `window.try_find`):
  - `open_help` → `active_dialog() == Some(Dialog::Help)`;
    `dismiss_dialog_with_escape` → `None`.
  - `open_settings` → choose provider + model + key → `save_settings()`
    returns the change and closes.
  - Invalid draft (no provider) → `save_settings()` returns `None` and leaves
    `Some(Dialog::Settings)` active.
  - Cancel path discards the draft (reopening shows a fresh/default draft).
- [x] **T4.3 — `KeyCap` + dialog render smoke**:
  - One `#[gpui_kit::test]` renders a frame with Help open and another with
    Settings open; asserts no panic and, optionally, that the window has a
    non-empty root. No placement assertions.
  - Each dialog opens its own window: Help and Settings each add exactly one
    window beside the widget, opening one replaces the other, and Escape
    closes only the dialog window.
  - A simulated `escape` keystroke on the focused dialog window closes it and
    clears the controller state.
- [x] **T4.4 — Implementation**: `Dialog`/`SettingsDraft`/commands,
  `KeyCap`, `views/dialogs/*`, floating windows + header wiring.
- [x] **T4.5 — Visual QA (manual)**: side-by-side against
  `mockups/dialogs/help.png` and `settings.png`; fix spacing/typography only.
- [x] **T4.6 — Gates**: `cargo fmt -- --check`,
  `cargo clippy --all-targets -- -D warnings`, `cargo test` all clean.

## Acceptance Criteria

- `?` opens Help; `Got it` and `Escape` close it.
- Cog opens Settings; `Cancel` and `Escape` discard, `Save` emits exactly one
  `SettingsChange` when the draft is valid.
- No behavioral test asserts element placement, geometry, or labels.
- No API key appears in `Debug`, test output, or logs.
- Settings/Help open without disturbing an in-flight recording.

## Implementation Notes

- **View-model contract.** `Dialog`, `ProviderOption`, `ModelOption`,
  `SettingsDraft`, `SettingsChange` and the dialog commands live in
  `controller.rs`. `SettingsDraft` derives `Default`; its manual `Debug` and
  `SettingsChange`'s `Debug` redact the key (`[EMPTY]` / `[REDACTED]`).
- **Provider catalogue.** `provider_options()` is UI-owned and will be
  replaced by the Bun capabilities report in T0009. Each option also carries
  `api_key_env` (`AI_GATEWAY_API_KEY`, `MISTRAL_API_KEY`, …) so the Settings
  dialog can show the variable the worker reads, as in the mockup.
- **Dialogs are separate floating OS windows**, not in-window overlays. Each
  opens as its own `WindowKind::Floating` window (600×520 for both) beside the widget, so they never cover the main views.
  `views/dialogs/window.rs` holds the `DialogWindow` root view, which renders
  the controller-backed dialog and forwards every intent to `SpeechTtApp`.
- **Shared Header + Body.** `components::AppHeader` is the one window header:
  title left, actions right, `title_bar` background and bottom border. The
  widget passes the `?`/cog actions; each dialog passes a close button. Dialog
  titles are `APP_TITLE - Settings` and `APP_TITLE - Help & Shortcuts`, used
  for both the OS window and the in-window header. `DialogShell` now owns only
  the scrollable body and fixed footer, below the shared header.
- **Deferred opening.** Creating a window synchronously inside an entity
  update that renders `SpeechTtApp` trips GPUI's re-entrant borrow panic
  (`cannot read speechtt_ui::app::SpeechTtApp while it is already being
  updated`), because the Settings root reads the app immediately. Opening is
  therefore scheduled with `cx.defer`: the deferred pass re-checks that the
  request is still current, opens the window outside any entity borrow, then
  records the handle. If the platform refuses the window, the controller is
  reset so state never claims a dialog is open.
- **Window lifecycle.** Only one dialog window exists at a time; opening the
  other closes the first. Cancel, Escape, Save and Got it remove the window.
  An OS-level close is observed through `App::on_window_closed` and clears
  the controller state; a `Rc<Cell<Option<WindowId>>>` mirror is cleared
  before our own closes so that observer never re-enters the app entity.
- **Positioning.** `dialog_window_bounds` places the dialog at the widget's
  right edge plus a 16 px gap, falls back to the left edge when the right
  side would leave the display, and clamps both axes to the visible display
  bounds.
- **Interaction.** Header `?`/cog buttons open the corresponding window. The
  dialog root takes keyboard focus in its constructor, so `Escape` and the
  other key handlers receive events as soon as the window opens. The API key
  input is masked (`InputState::masked(true)`); Reveal toggles masking in
  place and the stored value is cleared on close.
- **Setting rows.** `SettingRow::prominent()` is the dialog style from
  `mockups/app-window/new-session.png`: a `--font-size-md` off-white title,
  a `--font-size-md` muted description (color alone marks the hierarchy),
  and a hairline `border` separator under each row. The footer buttons use
  the default medium size (`--font-size-md`, roomier padding), not `small`. The compact default (uppercase micro-label) stays in use
  for the 480×280 setup form so it still fits.
- **Help typography.** Shortcut labels and `KeyCap` keys use
  `--font-size-md`, with roomier keycap padding (`px_2 py_1`), so the
  reference reads at dialog scale rather than as fine print.
- **Shared buttons.** `ActionButton` (primary/secondary, fixed medium size,
  optional disabled) replaced every footer text button in Help, Settings and
  the session setup form. `IconButton` (ghost, fixed 32px) replaced the header
  help/cog and the dialog close. Sizes are no longer set at call sites, and
  `DESIGN.md` now states the rule: action buttons and chrome icons come from
  those components; `small` is reserved for dense inline controls.
- **Selection controls.** Provider and model are compact cycle controls
  (`cycle_settings_provider` / `cycle_settings_model`) with the catalogue
  rendered by `provider_option` / `model_option`. Changing provider clears an
  incompatible model. This avoids pulling a stateful list widget into this
  ticket; T0009 can swap the controls for a real select when capabilities
  arrive.
- **Save hook.** `SpeechTtApp::save_settings` forwards the result to a single
  `apply_settings_change(SettingsChange)` hook that stages
  `pending_settings` for T0009. `None` keeps the dialog open.
- **Visual QA.** Compared against both mockups with a temporary QA build.
  Verified in real OS windows at 600×520 (Help) and 600×460 (Settings),
  sitting beside the widget without overlapping it. Aligned the
  version/status footer, the `GATEWAY CONNECTED` treatment, the provider env
  var name, the Reveal-in-field affordance and the info-icon notes. The
  temporary QA hook was removed before finishing.
- **Gates.** `cargo fmt -- --check`, `cargo clippy --all-targets -- -D warnings`
  and the full `cargo test` suite pass (13 controller tests, 7 dialog tests,
  plus the existing core/session/theme/widget suites).
