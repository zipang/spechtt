# T0031 — Stop the live transcript from accumulating duplicated fragments

- **Track**: Rust core (`transcript.rs`)
- **Wave**: 1 (independent)
- **Depends on**: nothing
- **Blocks**: nothing
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

1. It **slides**: the new hypothesis starts with the *suffix* of the previous
   one (`ée avec le modèle MAI.` → `le modèle MAI.`), not with its whole prefix.
2. It **re-tokenizes**: the same words come back with different spacing
   (`d'une` → `d 'une`, `dictée` → `dict ée`, `MAI` → `MA I`, `Transcribe` →
   `Trans cribe`), so an exact `starts_with` fails even for a genuinely
   cumulative hypothesis.

Every such event settles the old tail and starts fresh, so `committed`
accumulates every overlap. The screen shows `committed` followed by `live`,
which is exactly the smear above.

### Goal, and why the algorithm is not decided yet

Previous attempts at this problem picked a merge rule first and discovered
afterwards that it did not hold for every provider. That is the mistake this
ticket refuses to repeat: **the reducer's behaviour has to follow what the
providers actually emit**, and that has never been measured on more than one
model.

So this ticket is split in two halves, and only the first is specified in
detail:

1. **Measure (in scope, detailed below).** `SPEECHTT_STT_PARTIALS=1` dumps every
   hypothesis a provider produces, and `src/stt/trace-analyze.ts` classifies each
   consecutive pair as `extend` / `shrink` / `slide` / `identical` / `new`,
   measuring the seam overlap and the characters a naive accumulator would
   duplicate. Two takes on two models answer "do all models behave the same?"
   with numbers instead of impressions.
2. **Decide (recorded here, implemented after the data).** Two candidate
   strategies, chosen once the dump exists. No reducer change lands before that
   choice is written into this ticket.

### The two candidate strategies

**A — Overlap-aware reducer.** Classify each hypothesis against the live tail
with a whitespace-folded comparison, and extend the tail with only the
non-overlapping remainder instead of settling the whole previous window.
Keeps a growing transcript during a long take, but it is the approach that has
failed before: each provider needs its own threshold, a re-tokenizing provider
defeats a word-level comparison, and a wrong guess silently drops or duplicates
words in the middle of the text.

**B — Show the window, trust the final text.** During the take, the live area
shows only the newest hypothesis, replacing the previous one. Nothing is
accumulated, so nothing can duplicate. The complete transcript arrives with
`Stopped` (`fullText`), which the reducer already adopts as authoritative.

B is dramatically simpler and cannot produce the smear, at one real cost: during
a long take the screen shows only the last window, and the transcript fills in at
the end rather than growing. It is also exactly what the streaming adapter makes
available — it emits `transcript-final` into its accumulator without sending a
`Committed` event, so a live take has **no settled text at all** to build on.

**Decision rule.** If the dump shows every model extending its hypothesis
cumulatively, A is unnecessary: keep today's reducer. If it shows sliding
windows on any model the catalogue offers, B is the honest default, because A
can only ever approximate what the provider is doing. A is justified only if a
provider commits settled text mid-take (a `Committed` event with no overlap) that
B would throw away.

### Measured so far (one model, from a real take)

`microsoft/mai-transcribe-2-streaming`, replaying the capture in this ticket:
11 hypotheses, `identical=1 extend=1 slide=7 new=1`, seam overlap up to 4 words,
1 pair whose seam only matched after folding whitespace away. That is a sliding
window, and it is what the reducer's `settle_live()` path turns into the smear.
Still needed: a second model (`openai/gpt-realtime-whisper` accumulates
differently) to know whether A and B must coexist.

## Scope Boundary

Allowed to create or modify:

- `src/stt/trace.ts` + `src/stt/trace.spec.ts` (new: the opt-in dump)
- `src/stt/trace-analyze.ts` + its spec (new: the classifier that reads a dump)
- `src/stt/worker.ts` (wrap `emit` when the dump is on)
- `crates/speechtt-core/src/transcript.rs` (reducer + tests, **only after the
  strategy is chosen** — see the decision rule above)
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
 * Describes the take the dump belongs to. Two models land in one log, so the
 * header is what makes the comparison possible.
 */
export function formatSessionHeader(details: {
  provider: string; model: string; language?: string;
  sampleRate?: number; kind: "live" | "file";
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

## Contract — Strategy A, kept as a specification if it is chosen

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
- A genuine repeated two-word run inside one hypothesis is *not* lost:
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

- [ ] **T31.0 — Instrument.** `trace.ts` + spec: off unless the variable is
  exactly `1`, one line per event, JSON-safe newlines, only transcript-bearing
  types, header naming provider/model/language. `worker.ts` wraps `emit` once.
  The dump must not change behaviour when disabled (pinned by a test that runs
  the loop with the flag off and asserts no extra `stderr`).
- [ ] **T31.1 — Classify.** `trace-analyze.ts` + spec: `classifyPair` covers
  extend, shrink, identical, a two-word slide, a re-spaced seam that only the
  compact comparison sees, and a single shared word that must stay `new`.
  `analyzeEvents` keeps segments apart, ignores non-hypotheses, and reports
  `duplicatedChars` from the real overlap rather than from a length delta.
- [ ] **T31.2 — Collect.** Two real takes, one per model, with the dump on:
  a windowing model and an accumulating one. Attach both reports to this ticket
  under `## Measurements`, with the hypothesis counts and the dominant
  behaviour per stream.
- [ ] **T31.3 — Decide.** Write the choice (A, B, or "no change") into this
  ticket with the measurement that justifies it, and only then implement:
  - **B** — `TranscriptBuffer` gains a window mode: a `Partial` replaces the live
    tail without settling, `Committed` still commits, and `Stopped` adopts
    `fullText`. The regression fixture replays the capture and asserts the live
    text is never a smear (no two-word run repeated back to back) and that the
    final text is exact.
  - **A** — the compact-key contract this ticket used to describe, unchanged.
  - **No change** — close the ticket with the measurement showing every offered
    model already extends cumulatively.
- [ ] **T31.4 — Docs and gates.** Module headers per `AGENTS.md` §3, then
  `cargo fmt -- --check`, `cargo clippy --all-targets -- -D warnings`,
  `cargo test`, `bun test`, `bun check src/stt`, `bun run design:validate`.

## Acceptance Criteria

- `SPEECHTT_STT_PARTIALS=1` writes one JSON line per hypothesis to `stderr`,
  tagged `[stt-partials]`, with a session header naming provider and model; with
  the variable unset or not `1`, no line is written and behaviour is unchanged.
- `bun src/stt/trace-analyze.ts dump.log` prints, per stream, the dominant
  behaviour, the transition counts, the largest seam overlap and the characters a
  naive accumulator would duplicate.
- The classifier marks a re-spaced seam (`MAI.` / `MA I.`) as the same window, so
  a provider that re-tokenizes is not reported as opening new windows.
- Two models are measured and the comparison is written into this ticket.
- The reducer strategy is chosen from that measurement and named in this ticket
  before any reducer change lands.
- If Strategy B is chosen: the live area never shows a smeared transcript, the
  final text equals `Stopped`'s `fullText` exactly, and the whole-take Copy/Save
  payload is the final text.
- If Strategy A is chosen: no two-word run is repeated back to back, and every
  pre-existing `transcript.rs` test still passes unmodified.
- All six gates are green with zero warnings and no skipped tests.
