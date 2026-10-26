# T0001 — Refactor `crates/` for Adaptable Layout, Reusable Components, and Behavior-First Testing

## Context & Goal

As SpeechTT evolves, the UI inside `crates/speechtt-ui` must be easy to modify, resize, and redesign without fighting hard-coded dimensions, monolithic view files, or brittle DOM-like element presence assertions.

This specification covers:
1. **Critical review & refactoring of the UI test suite** so tests verify *state machine transitions and view-model function calls* rather than coupling to element IDs and screen structure.
2. **Phase 1 — Layout & Resizing**: Making the widget content resize fluidly with the window using flexbox constraints (`min_h_0`, `min_w_0`, `flex_1`) and relative `rem` sizing.
3. **Phase 2 — Box Layout Abstractions**: Extracting composable container primitives (`ScreenShell`, `SurfaceBox`) into `crates/speechtt-ui/src/layout.rs`.
4. **Phase 3 — Reusable Components**: Extracting standalone `RenderOnce` components (`TransportButton`, `IconAction`, `SettingRow`, `ChoicePill`) into `crates/speechtt-ui/src/components/` and splitting `app.rs` into focused view modules.
5. **Phase 4 — Rust Idioms & Performance**: Applying `rust-code-review` best practices (`thiserror` in `speechtt-core`, `SharedString` & slice borrows to avoid per-frame allocations, type-safe draft validation instead of `.expect()`, and `#[non_exhaustive]`).

---

## 0. Critical Review of Current UI Tests

### What exists today
Currently, `crates/speechtt-ui/tests/` contains three test files:
1. **`tests/theme_contract.rs`** (Fast, valuable): Verifies that `speechtt_ui::tokens` and `speechtt_ui::theme::apply` match the `design-tokens.css` contract.
2. **`tests/widget.rs`** (Low value, high coupling): Spawns a full headless window and asserts `window.try_find("app-title")`, `window.try_find("btn-help")`, and `window.try_find("btn-settings")`.
3. **`tests/session_flow.rs`** (Mixed value, brittle mechanism): Drives the session workflow by clicking specific `ElementId`s (`window.click("btn-new-session")`, `window.click("field-provider")`, etc.) and asserting which `ElementId`s exist or do not exist (`assert!(window.try_find("caption-well").is_none())`, `assert!(window.try_find("btn-open-file").is_some())`).

### Why `widget.rs` and `session_flow.rs` hurt UI evolution
Even though `PRODUCT.md` says *"test the function, not the pixels"*, asserting `window.try_find("...")` and `window.click("...")` across 15+ hardcoded `ElementId`s **couples tests to the exact widget tree and screen decomposition**:
- If you merge the `"New Session"` prompt and the setup form into a single screen, `session_flow.rs` breaks.
- If you move Provider/Model selection from the setup form into the Settings dialog, `session_flow.rs` breaks.
- If you replace `field-output-memory` and `field-output-file` pills with a single `Switch` or `Select` dropdown, `session_flow.rs` breaks.
- Every UI element has to carry `.id("...").test_support()` clutter in production rendering code solely to satisfy `window.try_find(...)`.

### The Better Testing Strategy: Pure Controller / View-Model Unit Tests
Instead of simulating pointer clicks on `ElementId`s and inspecting the rendered element tree, test **state transitions and command methods directly**:
1. **`crates/speechtt-core/tests/session_machine.rs`**: Keep and expand pure Rust unit tests (`#[test]`, zero GPUI window overhead, runs in milliseconds) for `SessionMachine` and domain logic.
2. **View-Model / Controller methods on `SpeechTtApp` (or `SessionController`)**:
   Expose intentional command methods (`open_form`, `cancel_form`, `select_provider`, `select_model`, `set_session_name`, `select_output`, `create_session`, `start_recording`, `pause_recording`, `resume_recording`, `finish_take`, `new_take`) and state readers (`view_mode()`, `session_state()`, `draft_complete()`, `finished_take()`).
