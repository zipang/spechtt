# T0015 — Transcribe a local audio or video file

- **Track**: TypeScript-Bun worker + Rust core/UI (cross-cutting)
- **Wave**: 2 (integration — builds on the batch segmentation pipeline)
- **Depends on**: T0002 (duplex bridge), T0003 (worker), T0012 (segmentation), T0014 (single gateway provider)
- **Related**: T0016 (dependency bootstrap) — the `ffmpeg_missing` hint
  points users at `./scripts/setup.sh` once that ticket lands
- **Owner**: CTO (sub-agent quotas exhausted)
- **Started**: 2026-10-29
- **Blocks**: — (a future "batch queue" ticket may build on it)
- **References**: `AGENTS.md` §2–§5, gateway speech-to-text docs,
  `ai` 7 `transcribe`/`experimental_streamTranscribe`, `@ai-sdk/gateway`
  `gateway-transcription-model.ts`

## Context & Goal

SpeechTT can only transcribe the live microphone: the Rust host captures PCM
from `cpal` and pushes it to the worker as `audio` commands. Recordings that
already exist on disk — voice notes, call recordings, screen captures, MP4
videos — cannot be transcribed at all.

**The file never crosses the bridge.** The Rust host and the Bun worker run on
the same machine, so the host sends only an absolute **path**; the worker opens
and decodes it locally. The wire gains one command (`transcribeFile`) and one
progress event, not an audio byte-stream.

Done means: the user clicks **Transcribe a file…**, picks any audio/video file,
and watches the transcript fill in exactly like a live recording — same
provider/model settings, same copy/paste/save actions, same error surface.

### Format coverage

The worker decodes with **ffmpeg**, so the accepted set is "anything ffmpeg
can demux": WAV, MP3, M4A/AAC, FLAC, Ogg/Opus, WMA, AIFF, MP4, MKV, WebM, MOV,
AVI, … A video file contributes its first audio stream (`-map 0:a:0`); the
video track is discarded.

Decoded audio is **canonicalised to 16 kHz mono PCM16** before transcription,
then WAV-framed — the one container every gateway transcription model accepts
(Azure MAI takes WAV/MP3/FLAC/Ogg-Opus but **not** WebM/MP4; WAV is the
intersection). We therefore never forward the original container bytes, and do
not depend on per-provider container support.

### Why not stream the file through `experimental_streamTranscribe`?

Investigated in the installed `ai` 7 / `@ai-sdk/gateway` 4 sources; three facts
close that path for file input:

1. **No server-side file reference exists.** `transcribe()` resolves `audio`
   as `Uint8Array | string | URL`; a `URL` is downloaded **client-side** by
   the SDK and then base64-encoded (`ai/dist/index.js` `transcribe`). The
   gateway itself posts `{ audio: <base64>, mediaType }`
   (`gateway-transcription-model.ts` `doGenerate`). The download path is
   ordinary `fetch` with a scheme allowlist of `http:`, `https:` and `data:`
   (`validateDownloadUrl` in `@ai-sdk/provider-utils`), and the whole body is
   buffered in memory (2 GiB cap) to sniff the media type. So:
   - a local path cannot be expressed as a URL — **`file://` is rejected**
     even though Bun's `fetch` would accept it;
   - an `http(s)` URL still means "download everything, then base64", which
     is useless for local files and no better than reading bytes ourselves.
2. **`experimental_streamTranscribe` only accepts raw audio, not containers.**
   Its options are `audio: ReadableStream<Uint8Array | string>` plus
   `inputAudioFormat: { type: "audio/pcm", rate }`, and the gateway opens a
   WebSocket (`toGatewayTranscriptionUrl`). Decoding with ffmpeg is required
   either way; streaming would not remove the dependency.
3. **Streaming is restricted to the 7 `websocket-transcription` models.**
   Batch-only models (`whisper-1`, `scribe-v2`, `voxtral-*`, `mai-transcribe-2`)
   implement no `doStream`, so file transcription would fail or be
   model-dependent. Feeding a recording faster than real time also loses the
   provider's whole-file context and is not a documented pacing mode.

