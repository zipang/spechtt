# T0043 — Dialogs are their own windows, not views of the widget

- **Track**: Rust UI (module layout, design-system gate)
- **Wave**: 1 (independent — pure move plus the gate that guards it)
- **Depends on**: nothing
- **Blocks**: nothing
- **Owner**: coding sub-agent, supervised by CTO
- **Started**: 2026-11-10
- **References**:
    - `AGENTS.md` §2 (module boundaries, the view rule, the component rule), §3
      (documentation in code — names its callers), §5 (quality gates), §6
      (Do/Don't)
    - `crates/speechtt-ui/src/views/mod.rs` — the module that declares `dialogs`
    - `crates/speechtt-ui/src/views/dialogs/` — the four files that move
    - `crates/speechtt-ui/src/lib.rs` — where a sibling module is declared
    - `crates/speechtt-ui/tests/design_system.rs` — `views_do_not_choose_spacing_type_or_radius`,
      the gate whose scan root this ticket widens

## Context & Goal

`views` is documented as "screen renderers, one screen per file", each drawing
one state of the **widget window**. It also holds `dialogs/`, whose members are
not screens of that window at all: `DialogWindow` opens its own floating OS
window, with its own header, its own size constants and its own keymap scope.

So the directory tree says something untrue. A reader looking for "what the
widget draws" finds four files that are never drawn in it, and the module docs
have to carry an exception instead of a rule.

Done means `crates/speechtt-ui/src/dialogs/` sits next to `views/` at the crate
root, `views/` holds only widget screens, every reference and doc comment names
the new path, and the design-system gate still covers the moved files.

### The trap this ticket must not spring

`tests/design_system.rs::views_do_not_choose_spacing_type_or_radius` walks
`src/views` **recursively** and fails a file that picks a `px(`, `rems(` or
`radius::` value. Today it reaches the dialogs only because they live under
`views/`. Moving them out without widening the scan silently removes four files
from a quality gate: nothing turns red, and the next raw spacing value in
`dialogs/settings.rs` passes. Contract §3 closes it, and the test that proves
the coverage exists comes **before** the move.

## Scope Boundary

May create, move or modify:

- `crates/speechtt-ui/src/dialogs/mod.rs`, `help.rs`, `settings.rs`,
  `window.rs` — **moved** from `crates/speechtt-ui/src/views/dialogs/` with
  `git mv`, preserving history. Contents change only where an intra-crate path
  or a doc comment names the old location.
- `crates/speechtt-ui/src/views/dialogs/` — **deleted** (emptied by the move).
- `crates/speechtt-ui/src/lib.rs` — declares the new module.
- `crates/speechtt-ui/src/views/mod.rs` — drops the `dialogs` declaration.
- `crates/speechtt-ui/src/app.rs` — the two `views::dialogs` references.
- `crates/speechtt-ui/tests/design_system.rs` — the spacing gate's scan root and
  the test that proves it covers both directories.
- Doc comments naming the old path in `crates/speechtt-ui/src/shortcuts.rs`,
  `src/components/text.rs`, `src/components/model_select.rs`,
  `src/components/language_select.rs`, `src/components/info_note.rs`,
  `src/components/app_header.rs` — path text only, no other edit.
- `AGENTS.md` — §2 module boundaries and the view rule, §3 scope if it names the
  old path.
- `tickets/README.md` — board row.

Out of scope — do **not** touch:

- No behaviour change of any kind. Not one line of rendering logic, controller
  logic, protocol, or test assertion moves in this ticket.
- No rename of `Router`-style items: `DialogWindow`, `DialogShell`,
  `window_title`, `DIALOG_WIDTH`, `HELP_DIALOG_HEIGHT`,
  `SETTINGS_DIALOG_HEIGHT` keep their names and their `pub(crate)`
  visibility. The module path changes; the API does not.
- No change to `components/`, `layout/`, `controller.rs` beyond path text in
  doc comments.
- No change to `tickets/done/**` — those are the audit trail and keep the paths
  that were true when they were written.
- No new gate, no new lint, no touched `DESIGN.md`: it names no module path.
- No `#[allow(...)]` to quiet a warning the move produces.

## Contract

### §1 — The module moves to the crate root

- `crates/speechtt-ui/src/dialogs/` holds `mod.rs`, `help.rs`, `settings.rs`,
  `window.rs`, moved with `git mv` so `git log --follow` still reaches their
  history.
- `crates/speechtt-ui/src/lib.rs` declares it beside `views`:
  `mod dialogs;` — private, exactly like `mod views;`, because every item the
  rest of the crate uses is `pub(crate)`.
- `crates/speechtt-ui/src/views/mod.rs` no longer declares `dialogs`, and its
  module doc describes only widget screens.

### §2 — Every reference names the new path

- Code: `crate::views::dialogs::X` → `crate::dialogs::X`. The affected sites are
  `views/dialogs/help.rs`, `views/dialogs/settings.rs`, `views/dialogs/window.rs`
  and `app.rs` (the `use` at the top and `views::dialogs::window_title`).
- Doc comments: `views::dialogs::X` → `dialogs::X` — in `dialogs/*.rs` itself,
  and in `shortcuts.rs` (2), `components/text.rs` (1),
  `components/model_select.rs` (1), `components/language_select.rs` (2),
  `components/info_note.rs` (1), `components/app_header.rs` (1).
- Every doc comment in the moved files that names its callers keeps doing so
  with the new path (`AGENTS.md` §3). `dialogs/mod.rs` gains a sentence stating
  it is dispatched from `crate::app`, because a module outside `views` no longer
  answers "who draws this?" by its neighbours.

### §3 — The spacing gate covers both directories

`tests/design_system.rs::views_do_not_choose_spacing_type_or_radius` scans
`src/views` **and** `src/dialogs`. Concretely:

```rust
/// The directories whose files may not choose a spacing, type or radius value.
///
/// Dialogs are not views of the widget window — they open their own — but they
/// are held to the same rule, so the scan root is these two directories and not
/// `src` at large.
fn styled_ui_dirs() -> Vec<PathBuf>;
```

- `styled_ui_dirs()` returns `src/views` and `src/dialogs`, and the gate walks
  both.
- The test is renamed to say what it covers
  (`views_and_dialogs_do_not_choose_spacing_type_or_radius`) and its failure
  message names both directories.
- A second test pins the coverage, so a future move cannot drop a directory
  silently:

    ```rust
    /// The spacing gate is only as wide as its scan root: these two files must be
    /// in the checked set, or moving a directory quietly exempts it.
    #[test]
    fn the_spacing_gate_checks_the_dialogs_too();
    ```

    It asserts that the scanned set contains `dialogs/settings.rs` and
    `views/session_setup.rs`.

### §4 — `AGENTS.md` describes the tree it has

- §2 lists the crate's modules with `dialogs` beside `views`, saying a file under
  `src/dialogs/` owns a dialog that opens its own OS window and is not a view of
  the widget window.
- The view rule stays, scoped to `crates/speechtt-ui/src/views/`, and the
  component rule's scope sentence covers both directories, because the gate now
  does.

## TDD Tasks

- [ ] **T43.1 — Coverage test first**: refactor the scan root of
      `tests/design_system.rs` into `styled_ui_dirs()`, add
      `the_spacing_gate_checks_the_dialogs_too`, and run it: red while `src/dialogs`
      does not exist or the helper still returns one directory.
- [ ] **T43.2 — Move**: `git mv crates/speechtt-ui/src/views/dialogs
crates/speechtt-ui/src/dialogs`, wire `lib.rs` and drop the declaration from
      `views/mod.rs`.
- [ ] **T43.3 — References**: code paths per §2; `cargo check` clean.
- [ ] **T43.4 — Green**: `the_spacing_gate_checks_the_dialogs_too` passes, and
      `views_and_dialogs_do_not_choose_spacing_type_or_radius` still passes on the
      moved files.
- [ ] **T43.5 — Docs**: §2 of this contract, plus `AGENTS.md` §2.
- [ ] **T43.6 — Gates**: `cargo fmt -- --check`,
      `cargo clippy --all-targets -- -D warnings`, `cargo test`, `bun test`,
      `bun check src/stt`, `bun run design:validate`. Zero warnings; nothing skipped
      or `#[ignore]`d.

## Acceptance Criteria

1. `crates/speechtt-ui/src/views/` contains no `dialogs/` directory, and
   `crates/speechtt-ui/src/dialogs/` contains the four files.
2. `git log --follow crates/speechtt-ui/src/dialogs/settings.rs` reaches the
   commits that predate the move.
3. `grep -rn "views::dialogs\|views/dialogs" crates/ AGENTS.md` matches nothing.
4. The spacing gate scans both `src/views` and `src/dialogs`, and
   `the_spacing_gate_checks_the_dialogs_too` fails if either is dropped from the
   scan root.
5. No behaviour changes: the Rust test count rises by exactly the tests this
   ticket adds, every pre-existing test keeps its name and its assertions, and
   the Settings dialog renders as before.
6. Every gate in T43.6 is green with zero warnings, and no `#[allow]` was added.

## Implementation Notes

_(added during work: the exact commands run, the test counts before and after,
and any deviation from the contract with its reason.)_
