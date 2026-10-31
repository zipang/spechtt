# T0021 — One consistent footer and a live-first navigation flow

- **Track**: Rust UI (views + components)
- **Wave**: 2 (integration)
- **Depends on**: T0017 (mode chooser), T0019 (one view per file), T0020
  (empty takes; it rewrites `views/stopped.rs` and `controller.rs`), T0023
  (file mode order is picker → form → take, which removes the last file-mode
  holding screen), so this ticket must land after them
- **Blocks**: T0022 (the live take screen builds on this footer)
- **References**: `DESIGN.md` §Components, `AGENTS.md` §2 (view rule, component
  rule), `PRODUCT.md` Phase 9

## Context & Goal

The workflow has no reliable way out. From the mode chooser the user presses
"Live dictation" and lands on the **Ready** screen, which does nothing: it only
repeats a microphone button that the recording screen already owns. The file
flow has the same dead end — its ready screen offers "CHANGE MODE", an exit that
sits in a different place from every other exit in the app.

Actions are also scattered: Cancel/Continue on the setup form live in a
window-level footer, the stopped screen puts "New take" in the header and
Copy/Save in the body footer, and the transport screens use a third
arrangement. Nothing teaches the user where "go back" is.

**Done means:** the live mode opens the live take screen directly, the file
flow has no holding screen at all, and every remaining screen exposes its
actions through the same footer with the same three zones — back on the left,
the main action in the middle, secondary actions on the right — so "go back" is
always in the same place.

## Scope Boundary

May create or modify:

- `crates/speechtt-ui/src/components/screen_footer.rs` — **new**: the three-zone
  footer and its back affordance.
- `crates/speechtt-ui/src/components/mod.rs` — **append-only** registration.
- `crates/speechtt-ui/src/layout.rs` — only if `ScreenShell::footer` needs to
  accept the new footer without a second wrapper.
- `crates/speechtt-ui/src/lib.rs` — **append-only** icon registration
  (`ArrowBigLeft`).
- `crates/speechtt-ui/src/views/welcome.rs`, `session_setup.rs`, `stopped.rs`,
  `recording.rs` — footer migration only.
- `crates/speechtt-ui/src/views/ready.rs` → **delete** (no replacement: after
  T0023 neither mode has a holding screen).
- `crates/speechtt-ui/src/controller.rs` — `ViewState` variants, `view_state()`
  derivation, `choose_mode` entry behaviour.
- `crates/speechtt-ui/src/app.rs` — screen routing, removal of the form's
  window-level footer, back handlers.
- `crates/speechtt-ui/tests/{widget,session_flow,end_to_end,design_system}.rs`.
- `DESIGN.md` — the footer section and the per-view action table.
- `tickets/README.md` — board row.

Do **not** touch: `design-tokens.css`, `tokens.rs`, the round-button tones, the
recording-screen visual design (T0022), the worker, or any dialog window
(`views/dialogs/` keeps its own window-level `AppFooter`). No new colour,
spacing, radius or type token in this ticket. No layout or geometry test.

## Contract

### 1. The live mode never shows a Ready screen

```rust
pub enum ViewState {
    Mode,    // mode chooser
    Form,    // provider + model setup (live or file)
    Live,    // live take screen (idle / recording / paused)
    Stopped, // finished take
}
```

- `ViewState::Ready` is removed, and no file-mode replacement screen is added:
  after T0023 the file flow is picker → form → take, so a "file ready" state
  has nothing left to show.
- `SessionState::Ready` in `speechtt-core` is unchanged: it still means "the
  configuration is complete". Only the *view* collapses.
- `view_state()`:
  - a finished take ⇒ `Stopped`;
  - `Blank` + `Screen::Mode` ⇒ `Mode`;
  - `Blank` + `Screen::Form` ⇒ `Form`;
  - live mode with a complete configuration (`Ready`, `Recording`, `Paused`) ⇒
    `Live`;
  - file mode ⇒ `Live` only while a file take is running, otherwise `Form`.
- `choose_mode(SessionMode::Live)` leaves the machine `Ready` and shows `Live`
  immediately. **Microphone capture does not start**: capture starts on the
  take screen's own control (T0022). No `SttCommand::Start` is sent here.
- File mode keeps T0023's order: the mode card opens the picker, cancelling it
  returns to `Mode`, a chosen file opens `Form`, and Continue starts the take.

### 2. `components::ScreenFooter`

One footer, three zones, used by every widget screen:

```rust
pub enum FooterZone { Leading, Primary, Trailing }

pub struct ScreenFooter { … }

impl ScreenFooter {
    pub fn new() -> Self;
    /// Left zone: the back affordance.
    pub fn back(self, id: impl Into<ElementId>, label: &'static str,
                on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self;
    /// Left zone: any other element.
    pub fn leading(self, element: impl IntoElement) -> Self;
    /// Centre zone: the screen's main action.
    pub fn primary(self, element: impl IntoElement) -> Self;
    /// Right zone: secondary actions, left to right in insertion order.
    pub fn trailing(self, element: impl IntoElement) -> Self;
}
```

