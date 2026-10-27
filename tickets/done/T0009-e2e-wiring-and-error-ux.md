# T0009 — Integration: End-to-End Wiring and Error UX

- **Track**: Cross-cutting (`crates/speechtt-core` + `crates/speechtt-ui` + bridge)
- **Wave**: 2 (integration)
- **Depends on**: T0004, T0005, T0006, T0007, T0008
- **Blocks**: Phase 9 polish / packaging
- **PRODUCT.md**: Phases 8 and 9
- **Owner**: coding sub-agent (deepseek-v4.1-flash), supervised by CTO
- **Started**: 2026-10-27

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

- [x] **T9.1 — `AppError` mapping tests** (`crates/speechtt-ui/tests/error_ux.rs`
  or controller unit tests): every source error maps to the expected variant;
  `SttEvent::Error` codes map deterministically; no key material in
  `Display`.
- [x] **T9.2 — Controller integration tests** (fakes only):
  - audio frame while Recording → exactly one `Audio` command; frame while
    Paused → none;
  - `partial` → `delta` → `committed` updates visible text and survives
    `finish_take`;
  - `Stopped` refines the finished take; stale session ids ignored;
  - hotkey actions produce the same transitions as button commands, with
    invalid actions rejected;
  - clock-driven elapsed read-out advances and resets per take.
- [x] **T9.3 — Output flow tests**: Copy/Save/Open call the injected adapters
  with exact text/path; Save failure shows `SaveFailed` and keeps the
  transcript; auto-paste runs only when enabled.
- [x] **T9.4 — Settings/capabilities tests**: capability payload populates
  options; unavailable provider renders not-connected; save round-trips to
  the store; `Start` carries the persisted key; missing key blocks recording.
- [x] **T9.5 — Happy-path end-to-end test** (`crates/speechtt-ui/tests/end_to_end.rs`):
  `MockAudioSource` → `MockSink`/`MockEventStream` → record → partial/delta →
  pause → resume → stop → Copy → assert clipboard text; repeat with File
  output → assert written path + opener called.
- [x] **T9.6 — Live bridge test**: keep T0008's smoke test green; optionally
  add an `#[ignore]` real-provider test gated on `SPEECHtt_LIVE_STT=1`.
- [x] **T9.7 — Manual QA checklist** (record in the ticket):
  - mic permission denied → error strip + recovery after granting;
  - invalid API key → auth error → Settings → fix → retry works;
  - unplug network mid-stream → error → retry resumes a new take;
  - hotkeys work while the widget is unfocused;
  - copy/paste into an external editor;
  - save to memory and to file; reopen the saved file;
  - visual QA against every `mockups/**/*.png`.
- [x] **T9.8 — Gates**: `cargo fmt -- --check`,
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

## Manual QA Checklist (T9.7)

Recorded here for the supervised manual pass. Automated gates cover the same
paths through injected fakes; the hardware/provider rows remain manual.

- [ ] Mic permission denied → error strip + recovery after granting.
- [ ] Invalid API key → auth error → Settings → fix → Retry works.
- [ ] Unplug network mid-stream → error → Retry starts a new take.
- [ ] Hotkeys work while the widget is unfocused.
- [ ] Copy/paste into an external editor.
- [ ] Save to memory and to file; reopen the saved file.
- [ ] Visual QA against every `mockups/**/*.png`.

## Implementation Notes

**Status**: implemented, automated gates green, awaiting CTO review. The
ticket stays in `tickets/in_progress/`.

### Files touched

- `crates/speechtt-core/src/lib.rs` — append-only re-exports of
  `async_channel` (audio/event pumps) and `serde_json` (UI error tests
  construct core error variants without a second dependency). No existing
  line changed.
- `crates/speechtt-ui/src/controller.rs` — `AppError` + `ErrorAction` +
  `Clock`/`SystemClock`, settings/capability state, hotkey `apply_action`,
  clock-driven elapsed time, output actions (copy/save/open/auto-paste),
  `MissingApiKey` guard, and Retry.
- `crates/speechtt-ui/src/app.rs` — `AppServices` composition root,
  `with_services` test seam, backend/audio/clock pumps, settings persistence,
  hotkey registration, status strip, output wrappers.
- `crates/speechtt-ui/src/views/stopped.rs` — real Copy/Save/Open File,
  written-path state, transient output status.
- `crates/speechtt-ui/src/views/dialogs/settings.rs` — capability-driven
  labels, unavailable-provider hint, persisted save path.
- `crates/speechtt-ui/tests/end_to_end.rs` — new (23 tests).
- `crates/speechtt-ui/tests/error_ux.rs` — new (9 tests).
- `crates/speechtt-ui/src/views/transport.rs` — reviewed, no edit needed: it
  already renders the clock-driven elapsed read-out and controller bars.

### Design decisions

- **Composition root.** `AppServices` has exactly the ticket's ten fields.
  `SpeechTtApp::with_services` destructures it; the sink goes to the
  controller, the receiver to one `cx.spawn` pump, and the rest stay in the
  app. `SpeechTtApp::new` builds `LazyCpalAudioSource`, `BunSubprocess`,
  `JsonFileStore`, `SystemClipboard`, `SystemPaste`, `SystemOpener`,
  `GlobalHotkeyListener` and `SystemClock`.
