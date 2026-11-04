# T0027 — Selectable, copyable transcript text

- **Track**: Rust UI (text component + caption well + design system)
- **Owner**: coding sub-agent, supervised by CTO
- **Started**: 2026-11-03
- **Wave**: 1 (independent)
- **Depends on**: T0026 (the caption well and its scroll/follow behaviour it extends)
- **Blocks**: nothing
- **Status**: done. Selection, highlight, `Ctrl-C` and the right-click `Copy`
  are all validated on the CEO's machine.
- **References**: `AGENTS.md` §2 (component and view rules, design-system
  governance) / §3 (documentation) / §6 (hermetic tests, no layout tests),
  `DESIGN.md` §Transcript well, `crates/speechtt-ui/src/components/text.rs`,
  `crates/speechtt-ui/src/components/caption_well.rs`,
  `gpui_base::{SelectableText, TextSelectionHandle, TextSelection, TextSelectionLayer}`,
  `gpui_base::Root` (already the window root of `gpui_kit::open_window`)

## Context & Goal

The transcript is the one thing a user wants to take out of SpeechTT by hand.
Most of the time they do not want the whole take: they want one sentence, a name,
a number, a fragment they are about to quote somewhere else. Today the transcript
is painted as plain text in a card, so the mouse cannot select it and
`Ctrl-C` copies nothing.

Done means: the user can drag across the transcript of a live take or of a
finished take with the mouse, see the selected range highlighted, and copy it
with the operating system's own shortcut (`Ctrl-C` on Linux, `Cmd-C` on macOS).
Paste is the OS's job: whatever the user pasted into is outside SpeechTT, and
SpeechTT gains no new clipboard *write* path — the existing Copy action of the
result screen is untouched.

Decision taken with the CEO/Designer before this ticket:

- **Highlight colour**: `brand.primary` at 30 % opacity, painted behind the
  glyphs. No new colour is introduced — one token role with an alpha, readable
  under both the muted committed ink and the orange live tail.
- **Scope**: the transcript only. Titles, labels, badges, buttons, notes and
  dialog copy stay non-selectable, so a drag on a button remains a click.

## Current Behaviour (audit, before the change)

- `components::Text` (shared by `Title`/`Body`/`Label`) ends in
  `div().text_size(..).font_weight(..).text_color(..).child(self.text)`: a raw
  `SharedString` child, which GPUI lays out as non-interactive text.
- `CaptionWell` renders two `Label`s (committed muted, live in
  `brand.accent`) inside a `SurfaceBox` whose inner column is
  `overflow_y_scroll()` with `id(CAPTION_SCROLL_ID)` and an optional
  `ScrollHandle` passed through `.follow(..)`.
- `gpui_kit::base::Root` — the root of every window opened through
  `gpui_kit::open_window`, which both `lib.rs::run` and the headless fixtures
  use — already mounts a `TextSelectionLayer` and binds `ctrl-c` (and `cmd-c` on
  macOS) under the `Root` key context to an action that writes
  `TextSelection::selected_text(..)` to the system clipboard. So the selection
  plumbing exists in this application; nothing selects yet.
- `gpui_base::SelectableText` is the supported way to join that plumbing:
  `SelectableText::new(id, text)` retains a per-element `TextSelectionHandle`
  across frames, `.document_order(u64)` orders several runs of one document,
  and `.selection_color(Hsla)` overrides the highlight. Its text style is
  inherited from the ambient `TextStyle` of the wrapping `div` (verified in
  gpui-pre `div.rs`: children are laid out and pre-painted inside
  `window.with_text_style(style.text_style())`), so size, weight, family and
  colour remain decided in `components/text.rs` and the design-system gate stays
  satisfied.

## Scope Boundary

May be created or modified:

- `crates/speechtt-ui/src/components/text.rs` — the selectable rendering path
  and its unit tests.
- `crates/speechtt-ui/src/components/caption_well.rs` — selectable committed and
  live runs, the highlight helper, the context menu, and its unit tests.
- `crates/speechtt-ui/src/app.rs` — the `CopySelection` action and its handler,
  and the key context it is handled in.
- `crates/speechtt-ui/src/lib.rs` — the `KeyBinding` that registers
  `secondary-c`, next to the existing initialisation.
- `AGENTS.md` — §3 gains the "every file and every component says where it is
  used" rule this ticket already applies, and §6 the rule that a library is read
  through its official documentation, never its source, with an API gap reported
  to the CEO.
- `DESIGN.md` — §Transcript well: the selection highlight and its token role,
  and the context-menu entry.
- `PRODUCT.md` — one sentence in the transcript section, only if the current text
  claims the transcript is not interactive.

Do **not** touch:

- `crates/speechtt-ui/tests/**` — **no UI test is written for this ticket.** See
  the note below.
- `crates/speechtt-ui/src/views/**` — the views already pass data; no view gains a
  style or a selection decision.
- `crates/speechtt-ui/src/controller.rs`, `layout.rs`, `tokens.rs` — no new state,
  no new token.
- `crates/speechtt-core/**`, `src/stt/**` — the worker and the reducer are not
  involved.
- `design-tokens.css` — the highlight is an alpha on `--color-brand-primary`,
  not a new token.
- Dialogs, buttons, badges and every other text surface: out of scope by
  decision.

### Verification split for this ticket

