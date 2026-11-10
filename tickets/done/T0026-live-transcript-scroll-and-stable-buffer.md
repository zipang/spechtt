# T0026 — A live transcript that scrolls and never loses words

- **Track**: Rust core (transcript reducer + NDJSON protocol) + Rust UI (caption well) + Bun worker (segment ids)
- **Owner**: coding sub-agent, supervised by CTO
- **Started**: 2026-11-02
- **Wave**: 1 (independent)
- **Depends on**: nothing
- **Blocks**: nothing
- **References**: `AGENTS.md` §2 (view and component rules) / §3 (documentation) /
  §6 (hermetic tests, no layout tests), `DESIGN.md` §Transcript well,
  `PRODUCT.md` §7 (NDJSON events), `crates/speechtt-core/src/transcript.rs`,
  `crates/speechtt-ui/src/components/caption_well.rs`,
  `src/stt/adapters/vercel-ai-sdk.ts`

## Context & Goal

A live take builds its text in the caption well while the user speaks. Today the
well is a clipped box whose text starts at the top and grows downwards: once the
take is longer than the box, the newest words fall outside the frame and are
never shown, so the user loses the thread of their own dictation. On top of
that, `TranscriptBuffer` treats every `partial` event as a replacement of the
whole live tail, and the worker drops the per-utterance identity the provider
attaches to it — a new utterance's hypothesis can therefore overwrite an earlier
unfinished one, and words disappear from the transcript for good.

Done means: the text only ever grows, every word the backend produced ends up in
the transcript exactly once, the newest line is always the visible one during a
take, and the user can scroll back to re-read earlier sentences without being
snapped to the bottom on the next word.

## Current Behaviour (audit, before the change)

- **Buffer** (`crates/speechtt-core/src/transcript.rs`): `committed: String` +
  `live: String`. `partial` _replaces_ `live`; `delta` appends to `live`;
  `committed` appends `text` to `committed` and clears `live`; `stopped`
  replaces `committed` with the authoritative `full_text` (or flushes the tail).
- **Events**: `partial` / `delta` / `committed` carry only `sessionId` and text.
  The AI SDK attaches an `id` to each part (`<sessionId>:<segmentIndex>` for
  ElevenLabs), and the worker discards it.
- **View**: `components::CaptionWell` renders two `Label`s (committed muted,
  live in `brand.accent`) inside `SurfaceBox::new().flex_1().clip()`. The text is
  top-anchored and `overflow_y_hidden`: growth past the box cuts the newest
  lines, with no scrolling at all.
- **Design intent**: `DESIGN.md` §Transcript well already says "Auto-scrolls to
  the newest line", which the component does not implement.

## Scope Boundary

May be created or modified:

- `crates/speechtt-core/src/transcript.rs` — the reducer and its unit tests.
- `crates/speechtt-core/src/backend.rs` — the `segment: None` line in the inline
  NDJSON parser test, which cannot compile once the event carries the field.
- `crates/speechtt-core/src/protocol.rs` — the optional `segment` field on
  `partial`, `delta`, `committed`, and the serde tests for it.
- `crates/speechtt-core/tests/bridge_smoke.rs` — compile fixes for the new field.
- `crates/speechtt-ui/src/components/caption_well.rs` — the scrollable well,
  `follows_bottom`, and its unit tests.
- `crates/speechtt-ui/src/components/mod.rs` — append-only re-exports.
- `crates/speechtt-ui/src/layout.rs` — retire `SurfaceBox::clip` if it becomes
  unused (it has no other caller).
- `crates/speechtt-ui/src/views/recording.rs` — pass the follow handle.
- `crates/speechtt-ui/src/views/session_end.rs` — only if the call site changes.
- `crates/speechtt-ui/src/app.rs` — own the caption scroll handle and expose it.
- `crates/speechtt-ui/tests/{session_flow,widget,end_to_end}.rs` — new field in
  fixtures, plus the live-transcript coverage described below.
