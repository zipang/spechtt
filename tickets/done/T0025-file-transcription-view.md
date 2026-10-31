# T0025 — A dedicated transcription screen for file takes

- **Track**: Rust UI (controller + views + components + design system)
- **Owner**: CTO agent
- **Started**: 2026-10-31
- **Closed**: 2026-10-31
- **Wave**: 2 (integration)
- **Depends on**: T0015 (file transcription and `progress` events), T0021
  (`ScreenFooter`, `ViewState`), T0022 (toggle round button, `cancel_take`),
  T0023 (picker → setup form → take)
- **Supersedes**: T0022 §3 "A file take renders the same screen with…" — a file
  take gets its own screen instead of borrowing the live one
- **Blocks**: nothing
- **References**: `AGENTS.md` §2 (view and component rules) / §3 (documentation),
  `DESIGN.md` §Components, `src/stt/adapters/file.ts` (`progress` / `stopped`
  events), `crates/speechtt-ui/src/controller.rs` (`view_state`,
  `handle_stt_event`, `finish_take`)

## Context & Goal

A file take renders the live take screen, which is built for microphone capture:
a record button, reset, a live waveform, an FFT-driven caption well. None of it
means anything for a file. The host never receives file audio, the worker
decodes and transcribes in one sequential batch run that cannot be paused, and
the only useful feedback is the decoded position.

**Done means:** a file take has its own screen showing which file is being
transcribed, how far the decode has got, and three actions: Back, Stop and
Continue. The screen never navigates by itself: when the worker reaches the end
of the file, the screen stays put with Stop inert and Continue enabled, and the
user decides when to open the transcript.

The result screen stops being called "stopped" too: the take ends with a
transcript to copy or save, so the view is `session_end`.

## Scope Boundary

May create or modify:

- `crates/speechtt-ui/src/views/session_transcription.rs` — **new**: the
  file-only transcription screen.
- `crates/speechtt-ui/src/views/session_end.rs` — **renamed** from
  `views/stopped.rs` (content unchanged apart from its doc header).
- `crates/speechtt-ui/src/views/mod.rs` — registration.
- `crates/speechtt-ui/src/views/recording.rs` — remove the file-take branch; it
  becomes the live-only take screen.
- `crates/speechtt-ui/src/components/progress_bar.rs` — **new**: the decode
  bar and its seconds read-out.
- `crates/speechtt-ui/src/components/mod.rs` — **append-only** registration.
- `crates/speechtt-ui/src/controller.rs` — `ViewState::Transcription` and
  `ViewState::SessionEnd`, `view_state()` routing, file-take finalization
  awaiting Continue, and file-take guards for pause/resume.
- `crates/speechtt-ui/src/app.rs` — screen routing and the Continue command.
- `crates/speechtt-ui/tests/{widget,session_flow,end_to_end,design_system}.rs`.
- `DESIGN.md` — the file-transcription screen and the view list.
- `tickets/README.md` — board row.

Do **not** touch: the TypeScript worker, `protocol.rs`, the file decoding path,
`design-tokens.css`, `tokens.rs`, `theme.rs`, the settings/help dialogs,
`views/welcome.rs`, `views/session_setup.rs`, or the footer/round-button
definitions from T0021/T0022 beyond consuming them. No new dependency, no new
icon, no new colour, no new spacing/type/radius token, and no visual-layout or
screenshot test.

## Contract

### 1. Two view states replace the borrowed one

```rust
pub enum ViewState {
    Mode,
    Form,
    Live,
    Transcription, // a file take: decoding, or decoded and awaiting Continue
    SessionEnd,    // the finished take, ready to copy or save
}
```

`SessionController::view_state()` maps:

- a file take that is running, **or** one whose decode has finished and is
  waiting for Continue ⇒ `Transcription`;
- any other finished take (`result.is_some()`) ⇒ `SessionEnd`;
- `Blank` + `Screen::Mode` ⇒ `Mode`;
- `Blank` + `Screen::Form` ⇒ `Form`;
- file mode with nothing running ⇒ `Form`;
- live mode with a complete configuration ⇒ `Live`.

`app.rs` routes `Transcription` to `views::session_transcription::render` and
`SessionEnd` to `views::session_end::render`. The live screen is never reached
for a file take.

### 2. The user leaves the screen; it never leaves by itself

The screen is a report, not a captive spinner. A file take ends in one of two
ways and **both stay on `Transcription`**:

- **Natural completion** — the worker emits `Stopped` at the end of the file. The
  host finalizes the take (machine → `Ready`, result built from the accumulated
  transcript) but sends **no** `SttCommand::Stop`: the worker finished on its
  own. Stop becomes inert, Continue becomes enabled.
- **Stop pressed** — `finish_take()` sends exactly one `SttCommand::Stop`, keeps
  the text produced so far, and leaves the same state: Stop inert, Continue
  enabled.

Continue (`continue_to_end()`) only reveals the result: it clears the pending
flag and the next `view_state()` is `SessionEnd`. It is refused when there is no
finished file take to show.

Rules:

- The natural-completion branch is gated on `file_take`; a `Stopped` event can
  never finalize a live take.
- A late `Stopped` event after the user stopped still refines the finished
  transcript in place, and never finalizes twice.
- Stale-session events stay filtered by `accepts_session`.
- If the worker reports `error` then `stopped`, the error strip stays visible and
  the partial take still reaches Continue.
- Back (`cancel_take()`) discards the take from either state — decoding or
  decoded — and returns to the mode chooser with no transcript left behind.

### 3. `views/session_transcription.rs`

| Band | Content |
|---|---|
| Header | `StatusBadge::tone("TRANSCRIBING", BadgeTone::Transcribing)` while decoding; `StatusBadge::new("TRANSCRIBED")` once it has finished. |
| Body | One `SurfaceBox` with the file's display name (`Body`, mono) and a `ProgressBar` from the controller's `progress()` reading. |
| Footer | Leading: Back (`ScreenFooter::back`, `ArrowBigLeft`) → `cancel_take()`. Primary: Stop (`IconAction`, `Square`) → `finish_take()`, enabled only while decoding. Trailing: Continue (`IconAction`, `ArrowBigRight`) → `continue_to_end()`, enabled only once decoding has finished. |

- The file name comes from `audio_source()`; the path is never shown.
- No waveform, caption well, round or toggle button, elapsed timer or transport
  header: the file has no microphone and no live text.
- The screen is a thin composition over `ScreenShell` + `SurfaceBox` +
  `ProgressBar` + `ScreenFooter`; it passes no `px(..)`, `rems(..)` or
  `radius::` value.
- Errors keep rendering in the app's existing strip above the body.

### 4. `components::ProgressBar`

```rust
/// `None` when the total is unknown or unusable: the track then renders empty.
#[must_use]
pub fn progress_fraction(processed: f32, total: Option<f32>) -> Option<f32>;

pub struct ProgressBar { … }

impl ProgressBar {
    /// `processed` and `total` are decoded seconds, as reported by the worker.
    pub fn new(processed: f32, total: Option<f32>) -> Self;
}
```

- `progress_fraction` clamps to `0.0..=1.0`, returns `None` when `total` is
  `None`, non-finite or `<= 0.0`, and `Some(0.0)` for non-positive `processed`.
  It never returns `NaN` or an out-of-range value.
- 8 px-high (`space::SM`) `surface-alt` track at `radius::SM`; the fill is
  `brand.primary` at the fraction. An unknown duration renders the empty track.
- Below the bar, a `Label` read-out: `"processed / total s"` when the total is
  known, `"processed s"` otherwise, rounded to whole seconds.
- Existing tokens only; no colour, size or radius of its own.

### 5. Pause and resume are inert for file takes

`pause_recording*()` and `resume_recording*()` return `false`, send no command
and leave the machine unchanged while `file_take` is true.

### 6. The live screen stays live-only

`views/recording.rs` no longer contains a file-take branch (disabled record
button, disabled reset). It renders `ViewState::Live` exactly as T0022
specifies.

### 7. Design system

- `BadgeTone::Transcribing` is added: the `action.info` role for the dot and
  label while the worker decodes, so the pill is distinct from ready (success),
  recording (danger) and paused (warning). One new badge tone, no new token.
- The progress bar reuses `space::SM`, `radius::SM`, `surface-alt` and
  `brand.primary`.
- The screen adds no text size, weight, spacing, radius, shadow or colour.

`DESIGN.md` gains a `### File transcription` subsection and lists the two new
views. `design-tokens.css`, `tokens.rs` and `theme.rs` are untouched, so
`bun run design:validate` passes unchanged.

## TDD Tasks

- [x] **T25.1 — Red first: view routing.** A running file take reports
      `Transcription`; a decoded file take awaiting Continue still reports
      `Transcription`; `continue_to_end()` reports `SessionEnd`; a finished live
      take reports `SessionEnd`; file mode with no take reports `Form`.