Selecting with a pointer, highlighting a range, and moving that range to the
system clipboard are the operating system's own behaviour; SpeechTT only hands
the text to the native facility that already exists in Rust
(`gpui_base::SelectableText` for the runs, `gpui_base::Root` for the copy
shortcut and the clipboard write). A headless test would therefore be mocking
the very thing it pretends to prove, and a green fake says nothing about the
user's machine — mouse capture, selection painting and the clipboard all belong
to the compositor and the clipboard daemon.

So this ticket ships **unit tests only**, covering what is genuinely ours:

- the builders record the id, the order and the highlight, and leave the type
  role untouched;
- the highlight helper resolves to the approved role (brand primary, 30 %
  opacity);
- the document order constants are committed before live;
- a role that was not asked to be selectable stays non-selectable.

**Manual verification is the user's**, on their own system: drag across the
transcript, then `Ctrl-C` / `Cmd-C`, then paste elsewhere. The acceptance
criteria below describe that manual check, not an automated one.

## Contract

### 1. A selectable rendering path on the type scale

`components::Text` gains two builders, forwarded by `Title`, `Body` and `Label`
like the existing `muted`/`tone`/`mono`/`bold`/`center`:

- `.selectable(id)` where `id: impl Into<ElementId>` — when set, the role
  renders a `gpui_base::SelectableText` child with that stable element id
  instead of the raw string child.
- `.selection_order(order: u64)` — the run's `document_order`; defaults to `0`.
  It only orders runs of one document for copy, and it is ignored when the text
  is not selectable.

Invariants:

- The wrapping `div` still owns the type role: size from `TextRole::size()`,
  weight from `TextRole::weight()`/`bold_weight()`, colour from `tone`/`muted`,
  family from `mono`. **No `text_size`/`font_weight` may move out of this
  module**, and the selectable child must inherit them ambiently rather than
  restate them.
- `.selectable(id)` with an empty string still renders (an empty
  `SelectableText` contributes nothing to the document).
- A `Title`/`Body`/`Label` without `.selectable(..)` renders exactly as today:
  same element tree, same gate behaviour.
- The stable id is the caller's responsibility: it must be unique per rendered
  element and stable across frames, because `SelectableText` retains its
  selection handle in the element state keyed by that id.

### 2. The transcript is one selectable document

`CaptionWell` renders:

- the committed text as a selectable `Label` with `selection_order(0)`;
- the live tail as a selectable `Label` with `selection_order(1)`;
- both with stable element ids that do not depend on the text content
  (a changing id would drop the selection on the next partial event).

Invariants:

- A drag that starts in the committed run and ends in the live tail selects
  across the boundary; the copied string is the committed text followed by the
  live tail, in that order (that is what `document_order` buys).
- The scroll column keeps its identity and its `ScrollHandle`, so
  `.follow(..)` and `follows_bottom(..)` are unchanged.
- The selection highlight is `brand.primary` (`color::BRAND_PRIMARY`) with
  alpha `0.30`, resolved by a small public helper in
  `components/caption_well.rs` (`selection_highlight()`) and passed through
  `.selection_color(..)` there. No literal colour, no new token, and no other
  component inherits it.
- The two document orders are named constants in `components/caption_well.rs`,
  so the reading order (committed before live) is one editable fact rather than
  two magic numbers at the call site.
- Selection never moves the scroll offset by itself: a partial event that
  arrives while the reader is scrolled up must not snap the view.

### 3. The OS owns the shortcut

- `Ctrl-C` / `Cmd-C` are handled by `gpui_kit::base::Root`'s existing binding.
  SpeechTT must not bind `ctrl-c` itself, and must not intercept it in any
  view, so the action reaches the root.
- `Ctrl-V` stays entirely outside SpeechTT: the widget does not read the
  clipboard for pasted text (its existing clipboard usage is the explicit
  Copy action of the result screen and the auto-paste of a finished take).
- Nothing is highlighted when the selection is empty; `Ctrl-C` with an empty
  selection must not overwrite the clipboard with an empty string (Root
  propagates instead of writing).

### 4. Documentation is part of the deliverable

The transcript selection touches the two most-reused files of the UI layer, so
the code must explain itself without a reader having to search the call sites
(`AGENTS.md` §3, "Every file and every component says where it is used"):

- Every module header touched by this ticket **names its callers**: the screens
  and components that render it (`views::recording`, `views::session_end`,
  `layout::SurfaceBox`, …), not just what it is.
- Every public builder, component and helper documents: purpose, the states it
  supports, the default of each option, the invariants a caller can violate, and
  the design source it implements (`DESIGN.md` §Transcript well, the token role
  it reads).
- The selectable branch in `text.rs` explains **why** the type decisions stay on
  the wrapping `div` (inheritance of the ambient text style, design-system gate)
  and **why** the element id must be stable across frames (the retained
  selection handle would otherwise be recreated and the selection dropped).
- The two-run document in `caption_well.rs` explains **why** committed and live
  stay two elements and **why** their `document_order` is what makes copy read
  left to right.
- Comments describe the code as it is: no ticket id, no history, no dated
  decision (`AGENTS.md` §3).

## TDD Tasks

- [x] **T27.1 — Type-scale specs (red)**: unit tests in `components/text.rs`
  proving that `.selectable(id)` marks the role as selectable, that
  `.selection_order(n)` and `.selection_color(c)` are recorded, that the
  builders do not change the role, and that a role without `.selectable` stays
  non-selectable. Follow the shape of the existing
  `builders_do_not_change_the_role` test (fields, not geometry).
