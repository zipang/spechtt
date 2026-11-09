# T0035 — Window-scoped keyboard shortcuts

- **Track**: Cross-cutting (Rust core: command vocabulary · Rust UI: bindings, dispatch, Help · docs)
- **Wave**: 1 (independent)
- **Depends on**: nothing
- **Blocks**: nothing
- **Owner**: coding sub-agent, supervised by CTO
- **Started**: 2026-11-09
- **References**: `PRODUCT.md` §3.3 (rewritten by this ticket) · `DESIGN.md`
  § "Shapes" (keycaps already exist as `components::KeyCap`; no new visual
  rule) · GPUI Kit documentation:
  - KeyBinding (signature, keystroke grammar, context precedence, `cx.propagate`):
    <https://gpui-kit.com/docs/keybinding.md>
  - Action (declaring commands, `on_action`, propagation):
    <https://gpui-kit.com/docs/action.md>
  - Testing (`window.press`, `run_until_parked`, `Task::ready`):
    <https://gpui-kit.com/docs/test.md>
  - Action index (`llms.txt`): <https://gpui-kit.com/llms.txt>

## Context & Goal

The Help dialog's **Shortcuts** tab is written by hand in
`crates/speechtt-ui/src/views/dialogs/help.rs` (`HELP_SHORTCUTS`) and does not
describe the program. It advertises `Ctrl+Shift+C`, `Ctrl+Shift+V` and
`Ctrl+Shift+F` — three bindings that do not exist — and omits the keys that do
work. The tab is the only place a user can learn the keyboard, so as it stands
the product lies about its controls.

At the same time every shortcut is registered with the `global-hotkey` crate,
so the widget grabs keys from the whole desktop. That is incompatible with what
the product now wants: **F1** opens Help, **F2** opens Settings, **Space**
starts / pauses / resumes a take, **Enter** stops it, and **Ctrl+S** saves the
finished transcript. A system-wide `Space` would stop the user from typing in
any other application, and a system-wide `Ctrl+S` would steal "Save" from all of
them.

**Goal.** Keyboard commands become **window-scoped**: they act only while the
SpeechTT window (or one of its dialog windows) holds focus, registered through
GPUI's keymap. The set shrinks to those five commands — no global hotkeys, no
per-user rebinding — and the Help dialog is rendered **from the same table that
registers the bindings**, so the two can never drift again.

Accepted consequence (CEO decision): dictation can no longer be started or
stopped from another application. Every command requires the widget window to be
focused.

## Scope Boundary

May change:

- `crates/speechtt-core/src/hotkeys.rs` → **renamed** to
  `crates/speechtt-core/src/actions.rs` (`git mv`), then reduced to the command
  vocabulary.
- `crates/speechtt-core/src/lib.rs` (module line + any `pub use`).
- `crates/speechtt-core/Cargo.toml` (drop `global-hotkey`; `Cargo.lock` follows).
- `crates/speechtt-core/src/settings.rs` (legacy `shortcuts` field only).
- `crates/speechtt-ui/src/shortcuts.rs` (**new**).
- `crates/speechtt-ui/src/lib.rs`, `src/app.rs`, `src/controller.rs`,
  `src/views/dialogs/help.rs`, `src/views/dialogs/window.rs`,
  `src/views/recording.rs`.
- `crates/speechtt-ui/tests/shortcuts.rs` (**new**), `tests/common/mod.rs`,
  `tests/end_to_end.rs`.
- `PRODUCT.md` §3.3, `tickets/README.md`, this ticket.

Do **not** touch: `DESIGN.md` (no new token, component or visual rule — the
Help rows keep the existing `KeyCap`/`Body` components), any component file,
`mockups/`, the TypeScript worker, or `src/stt/**`. Do not add a second keycap
component, a shortcut editor UI, or a settings row for shortcuts.

## Contract

### A. Command vocabulary — `speechtt-core/src/actions.rs`

The module keeps **only** the action enum; the key strings live in the UI layer
(§B) because they are GPUI keystrokes.

```rust
/// A command the widget can run from the keyboard, a button, or a dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SessionAction {
    ToggleRecording,
    StopRecording,
    ToggleHelp,
    ToggleSettings,
    SaveTranscript,
}

impl SessionAction {
    /// Every action, in the order the Help dialog lists them.
    pub const ALL: [Self; 5] = [
        Self::ToggleRecording,
        Self::StopRecording,
        Self::SaveTranscript,
        Self::ToggleHelp,
        Self::ToggleSettings,
    ];

    /// The stable kebab name, used in docs and error text.
    #[must_use]
    pub const fn as_str(self) -> &'static str;
}
```

`as_str` values: `toggleRecording`, `stopRecording`, `toggleHelp`,
`toggleSettings`, `saveTranscript`. `Display` delegates to `as_str`.

**Deleted from the module** (and their tests): `Shortcut`, `Modifier`,
`canonical_key`, `ShortcutError`, `HotkeyError`, `ShortcutListener`,
`ListenerGuard`, `GlobalHotkeyListener`, `default_bindings`, `ctrl_shift`,
`resolve`, `conflicts`, `bind`, `bindings_from_settings`, `to_hotkey`,
`code_name`. Nothing in the repository may import `global_hotkey` any more;
`crates/speechtt-core/Cargo.toml` drops the dependency.