Batch `transcribe()` over VAD segments is therefore the only option that works
for **every** listed model and for arbitrarily long files. Reusing the live
`AudioStreamBridge` for files is explicitly out of scope; if real-time file
playback is ever wanted, it belongs in a follow-up ticket that states the
model restriction.

### Platform matrix for the ffmpeg dependency

ffmpeg is available on Linux, macOS and Windows; the ticket does not add any
platform-specific code. What differs is what the user has to install:

| Platform | Preinstalled? | Install hint shown by `ffmpeg_missing` |
|---|---|---|
| Linux (project target) | Usually, not guaranteed | `sudo apt install ffmpeg` / `sudo dnf install ffmpeg` |
| macOS | No | `brew install ffmpeg` |
| Windows | No | `winget install ffmpeg` |

`README.md`/`AGENTS.md` define SpeechTT as a **Linux** widget, so a system
dependency is acceptable for this ticket; the hint text above keeps macOS and
Windows builds honest. Bundling a per-platform binary is deliberately *not*
done here (it adds ~80 MB per target and a release-packaging story that does
not exist yet) and is the natural fallback if non-Linux builds ship later.

### Would hosting the file on a public URL help? (measured, not assumed)

Some providers accept only a public URL, so this was tested against the
gateway rather than reasoned about:

```text
model.doGenerate({ audio: "https://…/sample.mp3", mediaType: "audio/mpeg" })
→ 400 invalid_request_error: "audio must be a base64 string"
```

The gateway transcription endpoint **rejects URLs**; only base64 bytes are
accepted. Combined with the client-side download above, uploading a recording
to a public URL and passing that URL to `transcribe()` would transfer the
same file three times (upload, download, base64 POST) with no server-side
fetch to show for it. On top of that:

- `vercel blob put <file> --access public` is not an ephemeral transfer
  service: it needs a linked project, a provisioned Blob store and OIDC or
  `BLOB_READ_WRITE_TOKEN`, it **publishes the user's recording** at a guessable
  URL until deleted, and it bills storage. Shipping that as the default path
  for a local desktop app is a privacy and operational regression.
- A future direct-provider track (outside the single-gateway scope of T0014)
  may legitimately need URL input, because those APIs differ. That belongs in
  its own ticket; it must not shape this one.

**Acceleration is already inherent here**: batch calls are not paced to
real time, so segments are transcribed back-to-back and a 2-hour recording
finishes in minutes, bounded by model throughput. To exploit that, file
sessions use **file-tuned segmentation thresholds** (bigger segments, fewer
calls) — see §3. Bounded parallel segment requests are a deliberate non-goal
(ordering, junction dedupe and rate-limit safety) and can be measured in a
follow-up ticket.

## Scope Boundary

May create or modify:

- `src/stt/protocol.ts`, `src/stt/protocol.spec.ts` — **append-only** for the
  new command/event and error codes.
- `src/stt/file-input.ts`, `src/stt/file-input.spec.ts` — **new**.
- `src/stt/adapters/file.ts`, `src/stt/adapters/file.spec.ts` — **new**.
- `src/stt/worker.ts`, `src/stt/worker.spec.ts`.
- `crates/speechtt-core/src/protocol.rs`.
- `crates/speechtt-ui/src/controller.rs`.
- `crates/speechtt-ui/src/app.rs`.
- `crates/speechtt-ui/src/views/` (Prompt screen action, Recording header
  status only).
- `crates/speechtt-ui/tests/{common/mod.rs,dialogs.rs,end_to_end.rs}`,
  `crates/speechtt-core/tests/bridge_smoke.rs`.

Do **not** touch: `src/stt/segmentation.ts`, `src/stt/transcript-merge.ts`,
`src/stt/discovery.ts`, `src/stt/registry.ts`, `settings.rs`, `audio.rs`,
`session.rs`, `DESIGN.md`, `design-tokens.css`, `tokens.rs`. No new npm or
Cargo dependency: ffmpeg is a **system** tool, and the file picker uses GPUI's
built-in `App::prompt_for_paths` (XDG desktop portal on Linux) — not `rfd`.

## Contract