- [x] **T27.2 — Caption-well specs (red)**: unit tests in
  `components/caption_well.rs` for the two pure facts the component passes on —
  `selection_highlight()` resolves to brand primary with alpha `0.30`, and the
  order constants put the committed run before the live one (strictly ordered,
  `u64`). No window, no mouse, no clipboard: those belong to the OS.
- [x] **T27.3 — Implementation**: `Text::selectable`/`selection_order`/
  `selection_color` and the selectable branch in `text.rs`; selectable
  committed/live runs plus the highlight helper and the order constants in
  `caption_well.rs`.
- [x] **T27.4 — Docs**: `DESIGN.md` §Transcript well gains the selection rule
  (highlight role, what is selectable, what is not); `PRODUCT.md` adjusted only
  if it currently contradicts it. Present tense, no ticket id, no history
  (`AGENTS.md` §3). Plus the documentation contract §4 above: every touched
  module header names its callers, and every public item added here documents
  its role, its options, its invariants and the design source it implements.
- [x] **T27.5 — Gates**: `cargo fmt -- --check`,
  `cargo clippy --all-targets -- -D warnings` (zero warnings), `cargo test`,
  `bun test`, `bun check src/stt`, `bun run design:validate`.

## Open — the selection cannot leave the application yet

**Resolved for the shortcut.** The widget now owns the copy command
(`CopySelection`, bound under its own key context) and offers the same command
from a transcript context menu; `Ctrl-C` pastes the highlighted range on the
CEO's machine. The menu's item needed a dispatch target of its own — see
"Implementation Notes — the menu dispatch". The audit below is kept as the record
of the defect that was fixed.

First real run on the CEO's machine (2026-11-03, `bun run ui`):

- **Selection works.** Dragging across the transcript highlights the range in
  the translucent brand orange, across the committed ink and the live tail. The
  native selection layer, the run ids and the highlight are therefore correctly
  wired.
- **The copy shortcut does nothing.** `Ctrl-C` after a visible selection leaves
  the system clipboard untouched; pasting elsewhere returns the previous
  clipboard content. `Cmd-C` is untested (no macOS machine).
- **There is no fallback either.** A right-click on the selection shows no
  context menu, so the only path to the clipboard would have been the shortcut
  that does not fire.

So the defect is downstream of the selection: either the `Copy` action never
reaches `gpui_base::Root`, or it reaches it and `TextSelection::selected_text`
answers an empty string. Both are silent — Root propagates the action instead of
writing when the resolved text is empty — which is exactly why no error appears.

### How to find out which, without a UI test

Two temporary stderr probes, no transcript content, removed once the answer is
known:

1. In `app.rs`, an `on_action` for `gpui_base::input::Copy` on the widget view,
   logging whether the action arrived (`[speechtt] copy action reached the view`)
   and to which element. If the line never prints, the binding or the key
   context is the problem; if it prints, the action reaches the view but not
   Root.
2. If the action arrives, log `TextSelection::has_selection(window, cx)` and the
   **length only** of `TextSelection::selected_text(window, cx)` (never the
   text, `AGENTS.md` §9). Length zero with a visible highlight points at the
   projection, a non-zero length points at the clipboard write.

Candidate causes to check in that order, from the source of the crates:

- the `Root` key context and the `Ctrl-C` binding registered by
  `gpui_base::init` (called from `gpui_kit::init`) versus whatever
  `gpui_component` installs on the same keystroke;
- a SpeechTT-registered binding or a global shortcut listener swallowing the
  keystroke before the window sees it (`speechtt_core::hotkeys`);
- the clipboard write itself on Wayland (`cx.write_to_clipboard`), which needs a
  live owner and a focused window.

### Contract addition — copy is reachable from two places

A selection that can only be copied by one shortcut is a single point of
failure, and a desktop application is expected to offer both:

- **Shortcut**: `Ctrl-C` / `Cmd-C` must put the selected range on the system
  clipboard. This is the behaviour that must be repaired first.
- **Context menu**: a right-click inside the transcript shows a menu whose
  `Copy` entry does the same thing, enabled only while a range is selected and
  disabled (or absent) otherwise. Wording and placement follow the platform
  convention; the menu is built from the project's own menu components, not
  from ad-hoc view code.
- Both paths write the **same** string: the selected range as
  `TextSelection::selected_text` resolves it, committed text first. Neither
  path invents its own notion of "the current transcript".
- Neither path may write when the selection is empty or whitespace-only; the
  clipboard keeps its previous content in that case.

### What the official documentation says

Sources (official only, no crate source):

- `https://gpui-kit.com/base/text-selection.md` — window-level selection. The
  application retains one `TextSelectionLayer` per window, one
  `TextSelectionHandle` per participant, and **queries the selection with
  `TextSelection::has_selection(window, cx)` and
  `TextSelection::selected_text(window, cx)`**. The guide states plainly: "The
  application remains responsible for layout and painting across the participant
  seam", and its example confirms copy with "the platform copy shortcut".
  `TextSelection` exposes **no copy method** — the listed API is
  `has_selection`, `selected_text`, `select_all`, `clear`, `end`, the touch
  entry points and the drag entry points. There is no `copy`, no `Copy` action
  and no clipboard accessor on it.
- `https://gpui-kit.com/component/root.md` — `Root` is documented as owning
  "keyboard traversal and selection copying", and `gpui_kit::open_window` always
  creates it. That is the path SpeechTT relies on today, and it is the path that
  silently produces nothing on the CEO's machine.