- **Backend failure.** A missing Bun binary is replaced by an
  `UnavailableSink` + closed channel and the app opens with
  `BackendUnavailable`. A worker that exits is also surfaced when its event
  stream closes.
- **Audio lifecycle.** The first `start_recording` moves the audio source to
  a dedicated thread that forwards chunks through an async channel; the UI
  pump computes FFT bars and posts them to the controller. Pause keeps the
  stream alive and the controller drops frames (the cheaper option). Finish /
  New take / app exit signal the thread and drop the source, releasing the
  device. A factory re-opens the device for later takes in production.
- **Elapsed time.** The controller stores an accumulated base plus the
  current segment start. `refresh_elapsed(Instant)` is driven by a 200 ms
  clock task and by audio frames; pause freezes the read-out, resume restarts
  the segment, and each take resets to zero.
- **Settings.** The draft key goes only to `SettingsStore::save` (the core
  store writes `0600`). `SettingsChange` remains redacted. `Start` uses the
  session's selected provider/model, falling back to persisted settings, and
  looks up the key for that provider. When `settings` is `Some`, a missing key
  reports `MissingApiKey`; when no settings were ever loaded (unit tests) the
  key stays `None` for the worker/environment fallback.
- **Open Settings.** The dialog seeds from `stored_settings` whenever a model
  is configured, so provider, model and CONNECTED state survive closing and
  reopening the dialog. `open_settings_with` remains the explicit seeding
  seam; a fresh install still opens on "Choose provider".
- **Capabilities.** `Capabilities` events populate labels, model lists and
  `available`; `settings_connected` is false when the selected provider is
  reported unavailable, even if a key is typed. `CONNECTED`/`NOT CONNECTED`
  therefore follows the worker's registry. `request_capabilities` forwards the
  stored key so a provider configured only in the settings file is still
  reported available; the worker never echoes it and `Debug` redacts it.
- **Errors.** `AppError` carries no transcript or key material. `Display` is a
  short fixed message; diagnostic detail is only in `detail()`. `SttEvent`
  codes are mapped in one table. `stream_failed` inspects the message only to
  split auth vs network vs generic protocol. Render shows a dismissible strip
  above the body with Open Settings / Retry / Dismiss; microphone permission
  shows an "Open settings" hint. Retry re-issues the stored `Start` with the
  same session id (worker-process restart is not recreated for injected
  sinks).
- **File takes.** `OutputTarget::File(non-empty)` is preserved by
  `create_session` (empty keeps the `~/Documents/<name>.md` default). A file
  take is written on `finish_take`; memory takes are written as Markdown on
  Save. Both expose the written path and Open File. `~` expands against
  `$HOME` only in the production path.

### Remaining gaps / follow-ups

