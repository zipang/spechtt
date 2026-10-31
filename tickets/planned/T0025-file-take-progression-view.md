# T0025 — A dedicated progression screen for file takes

- **Track**: Rust UI (controller + views + components + design system)
- **Wave**: 2 (integration)
- **Depends on**: T0015 (file transcription and `progress` events), T0021
  (`ScreenFooter`, `ViewState`), T0022 (live take screen, `cancel_take`),
  T0023 (picker → setup form → take)
- **Supersedes**: T0022 §3 "A file take renders the same screen with..." — a
  running file take gets its own screen instead of borrowing the live one
- **Blocks**: nothing
- **Owner**: unassigned (CTO to delegate)
- **References**: `AGENTS.md` §2 (view and component rules), `DESIGN.md`
  §Components, `src/stt/adapters/file.ts` (`progress` / `stopped` events),
  `crates/speechtt-ui/src/controller.rs` (`view_state`, `handle_stt_event`,
  `finish_take`)

## Context & Goal

A file take currently reuses the live recording screen. That screen is built
for microphone capture: pause/resume, reset, a live waveform and a transport
row. None of it means anything for a file:

- the host never receives file audio (`push_audio_chunk` is inert), so the
  waveform is always the idle mock;
- the worker decodes and transcribes a file in one sequential batch run; it
  cannot be paused, only stopped;
- the only useful feedback is the decoded position, which today is a small
  read-out squeezed into the transport header;
- when the worker reaches the end of the file it emits `stopped`, but the host
  does not finalize the take. The user is left on the recording screen and must
  press Stop on a take that is already over.

**Done means:** after Continue, a file take shows one quiet screen — the file
name and a progress bar — with no waveform, no pause/resume, no reset and no
microphone control. When the file ends the app moves to the Stopped screen by
itself, with the full transcript ready to copy or save. Stop remains available
to end early, and Cancel abandons the file.

## Scope Boundary

May create or modify:

- `crates/speechtt-ui/src/views/session_progression.rs` — **new**: the
  file-only progression screen.
- `crates/speechtt-ui/src/views/mod.rs` — **append-only** registration.
- `crates/speechtt-ui/src/views/recording.rs` — remove the file-take branch; it
  becomes the live-only take screen from T0022.
- `crates/speechtt-ui/src/components/progress_bar.rs` — **new**: the file
  progression bar and read-out.
- `crates/speechtt-ui/src/components/mod.rs` — **append-only** registration.
- `crates/speechtt-ui/src/controller.rs` — `ViewState::Progression`,
  `view_state()` routing, automatic finalization on `stopped`, and file-take
  guards for pause/resume.
- `crates/speechtt-ui/src/app.rs` — screen routing for `Progression`.
- `crates/speechtt-ui/tests/{widget,session_flow,end_to_end,design_system}.rs`.
- `DESIGN.md` — the file-progression component/view section and the state
  overview.
- `tickets/README.md` — board row.
- `tickets/planned/T0022-live-take-screen-record-control.md` — one cross
  reference so its file-take bullet does not contradict this ticket.

Do **not** touch: the TypeScript worker, `protocol.rs`, `speechtt-core`,
`settings.rs`, the file decoding path, `design-tokens.css`, `tokens.rs`,
`theme.rs`, the settings/help dialogs, `views/welcome.rs`,
`views/session_setup.rs`, `views/stopped.rs`, or the footer/round-button
definitions from T0021/T0022 beyond consuming them. No new dependency, no new
icon, no new colour, no new spacing/type/radius token, and no visual-layout or
screenshot test.

## Contract

### 1. A dedicated view state for a running file take

Add one variant to T0021's view model:

```rust
pub enum ViewState {
    Mode,
    Form,
    Live,
    Progression, // a file take is running
    Stopped,
}
```

`SessionController::view_state()` maps:

- a finished take (`result.is_some()`) ⇒ `Stopped`;
- `Blank` + `Screen::Mode` ⇒ `Mode`;
- `Blank` + `Screen::Form` ⇒ `Form`;
- file mode with an active machine (`Recording`/`Paused`) ⇒ `Progression`;
- file mode with no active take ⇒ `Form` (T0023's flow, unchanged);
- live mode with a complete configuration (`Ready`, `Recording`, `Paused`) ⇒
  `Live`;
- nothing else changes.

`app.rs` routes `ViewState::Progression` to
`views::session_progression::render`. The live `views::recording` renderer is
never reached for a file take.

### 2. Natural completion finalizes the take

`handle_stt_event` already applies `SttEvent::Stopped` to the transcript. For a
file take that is still running, the same event must also finalize it, exactly
as `finish_take()` does except that **no `SttCommand::Stop` is sent** — the
worker is the one that finished:

- stop the session machine and obtain the finished session;
- set `result = Some(FinishedTake { session, committed, live })` from the
  current transcript;
- clear `file_take`, `progress`, `written_path` and `output_status` like
  `finish_take()`;
- leave `finished_file_stem`, provider/model and the transcript intact;
- do **not** send any command, log an error or set an output status;
- the next `view_state()` is `Stopped`.

Rules:

- The branch is gated on `self.file_take`; a `Stopped` event cannot auto-finish
  a live take.
- A late `Stopped` event for a take the user already stopped is handled by the
  existing refinement path (update the finished transcript in place), not by a
  second finalization.
- Events for a stale session remain filtered by `accepts_session`.
- If the worker reports `error` and then `stopped`, the error strip stays
  visible and the partial take still reaches `Stopped`.
- Implement this as one small helper shared with `finish_take()` where
  practical, but the command emission rule above is part of the contract and
  is asserted in tests.

### 3. `views/session_progression.rs`

One screen, only for `ViewState::Progression`:

| Band | Content |
|---|---|
| Header | `StatusBadge::new("TRANSCRIBING")` with the default muted dot. No pulse, no time, no mode badge. |
| Body | One `SurfaceBox` holding the file's display name (`Body`, mono) and a `ProgressBar` built from the controller's `progress()` reading. |
| Footer | Leading: Cancel (`ScreenFooter::back`, `ArrowBigLeft`) → `cancel_take()`. Trailing: Stop (`IconAction`, `Square`) → `finish_take()`. No centre control. |

- The file name comes from `audio_source()`; the path is never shown.
- No `Waveform`, `CaptionWell`, `RoundButton`, record control, pause/resume,
  reset, elapsed timer or transport header is rendered.
- Stop is always enabled: it aborts the decode, keeps the text produced so
  far and shows `Stopped`.
- Cancel is always enabled: it aborts the decode, discards the take and returns
  to the mode chooser through T0022's `cancel_take()`.
- The screen is a thin composition over `ScreenShell` + `SurfaceBox` +
  `ProgressBar` + `ScreenFooter`; it passes no `px(..)`, `rems(..)` or
  `radius::` value.
- While an error is shown, it sits in the app's existing error strip above the
  body; the progression screen does not render errors itself.

### 4. `components::ProgressBar`

A reusable component, not a styled `div` at the view call site:

```rust
/// Unknown or non-positive totals yield `None`: the track renders empty.
#[must_use]
pub fn progress_fraction(processed: f32, total: Option<f32>) -> Option<f32>;

pub struct ProgressBar { … }

impl ProgressBar {
    /// `processed` and `total` are decoded seconds, as reported by the worker.
    pub fn new(processed: f32, total: Option<f32>) -> Self;
}
```

- `progress_fraction` clamps to `0.0..=1.0`, returns `None` when `total` is
  `None`, non-finite or `<= 0.0`, and returns `Some(0.0)` for negative
  `processed`. It never returns NaN or an out-of-range value.
- The component renders an 8 px-high (`space::SM`) `surface-alt` track with
  `radius::SM`, and a `brand.primary` fill whose width is the fraction. An
  unknown duration renders the empty track only.
- Below the bar it renders a `Label` read-out: `"processed / total s"` when the
  total is known, `"processed s"` otherwise (same wording as T0015's transport
  header). Values are rounded to whole seconds.
- It uses only existing tokens and theme roles; it adds no colour, size or
  radius of its own.

### 5. Pause and resume are inert for file takes

`SessionController::pause_recording*()` and `resume_recording*()` return
`false`, send no command and leave the machine unchanged when `file_take` is
true. This closes the keyboard-shortcut path into a `Paused` file take now that
the progression screen offers no pause control. Stop, cancel and the automatic
completion path are unaffected.

### 6. The live screen stays live-only

`views/recording.rs` no longer contains a file-take branch (disabled mic,
disabled reset, file-specific copy). It renders `ViewState::Live` exactly as
T0022 specifies. `views/paused.rs` is unchanged by this ticket (deleted by
T0022).

### 7. Design system

The CEO explicitly asked for this streamlined view; the visual decisions are
approved here and must be recorded in `DESIGN.md`:

- the status pill label `TRANSCRIBING` reuses `StatusBadge` and its existing
  muted dot — no new `BadgeTone`;
- the progress bar reuses `space::SM`, `radius::SM`, `surface-alt` and
  `brand.primary`;
- the screen adds no new text size, weight, spacing, radius, shadow or colour.

`DESIGN.md` gains a `### File progression` subsection (screen layout, bar,
read-out, footer actions, auto-completion) and its state overview mentions the
file-transcription state. `design-tokens.css`, `tokens.rs` and `theme.rs` are
untouched, so `bun run design:validate` passes unchanged.

## TDD Tasks

- [ ] **T25.1 — Red first: view routing.** `tests/session_flow.rs` and
      `tests/widget.rs`: a running file take reports `ViewState::Progression`;
      a running live take still reports `Live`; a file mode with no take still
      reports `Form`; a finished take reports `Stopped`. Rewrite any test that
      expected a file take to be `Recording`.
- [ ] **T25.2 — Red first: natural completion.** `tests/session_flow.rs`: a
      running file take that receives `Stopped` lands on `ViewState::Stopped`,
      exposes the merged transcript through `finished_take()`, clears
      `is_file_take()`/`progress()` and emits **no** `SttCommand::Stop`. A
      subsequent `Stopped` only refines the result; a stale-session `Stopped`
      is ignored; the same event on a live take does not auto-finish it. The
      user-initiated `finish_take()` still sends exactly one `Stop` and still
      stops early with the partial transcript.
- [ ] **T25.3 — Red first: pause/resume guard.** For a running file take,
      `pause_recording()` and `resume_recording()` return `false`, send no
      command and leave `view_state()` `Progression`; the same calls on a live
      take keep their current behaviour.
- [ ] **T25.4 — Red first: progression maths.** Unit tests for
      `progress_fraction`: middle value, clamping above 1 and below 0,
      `total = None`, zero, negative and non-finite totals, and negative
      processed. Pin the empty-track case. No rendering-geometry assertions.
- [ ] **T25.5 — Implement `ProgressBar`.** Build the component from existing
      tokens, export it from `components/mod.rs`, and give it a small builder
      test (known/unknown total produce the two read-out strings through the
      component state, not through pixels).
- [ ] **T25.6 — Implement the view and route it.** Create
      `views/session_progression.rs`, register it in `views/mod.rs`, route
      `ViewState::Progression` in `app.rs`, remove the file-take branch from
      `views/recording.rs`, and wire Cancel to `cancel_take()` and Stop to
      `finish_take()`.
- [ ] **T25.7 — Docs + gates.** Add `DESIGN.md` §File progression and the state
      overview sentence; update the board; point T0022's file-take bullet at
      this ticket. Run `cargo fmt -- --check`,
      `cargo clippy --all-targets -- -D warnings`, `cargo test`, `bun test`,
      `bun check src/stt`, and `bun run design:validate`. All green, zero
      warnings.

## Acceptance Criteria

1. After Continue on a picked file, the app shows one progression screen: a
   `TRANSCRIBING` status pill, the file name, a progress bar and its seconds
   read-out.
2. That screen has no waveform, no transcript well, no microphone control, no
   pause/resume and no reset. Only Cancel and Stop remain.
3. The bar advances from the worker's `progress` events, never exceeds the
   track, and stays empty when the duration is unknown (the read-out still
   advances in seconds).
4. When the file ends the app moves to the Stopped screen without a click, with
   the full transcript and the normal copy/save actions; the host sends no
   `Stop` command for that natural completion.
5. Stop before the end moves to Stopped with the text produced so far and sends
   one `Stop`; Cancel aborts the decode, discards the take and returns to the
   mode chooser.
6. Live dictation still shows the T0022 take screen; pause/resume cannot turn a
   file take into a paused recording.
7. All gates green with zero warnings; no layout, geometry or screenshot test
   added.

## Implementation Notes

_(added during work)_
