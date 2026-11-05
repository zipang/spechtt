# T0033 — One cleanup button that toggles between the repaired and original text

- **Track**: Rust UI (controller + view) + design system
- **Wave**: 1 (independent)
- **Depends on**: T0030 (the cleanup pass this replaces the controls of)
- **Blocks**: nothing
- **Owner**: CTO
- **Started**: 2026-11-05
- **References**:
  - `AGENTS.md` §2 (component rule), §3 (documentation), §5 (gates)
  - `DESIGN.md` § "Screen footer"
  - T0030's contract for the cleanup request and its cached result

## Context & Goal

T0030 shipped the cleanup pass as **two** footer actions: a magic wand that runs
the pass, and an undo that appears beside it once a repaired text is on screen.
That is one gesture split across two controls, and it makes the footer wider for
no reason: after the first repair the wand is permanently disabled, so the slot
holds a dead button next to the live one.

This ticket collapses them into **a single toggle**:

- no repair yet → **Clean up text** runs the pass;
- repaired text on screen → **Show original text** restores the pre-repair text;
- original on screen after a repair → **Show cleaned text** brings the repaired
  text back **without calling the model again**.

The repaired text is kept in memory for the life of the take. Flipping is local
and instant; only the first click costs a request.

## Scope Boundary

Allowed to create or modify:

- `crates/speechtt-ui/src/controller.rs`
- `crates/speechtt-ui/src/app.rs`
- `crates/speechtt-ui/src/views/session_end.rs`
- `crates/speechtt-ui/tests/session_flow.rs`
- `crates/speechtt-ui/tests/widget.rs`
- `DESIGN.md`
- `tickets/README.md` and this ticket

Out of scope — do **not** touch:

- `src/stt/**` (the worker, the prompt and the request are unchanged)
- `crates/speechtt-core/**` (no protocol change: the same one request per take)
- `crates/speechtt-ui/src/components/**` (one `IconAction` is enough)
- `crates/speechtt-ui/src/views/dialogs/**` (the Settings rows are unchanged)
- `crates/speechtt-ui/src/tokens.rs`, `design-tokens.css`, `tests/design_system.rs`

## Contract

### 1. One cached pass per take

```rust
/// The cached result of one repair pass.
///
/// Both halves are kept so the toggle can flip between them without a request:
/// `original` is the pair the take had before the repair, `cleaned` the text the
/// model returned. A take is repaired at most once; every later flip is local.
#[derive(Debug, Clone)]
struct CleanSnapshot {
    original: (SharedString, SharedString),
    cleaned: String,
}
```

`SessionController` replaces its `clean_original` field with
`clean_snapshot: Option<CleanSnapshot>`.

### 2. States

```rust
pub enum CleanState {
    /// No cleanup model resolves.
    Unavailable,
    /// Repaired text is on screen.
    Cleaned,
    /// The pre-repair text is on screen and the repaired text is cached.
    ShowingOriginal,
    /// Idle with a model available and no cached pass.
    Ready,
}

pub enum CleanStatus {
    Unavailable,
    Ready,
    Running,
    /// The button shows the repaired text and offers the original.
    Cleaned,
    /// The button shows the original and offers the cached repaired text.
    ShowingOriginal,
}

/// What the single cleanup button does next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CleanToggle {
    /// Run the pass: the caller sends `CleanTranscript`.
    Start,
    /// Show the cached repaired text. No request.
    ShowCleaned,
    /// Show the pre-repair text. No request.
    ShowOriginal,
    /// Nothing to do: no take, no model, or a request in flight.
    Inert,
}
```

### 3. Controller API

```rust
/// What the cleanup button does next.
pub fn clean_toggle(&self) -> CleanToggle;

/// Applies the local half of a toggle: flips `Cleaned` ⇄ `ShowingOriginal`
/// from the cached snapshot and returns `false` (changing nothing) for
/// `Start` / `Inert`.
pub fn flip_clean_transcript(&mut self) -> bool;

/// `true` when the button is reachable at all: a finished non-empty take, a
/// resolved model, or a cached pass that can still be flipped.
pub fn can_clean_transcript(&self) -> bool;
```

Rules:

- `clean_toggle` returns `Inert` while a request is in flight, so a double click
  cannot issue two requests.
- `can_clean_transcript` is `true` for `Cleaned` and `ShowingOriginal` **even
  with no model**: a cached pass must stay flippable if the catalogue later
  loses the model.
- `begin_clean_transcript`, `apply_cleaned`, `fail_clean_transcript` and
  `forget_clean` keep their current contracts, except that `apply_cleaned`
  records `CleanSnapshot { original, cleaned }` and always lands on `Cleaned`.
- `undo_clean_transcript` is replaced by `flip_clean_transcript`; `forget_clean`
  drops the snapshot so a new take starts un-repaired.
- Copy, Save, the automatic paste and the displayed text all keep reading the
  text on screen, whichever half that is.

### 4. One button in the footer

`views/session_end.rs` renders **one** `IconAction` in the trailing zone, before
Copy and Save, whose glyph, label and effect come from `clean_status()`:

| `CleanStatus` | Icon | Label | Effect |
|---|---|---|---|
| `Unavailable` | `WandSparkles` | "Clean up text" | disabled |
| `Ready` | `WandSparkles` | "Clean up text" | `CleanToggle::Start` |
| `Running` | `WandSparkles` | "Cleaning up…" | disabled |
| `ShowingOriginal` | `WandSparkles` | "Show cleaned text" | flip to cleaned |
| `Cleaned` | `RotateCcw` | "Show original text" | flip to original |

The glyph names **the action**, not the state: the wand means "end up on the
repaired text", the reverse arrow "go back to the original". The id stays
`btn-clean-transcript`, so the headless tests keep one target.

The status line under the transcript reads "Cleaning up…" while a request is
outstanding, "Cleaned" after it lands and "Original restored" after a flip to
the original — the existing transient status slot, no new element.

## TDD Tasks

- [x] **T33.1 — Red: the toggle contract.** Failing controller tests:
  `clean_toggle` returns `Ready → Start`, `Cleaned → ShowOriginal`,
  `ShowingOriginal → ShowCleaned`, `Running → Inert`, no model → `Inert`; a flip
  to the original and back restores both texts exactly and issues no request;
  `can_clean_transcript` stays true after a flip even when the model is gone.
- [x] **T33.2 — Red: one pass per take.** Failing tests: `apply_cleaned` caches
  both halves; flipping twice is stable; `forget_clean` (new take, reset, cancel)
  drops the cache; Copy/Save/auto-paste read whichever half is on screen.
- [x] **T33.3 — Green: controller.** Replace `clean_original` with
  `clean_snapshot`, add `CleanToggle`, `clean_toggle`, `flip_clean_transcript`,
  extend `CleanState`/`CleanStatus`, and remove `undo_clean_transcript`.
- [x] **T33.4 — Green: the single button.** Rewrite the footer control in
  `views/session_end.rs`; add `toggle_clean_transcript` on the app that switches
  on `CleanToggle` (sending the command only for `Start`). Headless test: one
  click cleans, a second shows the original, a third shows the cleaned text with
  no new `CleanTranscript` command on the sink.
- [x] **T33.5 — Docs and gates.** `DESIGN.md` § "Screen footer" documents the
  single toggle and its table. Then `cargo fmt -- --check`,
  `cargo clippy --all-targets -- -D warnings`, `cargo test`, `bun test`,
  `bun check src/stt`, `bun run design:validate`.

## Acceptance Criteria

- The finished-take footer has **one** cleanup button, not two.
- The first click repairs the transcript; the second restores the original; the
  third brings the repaired text back **without any new request** — the sink
  receives exactly one `CleanTranscript` per take.
- The button is disabled while a request is in flight and when no pass can run.
- Copy, Save and the automatic paste always carry the text on screen.
- A new take starts un-repaired with the button reading "Clean up text".
- All six gates are green with zero warnings and no skipped tests.
- Final visual review is left to the user.

## Implementation Notes

### Decisions

- **`CleanSnapshot` replaces `clean_original`.** It holds both halves — the
  pre-repair pair and the repaired text — because a toggle needs to restore
  either one. `clean_original` could only restore the original, which is why the
  previous design had to make the wand die after one pass.
- **The state is derived, not stored.** `clean_state` compares the displayed
  text with `snapshot.cleaned` to tell `Cleaned` from `ShowingOriginal`, so a
  single source of truth (the take plus the cache) decides what the button shows.
  No extra `showing_original` flag to keep in sync.
- **`CleanToggle` names the intent, the view picks the glyph.** The controller
  answers `Start` / `ShowCleaned` / `ShowOriginal` / `Inert`; `clean_action` in
  `views/session_end.rs` maps that to icon and label. A view never inspects the
  snapshot.
- **`Inert` covers a request in flight**, so a double click cannot issue two
  requests, and `can_clean_transcript` is defined as "the toggle is not inert
  and the take has text". That single definition replaces the old conjunction,
  which had to special-case "already cleaned".
- **A cached pass outlives its model.** `can_clean_transcript` is true for
  `Cleaned`/`ShowingOriginal` even when `cleanup_model()` stops resolving, so the
  two texts the user already has stay comparable. Pinned by
  `a_cached_pass_stays_flippable_after_the_model_disappears`.
- **One id, one target.** The button keeps `btn-clean-transcript`, so the
  headless test clicks one control whatever its glyph; `btn-undo-clean` is gone.
- `undo_clean_transcript` is removed from the controller and the app rather than
  kept alongside the toggle: two ways to leave the repaired text would drift.

### Gates (repository root, zero warnings, no skipped tests)

| Command | Result |
|---|---|
| `cargo fmt -- --check` | clean |
| `cargo clippy --all-targets -- -D warnings` | clean |
| `cargo test` | **549 passed, 0 failed** |
| `bun test` | 317 passed, 0 failed |
| `bun check src/stt` | 29 files, no type errors |
| `bun run design:validate` | 66 tokens, 0 errors |

### Proof the pass runs once

`the_clean_button_repairs_and_then_flips_between_both_texts` (headless, real
pointer events) presses the button three times against the real widget and
asserts the command sink grows **only on the first press**, that the second press
shows the original, the third shows the repaired text, and that exactly one
`CleanTranscript` left the widget for the take.

### Left for the user

Visual review of the single toggle: the wand → reverse-arrow glyph change, the
`Show original text` / `Show cleaned text` labels, and the status line wording.
No GUI run or screenshot was taken autonomously.
