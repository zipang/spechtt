# T0012 — Silence-Aligned Audio Segmentation for Batch Transcription

- **Track**: TypeScript/Bun worker (`src/stt/`) — no Rust, no protocol change
- **Wave**: 2 (independent of T0011; disjoint file scope)
- **Depends on**: T0003 (segmented adapter and WAV framing)
- **Blocks**: nothing (enables a later follow-up: previous-transcript context
  via `providerOptions`)
- **References**: `PRODUCT.md` Phase 3 (batch fallback), `AGENTS.md` §5
  (hermetic tests), `tickets/done/T0003-bun-stt-worker.md` §segmented contract
- **Owner**: coding sub-agent (`opencode-go/deepseek-v4.1-flash`), supervised by
  CTO
- **Started**: 2026-10-28

## Context & Goal

Batch/segmented models (`mistral/voxtral-*`, `openai/whisper-1`,
`google/gemini-3.5-transcribe`, …) receive audio in segments produced by
`MistralSession` (`src/stt/adapters/mistral.ts`). The current strategy is a
**byte-count cut**:

- `segmentBytes = sampleRate × 2 × segmentSeconds` with `segmentSeconds = 2.5`
  (`mistral.ts:175-178`, `mistral.ts:330`);
- the buffer is flushed the moment it crosses that count (`mistral.ts:193`),
  on `pause()` (`mistral.ts:200`) and on `stop()` (`mistral.ts:213`);
- each flush is WAV-framed and sent to `transcribe()`, serialized through a
  promise chain (`mistral.ts:263`);
- the resulting text is appended with `appendSegment()` — `trim()` + a space,
  **no junction dedupe** (`vercel-ai-sdk.ts:45`).

Observed consequences in real dictation:

- **Truncated phrases**: the cut lands mid-word roughly at random, so the last
  word of a segment loses its tail and the next segment starts with a
  fragment.
- **Duplicated words**: the split word is often "repaired" independently on
  both sides, and `appendSegment` concatenates both repairs.
- **Hallucinations**: continuous capture sends leading/trailing silence, and
  every segment starts/ends on a waveform discontinuity. The gateway
  diagnostic already produced the classic silence hallucination
  (`"Thank you."` on a near-silent buffer).
- **Wasted context**: only part of each 2.5 s window is speech, and no segment
  boundary respects sentence structure.

**Goal**: cut segments **on silence**, pad both edges, never send silence, and
let segments grow to a natural 2–4 s of speech (hard cap 7 s) so batch models
have enough context to reconstruct full phrases without truncation or
duplication.

## Scope Boundary

Only touch:

```text
src/stt/segmentation.ts               # NEW: energy VAD + segmenter
src/stt/segmentation.spec.ts          # NEW
src/stt/transcript-merge.ts           # NEW: overlap-aware junction merge
src/stt/transcript-merge.spec.ts      # NEW
src/stt/adapters/mistral.ts           # use the segmenter, drop byte-count flush
src/stt/adapters/mistral.spec.ts      # updated + new integration specs
```

Do **not** touch:

- Any Rust crate (`crates/**`) — the worker must remain the only place that
  decides segment boundaries.
- `src/stt/registry.ts`, `src/stt/discovery.ts`, `src/stt/worker.ts`,
  `src/stt/protocol.ts` (T0011 owns those).
- `src/stt/adapters/vercel-ai-sdk.ts` — keep `appendSegment` as is for
  non-overlapping segments; the new merge lives in `transcript-merge.ts`.
- `src/stt/stream-controller.ts` (streaming path).
- `package.json` / `bun.lock`: **no new dependency**, no VAD model file.

## Contract

### 1. Segmenter (`src/stt/segmentation.ts`, new, pure and deterministic)

