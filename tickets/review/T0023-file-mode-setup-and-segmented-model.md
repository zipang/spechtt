# T0023 — File sessions must choose a non-streaming model in the setup form

- **Track**: Rust UI + Rust core + TypeScript/Bun worker
- **Owner**: coder sub-agent (supervised by CTO)
- **Started**: 2026-10-31
- **Wave**: 2 (integration)
- **Depends on**: T0015 (file transcription), T0020 (typed errors + diagnostics),
  T0011 (capability catalogue with `TranscriptionMode`)
- **Blocks**: T0021 (its `ViewState` and per-view table assume this order)
- **References**: `PRODUCT.md` §5, `AGENTS.md` §2/§5, `src/stt/worker.ts`
  (`defaultAdapterFactory`, `handleTranscribeFile`), `src/stt/registry.ts`
  (`resolveModelCapability`)

## Context & Goal

Repro: choose **From a file** and pick a media file. The view jumps straight to
the recording screen. **The `session_setup` form is never shown.**

That is wrong for two reasons.

1. **The flow has no configuration step.** File transcription is a different
   job from live dictation and legitimately needs a different model. Today the
   user never gets to choose one for a file take; whatever the live default is,
   gets reused silently.
2. **Only non-streaming models can transcribe a file.** A file take is decoded
   by `ffmpeg` and sent in batches through the AI SDK `transcribe` API
   (`src/stt/adapters/file.ts`). A streaming-only model has no batch endpoint:
   the request is built, sent, and fails at the provider. The worker hides this
   because `defaultFileModel` unconditionally routes through
   `defaultSegmentedModel`, ignoring the model's declared capability — so the
   user sees an opaque provider error instead of "this model cannot transcribe
   files".

The file picker staying first is fine and is kept: the user chooses a file, and
only then is asked which model should transcribe it. Cancelling the picker must
leave them exactly where they were — on the welcome screen — not on a
half-configured form.

**Done means:** choosing a file leads to the setup form (never directly to a
take), the model chooser only ever offers segmented (non-streaming) models,
Continue is disabled until such a model is picked, and both the controller and
the worker refuse a streaming model for a file take instead of failing at the
provider.

## Scope Boundary

May create or modify:

- `crates/speechtt-ui/src/controller.rs` — mode-aware draft seeding, filtered
  model list, completeness rule, `choose_mode` / pending-file entry behaviour,
  the `start_file_session` guard.
- `crates/speechtt-ui/src/app.rs` — picker stays on the mode card; a chosen
  path routes to the form instead of starting a take; Continue starts the take.
- `crates/speechtt-ui/src/views/session_setup.rs` — the file-mode note and the
  mode badge already available.
- `crates/speechtt-ui/src/diagnostics.rs` — variant-name mapping for the new
  `AppError` variant (append-only arm).
- `crates/speechtt-ui/tests/{session_flow,widget,end_to_end,error_ux}.rs`.
- `src/stt/protocol.ts` — one new `SttErrorCode`.
- `src/stt/worker.ts` + `src/stt/worker.spec.ts` — reject a streaming model for
  `transcribeFile`.
- `DESIGN.md` — the setup-screen paragraph only, if the note changes the
  documented copy.
- `tickets/README.md` — board row.

Do **not** touch: `src/stt/adapters/*` behaviour (the batch adapter is already
correct), the registry catalogues, the Settings dialog (its default model stays
free for live dictation), `settings.rs`, `design-tokens.css`, `tokens.rs`, the
footer work owned by T0021, or the recording screen owned by T0022. No new
component, no new colour, no new token, no visual-layout test.

## Contract

### 1. Pick a file first, then configure, then transcribe

Entry order stays: **mode card → native picker → setup form → take**.

- `SessionController::choose_mode(SessionMode::File)` sets the mode to `File`,
  clears the draft and pending file, **keeps `Screen::Mode`** and returns
  `false`. Seeding is deferred: there is nothing to configure until a file has
  been chosen. It does **not** open the picker itself and never reaches
  `Ready`.
