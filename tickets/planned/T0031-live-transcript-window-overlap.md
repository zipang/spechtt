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
by it, so two takes that both call their session `take-1` stay apart.

**`elevenlabs/scribe-v2-realtime` — 24 hypotheses, no `delta` at all.**
`identical=1 extend=8 shrink=2 slide=5 new=7`. Each hypothesis is the **whole take
so far** (9 → 241 characters) and the first 50–99 % is byte-identical to the
previous one. But the provider **rewrites its own earlier words**: at 19→20 it
inserted French guillemets around "streaming" from character 117 onwards, at
23→24 it removed them, at 24→25 it put them back. The take then failed with
`Transcription provider stream error` and `stopped` arrived with an **empty**
text.

**`microsoft/mai-transcribe-2-streaming` — 79 partials and 24 deltas, one segment
for the whole take.** Two channels, and they mean different things:

- the **partials** are short sliding fragments — `Nouveau`, `Nouveau test.`, then
  ` fois-ci avec.`, ` avec le modèle`, ` le modèle M.` … — each one a window of a
  few words, with 1–3 words of overlap at the seam;
- the **deltas** are increments: `Nouveau`, ` test cette`, ` fois-ci`, ` avec`,
  ` le`, ` modèle`, ` MAI`, ` Trans`, `cribe`, ` to`, ` Streaming et`, …

**Concatenating the 24 deltas reproduces the `stopped` text exactly** — 185
characters, identical string. The partials carry no information the deltas do
not already hold; they are a preview of the window being spoken.

`google/gemini-3.5-transcribe-live` — still unmeasured.

### What the shipped reducer does with that data

Simulated faithfully against the shipped `TranscriptBuffer` rules, deltas
included:

| Take | Authoritative text | Shipped reducer, worst frame | Settles |
|---|---|---|---|
| ElevenLabs (24 hypotheses) | 241 chars (no final: provider failed) | 1 755 chars | 12 |
| MAI (79 partials + 24 deltas) | 185 chars | 1 049 chars mid-take | 32 |

Both smear while dictating. MAI happens to end correct because a non-empty
`stopped` replaces everything — the damage is confined to the live view. The
ElevenLabs take ends wrong: its `stopped` was empty, so the 1 755-character smear
is what the user was left with.

### Root cause, precisely

The reducer treats **every** `Partial` as a possible new window and *settles* the
previous tail into `committed` when the text is not a prefix of it. Two measured
behaviours defeat that test:

- a **retroactive rewrite** (ElevenLabs adding a full stop or guillemets to words
  already shown) — the prefix test fails, the whole window is settled, then
  re-shown as the live tail;
- a **sliding fragment** (MAI) — the window never was a prefix, so every fragment
  settles the one before it.

The second half of the bug is that the reducer never uses the `delta` channel for
MAI at all: it appends deltas to the *live tail*, which the next partial then
replaces. The one channel that reconstructs the take exactly is thrown away.

### Decision — channel split

**Deltas accumulate; partials preview; the two are joined at a word seam.**

| Event | What it is | What the buffer does |
|---|---|---|
| `Delta` | real incremental text | **append to `committed`** |
| `Partial` | a preview of the current window | **replace `live`** — never settle |
| `Committed` | the provider closed an utterance | commit it, clear `live` |
| `Stopped` with text | the authoritative take | replace both |
| `Stopped` empty | a take that died | fold `live` into `committed` at the seam |

The displayed text is `seam_join(committed, live)`: concatenation with the shared
word run removed at the joint.

Measured against both captures, at **every** frame, not only at the end:

| Take | Displayed at the worst frame | Real text | Repeated 2-word runs |
|---|---|---|---|
| ElevenLabs | 241 chars | 241 | **0** |
| MAI | 185 chars | 185 | **0** |

This is better than the two strategies this ticket started with, and the
measurements are why:

- **A (overlap-aware partial reducer)** cannot work: a retroactive rewrite leaves
  no seam to measure, and MAI's fragments overlap by 1–3 words, below any safe
  threshold.
- **B (show only the newest hypothesis)** loses MAI entirely: the screen would
  show ` session.` at the end of the take.
- **Channel split** gives MAI a *growing* transcript reconstructed from its deltas
  — 185 characters exactly — and gives ElevenLabs its clean cumulative preview.
  Neither provider is special-cased: the split follows what the two SDK channels
  mean.

The seam dedupe is the only heuristic left, and it is the same rule
`src/stt/transcript-merge.ts` already applies to segmented junctions: a repeated
word run of at least two words at the joint is a duplicate, a single repeated
word is real speech.

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

