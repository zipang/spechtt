# T0031 — Stop the live transcript from accumulating duplicated fragments

- **Track**: Rust core (`transcript.rs`)
- **Wave**: 1 (independent)
- **Depends on**: nothing
- **Blocks**: nothing
- **Owner**: two coding sub-agents (reducer, capture regression), supervised by CTO
- **Started**: 2026-11-05
- **References**:
    - `AGENTS.md` §5 (gates), §6 (Do: tests are the contract)
    - `DESIGN.md` § "Transcript well"
    - `src/stt/transcript-merge.ts` — the existing junction-merge policy for
      segmented takes, whose overlap rule this ticket mirrors for live windows
    - Observed event stream from MAI Transcribe 2 Streaming (below)

## Context & Goal

A live take with a windowing provider shows a heavily smeared transcript: each
hypothesis is preceded by fragments of the previous one, so the screen reads
like a stutter. The delivered final text is clean, because `Stopped` replaces
everything — but the whole live experience is unusable.

Real capture of one dictation, live text as displayed:

```text
Début. Début d'une. Début d'une nouvelle.Début d 'une nouvelle. 'une nouvelle
dictée. 'une nouvelle dictée. dictée avec. dict ée avec le. ée avec le modèle.
ée avec le modèle MAI. le modèle MAI. modèle MAI. MA I Transcribe I Trans cribe
to Speech. cribe to Stream. to Streaming. to Streaming. Streaming. Je vais vous
Je veux voir si le texte Je veux voir si le texte affiché si le texte affiché
en le texte affiché en direct texte affiché en direct est affiché en direct est
plein de doublons. en direct est plein de doublons direct est plein de doublons
et ça est plein de doublons et c'est le cas.
```

The final text delivered for the same take (authoritative, from `Stopped`):

```text
Début d'une nouvelle dictée avec le modèle MAI Transcribe to Streaming. Je veux
voir si le texte affiché en direct est plein de doublons et c'est le cas.
```

### Root cause

`TranscriptBuffer::apply` classifies a `Partial` with `revises_the_tail`, which
compares the new text against the tail **verbatim**:

- `text.starts_with(tail)` → revision, replace the tail;
- `tail.starts_with(text)` and at least half as long → revision, replace;
- anything else → **new window**: `settle_live()` pushes the whole old tail into
  `committed`, then the new text becomes the tail.

A windowing provider violates both assumptions:

1. It **slides**: the new hypothesis starts with the _suffix_ of the previous
   one (`ée avec le modèle MAI.` → `le modèle MAI.`), not with its whole prefix.
2. It **re-tokenizes**: the same words come back with different spacing
   (`d'une` → `d 'une`, `dictée` → `dict ée`, `MAI` → `MA I`, `Transcribe` →
   `Trans cribe`), so an exact `starts_with` fails even for a genuinely
   cumulative hypothesis.

Every such event settles the old tail and starts fresh, so `committed`
accumulates every overlap. The screen shows `committed` followed by `live`,
which is exactly the smear above.

### Goal

Previous attempts picked a merge rule first and discovered afterwards that it did
not hold for every provider. This ticket refuses to repeat that: **the reducer's
behaviour follows what the providers actually emit**, measured rather than
assumed. Two instruments were built for that, and they are specified in the
contract below — they stay in the tree as the way to re-answer the question when
a new model appears.

### Measurements

`SPEECHTT_STT_PARTIALS=1`, French dictation, one take per model, appended to one
file. The dump header names provider, model and language and the analyzer groups
by it, so three takes that all call their session `take-1` stay apart.

Three models, **three different shapes** — and none of them is the one earlier
attempts assumed.

**`elevenlabs/scribe-v2-realtime` — 24 partials, no delta.**
Each partial is the **whole take so far** (9 → 241 characters), and the first
50–99 % is byte-identical to the previous one. It **rewrites its own earlier
words**: at 19→20 it inserted French guillemets around "streaming" from character
117, at 23→24 it removed them, at 24→25 it put them back. The take then failed
with `Transcription provider stream error`, and `stopped` arrived **empty**.