- [x] **T25.2 — Red first: no automatic navigation.** A running file take that
      receives `Stopped` stays on `Transcription`, exposes the merged transcript
      through `finished_take()`, clears `is_file_take()`, and emits **no**
      `SttCommand::Stop`. A second `Stopped` only refines. A stale-session
      `Stopped` is ignored. The same event on a live take does not finalize it.
      User `finish_take()` still sends exactly one `Stop` and keeps the partial
      transcript.
- [x] **T25.3 — Red first: Continue.** `continue_to_end()` is refused before a
      file take has finished, refused for a live take, and otherwise yields
      `SessionEnd` with the same finished take. Back from either transcription
      state returns to `Mode` with no result, transcript, progress or error.
- [x] **T25.4 — Red first: pause/resume guard.** For a running file take both
      return `false`, send nothing and keep `view_state()` `Transcription`.
- [x] **T25.5 — Red first: progress maths.** `progress_fraction`: middle value,
      clamping above 1 and below 0, `None`/zero/negative/non-finite totals, and
      non-positive processed. Pin the empty-track case.
- [x] **T25.6 — Implement `ProgressBar`** from existing tokens and export it.
- [x] **T25.7 — Implement the views and route them.** Create
      `views/session_transcription.rs`, rename `views/stopped.rs` →
      `views/session_end.rs`, register both, route the two view states, remove
      the file-take branch from `views/recording.rs`, wire the three footer
      actions and add `SpeechTtApp::continue_to_end`.
- [x] **T25.8 — Docs + gates.** Add `DESIGN.md` §File transcription, update the
      view list and the board. Run `cargo fmt -- --check`,
      `cargo clippy --all-targets -- -D warnings`, `cargo test`, `bun test`,
      `bun check src/stt`, and `bun run design:validate`.

## Acceptance Criteria

1. After Continue on a picked file, the app shows the transcription screen: a
   `TRANSCRIBING` pill, the file name, a progress bar and its seconds read-out.
2. That screen has no waveform, no caption well, no microphone control, no
   pause/resume and no reset.
3. The bar advances from the worker's `progress` events, never exceeds the
   track, and stays empty when the duration is unknown; the read-out still
   advances.
4. When the file ends the screen **stays**, the pill reads `TRANSCRIBED`, Stop is
   inert and Continue is enabled; no `Stop` command was sent.
5. Pressing Stop mid-decode sends one `Stop`, keeps the partial transcript and
   leaves the same state: Continue enabled.
6. Continue opens `session_end` with the transcript and the usual copy/save
   actions; Back discards the take and returns to the mode chooser, from either
   state.
7. Live dictation still shows the T0022 take screen; pause/resume cannot turn a
   file take into a paused recording.
8. All gates green with zero warnings; no layout, geometry or screenshot test
   added.

## Implementation Notes

### Decisions

- **Two flags, not one.** `file_take` means "decoding right now"; a new
  `awaiting_continue` means "finalized, the result waits behind Continue".
  `view_state()` returns `Transcription` while either is set, so the screen owns
  the take from the first decoded frame until the user leaves it.
- **Finalization without navigation.** `finalize_file_take()` is the natural
  completion path: it stops the machine, builds the finished take from the text
  received so far and sends **no** command. `finish_take()` keeps its one `Stop`
  for the user-driven case and, for a file take, ends in the same state instead of
  going straight to `SessionEnd`. Both share the result-building code path.
- **Continue is a reveal, not a transition.** `continue_to_end()` only clears the
  pending flag; the finished take is already built, so there is no second
  assembly step and nothing can drift between the screen and the result.
- **`SessionEnd` replaced `Stopped` in the view model too**, so the view file,
  the enum variant and the docs use one vocabulary. `SttEvent::Stopped` keeps its
  protocol name: that is the worker's word for the flush event, not a screen.
- **`BadgeTone::Transcribing`** reuses `action.info` for dot and label, the one
  existing role not yet used by a pill. `TRANSCRIBED` stays on the default muted
  pill: it is a resting report, and duplicating the success green would blur the
  "ready to start" signal.
- **`progress_readout` is a sibling of `progress_fraction`**, so the two strings a
  user can see are testable without rendering, exactly like the fraction.

### CEO revision — the pill reports progress, the name is always the file's

1. **The pill is a progress report, not a state name.** `In progress` while the
   worker decodes (amber `action.warning` dot, muted label — the same
   working-state treatment as `RECORDING`), then `Done` (success dot *and*
   success label). `BadgeTone::Transcribing` became `BadgeTone::InProgress` plus
   `BadgeTone::Done`; the blue `action.info` idea was dropped, so the pill reuses
   the two roles the live badges already use.
