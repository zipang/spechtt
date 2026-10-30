# T0017 — Mode chooser and nameless sessions

- **Track**: Rust core + UI + design system (cross-cutting)
- **Wave**: 2 (integration — changes the entry flow and the session contract)
- **Depends on**: T0015 (file takes, file picker), T0009 (screen wiring), T0007 (output/save)
- **Blocks**: —
- **References**: `mockups/app-window/new-session.png` (visual ground truth),
  `mockups/app-window/new-session.html` (measurements), `mockups/app-window/idle.png`
  (Ready screen), `DESIGN.md` § Components, `PRODUCT.md` § Session workflow,
  `AGENTS.md` §2–§5

## Context & Goal

The Blank state is a single 74 px `+` circle labelled "New Session" with the
hint "Choose a service, model and output". It is a leftover from before file
transcription existed (T0015), so the only way to transcribe a file is a
secondary button buried on the Ready screen. Setup also demands five
parameters, two of which have no product reason:

- **session name** — a label typed before the user knows what the take will
  become; nothing in the app displays it except the default save path;
- **output target** — a per-take Memory/File switch that duplicates the Save
  action.

The new product flow is: **pick a mode, pick provider + model, take. The file
the user saves gives the take its name.**

Done means:

1. The app opens on a **mode chooser**: two cards, "Live dictation" and
   "From a file", matching `mockups/app-window/new-session.png`.
2. With a model already configured, **Live dictation** goes straight to the
   Ready screen (mic) — no form, no name. **From a file** opens the native file
   picker immediately; the take is named by the source file.
3. The setup form (only shown when settings are incomplete) asks for
   **provider + model only**.
4. **Save** opens the native save dialog; the chosen file name titles the
   transcript and names the take. No `SessionName` or `OutputTarget` remains
   in the session contract.
5. Cards and round buttons are reusable components (`Card`, `RoundButton`),
   documented in `DESIGN.md`, with no raw colors or sizes in views.

### Product decisions (locked)

| Question | Decision |
|---|---|
| Setup fields | Mode + provider + model. The **name** and **output** rows are deleted. |
| Live card with settings ready | Land on **Ready** (one click to record); the idle mockup stays valid. |
| Live card with settings incomplete | The setup form (provider + model), then Ready. |
| File card with settings ready | **Open the file picker immediately**; cancel → File Ready screen. |
| File card with settings incomplete | Setup form → Continue → picker. |
| File take naming | Source file stem (`meeting.mp4` → `meeting`), suggested as the save name. |
| Save | Native save dialog (`prompt_for_new_path`), suggested `<stem>-<stamp>.md` (file) or `speechtt-<stamp>.md` (live). The chosen file's stem is the transcript title / take name. |
| Drag & drop | **Out of scope** this wave (browse only). |
| Window | Default **600×460**. |
| Round button | Rename `TransportButton` → `RoundButton`, **one size: 56 px** (from the mockup), Primary + Secondary tones, `-active` hover, click ripple. |
| T0015 Ready-screen file action | **Removed** — the mode chooser owns file entry. |

## Scope Boundary

May create or modify:

- `crates/speechtt-core/src/session.rs` — session contract (no name, no output).
- `crates/speechtt-core/src/output.rs` — `save_transcript_to`,
  `suggested_transcript_name`; delete `save_finished` + `OutputTarget` +
  `save_transcript_at` (see §2).
- `crates/speechtt-core/tests/session_machine.rs` — updated contract tests.
- `crates/speechtt-core/src/lib.rs` — **append-only** re-exports only.
- `crates/speechtt-ui/src/controller.rs` — `SessionMode`, `choose_mode`,
  `change_mode`, save-to-path, removal of name/output plumbing.
- `crates/speechtt-ui/src/app.rs` — mode routing, `prompt_for_new_path` save,
  ripple pulse counters, removal of the session-name input.
- `crates/speechtt-ui/src/tokens.rs` — `WIDGET_WIDTH/HEIGHT` + the one round
  button diameter constant.