- `https://gpui-kit.com/docs/keybinding.md` — how to bind an `Action`: `actions!`
  declares it, `cx.bind_keys([KeyBinding::new("secondary-c", …, Some(CONTEXT))])`
  registers it, `.key_context(CONTEXT)` plus `.on_action(…)` on the focused
  region routes it. `secondary-c` is the documented platform modifier (Command
  on macOS, Control elsewhere). Precedence is by *depth of the matching context*
  on the focused path, so a binding on the application's own context outranks
  Root's ancestor binding.
- `https://gpui-kit.com/llms-full.txt` (capabilities table) — `cx.write_to_clipboard(text)`
  mirrors `App::write_to_clipboard` and is the documented way to replace the
  clipboard's text. "Nothing to import."
- `https://gpui-kit.com/component/menu.md` — context menus are attached to any
  element with `ContextMenuExt::context_menu(|menu, window, cx| …)`; menu items
  carry an `Action` (`menu.menu("Copy", Box::new(Copy))`), can be disabled with
  `menu_with_disabled(label, action, true)`, and the menu displays the key
  binding of that action automatically.

**API gap to report**: `Root` advertises selection copying, yet no documented
switch, feature flag or initialisation step changes that behaviour, and it does
not copy in this window. We therefore stop depending on it and implement the
documented path ourselves — an application action that reads
`TextSelection::selected_text` and writes it with `cx.write_to_clipboard`.

### Follow-up tasks

- [x] **T27.6 — Own the copy action**: declare `CopySelection` with `actions!` in
  `app.rs`; add `SPEECHTT_CONTEXT` (`"SpeechTT"`) to the widget view's root
  element with `.on_action(…)`; register
  `KeyBinding::new("secondary-c", CopySelection, Some(SPEECHTT_CONTEXT))` in the
  existing initialisation path. The handler reads
  `TextSelection::selected_text(window, cx)` and calls
  `cx.write_to_clipboard(text)` **only** when the selection is not
  whitespace-only, so an empty selection leaves the clipboard alone. Bind at the
  application context so this outranks Root's ancestor binding (documented
  precedence), and keep Root's binding untouched.
- [x] **T27.7 — Context menu Copy**: attach
  `.context_menu(|menu, window, cx| …)` to the transcript, with a single `Copy`
  item carrying the same `CopySelection` action, disabled while
  `TextSelection::has_selection(window, cx)` is false. The item inherits the
  keybinding hint from the action, so the menu and the shortcut can never
  diverge. No new colour, spacing or elevation is introduced: the menu is the
  library's own component. If the menu's visual surface needs a design decision,
  stop and ask the CEO before implementing (`AGENTS.md` §2).
- [x] **T27.8 — Manual re-check by the CEO**: `Ctrl-C` after a drag pastes the
  highlighted range into another application, and the clipboard content matches
  what is on screen character for character. The pasted text does contain the
  repeated words the transcript itself shows — that repetition is a defect of
  the transcript buffer, tracked separately, not of the copy path: the copy
  reproduces exactly what the well renders.
- [x] **T27.9 — Final gates and notes**: the T27.5 command list again, then the
  resolution recorded in the notes below.