- **OS hotkey event pump.** Bindings are registered and
  `SpeechTtApp::apply_action` applies them with the same guards as buttons
  (covered by T9.2), but `ShortcutListener` in `speechtt-core` has no event
  receiver, so production global events are not drained yet. Wiring them
  needs a small core trait extension (out of this ticket's scope).
- **Manual QA** rows above still need a machine with a microphone, network
  and a real provider key. Automated tests never require them.
- **Bun worker path.** `BunSubprocess` resolves `src/stt/worker.ts` relative
  to the process cwd. `cargo run -p speechtt-ui` from the repository root
  works; an installed/packaged binary will need an absolute path (backend
  ownership, not changed here).
- The shared headless fixtures now build `SpeechTtApp::with_services` with
  in-memory fakes, so `cargo test` no longer reads/writes the real settings
  file, spawns Bun or opens a microphone. The only remaining real-hardware
  path is `SpeechTtApp::new`, used by `run()` and by the manual checklist.

### Gate verification (2026-10-27)

```text
cargo fmt -- --check
  FMT_OK

cargo clippy --all-targets -- -D warnings
  Finished `dev` profile ... (no warnings)

cargo test
  speechtt-core lib:        164 passed
  bridge_smoke:               2 passed
  session_machine:           11 passed
  speechtt-ui lib:           14 passed
  dialogs:                    8 passed
  end_to_end:                23 passed
  error_ux:                   9 passed
  session_flow:              17 passed
  theme_contract:             4 passed
  widget:                     2 passed
  total: 254 passed, 0 failed, 0 ignored

bun test
  87 pass, 0 fail, 443 expect() calls (7 files)

bun check src/stt
  No type errors in 13 files
```

The `error: Module not found "src/stt/worker.ts"` lines printed by the
headless UI tests come from Bun's cwd being the `speechtt-ui` package
directory; the app treats the resulting closed event stream as
`BackendUnavailable`. The T0008 live bridge smoke test (run from the repo
root) stays green.


## Follow-up Fixes After CEO/PM Review (2026-10-27)

### Defect 1 — New Session form now uses Settings + capabilities

- Removed the legacy 2-entry `const PROVIDERS` mock and its index-based
  `SessionDraft`. `SessionDraft` now stores `provider: Option<String>` and
  `model: Option<String>` catalogue ids.
- `open_form()` seeds the draft from persisted `Settings` when the saved
  provider and model both exist in the active catalogue (capabilities first,
  `provider_options()` fallback). A fresh controller, or a settings file with
  no model, still starts empty so `session_flow.rs` behaviour is unchanged.
- `apply_settings()` re-seeds an open form with the new defaults.
- `cycle_provider`, `cycle_model`, `provider_label`, `model_label`,
  `is_draft_complete*` and `validate_draft` all use the same catalogue
  helpers as the Settings dialog.
- `create_session()` commits the selected provider/model ids to
  `SessionMachine`.
- `start_configuration()` prefers the session's frozen machine provider/model,
  then persisted settings, and looks up the key for that provider. When
  `settings` is `None` (unit tests that never configured it), `Start` keeps
  `api_key: None`; when `settings` is `Some`, a missing key reports
  `AppError::MissingApiKey`.
- `session_flow.rs` provider/model assertion updated from the old mock labels
  (`"Vercel AI Gateway"` / `"voxtral-mini-transcribe-3"`) to the real catalogue
  ids (`"vercel"` / `"fish-audio/transcribe-1"`), as allowed by the ticket.

### Defect 2 — Re-opening Settings shows persisted values

- `SpeechTtApp::with_services_and_factory` now always calls
  `controller.apply_settings(stored_settings.clone())`, including an empty
  model.
- `SpeechTtApp::open_settings` seeds the draft with
  `open_settings_with(Some(provider), Some(model), has_key)` when
  `stored_settings.model` is non-empty, so re-opening shows the saved
  provider, model and CONNECTED state.
- Covered by `the_app_constructs_from_fakes_and_persists_settings`, which
  reopens Settings after saving and asserts provider/model/`api_key_unchanged`
  /CONNECTED.

### Defect 3 — Hermetic shared test window

- `tests/common/mod.rs` now builds `SpeechTtApp::with_services` with:
  in-memory `SettingsStore`, silent `AudioSource`, no-op `SttSessionSink`
  (with an intentionally kept-open event channel), no-op clipboard/paste/
  opener/shortcut listener and `SystemClock`.
- `open_speechtt_window` / `open_speechtt_window_at` no longer spawn Bun,
  open a microphone, or read/write the user's real settings file.
- Added `the_shared_test_window_does_not_write_the_real_settings_file`, which
  snapshots the real file and asserts it is byte-identical after opening the
  shared window and saving Settings through the fake store.
- Removed the `"mistral": "sk-app-test"` pollution from
  `~/.config/speechtt/settings.json`; the file now keeps only the user's
  `vercel` key and remains mode `0600`.

### Follow-up gate verification

```text
cargo fmt -- --check            FMT_OK
cargo clippy --all-targets -D warnings   no warnings
cargo test                     254 passed, 0 failed, 0 ignored
  end_to_end                    23 passed
bun test                        87 pass, 0 fail
bun check src/stt               no type errors in 13 files
```

## UI Alignment Follow-up — AppFooter + Settings-Style Setup Form (2026-10-27)

### New `AppFooter`

- Added `crates/speechtt-ui/src/components/app_footer.rs` (exported from
  `components/mod.rs`). It mirrors `AppHeader` at the bottom edge:
  `w_full()`, `flex_shrink_0()`, `items_center()`, `px_5()`, `py_2p5()`,
  `bg(cx.theme().title_bar)`, `border_t_1()` and
  `border_color(cx.theme().title_bar_border)`.
- API: `AppFooter::new()`, optional `.leading(impl IntoElement)` and
  `.action(impl IntoElement)`. `justify_between()` with a leading slot,
  `justify_end()` when only trailing actions exist.
- `DialogShell` now renders Header → padded scrollable body →
  full-bleed `AppFooter` sibling. The footer is no longer nested inside the
  `.p_5()` body box.
- Settings uses `AppFooter` for Cancel / Save changes; Help uses it for the
  version + gateway status leading slot and Got it action.
- The New Session form uses a window-level `AppFooter` sibling in
  `SpeechTtApp::render` with Cancel / Create session. Create stays disabled
  until the live session-name input plus provider/model/output are complete,
  and the footer is pinned while the body scrolls.

### Settings-style New Session form

- `views/setup.rs::render_form` now uses the same four
  `SettingRow::new(...).prominent()` rows as Settings:
  Transcription Service, Model, Session Title, Output.
- Provider/model use the same dropdown `Button` style and fallback labels as
  Settings (`Choose provider`, `Choose model`, `Choose provider first`).
- Body uses `p_5()`, `gap_2()` and `overflow_y_scrollbar()`; the main window
  drops its compact `px_5/py_2p5` inset only for the Form state so the form
  matches the Settings padding exactly.

### UI-alignment gates

```text
cargo fmt -- --check                    FMT_OK
cargo clippy --all-targets -- -D warnings   no warnings
cargo test                              254 passed, 0 failed
  dialogs                                8 passed
  end_to_end                            23 passed
  widget                                 2 passed
bun run design:validate                 ✓ 66 tokens, 0 errors
```