**`microsoft/mai-transcribe-2-streaming` — 79 partials + 24 deltas.**
The partials are short **sliding fragments** (`Nouveau`, `Nouveau test.`,
` fois-ci avec.`, ` avec le modèle` …) with 1–3 words of overlap. The deltas are
**increments**, and they split mid-word: ` MAI`, ` Trans`, `cribe`, ` to`.
Concatenating them **verbatim** reproduces the 185-character `stopped` text
exactly.

**`google/gemini-3.5-transcribe-live` — 23 partials + 1 delta.**
The partials are **cumulative** (6 → 116 characters) with retroactive fixes: at
seq 2 it emitted `It was` — an English hallucination — then replaced it with
`Et voici le`; at seq 20 `fenêtre coulissante` became `fenêtre glissante`; the
full stop after `3.5` appears and disappears. Its single delta is **not an
increment**: it carries the whole take (117 characters), re-punctuated.

So:

| Model      | Partials are       | Deltas are           | Rewrites its past                 |
| ---------- | ------------------ | -------------------- | --------------------------------- |
| ElevenLabs | the whole take     | absent               | yes (guillemets)                  |
| MAI        | a sliding fragment | increments, mid-word | no                                |
| Gemini     | the whole take     | the whole take, once | yes (language, word, punctuation) |

### What the shipped reducer does with that data

Simulated faithfully, deltas included:

| Take       | Authoritative text                    | Shipped reducer, worst frame | Settles                       |
| ---------- | ------------------------------------- | ---------------------------- | ----------------------------- |
| ElevenLabs | 241 chars (no final: provider failed) | 1 755 chars                  | 12                            |
| MAI        | 185 chars                             | 1 049 chars mid-take         | 32                            |
| Gemini     | 117 chars                             | —                            | every retroactive fix settles |

Both smear while dictating. MAI ends correct only because a non-empty `stopped`
replaces everything; the damage is confined to the live view. The ElevenLabs take
ends wrong: its `stopped` was empty, so the 1 755-character smear is what the user
is left with.

### Root cause, precisely

The reducer treats **every** `Partial` as a possible new window and _settles_ the
previous tail into `committed` when the text is not a prefix of it. Three measured
behaviours defeat that test: a retroactive rewrite (no seam to compare), a sliding
fragment (never was a prefix), and a language switch mid-take.

The second half of the bug is that the `Delta` channel is discarded: deltas are
appended to the _live tail_, which the next partial overwrites. For MAI that
throws away the one channel that reconstructs the take exactly.

### Decision — channel split

**Deltas accumulate verbatim; partials preview; the two are joined at a word
seam.**

| Event               | What it is                                | What the buffer does                                                          |
| ------------------- | ----------------------------------------- | ----------------------------------------------------------------------------- |
| `Delta`             | incremental text, or the whole take again | append **verbatim**; replace when it is cumulative; ignore an exact duplicate |
| `Partial`           | a preview of the current window           | replace `live` — never settle                                                 |
| `Committed`         | the provider closed an utterance          | commit, clear `live`                                                          |
| `Stopped` with text | the authoritative take                    | replace both                                                                  |
| `Stopped` empty     | a take that died                          | fold `live` into `committed` at the seam                                      |

Deltas concatenate **without inserting a separator**: MAI's ` Trans` + `cribe` is
one word, and the provider already supplies its own leading spaces.

The display is `seam_join(committed, live)`:

1. if `live` is entirely a suffix of `committed` (compared per word, case and edge
   punctuation folded), show `committed` — this is what keeps Gemini's whole-take
   delta and its whole-take preview from doubling;
2. otherwise remove the longest shared word run at the joint when it is **at
   least two words**, or **exactly one word while `live` continues past it**;
3. otherwise join with one space.

The one-word clause is load-bearing and was found by the capture test, not by
inspection: MAI repeats a single word at two joints (`découvrir` then
`découvrir à la fin de cette vidéo.`, `cette` then `cette session.`) where the
preview restates the word and continues. With a flat two-word threshold the
display overshoots to 193 characters against a 185-character take — a violation
of the acceptance criterion, on a run the two-word repetition check does not even
see. The clause reads: _a preview that restates the end of the committed text and
then continues is a continuation, not a repetition_. A preview that **ends** at
the shared word is handled by rule 1 and adds nothing.

