# T0036 — Migrate the UI to gpui-kit 0.9.1

- **Track**: Rust UI (framework bump) — cross-cutting only in that it touches the
  documentation registry
- **Wave**: 1 (independent)
- **Depends on**: T0035 (the keyboard work is what made two framework gaps
  visible; it must be closed first so its behaviour is the baseline)
- **Blocks**: nothing
- **References**:
  - crates.io API: `<https://crates.io/api/v1/crates/gpui-kit>` (version +
    publish dates), `<https://crates.io/api/v1/crates/gpui-kit/0.9.1/dependencies>`
    (exact framework pins)
  - `gpui-kit` docs for the pinned target: `<https://docs.rs/gpui-kit/0.9.1>`
  - Markdown guide index: `<https://gpui-kit.com/llms.txt>` (already documents
    0.9.x, so the repository is behind its own reference documentation)
  - KeyBinding / Action / Testing pages:
    `<https://gpui-kit.com/docs/keybinding.md>`,
    `<https://gpui-kit.com/docs/action.md>`,
    `<https://gpui-kit.com/docs/test.md>`
  - `AGENTS.md` §6 "Official documentation sources — the registry"

## Context & Goal

The workspace pins `gpui-kit = "0.8"`, which resolves to **0.8.0** (published
2026-10-19) and drags `gpui-pre 0.3.9`. The framework has since published
**0.8.1** (2026-10-26), **0.9.0** (2026-11-02) and **0.9.1** (2026-11-09), so the
repository is two minor versions behind the crate *and* behind the
`gpui-kit.com` documentation it is instructed to treat as authoritative
(`AGENTS.md` §6). Every guide page fetched today describes 0.9.x, which means an
implementing agent following the documented API can write code that does not
compile — that is exactly what happened twice during T0035.

**Goal.** Move the workspace to `gpui-kit 0.9.1`, recover the two capabilities
that were worked around under 0.8.0, and re-run every gate.

## What 0.9.1 changes underneath

`gpui-kit 0.9.1` pins its layers exactly:

| Crate | Now (0.8.0) | Target (0.9.1) |
|---|---|---|
| `gpui-kit` | 0.8.0 | 0.9.1 |
| `gpui-base` | 0.8.0 | 0.9.1 |
| `gpui-component` | 0.8.0 | 0.9.1 |
| `gpui-kit-assets` | 0.8.0 | 0.9.1 |
| `gpui-pre` | 0.3.9 | **=0.3.12** (exact pin) |
| `gpui-fast` | 0.1.10 | ^0.1.14 |

`gpui-pre` is pre-1.0 and pinned exactly, so a patch bump is a real API surface
change, not a bugfix. Everything the UI re-exports through `gpui_kit` moves with
it.

## Two capabilities the bump recovers

Both were discovered while implementing T0035 and are recorded there as gaps.

1. **`App::platform_info()` exists in 0.9.1.**
   `https://docs.rs/gpui-kit/0.9.1/gpui_kit/struct.App.html`:

   > `pub fn platform_info(self) -> PlatformInfo` — *Returns the host OS and
   > application target reported by the backend.*

   `shortcuts::is_mac()` currently answers `cfg!(target_os = "macos")` because
   neither `App` nor the `Platform` trait exposed the host OS at 0.3.9. The
   documented snippet in `keybinding.md` (`cx.platform_info().is_mac()`) can
   finally be followed. Behaviour today is correct for a native build; the point
   is to stop guessing and to match the documentation.

