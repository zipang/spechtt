# T0002 — Rust Duplex Audio & Real-Time Transcript Bridge (`speechtt-core` + `speechtt-ui`)

## Context & Goal

With the Rust ↔ Bun communication protocol locked as **full-duplex Newline-Delimited JSON (NDJSON) over child-process `stdin`/`stdout`** (`PRODUCT.md` §5, §7), `speechtt-core` and `speechtt-ui` need the types, state reducers, and async duplex channels to:
1. Send session lifecycle commands (`start`, `pause`, `resume`, `stop`) and 16-bit mono PCM audio frames (`audio` with `pcm16Base64`) from Rust to the STT backend.
2. Receive real-time streaming transcription events (`ready`, `started`, `partial`, `delta`, `committed`, `stopped`, `error`) from the backend as soon as tokens arrive.
3. Update `SessionController` and `SpeechTtApp` (`CaptionWell`, `Waveform`, `ElapsedTimer`, and `FinishedTake`) live on every incoming event instead of reading static `MOCK_*` constants.

---

## 1. Wire Protocol Contract (`crates/speechtt-core/src/protocol.rs`)

Each message is serialized as a single-line JSON object terminated by `\n`.

### Inbound Commands (`Rust → Bun` over `stdin`)

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
#[non_exhaustive]
pub enum SttCommand {
    #[serde(rename_all = "camelCase")]
    Start {
        session_id: String,
        provider: String,
        model: String,
        sample_rate: u32,
        #[serde(skip_serializing_if = "Option::is_none")]
        api_key: Option<SecretString>, // Redacted in Debug impl
    },
    #[serde(rename_all = "camelCase")]
    Audio {
        pcm16_base64: String,
    },
    Pause,
    Resume,
    Stop,
}
```
- **Audio encoding helper**: Provide `SttCommand::audio_from_pcm16(samples: &[i16]) -> Self` (and `audio_from_f32(samples: &[f32]) -> Self`) that converts PCM samples to little-endian `i16` bytes and base64-encodes them (`base64::engine::general_purpose::STANDARD`).
- **Secret safety**: `api_key` must never appear in `fmt::Debug` or logs (`"[REDACTED]"`).

### Outbound Events (`Bun → Rust` over `stdout`)

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
#[non_exhaustive]
pub enum SttEvent {
    Ready,
    #[serde(rename_all = "camelCase")]
    Started {
        session_id: String,
    },
    /// Uncommitted live hypothesis (replaces current live tail in CaptionWell).
    #[serde(rename_all = "camelCase")]
    Partial {
        session_id: String,
        text: String,
    },
    /// Incremental token delta appended to the current live tail.
    #[serde(rename_all = "camelCase")]
    Delta {
        session_id: String,
        delta: String,
    },
    /// Finalized segment moved from live tail into committed transcript.
    #[serde(rename_all = "camelCase")]
    Committed {
        session_id: String,
        text: String,
    },
    /// Final flush emitted after `stop` completes.
    #[serde(rename_all = "camelCase")]
    Stopped {
        session_id: String,
        full_text: String,
    },
    #[serde(rename_all = "camelCase")]
    Error {
        #[serde(default)]
        session_id: Option<String>,
        code: String,
        message: String,
    },
}
```

---

## 2. Streaming Transcript Reducer (`crates/speechtt-core/src/transcript.rs`)

`CaptionWell` renders two distinct parts:
- **`committed`** (muted ink): finalized clauses/sentences.
- **`live`** (brand-accent orange): in-progress words currently being spoken.

Create a pure, deterministic `TranscriptBuffer` state reducer in `speechtt-core`:

```rust
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TranscriptBuffer {
    committed: String,
    live: String,
}
```