- `src/stt/protocol.ts` — optional `segment` on the three text events.
- `src/stt/adapters/vercel-ai-sdk.ts` — forward `part.id`.
- `src/stt/adapters/{mistral,file}.ts` — name the committed segment.
- `src/stt/adapters/{vercel-ai-sdk,mistral,file}.spec.ts` — the new field.
- `DESIGN.md` — the Transcript well paragraph.
- `PRODUCT.md` — the §7 event list.
- `tickets/**` — this ticket and the board.

**Do not touch** anything else: no mode chooser, setup form, file-take screen,
status pill, footer, waveform, worker session lifecycle, or `SessionMachine`.
`crates/speechtt-core/Cargo.toml` and `crates/speechtt-core/src/lib.rs` are
append-only shared files and need no change here.

## Contract

### 1. Wire protocol — an optional utterance identity

`PartialEvent`, `DeltaEvent` and `CommittedEvent` gain an optional `segment`
(TS) / `segment: Option<String>` (Rust):

- A **stable identity for the utterance the text belongs to**. Revisions of one
  utterance carry the same `segment`; the next utterance carries a different
  one.
- Absent for a backend that publishes none, and therefore absent from the JSON
  (`skip_serializing_if`), and defaulting to `None` when missing
  (`#[serde(default)]`). A worker that never sends it stays compatible.
- Opaque to Rust: any string the adapter has. The AI SDK adapter forwards the
  part's `id`; the segmented adapters send their segment index as a string.

The emitted text payloads are otherwise unchanged, so a legacy `partial` without
`segment` parses and behaves exactly as today.

### 2. Reducer — append-only, segment-aware

`TranscriptBuffer` keeps `committed`, `live`, and the identity of the live tail
(`live_segment: Option<String>`). Rules:

| Event                                | Rule                                                                                                                                                                                                                 |
| ------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `partial { segment: Some(s), text }` | If a non-empty tail belongs to another segment, **append that tail to `committed` first**, then `live = text.trim()`. Same segment ⇒ plain replacement of the hypothesis.                                            |
| `partial { segment: None, text }`    | Historic behaviour: replace `live`.                                                                                                                                                                                  |
| `delta { segment: Some(s), delta }`  | Same settle rule, then append `delta` to `live`.                                                                                                                                                                     |
| `delta { segment: None, delta }`     | Append to `live`.                                                                                                                                                                                                    |
| `committed { segment, text }`        | If the tail belongs to this segment (same id, or the event has no id) it is **superseded, not appended**; a non-empty tail of an older segment is appended first. Then append `text` to `committed`, clear the tail. |
| `stopped { full_text }`              | Unchanged: a non-empty `full_text` is authoritative and replaces `committed`; empty flushes the tail.                                                                                                                |
| others                               | Unchanged.                                                                                                                                                                                                           |

Invariants, pinned by tests:

1. `committed` never shrinks during a take — except `stopped` with authoritative
   text, and `clear()`.
2. No utterance is discarded at a segment boundary: an unfinished tail is
   appended to `committed` before the next segment starts.
3. A final never duplicates the hypothesis it supersedes.
4. Whitespace-only text settles nothing and appends nothing (`push_committed`
   keeps its single-space join).
5. `full_text()` contains every word the backend produced, exactly once, in
   arrival order.

### 3. Caption well — scrollable, follows the newest line

- The well's text lives in a vertical scroll area that fills the
  `SurfaceBox`: `overflow_y_scroll`, **no scrollbar**. The panel keeps its
  current height (it flexes into the screen), so the visible window is the
  screen-adapted one; the full transcript stays in the buffer for copy/save.
- `CaptionWell::new(committed, live)` — top-anchored: the reader starts at the
  beginning of the text (session end screen). Wheel and keyboard scrolling
  reach the rest.
- `CaptionWell::new(committed, live).follow(&ScrollHandle)` — live take: the
  well tracks the handle and asks for the bottom on every frame **while the
  view is at the bottom**. Scrolling up stops following; scrolling back to the
  bottom resumes it. This is the only navigation rule: no "sticky" flag, no
  timer, no content measurement.
