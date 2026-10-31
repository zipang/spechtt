# T0018 — Three-size typography scale and round-button polish

- **Track**: Rust UI + design system (cross-cutting)
- **Wave**: 1 (independent — no contract change outside `speechtt-ui`)
- **Depends on**: T0017 (mode chooser, `Card`, `RoundButton`), T0019 (one view per file)
- **Blocks**: —
- **Owner**: CTO (sub-agent quotas exhausted), supervised by the board owner
- **Started**: 2026-10-30
- **References**: `mockups/app-window/welcome.png` (visual target),
  `mockups/app-window/new-session.png`, `DESIGN.md` § Components,
  `AGENTS.md` §2 (component rule) and §5

## Context & Goal

Type was chosen per call site: `rem::XS`, `rem::SM` and `rem::MD` were picked
ad-hoc across `views/` and `components/`, so the same kind of copy rendered at
different sizes on different screens. The mode chooser showed the symptom: its
card titles and descriptions were too small next to the mockup, and the sizes
were baked into the view instead of being a component.

The round buttons had three defects: the two chooser buttons were not aligned
(the description above them wraps to a different number of lines), the click
ripple never fired, and the icon colour/hover treatment did not match the
design.

Goal: exactly three text roles in the whole application, rendered by three
components that views must use; chooser cards laid out so their buttons are
pinned to the bottom next to their caption; round buttons with white icons, an
active-colour hover and outline, and a working press wave.

## Scope Boundary

May create or modify:

- `crates/speechtt-ui/src/components/text.rs` (new: `Title`, `Body`, `Label`)
- `crates/speechtt-ui/src/components/{mod,card,round_button,ripple,button_anchors,status_dot,info_note}.rs`
- `crates/speechtt-ui/src/components/{app_header,status_badge,setting_row,key_cap,caption_well,elapsed_timer,action_button,icon_action,transport_header}.rs`
- `crates/speechtt-ui/src/components/{status_badge,transcription_mode_badge}.rs`
- `crates/speechtt-ui/src/views/{welcome,session_setup,ready,recording,paused,stopped}.rs`
- `crates/speechtt-ui/src/views/dialogs/{help,settings,window,mod}.rs`
- `crates/speechtt-ui/src/app.rs` (error strip copy + press wiring)
- `crates/speechtt-ui/tests/design_system.rs` (new: the design-system gate)
- `crates/speechtt-ui/tests/widget.rs` (wave wiring)
- `AGENTS.md`, `DESIGN.md`

Do **not** touch `crates/speechtt-core`, `src/stt`, or any `.ts`/`.tsx` file.

## Contract

### Text roles

```rust
pub enum TextRole { Title, Body, Label }

pub struct Title { .. }   // --font-size-lg   (20 px), semibold
pub struct Body { .. }    // --font-size-md   (16 px), regular
pub struct Label { .. }   // --font-size-sm   (12.8 px), medium
```

