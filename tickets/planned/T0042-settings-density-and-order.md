# T0042 — Settings fits, leads with what matters, and drops the empty choice

- **Track**: Rust UI (dialog view, `SettingRow` component) + design system
- **Wave**: 2 (integration — touches a shared component, a controller rule and `DESIGN.md`)
- **Depends on**: T0040 (the `Bun path` row and the recovery state this ticket must keep working), T0041 (the dialog height and row list this ticket changes)
- **Blocks**: nothing
- **References**:
    - `AGENTS.md` §2 (component rule — spacing decisions live in `components/`, never in a view), §3 (documentation in code), §5 (quality gates), §6 (Do/Don't — no layout tests, no ad-hoc styles)
    - `DESIGN.md` §"Settings panel" — the row list, order and rhythm this ticket changes
    - `crates/speechtt-ui/src/components/setting_row.rs` — `SettingRow`, `prominent`
    - `crates/speechtt-ui/src/views/dialogs/settings.rs` — the composed rows
    - `crates/speechtt-ui/src/views/dialogs/mod.rs` — `DialogShell` (the body gap) and `SETTINGS_DIALOG_HEIGHT`
    - `crates/speechtt-ui/src/controller.rs` — `settings_draft`, `open_settings_with`, `settings_provider_options`, `settings_can_save`
    - `speechtt_core::settings::DEFAULT_PROVIDER` — the fallback provider id

## Context & Goal

The Settings dialog no longer fits and no longer leads with the fields a user
comes there to set. Three separate problems, all visible in the screenshot that
prompted this ticket:

**1. The rhythm is lopsided.** Each row carries `py_3()` and the shell puts an
8px gap between them, so a divider has 12px below it and 20px above the next
title. The eye reads the top of each row as heavier than its bottom, and six rows
of that make the panel taller than it needs to be.

**2. The important fields are last.** `Bun path` and the API key are the two
values that make the app work at all — the runtime the worker needs and the
credential the provider needs. They sit at the bottom of the list, below four
selections that only matter once those two are right.

**3. The provider menu asks a question with one answer.** The catalogue offers a
single provider, so `Transcription Service` renders a menu whose only entry is
already selected. It occupies a row to say nothing.

Done means: the dialog fits its window without scrolling, the two load-bearing
fields are first, the row rhythm is symmetric and tighter, and the provider row
is absent until a second provider actually exists — without making a fresh
install unsavable.

### The trap this ticket must not spring

`settings_draft.provider` is only ever set through `set_settings_provider`,
which requires the provider to be in the loaded catalogue. Hiding the row while
leaving that rule alone means a fresh install has `provider: None` forever, the
model menus stay disabled, and `settings_can_save` can never be true — the exact
dead end T0040 removed, reintroduced by a layout change. Contract §3 closes it.

## Scope Boundary

May create or modify:

- `crates/speechtt-ui/src/components/setting_row.rs` — **modified**: the
  `.dense()`, `.stacked()` and `.accessory()` builders per Contract §1.
- `crates/speechtt-ui/src/views/dialogs/settings.rs` — **modified**: the row
  list, its order, and the provider row's condition.
- `crates/speechtt-ui/src/views/dialogs/mod.rs` — **modified**:
  `SETTINGS_DIALOG_HEIGHT`.
- `crates/speechtt-ui/src/controller.rs` — **modified**: the provider pinning of
  Contract §3 and the predicate the view asks.
- `crates/speechtt-ui/tests/dialogs.rs` — **modified**: row presence and
  ordering assertions, plus the fresh-install save case.
- `DESIGN.md` — the Settings panel section: row list, order, rhythm, hidden row.
- `tickets/README.md` — board row.

Out of scope — do **not** touch anything else, in particular:

- No change to any other screen. `views::session_setup` also uses
  `SettingRow::prominent`; it keeps its current rhythm, which is why the new
  builders are opt-in rather than a change to `prominent`.
- No new colour, text size, weight or radius. The only visual values that move
  are vertical padding and the row order; both are documented in `DESIGN.md`.
- No change to the worker, the protocol, the provider catalogue, the settings
  file format or the packaging script.
- No scrolling behaviour change and no layout test: the window height is tuned
  by hand and verified by eye by the CEO (AGENTS.md §6).

## Contract

### §1 — `SettingRow` gains an opt-in dense, stacked form

```rust
/// Tighter, symmetric vertical rhythm for a dialog that must fit its window.
///
/// The row keeps `prominent`'s typography and hairline; only its vertical
/// padding shrinks and becomes equal above and below the content, so a divider
/// sits centred between two rows instead of closer to the lower one. Off unless
/// called: `views::session_setup` keeps the roomier default.
pub fn dense(mut self) -> Self;

/// Puts the control on its own line, spanning the row, below the label.
///
/// For a control that needs the full width — a text field, not a dropdown —
/// where a trailing slot would squeeze the label to nothing.
pub fn stacked(mut self) -> Self;

/// Adds an element to the label line's trailing edge, opposite the title.
///
/// Independent of the control: a stacked row can carry a status badge beside
/// its title while its control spans the row below.
pub fn accessory(mut self, accessory: impl IntoElement) -> Self;
```

- `dense()` sets the row's vertical padding to one step tighter than
  `prominent` and applies it symmetrically; the hairline stays.
- `stacked()` renders the control in a full-width child below the label block
  instead of the trailing `h_flex` slot.
- `accessory()` renders its element at the right of the label line, aligned with
  the title.
- The three are composable and independent; the default row, and `prominent`,
  keep their current behaviour byte for byte.

### §2 — The body stacks the rows without a gap

The dialog body currently gives every child an 8px gap, and the rows add their
own padding on top of it. `views::dialogs::settings` therefore passes its rows as
**one** child of a `v_flex()` with no gap, so each divider has equal space above
and below it. The notes (`InfoNote`) stay separate children and keep the shell's
gap, because they are not rows.

The view chooses no spacing value itself: the rhythm comes from `dense()`.

### §3 — The provider row is absent while there is nothing to choose

The view asks the controller one question, and renders the
`Transcription Service` row only when the answer is yes:

```rust
/// Whether Settings offers a provider choice.
///
/// False while the catalogue holds at most one provider: a menu with a single
/// entry only restates what the app already decided. Hiding the row must not
/// change the draft, so the same rule pins it (see [`Self::pin_single_provider`]).
pub fn settings_offers_provider_choice(&self) -> bool;

/// Pins the draft's provider when the dialog offers no choice.
///
/// The catalogue's only provider when one has arrived, [`DEFAULT_PROVIDER`]
/// otherwise, so a fresh install still has a provider to save and the T0040
/// recovery state keeps a complete draft. A draft that already has a provider
/// is never overwritten.
pub fn pin_single_provider(&mut self);
```

- `settings_offers_provider_choice` returns `settings_provider_options().len() > 1`.
- `pin_single_provider` runs wherever the draft is created and whenever a
  catalogue arrives while the draft is open, and is a no-op once a provider is
  set or when a choice exists.
- With the row hidden, `settings_can_save` still succeeds on a fresh install: the
  draft carries the pinned provider, the key the user typed, and (after the
  catalogue arrives) the two mode defaults.

### §4 — Row order

The rows read, top to bottom:

1. **Bun path** — the runtime the worker needs.
2. **API key** — the credential, as a stacked row: badge on the label line,
   masked field spanning the row below.
3. **Dictation Language**
4. **Default Streaming Model**
5. **Default Batch Model**
6. **Cleanup Model**

`Transcription Service` appears between 2 and 3 only when §3 says there is a
choice. The remaining rows keep their current relative order.

### §5 — The window height

`SETTINGS_DIALOG_HEIGHT` shrinks to the value that lets the six visible rows and
the footer fit without scrolling. The exact number is chosen by building and
looking, and recorded in `## Implementation Notes`; the contract requires only
that the default dialog shows every visible row and its footer at once.

### §6 — `DESIGN.md`

The Settings panel section is rewritten to match: the row list and its order, the
dense symmetric rhythm with the note that a divider is centred between two rows,
the stacked API-key row, and the rule that the provider row appears only when the
catalogue offers more than one provider. "Seven rows" becomes the ordered list
above, described as conditional at one position.

## TDD Tasks

- [ ] **T42.1 — Component specs first**: red tests for `SettingRow`'s new
      builders — `dense()` renders the row (the element id still resolves),
      `stacked()` renders the control, `accessory()` renders its element, and the
      default/`prominent` row is unchanged. Element-presence assertions only;
      no geometry (AGENTS.md §6).
- [ ] **T42.2 — Component implementation**: the three builders per §1.
- [ ] **T42.3 — Controller specs first**: red tests for
      `settings_offers_provider_choice` (false with no catalogue, false with one
      provider, true with two) and `pin_single_provider` (pins the catalogue's
      single provider, pins `DEFAULT_PROVIDER` with no catalogue, never
      overwrites a set provider, does nothing when a choice exists), plus the
      end-to-end case: a fresh install with no catalogue can save `Bun path` and
      a key.
- [ ] **T42.4 — Controller implementation**: §3.
- [ ] **T42.5 — View specs first**: red tests that the Settings dialog renders
      `Bun path` and the API key field before the language row, that
      `Transcription Service` is absent with one provider and present with two,
      and that the API-key field is still reachable. Ordering assertions compare
      the document order of element ids, never coordinates.
- [ ] **T42.6 — View implementation**: §2 and §4 — the gap-free row list, the
      new order, the provider condition, the stacked API-key row.
- [ ] **T42.7 — Height**: §5, tuned by building and looking; record the value.
- [ ] **T42.8 — Docs**: `DESIGN.md` per §6, `tickets/README.md` board row.
- [ ] **T42.9 — Gates**: `cargo fmt -- --check`,
      `cargo clippy --all-targets -- -D warnings`, `cargo test`, `bun test`,
      `bun check src/stt`, `bun run design:validate`. Zero warnings; nothing
      skipped or `#[ignore]`d.
- [ ] **T42.10 — Visual check**: build the AppImage and open Settings; confirm
      that every visible row and the footer fit without scrolling, that the
      dividers look evenly spaced, and that the two leading rows are the ones
      named above. Record the window size and what was observed.

## Acceptance Criteria

1. Every visible row and the footer fit in the default Settings window without
   scrolling, and the vertical space above and below each divider is equal.
2. `Bun path` and the API key are the first two rows; the language, the two
   defaults and the cleanup model follow in that order.
3. The `Transcription Service` row is absent while the catalogue offers at most
   one provider, and appears as soon as two exist — with no code change needed
   to switch.
4. A fresh install with no catalogue can still be saved: `Bun path` plus a key
   is enough, and the saved settings carry the single provider.
5. `views::session_setup` renders exactly as before: the new builders are
   opt-in, and `prominent` is untouched.
6. No new colour, text size, weight or radius; `bun run design:validate` and the
   design-system gate stay green.
7. `DESIGN.md` documents the order, the rhythm and the conditional row.
8. Every gate in T42.9 is green with zero warnings, and no test asserts layout,
   position, size or geometry.
9. T42.10 records the visual verification: the window size, and that the dialog
   fits.

## Implementation Notes

_(added during work: the chosen height, the decisions taken, any deviation from
the contract with its reason, the exact gate output, and the T42.10 result.)_