2. **The file name is never a placeholder.** The name is held in a new
   `file_display_name` field from the moment the take starts, because the session
   machine drops its configuration when the take ends — which is what forced the
   old `"Selected file"` fallback. It survives finalization and Continue and is
   cleared only with the take, so the screen always prints the real name.

### CEO revision — one clock, and a waveform progress row

1. **The corner shows the take's elapsed time**, not the decoded index. The
   progress events no longer write `elapsed_seconds` at all; the file take gets a
   real clock (`segment_started` set at start, `refresh_elapsed` ticking while the
   machine records), and the same `TransportHeader` renders it in the same place
   and style as on the live screen. `TransportHeader` lost its progress branch,
   which only the file take ever used.
2. **The progress row is a waveform**, not a continuous bar: 80 rounded bars
   tracing `PROGRESS_OSCILLATIONS` = 10 complete raised-cosine periods (one clean
   rise and fall each, trough at every period boundary), in the same bar geometry
   as `components::Waveform`, revealed from the left. A single slow hump read as a
   styling flourish; a dense wave reads as a signal the take is moving through. Revealed bars
   are `brand.primary` and the remainder stays on the theme border, so the whole
   length is always visible and the reveal reads as a fill. The reveal is the
   clipping box at bar resolution (`revealed_bars`), which needs no container
   measurement: a bar lights up once the fraction passes its midpoint.

### CEO revision — `STOPPED` for an interrupted take, no pill on the result

1. **Stopping reports `STOPPED` in the danger role.** A new
   `BadgeTone::Stopped` (danger dot *and* danger label) is shown when the user
   ended the decode before the worker reached the end of the file, so an
   interrupted transcription never reads as a completed one. `Done` keeps the
   success green for a natural completion, and `In progress` keeps the working
   amber while the worker is busy.
2. **The controller records how the take ended.** `file_take_stopped_early` is
   set by `finish_take()` for a file take and never by `finalize_file_take()`,
   so the two endings are distinguishable; it survives Continue into the result
   and is cleared with the take.
3. **`session_end` carries no status pill.** The screen is the transcript and its
   copy/save actions; the pill belonged to the take, not to the result.

### Deviations

- **The ticket is now `T0025-file-transcription-view.md`** (was
  `T0025-file-take-progression-view.md`) and the view is `session_transcription`
  rather than `session_progression`, per the CEO's naming.
- **The footer is Back / Stop / Continue**, with Stop centred and Continue
  trailing. The original contract had Cancel plus a trailing Stop and no
  Continue, and let the app advance by itself; the CEO replaced both: no
  automatic navigation, and the user opens the transcript.
- **`views/stopped.rs` → `views/session_end.rs`**, and `ViewState::Stopped` →
  `ViewState::SessionEnd`, both at the CEO's request. Content is unchanged.
- **`change_mode` now also clears `file_take`, `awaiting_continue` and
  `progress`.** Without it, backing out of a file take left `file_take` set and
  the chooser would still route to `Transcription`.
- **`reset_take` still refuses a file take**, and pause/resume are now inert for
  one: the worker decodes a batch and cannot be paused, so the keyboard shortcut
  must not turn a file take into a paused recording.

### Verification

```text
cargo fmt -- --check                          → clean
cargo clippy --all-targets -- -D warnings     → clean, 0 warnings
cargo test                                    → 14 suites ok, 0 failed
bun test                                      → 217 pass, 0 fail
bun check src/stt                             → No type errors in 23 files
bun run design:validate                       → 66 tokens, 0 errors
```

Mutation check: short-circuiting `finalize_file_take()` in `handle_stt_event`
fails three specs (`a_finished_file_take_is_not_navigated_by_the_app`,
`a_running_and_a_finished_file_take_both_stay_on_the_transcription_screen`,
`a_late_stopped_event_only_refines_the_finished_take`), so the routing specs are
not vacuous. No layout, position or geometry assertion was added.

### Review

Validated by the CEO on 2026-10-31 in the running widget, over four review
rounds: the screen and its Back/Stop/Continue footer, the progress pill and the
always-present file name, the real elapsed clock with the dense waveform
progress row, and the `STOPPED` report for an interrupted take. No independent
reviewer pass ran: the `reviewer` sub-agent was still unavailable (usage limit),
so the audit was the CTO agent's, backed by the controller tests, the pure
geometry specs and the window tests that render every state of the screen.