`très très` survives because the repeated word is inside one hypothesis, which is
how a provider delivers a genuine repetition; a seam that restates one word and
keeps going is a sliding window.

Measured against all three captures, at **every** frame:

| Take       | Worst displayed frame | Real text | Repeated 2-word runs | Final exact |
| ---------- | --------------------- | --------- | -------------------- | ----------- |
| ElevenLabs | 241 chars             | 241       | **0**                | yes         |
| MAI        | 185 chars             | 185       | **0**                | yes         |
| Gemini     | 117 chars             | 117       | **0**                | yes         |

### Why this beats the strategies the ticket started with

- **A — overlap-aware partial reducer.** A retroactive rewrite leaves no seam to
  measure, and MAI's fragments overlap by 1–3 words, below any safe threshold.
  Gemini rewrites a word (`coulissante` → `glissante`) and switches language;
  there is nothing to key on.
- **B — show only the newest hypothesis.** It discards MAI's delta channel and
  would end that take on the fragment ` session.` instead of 185 characters.
- **Channel split** gives MAI a transcript that _grows_, reconstructed exactly
  from its deltas, and gives the two cumulative providers their clean preview.
  No model is special-cased: the split follows what the two SDK channels mean.

## Scope Boundary

Allowed to create or modify:

- `src/stt/trace.ts` + `src/stt/trace.spec.ts` (new: the opt-in dump)
- `src/stt/trace-analyze.ts` + its spec (new: the classifier that reads a dump)
- `src/stt/worker.ts` (wrap `emit` when the dump is on)
- `crates/speechtt-core/src/transcript.rs` (reducer + its unit tests)
- `crates/speechtt-core/tests/transcript_captures.rs` (new: replay the three
  measured captures)
- `crates/speechtt-core/tests/fixtures/*.tsv` (the three captures)
- `tickets/README.md` and this ticket

Out of scope — do **not** touch:

- `crates/speechtt-ui/**` (the screen keeps rendering `committed` + `live`; the
  reducer is the single source of truth for both, and Strategy B changes nothing
  in the view)
- `src/stt/adapters/**` (an adapter must keep forwarding the provider's events as
  they are: the reducer and the analysis own the policy, and hiding events in the
  adapter is what makes a dump lie)
- `crates/speechtt-core/src/protocol.rs` (no wire change)
- `DESIGN.md` (no visual change)

## Contract — the measurement half

### 1. The opt-in dump — `src/stt/trace.ts`

```ts
/** Environment variable that switches the dump on. */
export const PARTIALS_ENV = "SPEECHTT_STT_PARTIALS";
/** Prefix every dump line carries, so a log filters with one grep. */
export const PARTIALS_TAG = "[stt-partials]";

export function partialsTraceEnabled(env?: Record<string, string | undefined>): boolean;
/** One-line JSON record for `event`, or `null` for an event with no transcript. */
export function formatPartialRecord(event: unknown, seq: number): string | null;
/**
 * Describes the take the dump belongs to. Two models land in one log, so the header is what makes
 * the comparison possible.
 */
export function formatSessionHeader(details: {
	provider: string;
	model: string;
	language?: string;
	sampleRate?: number;
	kind: "live" | "file";
}): string;
```

Rules:

- Enabled **only** when `SPEECHTT_STT_PARTIALS` is exactly `1`, and it is a
  different variable from `SPEECHTT_STT_TRACE`: that one writes lengths, this one
  writes transcript text, which is the user's own content. Keeping them separate
  means enabling the harmless trace never starts writing text to a log.
- One physical line per event, JSON after the tag, so a newline inside a
  hypothesis cannot split a record. Fields: `seq`, `type`, `sessionId`,
  `segment`, `requestId` when present, `chars`, `text` (or the event's own text
  field name: `delta`, `fullText`).
- `seq` is monotonic per process, so a dump replays in emission order.
- Only transcript-bearing types are recorded (`started`, `partial`, `delta`,
  `committed`, `stopped`); `progress`, `capabilities` and `ready` are noise.
