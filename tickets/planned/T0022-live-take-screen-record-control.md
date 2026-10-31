# T0022 — Live take screen: microphone record control, cancel and reset

- **Track**: Rust UI + Rust core + design system
- **Wave**: 2 (integration)
- **Depends on**: T0021 (shared footer, `ViewState::Live`), T0020 (empty takes)
- **Blocks**: nothing
- **References**: `DESIGN.md` §Round button / §Press wave / §Record & transport
  controls, `AGENTS.md` §2 (component rule), CEO decision of this ticket:
  *"the record button should feature a microphone icon, red and pulsating with
  waves when recording, black and inert when not recording."* — where "red" is
  the existing `brand.primary` Agfa orange, and "black" is the existing
  `Secondary` round-button surface. **No new colour is introduced.**

## Context & Goal

The take screen borrowed the iconography of a media player: pause while
recording, play while paused, a separate square Stop. That is the language of
*playback*, not of *capture*. The user has to read two buttons to understand one
state, and Paused is a whole screen away from Recording even though nothing
about the layout changes.

The screen also has no escape hatch. Once a take is open the user cannot go
home, and recording a microphone check leaves a take they cannot clear without
finishing and discarding it downstream.

**Done means:** the take screen is one screen with one on/off control —
microphone glyph, `brand.primary` and pulsing while capturing, dark and still
while not — plus Cancel, Reset and Stop in the shared footer. Idle, recording
and paused are three states of that one screen, not three screens.

## Scope Boundary

May create or modify:

- `crates/speechtt-ui/src/components/record_control.rs` — **new**: the mic
  on/off control, pulse and repeating waves.
- `crates/speechtt-ui/src/components/round_button.rs` — expose the existing
  tone fill and the pure wave/pulse geometry the control reuses.
- `crates/speechtt-ui/src/components/mod.rs` — **append-only** registration.
- `crates/speechtt-ui/src/components/stop_control.rs` — re-point at the new
  enabled rule, or delete it if `IconAction` covers the footer's Stop.
- `crates/speechtt-ui/src/views/recording.rs` — the one live take screen.
- `crates/speechtt-ui/src/views/paused.rs` — **delete** (merged).
- `crates/speechtt-ui/src/controller.rs` — idle/recording/paused queries,
  `reset_take()`, `cancel_take()`, capture gating.
- `crates/speechtt-ui/src/app.rs` — capture start/stop wiring for the new
  controls, `reset` handler.
- `crates/speechtt-core/src/session.rs` — `discard_take()` transition.
- `crates/speechtt-ui/src/lib.rs` — **append-only** icon registration
  (`RotateCcw`).
- `crates/speechtt-ui/tests/{widget,session_flow,end_to_end,design_system}.rs`,
  `crates/speechtt-core/src/session.rs` unit tests.
- `DESIGN.md` — the rewritten record-control section only (it documents the
  reuse of the two existing tones; it introduces nothing).
- `tickets/README.md` — board row.

Do **not** touch: `design-tokens.css`, `crates/speechtt-ui/src/tokens.rs`,
`crates/speechtt-ui/src/theme.rs`, the worker, providers, the file-decoding path,
settings, the dialogs, `docs`/`PRODUCT.md`, the press-wave geometry contract
(only extend it), or `views/welcome.rs`, `views/session_setup.rs`,
`views/stopped.rs` beyond what T0021 already established.
No new screen, no new colour, no new round-button tone, no new font size, weight
or spacing token. No layout, position or animation-snapshot test.

## Contract

### 1. One screen for the whole live take

`views/recording.rs` renders `ViewState::Live` in all three take states.
`views/paused.rs` is deleted; there is no `ViewState::Paused`.

| Take state | Status badge | Waveform |
|---|---|---|
| idle (no take open) | `READY`, muted dot | idle bars, still |
| recording | `RECORDING`, primary dot pulsing | live FFT bars |
| paused | `PAUSED`, warning dot, static | flat, low, secondary colour |

The badge and waveform carry the state, exactly as `DESIGN.md` requires —
colour is never the only signal.

### 2. `components::RecordControl`

```rust
pub enum RecordState { Off, On }

pub struct RecordControl { … }

impl RecordControl {
    pub fn new(state: RecordState) -> Self;
    pub fn on_click(self, handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self;
    pub fn disabled(self, disabled: bool) -> Self;
}
```