Module header states: which commands exist, their default keys (the table in
§B), that bindings are **window-scoped**, and that this module owns no keystroke
string.

### B. Binding table — `speechtt-ui/src/shortcuts.rs` (new)

One table is the single source of truth for the bindings **and** for the Help
dialog rows.

```rust
/// One keyboard command: what it is called, when it applies, and its keys.
pub struct Binding {
    pub action: SessionAction,
    /// The row's description in the Help dialog.
    pub description: &'static str,
    /// The key context predicate the command is registered under, so it only
    /// matches in the state where it can act (see §C).
    pub context: &'static str,
    /// GPUI keystroke on Linux and Windows (see the KeyBinding page).
    pub keys: &'static str,
    /// The same command with the platform primary modifier on macOS.
    pub mac_keys: &'static str,
}

/// Every binding, in Help order.
pub const BINDINGS: [Binding; 5];

/// Registers [`BINDINGS`] on the application keymap.
pub fn bind_keys(cx: &mut App);

/// The keycaps the Help dialog draws for `binding` (`["Ctrl", "S"]`).
#[must_use]
pub fn key_caps(binding: &Binding, is_mac: bool) -> Vec<&'static str>;
```

Table (Help order):

| `action` | `description` | `context` | `keys` | `mac_keys` |
|---|---|---|---|---|
| `ToggleRecording` | `Start · pause · resume recording` | `SpeechTT && Take` | `space` | `space` |
| `StopRecording` | `Stop recording and show the transcript` | `SpeechTT && Take` | `enter` | `enter` |
| `SaveTranscript` | `Save transcript to a file` | `SpeechTT && SessionEnd` | `ctrl-s` | `cmd-s` |
| `ToggleHelp` | `Open or close Help` | `SpeechTT \|\| SpeechTTDialog` | `f1` | `f1` |
| `ToggleSettings` | `Open or close Settings` | `SpeechTT \|\| SpeechTTDialog` | `f2` | `f2` |