- `crates/speechtt-ui/src/layout.rs` — `ScreenShell::headerless()`.
- `crates/speechtt-ui/src/views/{mod.rs,setup.rs,transport.rs,stopped.rs}`.
- `crates/speechtt-ui/src/components/mod.rs` and **new**
  `components/{card.rs,round_button.rs,icon_action.rs}`; **delete**
  `components/transport_button.rs` and `components/choice_pill.rs` (unused
  after the output row leaves).
- `crates/speechtt-ui/src/components/setting_row.rs` — doc comment only
  (stale `new-session.png` reference).
- `crates/speechtt-ui/tests/{common/mod.rs,widget.rs,session_flow.rs,end_to_end.rs,dialogs.rs}`.
- `DESIGN.md`, `PRODUCT.md`, `README.md`, `tickets/README.md`,
  `tickets/in_progress/T0015-*` (bookkeeping only, CTO edit).

Do **not** touch: `src/stt/**` (worker, file decoding, protocol unchanged),
`crates/speechtt-core/src/{protocol.rs,backend.rs,audio.rs,settings.rs}`,
`crates/speechtt-ui/src/views/dialogs/**`, `design-tokens.css`, `mockups/**`,
`crates/speechtt-core/tests/bridge_smoke.rs`.

No new source file may hardcode a color, radius, spacing or font size: read
`crate::tokens` / the active theme. No new token is needed (see §7).

## Contract

### 1. Session machine (`crates/speechtt-core/src/session.rs`)

```rust
/// The three parameters a take requires.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SessionField {
    AudioSource,
    Provider,
    Model,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct SessionConfig {
    pub audio_source: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct FinishedSession {
    pub audio_source: String,
    pub provider: String,
    pub model: String,
}
```

- `SessionConfig::missing()` returns `AudioSource, Provider, Model` in that
  order; `is_complete()` is true only when all three are set.
- `SessionState` is unchanged: `Blank`/`Ready` remain pure functions of
  completeness (`refresh`).
- Deleted: `SessionField::{SessionName, Output}`, `SessionConfig::{session_name,
  output}`, `set_session_name`, `set_output`, `FinishedSession::{session_name,
  output}`, the `OutputTarget` enum. `clear(field)` keeps its three remaining
  arms.
- `stop()` returns the `FinishedSession` and **clears the whole config**
  (`audio_source`, `provider`, `model`), leaving the machine `Blank`. Rationale:
  the next take starts from the mode chooser, which re-derives everything; the
  old "keep source/provider/model" reset only existed to pre-fill a removed form.
- **New** `SessionMachine::reset()`: refuses while `state.is_active()`
  (returns `SessionError::InvalidTransition { from }`); otherwise clears the
  config and sets `Blank`. Used by the UI's "Change mode".
- Invariants: a take never starts unless `Ready`; setup setters stay frozen
  during a take; refusals never mutate.

### 2. Save layer (`crates/speechtt-core/src/output.rs`)

```rust
/// Writes `text` to exactly `path`, overwriting what the OS dialog already
/// confirmed. `title` titles the document (Markdown heading). The parent
/// directory is created when missing.
pub fn save_transcript_to(
    path: &Path,
    title: &str,
    format: TranscriptFormat,
    text: &str,
) -> Result<SavedFile, OutputError>;

/// Suggested name for the save dialog: `<stem>-<YYYYMMDD-HHMMSS>.md`.
/// `stem` is sanitised; an empty result uses `speechtt`.
pub fn suggested_transcript_name(stem: &str, now: SystemTime) -> String;
```

- Empty/whitespace-only text ⇒ `OutputError::EmptyTranscript`.
- The file name is **exactly** `path`: no timestamp is appended and no
  collision suffix is invented (the save dialog owns naming). An existing
  target is removed before the atomic write, so "Replace" in the dialog works
  on every platform.
- `render(title, format, body)` is reused unchanged; an empty title falls back
  to the product name (existing `display_title`).