- Always `IconName::Mic`. Never a play, pause or square glyph.
- The control is a [`RoundButton`] at the one 56 px diameter; it adds the pulse
  and the repeating wave, and picks no colour of its own.
- **`Off`** — the existing `RoundTone::Secondary` (surface fill, `brand.secondary`
  outline, `--color-text` glyph), still. No pulse, no wave, no shimmer. "Inert"
  means *visually at rest*: the control still accepts a click, because pressing
  it is how a take starts and how a paused take resumes.
- **`On`** — the existing `RoundTone::Primary` (`brand.primary` fill and
  outline, `--color-text` glyph), plus two continuous animations:
  - a **pulse** of the disc's scale (1.00 → 1.06 → 1.00) over
    `RECORD_PULSE_PERIOD` (1.6 s), repeating;
  - one **ring wave** per period, starting at the button diameter and expanding
    to `RECORD_WAVE_MAX_SIZE` while fading 1 → 0, in `brand.primary` — the
    button's own colour, exactly like the existing press wave.
- Both animations start on the transition to `On` and stop on the transition to
  `Off`; a paused take is `Off` and therefore completely still.
- The existing press wave (`RippleRing` at the window root) is unchanged: a
  click still emits one press wave in the button's own colour.
- Pure geometry, unit-tested, no rendering assertions:

```rust
pub const RECORD_PULSE_PERIOD: Duration;
pub const RECORD_WAVE_MAX_SIZE: f32;
#[must_use] pub fn pulse_scale(phase: f32) -> f32;              // phase 0..=1 → 1.0..=1.06..=1.0
#[must_use] pub fn record_wave_frame(phase: f32) -> (f32, f32); // (diameter, opacity)
```

  Phase outside `0..=1` clamps, exactly like `ripple_frame`.

### 3. Footer actions

Uses T0021's `ScreenFooter`:

| Zone | Control | Behaviour |
|---|---|---|
| Leading | `IconAction` `ArrowBigLeft`, label "Cancel" | Stops capture, discards the take if one is open, `change_mode()` → mode chooser |
| Primary | `RecordControl` | `Off` in idle and paused → starts capture / resumes; `On` while recording → pauses |
| Trailing | `IconAction` `RotateCcw`, label "Reset" | `reset_take()`: clears transcript, elapsed and waveform and returns the screen to idle **on the same screen**. Disabled in idle and for a file take |
| Trailing | `IconAction` `Square`, label "Stop" | `finish_take()` → stopped screen. Enabled whenever a take is open (recording **or** paused) |

- Cancel is always enabled, including mid-take.
- Stop no longer requires a pause first: with the mic owning pause/resume, Stop
  is the only way to end the take and must be reachable while recording.
- A **file take** renders the same screen with: mic disabled (a decode cannot
  be paused), Reset disabled, Stop and Cancel enabled. Its progress and
  transcript are unchanged.
- Keyboard shortcuts keep their existing meanings (`StartRecording`,
  `PauseRecording`, `ResumeRecording`, `StopRecording`); on the idle Live
  screen `StartRecording` starts capture exactly like the mic press.

### 4. Reset semantics (`controller` + `core`)

- `SessionMachine::discard_take() -> Result<(), SessionError>`:
  - from `Recording` or `Paused`: drop the take, **keep** provider/model/audio
    source, return to `Ready`;
  - from `Blank` or `Ready`: no-op success.
- `SessionController::reset_take() -> bool`:
  - returns `false` (and changes nothing) when no take is open or when the take
    is a file take;
  - otherwise sends `SttCommand::Stop` to the worker, ignores any late event of
    the discarded session, clears transcript, live tail, waveform, elapsed,
    progress and output status, and lands on the idle Live screen with the
    configuration intact;
  - reports no error and sets no output status;
  - the reset take's transcript can never reach Copy/Save: the finished result
    stays `None`.
- `SessionController::cancel_take()`: `discard_take()` when a take is open (for
  a file take, also abort the decode via `SttCommand::Stop`), then the existing
  `change_mode()`.
- Capture is stopped on reset and on cancel; a reset followed by a mic press
  starts a fresh session with a new `session_id`.

### 5. Design system