```ts
export type SegmentReason = "silence" | "max" | "pause" | "stop";

export interface AudioSegment {
  /** Little-endian PCM16 mono bytes, WAV-ready (no header). */
  readonly pcm16: Uint8Array;
  /** Monotonic segment index within the session (0-based). */
  readonly index: number;
  readonly reason: SegmentReason;
  /** True when this segment repeats audio from the previous one (forced cut). */
  readonly startsWithOverlap: boolean;
  /** Speech duration, excluding pre-roll and tail padding. */
  readonly speechMs: number;
  /** Total duration, including padding. */
  readonly durationMs: number;
  /** Mean energy over speech frames, in dBFS (negative). */
  readonly rmsDb: number;
}

export interface SegmenterOptions {
  readonly sampleRate: number;
  /** Analysis window; default 20 ms. */
  readonly frameMs?: number;
  /** Audio kept before speech onset; default 300 ms. */
  readonly preRollMs?: number;
  /** Silence appended after the last speech frame; default 200 ms. */
  readonly tailPadMs?: number;
  /** Voiced audio below this is merged forward, not transcribed; default 1200 ms. */
  readonly minSpeechMs?: number;
  /** Nominal length used only for diagnostics; default 3000 ms. */
  readonly targetMs?: number;
  /** Hard cap before a forced cut; default 7000 ms. */
  readonly maxSegmentMs?: number;
  /** Silence run that closes a segment; default 500 ms. */
  readonly hangoverMs?: number;
  /** Audio repeated at the start of the segment after a forced cut; default 250 ms. */
  readonly overlapMs?: number;
  /** EMA factor for the noise floor; default 0.05. */
  readonly noiseFloorAlpha?: number;
  /** Speech onset threshold, as a multiple of the noise floor; default 2.5. */
  readonly speechTriggerRatio?: number;
  /** Silence threshold, as a multiple of the noise floor; default 1.5. */
  readonly silenceRatio?: number;
}

export class Segmenter {
  constructor(options: SegmenterOptions);
  /** Appends PCM16 LE bytes; returns every segment completed by this push. */
  push(pcm16: Uint8Array): AudioSegment[];
  /** Pads and closes the open segment; `pause` and `stop` are the only reasons. */
  flush(reason: "pause" | "stop"): AudioSegment[];
  /** Drops all buffered audio and resets indices (abort/teardown). */
  reset(): void;
}
```

Default constants (exported for tests):

| Constant | Value |
|---|---|
| `DEFAULT_FRAME_MS` | `20` |
| `DEFAULT_PRE_ROLL_MS` | `300` |
| `DEFAULT_TAIL_PAD_MS` | `200` |
| `DEFAULT_MIN_SPEECH_MS` | `1200` |
| `DEFAULT_TARGET_MS` | `3000` |
| `DEFAULT_MAX_SEGMENT_MS` | `7000` |
| `DEFAULT_HANGOVER_MS` | `500` |
| `DEFAULT_OVERLAP_MS` | `250` |

Rules (all sample-count driven — **no wall clock, no randomness**):

1. **Framing**: accumulate bytes into frames of
   `frameSamples = round(sampleRate × frameMs / 1000)` samples. Every buffer
   trim and cut is sample-aligned (even byte offsets). A partial frame is only
   processed by `flush()`.
2. **Energy**: `rms = sqrt(mean(sample²)) / 32768` over the frame; `rmsDb =
   20 × log10(rms)`, clamped to `-120` for digital silence.
3. **Noise floor**: starts at `0.001` (−60 dBFS), clamped to `[1e-5, 0.05]`,
   and updates **only on frames classified as silence**:
   `floor = floor × (1 − α) + rms × α`.
4. **Speech onset**: `rms > max(floor × speechTriggerRatio, 0.001)`. Onset
   starts a segment that begins with the **pre-roll ring buffer** (last
   `preRollMs` of audio, including silence) followed by the triggering frame.
5. **Silence run / close**: while in speech, a frame with
   `rms < floor × silenceRatio` starts/extends a silence run. When the run
   reaches `hangoverMs`, the segment is closed with the cut point at
   `run_start + tailPadMs`, so the segment ends with exactly `tailPadMs` of
   silence (zero-filled if the run is shorter, which cannot happen with the
   defaults but must be handled). Audio after the cut point is discarded.