- `pub fn follows_bottom(offset_y: f32, max_offset_y: f32) -> bool` — the pure
  predicate behind it, testable headlessly. GPUI scroll offsets are
  non-positive and `max_offset` is the positive scroll range, so the bottom is
  `-offset_y >= max_offset_y - FOLLOW_EPSILON` (0.5 px). A view that cannot
  scroll (`max_offset_y == 0`, `offset_y == 0`) counts as at the bottom.
- The handle lives on `SpeechTtApp` and is passed by the live view; the result
  screen keeps its own top-anchored state.

No layout, position or geometry is asserted by any test: the predicate above is
behaviour, the rest is reviewed by the user in the running widget.

## TDD Tasks

- [x] **T26.1 — Reducer specs (red)**: in `transcript.rs`, add failing unit tests
      for: a new segment settles the previous hypothesis (`"hello"` then `"world"`
      ⇒ committed `"hello"`, live `"world"`); a revision inside one segment only
      replaces the tail; deltas of a new segment settle the old tail; a final for
      the live segment supersedes it without duplication; a final for the next
      segment settles an older unsettled tail; `segment: None` keeps today's
      replacement behaviour; whitespace-only text settles nothing; and a sequence
      over four segments whose `full_text()` holds every word once.
- [x] **T26.2 — Protocol specs (red)**: in `protocol.rs`, assert the legacy
      `partial`/`delta`/`committed` JSON is unchanged (no `segment` key, parses to
      `None`) and that a payload carrying `segment` round-trips.
- [x] **T26.3 — Reducer + protocol implementation**: add `segment` to the three
      Rust variants, thread `live_segment` through the reducer, keep `stopped`
      authoritative, and fix the existing tests/fixtures for the new field.
- [x] **T26.4 — Worker specs (red)**: `vercel-ai-sdk.spec.ts` — a
      `transcript-partial`/`transcript-delta`/`transcript-final` with an `id`
      produces an event with that `segment`; a part without `id` produces an event
      with no `segment` key. `mistral.spec.ts` and `file.spec.ts` — a committed
      event names its segment index.
- [x] **T26.5 — Worker implementation**: forward `part.id` in the SDK adapter,
      add the index to the two segmented adapters, and update `protocol.ts`.
- [x] **T26.6 — Caption well specs (red)**: unit tests for `follows_bottom` —
      at the bottom (`offset_y == -max_offset_y`), part-way up, on an unscrollable
      view, and inside the tolerance.
- [x] **T26.7 — Caption well implementation**: move the text into an
      `overflow_y_scroll` area inside the `SurfaceBox`, add the `follow` builder
      and the handle plumbing on `SpeechTtApp`, wire the live view to it, retire
      `SurfaceBox::clip` if now unused, and drop the stale "window decides how much
      is visible" comments.
- [x] **T26.8 — Integration coverage**: a controller-level live sequence over
      several segments asserting `transcript_committed()` grows and never shrinks;
      a widget smoke pass that feeds a long transcript to the live screen and
      renders frames without panicking.
- [x] **T26.9 — Docs**: `DESIGN.md` §Transcript well states the follow rule,
      the absence of a scrollbar and the kept-full-transcript rule; `PRODUCT.md` §7
      lists the optional `segment`; module docs describe the reducer as it is.
- [x] **T26.10 — Gates**: `cargo fmt -- --check`,
      `cargo clippy --all-targets -- -D warnings`, `cargo test`, `bun test`,
      `bun check src/stt`, `bun run design:validate`.

## Acceptance Criteria

- Dictating for longer than the well shows the newest words at all times, and
  the text keeps building: no committed word ever disappears while the take is
  open.
- Scrolling up with the wheel keeps the reader where they went; the next word
  does not yank the view back. Returning to the bottom resumes the follow.
- The live take has no visible scrollbar; the result screen opens at the start
  of the transcript and scrolls when the text is longer than the panel.
- A backend that sends no utterance identity behaves exactly as before, and an
  older worker's payload still parses.
- Every automated gate from `AGENTS.md` is green with zero warnings.

## Implementation Notes

### Wire protocol (`crates/speechtt-core/src/protocol.rs`)

