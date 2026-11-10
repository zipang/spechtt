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
- `crates/speechtt-ui/tests/shortcuts.rs` — **modified**: the Settings key
  test loses its scroll step. It reached the API-key field by scrolling the
  provider row into view because that field was last; with the new order it is
  the second row and needs no scrolling, and the scroll target no longer exists
  while the provider row is hidden.
- `crates/speechtt-ui/tests/widget.rs` — **modified**: the Settings menu test
  clicked inside `field-settings-provider`, which does not exist under the
  one-provider fixture. It installs a second provider before opening Settings so
  the row exists for the reason it exists; every assertion it makes is
  unchanged.
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

The view chooses no spacing value itself: the row rhythm comes from `dense()`.
A control that stacks a field over its hint sets that hint's gap itself, inside
the control it passes to the row; that is control-internal layout, not row
rhythm, and it is the same relative step `views::dialogs::help` uses.

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
  catalogue arrives while the draft is open.
- With no catalogue it pins [`DEFAULT_PROVIDER`] into an empty draft.
- With exactly one catalogue provider it pins that provider, **replacing a draft
  provider the catalogue does not list**. [`DEFAULT_PROVIDER`] is a guess made
  before any catalogue exists; when the arrived catalogue names a different
  single provider, keeping the guess leaves the row hidden, the provider
  unknown and `Save changes` permanently disabled. No user choice is at risk:
  the row is only ever drawn when a choice exists, and then this function
  returns before touching the draft.
- With a choice (two or more providers) it does nothing, so a chosen provider
  is never overwritten.
- A loaded catalogue that lists **no** provider pins nothing: there is no
  provider to guess, and a stale draft is left alone.

### §3b — An empty catalogue says so

A worker that reports a catalogue with zero providers leaves no provider to
pin, no row to show and no way to save. `settings_backend_note` covers that state
with one sentence so the dialog explains itself instead of silently disabling
`Save changes`:

```text
The transcription backend reported no providers.
```

The note is rendered by the existing `InfoNote` slot, so no view code changes.
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

- [x] **T42.1 — Component specs first**: red tests for `SettingRow`'s new
      builders — `dense()` renders the row (the element id still resolves),
      `stacked()` renders the control, `accessory()` renders its element, and the
      default/`prominent` row is unchanged. Element-presence assertions only;
      no geometry (AGENTS.md §6).
- [x] **T42.2 — Component implementation**: the three builders per §1.
- [x] **T42.3 — Controller specs first**: red tests for
      `settings_offers_provider_choice` (false with no catalogue, false with one
      provider, true with two) and `pin_single_provider` (pins the catalogue's
      single provider, pins `DEFAULT_PROVIDER` with no catalogue, never
      overwrites a set provider, does nothing when a choice exists), plus the
      end-to-end case: a fresh install with no catalogue can save `Bun path` and
      a key.
- [x] **T42.4 — Controller implementation**: §3.
- [x] **T42.5 — View specs first**: red tests that the Settings dialog renders
      `Bun path` and the API key field before the language row, that
      `Transcription Service` is absent with one provider and present with two,
      and that the API-key field is still reachable. Ordering assertions compare
      the document order of element ids, never coordinates.
- [x] **T42.6 — View implementation**: §2 and §4 — the gap-free row list, the
      new order, the provider condition, the stacked API-key row.
- [x] **T42.7 — Height**: §5, tuned by building and looking; record the value.
- [x] **T42.8 — Docs**: `DESIGN.md` per §6, `tickets/README.md` board row.
- [x] **T42.9 — Gates**: `cargo fmt -- --check`,
      `cargo clippy --all-targets -- -D warnings`, `cargo test`, `bun test`,
      `bun check src/stt`, `bun run design:validate`. Zero warnings; nothing
      skipped or `#[ignore]`d.
- [x] **T42.10 — Visual check**: build the AppImage and open Settings; confirm
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
9. A catalogue that names a single provider different from `DEFAULT_PROVIDER`
   leaves a savable dialog: the draft ends up on the provider the catalogue
   lists, not on the guess.