**Documented API being used** (<https://gpui-kit.com/docs/keybinding.md>):

- `KeyBinding::new(keys, action, context)` takes a `&'static str` keystroke, an
  Action **value**, and an optional context predicate; `cx.bind_keys(...)` takes
  them as a collection and must run after `gpui_kit::init(cx)`.
- Keystroke grammar: a key name with hyphen-separated modifiers; named keys are
  `space`, `enter`, `f1`; `ctrl-s` is Control on every platform, `cmd-s` the
  platform modifier on macOS. Choose with `cx.platform_info().is_mac()`, exactly
  as the page's snippet does.
- The page states `KeyBinding::new` **panics** on an unparsable key string or
  context, so the table is static and covered by a test that parses each entry.

`bind_keys` binds every row under its own `context` (§C); `key_caps` splits the
keystroke of the active platform into display caps: `f1` → `["F1"]`,
`space` → `["Space"]`, `enter` → `["Enter"]`, `ctrl-s` → `["Ctrl", "S"]`,
`cmd-s` → `["Cmd", "S"]`. Modifiers keep this order: `Ctrl`/`Cmd`, `Alt`,
`Shift`, `Super`.

### C. Registration and state-scoped contexts

A command is not "always available while the widget has focus". It is available
**only in the state where it means something**, because a keystroke that matches
but cannot act is a keystroke stolen from a button, a toggle or a text field.
Save has no meaning before a take ends; pause and stop have none before it
starts. The keymap therefore knows the session state, not just the window.

```rust
// app.rs
pub const SPEECHTT_CONTEXT: &str = "SpeechTT";              // exists
pub const SPEECHTT_DIALOG_CONTEXT: &str = "SpeechTTDialog"; // new

// lib.rs::run, after gpui_kit::init(cx):
shortcuts::bind_keys(cx);
```

`SpeechTtApp`'s root element declares **one context string that is rebuilt from
the controller every frame** — the window fact plus exactly one state fact:

| Fact | When it is on the root |
|---|---|
| `Take` | `view_state() == ViewState::Live` — the live take screen, Ready to start or already active |
| `Active` | `session_state().is_active()` — Recording or Paused |
| `SessionEnd` | `view_state() == ViewState::SessionEnd` — a finished transcript is on screen |

```rust
// SpeechTtApp::render
let context = match self.view_state() {
    ViewState::Live => "SpeechTT Take",
    ViewState::SessionEnd => "SpeechTT SessionEnd",
    _ => SPEECHTT_CONTEXT,
};
```

`Active` is not a separate string: `Take` covers the recording screen, and the
`Active` distinction is the controller guard in §D (a Space that matches on the
turn screen is still refused while the machine is `Ready`… which is the start,
so it applies). The three facts are mutually exclusive because they are chosen
from one `match`.

`bind_keys` registers each row under its own predicate (§B, `context` column):

| Command | Predicate |
|---|---|
| `ToggleRecording` | `SpeechTT && Take` |
| `StopRecording` | `SpeechTT && Take` |
| `SaveTranscript` | `SpeechTT && SessionEnd` |
| `ToggleHelp` | `SpeechTT \|\| SpeechTTDialog` |
| `ToggleSettings` | `SpeechTT \|\| SpeechTTDialog` |

Why this split: the Help and Settings dialogs live in their **own windows**, so
their dispatch path carries `SpeechTTDialog` and never `SpeechTT`, `Take` or
`SessionEnd`. Space therefore keeps typing a space into the Settings API-key
field, and Enter stays the activation key of every dialog button, without any
per-handler negotiation. Inside the widget window, `Take` keeps Space and Enter
away from the mode chooser and the setup form, and `SessionEnd` keeps `Ctrl+S`
away from every screen that has nothing to save.

Still make every handler decline through `cx.propagate()` (§D): the predicate is
the first gate, the state guard is the second, and a command that matches in a
rare race must not swallow the key.

Actions are declared once, so keystroke, button and dialog reach one handler:

```rust
actions!(speechtt, [
    CopySelection,      // exists
    ToggleRecording,
    StopRecording,
    ToggleHelp,
    ToggleSettings,
    SaveTranscript,
]);
```

`SpeechTtApp` root adds `.on_action(...)` for the five new actions;
`DialogWindow` root adds handlers for `ToggleHelp` and `ToggleSettings` only
(§F).

**Precedence note (from the KeyBinding page):** a contextual binding matches only
when its context lies on the *focused* dispatch path, and an inner context ranks
ahead of an ancestor. A focused `components` button therefore keeps its own
Space/Enter activation; the window binding only wins when focus sits on the
window root. Do not register a `None`-context binding.

**Documented-API note:** `KeyBinding::new` takes `&'static str` for the
predicate, so the predicates are static; only the *element's* context string is
dynamic, which is what the `.key_context(...)` builder accepts.

### D. Dispatch and guards

Every handler runs `SpeechTtApp::apply_action` and **declines** with
`cx.propagate()` when the command does not apply, so the keystroke falls through
to the focused control. The KeyBinding page documents this: "A handler that
declines an Action can call `cx.propagate()` to let dispatch continue."

```rust
/// Runs a keyboard command through the same guards as its button.
///
/// Returns `false` — having changed nothing — when the command is inert in the
/// current state; the key handler then propagates the keystroke.
pub fn apply_action(
    &mut self,
    action: SessionAction,
    window: &mut Window,
    cx: &mut Context<Self>,
) -> bool;
```

Behaviour per action:

- **`ToggleRecording`** → `self.toggle_recording(cx)`, which delegates to
  `SessionController::toggle_recording_at(now)`:
  `Recording` → pause, `Paused` → resume, `Ready` (live mode) → start, every
  other state or a file take → `false`. On a start it also does what
  `start_recording` does (`rewind_caption`, `start_audio_capture`).
  `views/recording.rs`'s record button **stops duplicating this match** and calls
  the same `app.toggle_recording(cx)`; button and Space cannot diverge.
- **`StopRecording`** → `self.finish_take(cx)` when
  `controller.session_state().is_active()`, else `false`.
- **`ToggleHelp` / `ToggleSettings`** → if that dialog is already the open one,
  `close_dialog(window, cx)` (a real toggle); otherwise `open_help` /
  `open_settings`. Always `true`.
- **`SaveTranscript`** → `self.save_transcript(cx)` (§E): `false` when there is
  no finished take or a prompt is already open.

### E. Save prompt seam

`Ctrl+S` must be testable headlessly, so the platform dialog joins the other
injected side effects (`AudioSource`, `ClipboardSink`, `PasteSink`, `FileOpener`).

```rust
// app.rs
/// Opens the platform save dialog for the finished take.
pub trait SavePrompt: Send {
    /// The path the user chose, or `None` when they cancelled.
    fn pick_save_path(&mut self, cx: &mut App, suggested: &str) -> Task<Option<PathBuf>>;
}

pub struct AppServices {
    // ... existing fields, minus `listener`
    pub save_prompt: Box<dyn SavePrompt>,
}
```

Production implementation `GpuiSavePrompt` moves the **existing** expression from
`SpeechTtApp::save_transcript` verbatim
(`cx.prompt_for_new_path(&documents_dir(), Some(suggested))`, `app.rs` — the
current code is the reference shape, do not invent a new call) and maps the
result to `Option<PathBuf>`; a platform error is reported through
`AppError::FileDialogFailed` exactly as today.

`SpeechTtApp` gains a real, documented guard:

```rust
/// Whether a save dialog is open. A second Ctrl+S while the OS dialog is up
/// must not stack another one.
pub fn save_prompt_open(&self) -> bool;
```

`save_transcript` returns `bool`: `false` with no finished take
(`SessionController::can_save_transcript(&self) -> bool`, true iff a finished
result exists) or while `save_prompt_open`; otherwise it sets the flag, asks the
`SavePrompt`, and on completion clears the flag and calls `save_transcript_to`.

### F. Dialog windows

`DialogWindow::render` root declares `SPEECHTT_DIALOG_CONTEXT` and handles
`ToggleHelp` / `ToggleSettings` by forwarding to the app entity it already holds
(`self.app()`), passing the dialog window. This makes **F1 with Help open close
it**, **F2 with Help open switch to Settings**, and the reverse. The new dialog
opens anchored to the current dialog's screen — acceptable, and the only
geometry the code already supports.

### G. Help dialog

`HELP_SHORTCUTS` is deleted. The Shortcuts tab renders
`shortcuts::BINDINGS` and, per row, `binding.description` with
`KeyCap::new(shortcuts::key_caps(binding, is_mac))`. The tab shows exactly the
five commands that exist; adding a binding to the table adds its row.

### H. Settings migration

`global-hotkey` is gone and nothing reads the persisted map, but
`Settings` uses `#[serde(deny_unknown_fields)]`: removing the field would make
every existing settings file fail to load. Keep it as legacy-only:

```rust
/// Action name -> shortcut string, written by releases that had rebindable
/// global shortcuts. Kept so an older file still loads; cleared by
/// [`Settings::normalize`] so the next save drops it. Nothing reads it.
#[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
pub shortcuts: BTreeMap<String, String>,
```

`Settings::normalize` gains `self.shortcuts.clear();` (same place as the legacy
provider/model cleanup). Update the settings tests that assume the key is always
serialized or that its value survives a round trip.

### I. Product documentation

`PRODUCT.md` §3.3 becomes the truth:

| Action | Shortcut |
|---|---|
| Start / pause / resume recording | `Space` |
| Stop recording | `Enter` |
| Save transcript to file | `Ctrl+S` |
| Open Help | `F1` |
| Open Settings | `F2` |

Paragraph: shortcuts are window-scoped (they act while the widget or one of its
dialogs is focused), the set is fixed, and the Help tab is generated from the
same table. Delete the "configurable in Settings / top info strip reflects the
bindings" sentence, which is no longer true.

## TDD Tasks

- [x] **T1.1 — Red: command vocabulary.** In `actions.rs` (after the rename),
      tests for `SessionAction::ALL` order, `as_str` values and `Display`. Delete
      the old shortcut-parser/listener tests. `cargo test -p speechtt-core`
      compiles and fails only on the new expectations.
- [x] **T1.2 — Green: reduce the module.** Keep `SessionAction` + `ALL` +
      `as_str` + `Display`; delete everything else in §A; `git mv` the file;
      update `lib.rs` and every import; drop `global-hotkey` from
      `speechtt-core/Cargo.toml`.
- [x] **T2.1 — Red: controller toggle.** Unit tests: `Ready`+live → `Recording`;
      `Recording` → `Paused`; `Paused` → `Recording`; `Blank`/`Stopped` and a
      file take → `false` with no command sent. Plus
      `can_save_transcript()` false before a finish, true after.
- [x] **T2.2 — Green: `SessionController::toggle_recording_at(now)`** built from
      `start_recording_at` / `pause_recording_at` / `resume_recording_at`,
      `SessionController::can_save_transcript()`, and `app.toggle_recording(cx)`
      (caption rewind + capture start on a start only).
- [x] **T3.1 — Red: binding table.** Unit tests in `shortcuts.rs`:
      `BINDINGS` covers `SessionAction::ALL` exactly once, in Help order; no two
      rows share a keystroke; `key_caps` yields `["F1"]`, `["Space"]`,
      `["Enter"]`, `["Ctrl", "S"]`, `["Cmd", "S"]`.
- [x] **T3.2 — Green: `shortcuts.rs`** with the table, `bind_keys` and
      `key_caps`; call `shortcuts::bind_keys(cx)` from `lib.rs::run`; declare the
      five actions; add `SPEECHTT_DIALOG_CONTEXT`.
- [x] **T4.1 — Red: keystrokes reach the app.** New headless test file
      `crates/speechtt-ui/tests/shortcuts.rs` driving a real window with
      `window.press("f1", cx)` etc. (KeyBinding/test pages):
      - `f1` opens Help; `f1` again closes it;
      - `f2` opens Settings;
      - `space` from the live screen starts the take, then pauses, then resumes;
      - `enter` while recording stops the take and shows the transcript;
      - `space` on the mode chooser starts nothing;
      - `f2` pressed in the Help dialog switches the open dialog to Settings.
- [x] **T4.2 — Green: handlers.** `apply_action` per §D, `.on_action(...)` on the
      widget root, `cx.propagate()` on every `false`, and `views/recording.rs`
      delegating its record button to `app.toggle_recording(cx)`.
- [x] **T5.1 — Red: Ctrl+S.** With a fake `SavePrompt` returning a temp path:
      `ctrl-s` on the transcript screen writes the file and clears the flag;
      `ctrl-s` with no finished take never asks; two presses while a prompt is
      open ask once.
- [x] **T5.2 — Green: `SavePrompt` seam** in `AppServices`, `GpuiSavePrompt`,
      `save_prompt_open`, `save_transcript -> bool`; update
      `tests/common/mod.rs`, `tests/end_to_end.rs` and the mock listener's
      removal there.
- [x] **T6.1 — Red: Help tab is the table.** Test that the Shortcuts tab renders
      one row per `BINDINGS` entry (labels from `description`); the old
      `HELP_SHORTCUTS` const no longer exists.
- [x] **T6.2 — Green: Help dialog** renders from `BINDINGS` + `key_caps`.
- [x] **T7.1 — Red: settings legacy field.** A file containing
      `"shortcuts": {"startRecording": "Ctrl+Shift+R"}` still loads, and the next
      save omits the key; `normalize` clears it.
- [x] **T7.2 — Green: settings changes** per §H; fix the affected settings tests.
- [x] **T4.3 — Red: the keymap follows the state machine.** Headless tests
      proving every command is absent outside the state it acts on: `ctrl-s` on
      the mode chooser, on the setup form and on the live take screen asks
      nothing and writes nothing; `enter` on the mode chooser and on the
      transcript screen changes nothing; `space` on the setup form and on the
      transcript screen changes nothing; and inside the Settings dialog a
      `space` typed into the API-key field inserts a space while the session
      state stays untouched.
- [x] **T4.4 — Green: state-scoped contexts.** `SpeechTtApp::render` builds the
      root's `key_context` from `view_state()` (`SpeechTT Take`,
      `SpeechTT SessionEnd`, else `SpeechTT`); `Binding` gains its `context`
      (already in §B) and `bind_keys` uses it instead of one shared predicate.
      The three take/save rows drop `SpeechTTDialog` from their predicate
      entirely.