- `worker.ts` wraps its `emit` once, so every adapter and the file session are
  covered without touching either.
- `stdout` stays NDJSON-only; the dump goes to `stderr`.

### 2. The classifier — `src/stt/trace-analyze.ts`

```ts
export type Transition = "identical" | "extend" | "shrink" | "slide" | "new";

export interface PairClassification {
	readonly kind: Transition;
	readonly overlapWords: number;
	/** Characters of `next` the seam already showed, i.e. shown twice. */
	readonly overlapChars: number;
	readonly newWords: number;
	/** `true` when the seam only matched after folding whitespace away. */
	readonly retokenized: boolean;
}

export function classifyPair(previous: string, next: string): PairClassification;
export function parseDumpLine(line: string): TracedEvent | null;
export function analyzeEvents(events: readonly TracedEvent[]): StreamReport[];
export function behaviorOf(report: StreamReport): string;
export function formatReport(reports: readonly StreamReport[]): string;
```

`classifyPair` tries, in order: an exact repeat; a strict prefix relation in
either direction (extend / shrink); a word run of **at least two words** shared
at the seam (`slide`, the same threshold as `src/stt/transcript-merge.ts`); then
the same test with every whitespace removed, which catches a provider re-spacing
its own words (`MAI.` against `MA I.`) and sets `retokenized`. A single shared
word is never an overlap: two sentences genuinely can start with the same word.

`StreamReport` carries the per-stream counts, the `dominant` transition, the
largest seam overlap, `duplicatedChars` (the characters a naive accumulator would
show twice) and `retokenizedPairs`.

Run it with Bun:

```sh
SPEECHTT_STT_PARTIALS=1 cargo run -p speechtt-ui 2> dump.log
bun src/stt/trace-analyze.ts dump.log
```

## Contract — the channel split (the chosen one)

### 1. What each channel means

```rust
/// Folds one `Delta` into the buffer.
///
/// A delta is text the provider has already committed, and three shapes were
/// measured: an increment (` Trans`, `cribe`), an increment that continues the
/// word it follows (so nothing may be inserted between them), and the whole take
/// re-sent (Gemini). The delta is therefore appended **verbatim**, replacing the
/// committed text when it is cumulative and ignored when it is an exact
/// duplicate.
fn apply_delta(&mut self, delta: &str);

/// Folds one `Partial` into the buffer.
///
/// A partial is a *preview* of the window being spoken: a provider may grow it,
/// shrink it, re-punctuate words it already showed, slide it forward, or replace
/// it wholesale after a language switch. All of those are one thing here — the
/// current best text for right now — so the preview is **replaced**, never
/// settled into the committed text.
fn apply_partial(&mut self, text: &str, segment: Option<&str>);

/// Joins committed text and the preview for display.
///
/// Three rules, in order: a preview entirely contained at the end of the
/// committed text adds nothing; a word run shared at the joint is a duplicate
/// when it spans two words, or one word while the preview continues past it;
/// otherwise the two are joined by one space.
fn seam_join(committed: &str, live: &str) -> String;
```

`TranscriptBuffer` keeps its two fields and changes what they hold:

- `committed` — text the provider _committed_: every `Delta`, every `Committed`
  payload, and the `Stopped` text when non-empty.
- `live` — the current partial preview.

`full_text()` returns `seam_join(committed, live)`.

### 2. The event rules

| Event               | Rule                                                                                                                                           |
| ------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- |
| `Delta`             | append verbatim to `committed`; replace when `delta.starts_with(committed)` (cumulative); ignore when `committed.ends_with(delta)` (duplicate) |
| `Partial`           | `live = text.trim()`, `live_segment = segment`. No settle, no comparison with the previous preview                                             |
| `Committed`         | seam-join its text into `committed`, clear `live`                                                                                              |
| `Stopped` non-empty | `committed = full_text`, `live` cleared — authoritative                                                                                        |
| `Stopped` empty     | `committed = seam_join(committed, live)`, then clear `live`                                                                                    |

