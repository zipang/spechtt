# T0010 — Settings: Prove Key Integrity and Fix Stored-Key Reveal UX

- **Track**: Rust UI (`crates/speechtt-ui`) + Rust core (`crates/speechtt-core`)
- **Wave**: 2 (follow-up to T0009)
- **Depends on**: T0006 (settings store), T0009 (settings wiring)
- **Blocks**: nothing
- **PRODUCT.md**: Phase 4 / Phase 9
- **Owner**: coding sub-agent (deepseek-v4.1-flash), supervised by CTO
- **Started**: 2026-10-27

## Context & Goal

The CEO/PM reported that a Vercel AI Gateway key entered in Settings was
rejected by Vercel with `401 Invalid API key or token`, even though the key in
`.env.local` works. Comparing the two values showed a single-character case
difference (settings had `h`, `.env.local` had `H`), which is fatal because
Vercel keys are case-sensitive. A fresh copy now matches and transcribes
successfully, so the mismatch was most likely an older stored value — but the
event exposed two gaps:

1. Nothing proves that the Settings save path (input → controller → JSON file)
   preserves the key byte-for-byte.
2. The dialog never shows the stored key at all: on reopen the field is empty
   and **Reveal** reveals an empty string, so a corrupted key cannot be
   noticed.

This ticket adds regression tests for byte-exact key handling and makes the
stored-key state visible without exposing the secret.

## Scope Boundary

Only touch:

```text
crates/speechtt-core/src/settings.rs              # tests only (no behaviour change)
crates/speechtt-ui/src/app.rs                     # reveal/stored-key plumbing
crates/speechtt-ui/src/controller.rs              # stored-key query helpers
crates/speechtt-ui/src/views/dialogs/settings.rs  # stored-key presentation
crates/speechtt-ui/tests/end_to_end.rs            # regression tests
crates/speechtt-ui/tests/dialogs.rs               # regression tests
```

Do **not** touch `src/stt/`, `crates/speechtt-core/src/audio.rs`, `fft.rs`,
`output.rs`, or other tickets.

## Contract

### 1. Byte-exact preservation (tests first)

- `Settings::set_api_key(provider, key)` stores the exact string, including
  mixed case and non-ASCII, and `save` → `load` round-trips it byte-for-byte.
- The UI path must preserve the exact value:
  `InputState::set_value(mixed_case)` → subscription →
  `set_settings_api_key_value` → `save_settings` → the injected
  `SettingsStore` receives the identical string.
- Toggling `set_masked(true)` / `set_masked(false)` on the API-key input must
  not change `InputState::value()`.