- Deleted: `save_finished` (it required `FinishedSession.output`), the
  `OutputTarget` enum, and `save_transcript_at` + its `file_stem` /
  collision-suffix loop. Nothing auto-names a save any more, so keeping two
  writers plus an unreachable collision path would be dead code.
  `sanitise_stem` and `format_timestamp` stay — `suggested_transcript_name`
  is now their only (and tested) user.

### 3. Controller (`crates/speechtt-ui/src/controller.rs`)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SessionMode {
    #[default]
    Live,
    File,
}
```

- `mode() -> SessionMode`, stored on `SessionController`, reset by
  `new_take()`/`change_mode()`.
- `choose_mode(mode) -> bool`:
  - refuses (returns `false`, no mutation) while a take is active;
  - sets `mode`, `screen = Screen::Mode`, resets the draft, and seeds
    provider + model from persisted settings **only when both resolve in the
    current catalogue** (the existing `provider_exists`/`model_exists` check);
  - when seeding resolved both: sets `audio_source = DEFAULT_AUDIO_SOURCE`,
    commits provider + model (`Ready`) and returns `true`;
  - otherwise: `screen = Screen::Form` and returns `false`.
- `create_session() -> bool`: commits the draft (provider + model only) to the
  machine, resets `screen`/draft, returns `true` on success. No name, no
  derived path.
- `change_mode()`: `machine.reset()` (idle only) + `screen = Screen::Mode` +
  draft reset. Backs the File Ready "Change mode" link.
- `start_recording`/`start_recording_at` **refuse unless `mode == Live`** — a
  file session must never open the microphone. (Defensive: the File Ready
  screen has no mic.)
- `start_file_session(&Path)` is unchanged in validation and wire behavior,
  except that it no longer touches a session name: it sets `audio_source` to
  the file's display name, then starts. It is the only path that starts a File
  take.
- `suggested_save_name(&self) -> Option<String>`: `Some(file_stem)` when the
  finished take is a file take (`file_take`), `None` for a live take.
- `save_finished_transcript_to(&mut self, path: &Path)`:
  replaces `save_finished_transcript` + `save_finished_file`. Derives the
  title from `path.file_stem()` (fallback: product name), picks the format
  from the extension (`.txt` → Text, anything else → Markdown), calls
  `save_transcript_to`, sets `written_path` and `output_status = "Saved"`.
- `open_saved_file()` uses `written_path` only (no output-target fallback).
- Deleted: `session_name()`, `set_draft_name`, `is_draft_complete_with_name`,
  `selected_output`, `set_draft_output`, `SessionDraft::{session_name,output}`,
  `ValidatedDraft::{session_name,output}`, `save_finished_transcript(directory)`,
  `save_finished_file`.
- `ViewState::Prompt` is renamed `ViewState::Mode`; `Screen::Prompt` is renamed
  `Screen::Mode`. `view_state()` mapping is otherwise unchanged.
- `is_draft_complete()` = provider and model both resolve in the catalogue.

### 4. App (`crates/speechtt-ui/src/app.rs`)

- `choose_mode(&mut self, mode: SessionMode, cx: &mut Context<Self>)`:
  calls `controller.choose_mode(mode)`; when it returns `true` **and**
  `mode == File`, calls `pick_transcription_file(cx)`. `open_form()` is deleted.
- `create_session(&mut self, cx)`: after a successful commit, if
  `controller.mode() == File`, open the picker.
- `change_mode(&mut self, cx)`: resets to the chooser.
- `save_transcript(&mut self, cx)`: computes
  `suggested_transcript_name(prefix, clock.system_now())` where `prefix` is the
  source stem for file takes (`controller.suggested_save_name()`) or
  `"speechtt"`; calls `cx.prompt_for_new_path(&documents_dir(), Some(&name))`;
  spawns and, on `Ok(Some(path))`, calls
  `controller.save_finished_transcript_to(&path)` then notifies. Cancel
  (`Ok(None)`) is silent; `Err` reports `AppError::FileDialogFailed`.
- **New** headless seam `save_transcript_to(&mut self, path: &Path, cx)` so
  tests never open a dialog.
- `finish_take()` no longer auto-saves file takes (they had an `OutputTarget`
  before); the Stopped screen offers Save for every take.
- The `session_name: Entity<InputState>` field, its construction, change
  subscription, `clear_session_name`, `session_name_input`, and the name read
  in `create_session`/`is_draft_complete` are deleted.
- Ripple state: `pulses: BTreeMap<SharedString, u64>` with
  `pub fn pulse(&mut self, id: &str)` (increments) and
  `pub fn pulse_count(&self, id: &str) -> u64`. Views read the count and the
  click listener bumps it before running the action; the widget test asserts
  the counter.

### 5. Components

**`RoundButton`** (`components/round_button.rs`, replaces `transport_button.rs`):

```rust
pub const ROUND_BUTTON_SIZE: f32 = 56.0; // mockup w-14