Nothing else changes: `clear()`, the public accessors and every caller of
`full_text()` keep their contracts.

### 3. A finished take is settled

`FinishedTake` used to store the two halves and let Copy/Save re-join them with a
plain space. That was correct while `live` was always a continuation; it is not
any more, because a preview may repeat what `committed` already holds. A finished
take therefore stores the **displayed** text in its committed half and leaves
`live` empty — a take that is over has no preview.

This is not cosmetic: with the old join, a take whose preview restated its
committed text was copied and saved twice over.

### 4. A named utterance that changes is a boundary

A provider can open a **second utterance inside one take**. ElevenLabs Scribe v2
Realtime did exactly that: segment `…242:0` ran for 34 partials and ended at
`…le modèle choisi pour la`, then segment `…242:1` opened at `Et certainement`.

Replacing the preview unconditionally threw the first utterance away: the screen
collapsed from 328 characters to `Et` in the middle of the dictation, and the
reader lost the take and watched it start again. A preview whose segment differs
from the one being shown is therefore **settled** into the committed text through
`seam_join` before the new utterance takes the preview. A preview with no
identity, or the same identity, still just replaces.

The captured take then reads as one continuous transcript, and the regression
test asserts the display never shrinks and never loses either utterance.

**The boundary word is lost, and cannot be recovered from the previews.** The
first utterance ends at `pour la` and the second opens at `Et certainement`; the
word `transcription.` exists only in the provider's `stopped` text. The live
display is therefore one word short at that boundary until the take ends, where
the authoritative text corrects it. That is a property of the provider's stream,
not of the reducer, and the test states it rather than hiding it.

### 5. Known limitation, and how to catch it

The channel split assumes the preview is **at or after** the committed text, which
is what all three measured providers do. A provider that sends a delta
_continuing an open preview_ (`Partial "audio"` then `Delta " capture"`) reverses
that order, and the display would show `capture audio`.

No measured provider does this, and every guard tried against the captures broke
one of them — which is the signal that the condition cannot be derived from the
data. The rule is therefore left out rather than guessed. The `Partial "audio"`
then `Delta " capture"` shape is a test fixture written before this ticket, and it
has been rewritten to the measured order.

If such a provider appears, the dump is the instrument that will show it: its
`partials=` and `deltas=` lines interleaved in `seq` order reveal an increment
that extends an open preview. The fix then has data to rest on, and the six UI
tests that used the old order are the place to start.

### 6. Tests

- The **ElevenLabs capture** replays with zero duplication: no frame exceeds 241
  characters, no two-word run repeats at any frame, and the final text is the last
  hypothesis because its `stopped` was empty.
- The **MAI capture** grows: the verbatim concatenation of its deltas is its
  185-character `stopped` text, the display never exceeds it, and the mid-word
  join (` Trans` + `cribe`) produces one word, not `Trans cribe`.
- The **Gemini capture** does not double: its whole-take delta and its whole-take
  preview join to 117 characters, not 234.
- A retroactive rewrite replaces the preview in place: before and after differ
  only where the provider re-punctuated.
- A language switch (`It was` → `Et voici le`) replaces the preview rather than
  accumulating.
- `Stopped` with empty text keeps the preview; `Stopped` with text replaces
  everything.
- A `Committed` event still commits and clears the preview.
- A repeated single word inside one hypothesis survives (`très très`); a two-word
  run shared at a seam does not, and neither does a single shared word when the
  preview continues past it.
- Every pre-existing `transcript.rs` test that encoded the settle behaviour is
  **rewritten to the new contract**, not deleted: the invariants that still hold
  (`committed` never shrinks, no word dropped on `Committed`, whitespace never
  doubles) keep their assertions.

## Appendix — the overlap-aware reducer that was rejected

**Not selected.** Recorded only so the analysis is not repeated from scratch if a
future provider forces it.

Everything below describes the overlap-aware reducer. It is **not** a commitment:
it is written out so that choosing A after the measurements costs an
implementation rather than a design round.

### 1. Comparison key

Overlap and revision comparisons run on a **compact key** rather than on the raw
string, because the provider re-tokenizes:

```rust
/// Case-folded text with every whitespace character removed.
///
/// `d'une` and `d 'une` share the key `d'une`; `MAI` and `MA I` share `mai`.
/// The key is only ever used for comparison — the buffer always stores and
/// renders the provider's own spelling and spacing.
fn compact_key(text: &str) -> String;
```

Accents are **kept** (`transcription` ≠ `transcription` without accents is not a
case this provider produces, and stripping them would merge distinct French
words). Only casing and whitespace are folded.

Each compact key must be able to map a compact offset back to a character offset
in the original string, so the tail can be extended with the provider's own
slice of the new text. Implement it as one helper returning both, e.g.:

```rust
/// A comparison form of `text` plus, for every compact index, the byte offset
/// in `text` of the character that produced it.
struct Compact { key: String, offsets: Vec<usize> }
fn compact(text: &str) -> Compact;
```

### 2. Classifying a `Partial` — three outcomes

```rust
/// What a `Partial` does to the current live tail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TailChange {
    /// The same window, re-sent: replace the tail wholesale.
    Revision,
    /// A later window of the same utterance: append only the remainder that
    /// the tail does not already end with.
    Continuation { skip_bytes: usize },
    /// The first words of a new window: settle the tail, then replace it.
    NewWindow,
}

fn classify_partial(&self, text: &str) -> TailChange;
```

Rules, in order, on `tail_key = compact(live).key` and
`next_key = compact(text).key` (both trimmed first; an empty tail is always
`Revision`):

1. **Revision** when `next_key.starts_with(&tail_key)` or
   `tail_key.starts_with(&next_key)`. This widens the current rule to ignore
   spacing and case, and keeps its existing "a shorter draft of the same window
   is still that window" behaviour (no half-length guard needed: a shorter
   prefix always belongs to the same window).
2. **Continuation** when the longest suffix of `tail_key` that is also a prefix
   of `next_key` spans **at least two words** of the new text. `skip_bytes` is
   the byte offset in `text` just past that overlap, taken from `Compact::offsets`
   so the provider's own spacing is preserved.
3. **NewWindow** otherwise.

Overlap word counting uses the raw `text`: the overlap boundary is placed at the
start of a word, so a single repeated word (`chat` → `chat noir`) is not
swallowed and a repeated two-word run (`le modèle` → `le modèle MAI`) is.

### 3. Applying the outcome

`apply` keeps its current shape, with `Continuation` inserted:

- `Revision` → `begin_utterance(segment)`, `self.live = text`.
- `Continuation { skip_bytes }` → `begin_utterance(segment)`, then append
  `text[skip_bytes..]` to `self.live` with exactly one space when the tail does
  not already end with whitespace and the remainder does not start with one.
  An empty remainder changes nothing (the tail already ends with this window).
- `NewWindow` → `settle_live()`, `begin_utterance(segment)`, `self.live = text`.

`Delta`, `Committed`, `Stopped` and `begin_utterance` are unchanged.

### 4. Invariants (tests pin these)

- `committed` never shrinks and never re-appends text it already ends with.
- A genuine repeated two-word run inside one hypothesis is _not_ lost:
  `très très` stays `très très` (an intra-word run is not a tail overlap).
- A new utterance with a matching opening still starts a new window:
  `Et ensuite le workflow va consister` followed by `Et` keeps the first
  sentence committed (the existing behaviour, still required).
- The replayed capture below shows **no repeated run longer than one word** and
  the words appear in the provider's order.

### 5. The regression fixture

The test module gains one test that replays the capture above as a sequence of
`Partial` events under one `segment`, then the authoritative `Stopped`, and
asserts:

- the live text after every event has no two-word run repeated back to back
  (`"le modèle le modèle"`, `"dictée avec. dict ée avec"` …);
- `full_text()` before `Stopped` contains each distinctive phrase exactly once —
  at minimum `"Début d'une nouvelle dictée"`, `"MAI Transcribe to Streaming"`
  and `"plein de doublons"`;
- `committed()` is monotonic across the replay;
- after `Stopped` the buffer equals the authoritative text exactly.

The event list is transcribed from the capture; the test states it verbatim so
the next change to the reducer has to keep this take readable.

## TDD Tasks

- [x] **T31.0 — Instrument.** `trace.ts` + spec: off unless
      `SPEECHTT_STT_PARTIALS` is exactly `1`, one line per event, JSON-safe newlines,
      only transcript-bearing types, header naming provider/model/language.
      `worker.ts` wraps `emit` once.
- [x] **T31.1 — Classify.** `trace-analyze.ts` + spec: `classifyPair` covers
      extend, shrink, identical, a two-word slide, a re-spaced seam and a single
      shared word that must stay `new`. **Partials and deltas are analysed on separate
      channels**, because an increment compared against a hypothesis describes
      nothing. Reports are grouped by model and the duplication factor is measured
      against the `stopped` text, the only ground truth in a dump.
- [x] **T31.2 — Collect.** Three takes, three shapes:
      `elevenlabs/scribe-v2-realtime` (cumulative, rewrites its past, no delta),
      `microsoft/mai-transcribe-2-streaming` (sliding partials + incremental deltas)
      and `google/gemini-3.5-transcribe-live` (cumulative, whole-take delta). Reports
      in `### Measurements`.