10. A catalogue with zero providers shows the note from §3b instead of a
    silently disabled `Save changes`.
11. T42.10 records the visual verification: the window size, and that the dialog
    fits.

## Implementation Notes

### Chosen height

`SETTINGS_DIALOG_HEIGHT = 660.` The layout was measured headlessly at 700 and at
660: at 660 the footer button starts at y=618, leaving the body viewport about
24px below the last row, so the six visible rows and the footer fit at once. The
stored-key hint and the conditional notes keep scrolling inside the body. The
value is not visually confirmed; see T42.10.

### Row order as implemented

1. Bun path (dense, trailing `Input`)
2. API key (dense, stacked: badge accessory on the label line, masked input plus
   stored-key label below)
3. Transcription Service — only when `settings_offers_provider_choice()` is true
4. Dictation Language
5. Default Streaming Model
6. Default Batch Model
7. Cleanup Model

All rows are passed to `DialogShell` as one child of a gap-free `v_flex()`; the
`InfoNote`s stay separate children and keep the shell gap. Every row uses
`.prominent().dense()`; `session_setup` still uses `.prominent()` alone.

### The two dead ends

**A stale guess could never be corrected.** `DEFAULT_PROVIDER` is a guess made
before any catalogue exists. When the first catalogue named a *different* single
provider, the draft kept the guess, `settings_provider_known()` stayed false, the
provider row was hidden, and `Save changes` could never enable. `pin_single_provider`
now ranks an arrived catalogue above the guess.

**An empty catalogue disabled Save silently.** It leaves no provider to pin, no
row to show and nothing to save, with no explanation on screen. `settings_backend_note`
now returns `"The transcription backend reported no providers."` while the loaded
catalogue holds zero providers, alongside the existing backend-failure cases,
which keep their precedence.

### `pin_single_provider` in its four states

| State | Behaviour |
| ----- | --------- |
| No catalogue | Pins `DEFAULT_PROVIDER` into a draft that has no provider; an existing provider is left alone. |
| Exactly one catalogue provider | Pins that id, replacing a draft provider the catalogue does not list, and clears `model_streaming` / `model_batch` when the id actually changes. |
| Two or more providers | Returns untouched: the row is on screen, so the choice is the user's. |
| Catalogue loaded, no providers | Pins nothing and leaves the draft alone: there is no provider to guess. |

Replacing a draft provider risks no user choice, because the only branch that
replaces it is the one where no choice is drawn. `pin_single_provider` runs at the
end of `open_settings_with` and `open_settings_from_stored`, and from
`apply_capabilities` whenever the Settings dialog is open.

### Copy and control states

- The API-key description falls back to `"The credential your provider reads"`
  when no environment variable is known. The previous fallback told the user to
  select a provider, which the dialog does not offer in the pinned state; the
  env-var name still shows whenever `settings_api_key_env()` resolves.
- Both default-model menus are disabled while `!has_provider || !catalogue_loaded`.
  A pinned provider makes `has_provider` true before any catalogue arrives, so the
  old guard left two enabled menus with nothing in them.
- `settings_provider_placeholder` returns `"Choose provider"`. Its only caller
  draws the row when two or more providers exist, where the catalogue is always
  loaded, so `"Loading providers…"` and `"Backend unavailable"` were unreachable.
  The tests that asserted those two strings were removed with them; the
  `InfoNote` cases that explain a missing backend are unchanged and still pinned.

### Tests added

Component and view (`crates/speechtt-ui/tests/dialogs.rs`):

- `pin_single_provider_replaces_the_guess_the_catalogue_does_not_list` — a draft
  on `DEFAULT_PROVIDER` ends on the catalogue's single provider.
- `pin_single_provider_clears_the_models_of_a_provider_it_replaces` — the two mode
  defaults are cleared when the provider changes.
- `pin_single_provider_leaves_the_draft_alone_without_a_usable_single_provider` —
  two providers and an empty catalogue both leave the draft untouched.