3. Test those methods directly via Rust unit tests (or lightweight entity updates) **without querying `window.try_find(...)`**. One smoke test that opens the window and renders a frame is enough to prove GPUI doesn't panic.

### Tasks — Test Suite Refactoring
- [x] **T0.1**: Extract or expose clean command & query methods on `SpeechTtApp` (or a pure `AppController` struct in `crates/speechtt-ui/src/controller.rs` that doesn't even require a GPUI `Window` for draft/session logic):
  - Queries: `view_state()`, `session_state()`, `session_missing()`, `is_draft_complete()`, `finished_take()`.
  - Commands: `open_form()`, `cancel_form()`, `cycle_provider()`, `cycle_model()`, `set_draft_name(impl Into<String>)`, `set_draft_output(OutputTarget)`, `create_session()`, `start_recording()`, `pause_recording()`, `resume_recording()`, `finish_take()`, `new_take()`.
- [x] **T0.2**: Rewrite `crates/speechtt-ui/tests/session_flow.rs` to call the controller/view methods directly and assert on state transitions and returned data (`SessionState`, `FinishedTake`, `ViewState`) — removing all `window.click(...)` and `window.try_find(...)` assertions.
- [x] **T0.3**: Replace `crates/speechtt-ui/tests/widget.rs` with a single lightweight render smoke test (`window.render_frame(cx)` across the major states without asserting specific `ElementId`s) so any future layout/component redesign never breaks tests unless rendering actually panics.
- [x] **T0.4**: Remove unnecessary `.test_support()` calls from presentational elements in `crates/speechtt-ui/src/app.rs` and `crates/speechtt-ui/src/components.rs`.

---

## Phase 1 — Layout & Resizing (Window Adaptability)

### Problem
The current UI assumes a fixed `480×280` pixel box:
- Flex column children lack `.min_h_0()`, preventing them from shrinking when the window height decreases.
- `CaptionWell` has a static height based on two lines of text instead of flexing to fill available vertical space and scrolling when needed.
- `Waveform` has a fixed `h(px(36.))` and 36 fixed `3px` bars (~213px wide), staying small when the window widens.
- Form inputs use a hardcoded `.w(px(200.))` (`crates/speechtt-ui/src/app.rs:530`).
- Padding and gaps mix raw `px(20.)`, `px(10.)`, `px(14.)` instead of using relative `rem`/Tailwind scale helpers that respond to `window.rem_size()`.

### Tasks — Phase 1
- [x] **T1.1 — Make root and body containers responsive (`crates/speechtt-ui/src/app.rs`)**:
  - On the main window body container (`app.rs:270`), add `.min_h_0().w_full().overflow_hidden()` and replace `.px(px(20.)).py(px(10.))` with scale helpers (`.px_5().py_2p5()` or token-derived `rems`).
  - Ensure all screen root containers use `.size_full().min_h_0()` so flex children can both grow and shrink with the window.
- [x] **T1.2 — Make `CaptionWell` expand and scroll (`crates/speechtt-ui/src/components.rs`)**:
  - Update `CaptionWell` (`components.rs:198`) to apply `.w_full().flex_1().min_h_0().overflow_y_hidden()` (or scrollable wrapper) so that as the window grows taller, the caption well absorbs the extra vertical space, and as the window shrinks, it does not push the bottom transport buttons off-screen.
- [x] **T1.3 — Make `Waveform` fluid across window widths (`crates/speechtt-ui/src/components.rs`)**:
  - Update `Waveform` (`components.rs:154`) to use `.w_full().min_h(rems(2.0)).max_h(rems(4.0))` and let each bar flex horizontally (`.flex_1().min_w(px(2.)).max_w(px(6.))`) so the waveform naturally spans the width of the container and scales cleanly when resized.
- [x] **T1.4 — Replace fixed `w(px(200.))` on Form Controls (`crates/speechtt-ui/src/app.rs`)**:
  - In `render_form` (`app.rs:528`), remove `.w(px(200.))` on `Input::new(&self.session_name)`.
  - Wrap the right-side control slot of form rows in a flexible container (`.flex_1().min_w(rems(8.0)).max_w(rems(18.0))`) while keeping the left label column `.flex_shrink_0()`, so inputs expand when the window is widened.
- [x] **T1.5 — Replace raw `px(...)` spacing with `rem`/scale helpers**:
  - Audit `app.rs` and `components.rs` for raw `px(...)` used in padding, gaps, and font sizes, replacing them with GPUI scale methods (`.gap_1()`, `.gap_2()`, `.gap_6()`, `.px_3()`, `.py_2()`, `.text_xs()`, `.text_sm()`, `.text_lg()`) or `rems(...)` constants from `tokens.rs`. Keep `px(...)` only for true physical hairlines or radii where required by the token contract.

#### Phase 1 implementation notes
- Typography now uses `rems(tokens::rem::*)`, the rem mirror of the design type scale; `tokens::rem` is guarded by `tests/theme_contract.rs`. Radii and transport-control geometry stay in `px` (token contract / physical component geometry).
- The form label column uses `.flex_shrink_1().min_w_0()` instead of `.flex_shrink_0()`: a long helper line must wrap on narrow windows rather than push the control slot off-screen. The control slot keeps `.flex_1().min_w(8rem).max_w(18rem)` and the input fills it via `.w_full()`.
- The setup form uses `.gap_0p5()` (2px): at the default 480×280 body the four rows + badge + footer fit without clipping (verified at 480×280, 960×640 and 320×190).
- Recording/Paused caption wells absorb vertical surplus; their transport rows are `.flex_shrink_0()` so controls stay pinned at the bottom.
- `tests/widget.rs` renders every screen at 480×280 and at extreme sizes (320×190, 960×640).

---

## Phase 2 — Box Layout Abstractions

### Problem
Every screen method in `app.rs` (`render_prompt`, `render_form`, `render_ready`, `render_recording`, `render_paused`, `render_stopped`) manually reconstructs the same 3-band vertical box layout (top status row, middle content area, bottom action bar) and repeats card styling (`.px(px(14.)).py(px(8.)).rounded(px(radius::MD)).bg(cx.theme().popover)`).

### Target Design
Create `crates/speechtt-ui/src/layout.rs` and export it from `crates/speechtt-ui/src/lib.rs`. Provide two primary `RenderOnce` box abstractions:

#### 1. `ScreenShell`
A 3-zone vertical flex container that guarantees consistent alignment spines across all screens:
```rust
#[derive(IntoElement)]
pub struct ScreenShell {
    status: AnyElement,
    header_trailing: Option<AnyElement>,
    body: Vec<AnyElement>,
    footer: Option<AnyElement>,
    center_body: bool,
}
```
- **Header row (`flex_shrink_0`)**: `h_flex().w_full().justify_between().items_center()`, placing `status` on the leading edge and optional `header_trailing` (e.g., `ElapsedTimer` or `"New take"` button) on the trailing edge.
- **Body zone (`flex_1().min_h_0().w_full()`)**: `v_flex()` that holds the main content. When `.centered()` is called (used by `Prompt` and `Ready`), it applies `.items_center().justify_center()`. Otherwise it stretches children vertically.
- **Footer zone (`flex_shrink_0`)**: Renders the transport controls or form submit buttons at a consistent bottom position.

#### 2. `SurfaceBox`
A reusable themed container for recessed/card wells (used by `CaptionWell`, the `"TRANSCRIPT WRITTEN TO"` file card in `Stopped`, and future Settings sections):
```rust
#[derive(IntoElement)]
pub struct SurfaceBox {
    flex_grow: bool,
    children: Vec<AnyElement>,
}
```
- Applies `.w_full().px_3p5().py_2().gap_1().rounded(px(radius::MD)).bg(cx.theme().popover)`.
- Supports `.flex_1()` (which also sets `.min_h_0()`) and `.child(...)` / `.children(...)`.

### Tasks — Phase 2
- [x] **T2.1**: Create `crates/speechtt-ui/src/layout.rs` implementing `ScreenShell` and `SurfaceBox` as `RenderOnce` components.
- [x] **T2.2**: Refactor `CaptionWell` (`crates/speechtt-ui/src/components.rs`) and the file-path well in `render_stopped` (`crates/speechtt-ui/src/app.rs:758`) to use `SurfaceBox`.
- [x] **T2.3**: Refactor all 6 screen renderers (`render_prompt`, `render_form`, `render_ready`, `render_recording`, `render_paused`, `render_stopped`) to compose their layout through `ScreenShell`.

#### Phase 2 implementation notes
- `ScreenShell` adds `.gap(...)` and `.body_gap(...)` builders because the screens use different rhythms (2px form, 4px transport, 8px ready/stopped). `centered()` covers the prompt and ready bodies.
- `SurfaceBox` adds `.clip()` (caption wells clip overflowing transcript text) alongside `.flex_1()` and `.child(...)`; padding, gap, radius and surface color live in the box, so the caption and the file-path card share one treatment.
- The form keeps its action row inside the body band rather than the shell footer: the four rows plus badge already fill the 480×280 body, and a pinned footer would overlap them. Verified: actions bottom out at y=278 in the 280px window; recording transport stays pinned at the body bottom (y=196–270 at 480×280, 556–630 at 960×640); the caption still absorbs vertical surplus (64.5px → 424.5px).

---

## Phase 3 — Reusable Components & View Decomposition

### Problem
1. `crates/speechtt-ui/src/app.rs` defines UI controls as private helper methods on `SpeechTtApp` (`circle_action`, `primary_circle`, `ghost_control`, `stop_control`, `form_row`, `value_pill`). Because they take `&self` and `&mut Context<SpeechTtApp>`, they cannot be reused in dialogs or other views.
2. Buttons like `"Cancel"`, `"Create session"`, and `"New take"` are built with raw `div()`s instead of `gpui_kit::component::button::Button`, losing keyboard navigation, focus rings, and proper `.disabled(...)` click guarding.
3. `app.rs` is over 800 lines long and mixes all screens and components in one file.

### Target Component Architecture
Convert `crates/speechtt-ui/src/components.rs` into a module directory `crates/speechtt-ui/src/components/` (keeping all existing public exports in `components/mod.rs` so external imports don't break):

```text
crates/speechtt-ui/src/
├── components/
│   ├── mod.rs              # Re-exports all components
│   ├── status_badge.rs     # StatusBadge, BadgeTone
│   ├── elapsed_timer.rs    # ElapsedTimer
│   ├── waveform.rs         # Waveform
│   ├── caption_well.rs     # CaptionWell
│   ├── transport_button.rs # TransportButton (74px/64px circle button) + IconAction (34px square button)
│   ├── setting_row.rs      # SettingRow (uppercase label + help subtitle + right control slot)
│   └── choice_pill.rs      # ChoicePill (selectable pill control for provider/model/output)
```

#### Component Specifications
1. **`TransportButton` (`components/transport_button.rs`)**:
   - Replaces `circle_action` and `primary_circle` (and removes the 8-argument `#[allow(clippy::too_many_arguments)]` method).
   - Variants: `TransportTone::Primary` (74px, brand primary bg, primary hover, on-color fg) and `TransportTone::Warning` (64px, warning bg, on-color fg).
   - Builder API:
     ```rust
     TransportButton::new("btn-record", IconName::Mic)
         .tone(TransportTone::Primary) // default
         .on_click(cx.listener(|this, _ev, _win, cx| this.start_recording(cx)))
     ```
2. **`IconAction` (`components/transport_button.rs`)**:
   - Replaces `ghost_control` and `stop_control`.
   - 34×34px rounded secondary icon button with `.disabled(bool)` support that properly guards `.hover(...)`, `.cursor_pointer()`, and `.on_click(...)`.
   - Builder API:
     ```rust
     IconAction::new("btn-stop", IconName::Square)
         .disabled(!enabled)
         .on_click(cx.listener(|this, _ev, _win, cx| this.finish_take(cx)))
     ```
3. **`SettingRow` (`components/setting_row.rs`)**:
   - Replaces `SpeechTtApp::form_row`.
   - Takes `label: impl Into<SharedString>`, `description: impl Into<SharedString>`, and `control: impl IntoElement`.
   - Reusable across both the setup form and the upcoming Settings dialog.
4. **`ChoicePill` (`components/choice_pill.rs`)**:
   - Replaces `SpeechTtApp::value_pill`.
   - Builder API:
     ```rust
     ChoicePill::new("field-provider", provider_label)
         .selected(has_provider)
         .on_click(cx.listener(|this, _ev, _win, cx| this.cycle_provider(cx)))
     ```
5. **Use `gpui_kit::component::button::Button` for standard text buttons**:
   - Replace the raw `div()`s for `"Cancel"`, `"Create session"`, and `"New take"` with `Button::new(...)` using `ButtonVariants` (`.primary()`, `.secondary()` / `.ghost()`), `Sizable` (`.small()`), and `Disableable` (`.disabled(!can_create)`).

### Tasks — Phase 3
- [x] **T3.1**: Convert `crates/speechtt-ui/src/components.rs` into `crates/speechtt-ui/src/components/mod.rs` and split existing components into `status_badge.rs`, `elapsed_timer.rs`, `waveform.rs`, and `caption_well.rs`.
- [x] **T3.2**: Implement `TransportButton` and `IconAction` in `crates/speechtt-ui/src/components/transport_button.rs` and delete `circle_action`, `primary_circle`, `ghost_control`, and `stop_control` from `app.rs`.
- [x] **T3.3**: Implement `SettingRow` in `crates/speechtt-ui/src/components/setting_row.rs` and `ChoicePill` in `crates/speechtt-ui/src/components/choice_pill.rs`, and delete `form_row` and `value_pill` from `app.rs`.
- [x] **T3.4**: Replace raw `div()` text buttons (`btn-cancel-session`, `btn-create-session`, `btn-new-take`) in `app.rs` with `gpui_kit::component::button::Button`.
- [x] **T3.5**: Split the screen rendering methods out of `crates/speechtt-ui/src/app.rs` into focused submodules under `crates/speechtt-ui/src/views/` (`setup.rs`, `transport.rs`, `stopped.rs`) so `app.rs` only owns top-level state, actions, and window shell composition.

#### Phase 3 implementation notes
- `components/` keeps the same public names through `mod.rs` re-exports, so `crate::components::{…}` imports did not change at call sites.
- `TransportButton`, `IconAction` and `ChoicePill` take a boxed `Fn(&ClickEvent, &mut Window, &mut App)` (fed by `cx.listener(...)`), so they hold no application state and can be reused by dialogs. `IconAction::disabled(true)` drops its cursor, hover and click behavior entirely.
- `TransportButton` reads `primary`/`warning` semantic theme roles; the previous raw warning/on-color literals are gone.
- `SettingRow` owns the label/help/control-slot layout once, so the setup form and the upcoming Settings dialog share it. The setup screen now builds pills with `ChoicePill` and text actions with gpui-kit `Button` (which supplies focus, keyboard activation and real disabled handling).
- `views/setup.rs`, `views/transport.rs` and `views/stopped.rs` receive `&mut SpeechTtApp`/`Context` and call its public commands. `app.rs` keeps the struct, input subscription, command/query methods, header strip and `ViewState` dispatch; it is now ~260 lines.
- **Finding while wiring the new controls:** headless `window.input(...)` updates the `InputState` but does not deliver `InputEvent::Change` to entity subscribers in this gpui-kit version. The setup form therefore computes `can_create` with `SessionController::is_draft_complete_with_name(input.value())`, and `SpeechTtApp::create_session` syncs the input value into the controller before committing. A pure unit test covers the new check; live typing still triggers a re-render through the retained subscription when the platform delivers change events.

---

## Phase 4 — Rust Idioms & Performance (`rust-code-review`)

### Problem
1. `SessionError` in `crates/speechtt-core/src/session.rs` does not implement `std::error::Error` or `Display`.
2. Public structs `SessionConfig` and `FinishedSession` in `speechtt-core` have `pub` fields without `#[non_exhaustive]`.
3. `SpeechTtApp::create_session` (`crates/speechtt-ui/src/app.rs:186-203`) checks `self.draft_complete(cx)` and then calls `.expect("checked by draft_complete")` three times, violating the *"Make invalid states unrepresentable"* principle.
4. `CaptionWell`, `StatusBadge`, and `Waveform` allocate `String` or `Vec<f32>` on every frame during `render`.

### Tasks — Phase 4
- [x] **T4.1 — Implement `Error` and `Display` for `SessionError` (`crates/speechtt-core`)**:
  - Add `thiserror` to `crates/speechtt-core/Cargo.toml` (or implement `std::fmt::Display` + `std::error::Error`) on `SessionError` with clear error messages.
  - Add `#[must_use]` to `SessionState::is_active`, `SessionConfig::missing`, `SessionConfig::is_complete`, and `SessionMachine` getters.
- [x] **T4.2 — Future-proof public structs (`crates/speechtt-core/src/session.rs`)**:
  - Annotate `SessionConfig`, `FinishedSession`, and `SessionError` appropriately, or provide reader/constructor methods so downstream crates cannot break when new fields are added.
- [x] **T4.3 — Eliminate `.expect()` in `create_session` (`crates/speechtt-ui/src/app.rs`)**:
  - Replace the coupled boolean `draft_complete(&self)` + `.expect(...)` pattern with a method returning `Option<ValidatedDraft>`:
    ```rust
    struct ValidatedDraft {
        provider: &'static str,
        model: &'static str,
        session_name: String,
        output: OutputTarget,
    }
    ```
    In `create_session`, use `let Some(draft) = self.validated_draft(cx) else { return; };` and derive `can_create` from `self.validated_draft(cx).is_some()`.
- [x] **T4.4 — Zero-allocation `RenderOnce` props (`crates/speechtt-ui/src/components/`)**:
  - Change `CaptionWell` fields from `String` to `gpui_kit::SharedString` (`pub fn new(committed: impl Into<SharedString>, live: impl Into<SharedString>)`) and store `SharedString` in `FinishedTake` so rendering never clones heap `String`s.
  - Change `StatusBadge::label` from `&'static str` to `SharedString` (`impl Into<SharedString>`) for flexibility without allocation on static strings.
  - Change `Waveform` to accept a borrowed slice `&'static [f32]` or `Arc<[f32]>` / `SmallVec` instead of allocating a `Vec<f32>` on every frame.
- [x] **T4.5 — Quality Gate Verification**:
  - Run `cargo fmt -- --check`
  - Run `cargo clippy --all-targets -- -D warnings`
  - Run `cargo test` across the workspace and verify all unit and smoke tests pass with zero warnings.

#### Phase 4 implementation notes
- `SessionError` now derives `thiserror::Error` with messages such as `session is not ready: still missing [Provider, Model]`; `SessionConfig`, `FinishedSession` and `SessionError` are `#[non_exhaustive]` so new fields/variants are additive.
- `SessionDraft::validate()` returns `Option<ValidatedDraft>`; `create_session` early-returns on `None`, so the former `.expect("checked by draft_complete")` calls are gone. The UI's `can_create` still comes from a name-aware check because the input value may not have reached the controller yet.
- `CaptionWell` and `StatusBadge` hold `SharedString`, and `FinishedTake` already did — cloning them in `render` is a refcount bump. `Waveform` borrows `&'static [f32]` (`Waveform::new(&MOCK_BARS, …)`), so a frame allocates no bar buffer. A comment notes `Arc<[f32]>` as the path for live FFT bins.
