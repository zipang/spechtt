# T0020 — Silent empty takes and explicit, logged backend failures

- **Owner**: CTO agent
- **Started**: 2026-10-31
- **Track**: Cross-cutting (TypeScript/Bun worker + Rust core/UI)
- **Wave**: 2 (integration)
- **Depends on**: T0009 (typed error surface), T0012 (segmentation), T0015 (file takes)
- **Blocks**: nothing
- **References**: `PRODUCT.md` Phase 9, `AGENTS.md` §4/§5/§8, `crates/speechtt-ui/src/controller.rs` (`AppError`), `src/stt/worker.ts`, `src/stt/adapters/vercel-ai-sdk.ts`

## Context & Goal

Reported repro:

1. Start a live dictation session.
2. Record nothing (silence, or pause immediately) for a moment.
3. Pause, then stop the take.

The status strip then shows, one after the other:

- **"Could not save the transcript."**
- **"The transcription backend sent an unexpected message."**

Neither message says what actually went wrong. There may be no real failure at
all, in which case both messages are noise.

Code review shows three concrete defects:

1. **An empty take is treated as a save failure.** `OutputError::EmptyTranscript`
   maps to `AppError::SaveFailed` (`controller.rs` `from_output`), and the
   `Stopped` screen keeps Copy/Save enabled when the transcript is blank.
   Clicking Save on an empty take reports "Could not save the transcript."
2. **The backend message is thrown away.** `AppError::from_stt_error` maps
   `stream_failed` and every unknown worker code to
   `ProtocolError(code)` — the worker's actual message never reaches the screen
   or a log. `last_error` briefly stores `code: message`, but nothing renders
   or logs it.
3. **There is no Rust diagnostics log.** The worker logs segments on `stderr`,
   but Rust reports typed errors only to the status strip. There is no way to
   reconstruct why a failure happened after the strip is dismissed.

In addition, the streaming adapter creates the provider stream at `start` and
flushes it by closing an empty `ReadableStream` when the user never spoke. A
provider-side "nothing to transcribe" error at that point is surfaced as
`stream_failed`, producing the phantom "unexpected message" on stop. T20.1 must
confirm this with a hermetic fake before changing it.

**Done means:** an empty take is a neutral, silent outcome; every genuine
failure names its cause in the strip and writes exactly one redacted diagnostic
line to `stderr`; and no secret or transcript text ever appears in either.

## Scope Boundary

May create or modify:

- `crates/speechtt-ui/src/controller.rs` — empty-take queries/guards, `AppError`
  mapping/messages, one-call error logging from `report_error`.
- `crates/speechtt-ui/src/app.rs` — stopped-screen wiring/status only.
- `crates/speechtt-ui/src/views/stopped.rs` — disable Copy/Save for empty
  takes and show neutral copy with existing `Label`/`IconAction::disabled`.
- `crates/speechtt-ui/src/diagnostics.rs` — **new**: redacted log formatter and
  `stderr` writer.
- `crates/speechtt-ui/src/lib.rs` — **append-only** module registration.
- `crates/speechtt-ui/tests/error_ux.rs`,
  `crates/speechtt-ui/tests/session_flow.rs`, plus one new focused
  `crates/speechtt-ui/tests/empty_take.rs` if needed.
- `src/stt/adapters/vercel-ai-sdk.ts` and `.spec.ts`.
- `src/stt/adapters/mistral.ts` and `.spec.ts` (only empty-stop parity).
- `src/stt/worker.ts` and `worker.spec.ts` (error diagnostics only).
- `tickets/README.md` — board row for this ticket.

Do **not** touch: `DESIGN.md`, `design-tokens.css`, `tokens.rs`, any component
under `crates/speechtt-ui/src/components/`, file decoding, provider registry,
settings schema, or the audio capture pipeline. No new dependency. No new
visual style: reuse `Label`, `IconAction` and its existing `disabled` state.

## Contract

### 1. Empty takes are a normal outcome

- `SessionController::finished_take_is_empty() -> bool` is `true` when there is
  no finished result **or** `committed` + `live` trim to an empty string.
