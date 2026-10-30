# T0018 — Three-size typography scale and round-button polish

- **Track**: Rust UI + design system (cross-cutting)
- **Wave**: 1 (independent — no contract change outside `speechtt-ui`)
- **Depends on**: T0017 (mode chooser, `Card`, `RoundButton`)
- **Blocks**: —
- **Owner**: unassigned
- **Started**: —
- **References**: `mockups/app-window/welcome.png` (visual target),
  `mockups/app-window/new-session.png`, `DESIGN.md` § Components,
  `AGENTS.md` §5 (no hardcoded type values in views)

## Context & Goal

Type is currently chosen per call site: `rem::XS`, `rem::SM` and `rem::MD` are
picked ad-hoc across `views/` and `components/`, so the same kind of copy
renders at different sizes on different screens. The mode chooser shows the
symptom: its card titles and descriptions are too small next to the mockup,
and the sizes are baked into the view instead of being a component.

The round buttons have three defects: the two chooser buttons are not aligned
(the description above them wraps to a different number of lines), the click
ripple never fires, and the icon colour/hover treatment does not match the
design.

Goal: exactly three text roles in the whole application, rendered by three
components that views must use; chooser cards laid out so their buttons are
pinned to the bottom next to their caption; round buttons with white icons, an
active-colour hover and outline, and a working ripple.

## Scope Boundary

May create or modify:

- `crates/speechtt-ui/src/components/text.rs` (new: `Title`, `Body`, `Label`)
- `crates/speechtt-ui/src/components/mod.rs`
- `crates/speechtt-ui/src/components/card.rs`
- `crates/speechtt-ui/src/components/round_button.rs`
- `crates/speechtt-ui/src/components/{app_header,status_badge,setting_row,key_cap,caption_well,elapsed_timer,action_button,icon_action,transcription_mode_badge}.rs`
- `crates/speechtt-ui/src/views/setup.rs`, `views/transport.rs`, `views/stopped.rs`
- `crates/speechtt-ui/src/views/dialogs/{help,settings,window,mod}.rs`
- `crates/speechtt-ui/src/app.rs` (error strip copy + `pulse` call sites)
- `crates/speechtt-ui/src/tokens.rs` (`MIN_WINDOW_WIDTH` if the cards grow)
- `crates/speechtt-ui/tests/*` (only where a behaviour changes)
- `DESIGN.md`

Do **not** touch `crates/speechtt-core`, `src/stt`, or any `.ts`/`.tsx` file.

## Contract

### Text roles

```rust
pub enum TextRole { Title, Body, Label }

pub struct Title { .. }   // --font-size-lg   (20 px), semibold
pub struct Body { .. }    // --font-size-md   (16 px), regular/medium
pub struct Label { .. }   // --font-size-sm   (12.8 px), medium
```

- Each component takes one string (or child) plus optional `.muted()` and
  `.tone(Hsla)`; size and weight come from the component, never the caller.
- No view or component may contain `.text_size(rems(..))` after this ticket,
  except `text.rs` itself and `tokens.rs`. `TextRole` covers every call site
  today: badge/status/chip copy is `Label`, card copy is `Title` + `Body`,
  transport captions are `Label`, dialog rows are `Body`.
- `XS` disappears from the UI: former `XS` copy becomes `Label`.

### Round buttons

- Icon colour is `--color-on-primary` ink on the `Primary` fill; on
  `Secondary` it is `text` (`#f2efeb`). Neither is tinted by tone.
- Hover changes the fill to the active variant (`primary_active` /
  `secondary_active`) and adds a 2 px outline in `brand.primary`.
- One click starts a circular ring expanding from the button edge to
  `RIPPLE_MAX_SIZE` while fading out over `RIPPLE_DURATION`; the ring restarts
  on every click and never affects layout.
- `SpeechTtApp::pulse(id)` is called by the real click path of every round
  button (chooser cards, record, pause, resume, browse) so `pulse_count`
  increments in the running app, not only when a test calls `pulse` itself.

### Chooser cards

- Title, description, then a spacer, then the round button and its caption
  with the caption directly under the button (`--space-sm`), so the button is
  pinned to the card bottom and both cards' buttons share one baseline.
- Title and description grow one step up the scale (title `Title`, description
  `Body`), keeping both descriptions on two lines at the default width.

## TDD Tasks

- [ ] **T18.1 — Red: text roles**: `text.rs` unit tests assert the rendered
  size comes from the role (not the caller), and `.muted()` uses the muted
  token. Include a `Title`/`Body`/`Label` size table asserting they match
  `tokens::font::{LG, MD, SM}`.
- [ ] **T18.2 — Red: no ad-hoc type**: add a source scan test in
  `tests/` that fails while any file under `src/` other than `text.rs` and
  `tokens.rs` contains `.text_size(rems(` or `.text_size(px(`.
- [ ] **T18.3 — Red: ripple is wired**: drive a real click through the
  chooser card and assert `pulse_count` moves *because of the click*; delete
  the current test that calls `pulse` directly.
- [ ] **T18.4 — Red: ripple geometry**: keep `ripple_frame` tests (expanding,
  bounded, fading, clamped) as the pure spec for the ring.
- [ ] **T18.5 — Implement `text.rs`** and migrate every call site to
  `Title`/`Body`/`Label`; no visual value may be passed from a view.
- [ ] **T18.6 — Implement the `RoundButton` treatment**: white icon, active
  hover fill + `brand.primary` outline, working ring started by the click.
- [ ] **T18.7 — Wire `pulse`**: every round-button click handler bumps its
  counter before running its command.
- [ ] **T18.8 — Re-layout the chooser cards**: bottom-pinned button, caption
  `--space-sm` below it, `Title`/`Body` copy, cards still centered as a pair.
- [ ] **T18.9 — Gates**: `cargo fmt -- --check`,
  `cargo clippy --all-targets -- -D warnings`, `cargo test`, `bun test`,
  `bun check src/stt`, `bun run design:validate`.
- [ ] **T18.10 — Manual QA**: screenshot the chooser at 600x460 and compare
  with `mockups/app-window/welcome.png`; click both cards and confirm the ring
  is visible; hover both buttons and confirm the active fill + outline.

## Acceptance Criteria

1. The application uses exactly three text sizes: `Title` (lg), `Body` (md)
   and `Label` (sm), each rendered by its own component.
2. No view or component contains a literal font size; the source-scan test
   enforces it.
3. The chooser's two round buttons are bottom-aligned on one baseline and sit
   `--space-sm` above their caption.
4. Card titles use `Title` and descriptions use `Body`; both descriptions stay
   on two lines at 600 px width.
5. Round-button icons are always white (ink on the primary fill never tinted).
6. Hovering a round button switches it to the active colour and shows an
   outline.
7. Clicking a round button in the running application shows one expanding,
   fading ring — verified by a click-driven test, not by calling `pulse`.
8. All gates green with zero warnings.
