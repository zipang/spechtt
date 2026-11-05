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

Classify a `Partial` against the tail with a whitespace- and case-insensitive
comparison, and add the third outcome the provider needs: a **window
continuation**, where the tail is extended with only the non-overlapping
remainder instead of being settled and repeated.

## Scope Boundary

Allowed to create or modify:

- `crates/speechtt-core/src/transcript.rs` (reducer + its test module)
- `tickets/README.md` and this ticket

Out of scope — do **not** touch:

- `crates/speechtt-ui/**` (the screen already renders `committed` + `live`; the
  buffer is the single source of truth for both)
- `src/stt/**` (the adapter must keep forwarding the provider's events as they
  are; the reducer owns the policy)
- `src/stt/transcript-merge.ts` (segmented junction merging is already correct
  and stays the reference policy, not a dependency)
- `crates/speechtt-core/src/protocol.rs` (no wire change)
- `DESIGN.md` (no visual change)

## Contract

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

- [ ] **T31.1 — Red: `compact`.** Failing tests: whitespace and case are folded
  (`d'une` / `d 'une`, `MAI` / `MA I`), accents survive, offsets map every
  compact index back to a byte offset in the source, an empty string yields an
  empty key.
- [ ] **T31.2 — Red: classification.** Failing tests for `classify_partial`:
  cumulative growth with different spacing → `Revision`; sliding window
  (`ée avec le modèle MAI.` → `le modèle MAI.`) → `Continuation` with the right
  `skip_bytes`; a two-word overlap → `Continuation`; a single repeated word →
  `NewWindow`; an unrelated window → `NewWindow`; an empty tail → `Revision`.
- [ ] **T31.3 — Red: the regression capture.** Add the replay test from §5 and
  watch it fail against today's reducer. Keep the failure output as the before
  picture.
- [ ] **T31.4 — Green: reducer.** Implement `compact`, `classify_partial` and
  the `Continuation` branch in `apply` until T31.1–T31.3 pass. Existing tests
  must all still pass unchanged — in particular
  `a_short_fragment_matching_the_opening_starts_a_new_window`,
  `a_shorter_draft_of_the_same_window_replaces_the_tail`,
  `a_repeated_word_is_not_swallowed_as_a_duplicate` and
  `committed_never_shrinks_and_keeps_every_word_across_four_segments`.
- [ ] **T31.5 — Docs.** The module header states the three outcomes and why the
  comparison is spacing-insensitive (`AGENTS.md` §3: present tense, callers
  named, no ticket ids, no change history).
- [ ] **T31.6 — Gates.** `cargo fmt -- --check`,
  `cargo clippy --all-targets -- -D warnings`, `cargo test`, `bun test`,
  `bun check src/stt`, `bun run design:validate`. Record exact counts in
  `## Implementation Notes`.

## Acceptance Criteria

- A live take with a windowing provider no longer shows fragments repeated back
  to back while dictating; the screen reads as one continuous sentence.
- The provider's re-tokenization (`d 'une`, `dict ée`, `MA I`, `Trans cribe`) no
  longer produces duplicates.
- No word the provider produced is dropped, except a genuine ≥2-word overlap run
  that the previous hypothesis already ends with.
- The final `Stopped` text still replaces everything, and the result screen is
  unchanged.
- Every pre-existing test in `transcript.rs` passes without modification.
- All six gates are green with zero warnings and no skipped tests.
- Final visual review of a real dictation is left to the user.