6. **Minimum length / merge forward**: if `speechMs < minSpeechMs` at close,
   the audio is **not** emitted and is not discarded — it is retained (bounded
   to `preRollMs + minSpeechMs`) and prepended to the next segment. A closed
   segment containing **no** frame above the onset threshold is dropped
   entirely (no `transcribe` call, no `committed` event).
7. **Forced cut**: if the open segment reaches `maxSegmentMs`, cut at the
   **quietest 20 ms frame** within the last `600 ms` (ties → earliest frame),
   never inside the final `100 ms`. The remainder of the buffer starts the next
   segment, and the last `overlapMs` before the cut is **repeated** at its
   start (`startsWithOverlap = true`). The repeated part is not counted in
   `speechMs`.
8. **`flush("pause" | "stop")`**: closes the open segment with `tailPadMs` of
   zero-fill, then resets to idle. Emits nothing when the buffer holds no frame
   above the onset threshold. `stop` also emits short (< `minSpeechMs`) voiced
   audio rather than dropping it — a user who stops mid-sentence must still get
   their words.
9. **`reset()`**: clears buffers, index counter, and the noise floor (back to
   `0.001`). Used on `abort()`.
10. `index` increments only for emitted segments; `reason` is `silence`,
    `max`, `pause`, or `stop`.
11. Memory is bounded: pre-roll ≤ `preRollMs`, open segment ≤
    `maxSegmentMs + 1 frame`, and no unlimited chunk list.

### 2. Junction merge (`src/stt/transcript-merge.ts`, new)

```ts
export interface MergeOptions {
  /** Longest repeated word run removed at a junction; default 4. */
  readonly maxOverlapWords?: number;
  /** Allow a 1-word overlap to be removed; default false. */
  readonly allowSingleWordOverlap?: boolean;
}

export function mergeTranscript(
  previous: string,
  next: string,
  options?: MergeOptions,
): string;
```

- Compares word sequences after normalization (lowercase, strip leading and
  trailing punctuation; whitespace splits words) but **emits original text**.
- Finds the longest `k ≤ maxOverlapWords` where the last `k` normalized words
  of `previous` equal the first `k` normalized words of `next`.
- If `k ≥ 2` (or `k == 1` with `allowSingleWordOverlap`), remove `k` words from
  the head of `next` and join with a single space.
- `k == 0` → plain join (trim + single space, empty side handled).
- `next` fully contained in the overlap → return `previous`.
- Legitimate repetitions (`"very very"` with no matching tail/head run) are
  preserved: only a true suffix/prefix run is removed.
- Never drops punctuation-only or numeric-only tokens; numbers compare as
  written.
- Called with `allowSingleWordOverlap: segment.startsWithOverlap`.

### 3. Adapter integration (`src/stt/adapters/mistral.ts`)

- `MistralSession` keeps a `Segmenter` instead of `chunks`/`bufferLength`/
  `segmentBytes`.
- `pushAudio` decodes base64 → `Uint8Array` (existing helper), returns early on
  empty input, and queues a transcription for each returned `AudioSegment`.
- `pause()` → `segmenter.flush("pause")`; `stop()` → `flush("stop")`, then
  drain the serialized queue before emitting `stopped`, as today.
- `abort()` → `segmenter.reset()` and abort in-flight requests.
- WAV framing: the segment bytes are passed through the existing
  `pcm16ToWav(pcm, sampleRate)` — no change to the header.
- Each `committed` event keeps the exact current shape
  (`{ type: "committed", sessionId, text }`); the protocol is unchanged.
- Text composition: `mergeTranscript(accumulated, text, { allowSingleWordOverlap:
  segment.startsWithOverlap })`, replacing the direct `appendSegment` call for
  segmented sessions. Empty/whitespace results are skipped as today.
- Diagnostics to `stderr` only, one line per dispatched segment:
  `[stt-worker] segment index=3 reason=silence durationMs=2840 speechMs=2410
  rmsDb=-21.4 overlap=false`. Never log transcript text, audio, or keys.