- **No token, theme role or round-button tone is added.** The two states map
  onto the two tones `DESIGN.md` already defines:
  - `On` ⇒ `RoundTone::Primary` — `--color-brand-primary` fill and outline,
    `--color-text` glyph. That brand orange *is* the "red" that was asked for;
    no palette entry is added.
  - `Off` ⇒ `RoundTone::Secondary` — `--color-surface` fill,
    `--color-brand-secondary` outline, `--color-text` glyph. That near-black
    resting face *is* the "black and inert" look; no palette entry is added.
- `DESIGN.md` §Record & transport controls is rewritten: microphone glyph in
  both states, on/off reading, pulse and repeating waves while recording, and
  the three footer actions. The Paused-screen paragraph becomes a take-state
  paragraph on the one screen. §Round button stays exactly as written (two
  tones), and the new paragraph states explicitly that the record control
  reuses them.
- `design-tokens.css`, `tokens.rs` and `theme.rs` are untouched, so
  `tests/theme_contract.rs` and `bun run design:validate` pass unchanged.
- No component outside `record_control.rs` picks a colour; views still pass no
  `px(..)`, `rems(..)` or `radius::`.

## TDD Tasks

- [ ] **T22.1 — Red first: take states.** Controller tests: a complete live
      configuration is `Live` in idle; the mic press starts a take
      (`SessionState::Recording`, capture requested); a second press pauses
      (`Paused`); a third resumes; Stop from either state yields `Stopped`.
      No `ViewState::Paused` remains anywhere.
- [ ] **T22.2 — Red first: reset and cancel (core).** `discard_take()` returns
      to `Ready` keeping the configuration, refuses nothing, and is a no-op when
      idle. Controller: `reset_take()` clears transcript/elapsed/waveform,
      sends exactly one `Stop`, keeps the configuration and reports no error;
      `reset_take()` is refused for a file take and when idle.
- [ ] **T22.3 — Red first: cancel.** `cancel_take()` from recording and from
      paused ends on `ViewState::Mode`, sends exactly one `Stop`, stops capture,
      and leaves no finished take to copy or save.
- [ ] **T22.4 — Pure geometry specs.** `pulse_scale` at phases `0`, `0.5`, `1`
      and out-of-range clamps; `record_wave_frame` start diameter, monotonic
      growth, opacity 1 → 0, clamping; period and max-size constants pinned.
- [ ] **T22.5 — `RecordControl`.** Implement the component on top of the
      existing `RoundTone::Primary` / `RoundTone::Secondary` fills — no new tone,
      no new token. An `Off` instance renders no animation, an `On` instance
      renders pulse + wave (asserted through the pure functions and the
      component's state, never through pixels).
- [ ] **T22.6 — Rebuild `views/recording.rs`** with the footer table above,
      delete `views/paused.rs`, and wire capture start/stop in `app.rs`
      (reset stops capture; resume restarts it; cancel stops it).
- [ ] **T22.7 — Document, do not tokenise.** Rewrite `DESIGN.md` §Record &
      transport controls to describe the microphone on/off control and the
      reuse of the two existing tones; confirm `design-tokens.css`, `tokens.rs`
      and `theme.rs` are unchanged and `bun run design:validate` still passes.
- [ ] **T22.8 — Gates.** `cargo fmt -- --check`,
      `cargo clippy --all-targets -- -D warnings`, `cargo test`, `bun test`,
      `bun check src/stt`, `bun run design:validate`. All green, zero warnings.

## Acceptance Criteria

1. From the mode chooser, "Live dictation" lands on the take screen with a dark,
   still microphone button. Pressing it fills with the existing brand colour
   (`brand.primary`) and pulses with expanding waves while capturing.
2. Pressing the brand-coloured microphone pauses: the button returns to its
   dark, still state, the badge reads PAUSED and the waveform flattens. Pressing
   it again resumes. No separate Paused screen exists.
3. The footer reads, left to right: Cancel, the microphone control, Reset and
   Stop. Cancel returns to the mode chooser; Reset clears the take and stays on
   the screen; Stop ends the take and shows the stopped screen.
4. Cancel works mid-take; Reset is refused in idle; Stop works without pausing
   first.
5. A file take shows the same screen with the microphone and Reset disabled and
   Stop enabled.
6. The record control never shows a play, pause or square glyph.
7. All gates green with zero warnings — including `bun run design:validate` —
   and no animation, layout or screenshot test was added.

## Implementation Notes

_(added during work)_