`Partial`, `Delta` and `Committed` gained `segment: Option<String>` as the last
field, with `#[serde(default, skip_serializing_if = "Option::is_none")]`. A
legacy payload therefore parses to `None` and serializes byte-identically to
before, which the new `a_legacy_text_event_parses_without_a_segment_and_stays_unchanged_on_the_wire`
test pins field by field; `a_text_event_carrying_a_segment_round_trips` pins
the carrying case.

### Reducer (`crates/speechtt-core/src/transcript.rs`)

- `live_segment: Option<String>` tracks the utterance of the live tail.
- `begin_utterance(segment)` settles the tail only when both sides have an
  identity and they differ, so a `None` event keeps the historic replacement
  behaviour.
- `Committed` settles a tail of another segment, or a tail with no identity when
  the event names one, before appending; it supersedes a tail of its own segment,
  and any tail when the event carries no identity.
- `flush_live` became `settle_live`: it clears the identity and skips a
  whitespace-only tail, which the old code would have committed as a stray
  separator (`push_committed("")` after appending a space). This is the only
  behaviour change outside the segment rules, and it is required by invariant 4.
- `clear()` and an authoritative `stopped` reset the identity too.
- Nine unit tests cover the rules table and all five invariants, including a
  four-segment sequence that asserts `committed` is append-only after _every_
  event and `full_text()` holds each word once.

### Worker (`src/stt/`)

- `protocol.ts`: optional `segment` on `PartialEvent`, `DeltaEvent`,
  `CommittedEvent`.
- `vercel-ai-sdk.ts`: a module-local `withSegment(event, id)` helper returns the
  event untouched when `part.id` is `undefined`, so the key is absent from the
  JSON — not `null`, not an empty string. Used for partial, delta and final.
- `mistral.ts` / `file.ts`: `segment: String(segment.index)` on the committed
  event, so the segmented adapters name the utterance they finalized.
- Specs: two forwarding tests in `vercel-ai-sdk.spec.ts` (id present / absent),
  one index test each in `mistral.spec.ts` and `file.spec.ts`. The four existing
  exact-object assertions on `committed` events in `mistral.spec.ts` were
  updated to the new contract (deterministic indices `"0"`/`"1"`).

### Caption well (`crates/speechtt-ui/src/components/caption_well.rs`)

- The two labels moved into a `v_flex().id("caption-well-text")` with
  `.overflow_y_scroll()`, `.flex_1()`, `.min_h_0()` inside the `SurfaceBox`, so
  the panel keeps the height the screen gives it and the text scrolls inside.
  No `overflow_y_scrollbar()`, so nothing is painted at the edge.
- `pub fn follows_bottom(offset_y, max_offset_y) -> bool` is the whole rule:
  `-offset_y >= max_offset_y - FOLLOW_EPSILON` (0.5 px). `follow(handle)` calls
  `scroll_to_bottom()` only while it holds, then always `track_scroll(handle)`.
  Because the offset and `max_offset` read in one render come from the same
  previous frame, content growth does not break the follow: the request for the
  bottom is issued before layout measures the new text.
- `SpeechTtApp` owns one `ScrollHandle` (`caption_scroll`, `pub(crate)`
  accessor); `views/recording.rs` passes it. `views/session_end.rs` is
  unchanged: no handle means top-anchored, and each screen names its scroll
  area with its own element id (`LIVE_SCROLL_ID` / `RESULT_SCROLL_ID`), so a
  finished take cannot inherit the live take's bottom offset.
- `SurfaceBox::clip` removed with its stale doc (no caller left). The
  "window decides how much of a take is visible" comment went with it.
- Four unit tests on `follows_bottom`: unscrollable view, at the end, part-way
  up, and inside/outside the tolerance. No position or geometry is asserted
  anywhere.

### Integration coverage

- `tests/session_flow.rs`: `a_segmented_live_take_commits_every_word_once_and_never_shrinks`
  drives seven segmented events through `SessionController`, asserting
  `transcript_committed()` is append-only after each one, then the word list,
  then that an authoritative `stopped` still owns the final text.