### 1. Wire protocol (TS `protocol.ts` ↔ Rust `protocol.rs`)

New inbound command, camelCase on the wire:

```ts
export interface TranscribeFileCommand {
  type: "transcribeFile";
  sessionId: string;
  provider: string;
  model: string;
  /** Absolute path to an audio or video file on the worker's host. */
  path: string;
  apiKey?: string;
}
```

```rust
SttCommand::TranscribeFile {
    session_id: String,
    provider: String,
    model: String,
    path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    api_key: Option<SecretString>,
}
```

- No `sampleRate` field: file decoding is always 16 kHz mono.
- Parsing rejects a missing/empty `path`, a path containing `NUL`, and a
  non-string `apiKey`, each with a specific `invalid_protocol` message.

New outbound event:

```ts
export interface ProgressEvent {
  type: "progress";
  sessionId: string;
  /** Seconds of decoded audio consumed so far (monotonic, ≥ 0). */
  processedSeconds: number;
  /** ffprobe duration, or null when it cannot be determined. */
  totalSeconds: number | null;
}
```

```rust
SttEvent::Progress {
    session_id: String,
    processed_seconds: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    total_seconds: Option<f64>,
}
```

`SttEvent::session_id()` includes `Progress`.

`SttErrorCode` gains four values, documented in the same union:
`"file_not_found" | "unsupported_file" | "decode_failed" | "ffmpeg_missing"`.

### 2. Decoding (`src/stt/file-input.ts`)

```ts
export interface FfmpegTools { readonly ffmpeg: string; readonly ffprobe: string; }

/** Resolution order: env override, then PATH. Cached; `undefined` when
 *  either binary is absent. Env: SPEECHTT_FFMPEG, SPEECHTT_FFPROBE. */
export function findFfmpeg(env?: Record<string, string | undefined>): FfmpegTools | undefined;

/** `format.duration` via ffprobe; null on any failure. Never throws. */
export function probeDuration(path: string, ffprobe: string, signal?: AbortSignal): Promise<number | null>;

/** Streams 16 kHz mono PCM16 LE from the file's first audio stream.
 *  Chunks are even-length; a partial sample is carried to the next chunk. */
export function decodeFileToPcm16(
  path: string,
  options: { ffmpeg: string; signal?: AbortSignal },
): AsyncIterable<Uint8Array>;
```

ffmpeg invocation contract (exact flags asserted in tests):

```text
ffmpeg -nostdin -v error -i <path> -map 0:a:0 -vn -ac 1 -ar 16000 -f s16le -acodec pcm_s16le pipe:1
```

- `stderr` is captured (drained concurrently so the pipe never fills) and
  truncated to the **last 500 characters** for diagnostics; it never contains
  API keys. No transcript text is ever written to `stdout` by this module.
- Non-zero exit ⇒ throw `FileDecodeError` carrying the stderr tail. If exit
  happens **after** audio was yielded, the session still finalizes the text it
  already produced (see §3).
- A path that does not exist, is a directory, or is unreadable maps to
  `file_not_found`; the `ffmpeg_missing` error message names the install command
  for the current platform (see the platform matrix above) and, when the T0016
  bootstrap exists, points at `./scripts/setup.sh`; an ffmpeg failure
  before any output containing
  "does not contain any stream" / "Invalid data found" maps to
  `unsupported_file`; every other non-zero exit maps to `decode_failed`.
- Abort kills the child (`SIGTERM`, `SIGKILL` after 1 s) and ends the iterator
  without throwing.

### 3. File session (`src/stt/adapters/file.ts`)

A `SttSession` driven by a decoded file rather than pushed chunks. It reuses
`Segmenter`, `pcm16ToWav`, `mergeTranscript` and `resolveModelCapability`'s
mode split unchanged — no duplication of segmentation or junction dedupe.

```ts
export interface FileSessionDeps {
  decode(path: string, options: { signal: AbortSignal }): AsyncIterable<Uint8Array>;
  probeDuration(path: string, signal: AbortSignal): Promise<number | null>;
  transcribe: TranscribeFn;
  resolveModel(config: SttSessionConfig): TranscriptionModel;
  segmentation?: Partial<Omit<SegmenterOptions, "sampleRate">>;
}

export function createFileSession(
  config: SttSessionConfig & { path: string },
  emit: EmitFn,
  deps: FileSessionDeps,
): SttSession;
```