- `app.rs::choose_mode(File)` keeps opening the picker exactly as today, after
  the controller call.
- **Cancelling the picker is a full no-op on the flow.** The user stays on the
  welcome screen (`ViewState::Mode`), with no draft, no error strip and no
  pending path. The app calls `change_mode()` on that branch so the recorded
  mode returns to its default.
- **Choosing a file does not start a take.** `set_pending_file(path)` remembers
  the path, resets and seeds the draft, and opens `Screen::Form`, where the user
  picks the model.
- **Continue starts the take for the pending file.** `app.rs::create_session`
  captures the pending path, commits the draft, then calls
  `start_file_from_path` with it. The picker is not shown a second time.
- **Back / Cancel from the form discards the pending file** and returns to the
  welcome screen. No take starts, nothing is written.
- `choose_mode(SessionMode::Live)` is unchanged: it returns `true` and reaches
  `Ready` when the persisted settings resolve.

### 2. The draft only ever holds a segmented model in file mode

- `seed_draft_from_settings`:
  - seeds the provider whenever the persisted provider exists in the catalogue;
  - seeds the model only when it exists **and** its mode is
    `TranscriptionMode::Segmented` while `mode == SessionMode::File`;
  - therefore, with a persisted streaming model, file mode opens with a
    provider and **no** model, and Continue is disabled until the user picks
    one.
- New query:

```rust
/// Model ids offered by the setup form for the current mode.
pub fn selectable_model_ids(&self, provider: &str) -> Vec<String>;
```

  In `SessionMode::File` it returns only models whose capability mode is
  `Segmented`; in `SessionMode::Live` it returns the whole provider list. It
  returns an empty `Vec` for an unknown provider or before the catalogue
  arrives.
- `cycle_model` iterates `selectable_model_ids`, so a streaming model is never
  reachable in file mode. When the current model is not in the selectable set
  (for example the draft was seeded before a catalogue update), the first
  cycle lands on the first selectable model.
- `cycle_provider` keeps clearing the dependent model, as today.

### 2b. The pending file

```rust
pub fn set_pending_file(&mut self, path: PathBuf);
pub fn pending_file(&self) -> Option<&Path>;
pub fn clear_pending_file(&mut self);
```

- `set_pending_file` stores the validated path and leaves the machine `Blank`
  with `Screen::Form`; it never starts a take.
- The pending file is cleared by `create_session` (after the take starts),
  `change_mode`, `cancel_form` and `new_take`.
- `view_state()` reports `Form` whenever no file take is running in file mode.
  There is no file-mode "ready" view: Continue goes straight into the take.
- The path is never logged and never placed in an error message.

### 3. Completeness and commit enforce the rule

- `is_draft_complete()` additionally requires, in `SessionMode::File`, that the
  draft model resolves to `Segmented`. A file draft holding a streaming model is
  incomplete, which is what disables Continue.
- `create_session()` returns `false` (changing nothing) for an incomplete file
  draft, including the streaming-model case.
- `start_file_session(path)` is guarded defensively, because it is also
  reachable from tests and future callers: before starting, it resolves the
  model's capability and, when it is `Streaming`, returns
  `Err(AppError::FileModelNotSupported)` and sends no `TranscribeFile`. Unknown
  models keep the current permissive behaviour (`resolveModelCapability`
  defaults to segmented).

New typed error, added to `AppError` and to `diagnostics::variant_name`:

```rust
/// A streaming-only model was selected for a file take.
FileModelNotSupported,
```

- `message()`: `"File transcription needs a non-streaming model."`
- `action()`: `ErrorAction::OpenSettings`
- `detail()`: `None`. Logged like any other error; no transcript, path or key
  material in the line.

### 4. The worker refuses too

- `handleTranscribeFile` resolves the capability **before** creating a session.
  When `resolveModelCapability(provider, model)?.mode === "streaming"` it emits
  exactly one event and returns:

```json
{"type":"error","code":"unsupported_file_model","message":"<model id> cannot transcribe files; choose a non-streaming model"}
```

  No `started`, no `progress`, no `stopped`, and no provider request is made.
  The message carries the model id only — never a path, key or transcript.
- `SttErrorCode` gains `"unsupported_file_model"`; `AppError::from_stt_error`
  maps it to `FileModelNotSupported` (it must not fall into the generic
  `BackendFailed` bucket).
- `defaultFileModel` / `createFileSession` stay as they are: with the guard in
  place the batch adapter only ever receives a segmented model.

### 5. Setup screen copy (file mode only)

- `views/session_setup.rs` shows the existing `components::InfoNote` above the
  fields when `mode() == SessionMode::File`, with the copy:
  `"Files are transcribed in batches, so only non-streaming models are listed."`
- The model row keeps its existing `components::TranscriptionModeBadge`, which
  therefore always reads the segmented mode for a file draft.
- No new component, no colour, spacing, radius or type decision in the view.

## TDD Tasks

- [x] **T23.1 — Red first: picker → form, never picker → take.**
      `tests/session_flow.rs`: `choose_mode(File)` returns `false` and reports
      `ViewState::Mode` with no draft and no pending file, whatever the saved
      settings say (the picker is still open at that point). `set_pending_file`
      then reports `ViewState::Form`: with configured **segmented** settings it
      seeds the provider and a complete draft; with configured **streaming**
      settings it seeds the provider only, leaving `is_draft_complete()` false.
      `choose_mode(Live)` keeps its current "straight to Ready" result. The old
      `choosing_file_with_configured_settings_goes_straight_to_ready` test is
      rewritten to the new contract.
- [x] **T23.2 — Red first: the pending file.** `set_pending_file` stores the
      path, keeps `session_state()` `Blank` and reports `Form`; `pending_file()`
      returns it; `clear_pending_file`, `change_mode`, `cancel_form` and
      `new_take` clear it; `create_session` clears it after committing; the path
      never appears in `AppError::detail()` or a diagnostic line.
- [x] **T23.3 — Red first: cancelling the picker is a no-op.**
      `app.choose_mode(File)` followed by a cancelled selection leaves
      `view_state() == ViewState::Mode`, no pending file, no error strip and no
      `SttCommand`. This test drives the headless seam, not the OS dialog.
- [x] **T23.4 — Red first: the model list is filtered.** New controller specs:
      `selectable_model_ids` returns only `Segmented` ids in file mode, all ids
      in live mode, and an empty list for an unknown provider; `cycle_model`
      never lands on a streaming id in file mode (walk the full cycle and
      assert every visited id); cycling from a streaming current model lands on
      the first selectable id.
- [x] **T23.5 — Red first: commit refuses a streaming model.**
      `is_draft_complete()` is `false` for a file draft pinned to a streaming
      model, even with a valid provider; `create_session()` returns `false` and
      leaves `session_state()` `Blank`. Live mode still accepts the same model.
- [x] **T23.6 — Red first: the take-start guard.**
      `start_file_session` on a `Ready` file machine whose model is streaming
      returns `Err(AppError::FileModelNotSupported)`, sends no command and
      leaves `view_state()` unchanged. A segmented model still starts the take.
- [x] **T23.7 — Red first: worker refusal.** `worker.spec.ts`: a
      `transcribeFile` command naming a streaming model emits exactly one
      `unsupported_file_model` error, sends nothing to the provider, and never
      emits `started`/`stopped`. A segmented model keeps the existing happy
      path. `protocol.spec.ts` covers the new code's round trip if it is
      validated there.
- [x] **T23.8 — Routing + fixtures.** `tests/end_to_end.rs`: the file helper
      walks the new order — `choose_mode(File)` (picker seam), then the form
      (`cycle_provider`, pick a segmented model, `create_session`) — and asserts
      the `TranscribeFile` command is sent only after Continue. The
      `openai/gpt-realtime-whisper` fixture used by file helpers is replaced
      with a segmented id. `tests/error_ux.rs` gains the new
      `FileModelNotSupported` row (message, action) and the
      `from_stt_error("unsupported_file_model", …)` mapping.
