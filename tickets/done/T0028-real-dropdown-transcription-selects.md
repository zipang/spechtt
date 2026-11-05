# T0028 — Real dropdown menus for the transcription provider and model

- **Track**: Rust UI (components + views + controller) / design system
- **Wave**: 1 (independent)
- **Depends on**: nothing
- **Blocks**: nothing
- **Owner**: coding sub-agent, supervised by CTO
- **Started**: 2026-11-05
- **References**:
  - `AGENTS.md` §2 (component rule, view rule), §5 (quality gates), §6 (official documentation rule)
  - `DESIGN.md` § "Session setup form" and § "Settings panel"
  - gpui-kit `Select` guide: <https://gpui-kit.com/component/select.md>
  - `SelectState` API: <https://docs.rs/gpui-component/latest/gpui_component/select/struct.SelectState.html>
  - `Select` API: <https://docs.rs/gpui-component/latest/gpui_component/select/struct.Select.html>
  - `SelectEvent` API: <https://docs.rs/gpui-component/latest/gpui_component/select/enum.SelectEvent.html>
  - `SearchableListItem` (re-exported as `SelectItem`) API: <https://docs.rs/gpui-component/latest/gpui_component/searchable_list/trait.SearchableListItem.html>
  - `SearchableVec` API: <https://docs.rs/gpui-component/latest/gpui_component/searchable_list/struct.SearchableVec.html>

## Context & Goal

The provider and model fields in the **Session setup** screen and in the
**Settings** dialog are ordinary buttons that change their selection on every
click. The user cannot see which models exist, cannot compare their
streaming/batch mode before choosing, and cannot reach a distant entry except
by clicking through the whole list. This is now the main usability defect of the
setup flow: the catalogue contains many gateway models and only a real list
makes them discoverable.

This ticket replaces those four controls with real gpui-kit `Select` dropdown
menus:

- **Provider** — a non-searchable menu listing every provider in the capability
  catalogue. Selecting one commits the provider and clears the dependent model.
- **Model** — a **searchable** menu listing the models the current provider
  offers (file takes keep the existing "batch models only" rule). Every menu row
  shows the model label **and** its `TranscriptionMode` badge; after selection the
  same badge stays visible beside the field.
- **Settings** — the provider and model controls use the same menus against the
  settings draft.

Done means: a click opens a menu, keyboard navigation and search work, choosing
an entry commits the same provider/model state the old cycling buttons produced,
and the mode badge is visible both inside the menu and beside the field.

## Scope Boundary

Allowed to create or modify:

- `crates/speechtt-ui/src/components/model_select.rs` (new)
- `crates/speechtt-ui/src/components/mod.rs` (append-only exports)
- `crates/speechtt-ui/src/app.rs`
- `crates/speechtt-ui/src/views/session_setup.rs`
- `crates/speechtt-ui/src/views/dialogs/settings.rs`
- `crates/speechtt-ui/src/views/dialogs/window.rs`
- `crates/speechtt-ui/src/controller.rs`
- `crates/speechtt-ui/tests/widget.rs`
- `DESIGN.md` (the two existing sections named above)
- `tickets/README.md` and this ticket

Out of scope — do **not** touch:

- `crates/speechtt-core/**`, `src/stt/**` (the worker and its catalogue are unchanged)
- `crates/speechtt-ui/src/views/dialogs/help.rs`
- `design-tokens.css`, `crates/speechtt-ui/src/tokens.rs` (no new token is needed;
  the menus use existing theme roles and the component's own `rems(..)` sizing)
- `crates/speechtt-ui/src/components/setting_row.rs` (its control slot is enough)
- `crates/speechtt-ui/tests/design_system.rs` (the existing gate must pass unchanged)

`crates/speechtt-ui/src/components/mod.rs` is append-only: add the new module and
its exports without reordering or rewriting the existing lines.

## Contract

### 1. Controller: selectable option data and draft setters

Add public, GPUI-free option data to `controller.rs`:

```rust
/// One entry offered by a provider/model dropdown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectOption {
    pub id: String,
    pub label: String,
    /// `None` for provider entries; the catalogue mode for model entries.
    pub mode: Option<TranscriptionMode>,
}
```

Add these accessors (all return an owned `Vec`, empty before the catalogue
arrives / when the provider is unknown):

```rust
pub fn draft_provider_options(&self) -> Vec<SelectOption>;
pub fn draft_model_options(&self) -> Vec<SelectOption>;
pub fn settings_provider_options(&self) -> Vec<SelectOption>;
pub fn settings_model_options(&self) -> Vec<SelectOption>;
```

- Provider options preserve the catalogue order and use
  `ProviderCapability::label`.
- `draft_model_options` applies the existing `selectable_model_ids` rule: file
  mode offers only `TranscriptionMode::Segmented` models. Its `mode` is the
  catalogue mode.
- `settings_model_options` offers every model of the settings provider.
- Add explicit draft setters, mirroring the existing settings setters:

```rust
/// Returns `true` when `provider` exists in the catalogue and was applied.
/// Changing the provider clears the draft model.
pub fn set_draft_provider(&mut self, provider: &str) -> bool;

/// Returns `true` when `model` belongs to the draft provider and, in file mode,
/// is a segmented model.
pub fn set_draft_model(&mut self, model: &str) -> bool;
```

The old `cycle_provider` / `cycle_model` / `cycle_settings_*` helpers may stay
(for tests and programmatic fixtures) but **no view, app method or button may
call them any more**. The dropdown is the only user-facing path to a selection.

### 2. Component: menu items and select wrappers — `components/model_select.rs`

Two item types implement `gpui_kit::component::select::SelectItem`
(re-export of `searchable_list::SearchableListItem`):

```rust
/// One provider row. `Value` is the provider id.
#[derive(Debug, Clone)]
pub struct ProviderOption { pub id: SharedString, pub label: SharedString }

impl SelectItem for ProviderOption {
    type Value = SharedString;
    fn title(&self) -> SharedString;                 // label
    fn value(&self) -> &Self::Value;                 // id
    fn matches(&self, query: &str) -> bool;          // id OR label, case-insensitive
}

/// One model row. `Value` is the model id; `mode` draws the chip.
#[derive(Debug, Clone)]
pub struct ModelOption {
    pub id: SharedString,
    pub label: SharedString,
    pub mode: Option<TranscriptionMode>,
}

impl SelectItem for ModelOption {
    type Value = SharedString;
    fn title(&self) -> SharedString;                 // label only (accessible value)
    fn value(&self) -> &Self::Value;                 // id
    fn matches(&self, query: &str) -> bool;          // id OR label, case-insensitive
    fn render(&self, window: &mut Window, app: &mut App) -> impl IntoElement;
}
```

`render` draws the label with `components::Label` and, when `mode` is `Some`, a
`components::TranscriptionModeBadge` beside it inside an
`h_flex().items_center().gap_*`. The trigger keeps `title()` (the label alone):
the committed mode is already shown by the badge both screens render beside the
field, so a second chip inside the trigger would be the same information twice.
The badge is the existing component; no new chip style is introduced.

Two `RenderOnce` wrappers hide gpui-kit wiring from the views:

```rust
#[derive(IntoElement)]
pub struct ProviderSelect { /* id, Entity<SelectState<SearchableVec<ProviderOption>>>, placeholder, disabled */ }

impl ProviderSelect {
    pub fn new(id: impl Into<ElementId>, state: &Entity<SelectState<SearchableVec<ProviderOption>>>) -> Self;
    pub fn placeholder(self, placeholder: impl Into<SharedString>) -> Self;
    pub fn disabled(self, disabled: bool) -> Self;
}

#[derive(IntoElement)]
pub struct ModelSelect { /* same shape with ModelOption */ }

impl ModelSelect {
    pub fn new(id: impl Into<ElementId>, state: &Entity<SelectState<SearchableVec<ModelOption>>>) -> Self;
    pub fn placeholder(self, placeholder: impl Into<SharedString>) -> Self;
    pub fn disabled(self, disabled: bool) -> Self;
}
```

Their `RenderOnce` implementations:

- `ProviderSelect` → `Select::new(state).id(id).small().accessibility_label("Transcription service").placeholder(..).disabled(..).menu_width(rems(18.0))`
- `ModelSelect` → `Select::new(state).id(id).small().accessibility_label("Model").placeholder(..).disabled(..).menu_width(rems(20.0)).search_placeholder("Search models…").empty(..)`

Exact documented signatures used (official documentation only — never the
installed dependency source):

```rust
// https://docs.rs/gpui-component/latest/gpui_component/select/struct.SelectState.html
pub fn new(delegate: D, selected_index: Option<IndexPath>, window: &mut Window, cx: &mut Context<'_, Self>) -> Self
pub fn searchable(self, searchable: bool) -> Self
pub fn set_items(&mut self, items: D, window: &mut Window, cx: &mut Context<'_, Self>)
pub fn set_selected_index(&mut self, selected_index: Option<IndexPath>, window: &mut Window, cx: &mut Context<'_, Self>)
pub fn set_selected_value(&mut self, selected_value: &<D::Item as SearchableListItem>::Value, window: &mut Window, cx: &mut Context<'_, Self>)
pub fn selected_value(&self) -> Option<&<D::Item as SearchableListItem>::Value>

// https://docs.rs/gpui-component/latest/gpui_component/select/struct.Select.html
pub fn new(state: &Entity<SelectState<D>>) -> Self
pub fn id(self, id: impl Into<ElementId>) -> Self
pub fn placeholder(self, placeholder: impl Into<SharedString>) -> Self
pub fn search_placeholder(self, placeholder: impl Into<SharedString>) -> Self
pub fn menu_width(self, width: impl Into<Length>) -> Self
pub fn menu_max_h(self, max_h: impl Into<Length>) -> Self
pub fn accessibility_label(self, label: impl Into<SharedString>) -> Self
pub fn disabled(self, disabled: bool) -> Self
pub fn empty<E: IntoElement + 'static>(self, builder: impl Fn(&mut Window, &App) -> E + 'static) -> Self
// `.small()` comes from `gpui_kit::component::Sizable`.

// https://docs.rs/gpui-component/latest/gpui_component/select/enum.SelectEvent.html
pub enum SelectEvent<D> { Confirm(Option<<D::Item as SearchableListItem>::Value>) }

// https://docs.rs/gpui-component/latest/gpui_component/searchable_list/struct.SearchableVec.html
pub fn new(items: impl Into<Vec<T>>) -> Self   // T: Clone
```

Also add a reusable state holder in the same module so both windows share the
refresh logic:

```rust
/// The two select states of one provider/model pair plus the snapshot they were
/// last built from.
pub struct SelectFields { /* provider, model, last fingerprint */ }

/// The offered choices and current selection for one pair of selects.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SelectSnapshot {
    pub providers: Vec<SelectOption>,
    pub selected_provider: Option<String>,
    pub models: Vec<SelectOption>,
    pub selected_model: Option<String>,
}

impl SelectFields {
    /// Creates both states empty. Both are `SearchableVec`; only the model one
    /// enables `searchable(true)`.
    pub fn new(window: &mut Window, cx: &mut App) -> Self;

    pub fn provider(&self) -> &Entity<SelectState<SearchableVec<ProviderOption>>>;
    pub fn model(&self) -> &Entity<SelectState<SearchableVec<ModelOption>>>;

    /// Rebuilds items and selection only when `snapshot` differs from the last
    /// one applied. Returns `true` when it changed something.
    pub fn sync(&mut self, snapshot: SelectSnapshot, window: &mut Window, cx: &mut App) -> bool;
}
```

`sync` must:

1. Compare the snapshot with the last applied one; do nothing when equal.
2. `set_items(SearchableVec::new(..), window, cx)` on both states.
3. Apply the selection with `set_selected_value(&SharedString::from(id), ..)` when
   an id is still present in the new items, otherwise `set_selected_index(None, ..)`.
4. Update the stored snapshot and return `true`.

### 3. Wiring: `SpeechTtApp` (Session setup) and `DialogWindow` (Settings)

`SpeechTtApp` gains `draft_selects: SelectFields`. It is created in
`with_services_and_factory` while `window` is available, then synced from the
controller's draft. `cx.subscribe_in` on `draft_selects.provider()` and
`draft_selects.model()` routes `SelectEvent::Confirm(Some(id))` to
`set_draft_provider` / `set_draft_model` (through the app, with `cx.notify()`),
and stores the subscriptions in `_subscriptions`.

`SpeechTtApp::render` builds `SelectSnapshot` from
`draft_provider_options`, `draft_model_options`, the draft provider/model ids,
and calls `sync` before rendering the current screen. Add a small accessor so
views can borrow the states:

```rust
pub(crate) fn draft_selects(&self) -> &SelectFields;
```

`DialogWindow` gains its own `selects: SelectFields` and `_subscriptions`. In
`new` it creates the states, subscribes both to the Settings draft setters
(`app.update(cx, |app, cx| app.set_settings_provider(..))` and the model
equivalent), and observes the `SpeechTtApp` entity (`cx.observe`) so an arriving
catalogue repaints the dialog. In `render`, build the snapshot from the app's
controller (`settings_provider_options`, `settings_model_options`,
`settings_draft().provider/model`), call `self.selects.sync(..)` with the window,
then render `settings::render(self, cx)` as today.

### 4. Views

`views/session_setup.rs`:

- Replace the two `Button::new("field-provider")` / `Button::new("field-model")`
  controls with `ProviderSelect::new("field-provider", app.draft_selects().provider())`
  and `ModelSelect::new("field-model", app.draft_selects().model())`.
- Provider placeholder: `"Choose provider"` once the catalogue is loaded,
  `"Loading providers…"` before it is.
- Model placeholder: `"Choose provider first"` with no provider, otherwise
  `"Choose model"`; disabled until a provider is selected.
- Keep the `TranscriptionModeBadge` rendered beside the model select whenever
  `controller.draft_model_mode()` is `Some`.
- Remove `cycle_provider` / `cycle_model` clicks. The view imports only the
  components and controller data; no `px(`, `rems(`, `radius::` or raw styling.

`views/dialogs/settings.rs`:

- Same replacement with ids `"field-settings-provider"` / `"field-settings-model"`
  and `dialog.selects().provider()/model()`.
- Provider disabled until `catalogue_loaded`; model disabled without a provider
  (as today).
- Keep the `TranscriptionModeBadge` beside the model select.
- Keep the API-key row, reveal button, connection badge and footer unchanged.

`views/dialogs/window.rs` removes `cycle_provider` / `cycle_model` and exposes
`selects()` to `settings.rs`.

### 5. Behaviour that tests must pin

- Provider selection by id commits the draft provider and clears the model.
- Model selection by id commits the draft model; file mode rejects a streaming
  model and `draft_model_options` never lists one.
- The catalogue drives the menu contents: labels come from the handshake, not
  from hardcoded UI lists.
- Search matching is case-insensitive over both id and label.
- `sync` is idempotent: the same snapshot does not rebuild items.
- Opening a menu and confirming an entry through the real gpui-kit `Select`
  reaches the controller (headless behaviour test; no layout assertions).

## TDD Tasks

- [x] **T28.1 — Red: controller option data and draft setters.** Write failing
  unit tests in `controller.rs` for `draft_provider_options`,
  `draft_model_options` (live + file mode), `settings_provider_options`,
  `settings_model_options`, `set_draft_provider` (unknown rejected, model
  cleared) and `set_draft_model` (unknown rejected, streaming rejected in file
  mode, streaming accepted in live mode). Run them; they must fail.
- [x] **T28.2 — Green: controller.** Add `SelectOption` and the accessors/setters
  until T28.1 passes. Keep the old cycling helpers if they are still used by
  tests, but add a test asserting no view/app call site remains (or simply
  remove the app-level cycling wrappers and update the fixtures).
- [x] **T28.3 — Red: select item adapters.** Write failing tests for
  `ProviderOption` / `ModelOption`: `value()` is the id, `title()` is the label,
  `matches` accepts id and label in any case and rejects a non-match, and
  `ModelOption` retains its mode.
- [x] **T28.4 — Green: component items and wrappers.** Implement
  `components/model_select.rs` (items, `ProviderSelect`, `ModelSelect`,
  `SelectFields`, `SelectSnapshot`) and export it from `components/mod.rs`
  (append-only). Include the mode badge inside `render`/`display_title`.
- [x] **T28.5 — Red: sync and wiring behaviour.** Write failing tests:
  `SelectFields::sync` rebuilds on change and not on an identical snapshot
  (pure part testable without a window if split into a helper), and a headless
  `tests/widget.rs` test that opens `field-provider`, confirms the first entry
  and asserts the draft provider; then opens `field-model`, confirms an entry
  and asserts the draft model + `draft_model_mode`. Add the equivalent Settings
  coverage (provider select and model select against the settings draft).
- [x] **T28.6 — Green: wire both screens.** Update `app.rs`,
  `session_setup.rs`, `dialogs/window.rs` and `dialogs/settings.rs` so the tests
  pass, including subscriptions and catalogue-arrival repaint for the dialog.
- [x] **T28.7 — Refactor and documentation.** Update the `model_select.rs` module
  header (purpose, callers, which `DESIGN.md` sections it implements), all new
  public item docs, and the two `DESIGN.md` sections. Remove the old button
  wiring and any now-dead app-level cycling methods. Apply `AGENTS.md` §3: no
  change history, no ticket IDs in code docs.
- [x] **T28.8 — Gates.** Run, from the repository root, with zero warnings and
  no skipped tests:
  - `cargo fmt -- --check`
  - `cargo clippy --all-targets -- -D warnings`
  - `cargo test`
  - `bun test`
  - `bun check src/stt`
  - `bun run design:validate`

  Record the exact counts in `## Implementation Notes`.

## Acceptance Criteria

- Clicking **Transcription Service** on the Session setup screen opens a real
  dropdown listing every provider from the capability catalogue; choosing one
  updates the field, clears the model and enables the model field.
- Clicking **Model** opens a real dropdown whose rows show the model label and
  its **Stream / Batch** badge; typing in the search box filters by id and label;
  choosing an entry updates the field and leaves the mode badge visible beside it.
- In file mode the model menu lists only **Batch** models and the existing
  `InfoNote` stays.
- The Settings dialog's **Transcription Service** and **Default Model** fields
  behave the same way against the settings draft, and the dialog refreshes when
  the catalogue arrives while it is open.
- No click on any of the four fields cycles the selection; keyboard
  Up/Down/Enter/Escape behave as the gpui-kit `Select` documents.
- Views contain no styling decisions: no `px(`, `rems(`, `radius::`,
  `.text_size(`, `.font_weight(`, raw hex or ad-hoc colors. All visual choices
  live in `components/model_select.rs` and the existing badge component.
- Every gate in T28.8 is green and the full test suite passes.
- Final visual/animation check is left to the user (no autonomous screenshot or
  GUI runs).

## Implementation Notes

Delivered by the CTO after the coding sub-agent pool hit its usage limit; the
work was implemented and audited in-tree rather than delegated.

### Decisions

- **`Select` over `PopupMenu`.** The linked menu page documents
  `DropdownMenu`/`PopupMenuItem`, but only `Select` ships the search box the
  ticket requires. `Select` is the same component family (`gpui-component`) and
  the only one whose state (`SelectState`) exposes `searchable(true)`,
  `set_items`, `set_selected_value` and `SelectEvent::Confirm`.
- **State ownership.** Both pairs of `SelectState` entities live where `&mut
  Window` is available: `SelectFields` is built in
  `SpeechTtApp::with_services_and_factory` and in `DialogWindow::new`. The
  entities are never recreated after that; item and selection updates go through
  `set_items` + `set_selected_value` so the `cx.subscribe` handlers stay alive.
- **Snapshot guard.** `SelectSnapshot` (providers, selected provider, models,
  selected model) is compared with the last applied one, so a repaint that
  changes nothing does not rebuild the menus. The session-setup snapshot is only
  built while the form screen is on screen, which keeps the recording screens
  free of per-frame catalogue allocations.
- **Explicit setters, no cycling.** The four fields commit through
  `SessionController::set_draft_provider` / `set_draft_model` (session setup) and
  the existing `set_settings_provider` / `set_settings_model` (Settings). The
  app-level `cycle_provider`, `cycle_model`, `cycle_settings_provider` and
  `cycle_settings_model` wrappers were removed; the fixtures in
  `tests/widget.rs`, `tests/dialogs.rs` and `tests/end_to_end.rs` now name the
  ids explicitly. Controller-level cycling helpers remain because the pure
  controller tests exercise them, and they are no longer reachable from any view.
- **Mode badge placement.** The badge is drawn inside every model menu row
  (`ModelOption::render`) and beside the field in both screens. The trigger
  deliberately keeps the plain label (`title()`), because a badge there would
  duplicate the one next to the field.
- **Settings refresh.** `DialogWindow` observes the `SpeechTtApp` entity, so the
  open Settings dialog repaints and re-syncs its menus when the capability
  catalogue arrives.
- **File mode.** `draft_model_options` keeps the existing rule (segmented models
  only) and `set_draft_model` rejects a streaming model in file mode, so the menu
  and the committed draft cannot disagree.
- **Controller label queries.** `provider_label` / `model_label` /
  `settings_provider_label` / `settings_model_label` stay on the controller as
  catalogue-label queries; the views no longer read them now that the `Select`
  trigger renders the item title.

### Deviations

- The ticket's first draft asked `ModelOption::display_title` to draw the badge
  in the trigger; it now returns the default label-only presentation for the
  duplication reason above. The contract section was updated accordingly.

### Gates (repository root, zero warnings, no skipped tests)

| Command | Result |
|---|---|
| `cargo fmt -- --check` | clean |
| `cargo clippy --all-targets -- -D warnings` | clean |
| `cargo test` | 483 passed, 0 failed (core 219 + 3 + 14, ui 81 + 3 + 11 + 33 + 14 + 74 + 4 + 27) |
| `bun test` | 221 passed, 0 failed |
| `bun check src/stt` | no type errors in 23 files |
| `bun run design:validate` | 66 tokens, 0 errors |

### Left for the user

Final visual review (menu look, badge alignment, menu width on the 600 px
widget) — no GUI run or screenshot was taken autonomously.