Behaviour:

0. Segmentation is **file-tuned**, not live-tuned: the same `Segmenter` runs
   with `targetMs = 30_000` and `maxSegmentMs = 60_000` (instead of the live
   3 s / 7 s cadence), keeping the silence alignment, hangover, min-speech
   merge and 250 ms overlap unchanged. Rationale: with no live cadence to
   feed, larger segments cost fewer requests, transcribe faster and give the
   model more context; the junction dedupe keeps the seams clean. This is
   injectable through `FileSessionDeps.segmentation` and asserted in tests.
1. Probe duration (best effort), then emit `started`.
2. Decode chunk → `Segmenter.push` → each completed segment is transcribed
   **sequentially**; the decoder is not pulled again until the previous segment
   finished, so memory stays bounded by one segment regardless of file length.
3. Each transcribed segment emits `committed` with the **non-duplicated delta**
   (T0012 semantics: `mergeTranscript` absorbs the word overlap with the
   previous tail, and the event carries only the new text). `stopped.fullText`
   stays the authoritative merged text.
4. After each segment, emit `progress` with the decoded position and the probed
   total. `processedSeconds` is monotonic and never exceeds the decoded audio
   length.
5. **Never** emits `partial` or `delta`: there is no live hypothesis to show.
6. End of decode: `flush("stop")`, drain the queue, emit `stopped`.
7. `stop()` mid-decode aborts the decoder, drains what was queued, and emits
   `stopped` with the text produced so far.
8. `abort()` is silent (no events) and kills the child.
9. `pushAudio`/`pause`/`resume` are no-ops (documented): a file session has no
   live input.

### 4. Worker (`src/stt/worker.ts`)

- `transcribeFile` resolves provider/key exactly like `start`:
  `unsupported_provider` for an unknown provider, `missing_api_key` naming the
  provider env vars otherwise. It replaces any active session (awaiting its
  `stop`) first.
- When `findFfmpeg()` is `undefined`, emit `error { code: "ffmpeg_missing" }`
  and start **no** session (no `started`).
- `stop` awaits the file session's `stop()`.
- Accepted `affects routing`: streaming models (`websocket-transcription`) are
  still transcribed **batch-wise** for files (the mode badge states this); the
  file session always uses the segmented path.

### 5. Rust host (`speechtt-ui`)

- **Validation** (`SessionController::start_file_session(&Path) -> Result<(), AppError>`):
  empty/NUL path, missing metadata, or a directory ⇒ `AppError::FileUnreadable`;
  a non-existent path ⇒ `AppError::FileNotFound`.
- On success: `audio_source` becomes the file's display name,
  `session_name` defaults to the file stem when empty, a take starts
  (`SessionState::Recording`), `SttCommand::TranscribeFile` is sent, and
  `view_state()` moves to `Recording`. Nothing else changes: the take still
  goes through the existing stop → result → copy/paste/save flow.
- `AppError` gains `FileNotFound`, `FileUnreadable`, `FfmpegMissing`,
  `FileDialogFailed` — each with a one-line user hint and a `Dismiss` (or
  `OpenSettings` for a missing key) action.
- `on_progress(processed, total)` stores the reading; `elapsed_seconds()`
  reflects the decoded position while a file take runs.
- **File picker**: `SpeechTtApp::pick_transcription_file(cx)` calls
  `cx.prompt_for_paths(PathPromptOptions { files: true, directories: false,
  multiple: false, prompt: Some("Transcribe") })`. `Ok(Some([p]))` ⇒
  `start_file_session(p)`; `Ok(None)` ⇒ no-op; `Err` ⇒ `FileDialogFailed`.
  A headless-testable seam `SpeechTtApp::start_file_from_path(path, cx)` is the
  entry point the picker calls.