- `SessionController::finish_take()` never reports an `AppError`, including
  when the flush arrives later with `fullText: ""`.
- `copy_finished_transcript` and `save_finished_transcript_to` guard an empty
  take defensively:
  - return `Err(AppError::NothingRecorded)`;
  - do **not** call `report_error` / `fail` (no strip);
  - do **not** write a file, touch the clipboard, or set `output_status`.
- `auto_paste_after_finish` returns `None` for an empty take (no copy/paste
  attempt, no clipboard error).
- `AppError::NothingRecorded` has message `"Nothing was recorded."` and
  `ErrorAction::Dismiss`. It is informational, so it is **not** logged at error
  level.
- `views/stopped.rs`:
  - when `finished_take_is_empty()`, Copy and Save render disabled
    (`IconAction::disabled(true)`);
  - the body still renders the (empty) `CaptionWell`;
  - a muted `Label::new("Nothing was recorded.")` replaces the transient output
    status area. No new component, token, size, weight or colour.
- There is no automatic save anywhere in the flow.

### 2. No phantom backend failure on an empty/silent start → stop

- Each streaming session tracks whether it received at least one non-empty
  audio chunk and whether it produced any transcript text.
- Calling `stop()` on a session that received no audio emits exactly one
  `stopped { fullText: "" }` and **no `error` event**. A provider flush failure
  for zero audio must not become a user-visible error.
- A session that did receive audio keeps existing semantics: a genuine
  provider failure is still reported exactly once.
- `worker.ts` `stopActive()` does not emit `stream_failed` for a normal `stop`
  (only for an actual `session.stop()` throw), and never emits both `error`
  and `stopped` for the same empty take.
- Mistral's silence-segmented path already behaves this way; the specs must
  lock the behaviour so a regression cannot reintroduce an error on an empty
  stop.

### 3. Every real failure is explicit

`AppError` changes:

```rust
pub enum AppError {
    // existing variants unchanged …

    /// Worker reported a failure that is not one of the canonical buckets.
    BackendFailed { code: String, detail: String },

    /// Copy/save was requested for an empty take; informational, not an error.
    NothingRecorded,
}
```

- `AppError::from_stt_error`:
  - `missing_api_key`, auth, network and timeout mappings stay as today;
  - `invalid_protocol` → `ProtocolError(message)` (keep the worker's parse
    detail);
  - `stream_failed` with a non-auth/non-network message → `BackendFailed
    { code: "stream_failed", detail: sanitized(message) }`;
  - any unknown code → `BackendFailed { code, detail: sanitized(message) }`
    instead of `ProtocolError(code)`; an empty message yields
    `detail = "no detail provided"`.
- `AppError::message()` returns an owned `String`/`Cow` so it can include the
  detail:
  - `BackendFailed` → `"The transcription backend failed: {detail}."`
  - `ProtocolError` → `"The transcription backend sent an unexpected message:
    {detail}."`
  - `SaveFailed` → `"Could not save the transcript: {cause}."` where `cause`
    is a short sanitized summary (`permission denied`, `no space left`,
    `unsupported format`, …), never a raw multi-line OS dump.
  - Visible message stays **≤ 160 characters**.
- `sanitize_detail(raw)` (pure, tested):
  - trims, collapses whitespace/newlines, strips control characters;
  - caps at 120 characters with `…` when truncated;
  - redacts `sk-…`, `Bearer …`, `Authorization: …`, `api_key=…`, `key=…`
    patterns to `[REDACTED]`;
  - returns an empty string only when the input is empty.
- `detail()` keeps returning the sanitized cause for `BackendFailed`,
  `ProtocolError` and `SaveFailed`.
- Existing guarantee stays: no transcript text, API key or secret in
  `Display`, `Debug`, the strip, or a log line.

### 4. Diagnostics logging

New `crates/speechtt-ui/src/diagnostics.rs`:

```rust
pub fn log_error(context: &str, error: &AppError);
pub fn format_error_line(context: &str, error: &AppError) -> String;
```

- `format_error_line` is pure and returns one physical line:
  `[speechtt] context=<context> kind=<Variant> code=<code|-> detail="<sanitized>"`
  (fields omitted when not applicable).