Rules:

- `back` renders `components::IconAction` (34 px) with `IconName::ArrowBigLeft`,
  so back is the **same size and shape as every other secondary action** and
  the big round button is unmistakably the main one.
- The centre zone is centred on the footer's own axis, independent of how much
  content the leading and trailing zones hold (equal flexible side zones, no
  offset when one side is empty).
- Zones may be empty; an empty footer renders nothing.
- The footer draws the same strip as today's window footer (same surface role,
  top hairline, same horizontal padding) so migrating the setup and stopped
  screens shifts no pixel.
- The component picks no colour, radius, size or spacing of its own beyond
  tokens already used by `AppFooter`/`IconAction`; views still never pass
  `px(..)`, `rems(..)` or `radius::` (gate `tests/design_system.rs`).

### 3. Per-screen action table

| View | Leading (left) | Primary (centre) | Trailing (right) |
|---|---|---|---|
| `welcome` | — | — | — (no footer) |
| `session_setup` | Back → `cancel_form()` (discards a pending file) | Continue (`ActionButton::primary`, disabled until the draft is complete) | — |
| `recording` (live) | Back/Cancel → home (T0022) | Take control (T0022) | Reset + Stop (T0022) |
| `stopped` | Back → `new_take()` | — | Copy, Save, or Open file (as today) |

- The stopped screen's header no longer carries "New take": going home is now
  the footer's back action everywhere.
- `session_setup` no longer renders through the window-level `AppFooter`;
  dialogs keep using it unchanged.

### 4. Back semantics are uniform

- Back always means "one step towards the mode chooser": `Form → Mode`,
  `Live → Mode` (discarding an unfinished take, T0022), `Stopped → Mode` (as
  `new_take` does today).
- Back never leaves a stale state behind: the previous screen's draft, pending
  file, errors, output status and finished take are cleared exactly as
  `change_mode` / `cancel_form` / `new_take` already do.
- Back is refused (and the control is not rendered as actionable) while an
  operation is genuinely in flight — for this ticket: nothing; the take-screen
  case is T0022's.

## TDD Tasks

- [ ] **T21.1 — Red first: view states.** `tests/session_flow.rs` and
      `tests/widget.rs`: choosing Live with a complete configuration reports
      `ViewState::Live` (never `Ready`) and does not send `Start`; choosing File
      keeps `ViewState::Mode` when the picker is cancelled and reports `Form`
      once a path is pending; every existing `ViewState::Ready` assertion is
      rewritten to `Live` or `Form` according to the mode.
- [ ] **T21.2 — Red first: back contract.** For each of `Form`, `Live`,
      `Stopped`, one test drives the back action and asserts the resulting
      `ViewState::Mode` plus cleared transient state (no error strip, no
      finished take, no output status, no pending file). These are behaviour
      tests; no positions, sizes or geometry are asserted.
- [ ] **T21.3 — Implement `ScreenFooter`.** Golden-shape unit tests for the
      builder (zones default to empty, back builds an `IconAction`, trailing
      keeps insertion order). No rendering geometry assertions.
- [ ] **T21.4 — Migrate the views.** `session_setup`, `stopped`, and the
      current `recording` use the footer; `app.rs` drops the form's
      window-level footer; `welcome` stays headerless and footerless.
- [ ] **T21.5 — Remove the Ready view.** Delete `views/ready.rs` and register
      `ArrowBigLeft` in `AppIcons`. No file-mode holding screen replaces it.
- [ ] **T21.6 — Docs + gates.** Update `DESIGN.md` (footer spec + per-view
      action table + view list) and the board. Run `cargo fmt -- --check`,
      `cargo clippy --all-targets -- -D warnings`, `cargo test`, `bun test`,
      `bun check src/stt`, and `bun run design:validate` if any doc/token file
      changed. All green, zero warnings.

## Acceptance Criteria

1. Choosing "Live dictation" opens the take screen directly. No intermediate
   Ready screen exists anywhere in the live flow.
2. No file-mode holding screen exists: the file flow is picker → setup form →
   take, and cancelling the picker returns to the welcome screen (T0023).
3. Every widget screen that has actions renders them in one footer, with back
   on the left, the main action centred, and secondary actions on the right.
4. Back returns to the mode chooser from the setup form, the live take screen,
   and the stopped screen, with no stale draft, pending file, error or
   transcript left behind.
5. The setup and stopped screens look the same as before the migration apart
   from where the back/home action sits.
6. All gates green with zero warnings; no visual-layout test added.

## Implementation Notes

_(added during work)_