2. **`Window::bindings_for_action` may become usable as an oracle.**
   The method exists in both versions and is documented as returning *"key
   bindings that invoke an action on the currently focused element"*. Under
   0.8.0 it returned no bindings for any action, including `ToggleHelp` and
   `CopySelection` while `f1` demonstrably dispatched and opened the Help dialog
   (re-verified independently during T0035's second review round). If 0.3.12
   fixes it, T0035's follow-up assertion can be written:
   in a dialog window, `window.bindings_for_action(&ToggleRecording).is_empty()`.

## Scope Boundary

May change:

- `Cargo.toml` (workspace dependency version), `Cargo.lock`.
- Every `crates/speechtt-ui/src/**/*.rs` and `crates/speechtt-ui/tests/**` file
  that the new API forces to change — nothing else, and only for compile or
  behaviour reasons.
- `crates/speechtt-core/**` only if a transitive type it names changes.
- `AGENTS.md` §6 (documentation registry rows for `gpui-kit` / `gpui-base` /
  `gpui-component`) and §4 (tech stack version), `README.md`, `PRODUCT.md`
  (§7 topology line), `DESIGN.md` only if a token or component signature moved.
- `tickets/README.md`, this ticket.

Do **not** touch: the transcript reducer's behaviour, the worker (`src/stt/**`),
the design tokens' values, or any `tickets/done/**`. Do not "fix" a failing test
by relaxing it — a red gate after a framework bump is information, not noise.

## Contract

### A. Dependency

```toml
# Cargo.toml
[workspace.dependencies]
gpui-kit = "0.9.1"
```

`Cargo.lock` must show `gpui-kit`, `gpui-base`, `gpui-component`,
`gpui-kit-assets` at `0.9.1` and `gpui-pre*` at `0.3.12`. No crate may resolve to
two framework versions; `cargo tree -d` must not list `gpui-*` twice.

### B. Recovering `is_mac`

`shortcuts::is_mac()` delegates to the documented accessor instead of the build
target:

```rust
/// Whether this build follows macOS keyboard conventions.
#[must_use]
pub fn is_mac(cx: &App) -> bool { cx.platform_info().is_mac() }
```

`bind_keys` already receives `&mut App`; `key_caps` callers in
`views::dialogs::help` read `App`, so the signature change stays local. Every
call site compiles unchanged in meaning.

**Documented signature used** (docs.rs 0.9.1 `struct.App.html`): `pub fn
platform_info(self) -> PlatformInfo`. If `PlatformInfo` does not expose `is_mac`
at 0.3.12, the implementation reads the OS enum it does expose — and the ticket
records the actual shape instead of assuming.

### C. Re-probing `bindings_for_action`

A throwaway probe (not committed) answers, in a widget window whose `f1` press
demonstrably opens Help:

1. `window.bindings_for_action(&ToggleHelp).len()` — expected `>= 1` if the
   method works;
2. the same for `CopySelection`;
3. in a focused Settings dialog, the same for `ToggleRecording`,
   `StopRecording`, `SaveTranscript` — expected `0` **and** `ToggleHelp` `>= 1`
   for the control.

If (1) and (2) answer non-zero, add to `tests/shortcuts.rs` the assertion
T0035 could not ship:

```rust
// The predicate, not merely the fall-through: the take commands are not bound
// on a dialog's dispatch path.
assert!(window.bindings_for_action(&ToggleRecording).is_empty());
assert!(window.bindings_for_action(&StopRecording).is_empty());
assert!(window.bindings_for_action(&SaveTranscript).is_empty());
assert!(!window.bindings_for_action(&ToggleHelp).is_empty()); // control
```

If it still answers zero, record the finding in this ticket and leave T0035 as
it is: the predicate stays pinned by the table-literal tests.

## TDD Tasks

- [ ] **T1 — Baseline capture.** Before touching the version, run and record the
      six gates verbatim (they are green on `0.8.0`). Note the exact test counts;
      every later comparison is against these numbers.
- [ ] **T2 — Bump.** Set `gpui-kit = "0.9.1"`; `cargo update -p gpui-kit`;
      confirm the resolved versions in §A and that `cargo tree -d` shows no
      duplicate framework crate.
- [ ] **T3 — Compile errors, one group at a time.** Group the failures by API
      area (prelude re-exports, `base`/`component` paths, test-support) and fix
      each group against the 0.9.1 docs.rs page — never against
      `~/.cargo/registry/src/**`. Record every signature that changed.
- [ ] **T4 — `is_mac` via `platform_info`.** Red first: a test asserting
      `shortcuts::is_mac` answers the platform's convention on the host running
      the suite. Then implement §B.
- [ ] **T5 — `bindings_for_action` probe.** §C, throwaway, then either the added
      assertions or a recorded negative finding.
- [ ] **T6 — Design system & theme.** `bun run design:validate`,
      `crates/speechtt-ui/tests/design_system.rs` and `theme_contract.rs` must
      pass unchanged. If a token name or role moved in 0.9.1, that is a finding
      to report, not a silent edit of `design-tokens.css`.
- [ ] **T7 — Visual smoke.** No screenshots, no GUI runs (AGENTS.md §6): the
      user performs the visual pass. The ticket lists what to look at.
- [ ] **T8 — Documentation.** `AGENTS.md` §4 version and §6 registry rows,
      `README.md`, `PRODUCT.md` §7; then all six gates green.

## Acceptance Criteria

- `Cargo.lock` resolves the framework to 0.9.1 / `gpui-pre 0.3.12` with no
  duplicate `gpui-*` crate.
- All six gates pass with zero warnings and **at least** the T1 test counts; no
  test was weakened, skipped or `#[ignore]`d to accommodate the bump.
- `shortcuts::is_mac()` reads the documented platform accessor and no longer
  consults `cfg!(target_os = "macos")`.
- The `bindings_for_action` question is answered with evidence: either T0035's
  discriminating assertions are now in `tests/shortcuts.rs`, or the ticket
  records that 0.3.12 still returns nothing and why it cannot be used.
- Every API whose signature changed is listed in `## Implementation Notes` with
  the URL consulted, so the next bump has a starting point.
- `AGENTS.md` names the version actually in use.

## Implementation Notes

_(added during work)_
