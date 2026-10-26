# T0009 — Integration: End-to-End Wiring and Error UX

- **Track**: Cross-cutting (`crates/speechtt-core` + `crates/speechtt-ui` + bridge)
- **Wave**: 2 (integration)
- **Depends on**: T0004, T0005, T0006, T0007, T0008
- **Blocks**: Phase 9 polish / packaging
- **PRODUCT.md**: Phases 8 and 9

## Context & Goal

Everything needed for a real dictation exists after Wave 1, but the pieces
are still disconnected: the UI reads mock bars, the Stopped-screen buttons
are stubs, hotkeys and settings are not wired, and failures are invisible.
This ticket performs the **single integration pass** and adds **error UX**:

```text
mic (T0005) → controller → BunSttBackend (T0002/T0008) → transcript
     ↓             ↑                                        ↓
 hotkeys (T0006)  settings (T0006)                 output adapters (T0007)
```

It also introduces one composition root (`AppServices`) so the widget can be
constructed with fakes in tests and with real adapters in `run()`.

## Scope Boundary

This is the only ticket allowed to touch many files; keep edits surgical and
do not redesign components.

```text
crates/speechtt-core/src/lib.rs                 # re-exports only
crates/speechtt-ui/src/app.rs                   # composition root + wiring
crates/speechtt-ui/src/controller.rs            # integration commands/state
crates/speechtt-ui/src/views/transport.rs       # live bars/timer
crates/speechtt-ui/src/views/stopped.rs         # real Copy/Save/Open
crates/speechtt-ui/src/views/dialogs/settings.rs# capability-driven options
crates/speechtt-ui/tests/end_to_end.rs          # new
crates/speechtt-ui/tests/error_ux.rs            # new
src/stt/*                                       # only if a smoke gap is found
```

## 1. Composition root (`crates/speechtt-ui/src/app.rs`)

```rust
pub struct AppServices {
    pub audio: Box<dyn AudioSource>,
    pub analyzer: SpectrumAnalyzer,
    pub sink: Box<dyn SttSessionSink>,
    pub events: SttEventReceiver,
    pub settings: Box<dyn SettingsStore>,
    pub clipboard: Box<dyn ClipboardSink>,
    pub paste: Box<dyn PasteSink>,
    pub opener: Box<dyn FileOpener>,
    pub listener: Box<dyn ShortcutListener>,
    pub clock: Box<dyn Clock>, // now() + elapsed for deterministic tests
}
```

- `SpeechTtApp::with_services(services, cx)` is used by tests.
- `SpeechTtApp::new(cx)` builds real services: `CpalAudioSource` (lazily
  started on record), `BunSubprocess::spawn_default()`, `JsonFileStore`,
  `SystemClipboard`, `SystemPaste`, `SystemOpener`, `GlobalHotkeyListener`.
- If the Bun worker fails to spawn, the app still opens and surfaces
  `BackendUnavailable` instead of panicking.
- Audio capture starts on `start_recording` and stops on `finish_take` /
  pause (pause stops feeding the backend but keeps the capture stream alive
  only if that is cheaper; otherwise fully stop and restart on resume —
  document the choice).

## 2. Runtime loop

- One `cx.spawn` task drains `AppServices::events` and calls
  `controller.handle_stt_event(event)` then `cx.notify()`.
- Audio: a dedicated thread/task reads `AudioSource::next_chunk`, computes
  `SpectrumAnalyzer::bars(&window)`, and posts
  `ControllerInput::AudioFrame { pcm, bars }` to the UI thread. The
  controller forwards `SttCommand::Audio` only while `Recording` (paused
  frames are dropped, matching T0002).
- Elapsed time comes from `Clock`, not a wall-clock read in render code.
- Hotkey events from `ShortcutListener` dispatch to
  `controller.apply_action(SessionAction)` with the same guards as clicks
  (e.g. `StopRecording` inert while Recording).
- On `Stopped`, the final `TranscriptBuffer::full_text()` is the source of
  truth; if the backend never sends `Stopped` within a grace period, flush
  the buffer locally and surface `BackendTimeout`.
- On app exit, `unregister_all`, stop audio, drop the backend (stdin EOF).

## 3. Settings and capabilities

- On startup (and on opening the Settings dialog), send
  `SttCommand::Capabilities` and populate provider/model options from the
  `Capabilities` event; show `CONNECTED` when the selected provider is
  `available`, else `NOT CONNECTED` with an "add API key" hint.
- Save in the dialog persists through `SettingsStore::save`; the draft key is
  written only to the store (mode `0600`) and passed to `Start` as
  `SecretString`.