- Each component takes one string plus optional `.muted()`, `.tone(Hsla)`,
  `.mono()`, `.bold()` (one weight step above the role's own) and `.center()`.
  Size and weight come from the role, never from the caller.
- `Title`/`Body`/`Label` are the whole scale. `.text_size`/`.font_weight` are
  allowed only in `components/text.rs` and `tokens.rs`;
  `tests/design_system.rs` scans the source and fails the gate otherwise, per
  `AGENTS.md` §2 (component rule).
- A view may not choose spacing, type or radius values either: no `px(`,
  `rems(` or `radius::` under `src/views/`.
- `XS` disappears from the UI: former `XS` copy becomes `Label`.

### Round buttons

- The glyph is `--color-text` on both tones; a tone never tints the icon.
- A 2 px outline is always present, so hover only recolours it (tone or
  `brand.secondary` at rest → `brand.primary`) and swaps the fill to the tone's
  active variant. Hovering never shifts layout.
- The button records its bounds and tone in `components::button_anchors` while
  prepainting, so the press wave can be concentric with it.
- `RoundButton` and `Card` expose `on_press`, which fires on mouse-down.

### Press wave

- A press spreads one filled circle in the **button's own colour**, starting at
  the **button's own diameter and centre**.
- It grows by `RIPPLE_TRAVEL` (20 px) while its opacity falls from 1 to 0 over
  exactly that travel, over `RIPPLE_DURATION`. It never affects layout.
- It is painted at the **window root, above the screen**: being the button's
  colour, the overlapping part is invisible and only the sliver past the button
  edge reads, so the wave emerges from behind the button face.
- The window owns it, so it survives the navigation every round action triggers
  (chooser → form, Ready → Recording, Pause → Paused).

### Chooser cards

- Title (`Title`), description (`Body`, muted), then a spacer, then the round
  button with its `Label` caption `--space-sm` beneath, pinned to the card
  bottom so both cards' buttons share one baseline.
- The screen title, description and the card pair share one left spine; cards
  stay pinned to `MODE_CARD_WIDTH`, and both descriptions stay on two lines at
  the default width.

## TDD Tasks

- [x] **T18.1 — Text roles spec**: unit tests assert each role's size and
  weight come from the role, the three sizes mirror
  `tokens::font::{LG, MD, SM}`, and the builders never change the role.
- [x] **T18.2 — Design-system gate**: `tests/design_system.rs` fails while any
  file outside `components/text.rs`/`tokens.rs` contains `.text_size(` or
  `.font_weight(`, and while a view contains `px(`/`rems(`/`radius::`.
- [x] **T18.3 — Wave wiring spec**: pressing a real card (at a point *outside*
  the button) starts a wave whose coordinates equal the button's centre, laid
  out at the start diameter, still painting after the click switches screens.
- [x] **T18.4 — Wave geometry spec**: `ripple_frame` starts at the button
  diameter, ends at `+RIPPLE_TRAVEL`, fades 1 → 0, and clamps out-of-range
  deltas.
- [x] **T18.5 — Implement `text.rs`** and migrate every call site to
  `Title`/`Body`/`Label`.
- [x] **T18.6 — Round-button treatment**: ink glyph, always-present outline,
  active-variant hover.
- [x] **T18.7 — Press wave**: `components::{ripple,button_anchors}` plus the
  `on_press` wiring in every view.
- [x] **T18.8 — Re-layout the chooser cards**: bottom-pinned button, caption
  `--space-sm` below, `Title`/`Body` copy, one left spine with the title.
- [x] **T18.9 — Gates**: `cargo fmt -- --check`,
  `cargo clippy --all-targets -- -D warnings`, `cargo test`, `bun test`,
  `bun check src/stt`, `bun run design:validate`.
- [x] **T18.10 — Manual QA**: chooser at 600×460 shows larger titles and both
  round buttons on one baseline; a press paints the wave at the button edge.

## Acceptance Criteria

1. The application uses exactly three text sizes — `Title` (lg), `Body` (md),
   `Label` (sm) — each rendered by its own component.
2. No view or component contains a literal font size or weight; the source-scan
   gate enforces it, and views choose no spacing/radius either.
3. The chooser's two round buttons sit on one baseline with their captions
   `--space-sm` beneath.
4. Card titles use `Title` and descriptions `Body`; both descriptions stay on
   two lines at 600 px width.
5. Round-button glyphs are always `--color-text`; hover switches to the active
   fill and shows a `brand.primary` outline.
6. Pressing a round button paints one wave in the button's colour, concentric
   with the button, from the button diameter outward 20 px while fading out.
7. All gates green with zero warnings.

## Implementation Notes

### Text scale as implemented

`components/text.rs` owns `TextRole {Title, Body, Label}` and a private `Text`
element. Three public wrappers forward the same builders:

```rust
Title::new("Choose a mode")
Body::new(description).muted().center()
Label::new("Start").bold().mono().tone(hsla)
```

Builders: `.muted()`, `.tone(Hsla)`, `.mono()`, `.bold()` (one step above the
role's weight), `.center()`. There is no public size or weight setter —
`TextRole::size()`/`weight()`/`bold_weight()` are the only source. `TextRole`
is `Copy + PartialEq`, so tests can assert which role a component carries.

### Call sites migrated

Every `text_size`/`font_weight` under `crates/speechtt-ui/src` is gone; the grep
returns nothing outside `components/text.rs`. Migrated surfaces: `status_badge`,
`setting_row`, `key_cap`, `caption_well`, `elapsed_timer`,
`transcription_mode_badge`, `app_header`, `transport_header`, the welcome /
session-setup / ready / stopped screens, both dialogs and the app error strip.
Two new components replaced markup a view was styling by hand: `StatusDot` (the
6 px state dot) and `InfoNote` (the recessed info row in Settings), which is what
let `views/dialogs/settings.rs` drop its last `px`/`radius` use.

### The design-system gate

`crates/speechtt-ui/tests/design_system.rs` (3 tests):
1. `only_the_text_scale_sets_font_size_or_weight` — no `.text_size(` or
   `.font_weight(` under `src/` except `components/text.rs` and `tokens.rs`;
   lists file:line offenders.
2. `views_do_not_choose_spacing_type_or_radius` — no `px(`, `rems(` or
   `radius::` under `src/views/`.
3. `the_type_scale_has_exactly_three_ordered_roles` — the roles are ordered and
   match `tokens::font::{LG, MD, SM}`.

### Round buttons

- Glyph ink is `theme.foreground` for both tones (never the tone colour).
- A 2 px outline is always present, so hover only recolours it
  (tone/border → `brand.primary`) and swaps the fill to the active variant; no
  layout shift on hover.
- `RoundButton` and `Card` gained `on_press` (mouse-down), and the button
  records its bounds + tone in `components::button_anchors` while prepainting.

### The press wave — three bugs, not one

Each was found by proving the *frame*, not a counter:

1. **Nothing painted.** The old `pulse()` bumped a counter but never called
   `cx.notify()`, so no frame was produced. The old test called `pulse` itself,
   so it could not notice.
2. **The circle never grew.** The animated element was a flex child of the
   button container; `flex-shrink: 1` squeezed every frame back to 56 px.
   `flex_none()` fixed it.
3. **The wave died on release.** Every round action navigates, so a wave owned
   by the pressed screen was destroyed the instant the click changed screens.

Final design (`components::ripple`): a filled circle in the **button's own
colour**, starting at the **button's diameter and centre**, growing by
`RIPPLE_TRAVEL` (20 px) while its opacity falls 1 → 0 over exactly that travel
(`ripple_frame`). It is drawn at the **window root, above the screen** — being
the button's colour, the overlap is invisible and only the sliver past the
button edge shows, so it reads as emerging from behind the button face. The
window owns it, so it survives navigation; its centre comes from the anchor
recorded at prepaint, so it is concentric with the button even when the press
lands near the card's corner.

`ripple_frame` is the pure spec. The widget test owns the wiring: a press
produces a wave at the button's centre coordinates, at the start diameter, still
painting after the click switches screens. Growth over time is deliberately
*not* asserted there — `Animation` advances on wall-clock time, which would make
the test flaky.

### Welcome cards

`Title` + muted `Body` copy, then a `flex_1 justify_end` block holding the round
button with its `Label` caption `--space-sm` beneath, so both buttons share one
baseline. Cards stay pinned to `MODE_CARD_WIDTH` (272). The screen title, its
description and the card pair are left-aligned on one spine (`items_start` /
`justify_start`) per the mockup. The window minimum is 600×378 — the smallest
box that still renders the chooser intact; below it the cards are truncated and
overlap the description. It is enforced three ways: the min-size hint, the
restore clamp, and a runtime clamp in `SpeechTtApp::observe_window_bounds`,
which resizes the window back up (and records the clamped size) because XFCE
ignores the hint. That also stops a too-small geometry from being persisted —
and then silently dropped by `Settings::normalize_window_layout`.

### Gates

- `cargo fmt -- --check` — clean.
- `cargo clippy --all-targets -- -D warnings` — clean, zero warnings.
- `cargo test` — 331 passed (14 suites), 0 failed.
- `bun test` — 206 passed, 0 failed.
- `bun check src/stt` — no type errors (23 files).
- `bun run design:validate` — 66 tokens, 0 errors.
- Manual QA: chooser at 600×460 with a throwaway `XDG_CONFIG_HOME`; larger
  titles/descriptions, both buttons on one baseline, captions beneath, and a
  press paints the wave at the button edge.

### Deliberate deviation

The ticket originally described a *ring* growing 56 → 84 px. The final shape is
a solid disc in the button's own colour, starting at the button diameter and
travelling 20 px, per the CEO's direction: simpler, and it reads as the button
surface itself rippling. T18.4's pure geometry test was rewritten to the new
contract (start = `ROUND_BUTTON_SIZE`, end = start + `RIPPLE_TRAVEL`, opacity
1 → 0).