- [x] **T31.3 — Decide.** **Channel split**, from the measurement that MAI's 24
      deltas concatenate to its `stopped` text exactly while its 79 partials are
      previews.
- [x] **T31.4 — Implement.** `apply_partial` replaces the preview; `apply_delta`
      appends to `committed`; `full_text()` becomes `seam_join`. Replay both captures
      as tests, rewrite the pre-existing settle tests to the new contract.
- [x] **T31.5 — Verify against a third model.** `google/gemini-3.5-transcribe-live`
      measured, and it forced two contract changes: its delta is the **whole take**,
      not an increment, and its preview duplicates that delta entirely — hence the
      "preview already contained at the end of the committed text" rule. Deltas also
      concatenate **without a separator** (` Trans` + `cribe`), which the MAI capture
      proves and an inserted space would break.
- [x] **T31.6 — Docs and gates.** Module headers per `AGENTS.md` §3, then
      `cargo fmt -- --check`, `cargo clippy --all-targets -- -D warnings`,
      `cargo test`, `bun test`, `bun check src/stt`, `bun run design:validate`.

## Acceptance Criteria

- `SPEECHTT_STT_PARTIALS=1` writes one JSON line per hypothesis to `stderr`,
  tagged `[stt-partials]`, with a header naming provider and model; with the
  variable unset or not `1`, nothing is written and behaviour is unchanged.
- `bun src/stt/trace-analyze.ts dump.log` prints, per model, the authoritative
  text length, then a line per channel with its transition counts, the largest
  seam overlap and how much the shipped reducer would duplicate.
- Partials and deltas are reported separately; a provider that uses both is
  visible as two lines, not one misleading one.
- Two takes appended to one file stay apart even though both call their session
  `take-1`.
- The live transcript never repeats a two-word run back to back, on either
  measured model, at any frame of the replay.
- The displayed text never exceeds the provider's authoritative text.
- The MAI take shows a **growing** transcript, reconstructed from its deltas, and
  ends equal to its `stopped` text.
- The ElevenLabs take shows its cumulative preview and, because its `stopped` was
  empty, ends with the last hypothesis rather than a blank.
- A repeated single word survives; a repeated two-word run at a seam does not.
- All three measured models display exactly their authoritative text at the
  worst frame, with zero repeated word runs — including MAI, whose 185-character
  ceiling needs the one-word continuation clause.
- All six gates are green with zero warnings and no skipped tests.

## Implementation Notes

Delivered by two coding sub-agents in parallel with disjoint scopes (the reducer,
and a capture-regression test), then audited and completed by the CTO.

### What each half produced

- **Reducer** (`crates/speechtt-core/src/transcript.rs`): `apply_delta` appends
  verbatim, replaces on a cumulative delta, ignores a duplicate; `apply_partial`
  replaces the preview with no settle; `seam_join` implements the three display
  rules; `Committed`/`Stopped` follow the contract. 37 pre-existing tests kept
  (17 renamed to the new vocabulary), 17 added, none deleted.