- `Start` uses the persisted provider/model/key; a missing key prevents
  recording and raises `MissingApiKey` with a shortcut to Settings.
- `auto_paste`: when enabled, `finish_take` copies the transcript and calls
  `PasteSink::paste()` after the Stopped screen appears; failures are
  non-fatal and reported.

## 4. Output actions (`views/stopped.rs`)

- **Copy** → `copy_transcript`, then a transient "Copied" status.
- **Save** (memory take) → `save_transcript` into `~/Documents` (or the
  configured directory) using `TranscriptFormat::Markdown`; the UI switches
  to a written-path state with **Open File**.
- **Open File** → `FileOpener::open(&path)`.
- File takes: only the path + Open File, as specified in T0001.

## 5. Error UX (`PRODUCT.md` Phase 9)

Add a single typed error surface used by every layer:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppError {
    MicPermissionDenied,
    NoInputDevice,
    MissingApiKey { provider: String },
    BackendUnavailable,
    BackendTimeout,
    ProviderAuthFailed,
    NetworkLost,
    SaveFailed(String),
    ClipboardFailed,
    PasteFailed,
    ProtocolError(String),
}
```

- Map `AudioError`, `BackendError`, `SttEvent::Error { code, message }`,
  `SettingsError` and `OutputError` into `AppError` in one place
  (`AppError::from_*`), with unit tests for each arm.
- Rendering: a dismissible status strip above the body with a short message
  and an action (`Open Settings`, `Retry`, `Dismiss`). It must not replace the
  transport controls or move them between Recording/Paused.
- Retry semantics: `BackendUnavailable` / `NetworkLost` restart the backend
  process and re-issue `Start` with the same session id; `ProviderAuthFailed`
  opens Settings; `MicPermissionDenied` offers "Open settings" text only.
- Error text never includes API keys or full transcripts.

## 6. TDD Tasks

- [ ] **T9.1 — `AppError` mapping tests** (`crates/speechtt-ui/tests/error_ux.rs`
  or controller unit tests): every source error maps to the expected variant;
  `SttEvent::Error` codes map deterministically; no key material in
  `Display`.
- [ ] **T9.2 — Controller integration tests** (fakes only):
  - audio frame while Recording → exactly one `Audio` command; frame while
    Paused → none;
  - `partial` → `delta` → `committed` updates visible text and survives
    `finish_take`;
  - `Stopped` refines the finished take; stale session ids ignored;
  - hotkey actions produce the same transitions as button commands, with
    invalid actions rejected;
  - clock-driven elapsed read-out advances and resets per take.
- [ ] **T9.3 — Output flow tests**: Copy/Save/Open call the injected adapters
  with exact text/path; Save failure shows `SaveFailed` and keeps the
  transcript; auto-paste runs only when enabled.
- [ ] **T9.4 — Settings/capabilities tests**: capability payload populates
  options; unavailable provider renders not-connected; save round-trips to
  the store; `Start` carries the persisted key; missing key blocks recording.
- [ ] **T9.5 — Happy-path end-to-end test** (`crates/speechtt-ui/tests/end_to_end.rs`):
  `MockAudioSource` → `MockSink`/`MockEventStream` → record → partial/delta →
  pause → resume → stop → Copy → assert clipboard text; repeat with File
  output → assert written path + opener called.
- [ ] **T9.6 — Live bridge test**: keep T0008's smoke test green; optionally
  add an `#[ignore]` real-provider test gated on `SPEECHtt_LIVE_STT=1`.
- [ ] **T9.7 — Manual QA checklist** (record in the ticket):
  - mic permission denied → error strip + recovery after granting;
  - invalid API key → auth error → Settings → fix → retry works;
  - unplug network mid-stream → error → retry resumes a new take;
  - hotkeys work while the widget is unfocused;
  - copy/paste into an external editor;
  - save to memory and to file; reopen the saved file;
  - visual QA against every `mockups/**/*.png`.
- [ ] **T9.8 — Gates**: `cargo fmt -- --check`,
  `cargo clippy --all-targets -- -D warnings`, `cargo test`, `bun test`,
  `bun check src/stt`.

## Acceptance Criteria

- A real recording produces a live waveform, live transcript, and a finished
  take that can be copied, pasted and saved — with no mock constants left in
  the render path.
- Every failure surface (mic, auth, network, backend, save) shows a typed,
  actionable message and never leaks secrets.
- Tests construct the app with fakes; only the manual checklist touches real
  hardware/providers.
- `PRODUCT.md` Phases 8–9 can be marked complete after the manual checklist.