### Reducer semantics
- **`apply(&mut self, event: &SttEvent)`**:
  - `SttEvent::Partial { text, .. }`: sets `self.live = text.trim().to_string()`.
  - `SttEvent::Delta { delta, .. }`: appends `delta` to `self.live`.
  - `SttEvent::Committed { text, .. }`: appends `text.trim()` to `self.committed` (inserting a single space separator when `committed` is non-empty and doesn't end with whitespace) and clears `self.live`.
  - `SttEvent::Stopped { full_text, .. }`: if `full_text` is non-empty, sets `self.committed = full_text` and clears `self.live`; otherwise flushes any remaining `self.live` into `self.committed`.
- **Readers (`#[must_use]`)**:
  - `committed(&self) -> &str`
  - `live(&self) -> &str`
  - `full_text(&self) -> String` (combines `committed` and `live` with a space if both are non-empty)
  - `clear(&mut self)`

---

## 3. Duplex Backend Trait & Stdio Transport (`crates/speechtt-core/src/backend.rs`)

Decouple the transport behind a trait so `SessionController` and tests can run against an in-memory mock duplex channel or the real `bun` subprocess:

```rust
pub trait SttSessionSink: Send {
    fn send(&mut self, cmd: SttCommand) -> Result<(), BackendError>;
}
```

### `StdioTransport` (Read/Write Line Framing)
1. **Pure generic line transport (`StdioClient<W: Write, R: BufRead>`)**:
   - `send(&mut self, cmd: &SttCommand)` serializes one JSON line + `\n` and calls `writer.flush()`.
   - `read_event(&mut self) -> Result<Option<SttEvent>, BackendError>` reads a line from `R`, returning `Ok(None)` on `EOF`.
   - Because it is generic over `Read` + `Write`, unit tests can test the full NDJSON framing over in-memory `std::io::Cursor` / pipe pairs without spawning an OS process.
2. **Subprocess spawner (`BunSubprocess`)**:
   - Spawns `Command::new("bun").arg("run").arg(worker_script_path)` with `Stdio::piped()` for `stdin` and `stdout`, and `Stdio::inherit()` (or piped line logger) for `stderr`.
   - Spawns a background reader thread/task that parses `stdout` lines into `SttEvent`s and forwards them over an `std::sync::mpsc` or `futures::channel::mpsc` / `async_channel` to the UI.
   - Dropping `BunSubprocess` closes `stdin`, signalling the Bun worker to abort and exit cleanly.

---

## 4. `SessionController` & `SpeechTtApp` Duplex Wiring (`crates/speechtt-ui`)

### Controller Updates (`crates/speechtt-ui/src/controller.rs`)
Extend `SessionController` so it owns live session telemetry alongside `SessionMachine`:
- **Live state fields**:
  - `session_id: u64` (monotonic counter formatted as `"take-1"`, `"take-2"`, …)
  - `transcript: TranscriptBuffer`
  - `waveform_bars: Arc<[f32]>` (defaults to flat zeros when idle, updated by live audio FFT frames)
  - `elapsed_seconds: u32`
  - `last_error: Option<SharedString>`
  - `sink: Option<Box<dyn SttSessionSink>>` (optional injected sink for outbound `SttCommand`s)
- **Outbound lifecycle hooks**:
  - `start_recording()` → transitions `machine.start()`, resets `transcript` and `elapsed_seconds`, increments `session_id`, and emits `SttCommand::Start { session_id, provider, model, sample_rate, api_key }`.
  - `push_audio_chunk(pcm16: &[i16], bars: Arc<[f32]>)` → ignored unless `SessionState::Recording`; updates `self.waveform_bars = bars` and emits `SttCommand::audio_from_pcm16(pcm16)`.
  - `pause_recording()` → transitions `machine.pause()` and emits `SttCommand::Pause`.
  - `resume_recording()` → transitions `machine.resume()` and emits `SttCommand::Resume`.
  - `finish_take()` → transitions `machine.stop()`, emits `SttCommand::Stop`, flushes `transcript` into `FinishedTake { session, committed, live }`.
- **Inbound event handler**:
  - `handle_stt_event(&mut self, event: SttEvent)`:
    - Ignores events whose `session_id` does not match the active take (prevents late/stale packets from a previous take corrupting a new take).
    - Routes `Partial`, `Delta`, `Committed`, and `Stopped` to `self.transcript.apply(&event)`.
    - If `Stopped` arrives after `finish_take()` is already showing the `Stopped` screen, updates `self.result`'s `committed` text with the final flushed transcript.
    - Routes `Error` to `self.last_error`.

### UI View Updates (`crates/speechtt-ui/src/app.rs` & `views/transport.rs`)
- Replace `MOCK_ELAPSED_SECONDS`, `MOCK_BARS`, `MOCK_COMMITTED`, and `MOCK_LIVE` in `views/transport.rs` with readers from `app.controller()`:
  - `app.controller().elapsed_seconds()`
  - `app.controller().waveform_bars()` (requires updating `Waveform` to accept `Arc<[f32]>` or `&[f32]`)
  - `app.controller().transcript_committed()` and `app.controller().transcript_live()`
- In `SpeechTtApp::new`, wire an async receiver loop (`cx.spawn(...)`) that listens for incoming `SttEvent`s (and audio/FFT frames), calls `this.controller.handle_stt_event(event)`, and calls `cx.notify()` so `CaptionWell` re-renders immediately as tokens stream in.

---

## 5. Implementation Checklist (TDD-First)

- [x] **T2.1 — Protocol unit tests & implementation (`crates/speechtt-core/src/protocol.rs`)**:
  - Add `serde`, `serde_json`, and `base64` to `crates/speechtt-core/Cargo.toml`.
  - Write unit tests verifying exact camelCase NDJSON serialization/deserialization for all `SttCommand` and `SttEvent` variants (`start`, `audio`, `pause`, `resume`, `stop`, `ready`, `started`, `partial`, `delta`, `committed`, `stopped`, `error`).
  - Verify `Debug` formatting of `SttCommand::Start` masks `api_key`.
  - Verify `SttCommand::audio_from_pcm16` round-trips 16-bit little-endian samples to base64.
- [x] **T2.2 — `TranscriptBuffer` unit tests & implementation (`crates/speechtt-core/src/transcript.rs`)**:
  - Test sequence: `Partial("audio")` → `Delta(" capture")` → `Delta(" layer")` → `Committed("audio capture layer.")` → `Partial("next words")` → `Stopped`.
  - Assert `committed()` and `live()` at every step.
- [x] **T2.3 — Generic NDJSON line transport tests & implementation (`crates/speechtt-core/src/backend.rs`)**:
  - Test `StdioClient` over in-memory buffers (`BufReader`/`Vec<u8>`), asserting line-by-line flushing and `EOF` handling.
  - Implement `BunSubprocess` wrapper for spawning `bun run src/stt/worker.ts`.
- [x] **T2.4 — Controller duplex flow tests (`crates/speechtt-ui/tests/session_flow.rs`)**:
  - Using a mock `SttSessionSink`, test that `start_recording`, `push_audio_chunk`, `pause_recording`, `resume_recording`, and `finish_take` emit the exact expected `SttCommand` sequence: `Start` → `Audio` → `Pause` (audio ignored while paused!) → `Resume` → `Audio` → `Stop`.
  - Test feeding `SttEvent::Partial`, `SttEvent::Delta`, `SttEvent::Committed`, and `SttEvent::Stopped` into `controller.handle_stt_event(...)` and assert `CaptionWell` data (`transcript_committed()`, `transcript_live()`, and `finished_take()`) updates in real time.
  - Test that stale `SttEvent`s from an old `session_id` are ignored.
- [x] **T2.5 — UI View & `Waveform` wiring (`crates/speechtt-ui`)**:
  - Update `Waveform` to hold `Arc<[f32]>` (zero-copy clone per frame).
  - Connect `views/transport.rs` to `SessionController`'s live elapsed timer, waveform bars, and `TranscriptBuffer`.
  - Verify `cargo fmt -- --check`, `cargo clippy --all-targets -- -D warnings`, and `cargo test` pass.

---

## 6. Implementation Notes (T0002 landed)

### Files

| File | Contents |
| --- | --- |
| `crates/speechtt-core/src/protocol.rs` | `SttCommand`, `SttEvent`, `SecretString`, `audio_from_pcm16`, `audio_from_f32`, `SttEvent::session_id`, 10 unit tests |
| `crates/speechtt-core/src/transcript.rs` | `TranscriptBuffer` (`apply`, `committed`, `live`, `full_text`, `clear`), 10 unit tests |
| `crates/speechtt-core/src/backend.rs` | `SttSessionSink`, `BackendError`, `StdioClient<W, R>`, `BunSubprocess`, 8 unit tests |
| `crates/speechtt-core/Cargo.toml` | Added `async-channel`, `base64`, `serde` (derive), `serde_json` |
| `crates/speechtt-ui/src/controller.rs` | Live take telemetry, command emission, inbound event reducer |
| `crates/speechtt-ui/src/components/waveform.rs` | Shared `Arc<[f32]>` bars |
| `crates/speechtt-ui/src/views/transport.rs` | Reads controller elapsed/bars/transcript |
| `crates/speechtt-ui/src/app.rs` | Backend event pump (`connect_backend`) |
| `crates/speechtt-ui/tests/session_flow.rs` | 5 duplex-flow tests on top of the existing 12 |

### Protocol (T2.1)

- `SecretString` is `#[serde(transparent)]`: it serializes as the real key but
  `Debug` prints `SecretString("[REDACTED]")` and `Display` prints
  `"[REDACTED]"`. `expose_secret()` is the only way to read it.
- `Start.api_key` uses `skip_serializing_if = "Option::is_none"`, so a keyless
  start line is byte-identical to the T0003 contract.
- `audio_from_f32` clamps to `-1.0..=1.0`, scales by `i16::MAX` and rounds;
  NaN becomes a silence frame (`0`) rather than panicking.
- `SttEvent::session_id()` centralizes stale-packet filtering (and is used by
  `SessionController`).

### Transcript reducer (T2.2)

- `Partial` replaces the live tail (trimmed); `Delta` appends raw;
  `Committed` joins trimmed text with exactly one separator and clears the
  tail; `Stopped` replaces everything when `fullText` is non-empty, otherwise
  promotes the tail into `committed`.
- Events without text (`ready`, `started`, `error`) are no-ops.

### Backend (T2.3)

- `StdioClient` is transport-only: one JSON line + `\n` per command, flushed
  immediately; `read_event` skips blank lines, tolerates CRLF and returns
  `Ok(None)` at EOF. It implements `SttSessionSink`, so the same reducer works
  against memory buffers, pipes and the subprocess.
- Inbound events use `async_channel` (already in the dependency graph) because
  it is awaitable from GPUI's `cx.spawn` without a polling timer or blocking a
  foreground thread. The type alias is `SttEventReceiver`.
- `SttSessionSink` gained one additive, defaulted method, `events()`, so the
  controller can own the inbound half of a duplex backend without a second
  trait. Purely outbound sinks keep implementing only `send`.
- `BunSubprocess::spawn(script)` / `spawn_default()` run
  `bun run src/stt/worker.ts` with piped stdin/stdout and inherited stderr.
  A reader thread decodes stdout into the channel. Drop closes stdin (the
  worker's EOF signal), waits up to 100 ms for a clean exit, then kills and
  joins. `spawn_with(program, script)` exists as the test seam for the
  spawn-failure path.

### Controller and views (T2.4 / T2.5)

- New state: `session_id: u64` (wire id `take-N`), `TranscriptBuffer`,
  `waveform_bars: Arc<[f32]>` (36 flat zeros when idle), `elapsed_seconds`,
  `last_error`, optional `Box<dyn SttSessionSink>` and its event stream.
- `start_recording` mints the session id, clears the transcript, resets the
  clock/bars and emits `Start` with the machine's provider/model and 16 kHz.
  `push_audio_chunk` is a no-op unless `Recording`, so paused audio never
  reaches the provider. `pause`/`resume`/`finish` emit their commands only on a
  successful state transition.
- `handle_stt_event` drops events from a non-current `session_id` (late
  packets), applies transcript events, maps `Error` to `last_error`, and
  refines the already-visible `FinishedTake` when the final `Stopped` arrives.
- Offline fallback: with no sink attached and an empty buffer, `finish_take`
  still seeds the stopped screen with the previous mock transcript. This keeps
  the T0001 demo behavior (and its tests) until the worker is connected.
- `set_elapsed_seconds` is the capture-clock seam used by the audio pipeline;
  it only moves while a take is active.
- `Waveform::new` takes `impl Into<Arc<[f32]>>`, so `&MOCK_BARS[..]`,
  `Vec<f32>` and the controller's live `Arc<[f32]>` all work; the renderer
  clones a refcount, never the samples.
- `SpeechTtApp::new` installs the event pump (`start_event_loop`), and
  `connect_backend(sink, cx)` attaches a duplex backend and starts it. Until
  T0003 provides `src/stt/worker.ts`, `new` has no sink, so the pump is
  dormant rather than spawning a process during tests.
- Transport screens read `elapsed_seconds()`, `waveform_bars()`,
  `transcript_committed()` and `transcript_live()` from the controller;
  `MOCK_ELAPSED_SECONDS`/`MOCK_BARS` remain only as documented mockup
  reference constants.

### Verification

- `cargo fmt -- --check` — clean.
- `cargo clippy --all-targets -- -D warnings` — clean.
- `cargo test` — 62 passed (28 core unit + 11 session machine + 17 session
  flow + 4 theme contract + 2 widget smoke), 0 failed.