- **Capture regression**
  (`crates/speechtt-core/tests/transcript_captures.rs`): replays all three
  captures through the public API and asserts, after **every** event, that the
  display never exceeds the authoritative length, shows no repeated two-word run,
  and that `committed` never shrinks; plus one guard per model.

### Defects found by the work itself

1. **The two-word seam threshold was insufficient.** The capture test proved two
   MAI frames overshoot the 185-character take on a **single-word** joint
   duplicate (`découvrir` + `découvrir à la fin…`, `cette` + `cette session.`).
   The contract gained the clause: a shared run is removed when it spans two
   words **or** when it is one word and the preview continues past it. Verified
   against all three captures: 241 / 185 / 117 at the worst frame, zero repeated
   runs.
2. **`FinishedTake` doubled a settled take.** Copy and Save re-joined the two
   halves with a plain space, correct while `live` was a continuation and wrong
   once a preview can restate `committed`. A finished take now stores the
   displayed text and leaves the preview empty.
3. **The old fixtures used the reverse channel order.** Six UI tests emitted
   `Partial "audio"` then `Delta " capture"`, producing `capture audio`. They now
   emit the measured order (committed deltas first, then a preview), which keeps
   each test's user-visible intent.

### Rejected: guessing a guard for the reverse order

Every variant tried — fold the preview when the delta is disjoint, with a
normalized containment test — fixed the fixture and **broke a real capture**
(Gemini doubling to 233, MAI overshooting by 14 with a repeated run). A condition
that cannot be derived without breaking measured data is a guess, so it was left
out and recorded as the ticket's known limitation, with the dump as the
instrument that would reveal such a provider.

### Gates (repository root, zero warnings, no skipped tests)

| Command                                     | Result                   |
| ------------------------------------------- | ------------------------ |
| `cargo fmt -- --check`                      | clean                    |
| `cargo clippy --all-targets -- -D warnings` | clean                    |
| `cargo test`                                | **583 passed, 0 failed** |
| `bun test`                                  | 348 passed, 0 failed     |
| `bun check src/stt`                         | 32 files, no type errors |
| `bun run design:validate`                   | 66 tokens, 0 errors      |

### Left for the user

A real dictation on the model of their choice, watching the live area: it must
read as one sentence with no stutter, and the finished screen must show the same
text that was on screen. No GUI run or screenshot was taken autonomously.

## Second round — the segment boundary

Found by replaying a real ElevenLabs Scribe v2 Realtime take that opened two
utterances. The first reducer discarded the preview on **every** partial, so a
segment change wiped the screen: `collapsed from 328 to 2 characters after
Partial "Et"`. Fixed by settling the preview at a segment boundary, pinned by
`elevenlabs_keeps_the_first_utterance_when_a_second_one_opens` over a new
`fixtures/elevenlabs-two-segments.tsv` capture, and by three rewritten unit tests
that had encoded the old "a new segment replaces the hypothesis" rule.

The same take also proved the boundary word (`transcription.`) lives only in the
provider's final text, so the live display is one word short until `stopped`
lands. Recorded above rather than papered over.

## Validation

Closed after two field dictations. The user's report: _"tout n'est pas forcément
stable lors de la dictée mais globalement le contenu est préservé."_

That is the contract this ticket set out to satisfy. Live text can still shift as
a provider re-punctuates its own window or opens a new utterance, and one word at
a segment boundary can be missing until `stopped` lands — but no words are lost
and nothing is duplicated, which is what made the earlier attempts unusable.

### Known behaviour, accepted

- **Sliding-window models** (MAI Transcribe 2 Streaming) show only the current
  fragment in the preview; the full take appears from the delta channel and is
  finalised by `stopped`.
- **A segment boundary** can be missing its junction word until the take ends,
  because the provider never emits it on either channel.
- **A provider that continued an open preview with a delta** would reverse the
  channel order and display `capture audio`; no measured provider does it, and
  the guard was left out because every variant tried broke a real capture. The
  dump is the instrument that would reveal such a provider.