- [x] **T8.1 — Docs & gates.** Rewrite `PRODUCT.md` §3.3; update
      `tickets/README.md`; then run, from the repository root:
      `cargo fmt -- --check`, `cargo clippy --all-targets -- -D warnings`,
      `cargo test`, `bun test`, `bun check src/stt`, `bun run design:validate`.

## Acceptance Criteria

- Pressing **F1** while the widget window is focused opens the Help dialog;
  pressing **F1** again closes it. **F2** opens Settings, and either key switches
  the dialog when the other one is open.
- **Space** starts a live take, then pauses it, then resumes it — the same
  sequence as the record button, and the button and the key cannot disagree
  because they share one method. Space does nothing on the mode chooser, on the
  setup form, on a file take, or on the transcript screen.
- **Enter** stops a recording or paused take and shows the transcript; it does
  nothing when no take is active, and it does nothing on any other screen.
- **Ctrl+S** on a finished transcript opens the save dialog and writes the file;
  it does nothing when no finished take exists, cannot stack two dialogs, and is
  **not bound at all** on the mode chooser, the setup form or the live take
  screen.
- A command is available only in the state it acts on: Space and Enter are bound
  under `SpeechTT && Take`, `Ctrl+S` under `SpeechTT && SessionEnd`. Typing a
  space into the Settings API-key field inserts a space (the take command does
  not run), and Enter still activates the focused dialog button.