- Prompt screen gains one secondary action, **Transcribe a file…**
  (`ActionButton::secondary`, id `btn-prompt-transcribe-file`), placed beside
  the existing record action. Its label is mono/uppercase like the other
  actions; no new tokens or colors.

## TDD Tasks

- [x] **T15.1 — Protocol specs (TS)**: failing tests in `protocol.spec.ts` for
  `transcribeFile` parse/serialize happy path, every rejection rule, and
  `progress` serialization incl. `totalSeconds: null`. Red first.
- [x] **T15.2 — Protocol specs (Rust)**: failing round-trip tests in
  `protocol.rs` for `TranscribeFile` (camelCase, key omitted when absent) and
  `Progress` (`total_seconds` omitted when `None`), plus `session_id()`.
- [x] **T15.3 — Decoder specs**: `file-input.spec.ts` against an injected
  spawn/exec seam covering flag exactness, odd-byte chunk framing, stderr
  truncation, every error-code mapping, abort killing the child, and
  `findFfmpeg` env/PATH precedence.
- [x] **T15.4 — Session specs**: `file.spec.ts` with a fake decode iterator and
  fake `transcribe` asserting: `started` then `committed`s in order,
  junction dedupe (no repeated words), monotonic `progress`, no
  `partial`/`delta`, `stopped.fullText` equals the merged text, mid-file
  `stop()` keeps partial text, `abort()` emits nothing, sequential
  (never concurrent) transcription calls.
- [x] **T15.5 — Worker specs**: `worker.spec.ts` for routing, missing key,
  unknown provider, `ffmpeg_missing` (no `started`), and that `stop` cancels.
- [x] **T15.6 — Implement** `file-input.ts`, `adapters/file.ts`, wire
  `transcribeFile` into `worker.ts`, and add the Rust command/event variants.
- [x] **T15.7 — Controller specs**: `start_file_session` validation matrix,
  naming defaults, `Recording` transition, progress mapping into
  `elapsed_seconds`, error mapping, and that no `Audio` command is ever sent
  for a file take (assert the recorded sink commands).
- [x] **T15.8 — UI wiring**: Prompt action, `prompt_for_paths` handler,
  `start_file_from_path` seam, Recording status line showing
  `processed / total s` when the total is known. Add a headless test proving
  `start_file_from_path` starts a take and that a `progress` event updates the
  elapsed read-out.
- [x] **T15.9 — Live smoke**: extend `bridge_smoke.rs` to send a real
  `transcribeFile` for `tests/test-record.wav` **skipped** when `ffmpeg` or
  `AI_GATEWAY_API_KEY` is absent (never fail CI for missing tools).
- [x] **T15.10 — Gates**: `cargo fmt -- --check`,
  `cargo clippy --all-targets -- -D warnings`, `cargo test`, `bun test`,
  `bun check src/stt`. Manual QA: transcribe a `.wav`, an `.mp3`, and an
  `.mp4` (video, audio track only) and confirm identical quality to the mic
  path.

## Acceptance Criteria

1. `{ "type": "transcribeFile", ... "path": "/abs/file.mp4" }` on stdin
   produces `started`, ordered `committed` deltas, monotonic `progress`
   events, and one `stopped` with the full merged text — **without a single
   `audio` command ever being written** (asserted in tests).
2. A WAV, an MP3, an M4A, a FLAC and an MP4 (video) all transcribe; the video
   track is ignored; no format-specific code paths other than ffmpeg flags.
3. Missing `ffmpeg` yields `error { code: "ffmpeg_missing" }` before any
   `started`, and the UI shows an actionable hint.
4. A missing/unreadable/directory path is refused by the Rust host before any
   command is sent, with a user-facing error.
5. Picking a file from the Ready screen's **Transcribe a file…** action starts
   a take indistinguishable from a microphone take downstream: stop, copy,
   paste and save all work, and the transcript appears in the same caption
   well. (T0017 replaces this entry point with the mode chooser and removes
   the session name; the worker/controller file path is unchanged.)
6. Cancelling a file take mid-way keeps the text produced so far and leaves
   the app Ready.
7. Files longer than an hour transcribe with memory bounded by one segment
   (~60 s of 16 kHz PCM), not by file length, and finish markedly faster than
   real time (batch calls are not paced).
