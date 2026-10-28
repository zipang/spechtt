# T0013 — Remember the Window Size and Position

- **Track**: Rust core (settings) + Rust UI (`app`, `lib`)
- **Wave**: 2 (follow-up to T0011's setup-screen truncation)
- **Depends on**: T0006 (settings store), T0011 (setup form with mode chips)
- **Blocks**: nothing
- **Owner**: CTO (direct implementation)
- **Started**: 2026-10-28

## Context & Goal

The New Session form uses the roomier prominent rows, but the widget window is
opened at `480 x 280`, so the last rows are clipped (reported twice). Raising
the default is only half the answer: a floating widget should also reopen where
the user left it, at the size they chose.

`Settings` already persists the provider, model and keys; extend it with the
window geometry instead of adding a second file.

## Scope Boundary

```text
crates/speechtt-core/src/settings.rs                  # WindowLayout + field + normalize + tests
crates/speechtt-ui/src/lib.rs                         # default size, restore + clamp
crates/speechtt-ui/src/app.rs                         # observe bounds, debounce, persist
crates/speechtt-ui/src/tokens.rs                      # default height that fits the form
crates/speechtt-ui/tests/end_to_end.rs                # persistence + clamping tests
crates/speechtt-ui/tests/widget.rs                    # form-fits regression test
DESIGN.md                                             # window size note
tickets/README.md                                     # board
```

Do not touch `src/stt/**` (T0011/T0012 own it), dialogs, or the design tokens
CSS.

## Contract

### 1. `WindowLayout` (core)

```rust
pub struct WindowLayout { pub width: u32, pub height: u32, pub x: i32, pub y: i32 }
```

- Logical pixels: DPI-independent, negative `x`/`y` allowed (multi-monitor).
- `MIN_WIDTH = 420`, `MIN_HEIGHT = 320`; `is_usable()` requires both.
- `Settings` gains `#[serde(default, skip_serializing_if = "Option::is_none")]
  pub window: Option<WindowLayout>` (last field, so the pretty-print order test
  still holds).
- `Settings::normalize_window_layout()` drops unusable layouts; it runs on
  load and save (next to `normalize_provider_ids`).
- `Debug` never gains key material; geometry is safe to print.

### 2. Startup (`lib.rs`)

- `WIDGET_HEIGHT = 520` (fits four prominent rows plus the optional error
  strip; line height is 26 px in this theme).
- `widget_window_options_for(layout: Option<WindowLayout>)`:
  - size = saved layout clamped to at least the minimum size, else
    `WIDGET_WIDTH × WIDGET_HEIGHT`;
  - origin = saved position, clamped so at least 60 logical px of the window
    remain inside the primary display's visible area; default `(0, 0)`.
- `run()` loads `JsonFileStore::default_path()` before creating the window and
  passes the stored layout.

### 3. Remembering (`app.rs`)

- Every render records `window.window_bounds()` when it is
  `WindowBounds::Windowed` (ignored for maximized/fullscreen).
- A change stores `(layout, now)` as pending; the existing 200 ms clock loop
  persists once the geometry has been stable for **600 ms**, so a drag writes
  at most once.
- Persisting clones `stored_settings`, sets `window`, calls the injected
  `SettingsStore::save`, and updates `stored_settings` on success. Failures are
  best-effort (no error strip, no panic).
- `apply_settings_change` keeps `window: self.stored_settings.window` so saving
  a provider never erases the geometry.
- Public seam for tests:
  `remember_window_layout(layout, now)` and
  `flush_pending_window_layout(now) -> bool`.

## TDD Tasks

- [x] **T13.1** — core: round-trip, missing field, unusable layout dropped,
  save/load preserves geometry and still normalizes provider ids.
- [x] **T13.2** — UI: `widget_window_options_for(Some(...))` restores size and
  position; unusable/out-of-screen values clamp; no layout uses the 520 px
  default.
- [x] **T13.3** — app: a stable layout is persisted after 600 ms; moving again
  resets the debounce; saving settings keeps the geometry.
- [x] **T13.4** — widget: a regression test computes the New Session height
  from the real line height and asserts `WIDGET_HEIGHT` covers it.
- [x] **T13.5** — gates.

## Acceptance Criteria

- First launch opens a window tall enough to show all four New Session rows.
- Moving or resizing the window and relaunching restores the same geometry.
- A geometry saved on a disconnected monitor is clamped into the visible area.
- Saving provider/model/key never clears the remembered geometry.
- All gates green, zero warnings.

## Implementation Notes

### Delivered

- `WindowLayout { width, height, x, y }` (logical px, i32 position) in
  `speechtt-core::settings`, stored as `Settings.window: Option<WindowLayout>`
  (last field, camelCase, skipped when `None`). Defaults require no migration:
  an older file simply loads with `None`.
- `normalize()` (provider ids + geometry) runs on load and save; a layout below
  `420 x 320` is dropped.
- `Widget height 520`: verified against the real theme metrics (line height
  26 px) by a regression test that recomputes the form height from the layout
  constants, including the optional error strip. It is the default only.
- Startup restore: `restored_window_bounds(layout, displays)` clamps the size to
  the first display and keeps at least 60 logical px of the window on screen
  (a layout saved on a disconnected monitor cannot hide the widget). `run()`
  loads the settings file **before** creating the window, so the first frame is
  already correct.
- Remembering: every render records `WindowBounds::Windowed`; a change is
  pending until it has been stable for 600 ms, then the existing 200 ms clock
  loop persists it through the injected `SettingsStore`. Moves during a drag
  therefore write at most once, and a geometry identical to the stored one
  clears the pending change. The untouched default is never persisted (no
  settings file created just because the app rendered).
- Saving provider/model/key keeps `window` (`apply_settings_change`), so the two
  concerns cannot erase each other.
- Failures to persist the geometry are silent: no error strip for a window
  position.

### Notes

- Maximized/fullscreen bounds are ignored deliberately: restoring a maximized
  flag is out of scope and the windowed rectangle stays the useful one.
- Multi-monitor: the clamp uses the first display reported by the platform
  (primary). Restoring onto a specific secondary display is a possible
  follow-up.

### Gates

```
cargo fmt -- --check                      clean
cargo clippy --all-targets -- -D warnings clean
cargo test                                283 passed, 0 failed
bun test                                  146 passed, 0 failed
bun check src/stt                         ✓ 19 files, no type errors
bun run design:validate                   ✓ 66 tokens, 0 errors
```