- No keystroke is registered with the OS: `global-hotkey` is no longer a
  dependency, and every shortcut requires SpeechTT to hold focus.
- The Help → Shortcuts tab lists exactly the five commands above with their real
  keys, generated from the same table that registers the bindings.
- A settings file written by an earlier release still loads and loses its dead
  `shortcuts` key on the next save.
- `PRODUCT.md` §3.3 matches the program, including the state scoping.
- All six gates pass with zero warnings; no test is skipped or `#[ignore]`d.

## Implementation Notes

### What landed

- `crates/speechtt-core/src/hotkeys.rs` was `git mv`-renamed to
  `crates/speechtt-core/src/actions.rs` and reduced to `SessionAction` + `ALL` +
  `as_str` + `Display`, with the five-command vocabulary from §A. The parser,
  the listener seam, the `global-hotkey` backend and their tests are deleted;
  `crates/speechtt-core/Cargo.toml` no longer lists `global-hotkey` and
  `Cargo.lock` lost the crate (plus its now-unused `crossbeam-channel`).
- `crates/speechtt-ui/src/shortcuts.rs` (new) holds `Binding`, `BINDINGS`,
  `bind_keys`, `key_caps` and the `is_mac()` helper. `lib.rs::run` calls
  `shortcuts::bind_keys(cx)` right after `gpui_kit::init(cx)`.
- `app.rs` declares the five GPUI actions next to `CopySelection`, declares
  `SPEECHTT_DIALOG_CONTEXT`, rewrites `apply_action` per §D, adds the five
  `on_action` handlers (each declining with `cx.propagate()`), adds
  `toggle_recording`, adds the `SavePrompt`/`GpuiSavePrompt` seam,
  `save_prompt_open` and the boolean `save_transcript`.
- `controller.rs` gains `toggle_recording_at(now)` and `can_save_transcript()`;
  the old `SessionAction`-based `apply_action` is gone.
- `views/recording.rs`'s record button now calls `app.toggle_recording(cx)`, so
  the button and **Space** share one code path.
- `views/dialogs/help.rs` renders `BINDINGS` (no `HELP_SHORTCUTS`) and each row
  carries the element id `help-shortcut-<action>`.
- `views/dialogs/window.rs` declares `SPEECHTT_DIALOG_CONTEXT` and forwards
  `ToggleHelp`/`ToggleSettings` to the app.
- `settings.rs` keeps `shortcuts` as a legacy-only map (`skip_serializing_if`
  empty) and `normalize` clears it.
- `PRODUCT.md` §3.3 rewritten; `tickets/README.md` already listed T0035 in the
  In Progress table, so it needed no edit.

### Deviations and judgement calls