- `report_error` writes exactly one `format_error_line` to `stderr` per call.
- `handle_stt_event` on `SttEvent::Error` logs the event context **and** the
  worker-provided code/message (sanitized) before mapping it, so the cause is
  recoverable even when the typed variant is generic.
- `NothingRecorded` and user-dismissed strips produce no error-level line.
- `worker.ts`: before/at every emitted `error` event, write exactly one
  `console.error` diagnostic with `code`, `sessionId` and the sanitized
  original message. Diagnostics remain on `stderr`; `stdout` stays
  NDJSON-only.
- Log lines never contain a transcript, an API key, or a full provider
  payload.

## TDD Tasks

- [x] **T20.1 — Reproduce hermetically (red first).**
  - Rust: `finish_take()` on a paused take with no transcript leaves
    `app_error()` `None`; currently it can be followed by a save failure.
  - Rust: saving/copying an empty finished take does not write, does not touch
    the sink, and does not populate the strip.
  - TypeScript: a fake streaming provider whose flush rejects on zero audio —
    `session.stop()` must emit only `stopped {fullText:""}`; today it emits
    `stream_failed`.
  - Confirm and record the real root cause (also via the integration event
    loop) in **Implementation Notes** before changing adapters.
- [x] **T20.2 — Empty-take UX.** Add `finished_take_is_empty`, the
  `NothingRecorded` guards, auto-paste no-op, and the disabled Copy/Save +
  neutral label in `views/stopped.rs`. Tests cover the controller queries and
  the absence of `app_error`.
- [x] **T20.3 — Adapter empty-stop.** Implement the zero-audio stop path in the
  Vercel AI SDK adapter; lock Mistral parity. Specs: no provider call for zero
  audio, exactly one `stopped`, no `error`; audio-ful sessions keep existing
  error semantics.
- [x] **T20.4 — Worker diagnostics.** Emit one `stderr` line per `error` event
  and none for a clean empty stop. `worker.spec.ts` asserts `stdout` carries
  the same events as before and `stderr` carries the diagnostic.
- [x] **T20.5 — Explicit Rust mapping.** Add `BackendFailed` and
  `NothingRecorded`; rework `from_stt_error`, dynamic messages and
  `sanitize_detail`. Update `error_ux.rs` (tuple-variant call sites, 160-char
  bound) and add table-driven tests for every canonical code, `stream_failed`,
  unknown code, empty message, and redaction of `sk-…`/`Bearer …`.
- [x] **T20.6 — Diagnostics logger.** Implement `diagnostics.rs`; assert the
  exact one-line format, one call per `report_error`, and that
  `NothingRecorded` produces no error line. Add an integration test that an
  inbound `SttEvent::Error` logs the code/message and still maps to the right
  `AppError`.
- [x] **T20.7 — Gates.** `cargo fmt -- --check`,
  `cargo clippy --all-targets -- -D warnings`, `cargo test`, `bun test`,
  `bun check src/stt`. All green, zero warnings.

## Acceptance Criteria

1. The reported repro (start live, record nothing, pause, stop) leaves **no
   error strip**. The stopped screen says "Nothing was recorded." and neither
   Copy nor Save can be triggered.
2. Stopping a take that never received audio emits no `error` event, verified
   by worker/adapter specs with a fake provider that rejects empty input.
3. A genuine backend failure shows a message that names the failure and its
   code (for example `stream_failed` plus the sanitized provider reason), not
   the generic "unexpected message".
4. Every reported error writes exactly one `stderr` diagnostic line containing
   its context, code and sanitized detail; `stdout` remains NDJSON-only.
5. No test, strip, `Display`, `Debug` or log line contains transcript text, an
   API key, `sk-…` material or an `Authorization` header.
6. Saving a real transcript still succeeds exactly as today; saving an empty
   take never creates a file.
7. All gates green with zero warnings; no layout or visual-style test added.

## Implementation Notes

### Root cause (confirmed red-first, T20.1)