- `tests/widget.rs`: `a_long_live_transcript_keeps_growing_and_renders` feeds 40
  committed sentences into the live screen through the backend channel and
  repaints after each, asserting the screen stays `Live` and keeps all 40.

### Deviation

`crates/speechtt-core/src/backend.rs` is not in the Scope Boundary, but its
inline `read_event_parses_one_event_per_line_and_reports_eof` test constructs
`SttEvent::Partial` as a struct literal and cannot compile once the variant has
a third field. The fix is the single mechanical `segment: None` line the
mitigation in the brief prescribes for every construction site; no test
semantics changed. All other construction sites were updated in the test files
the ticket lists (`session_flow.rs`, `widget.rs`, `end_to_end.rs`).
`crates/speechtt-core/tests/bridge_smoke.rs` needed no change: it destructures
`Committed { text, .. }` and therefore never names the new field.

### Gate results

| Command                                     | Result                                                                                                                                                                                    |
| ------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `cargo fmt -- --check`                      | clean, no diff                                                                                                                                                                            |
| `cargo clippy --all-targets -- -D warnings` | clean, zero warnings                                                                                                                                                                      |
| `cargo test`                                | 427 passed, 0 failed (core lib 201, bridge_smoke 3, session_machine 14, ui lib 45, design_system 3, dialogs 11, end_to_end 33, error_ux 14, session_flow 74, theme_contract 4, widget 25) |
| `bun test`                                  | 221 passed, 0 failed across 12 files (796 assertions)                                                                                                                                     |
| `bun check src/stt`                         | no type errors in 23 files                                                                                                                                                                |
| `bun run design:validate`                   | design contract valid — 66 tokens, 0 errors                                                                                                                                               |

New test count: +14 in `speechtt-core` (11 for the segment and delta rules,
three for the polish round), +5 in the `speechtt-ui` unit tests, +1 in
`session_flow.rs`, +1 in `widget.rs`, +3 in the worker specs.

### Left for the user's visual review

The follow behaviour itself (newest line pinned, scrolling up stops the follow,
returning to the bottom resumes it) and the absence of a scrollbar are
behaviour-pinned but not pixel-pinned: they need the running widget.

### Diagnosis — duplicated live text

**Reported sequence.** After ~12 s of dictation and one pause, the live tail
(orange text) read `d'une transcription quelconqued'une transcription
quelconque` with no committed text above it.

**Cause.** `live` has exactly two producers: `partial` replaces it, `delta`
appends to it. Two identical adjacent copies therefore mean the provider sent a
`partial` carrying the whole utterance and then a `delta` carrying the same
whole text; the old `Delta` arm (`live.push_str(delta)`) pasted the cumulative
text on top of the tail it duplicated. The emit path and the Rust event pump
each run once, so this is neither a Bun double-emit nor a double-apply, and
`@ai-sdk/gateway` forwards server parts unchanged — the shape comes from the
provider, not from the adapter.

**Rule chosen.** `TranscriptBuffer::push_delta` classifies the incoming delta
against the tail, in order: whitespace-only is ignored; a delta that extends the
tail from its start is cumulative and replaces it; a delta the tail already ends
with is a duplicate and is ignored; anything else is new text and is appended.
An empty tail takes the delta as is. Six new unit tests pin each branch,
including the exact reported string, plus a regression on the segment settle
rule. `begin_utterance` is untouched. The polish round below refines the
definition at word boundaries (F3): a shorter suffix of the tail is no longer
treated as a duplicate.

**Why not "partial wins, ignore deltas".** A provider may send a partial and
then deltas that add words the partial never contained
(`a_partial_then_a_delta_that_adds_words_keeps_the_new_words`). Blanket-ignoring
deltas after a partial would silently lose that text — the opposite failure, and
one the user cannot see.

**Temporary probe (removed).** `logPartShape` in
`src/stt/adapters/vercel-ai-sdk.ts` logged one line per received transcript part
on `stderr`, marked `// TEMPORARY`, to confirm the gateway sequence. It printed
the part type, the part `id` (`-` when absent) and the text/delta length — a
single template literal with no text, audio or credential interpolated
(AGENTS.md §9). It changed no behaviour; the adapter specs assert emitted
events, not stderr, and stayed green. The probe was deleted in the polish round
below (B1), so the worker no longer writes per-part diagnostics.