1. **`ALL` order.** §A originally listed `ALL` as
   `[ToggleRecording, StopRecording, ToggleHelp, ToggleSettings, SaveTranscript]`
   while §B's table (and the `PRODUCT.md` table) put `SaveTranscript` before the
   two dialog toggles. Resolved in the second review round: §A was corrected to
   `[ToggleRecording, StopRecording, SaveTranscript, ToggleHelp, ToggleSettings]`,
   so the contract and the code now agree with §B. `ALL`, `BINDINGS` and the
   Help dialog all use that reading order, pinned by `every_action_is_listed_once`
   and `the_table_is_in_help_order`.
2. **`can_save_transcript` is stricter than "a finished result exists".** It
   returns `!finished_take_is_empty()`, i.e. true only when there is text to
   write, which is exactly the predicate the transcript screen's Save button
   uses. With `result.is_some()` as the literal reading, `Ctrl+S` would open a
   save dialog for an empty take and then fail with `NothingRecorded`.
3. **`SavePrompt` cannot report a platform failure.** As specified (and
   documented) the trait returns `Task<Option<PathBuf>>`, so a cancelled dialog
   and a dialog that could not open are the same `None`. `save_transcript` no
   longer reports `AppError::FileDialogFailed` on this path; the file *picker*
   (`pick_transcription_file`) still does. A richer return type (or a second
   trait method) would restore it and would deviate from the contract.
4. **Documented-API gap: `cx.platform_info()` does not exist in the pinned
   version.** `https://gpui-kit.com/docs/keybinding.md` (gpui-kit 0.9.x) uses
   `cx.platform_info().is_mac()`, but the workspace pins `gpui-kit = "0.8"`
   (gpui-pre 0.3.9): `App` has no `platform_info`, and the `Platform` trait's
   `platform_info` was added after 0.3.9 (`gpui_kit::platform::current_platform()`
   does return `Rc<dyn Platform>`, but the trait exposes no OS accessor at this
   version). `shortcuts::is_mac()` therefore answers from the build target
   (`cfg!(target_os = "macos")`), which is what a native build's key conventions
   follow. Revisit when the framework is bumped.
5. **Dialog toggling needed window-handle hygiene, not just `close_dialog`.**
   The §D/§F wording (call `close_dialog`, forward with the dialog window) would
   reach into the dispatching dialog window through its `AnyWindowHandle` to
   remove it, and would place the replacement dialog with `window.window_handle()`
   of a window that is closing itself. Two additions make the switch safe and
   are documented in `app.rs`:
   - `SpeechTtApp::release_dialog_window`: when the command arrives from the
     dialog's own window, only the tracked handle is forgotten and the dialog
     removes itself (`DialogWindow::forward` calls `window.remove_window()` when
     it is no longer the active dialog); any other window closes it as before.
   - `SpeechTtApp::shell_window` (captured at construction): the deferred dialog
     open is always anchored to the widget window, so it never updates the
     window that is disappearing. Observable behaviour matches §F: F2 in Help
     replaces it with Settings (2 windows, no flicker of a closed dialog).
6. **Help rows use element ids for the T6.1 assertion.** `Body` exposes no
   accessibility label and components are out of scope, so each row is a
   `test_support()` div with the stable id `help-shortcut-<action>`; the test
   walks `BINDINGS` and finds each row. No visual decision changed: the row
   still composes `Body` + `KeyCap` with the same tokens.
7. `SpeechTtApp::start_recording` stays as the start-only command (used by the
   headless tests); the button and `Space` use `toggle_recording`.

### Gates (run from the repository root, all green)

- `cargo fmt -- --check` — clean.
- `cargo clippy --all-targets -- -D warnings` — `Finished` with no warnings.
- `cargo test` — 229 + 5 + 14 + 5 (core) and 120 + 0 + 3 + 12 + 37 + 14 + 96 +
  10 + 4 + 35 + 0 + 0 (ui) passed, 0 failed.
- `bun test` — 348 pass, 0 fail, 1269 expect() calls, 16 files.
- `bun check src/stt` — no type errors in 32 files.
- `bun run design:validate` — design contract valid, 66 tokens, 0 errors.

### Review round

Polish pass after `POLISH_REQUIRED`, plus the CEO's state-scoping requirement
(contract §B/§C and tasks T4.3/T4.4).

1. **Commands are scoped to the state machine (§C).** `SpeechTtApp::render`
   rebuilds the root's key context from the controller every frame
   (`ViewState::Live` → `SpeechTT Take`, `ViewState::SessionEnd` →
   `SpeechTT SessionEnd`, else `SpeechTT`), `Binding` carries the predicate the
   command is registered under (`SpeechTT && Take`, `SpeechTT && SessionEnd`,
   `SpeechTT || SpeechTTDialog`), and `bind_keys` uses each row's own value.
   `ContextScope` and its `predicate()` are gone.
2. **Decisions taken for item 1.** (a) The predicates are literals because
   `KeyBinding::new` takes `&'static str`; the compatibility with the context
   constants is pinned by `every_predicate_names_the_declared_context_constants`
   instead of by composing the strings at run time. (b) The dialog case of T4.3
   needed the API-key row scrolled into view first: it is the last row of the
   scrolling body and is clipped by the 640 px dialog, so the test scrolls the
   body (a reachability step a user performs) before clicking the field. It then
   proves the field receives the space (`"sk "`) and the session state stays
   `Blank`. That is the user-visible outcome, not a proof of the predicate: the
   dialog window registers no take handler, so a keystroke that matched would
   fall through to the field and produce the same `"sk "`. The predicate is
   pinned by the table tests in `shortcuts.rs`; a runtime absence proof is not
   available at the pinned framework version (see the second review round).