pub enum RoundTone { Primary, Secondary } // Default: Primary

pub struct RoundButton {
    id: ElementId,
    icon: IconName,
    tone: RoundTone,
    pulse: u64,
    on_click: Option<ClickHandler>,
}

impl RoundButton {
    pub fn new(id: impl Into<ElementId>, icon: IconName) -> Self;
    pub fn tone(self, tone: RoundTone) -> Self;
    /// Monotonic click counter from `SpeechTtApp::pulse_count`. 0 = no ripple
    /// yet; every later value starts exactly one ripple.
    pub fn pulse(self, pulse: u64) -> Self;
    pub fn on_click(self, handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self;
}

/// Pure ripple geometry: (diameter, border alpha) for an eased delta in 0..=1.
pub fn ripple_frame(delta: f32) -> (f32, f32);
```

- **One size only**: `ROUND_BUTTON_SIZE` (56 px), `rounded.full`, icon
  `Size::Large`. The old 74/64 px tones are gone; the Record screen uses 56 too.
- **Primary**: bg `theme.primary`, icon `theme.primary_foreground`; hover
  `theme.primary_active`; pressed keeps `primary` (no color change).
- **Secondary**: bg `theme.background` (`surface`), 1 px border
  `theme.border`, icon `theme.foreground`; hover bg `theme.secondary_active`
  and border `primary` at alpha 0.4 (mockup `hover:border-primary/40`);
  pressed keeps the hover fill.
- **Ripple**: a 2 px ring behind the button, colour = the tone's base colour
  (`primary` / `border`), diameter `56 → 84 px`, alpha `0.55 → 0.0`, over
  400 ms with `ease_out_quint()`, via
  `with_animation((id, pulse), Animation::new(..), ..)`. Rendered only when
  `pulse > 0`, absolutely positioned so it never affects layout. `pulse`
  changes remount the animation, which restarts it.
- `on_click` absent ⇒ no hover, no cursor, no click (visual-only inside cards,
  where the `Card` owns the click).
- `IconAction` moves to `components/icon_action.rs` unchanged (small square
  Stop/Copy/Save/Open action).
- Deleted: `TransportButton`, `TransportTone`.

**`Card`** (`components/card.rs`, new):

```rust
pub struct Card { /* id + children + optional click */ }

impl Card {
    pub fn new(id: impl Into<ElementId>) -> Self;
    pub fn child(self, content: impl IntoElement) -> Self;
    pub fn flex_1(self) -> Self;                  // equal-width columns
    pub fn on_click(self, handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self;
}
```

- `v_flex`, `w_full`, gap `space::MD`, padding `space::LG`, radius
  `radius::MD` (12), bg `theme.secondary` (surface-alt), 1 px border
  `theme.border`.
- Interactive cards: `cursor_pointer`, hover border `theme.primary` at alpha
  0.5 (mockup `hover:border-primary/50`), pressed bg `theme.button_active`.
- A non-interactive card renders no hover/cursor.

**`ChoicePill`** is deleted: the output row was its only user.

### 6. Views

**Mode chooser** (`views/setup.rs::render_mode(app, cx)`, replaces
`render_prompt`):

- `ScreenShell::headerless()` (new: no status header — the mockup has no pill
  on this screen) with `p_5`, `gap_4`.
- Title `rem::LG`, `FontWeight::SEMIBOLD`, "Choose a mode"; subtitle
  `rem::SM`, `muted_foreground`, "Select a transcription method".
- `h_flex`, gap `space::BASE`, two `Card::new(..).flex_1()`:
  - `mode-live`: title `rem::MD` semibold "Live dictation"; description
    `rem::SM` muted "Record and transcribe from your microphone in real time";
    centred `RoundButton::new("btn-mode-live", IconName::Mic)` (Primary) with
    `.pulse(app.pulse_count("btn-mode-live"))`; caption `rem::XS` medium muted
    "Start".
  - `mode-file`: title "From a file"; description "Import an audio or video
    recording"; `RoundButton::new("btn-mode-file", IconName::FileUp)`
    (Secondary); caption "Browse files" (drag & drop is out of scope, so the
    mockup's "Browse or drop" is not used).
- Cards are centred, text-centred, `justify_between` so captions align.
- Clicking `mode-live`/`mode-file` bumps that card's pulse and calls
  `app.choose_mode(..)`.
- No form, no status pill, no `+` button.

**Setup form** (`render_form`): only two `SettingRow::prominent` rows —
"Transcription Service" and "Model". The title input and the output pills are
removed. Footer: Cancel / **Continue** (`btn-create-session` keeps its id so
existing wiring stays; label changes from "Create session").

**Ready screen** (`render_ready(app, cx)`), branching on `mode`:

- Live: `RoundButton::new("btn-record", IconName::Mic)` + "Press to start
  dictating". The `btn-prompt-transcribe-file` action is removed.
- File: `RoundButton::new("btn-browse-file", IconName::FileUp)` (Primary)
  + "Browse a file" caption + a secondary `ActionButton`
  `btn-change-mode` labelled "CHANGE MODE" → `app.change_mode(cx)`.
  Clicking the round button re-opens the picker.

**Stopped screen**: no output-target branch; `written_path` decides between
Copy/Save and the written-path + Open layout.

### 7. Tokens

- `WIDGET_WIDTH = 600.0`, `WIDGET_HEIGHT = 460.0` (doc comment rewritten: the
  mode chooser's two 272 px cards and the tall form still fit).
- `ROUND_BUTTON_SIZE = 56.0` lives in `round_button.rs` (a component
  dimension, not a design token).
- No new design token: card radius `radius::MD`, padding `space::LG`, gap
  `space::BASE` already exist. `design:validate` must stay green unchanged.

## TDD Tasks

### Part A — Nameless session contract

- [ ] **T17.1 — Core contract specs (red)**: `session_machine.rs` — `missing()`
  returns only the three fields; `is_complete`; `set_*` freeze while active;
  `stop()` returns source/provider/model and leaves `Blank` with an empty
  config; `reset()` refuses while active and clears an idle machine. Delete the
  name/output cases.
- [ ] **T17.2 — Implement the core contract**: trim `session.rs` to
  `audio_source/provider/model`, delete `OutputTarget`, `set_session_name`,
  `set_output`, add `reset()`. Update doc comments (five parameters → three).
- [ ] **T17.3 — Save-layer specs (red)**: `output.rs` — `save_transcript_to`
  writes the exact path (no timestamp), overwrites an existing file, creates a
  missing parent, rejects empty text, Markdown title = `title`, `.txt` has no
  heading; `suggested_transcript_name` sanitises, stamps and falls back to
  `speechtt`. Delete the `save_finished`/`OutputTarget`/`save_transcript_at`
  tests and port their stem/timestamp assertions to the suggestion helper.
- [ ] **T17.4 — Implement the save layer**: add `save_transcript_to` +
  `suggested_transcript_name`, delete `save_finished`, `OutputTarget`,
  `save_transcript_at` and the collision loop.
- [ ] **T17.5 — Controller specs (red)**: `session_flow.rs` —
  `choose_mode(Live|File)` with complete vs incomplete settings; `create_session`
  commits provider + model; `start_recording` refused in File mode;
  `change_mode` resets to `Mode`; `save_finished_transcript_to` writes the
  chosen path and sets `written_path`; `suggested_save_name` (file stem vs
  `None`); `new_take` returns to `Mode`; no name/output API remains.
- [ ] **T17.6 — Implement the controller**: `SessionMode`, `choose_mode`,
  `change_mode`, save-to-path, delete name/output plumbing, rename
  `ViewState::Prompt` → `Mode`.

### Part B — Mode chooser UI

- [ ] **T17.7 — Component specs (red)**: in-file unit tests for
  `ripple_frame` (monotonic diameter, alpha decays to 0, delta clamped) and a
  `widget.rs` test that clicking `mode-live`/`mode-file` bumps
  `pulse_count` and renders a frame.
- [ ] **T17.8 — Implement `RoundButton` + `Card`**: new modules, move
  `IconAction`, delete `TransportButton`/`ChoicePill`, add
  `ScreenShell::headerless()`, update every transport call site to `RoundButton`
  with its pulse counter.
- [ ] **T17.9 — Implement the mode chooser and Ready branching**: `render_mode`,
  form rows, File Ready (browse + change mode), remove the T0015 ready-screen
  action, window 600×460, body padding for `ViewState::Mode`.
- [ ] **T17.10 — App wiring + save dialog**: `choose_mode`/`change_mode`/
  `create_session` picker routing, `prompt_for_new_path` save, remove the
  session-name input, stop auto-saving file takes. Widget test: click
  `mode-live` (empty settings) → Form; fill → Continue → Ready; click
  `btn-record` → Recording; finish → Stopped; click `btn-save` →
  `cx.simulate_new_path_selection` → file exists, path shown.
- [ ] **T17.11 — Docs**: `DESIGN.md` (Card, RoundButton 56 px + tones +
  ripple, record button size, headerless chooser), `PRODUCT.md` (workflow
  table, screen checklist, save/output rules), `README.md` (screen list,
  window size), `setting_row.rs` doc reference.
- [ ] **T17.12 — Gates + manual QA**: `cargo fmt -- --check`,
  `cargo clippy --all-targets -- -D warnings`, `cargo test`, `bun test`,
  `bun check src/stt`, `bun run design:validate`. Manual QA: side-by-side with
  `new-session.png` at 600×460; click ripple; live fast path; file fast path
  (pick, cancel, change mode); save dialog naming; dark-theme contrast.

## Acceptance Criteria

1. On launch the widget shows the mode chooser at 600×460: "Choose a mode",
   subtitle, two equal cards, no status pill, no `+` button.
2. With a configured model, **Live dictation** lands on Ready (mic) with no
   form and no name; one click starts the take.
3. With a configured model, **From a file** opens the native picker
   immediately. Picking a file starts the take; cancelling leaves the File
   Ready screen with "Browse a file" and "Change mode".
4. With no configured model, either card opens a setup form with exactly two
   rows (Service, Model); Continue commits them and routes as in (2)/(3).
5. No session name or output target is configured anywhere: the core config,
   controller API, form and Ready screen contain neither.
6. Save opens the native save dialog with a suggested name
   (`<source-stem>-<stamp>.md` for file takes, `speechtt-<stamp>.md` for
   dictation); the chosen file is written at exactly that path, its stem titles
   the document, and the Stopped screen shows the path + Open.
7. File takes are no longer auto-written at stop; Save is an explicit action
   for both modes.
8. `RoundButton` has exactly one size (56 px), Primary/Secondary tones, hover
   uses the `-active` variant, and a click triggers one expanding ring; a
   `RoundButton` inside a card is visual-only and the card owns the click.
9. `Card` is a reusable component; views contain no raw colors, radii, spacing
   or font sizes.
10. All gates green with zero warnings; no test skipped or `#[ignore]`d
    beyond the existing gated live smoke test.

## Implementation Notes

_(added during work)_