- `MistralAdapterOptions.segmentSeconds` is **removed** and replaced by
  `segmentation?: Partial<Omit<SegmenterOptions, "sampleRate">>`; `sampleRate`
  always comes from `SttSessionConfig`. `new MistralAdapter()` must keep
  working with defaults.
- All existing guarantees stay: serialized requests, `abortSignal` honoured,
  single `error` per session, `stopped` emitted exactly once.

## TDD Tasks

- [x] **T12.1 — `segmentation.spec.ts` (energy + onset)**: frame RMS in dBFS;
  noise floor adapts to a noisy floor and ignores speech; hysteresis uses
  `speechTriggerRatio`/`silenceRatio`; digital silence never triggers; onset
  includes exactly `preRollMs` of leading audio.
- [x] **T12.2 — `segmentation.spec.ts` (close on silence)**: a 3 s tone
  followed by 1 s of silence yields one `silence` segment whose last samples
  are silence, `speechMs ≈ tone length`, and the cut lands inside the silence
  run; three sentences separated by 600 ms yield exactly three segments; total
  audio outside segments is silence only.
- [x] **T12.3 — `segmentation.spec.ts` (edge policies)**: a 200 ms blip before
  speech merges forward into one call; a blip with no following speech is
  dropped; `pause` zero-pads and closes; `stop` emits short voiced audio; a
  continuous 20 s tone is split at the quietest frame near 7 s with
  `startsWithOverlap` and `overlapMs` repeated; `reset()` restores initial
  state; chunk-boundary fuzz (random chunk sizes, same audio) produces
  identical segments.
- [x] **T12.4 — `transcript-merge.spec.ts`**: 2-word and 4-word overlaps
  removed; 5-word run capped at 4; single-word overlap only with
  `allowSingleWordOverlap`; `"very very"` preserved; punctuation/case
  differences still match but original casing is emitted; empty sides; fully
  contained `next`.
- [x] **T12.5 — `mistral.spec.ts` (integration, fake `transcribe`)**: segments
  are WAV-framed and sent in order; no call for silence-only audio; one call
  per sentence; `committed` texts concatenate without duplicated junction
  words; `stop` drains the queue before `stopped`; `abort` cancels in-flight
  work; the recorded segment durations stay within `[minSpeechMs,
  maxSegmentMs]`; diagnostics go to `stderr` only.
- [x] **T12.6 — Implementation**: `segmentation.ts`, `transcript-merge.ts`,
  adapter rewiring, option migration, diagnostics.
- [x] **T12.7 — Gates**: `bun test`, `bun check src/stt`,
  `cargo fmt -- --check`, `cargo clippy --all-targets -- -D warnings`,
  `cargo test`.

## Acceptance Criteria

- No `transcribe()` call is made for a buffer containing no frame above the
  onset threshold (silence and noise cost nothing).
- Every segment produced by normal speech starts and ends inside silence, with
  `preRollMs` and `tailPadMs` padding, and lasts between `minSpeechMs` and
  `maxSegmentMs` of speech.
- A sentence followed by a ≥ 500 ms pause becomes **one** segment (no
  mid-word cut), and the transcribed text contains no word duplicated at a
  junction.
- A continuous monologue is cut at the quietest available frame near the hard
  cap, with overlap flagged, and the overlap words appear once in the final
  transcript.
- `pause` flushes what was spoken; `stop` never loses voiced audio; `abort`
  sends nothing further.
- Behaviour is identical for any input chunking (same audio, different chunk
  sizes → same segments), with no clock, randomness, network, or new
  dependency.
- All gates from `AGENTS.md` pass with zero warnings, and `bun test` remains
  fully hermetic (fake `transcribe`, synthetic PCM).

## Follow-ups (explicitly out of scope)

- **Context seeding**: pass the previous segment's tail through
  `transcribe({ providerOptions })` (`openai.prompt`, `mistral.contextBias`) to
  improve continuity across cuts — separate ticket.