3. **Ctrl+S reports a failed save dialog.** `SaveAnswer { Chosen, Cancelled,
   Failed }` replaces `Task<Option<PathBuf>>`: `Ok(Ok(Some(path)))` → `Chosen`,
   `Ok(Ok(None))` → `Cancelled`, `Ok(Err(_)) | Err(_)` → `Failed`, and a
   `Failed` answer reports `AppError::FileDialogFailed`. `ScriptedSavePrompt`
   gained `chosen` / `cancelled` / `failed` (keeping `answering`), and two tests
   cover the cancel and failure answers. `pick_transcription_file` is untouched.
4. **`caps_of` and `secondary`.** Already correct in the tree: `secondary` maps
   to `Ctrl` off macOS and `Cmd` on it, and the unhandled-token test proves a
   stroke whose modifier is not handled yields no caps at all rather than a
   shortened list that looks valid. Verified by
   `secondary_follows_the_platform` and `an_unhandled_token_has_no_caps`; no
   change was needed.
5. **Context-literal hole closed** by
   `every_predicate_names_the_declared_context_constants` (item 4 of the
   review), plus `the_take_and_save_predicates_never_mention_a_dialog` for the
   consequence §C states.
6. **Uniform decline rule.** All five handlers now propagate on `false`; the
   two dialog toggles already did after T4.2, so this was verified rather than
   changed.
7. **`release_dialog_window`'s "any other window" branch** is covered by
   `f2_in_the_widget_window_replaces_an_open_help_dialog`: F2 from the widget
   window while Help is open closes Help from outside and opens Settings, two
   windows throughout. Provoked deliberately (commenting out the
   `close_dialog_window` call makes the test report three windows), so the test
   has teeth.
8. **`SessionAction::ALL`** is `[ToggleRecording, StopRecording,
   SaveTranscript, ToggleHelp, ToggleSettings]`, documented as declaration
   order, which is also the Help dialog's reading order; `BINDINGS` and both
   order tests agree.
9. **Documentation corrections (AGENTS.md §3).** `app.rs`'s module header and
   `apply_action` describe `cx.propagate()` as letting *action dispatch* continue
   to the next candidate, and state separately that a focused component keeps
   its own Space/Enter through keybinding **precedence**. `actions.rs` no longer
   duplicates the key column and points at `crate::shortcuts::BINDINGS`; its
   `as_str` doc no longer claims error text. `speechtt-core/src/lib.rs` opens
   with a present-tense description of the crate. The Help-row test documents
   that it proves row presence by id, not label text.
10. **Global-hotkey leftovers (scope extension).** `AGENTS.md` §2 names the
    keyboard **command vocabulary** and the registry table drops its
    `global-hotkey` row. `PRODUCT.md` no longer claims global/OS-registered
    shortcuts anywhere (vision, users, interactions, stack, test list, Phase 2/4/8
    rows, §7 topology), and §3.3 now states the window **and state** scoping.
    `README.md`'s `speechtt-core` paragraph describes the core as it is.
11. **Deviation.** None beyond the two decisions above. The test-only helper
    `common::EventSender` is reused by the new T4.3 tests; no production module
    outside the ticket's scope boundary changed, and nothing under
    `.opencode/agents/` was touched.

### Second review round

Polish pass on the second review's `POLISH_REQUIRED` list.

1. **The dialog test says which proof it is — and the predicate proof is an API
   gap.** `space_typed_into_the_settings_api_key_field_stays_a_character` keeps
   its behaviour proof (the field receives the space, `"sk "`, and the session
   stays `Blank`) and its doc comment now names it as the **fall-through** proof:
   `DialogWindow` registers no take handler, so a reverted predicate would let
   the keystroke reach the field anyway. The discriminating assertion the review
   asked for — `window.bindings_for_action(&ToggleRecording).is_empty()`, plus
   `StopRecording` and `SaveTranscript` — **could not be added**. The method
   exists on the pinned `Window` (gpui-pre 0.3.9) and compiles, but returns no
   bindings for *any* action, and the CTO re-verified it independently after the
   fact: with `f1` pressed in the widget window, Help opens (the binding
   demonstrably dispatches) while `bindings_for_action(&ToggleHelp)` still
   answers `0`, and `bindings_for_action(&CopySelection)` — bound in
   `lib.rs::run` — answers `0` as well. `highest_precedence_binding_for_action`
   returns `None` for the same actions. The method is therefore unusable as an
   oracle in this version, not merely empty for an unfocused window: it was
   first misdiagnosed as a focus problem, and the probe ruled that out. The
   assertion was removed rather than shipped vacuous, and no substitute was
   invented. The predicate is pinned instead by the table-literal tests
   (`the_take_commands_are_take_scoped_and_the_toggles_are_not`,
   `every_predicate_names_the_declared_context_constants`,
   `the_state_facts_appear_in_the_take_and_save_predicates`,
   `the_take_and_save_predicates_never_mention_a_dialog`); the behavioural
   proof stays `space_typed_into_the_settings_api_key_field_stays_a_character`.
   **Open follow-up:** `Window::bindings_for_action` is documented as returning
   "key bindings that invoke an action on the currently focused element", so it
   should be re-probed after the framework migration (0.9.1, where the same
   method exists) and the assertion added if it answers there. Tracked in
   `tickets/planned/T0036-gpui-kit-0-9-migration.md`.