- **Phantom backend failure.** `VercelAiSdkSession` had no notion of whether a
  session ever received audio. `stop()` closed an empty `ReadableStream`, the
  provider rejected the empty input, `consume()` took the `catch` branch and
  emitted `stream_failed` before `stopped`. Reproduced hermetically with a fake
  provider whose flush throws on zero audio: without the guard the spec failed
  with `+ { type: "error", code: "stream_failed" }`; with the zero-audio guard
  the same spec emits exactly `started` + `stopped { fullText: "" }`.
- **Save failure for an empty take.** `AppError::from_output` mapped
  `OutputError::EmptyTranscript` to `SaveFailed`, and the stopped screen left
  Copy/Save enabled, so an empty take could report "Could not save the
  transcript.".
- **Discarded backend cause.** `from_stt_error` mapped `stream_failed` and every
  unknown code to `ProtocolError(code)`, keeping only the code and throwing the
  provider message away; only the transient, never-rendered `last_error` kept
  `code: message`.

### Decisions

- `AppError::message()` now returns an owned `String`; dynamic details are
  sanitized at render and clamped to 160 characters. `detail()` returns
  `Option<String>` for `BackendFailed`, `ProtocolError` and `SaveFailed`.
- `sanitize_detail` is pure and public: whitespace/control collapse, credential
  redaction (`sk-…`, `Bearer …`, `Authorization: …`, `api_key=…`, `key=…`),
  120-character cap with `…`. Empty input stays empty; blank-only input becomes
  `no detail provided` so no log line is ever silently blank.
- `OutputError::EmptyTranscript` → `NothingRecorded`. The empty-take guards in
  `copy_finished_transcript` / `save_finished_transcript_to` return it without
  touching the sink, the clipboard, `output_status` or the strip.
- `diagnostics::log_error` is the single write point for the strip; the
  `NothingRecorded` guard lives in a private `loggable_line` so it is unit
  tested without capturing `stderr`. `handle_stt_event` logs `context=stt_event`
  with the worker code and sanitized message before mapping, so the cause
  survives even when the variant is generic.
- `handle_stt_event` now stores the sanitized `code: message` in `last_error`
  to keep the controller's `Debug` free of raw provider text.
- Worker-side `emitError` funnels every `error` emission through one redacted
  `console.error` line (`code`, `sessionId`, sanitized message). `stdout` is
  unchanged: specs compare the exact event arrays.
- Mistral's silence-segmented path was already silent; a spec now locks it with
  a provider that throws on empty input.

### Follow-up (reported repro still showed a strip)

The first pass only suppressed the error when a session received **no** audio.
A real capture stream sends digital silence as PCM16 chunks, so
`receivedAudio` was already `true` and the provider's
`"Nothing was recorded."` flush failure still reached the strip as
`The transcription backend failed: Nothing was recorded..`.

Fixed in the same adapter (same scope):

- `isEmptyInputFailure(message)` recognises provider phrasings that mean
  "nothing to transcribe" (`no audio`, `no speech`, `nothing was recorded`,
  `audio is empty`, `silent audio`, …).
- `emitError` now suppresses when **no transcript text was produced** and either
  no audio arrived or the failure reports empty input. A take that produced real
  text still reports a later failure, and a silent take with a genuine failure
  (`provider down`, auth, network) still reports it exactly once — locked by
  specs.
- `AppError::message()` no longer doubles the full stop when a provider detail
  already ends a sentence (`sentence()` helper), so no future strip reads
  `recorded..`.
- Two red-first regressions lock the repro: an adapter spec with silent chunks
  plus a provider that rejects on flush (failed with `+ error stream_failed`
  before the guard) and a worker-level spec that drains the real audio stream
  and throws on flush (also failed before the guard).

### Verification

- `cargo fmt -- --check` — clean.
- `cargo clippy --all-targets -- -D warnings` — clean, zero warnings.
- `cargo test` — all suites green (182 lib + 3 + 12 + 29 + 3 + 11 + 31 + 13 +
  40 + 4 + 11).
- `bun test` — 216 pass, 0 fail.
- `bun check src/stt` — no type errors in 23 files.
- No new dependency; no token/theme/component change.