- [x] **T23.9 — Implement** until T23.1–T23.8 are green: the pending file,
      mode-aware seeding, `selectable_model_ids`, the completeness and commit
      rules, the `start_file_session` guard, the worker guard, the `InfoNote`.
- [x] **T23.10 — Docs + gates.** Update the setup-screen paragraph in
      `DESIGN.md` if the note changes the documented copy; update the board.
      Run `cargo fmt -- --check`, `cargo clippy --all-targets -- -D warnings`,
      `cargo test`, `bun test`, `bun check src/stt`. All green, zero warnings.

## Acceptance Criteria

1. Choosing **From a file** opens the file picker first.
2. Cancelling the picker leaves the user on the welcome screen, with no draft,
   no error and no started take.
3. Picking a file shows the session setup form; it never jumps to the recording
   screen directly.
4. The form's model chooser only offers non-streaming models in file mode;
   streaming models cannot be selected, by mouse or by cycling.
5. With a saved streaming model, the form opens with the provider filled in and
   no model selected, and Continue stays disabled until a segmented model is
   chosen.
6. Continue starts the transcription of the already-picked file, without asking
   for it again; Back/Cancel from the form returns to the welcome screen and
   discards it.
7. File transcription never reaches a provider with a streaming-only model: the
   controller refuses with "File transcription needs a non-streaming model."
   (with an Open Settings action), and the worker answers
   `unsupported_file_model` without making a request.
8. Live dictation keeps its current entry flow and may use any model.
9. All gates green with zero warnings; no layout, geometry or screenshot test
   added.

## Implementation Notes

### Decisions

- **Seeding is deferred to `set_pending_file`.** `choose_mode(File)` clears the
  draft and keeps `Screen::Mode`, so cancelling the native picker leaves no
  trace at all. The contract's §1 wording was corrected accordingly (it
  previously said `choose_mode` seeds and shows the form).
- **`create_session` consumes the pending path.** The app captures
  `pending_file()` *before* calling `create_session`, because the commit clears
  it; this keeps the "Continue starts the take without asking again" rule
  without a second field.
- **Cancel goes through `change_mode()`.** That resets the provisional
  `SessionMode::File` back to the default and clears the draft, which is what
  "no-op on the flow" means to an observer.
- **File mode has no holding screen.** `view_state()` maps
  `SessionState::Ready` in file mode to `ViewState::Form`; the old file "ready"
  branch in `views/ready.rs` is now unreachable and is left for T0021, which
  deletes that view.
- **Seeding now fills the provider even when the saved model is unusable**
  (missing from the catalogue, or streaming in file mode). Before, both fields
  were left empty. The draft is still incomplete, so Continue stays disabled;
  the user just picks a model from the right list instead of both fields.
- **`start_file_session` guard is defensive only.** A normal flow cannot reach
  it with a streaming model, so the test exercises the real-world case that
  motivated it: a catalogue refresh reclassifying an already-committed model as
  streaming (`set_capabilities` with a reclassified catalogue).

### Deviations

- **The picker-cancel e2e test is a controller-level test.** The gpui test
  harness cannot answer or dismiss a native path prompt, so the behaviour is
  pinned where it is decided: `choose_mode(File)` + `change_mode()` leave
  `Mode` with no draft, no pending file and no command. The app's cancel branch
  is a two-line call into exactly those methods.

### Verification

```text
cargo fmt -- --check                          → clean
cargo clippy --all-targets -- -D warnings     → clean, 0 warnings
cargo test                                    → 14 suites ok, 0 failed
bun test                                      → 217 pass, 0 fail
bun check src/stt                             → No type errors in 23 files
bun run design:validate                       → 66 tokens, 0 errors
```