- A simulated paste of a mixed-case key (via the input's value-update path)
  followed by Save stores the same bytes.

### 2. Stored-key presentation (no secret exposure)

- `SessionController` exposes the stored mask for the selected provider, using
  `Settings::masked_api_key` (e.g. `vck_…Nn3F`).
- When the dialog opens with a stored key (`api_key_unchanged = true`) and the
  input is empty, the API-key row shows a secondary line such as
  `Stored key: vck_…Nn3F` (never the raw key).
- **Reveal** must not pretend to reveal a stored key: when the field is empty
  and a key is stored, the Reveal control is disabled with a hint such as
  `Stored keys are never displayed; paste a new key to replace it.`
- Typing or pasting a new value re-enables Reveal for that new draft value.
- No raw key may appear in `Debug`, logs, error text, or test output.

## TDD Tasks

- [x] **T10.1 — Core round-trip test**: mixed-case key survives
  `set_api_key` + `save` + `load` exactly.
- [x] **T10.2 — UI save-path tests**: input value with mixed case reaches the
  store unchanged; masking toggle does not alter the value; simulated paste +
  Save is byte-exact.
- [x] **T10.3 — Stored-mask tests**: `masked_api_key` is surfaced for the
  selected provider; no raw key in any rendered/debug string.
- [x] **T10.4 — Reveal UX tests**: Reveal is disabled for a stored key with an
  empty field; enabled once a new value is typed; hint text present.
- [x] **T10.5 — Implementation**: controller helper + dialog row
  presentation + reveal gating.
- [x] **T10.6 — Gates**: `cargo fmt -- --check`,
  `cargo clippy --all-targets -- -D warnings`, `cargo test`, `bun test`,
  `bun check src/stt`.

## Acceptance Criteria

- A mixed-case API key saved through the UI is byte-identical on disk.
- Reopening Settings shows that a key is stored (`Stored key: …`) and does not
  offer a misleading empty Reveal.
- The raw key is never displayed, logged, or included in an error.
- All tests run headless with injected stores.

## Implementation Notes

### Decisions

- Core (`crates/speechtt-core/src/settings.rs`): added test-only
  `mixed_case_api_key_round_trips_byte_for_byte`. It saves the key
  `vck_AbCdEfGhIjKlMnOpQrStUvWxYzH0123456789` through `JsonFileStore`, reloads
  it, and compares both the value and its bytes. No store behaviour changed.
- Controller (`crates/speechtt-ui/src/controller.rs`): added
  `settings_stored_key_mask`, `settings_key_is_stored_unchanged`,
  `settings_stored_key_label`, `settings_stored_key_hint`, and
  `settings_key_reveal_enabled`. The mask comes only from
  `Settings::masked_api_key`, which keeps the first three and last two
  characters (`vck…89` for the regression key). The raw key is never copied
  into the draft or a presentation string.
- App (`crates/speechtt-ui/src/app.rs`): added read-only helpers for the
  stored-key label/hint and the input value; `toggle_settings_key_reveal` is
  now a no-op while the field is empty. A stored key cannot be "revealed"
  because it is not held in the field.
- Dialog (`crates/speechtt-ui/src/views/dialogs/settings.rs`): when a key is
  stored and untouched, the API-key row shows `Stored key: <mask>` plus the
  hint `Stored keys are never displayed; paste a new key to replace it.`
  The Reveal button uses gpui-kit's `Disableable` trait and is disabled until
  the user types or pastes a draft value. Rendering stays driven by controller
  queries; no raw key enters any element.
- Tests: `crates/speechtt-ui/tests/dialogs.rs` covers the stored mask, label,
  hint, redacted debug output, and reveal enabled/disabled transitions.
  `crates/speechtt-ui/tests/end_to_end.rs` covers byte-exact save through
  `set_settings_api_key_value`, masking toggles leaving `InputState::value()`
  unchanged, the simulated paste/value-update path, and the reopened-Settings
  stored-key presentation.

### Test coverage added

- `settings::tests::mixed_case_api_key_round_trips_byte_for_byte`
- `dialogs::stored_key_mask_is_surfaced_without_revealing_the_key`
- `dialogs::reveal_is_disabled_for_a_stored_key_until_a_value_is_typed`
- `end_to_end::a_mixed_case_settings_key_is_saved_byte_for_byte`
- `end_to_end::a_pasted_mixed_case_key_reaches_the_store_unchanged`
- `end_to_end::a_stored_key_shows_only_its_mask_and_reveal_stays_disabled`

### Gate results (2026-10-27)

- `cargo fmt -- --check` — pass (no output).
- `cargo clippy --all-targets -- -D warnings` — pass
  (`Finished dev profile ...`).
- `cargo test` — pass: 260 tests total, 0 failed.
  - `speechtt-core` lib: 165
  - `bridge_smoke`: 2
  - `session_machine`: 11
  - `speechtt-ui` lib: 14
  - `speechtt-ui` bin: 0
  - `dialogs`: 10
  - `end_to_end`: 26
  - `error_ux`: 9
  - `session_flow`: 17
  - `theme_contract`: 4
  - `widget`: 2
  - doc-tests: 0
- `bun test` — 87 pass, 0 fail, 443 `expect()` calls across 7 files.
- `bun check src/stt` — pass: `No type errors in 13 files`.