**Follow-up gate run after this change**

| Command                                     | Result                                                                                                                                                                                                                                        |
| ------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `cargo fmt -- --check`                      | clean, no diff                                                                                                                                                                                                                                |
| `cargo clippy --all-targets -- -D warnings` | clean, zero warnings                                                                                                                                                                                                                          |
| `cargo test`                                | 449 passed, 0 failed (core lib 210 — +6 delta-classification tests and +3 polish tests, bridge_smoke 3, session_machine 14, ui lib 58, design_system 3, dialogs 11, end_to_end 33, error_ux 14, session_flow 74, theme_contract 4, widget 25) |
| `bun test`                                  | 221 passed, 0 failed across 12 files (796 assertions)                                                                                                                                                                                         |
| `bun check src/stt`                         | no type errors in 23 files                                                                                                                                                                                                                    |
| `bun run design:validate`                   | design contract valid — 66 tokens, 0 errors                                                                                                                                                                                                   |

The total includes the tests T0027 added between the two runs; 210 and 58 are
the counts of this final run, not of the run right after the delta classifier.

### Tour de polissage (revue CTO)

Five findings from the CTO review were applied without widening the scope.

- **B1 — probe removed.** `logPartShape` and its three call sites are gone from
  `src/stt/adapters/vercel-ai-sdk.ts`; no other line of the adapter changed.