- [x] **T27.10 — Dispatch the menu action to a focus handle**: the menu item
  does nothing on the real machine. `PopupMenu::action_context(self, handle:
  FocusHandle) -> Self` is documented as *"Set the focus handle of Entity to
  handle actions. When the menu is dismissed or before an action is triggered,
  the focus will be returned to this handle. Then the action will be dispatched
  to this handle."*
  (<https://docs.rs/gpui-component/latest/gpui_component/menu/struct.PopupMenu.html>)
  Without it the menu has no dispatch target that carries the `SpeechTT` key
  context, so `CopySelection` never reaches `SpeechTTApp::on_copy_selection` —
  the menu closes and nothing is written. The shortcut works because the widget
  root is focused there. Set `menu.action_context(handle)` with the handle the
  window reports as focused (`Window::focused(&self, cx: &App) ->
  Option<FocusHandle>`,
  <https://docs.rs/gpui/latest/gpui/struct.Window.html>), so the action is
  dispatched exactly where focus already is — the same element that handles the
  shortcut.
- [x] **T27.11 — Manual re-check of the menu**: validated by the CEO — a
  right-click inside the transcript, then `Copy`, then a paste elsewhere
  reproduces the highlighted range.

## Acceptance Criteria

These are **manual**, performed by the CEO on their own machine against
`bun run ui`. No automated test covers them (see "Verification split"). Nothing
here is met until the copy path works: a selection the user cannot take out of
the widget is not the feature.

- Dragging across the transcript on a live take highlights the dragged range in
  a translucent brand-orange, and `Ctrl-C` puts exactly that text on the
  system clipboard; pasting into any other application yields it.
- A right-click on the selection opens a context menu whose `Copy` entry does
  the same, and that entry is disabled while nothing is selected.
- The same works on the result screen (`session_end`), including on the
  committed text that arrived while the take was running.
- A drag that spans the muted (committed) and orange (live) parts yields the
  committed text first, then the live text, with no marker or duplicated
  fragment at the join.
- `Ctrl-C` with nothing selected leaves the clipboard as it was.
- The transcript still follows the newest line at the bottom, still stops
  following when the reader scrolls up, and a new partial never resets a
  selection's scroll position.
- No other text surface in the application became selectable: a drag on a
  button, a card, a status pill or a settings row still behaves as before.
- `tests/design_system.rs` stays green: no `text_size`/`font_weight`/`px(`/
  `rems(`/`radius::` literal appears outside the allowed files, and no view file
  changes.
- The touched modules document themselves: each header names the screens and
  components that call it, and a reader can tell from the file alone what each
  selectable piece does and who depends on it.
- Every gate in T27.5 is green with zero warnings.

## Implementation Notes

### What was built

- `crates/speechtt-ui/src/components/text.rs` — `Text` gained
  `.selectable(id)`, `.selection_order(order)` (default `0`) and
  `.selection_color(color)`, all forwarded by `Title`/`Body`/`Label`. When
  `selectable` is set the role's `div` renders
  `SelectableText::new(id, text).document_order(order)[.selection_color(c)]` as
  its child instead of the raw `SharedString`; without it the element tree is
  the one that was there before. Every type decision (size, weight, family, ink)
  stays on the wrapping `div` and reaches the run through the ambient text style
  that `Div` installs around its children, so no `text_size`/`font_weight`
  moved out of the module and `tests/design_system.rs` is untouched.
- `crates/speechtt-ui/src/components/caption_well.rs` — the committed run is a
  selectable `Label` with the element id `caption-committed`, the live tail one
  with `caption-live`; `COMMITTED_ORDER = 0` / `LIVE_ORDER = 1` are named
  constants, so the reading order is one editable fact. Both pass
  `selection_highlight()` (public helper: `brand.primary` at alpha `0.30`) to
  `.selection_color(..)`. `CAPTION_SCROLL_ID`, `.follow(..)`,
  `follows_bottom(..)` and `FOLLOW_EPSILON` are unchanged.
- Module headers of both files now name their callers, and the tests carry a
  one-paragraph reason each.
- `DESIGN.md` §Transcript well — added what is selectable and what is not, the
  highlight role and opacity, and the OS shortcuts.

### TDD evidence

Red phase 1 (T27.1 — type-scale specs, before any implementation):

```
$ cargo test -p speechtt-ui --lib
error[E0609]: no field `selectable` on type `components::text::Text`
   --> crates/speechtt-ui/src/components/text.rs:264:35
error[E0433]: cannot find type `ElementId` in this scope
error[E0599]: no method named `selectable` found for struct `components::text::Label`
error[E0609]: no field `selection_order` on type `components::text::Text`
```

Red phase 2 (T27.2 — caption-well specs, after `text.rs` was implemented):

```
$ cargo test -p speechtt-ui --lib
error[E0432]: unresolved imports `super::COMMITTED_ORDER`, `super::COMMITTED_RUN_ID`,
              `super::LIVE_ORDER`, `super::LIVE_RUN_ID`, `super::SELECTION_ALPHA`,
              `super::selection_highlight`
   --> crates/speechtt-ui/src/components/caption_well.rs:102:9
```

Green: the same command reports `52 passed; 0 failed` for `speechtt-ui`.

### Tests added

`components/text.rs` (unit, fields only — no geometry, no window):

| Test | What it proves |
|---|---|
| `only_the_roles_asked_to_be_selectable_join_the_selection` | `Title`/`Body`/`Label` are non-selectable by default; `.selectable(id)` records exactly that id. |
| `the_selection_order_defaults_to_the_first_run` | `.selection_order` defaults to `0` and is recorded on the role; it is a plain builder with no required order. |
| `the_selection_builders_do_not_change_the_role` | Selecting is not a fourth role: all three roles keep their `TextRole`. |
| `the_highlight_colour_is_recorded_only_when_given` | The highlight defaults to `None` (the theme's selection role) and is recorded when passed. |

`components/caption_well.rs` (unit):

| Test | What it proves |
|---|---|
| `the_selection_highlight_is_brand_primary_at_thirty_percent` | `selection_highlight()` has the hue, saturation and lightness of `--color-brand-primary` and alpha `0.30` (the literal is pinned separately, so a different value is a visible design change). |
| `the_committed_run_comes_before_the_live_run` | The order constants are `0` and `1` and strictly ordered. |
| `the_two_runs_have_their_own_identities` | The two runs do not share an element id; sharing one would make the second run reuse the first run's selection handle. |

The existing `follows_bottom` tests are untouched and still pass.

### Decisions and deviations

- **`.selection_color(..)` on `Text`** is a third builder beyond the two named in
  Contract §1. Without it the approved highlight could only come from the theme
  (a global change, affecting every component) or a literal at the call site
  (forbidden). It is documented as a no-op on non-selectable text, and only
  `CaptionWell` passes it.
- **No `debug_selector`** was added, and no test lives under
  `crates/speechtt-ui/tests/**` — per the ticket's "Verification split", pointer
  selection, the highlight and the clipboard write are the OS's behaviour and
  are verified by the user, not headlessly.
- **`PRODUCT.md` was left untouched**: its transcript section claims an explicit
  Copy action but never says the transcript is non-interactive, so there was
  nothing to correct.
- The selection highlight is resolved per render by a pure helper rather than
  cached, so the value a test pins is exactly the value the well paints.

### Gates

```
$ cargo fmt -- --check
(no output)                                            # clean

$ cargo clippy --all-targets -- -D warnings
Finished `dev` profile … in 0.92s                      # zero warnings

$ cargo test
speechtt-core  lib          207 passed
speechtt-core  bridge_smoke   3 passed
speechtt-core  session_machine 14 passed
speechtt-ui    lib           52 passed
speechtt-ui    main           0 passed
speechtt-ui    design_system  3 passed
speechtt-ui    dialogs       11 passed
speechtt-ui    end_to_end    33 passed
speechtt-ui    error_ux      14 passed
speechtt-ui    session_flow  74 passed
speechtt-ui    theme_contract 4 passed
speechtt-ui    widget        25 passed
doc-tests                     0 passed
                              0 failed anywhere

$ bun test
221 pass, 0 fail, 796 expect() calls, 12 files

$ bun check src/stt
✓ No type errors in 23 files

$ bun run design:validate
✓ Design contract valid — 66 tokens, 0 errors.
```

### Manual verification for the user

Not covered by any automated test — these are the acceptance criteria, on your
own machine against `bun run ui`:

1. **Live take, partial range.** Start a take and let a sentence or two arrive.
   Drag across part of the transcript with the left button. Expect a translucent
   brand-orange wash behind exactly the dragged glyphs; release, press `Ctrl-C`
   (Linux/Windows) or `Cmd-C` (macOS), paste into any other application. Expect
   exactly the dragged characters, including trailing words of the line.
2. **Live take, across both inks.** While committed text is muted and the live
   tail is orange, drag from the committed text into the live tail. Expect the
   paste to read *committed text*, then a newline, then the live tail — with no
   duplicated fragment or marker at the join.
3. **Result screen.** Stop the take, then drag on the transcript of the result
   screen. Expect the same highlight and the same copy behaviour on the text that
   was committed while recording.
4. **Empty selection.** Select nothing and press `Ctrl-C` / `Cmd-C`. Expect the
   clipboard to keep its previous contents.
5. **Follow behaviour.** During a long take, scroll up: the well must stop
   following new partials and keep your scroll offset, while a selection you hold
   stays put. Scroll back to the bottom to resume following.
6. **Nothing else selects.** Drag on a footer button, a mode card, a status pill
   or a settings row. Expect each to behave as before (a click stays a click, no
   highlight appears).
7. **Visual check of the wash.** Confirm the highlight is legible over *both*
   the muted committed grey and the orange live tail and that it sits behind the
   glyphs, not over them. If the opacity needs tuning, the single fact to change
   is `SELECTION_ALPHA` in `components/caption_well.rs` (and the `0.30` in the
   `DESIGN.md` §Transcript well prose).


### First run on the real machine (2026-11-03)

The CEO ran `bun run ui` and reported:

- **Selection and highlight work.** Dragging across the transcript washes the
  range in the approved brand orange, over the committed ink and the live tail
  alike. Steps 1, 2 and 7 of the manual list above therefore pass, and the
  native selection layer, the run ids, the document order and the highlight are
  correctly wired.
- **`Ctrl-C` copies nothing.** Pasting into another application returns the
  previous clipboard content. Step 1 fails at the shortcut.
- **No context menu on right-click.** There is no secondary path to the
  clipboard, so the failure has no workaround.

Consequence for this ticket: T27.1–T27.5 are green and the selection half is
delivered, but the feature is **not** complete. The copy path is tracked in
"Open — the selection cannot leave the application yet" (T27.6–T27.9), and the
ticket stays in `in_progress/` until the CEO can paste the transcript on their
own machine.

## Implementation Notes — the copy path (T27.6 / T27.7)

### The decision

SpeechTT owns the copy action instead of relying on `gpui_kit::base::Root`.
`Root` is documented as owning "selection copying", it is already the window
root, and nothing in its documented surface changes that behaviour — yet it
writes nothing in this window. The documented path for an application that wants
a copy is explicit: read `TextSelection::selected_text(window, cx)` and write it
with `cx.write_to_clipboard(text)`. That is now what the widget does, with the
window's own key context and its own menu entry, and `Root`'s binding is left
untouched as an ancestor it outranks.

### The wiring

- `app.rs` declares `actions!(speechtt, [CopySelection])`, publishes
  `SPEECHTT_CONTEXT` (`"SpeechTT"`), keeps the pure
  `copyable_selection(&str) -> Option<&str>` helper, and handles the command in
  `SpeechTtApp::on_copy_selection`. The handler reads
  `TextSelection::selected_text(window, cx)`, and calls
  `cx.write_to_clipboard(..)` only when the helper answers `Some` — untrimmed, so
  the clipboard receives exactly the highlighted run. Nothing is logged, shown or
  recorded.
- `app.rs::render` puts `.track_focus(&self.focus)`,
  `.key_context(SPEECHTT_CONTEXT)` and `.on_action(cx.listener(..))` on the window
  view's root element.
- `lib.rs::run` binds `KeyBinding::new("secondary-c", app::CopySelection,
  Some(app::SPEECHTT_CONTEXT))` immediately after `gpui_kit::init(cx)`.
- `components/caption_well.rs` attaches `ContextMenuExt::context_menu(..)` to the
  transcript's scroll column with one item:
  `menu_with_disabled("Copy", Box::new(CopySelection), !TextSelection::has_selection(window, cx))`.
  One item only — no `Paste` (the widget has no paste path), no `Select All`.

### Documented APIs used

| API | Source |
|---|---|
| `TextSelection::has_selection` / `TextSelection::selected_text` | https://gpui-kit.com/base/text-selection.md |
| `actions!`, `cx.bind_keys`, `KeyBinding::new("secondary-c", ..)`, `.key_context(..)` + `.on_action(..)`, context-depth precedence | https://gpui-kit.com/docs/keybinding.md |
| `on_action` handler signature and dispatch/propagation | https://gpui-kit.com/docs/action.md |
| `ContextMenuExt::context_menu`, `menu_with_disabled(label, action, disabled)`, keybinding hint resolved from the action | https://gpui-kit.com/component/menu.md |
| `cx.write_to_clipboard(text)` (`ClipboardItem` argument) | https://gpui-kit.com/llms-full.txt (clipboard section) |

No dependency source was read for this work; the only non-documentary input was
the rustc type error that named `ClipboardItem` as the parameter of
`cx.write_to_clipboard`, which is why the call converts with `.into()`.

### TDD evidence — red phase

The helper's unit tests were written first, against a name that did not exist:

```
$ cargo test -p speechtt-ui --lib
error[E0432]: unresolved import `super::copyable_selection`
  --> crates/speechtt-ui/src/app.rs:1463:9
   |
   |     use super::copyable_selection;
   |         ^^^^^^---------------^^^^^
   |         |
   |         no `copyable_selection` in `app`
error: could not compile `speechtt-ui` (lib test) due to 1 previous error
```

Green after the helper, the action and the handler existed: `56 passed`.

### Tests added (`app.rs`, unit, no window)

| Test | What it proves |
|---|---|
| `an_empty_selection_leaves_the_clipboard_alone` | `""` resolves to `None`, so a stray shortcut never empties the clipboard. |
| `a_blank_selection_leaves_the_clipboard_alone` | A space, a tab, a newline and a mixed whitespace run all resolve to `None`. |
| `a_selected_sentence_is_copied_exactly_as_highlighted` | A real sentence comes back verbatim with its leading and trailing spaces — the copy is the highlight. |
| `punctuation_alone_is_a_real_selection` | Whitespace-only is the line: punctuation is a selection and is kept. |

Nothing was added under `crates/speechtt-ui/tests/**`, and no test renders the
menu, simulates a keystroke or touches the clipboard.

### Deviations and notes

- **`track_focus` + an initial `focus()` on the widget root.** The keybinding
  guide keeps the focus handle, the key context and the handler together in the
  owning entity, and says a binding does not focus the element itself: "Focus it
  when entering the region, or let pointer interaction focus its tracked
  handle". Without a keyboard target on the dispatch path the context matches
  nothing, which is one way a bound shortcut silently does nothing. The root now
  tracks a `FocusHandle` and takes focus when the window opens, so the same drag
  that creates a selection also leaves the root focused.
- **`actions!` cannot carry a doc comment.** Rustdoc warns
  `unused_doc_comments` on a macro invocation, so the command is documented in a
  leading comment plus the binding site and the handler's doc.
- **`cx.write_to_clipboard` takes a `ClipboardItem`**, not a `&str`; the handler
  passes `text.to_owned().into()`.
- **`context_menu` changes the element type** (`ContextMenu<Stateful<Div>>`), so
  the scroll column is built first and the menu is attached to it as the last
  step before it enters `SurfaceBox`.
- **No design decision was taken for the menu**: it is the library's own
  component, themed by the theme, and `DESIGN.md` §Transcript well says so.

### Global shortcut listener — checked, reported

`speechtt_core::hotkeys::default_bindings` registers seven OS-level grabs, all of
them `Ctrl+Shift+<key>` (`R`, `P`, `O`, `S`, `N`, `H`, `,`), so the default
configuration does **not** swallow `Ctrl-C` / `Cmd-C`. The risk is user data: a
`shortcuts` entry in the settings file naming `Ctrl+C` would be registered with
the `global-hotkey` backend, and an OS grab takes the keystroke before the window
sees it. Nothing in this ticket guards against that; if it is worth guarding, it
is a separate ticket (either reject `Ctrl+C`/`Cmd+V` in shortcut validation, or
surface the conflict).

### Gates

```
$ cargo fmt -- --check
(no output)                                            # clean

$ cargo clippy --all-targets -- -D warnings
Finished `dev` profile … in 1.26s                      # zero warnings

$ cargo test
speechtt-core  lib             207 passed
speechtt-core  bridge_smoke       3 passed
speechtt-core  session_machine   14 passed
speechtt-ui    lib               56 passed   (was 52: +4 helper tests)
speechtt-ui    main               0 passed
speechtt-ui    design_system      3 passed
speechtt-ui    dialogs           11 passed
speechtt-ui    end_to_end        33 passed
speechtt-ui    error_ux          14 passed
speechtt-ui    session_flow      74 passed
speechtt-ui    theme_contract     4 passed
speechtt-ui    widget            25 passed
doc-tests                         0 passed
                                  0 failed anywhere

$ bun test
221 pass, 0 fail, 796 expect() calls, 12 files

$ bun check src/stt
✓ No type errors in 23 files

$ bun run design:validate
✓ Design contract valid — 66 tokens, 0 errors.
```

### What is left for the user

Nothing on this ticket. The CEO validated both routes on the real machine: a drag
then `Ctrl-C` copies the highlighted range, and a right-click then `Copy` does
the same.

The repetition visible in the pasted text is not a copy defect: the well renders
those words, so the clipboard receives them. It belongs to the transcript buffer
and is tracked by T0026.

## Implementation Notes — the menu dispatch (T27.10)

### The documented cause

`PopupMenu::action_context` is documented as:

> "Set the focus handle of Entity to handle actions. When the menu is dismissed
> or before an action is triggered, the focus will be returned to this handle.
> Then the action will be dispatched to this handle. Submenus without their own
> action context use the nearest parent's explicit context for actions and
> shortcut hints."
> — <https://docs.rs/gpui-component/latest/gpui_component/menu/struct.PopupMenu.html>

The documented signature is `pub fn action_context(self, handle: FocusHandle) ->
Self`, confirmed on that page (gpui-component 0.9.0). The handle comes from:

> `pub fn focused(&self, cx: &App) -> Option<FocusHandle>` — "Obtain the
> currently focused FocusHandle. If no elements are focused, returns None."
> — <https://docs.rs/gpui/latest/gpui/struct.Window.html> (the linked
> `gpui-pre 0.3.11` page was checked for the same signature)

Without `action_context` the menu owned no dispatch target, so `CopySelection`
was dispatched into the menu's own context, where nothing handles it: the menu
dismissed and the clipboard was never written. The shortcut worked because the
widget root (`track_focus` + `.key_context("SpeechTT")` + `.on_action(..)`) is
the focused element at that moment. Passing `window.focused(cx)` therefore
reaches the same element as the shortcut **by construction**, not by a second
pairing of action and handler.

### What changed

`crates/speechtt-ui/src/components/caption_well.rs` only:

```rust
.child(column.context_menu(|menu, window, cx| {
    let target = window.focused(cx);
    let disabled =
        copy_item_disabled(TextSelection::has_selection(window, cx), target.is_some());
    let menu = match target {
        Some(handle) => menu.action_context(handle),
        None => menu,
    };

    menu.menu_with_disabled("Copy", Box::new(CopySelection), disabled)
}))
```

- `menu.action_context(handle)` is set **before** the item is added, and the item
  is the same single `Copy` entry carrying the same `CopySelection`.
- The predicate is one private helper, `copy_item_disabled(has_selection,
  has_target) -> bool`, so the invariant reads as one sentence in code and in a
  test instead of as a compound boolean at the call site: the item is disabled
  when there is no selection **or** no dispatch target. An item that cannot be
  dispatched is never offered as usable.
- Module header and `CaptionWell`'s `# Copying the selection` now state that the
  menu dispatches to the window's focused handle (which is why the item and the
  shortcut share one handler) and that the item is disabled on either missing
  half. Present tense, no ticket id, no history.
- Nothing else moved: no action, no handler, no keybinding, no selection
  rendering, no view, no token, nothing under `crates/speechtt-ui/tests/**`.

### `DESIGN.md` — deliberately unchanged

`DESIGN.md` §Transcript well says the menu's `Copy` entry is "disabled while
nothing is selected". That stays true. The added case — no focused handle, hence
no dispatch target — is not something the user can observe: the right-click that
opens the menu focuses the widget root. So no sentence became false, and the
mechanism is not restated in a document that describes what the user sees.

### Tests added (`components/caption_well.rs`, unit, no window, no menu)

| Test | What it proves |
|---|---|
| `the_copy_entry_needs_a_selection_and_a_dispatch_target` | `copy_item_disabled` is `false` only when both halves are present; each missing half (and both) disables the entry. |

Red phase, produced by removing the helper while the test stayed in place:

```
$ cargo test -p speechtt-ui --lib
error[E0432]: unresolved import `super::copy_item_disabled`
   --> crates/speechtt-ui/src/components/caption_well.rs:225:26
    |
225 |         SELECTION_ALPHA, copy_item_disabled, follows_bottom, selection_highlight,
    |                          ^^^^^^^^^^^^^^^^^^ no `copy_item_disabled` in `components::caption_well`
error: could not compile `speechtt-ui` (lib test) due to 1 previous error
```

Green: `57 passed` (was 56, +1). No test renders the menu, simulates a click or
touches the clipboard — that behaviour is the library's and T27.11 verifies it by
hand.

### Gates

```
$ cargo fmt -- --check
(no output)                                            # clean

$ cargo clippy --all-targets -- -D warnings
Finished `dev` profile … in 0.89s                      # zero warnings

$ cargo test
speechtt-core  lib             207 passed
speechtt-core  bridge_smoke      3 passed
speechtt-core  session_machine  14 passed
speechtt-ui    lib              57 passed   (was 56: +1 dispatch-target predicate)
speechtt-ui    main              0 passed
speechtt-ui    design_system     3 passed
speechtt-ui    dialogs          11 passed
speechtt-ui    end_to_end       33 passed
speechtt-ui    error_ux         14 passed
speechtt-ui    session_flow     74 passed
speechtt-ui    theme_contract    4 passed
speechtt-ui    widget           25 passed
doc-tests                        0 passed
                                 0 failed anywhere

$ bun test
221 pass, 0 fail, 796 expect() calls, 12 files

$ bun check src/stt
✓ No type errors in 23 files

$ bun run design:validate
✓ Design contract valid — 66 tokens, 0 errors.
```

The working tree also carries the unfinished T0026 work (`crates/speechtt-core/**`,
`src/stt/**`, `views/recording.rs`, `layout.rs`, `components/mod.rs`, `tests/*`,
and the scroll half of `caption_well.rs`); it was left untouched and is included
in the counts above because the gates run over the tree.

### Left for the user

T27.11 stays open: right-click inside the transcript, activate `Copy`, paste
elsewhere. The entry must be disabled while nothing is selected and must show the
shortcut hint.