8. All gates green with zero warnings; no `#[ignore]`d or skipped tests other
   than the explicitly gated live smoke test.

## Implementation Notes

### What shipped

- **Worker** — new `src/stt/file-input.ts` (ffmpeg/ffprobe resolution with
  `SPEECHTT_FFMPEG`/`SPEECHTT_FFPROBE` overrides, duration probe, streaming
  PCM16 decode, stderr tail truncation, abort kill) and
  `src/stt/adapters/file.ts` (sequential decode → segment → batch transcribe,
  file-tuned segmentation `targetMs 30_000` / `maxSegmentMs 60_000`, junction
  dedupe via `mergeTranscript`, monotonic `progress`, silent abort).
  `worker.ts` routes `transcribeFile` through `FileDeps`/`defaultFileDeps`,
  replaces any live session first, and refuses with `ffmpeg_missing` before
  `started`; an `activeKind` guard keeps microphone `audio` commands out of a
  file session. `defaultSegmentedModel` is exported from `adapters/mistral.ts`
  so both session kinds share the batch-mode resolution.
- **Protocol** — `transcribeFile` command and `progress` event in
  `src/stt/protocol.ts` / `crates/speechtt-core/src/protocol.rs`; four new
  error codes (`file_not_found`, `unsupported_file`, `decode_failed`,
  `ffmpeg_missing`). `SttEvent` derives `PartialEq` only (the event carries
  `f64`); `transcript.rs::apply` ignores `Progress`.
- **Controller** — `start_file_session(&Path)` validates (empty/NUL,
  metadata, directory), maps `AppError::{FileNotFound, FileUnreadable,
  FfmpegMissing, FileDialogFailed}` and `from_stt_error`, sets the audio
  source to the file name, starts the take and sends only
  `SttCommand::TranscribeFile`. `progress`/`file_take` state feeds
  `elapsed_seconds()` and the header read-out; `push_audio_chunk` is inert for
  file takes.
- **UI** — `pick_transcription_file` (`prompt_for_paths`, cancel silent,
  error surfaced) + the headless seam `start_file_from_path`; the Ready screen
  gains a secondary **TRANSCRIBE A FILE…** action; the Recording/Paused header
  shows `processed / total s` (or `processed s`) for file takes.

### Decisions recorded during the work

- `stop()` mid-file aborts the decoder, flushes the buffered segment and emits
  `stopped` with the text produced so far; `abort()` is silent. Decode errors
  emit `error` and then still flush + `stopped`, so a partial take is never
  lost.
- The file is never read by the Rust host: only the path crosses the bridge.
  ffmpeg is a **system** tool (no new Cargo/npm dependency), per the platform
  matrix in this ticket.
- File takes use file-tuned segmentation; memory stays bounded by one segment
  (≤ 60 s of 16 kHz PCM) regardless of file length.

### Manual QA (real gateway, 2026-10-30)

- `.wav`, `.mp3` and `.mp4` (audio track only) each returned the expected
  sentence ("Est-ce que quelqu'un m'entend?") in ~0.8 s for a 5 s clip.
- A 120 s file (~24 segments) transcribed well under real time; progress was
  monotonic and the final text matched the live path's quality.
- `file_not_found` was verified end to end for a missing path.

### Gates (all green, zero warnings)

- `cargo fmt -- --check` — clean.
- `cargo clippy --all-targets -- -D warnings` — clean.
- `cargo test` — 310 tests: core lib 183, `bridge_smoke` 3 (live test gated),
  `session_machine` 11, UI lib 22, `dialogs` 11, `end_to_end` 31,
  `error_ux` 9, `session_flow` 29, `theme_contract` 4, `widget` 7.
- `bun test` — 206 pass, 0 fail (12 files).
- `bun check src/stt` — no type errors (23 files).
- `bun run design:validate` — 66 tokens, 0 errors.

### Follow-ups

- **T0017** replaces the Ready-screen action with the mode chooser, removes
  the session name (the source file stem names the take) and switches Save to
  a native save dialog. No worker or protocol change is involved.