- `an_empty_catalogue_says_it_reported_no_providers` — the §3b note.
- `the_model_menus_stay_closed_before_a_catalogue_arrives` — clicking a model menu
  with no catalogue leaves it closed.
- `settings_leads_with_the_runtime_and_the_api_key` — the full six-id document
  order, including `field-settings-cleanup`, which the handshake in the test
  supplies so the row draws its menu rather than the `Label` fallback.
- `settings_shows_the_provider_row_when_two_providers_exist` — the provider row's
  document position between the API-key field and the language select.
- `settings_hides_the_provider_row_for_a_single_catalogue_provider` — an explicit
  one-provider catalogue hides the row and pins the draft.
- `setting_row_builders_render_their_content` — the default and `prominent` rows
  each render their title line (through a keyed accessory, the only keyed element
  the line can hold) and their control; the title copy is anonymous rendered text
  and is not enumerable.

Controller (`crates/speechtt-ui/src/controller.rs`):

- `a_catalogue_without_providers_says_so`.

### Deviations and notes

- `SettingRow::mono_description()` keeps the API-key description in the mono face;
  the row would otherwise have lost it. The face is not observable in a rendered
  frame, so a component unit test pins the flag the builder sets and the render
  test pins that the row still draws.
- The API-key field's hint stack keeps its own `gap_1()` inside the control the
  view passes. That is inter-child layout of the control, not the row rhythm
  `dense()` owns, and `views::dialogs::help` uses the same step.
- `crates/speechtt-ui/tests/shortcuts.rs` drops the scroll inside
  `field-settings-provider`: the API key is the second row, so the test clicks it
  where it is drawn, with every assertion intact.
- `crates/speechtt-ui/tests/widget.rs` installs a second provider before opening
  Settings, because its menu test clicked a row that only exists with a choice.
- Board: `tickets/README.md` carries the ticket's row in the Review table, moved
  there when the ticket was staged; the row's title, track and dependency are
  unchanged. An earlier draft of these notes described that edit as pending.
- T42.10: `bun run package --no-bump` built
  `dist/SpeechTT-0.5.1-x86_64.AppImage`, version 0.5.1. The Settings window was
  not opened on a display; the fit was measured headlessly. **Not visually
  verified, requires the CEO.**

### Gate output (final run)

- `cargo fmt -- --check` — clean (exit 0).
- `cargo clippy --all-targets -- -D warnings` — `Finished dev profile`, no
  warnings.
- `cargo test` — all 20 suites `ok`, `0 failed`: core lib 237, ui lib 154,
  dialogs 31, shortcuts 23, widget 36, and the remaining suites
  5/11/14/5/11/0/3/37/14/5/96/5/4/0/0; nothing skipped or `#[ignore]`d.
- `bun test` — `515 pass, 0 fail` across 18 files.
- `bun check src/stt` — `No type errors in 32 files`.
- `bun run design:validate` — `Design contract valid — 66 tokens, 0 errors.`

### Closure

Closed by the CEO on 2026-11-11 without a visual sign-off. The behaviour this
ticket introduced is kept — the dense row rhythm, the `SettingRow` builders, the
provider pinning and the conditional row — but the tests that pinned the row
composition were **removed**, not adjusted.

The CEO's rule, now in `AGENTS.md` §6: an interface is reworked continuously —
rows reorder, controls appear and disappear, a badge is dropped — so a test that
pins *which* elements a screen draws or *in what order* has to be deleted the
next time the layout improves. Only functions, state transitions and wiring are
covered. The nine composition tests this ticket added are gone; the decisions
they were guarding that were really functional moved into the controller:

- `settings_provider_unavailable()` holds the predicate behind the
  unavailable-provider note, and is unit-tested in three states (no catalogue,
  reported unavailable, reported available). The view asks it instead of
  recomputing it.
- `settings_offers_provider_choice()` and `pin_single_provider()` keep their
  tests, which are controller rules.