2. **F1 on the two composed-context screens.**
   `f1_opens_help_on_the_live_take_screen` (`SpeechTT Take`) and
   `f1_opens_help_on_the_transcript_screen` (`SpeechTT SessionEnd`) press `f1`
   after `prepare_live_take` / `finish_a_take` and assert Help opens in its own
   window, so the `SpeechTT` alternative of the toggle predicate is pinned on
   both screens whose context string is composed.
3. **The state facts are named.** `app.rs` declares `SPEECHTT_TAKE_CONTEXT`
   (`SpeechTT Take`) and `SPEECHTT_SESSION_END_CONTEXT` (`SpeechTT SessionEnd`)
   next to `SPEECHTT_CONTEXT`, documents why they are separate, and `render`
   uses them. `every_predicate_names_the_declared_context_constants` compares
   predicate **terms** (split on `&&`/`||`, trimmed) instead of substrings, and
   the new `the_state_facts_appear_in_the_take_and_save_predicates` asserts
   every identifier of each composed context appears in its predicates.
   Provoked: mutating `ToggleRecording` to `NotSpeechTT && Take` fails both
   tests (and `the_take_commands_are_take_scoped_and_the_toggles_are_not`), so a
   near-miss token can no longer satisfy the check.
4. **The default test save prompt is scripted.** `tests/common/mod.rs` hands the
   default fixtures (`services_with_events` → `open_speechtt_window`,
   `open_speechtt_window_at`, `open_speechtt_window_with_channels`) a
   `ScriptedSavePrompt::cancelled()`, so they no longer reach
   `documents_dir()`/`$HOME/Documents`. One fixture is the documented exception:
   `open_speechtt_window_with_events` keeps `GpuiSavePrompt`, because
   `tests/widget.rs::saving_from_the_stopped_screen_writes_the_chosen_path`
   answers that dialog with `simulate_new_path_selection`; that test file is
   outside the ticket's scope boundary, so the fixture keeps the adapter and the
   module doc states which fixtures do what. A `Ctrl+S` test still supplies its
   own prompt through `open_speechtt_window_with_save_prompt`.
5. **Documentation corrections.** `app.rs`'s module header now separates the two
   mechanisms: inside the widget window binding precedence keeps a focused
   control's Space and Enter; in a dialog window the take bindings do not match
   at all (no `SpeechTT` fact) and the dialog registers no take handler, so
   nothing is outranked there. `shortcuts.rs` names the `KeyContext` type as the
   authority for the stacked `SpeechTT Take` declaration (a context holds a
   *set* of whitespace-separated identifiers), cites the KeyBinding page's
   "Declare Key Contexts", and warns against rewriting it as
   `SpeechTT mode=take`. `AppError::FileDialogFailed` now renders "Could not
   open the file dialog.", true for the picker and the save prompt, and its
   variant doc says so; no test asserted the old string.
6. **Ticket.** Contract §A's `ALL` array now matches the code and §B, deviation 1
   records the resolution instead of the conflict, and review-round item 2b no
   longer claims the dialog test distinguishes the predicate.

Gate output after this round, from the repository root:

- `cargo fmt -- --check` — clean (no output).
- `cargo clippy --all-targets -- -D warnings` — finished with
  `Finished dev profile [unoptimized + debuginfo] target(s)`, no warnings.
- `cargo test` — 229 + 5 + 14 + 5 (speechtt-core) and 126 + 0 + 3 + 12 + 37 + 14 + 96 + 23 + 4 + 35 + 0 + 0 (speechtt-ui) passed, 0 failed, 0 ignored.
- `bun test` — `348 pass`, `0 fail`, `1269 expect() calls`, `Ran 348 tests across 16 files`.
- `bun check src/stt` — `No type errors in 32 files`.
- `bun run design:validate` — `Design contract valid — 66 tokens, 0 errors.`

## Validation

Closed after the CEO/PM tested the shortcuts by hand. The report: *"le ticket
T0035 est validé."*

Validated by observation, not by inference:

- **F1** opens Help and closes it again; **F2** opens Settings, and either key
  switches the dialog while the other is open.
- **Space** on the live take screen starts, pauses and resumes the same take the
  record button drives, because both call `SpeechTtApp::toggle_recording`.
- **Enter** ends the take and shows the transcript.
- **Ctrl+S** on a finished transcript opens the save dialog and writes the file.
- The keys that do not belong to a screen stay out of its way: no take command
  matches on the mode chooser, the setup form or the transcript screen, and no
  save command matches before a transcript exists.

### Follow-ups this ticket leaves behind

- `tickets/planned/T0036-gpui-kit-0-9-migration.md` re-probes
  `Window::bindings_for_action`, which returned no bindings for any action under
  gpui-kit 0.8.0, and restores `shortcuts::is_mac()` to the documented
  `App::platform_info()` accessor.