- **Pipelining**: allow two in-flight `transcribe` calls to hide latency on
  long monologues — only after segment boundaries are correct.
- **Waveform-consistent VAD**: surface the detected speech/silence regions in
  the Rust UI waveform (requires a protocol change).

## Implementation Notes

Implemented by the CTO directly (no coding sub-agent: both the free and the Go
coder quotas were exhausted mid-session).

### What landed

- `src/stt/segmentation.ts` — pure `Segmenter`: 20 ms RMS frames, adaptive
  noise floor with hysteresis, 300 ms pre-roll, 500 ms hangover, 200 ms tail
  pad, merge-forward below 1.2 s of speech, 7 s cap cut at the quietest frame
  in the last 600 ms with 250 ms of repeated audio, `pause`/`stop` flush,
  `reset`.
- `src/stt/transcript-merge.ts` — `mergeTranscript`: case/punctuation-insensitive
  suffix/prefix word dedupe, capped at 4 words, single-word overlaps only when
  `allowSingleWordOverlap` is set.
- `src/stt/adapters/mistral.ts` — byte-count buffering replaced by the
  segmenter; WAV framing, serialized queue, abort/error semantics and event
  shapes unchanged.
- Specs: `segmentation.spec.ts` (13), `transcript-merge.spec.ts` (10), and a
  rewritten `adapters/mistral.spec.ts` (13).

### Deviations from the contract (justified)

1. **100 ms calibration bootstrap.** The literal rule ("floor starts at
   `0.001`, updated only on frames classified as silence") makes any steady
   ambient noise above −60 dBFS look like speech on the first frame. Because
   the floor is frozen while in speech, such a session would never close and
   would loop on 7 s forced cuts, transcribing noise forever. The segmenter
   therefore calibrates the floor from the minimum RMS of the first 5 frames
   (100 ms), lets a frame louder than −26 dBFS (`STRONG_SPEECH_RMS = 0.05`)
   start speech immediately during calibration, and only then switches to the
   EMA/hysteresis rules from the contract. Covered by
   "does not mistake steady ambient noise for speech" and
   "detects loud speech at the very first frame".
2. **`committed` carries the non-duplicated delta.** The Rust
   `TranscriptBuffer` appends `committed.text`, so emitting the raw segment
   text would still duplicate junction words in the UI even with a correct
   `fullText`. The adapter emits `mergeTranscript(accumulated, text)` minus the
   accumulated prefix. `stopped.fullText` remains the authoritative merged
   text; the wire shape is unchanged.
3. **Overlap rounds to whole frames.** 250 ms is 12.5 frames of 20 ms, so the
   effective repeated audio is 13 frames (260 ms). The option stays in
   milliseconds.

### Measured behaviour (synthetic PCM, 16 kHz)

- Silence-only input: zero segments, zero `transcribe` calls.
- Three 1.5 s sentences separated by 600 ms pauses: exactly 3 segments
  (1.5 s of speech each, 300 ms pre-roll, 200 ms tail pad).
- 200 ms blip followed by a sentence: merged into one 1.7 s-speech segment.
- 12 s continuous tone: first segment `max` at 6.4 s, remainder emitted on
  `stop` with `startsWithOverlap = true` and 260 ms of repeated audio.
- 7.5 s tone with a 20 ms dip at 6.6 s: forced cut exactly at 6.6 s.
- Steady ambient noise (≈ −34 dBFS): no segment; a following tone is cut
  normally.
- Chunk sizes 1, 7, 641, 1024 and 5000 bytes produce byte-identical segments.

### Gates

```
bun test            → 113 pass, 0 fail (533 expect calls, 9 files)
bun check src/stt   → ✓ No type errors in 17 files
bun run design:validate → ✓ 66 tokens, 0 errors
cargo fmt -- --check    → clean
cargo clippy --all-targets -- -D warnings → clean
cargo test              → 260 passed, 0 failed
```
