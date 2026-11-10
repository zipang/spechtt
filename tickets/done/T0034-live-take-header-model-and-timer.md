# T0034 — Show the model and the timer on the live take screen from the start

- **Track**: Rust UI (components + view + controller) / design system
- **Wave**: 1 (independent)
- **Depends on**: T0028 (the model dropdown component), T0029 (per-mode defaults)
- **Blocks**: nothing
- **Owner**: CTO
- **Started**: 2026-11-05
- **References**:
    - `AGENTS.md` §2 (component and view rules), §3 (documentation), §5 (gates)
    - `DESIGN.md` § "Live take screen", § "Action buttons", § "Settings panel"
    - gpui-kit `Select`: <https://gpui-kit.com/component/select.md>
    - `SelectState`: <https://docs.rs/gpui-component/latest/gpui_component/select/struct.SelectState.html>

## Context & Goal

The live take screen tells the user nothing about the take before it starts. Two
facts they need are hidden:

- **The elapsed time only appears once recording has begun.** `views/recording.rs`
  attaches `TransportHeader` only `if active`, so the idle screen — the one where
  the user decides to speak — shows no clock at all. A `00:00` read-out before
  the first word is what makes the later `00:07` legible.
- **The model is nowhere on the screen.** It is visible in Settings and on the
  setup form, but once the take screen is open the user has no idea which engine
  is listening. The streaming/batched distinction in particular changes what to
  expect, and `DESIGN.md` already says that distinction "belongs to the model
  picker" — which this screen does not have.

Done means: the take screen shows, at its trailing header edge, the chosen model
on the left and `mm:ss` on the right, from the moment the screen appears; and the
model is a real dropdown that changes the model for the take about to be taken.

## Scope Boundary

Allowed to create or modify:

- `crates/speechtt-ui/src/components/transport_header.rs`
- `crates/speechtt-ui/src/components/live_model_select.rs` (new)
- `crates/speechtt-ui/src/components/mod.rs` (append-only)
- `crates/speechtt-ui/src/views/recording.rs`
- `crates/speechtt-ui/src/controller.rs`
- `crates/speechtt-ui/src/app.rs`
- `crates/speechtt-ui/tests/{session_flow.rs,widget.rs,common/mod.rs}`
- `DESIGN.md` (§ "Live take screen")
- `tickets/README.md` and this ticket

Out of scope — do **not** touch:

- `crates/speechtt-core/**` (`SessionMachine` already allows this; see the
  contract's constraint)
- `crates/speechtt-ui/src/views/{welcome,session_setup,session_end,session_transcription}.rs`
- `crates/speechtt-ui/src/views/dialogs/**`
- `src/stt/**`
- `crates/speechtt-ui/src/tokens.rs`, `design-tokens.css`, `tests/design_system.rs`

## Contract

### 1. Controller: name the model the take will use

The setup draft is consumed by `create_session`, so `provider_label()` and
`model_label()` return `None` on the take screen: they read a draft that no
longer exists. The serve-the-screen accessor is a new one that reads the take in
progress, then the persisted default:

```rust
/// Catalogue label of the model the live take uses (or will use).
///
/// The active take's own model wins, because the machine freezes it when
/// recording starts; before that, the default the settings resolve for Live.
/// `None` when no catalogue has arrived or nothing resolves.
#[must_use]
pub fn live_model_label(&self) -> Option<&str>;

/// The same model as an id, for the picker's selected value.
#[must_use]
pub fn live_model_id(&self) -> Option<&str>;

/// Models the live picker may offer for the take being prepared.
///
/// Every model of the live provider, not only the streaming ones: a live take
/// may use a segmented model, which batches the microphone audio.
#[must_use]
pub fn live_model_options(&self) -> Vec<SelectOption>;

/// Chooses the model for the take being prepared.
///
/// Returns `false` — changing nothing — when the catalogue does not know the
/// id, when the take mode is not Live, or when the session is already active
/// (the machine freezes its configuration from the first recorded frame).
pub fn set_live_model(&mut self, model: &str) -> bool;
```

`set_live_model` writes the session machine's model through
`SessionMachine::set_model`, which is already guarded by `guard_idle()`: it
accepts the change in `Blank`/`Ready` and refuses it in `Recording`/`Paused`.
The controller's own guard makes the refusal explicit instead of silently
swallowing an error.

**This choice is take-scoped, not persisted.** The picker changes the model for
the take about to be recorded; it does not rewrite the `model_streaming` default
in Settings, and the reason is in the contract: a live take may legitimately use
a segmented model, which `default_model_for(Live)` does not accept, so writing it
back would break the seeding T0029 guarantees. Changing the default stays
Settings' job.

### 2. Component: the header shows a model and a clock

`TransportHeader` gains the model on the left of the clock, which is the layout
the user asked for:

```rust
impl TransportHeader {
    /// Trailing read-out: the model (or a dropdown) on the left, `mm:ss` on the
    /// right.
    ///
    /// `pick` carries the picker when the session can still change its model
    /// and a plain label otherwise, so the header never offers a control that
    /// cannot act.
    pub fn new(elapsed: u32, pick: TransportModel) -> Self;
}

/// How the header presents the take's model.
pub enum TransportModel {
    /// A live dropdown the user can change.
    Picker(ModelSelect),
    /// A read-only label: no catalogue yet, no model resolved, or the take is
    /// frozen and the model can no longer change.
    Label(SharedString),
    /// Nothing to show: no catalogue and no resolved model.
    Absent,
}
```

`components/live_model_select.rs` (new, exported from `components/mod.rs`) owns a
`SelectState<SearchableVec<ModelOption>>` for the take screen, so no view builds
one and `views/recording.rs` stays a composition:

```rust
/// Model dropdown for the live take screen.
pub struct LiveModelSelect { /* id, state, placeholder, disabled */ }

impl LiveModelSelect {
    pub fn new(id: impl Into<ElementId>, state: &Entity<SelectState<SearchableVec<ModelOption>>>) -> Self;
    pub fn placeholder(self, placeholder: impl Into<SharedString>) -> Self;
    pub fn disabled(self, disabled: bool) -> Self;
}

/// The take screen's model state plus the snapshot it was last built from.
pub struct LiveModelField { /* … */ }

impl LiveModelField {
    pub fn new(window: &mut Window, cx: &mut App) -> Self;
    pub fn state(&self) -> &Entity<SelectState<SearchableVec<ModelOption>>>;
    /// Rebuilds items and selection only when the snapshot changes, with the
    /// same rule the Settings selects use.
    pub fn sync(&mut self, options: Vec<SelectOption>, selected: Option<&str>, window: &mut Window, cx: &mut App) -> bool;
}
```

The select is small — `Sizable::small` — and its `menu_width` leaves room for the
longest gateway id, as the Settings menus do.

### 3. View: always show the header, disable the picker when frozen

`views/recording.rs`:

- **always** attaches `TransportHeader`, in every state (`Ready`, `Recording`,
  `Paused`), so `00:00` is visible before the first word;
- builds `TransportModel::Picker` when `session_state()` is not active **and** a
  catalogue resolves at least one model, `TransportModel::Label` when the take is
  active or nothing can be picked, `TransportModel::Absent` when no model
  resolves at all;
- routes `SelectEvent::Confirm(Some(id))` to `SpeechTtApp::set_live_model`, which
  commits through the controller and repaints.

`SpeechTtApp` owns one `LiveModelField`, created in
`with_services_and_factory` (where a window is available) and synced from
`render` while `view_state() == Live`, exactly as `draft_selects` is synced on the
form screen. A `cx.subscribe` on its state lives in `_subscriptions`.

### 4. Behaviour the tests must pin

- At `Ready`, the take screen shows `00:00`, not an absent header.
- At `Ready`, the header shows the model the settings resolve for Live.
- Confirming a different model in the picker changes the model the next `Start`
  carries, and does **not** change the persisted `model_streaming` default.
- While `Recording` and `Paused`, the picker is not offered: the header shows a
  read-only label, and `set_live_model` returns `false`.
- With no catalogue, the header still shows the clock and no model.
- The clock keeps advancing while recording, unchanged.

## TDD Tasks

- [x] **T34.1 — Red: controller accessors.** Failing tests for `live_model_label`
      / `live_model_id` (active take wins over settings; `None` with no catalogue),
      `live_model_options` (every model of the provider, streaming and segmented),
      and `set_live_model` (accepts in `Ready`, refuses in `Recording`/`Paused`,
      refuses an unknown id, leaves `model_streaming` untouched). Then implement.
- [x] **T34.2 — Red: the header.** Failing unit tests for `TransportHeader`'s
      three `TransportModel` variants and for `LiveModelField::sync` idempotence.
      Then implement the component and `live_model_select.rs`.
- [x] **T34.3 — Green: the view.** Always attach the header; pick the variant per
      state; wire the confirm event. Headless test: at `Ready` the screen shows
      `00:00` and the model; after choosing another model, `Start` carries it; while
      recording the picker is gone.
- [x] **T34.4 — Docs and gates.** `DESIGN.md` § "Live take screen": the trailing
      edge now holds the model on the left and the clock on the right, with the
      picker's enabled/disabled states and why. Module headers and item docs per
      `AGENTS.md` §3. Then `cargo fmt -- --check`,
      `cargo clippy --all-targets -- -D warnings`, `cargo test`, `bun test`,
      `bun check src/stt`, `bun run design:validate`.

## Acceptance Criteria

- The live take screen shows `00:00` from the moment it appears, before the
  microphone is pressed, and the clock advances as it does today.
- The chosen model is displayed at the trailing header edge, to the **left** of
  the clock, in every state of the screen.
- Before recording, the model is a dropdown listing the models the live provider
  offers; choosing one changes the model the take will use.
- While recording or paused, the model is shown as text and cannot be changed,
  because the session configuration is frozen from the first recorded frame.
- With no catalogue, the clock is still shown and the model simply absent.
- No view picks a colour, size, spacing or radius: the header is a component and
  the select is an existing one.
- All six gates are green with zero warnings and no skipped tests.
- Final visual review is left to the user.

## Open question for the CEO

The picker is **take-scoped**: it changes the model for the take being prepared
and leaves the Settings default alone. If it should also become the remembered
Live default, that needs a rule for segmented models (which a live take may use
but `model_streaming` does not accept), and it is a follow-up rather than part of
this ticket.

## Implementation Notes

### Decisions

- **`TransportModel` carries the three presentations.** The view picks one from
  the session state; it never inspects select internals. A frozen take cannot
  offer a control that would be refused, so it shows a `Label`.
- **`live_provider` falls back to the catalogue's first provider.** The screen must
  name a model even on an install whose settings were never saved;
  `active_provider` alone returns an empty string in that case, which made the
  picker offer nothing.
- **`LiveModelField` mirrors the Settings selects.** Same snapshot rule, same
  `apply_models` helper (now `pub(crate)`), so a repaint that changes nothing
  leaves the menu alone and a selection the catalogue dropped is cleared.
- **Take-scoped, as agreed.** `set_live_model` writes the session machine's model
  and never touches `settings.model_streaming`.
- **The picker is not searchable.** It sits in a header beside the clock and the
  live catalogue is short; a search box there would outweigh the list.

### Deviations

- **`views/session_transcription.rs` needed one line.** `TransportHeader::new`
  gained a second argument, so the file-take screen must pass one. It passes
  `TransportModel::Absent`, which renders exactly as before — zero visual change,
  and the ticket keeps that screen out of scope. Showing the file take's own model
  there is a worthwhile follow-up, not part of this ticket.
- `components/model_select.rs` changed by two visibility keywords
  (`apply_models`, `searchable_model_state` → `pub(crate)`), so the new component
  reuses them instead of duplicating the sync rule.

### Gates (repository root, zero warnings, no skipped tests)

| Command                                     | Result                   |
| ------------------------------------------- | ------------------------ |
| `cargo fmt -- --check`                      | clean                    |
| `cargo clippy --all-targets -- -D warnings` | clean                    |
| `cargo test`                                | **582 passed, 0 failed** |
| `bun test`                                  | 348 passed, 0 failed     |
| `bun check src/stt`                         | 32 files, no type errors |
| `bun run design:validate`                   | 66 tokens, 0 errors      |

### Left for the user

Visual review of the take screen: `00:00` and the model before the first word,
the dropdown's width beside the clock, and the read-only name once recording
starts. No GUI run or screenshot was taken autonomously.