- **F1 — follow staleness documented.** `ScrollHandle::scroll_to_bottom` is
  documented as "Scrolls to the bottom" with no prepaint guarantee
  (<https://docs.rs/gpui/latest/gpui/struct.ScrollHandle.html>), while
  `scroll_to_item` is the method the docs describe as running "in prepaint".
  The docs do not state that `scroll_to_item` aligns the item at the bottom, so
  the behaviour is unchanged and `CaptionWell::follow`'s doc now states that
  the predicate reads the metrics of the previous frame and can therefore hide
  the newest line for that one frame.
- **F2 — one scroll id per screen.** `LIVE_SCROLL_ID` and `RESULT_SCROLL_ID`
  replace the shared `CAPTION_SCROLL_ID`: element state is keyed by id, so the
  finished take can no longer inherit the live take's bottom offset. A unit test
  pins that the two ids differ; no layout is asserted.
- **F3 — word boundaries in `push_delta`.** The delta is now classified at
  character boundaries: cumulative only when it repeats the whole tail and the
  remainder starts with a non-alphanumeric character; duplicate only when it is
  the whole tail verbatim; everything else appends. The duplicate rule in the
  brief also ignored a shorter suffix preceded by a non-alphanumeric character,
  which would swallow the two cases the brief's own tests require to survive
  (`" très"` after `"très très"`, `"chat"` after `"le chat"`). The
  whole-tail-equality rule keeps every word the backend produced, at the cost
  of a visible repetition; the two new tests pin it.
- **F4 — context menu on the whole card.** The menu hangs off an outer `div()`
  that fills the card (`flex_1`, `flex`, `flex_col`, `min_h_0`) and wraps the
  `SurfaceBox`; a right-click in the card's inner padding opens `Copy` again,
  and the scroll column keeps its own offset. The component doc says where the
  menu is attached and why.
- **F5 — a final naming a segment settles an unidentified tail.** `committed`
  now settles the tail unless the event and the tail carry the same identity
  (or the event has no identity). A final that cannot be matched to the tail is
  appended after it instead of replacing it; the documented compromise is a
  visible duplicate for a mixed worker rather than a silent loss.
- **B1 (independent re-reading) — a whitespace delta is a word separator.**
  `push_delta` ignored every whitespace-only delta, so a provider streaming the
  space between two words as its own delta produced `lemonde`. A whitespace delta
  is now appended when the tail is non-empty and does not already end with
  whitespace, and ignored otherwise, so a take never opens on a blank and a
  separator is never doubled. Two tests pin both halves.
- **B2 (independent re-reading) — the follow starts at the top of a new take.**
  The caption handle outlives a take, so an offset the reader scrolled to in the
  previous one opened the next take already scrolled up, with the follow inactive
  until they scrolled back down. `SpeechTtApp::rewind_caption` resets the offset
  and runs on `start_recording`, on the `StartRecording` action, and on
  `reset_take`.
- **B3 — local secrets are ignored.** `.env`, `.env.*` and the
  `!.env.example` exception were added to `.gitignore`, so `.env.local` can no
  longer be staged by accident. That file is outside this ticket's scope and
  ships as its own commit.

Final gate run of this round:

| Command                                     | Result                                                |
| ------------------------------------------- | ----------------------------------------------------- |
| `cargo fmt -- --check`                      | clean, no diff                                        |
| `cargo clippy --all-targets -- -D warnings` | clean, zero warnings                                  |
| `cargo test`                                | 451 passed, 0 failed                                  |
| `bun test`                                  | 221 passed, 0 failed across 12 files (796 assertions) |
| `bun check src/stt`                         | no type errors in 23 files                            |
| `bun run design:validate`                   | design contract valid — 66 tokens, 0 errors           |

### Field test — a 1:13 live take on the Gemini live model

**What the user saw.** The whole transcribed text was erased under their eyes
mid-take; the last window then appeared twice; the earlier part never came back
on the result screen.

**Why it happened.** Only two paths can erase `committed`: `clear()` and a
`stopped` carrying non-empty text. A provider that closes its stream early makes
the worker report `fullText = result.text`, which is the last window it
transcribed and not the take. The reducer adopted it as authoritative and
replaced the transcript with its own tail — and a finished take keeps that
result, so the earlier words were gone for good. The duplicate came from the
same window being re-sent under a new utterance identity: the settle appended a
tail the buffer already held.

**Fixes.**

- `TranscriptBuffer::authoritative_text_is_a_fragment` refuses a `stopped` text
  that is only a shorter substring of the transcript already assembled; the take
  is kept and the live tail promoted instead. A genuinely authoritative final —
  the whole take, or a rewrite that no longer contains the assembled text — still
  replaces, which `stopped_with_full_text_replaces_the_whole_transcript` pins.
- The `Partial` arm no longer settles when the incoming hypothesis is identical
  to the tail: the identity moves to the new segment and the words are neither
  appended twice nor dropped.
- The reducer now settles a restarted partial window before installing the next
  window. It keeps prefix extensions, same-window shortening/repunctuation, and
  cumulative deltas that extend inside a word distinct; the latter replace the
  tail instead of appending repeated characters.
- The worker does maintain a separate `accumulated` field, but it is only the
  concatenation of `transcript-final` parts. It is not an independent complete
  transcript for the Gemini sequence: the provider sent no finals during the
  take. The AI SDK's `result.text` is the other complete-text field, and the log
  proves it was the 292-character final emitted as `stopped`. The worker now
  uses that final result as the sole completion event, so `transcript-final` is
  accumulated without emitting a duplicate `committed` event.

Two tests pin the reported sequence
(`a_stopped_text_that_is_only_the_tail_does_not_erase_the_take`,
`the_same_hypothesis_under_a_new_segment_is_not_settled_twice`).

**Trace.** `SPEECHTT_STT_TRACE=1` prints one line per text event (`type`,
`session`, `accepted`, `segment`, `len`, never the text) from
`crate::diagnostics`, so the next occurrence yields the exact sequence. It is
off unless the variable is `1`.

| Command                                     | Result                                                |
| ------------------------------------------- | ----------------------------------------------------- |
| `cargo fmt -- --check`                      | clean, no diff                                        |
| `cargo clippy --all-targets -- -D warnings` | clean, zero warnings                                  |
| `cargo test`                                | 456 passed, 0 failed                                  |
| `bun test`                                  | 221 passed, 0 failed across 12 files (796 assertions) |
| `bun check src/stt`                         | no type errors in 23 files                            |
| `bun run design:validate`                   | design contract valid — 66 tokens, 0 errors           |