## Contract — the channel split (the chosen one)

### 1. What each channel means

```rust
/// Folds one `Partial` into the buffer.
///
/// A partial is a *preview* of the window being spoken: a provider may grow it,
/// shrink it, re-punctuate words it already showed, or slide it forward. All of
/// those are the same thing here — the current best text for right now — so the
/// preview is **replaced**, never settled into the committed text.
fn apply_partial(&mut self, text: &str, segment: Option<&str>);

/// Folds one `Delta` into the buffer.
///
/// A delta is real incremental text, not a preview: the provider has committed
/// those words. It is appended to the **committed** text, where a later partial
/// cannot overwrite it.
fn apply_delta(&mut self, delta: &str, segment: Option<&str>);
```

`TranscriptBuffer` keeps its two fields and changes what they hold:

- `committed` — text a provider *closed*: every `Delta`, every `Committed`
  payload, and the `Stopped` text when non-empty.
- `live` — the current partial preview.

`full_text()` becomes a seam join rather than a plain space join:

```rust
/// `committed` followed by the preview, with the word run they share removed at
/// the joint.
///
/// Two words repeated at a seam is a duplicate — the measured providers restate
/// 1–3 words when a window moves — while a single repeated word is real speech
/// ("très très"), so the run threshold is two, exactly as
/// `src/stt/transcript-merge.ts` applies it to segmented junctions.
fn seam_join(committed: &str, live: &str) -> String;
```

### 2. The four event rules

| Event | Rule |
|---|---|
| `Delta` | append to `committed` (the existing incremental/cumulative classification still decides replace-or-append inside the committed text) |
| `Partial` | `live = text.trim()`; `live_segment = segment`. No settle, no comparison with the previous preview |
| `Committed` | push its text into `committed`, clear `live` |
| `Stopped` non-empty | `committed = full_text`, `live` cleared — authoritative |
| `Stopped` empty | fold `live` into `committed` through `seam_join`, then clear it |

Nothing else changes: `clear()`, the public accessors and every caller of
`full_text()` keep their contracts.

### 3. Why not the two strategies this ticket first proposed

- **A — overlap-aware partial reducer.** A retroactive rewrite (ElevenLabs adding
  a full stop or guillemets to words already displayed) leaves **no seam** to
  measure, and MAI's fragments overlap by only 1–3 words, below any threshold that
  would not also swallow real repetition. Measured, not assumed.
- **B — show only the newest hypothesis.** It discards MAI's delta channel, which
  reconstructs the take exactly: the screen would end the take showing
  ` session.` instead of 185 characters of real text.

### 4. Tests

- The **ElevenLabs capture** replays with zero duplication: the displayed text
  never exceeds 241 characters, no two-word run repeats back to back at any
  frame, and the final text is the last hypothesis (the take's `stopped` was
  empty).
- The **MAI capture** replays into a *growing* transcript: concatenating the
  deltas reproduces the 185-character `stopped` text exactly, and the display
  never exceeds it.
- A retroactive rewrite replaces the preview in place: the text before and after
  differs only where the provider re-punctuated.
- `Stopped` with empty text keeps the preview; `Stopped` with text replaces
  everything.
- A `Committed` event still commits and clears the preview.
- A repeated single word survives the seam (`très très`), a repeated two-word run
  does not.
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
- [x] **T31.2 — Collect.** Three takes attempted: `elevenlabs/scribe-v2-realtime`
  and `microsoft/mai-transcribe-2-streaming` measured; `google/gemini-3.5-transcribe-live`
  still to do. Reports in `### Measurements`.
- [x] **T31.3 — Decide.** **Channel split**, from the measurement that MAI's 24
  deltas concatenate to its `stopped` text exactly while its 79 partials are
  previews.
- [ ] **T31.4 — Implement.** `apply_partial` replaces the preview; `apply_delta`
  appends to `committed`; `full_text()` becomes `seam_join`. Replay both captures
  as tests, rewrite the pre-existing settle tests to the new contract.
- [ ] **T31.5 — Verify against a third model.** Capture
  `google/gemini-3.5-transcribe-live`, run the analyzer, and append its report.
  If it uses a third channel shape, extend the contract before closing.
- [ ] **T31.6 — Docs and gates.** Module headers per `AGENTS.md` §3, then
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
- `google/gemini-3.5-transcribe-live` is measured before the ticket closes.
- All six gates are green with zero warnings and no skipped tests.
