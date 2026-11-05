# T0030 — Clean the transcript with a language model

- **Track**: Cross-cutting (Rust core protocol + Bun worker + Rust UI + design system)
- **Wave**: 1 (independent)
- **Depends on**: nothing (uses the T0028 dropdowns and the T0029 settings plumbing)
- **Blocks**: nothing
- **References**:
  - `AGENTS.md` §2 (component/view rules), §5 (gates), §6 (official documentation), §9 (secret safety)
  - `DESIGN.md` § "Action buttons", § "Icon buttons", § "Screen footer", § "Settings panel"
  - Vercel AI SDK `generateText`: <https://ai-sdk.dev/docs/reference/ai-sdk-core/generate-text.md>
  - AI SDK documentation index: <https://ai-sdk.dev/llms.txt>
  - gpui-kit icons and assets: <https://gpui-kit.com/docs/assets.md>, <https://gpui-kit.com/component/icon.md>
  - gpui-kit component index: <https://gpui-kit.com/llms.txt>

## Context & Goal

A streaming transcription produces artefacts: typos where the model guessed a
word, and fragments glued end to end when a segment boundary landed mid-phrase.
The text is usable but needs a reading pass every time. The last `Stopped` event
often arrives much cleaner than the live buffer, which proves a text model can
repair a lot of this cheaply.

This ticket adds a **cleanup pass** to the finished-take screen: a small
magic-wand action in the navigation footer, left of Copy and Download, that
sends the transcript to a language model and replaces it with the corrected
version. The correction is reversible, and everything downstream (Copy, Save,
automatic paste) uses the text that is on screen.

Done means: one click cleans the transcript through the Bun worker, the cleaned
text is what the user sees and copies, a second click restores the original, the
model is configurable in Settings, and `DEFAULT_LANGUAGE_MODEL` provides the
built-in default when nothing is chosen.

## Scope Boundary

Allowed to create or modify:

- `crates/speechtt-core/src/protocol.rs` (new command, new event, error correlation)
- `crates/speechtt-core/src/settings.rs` (`language_model`)
- `src/stt/protocol.ts` (mirror of the Rust protocol)
- `src/stt/clean.ts` + `src/stt/clean.spec.ts` (new: prompt and call)
- `src/stt/worker.ts` + `src/stt/worker.spec.ts` (route `cleanTranscript`)
- `src/stt/discovery.ts` + `src/stt/discovery.spec.ts` (language models)
- `src/stt/registry.ts` + `src/stt/registry.spec.ts` (`DEFAULT_LANGUAGE_MODEL`, capabilities payload)
- `crates/speechtt-ui/src/controller.rs`
- `crates/speechtt-ui/src/app.rs`
- `crates/speechtt-ui/src/components/model_select.rs`
- `crates/speechtt-ui/src/components/mod.rs` (append-only)
- `crates/speechtt-ui/src/views/session_end.rs`
- `crates/speechtt-ui/src/views/dialogs/settings.rs`
- `crates/speechtt-ui/src/views/dialogs/window.rs`
- `crates/speechtt-ui/src/lib.rs` (one icon added to `icon_assets!`)
- `crates/speechtt-ui/src/diagnostics.rs` (new event in the trace)
- `crates/speechtt-ui/tests/{common/mod.rs,widget.rs,end_to_end.rs,session_flow.rs,dialogs.rs}`
- `DESIGN.md`
- `tickets/README.md` and this ticket

Out of scope — do **not** touch:

- `crates/speechtt-ui/src/views/{welcome,session_setup,recording,session_transcription}.rs`
- `crates/speechtt-ui/src/views/dialogs/help.rs`
- `crates/speechtt-core/src/transcript.rs` (the live buffer is not rewritten in place)
- `src/stt/adapters/**` and `src/stt/stream-controller.ts` (transcription is unchanged)
- `crates/speechtt-ui/src/tokens.rs`, `design-tokens.css`, `tests/design_system.rs`
- `crates/speechtt-ui/src/components/setting_row.rs`

## Contract

### 1. Wire protocol: one command, one event, correlated errors

```rust
// crates/speechtt-core/src/protocol.rs
#[serde(rename_all = "camelCase")]
CleanTranscript {
    /// Client-chosen id echoed by `Cleaned` and by any `Error`.
    request_id: String,
    /// Gateway provider id, the same one the transcription used.
    provider: String,
    /// Language model id, e.g. `openai/gpt-4.1-mini`.
    model: String,
    /// The transcript to repair, exactly as displayed.
    text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    api_key: Option<SecretString>,
},

#[serde(rename_all = "camelCase")]
Cleaned {
    request_id: String,
    /// The repaired transcript, plain text.
    text: String,
},
```

`SttEvent::Error` gains a correlated id so a failed cleanup reaches the same
button that asked for it:

```rust
Error {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    session_id: Option<String>,
    /// Set when the error answers a `CleanTranscript`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    request_id: Option<String>,
    code: String,
    message: String,
},
```

New error code string, worker side: `cleanup_failed`. The `Debug`/`Display` of
every added type redacts nothing that is secret (`api_key` is a `SecretString`
and already prints `[REDACTED]`); the transcript text must never appear in an
error message — same rule as `AppError` today.

The TypeScript mirror in `src/stt/protocol.ts` adds the same two shapes:
`CleanTranscriptCommand` in `InboundCommand`, `CleanedEvent` in
`OutboundEvent`, `requestId?` on `ErrorEvent`, and parses/validates the new
command in `parseInboundLine` (a missing or empty `requestId`, `model` or `text`
is `invalid_protocol`).

### 2. Worker: the cleanup call — `src/stt/clean.ts`

```ts
/** Default model when neither settings nor the environment names one. */
export const FALLBACK_LANGUAGE_MODEL = "openai/gpt-4.1-mini";
/** Environment variable naming the default cleanup model. */
export const LANGUAGE_MODEL_ENV = "DEFAULT_LANGUAGE_MODEL";

export interface CleanRequest {
  readonly provider: string;
  readonly model: string;
  readonly text: string;
  readonly apiKey?: string;
}

export interface CleanDeps {
  /** Injected for tests; production passes the AI SDK `generateText`. */
  generate(args: { model: string; prompt: string }): Promise<{ text: string }>;
}

/** Repairs typos and fragment joins. Never returns an empty string: when the
 * model answers with nothing usable, the input is returned unchanged. */
export async function cleanTranscript(
  request: CleanRequest,
  deps: CleanDeps,
): Promise<string>;
```

`cleanTranscript` builds one prompt from two parts and returns
`result.text.trim()`; when that is empty it returns `request.text.trim()`
unchanged, so the UI can never blank a transcript. The prompt is:

- **Instructions** — repair a dictated transcript.
- **The transcript** — passed verbatim, delimited by a line that cannot occur in
  dictation, so the model cannot confuse it with the instructions.

The instructions are written as **categories of defect with illustrative
examples**, never as a closed list: every transcription model produces its own
artefacts, and an enumeration would be wrong for the next one. The categories:

1. **Transcription artefacts** — words glued together or split apart, stray
   spaces inside a word or around an apostrophe or hyphen (`d 'une` → `d'une`,
   `dict ée` → `dictée`), letter sequences that spell a word the speaker clearly
   said (`Trans cribe to Stream` → `Transcribe to Streaming`), doubled
   punctuation, and capitalisation at sentence starts. The instruction names the
   *symptom* a reader would notice rather than the byte pattern that produced it.
2. **Fragment joins** — a live transcriber commits windows that overlap, so a
   phrase can appear twice end to end or be cut mid-sentence. The instruction is
   to produce one continuous, correctly punctuated text.
3. **Hesitations** — filler and false starts the speaker did not mean to keep:
   `euh`, `bah`, `ben`, `voilà`, `en fait` when used as padding, a word restarted
   two or three times, an abandoned sentence fragment. Remove them; do not
   replace them with other words.
4. **The speaker's own repetitions** — a word or short phrase repeated because
   the speaker stumbled (`beaucoup, beaucoup de doublons`). Keep a repetition that
   carries meaning (emphasis, a genuine list, a quoted repetition) and drop the
   one that does not.

Fidelity rules, stated explicitly because they bound the four categories:

- Keep the speaker's **words, order and register**. Do not rewrite for style,
  do not merge or split sentences to improve them, do not harmonise the tone.
- Correct a sentence's grammar only where it is **broken as transcribed**
  (mis-glued words, a dropped article caused by a window boundary), not where it
  is merely informal. Spoken French stays spoken French.
- **Never add content**: no new facts, no transitions, no connective
  paragraph-opening, no closing sentence. Never summarise, never expand, never
  translate.
- Keep the original language.
- When a passage is already clean, leave it untouched — a clean transcript must
  come back identical.
- Answer with the corrected text only: no preamble, no explanation, no quotes,
  no markdown, no code fence.

`cleanTranscript` therefore has one more contract clause: **idempotence for
clean input**. Feeding an already-cleaned text through the pass again returns it
unchanged (trailing whitespace aside), and that is a pinned test.

`generateText` is called with a low `temperature` (documented parameter,
<https://ai-sdk.dev/docs/reference/ai-sdk-core/generate-text.md>) so two runs
over the same transcript stay close: the pass repairs, it does not improvise.

Production deps call the documented AI SDK API — the only authority for it:

```ts
// https://ai-sdk.dev/docs/reference/ai-sdk-core/generate-text.md
import { generateText } from "ai";
const { text } = await generateText({ model, prompt });
```

`model` is the gateway id string, exactly as the transcription path already
passes model ids to the AI SDK.

### 3. Worker: routing and capability discovery

`src/stt/worker.ts` routes `cleanTranscript` without touching the active
transcription session:

1. Resolve the provider's API key exactly like `start` does
   (`resolveProviderApiKey`); a missing key is `missing_api_key`.
2. Call `cleanTranscript` with the injected `generate` seam (a new optional
   `WorkerDeps.generate`, defaulting to the AI SDK call).
3. Emit `{ type: "cleaned", requestId, text }`.
4. Any thrown error emits
   `{ type: "error", requestId, code: "cleanup_failed", message: sanitized }`
   through the existing `sanitizeDiagnostic`. A transcript-bearing message is
   never emitted.

`src/stt/discovery.ts` learns the `language` model type. `parseModels` keeps its
transcription filter and a second exported parser collects
`type === "language"` entries as `{ id, label }` (sorted by label, deduped),
exposed as `DiscoverResult.languageModels`. `src/stt/registry.ts` adds:

```ts
export const FALLBACK_LANGUAGE_MODEL = "openai/gpt-4.1-mini";
export const LANGUAGE_MODEL_ENV = "DEFAULT_LANGUAGE_MODEL";

/** Default cleanup model: the environment wins, then the fallback. */
export function defaultLanguageModel(env: Env = process.env): string;

export function listCapabilities(options): {
  providers: ProviderCapabilityInfo[];
  /** `null` when discovery returned none: the UI then offers free text. */
  languageModels: readonly { id: string; label: string }[] | null;
  defaultLanguageModel: string;
};
```

The capabilities event therefore grows two fields, both optional on the Rust
side so an older worker still parses:

```rust
Capabilities {
    // …existing fields…
    #[serde(default, skip_serializing_if = "Option::is_none")]
    language_models: Option<Vec<LanguageModelInfo>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    default_language_model: Option<String>,
}

/// One text model offered for the cleanup pass.
pub struct LanguageModelInfo { pub id: String, pub label: String }
```

### 4. Settings: one persisted field, one seeded default

```rust
// crates/speechtt-core/src/settings.rs
/// Cleanup model id. Empty means "use `DEFAULT_LANGUAGE_MODEL`, else the
/// worker's fallback".
pub language_model: String,
```

- Default is empty; `Debug` prints it (a model id is not a secret).
- It is **not** part of `validate()`: a take must remain startable with no
  cleanup model configured, the wand simply stays disabled.
- `SettingsChange` and `SettingsDraft` carry it. The Settings dialog shows the
  value the worker reported as `default_language_model` when the stored field is
  empty, so the user sees an actual model id rather than a blank row.

### 5. Controller: the cleanup request as a small state machine

```rust
/// Cleanup state of the finished take.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CleanState {
    /// No model configured (no stored id, no worker default).
    Unavailable,
    /// A clean transcript is on screen; `undo` restores the original.
    Cleaned,
    /// Idle with a model available.
    Ready,
}

/// What the wand button should do next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CleanStatus { Unavailable, Ready, Running, Cleaned }
```

Controller API (all pure, no I/O — the app owns the sink):

```rust
pub fn clean_status(&self) -> CleanStatus;
/// Model the cleanup would call: stored id, else the worker default.
pub fn cleanup_model(&self) -> Option<&str>;
/// `true` when a shot can be taken: finished non-empty take, model known,
/// not already running, not already cleaned.
pub fn can_clean_transcript(&self) -> bool;
/// Starts one request and returns the id to send, or `None`.
pub fn begin_clean_transcript(&mut self) -> Option<String>;
/// Folds a successful `Cleaned`: keeps the original, shows the repaired text.
pub fn apply_cleaned(&mut self, request_id: &str, text: &str) -> bool;
/// Folds a failure: drops the pending request and reports an `AppError`.
pub fn fail_clean_transcript(&mut self, request_id: &str, error: AppError) -> bool;
/// Restores the original transcript (second click on the wand).
pub fn undo_clean_transcript(&mut self) -> bool;
/// True while a request is outstanding.
pub fn clean_is_running(&self) -> bool;
```

Rules:

- A request id the controller did not issue, or one issued for the previous
  take, is ignored (monotonic `clean_seq`, reset by `new_take`).
- `apply_cleaned` stores the pre-clean `FinishedTake` text in a
  `clean_original: Option<(SharedString, SharedString)>` and sets
  `committed = text`, `live = ""`, so the screen, Copy, Save and the automatic
  paste all read one source.
- `undo_clean_transcript` puts the stored pair back and clears `clean_original`.
- `finished_take_is_empty` stays about the **displayed** text.
- Starting a new take, cancelling, resetting or finishing another take clears
  the cleanup state.

### 6. App wiring

- `SpeechTtApp` gains `_clean_task: Option<Task<()>>`-free design: the request is
  a plain `send` on the existing sink, and the answer arrives on the existing
  event pump, which already runs `flush_auto_paste`. `handle_stt_event` routes
  `Cleaned` to `apply_cleaned` and `Error { request_id: Some(..) }` to
  `fail_clean_transcript`.
- New app method `clean_transcript(&mut self, cx)` — the button handler: asks
  the controller for a request id, sends
  `SttCommand::CleanTranscript { request_id, provider, model, text, api_key }`
  using the same provider/key resolution the take used, and notifies.
- The automatic paste of a take must keep copying the text on screen: it reads
  the finished take, so it already follows the cleaned text (no change needed;
  pin it with a test).

### 7. UI: the wand in the footer, one row in Settings

`views/session_end.rs` — in the **unwritten** branch, the trailing zone gains one
`IconAction` **before** Copy and Save (the trailing zone renders left to right in
insertion order, so this places the wand to their left):

```rust
IconAction::new("btn-clean-transcript", IconName::WandSparkles)
    .label("Clean up text")
    .disabled(!app.controller().can_clean_transcript())
    .on_click(cx.listener(|this, _event, _window, cx| this.clean_transcript(cx)))
```

plus, when `clean_status() == Cleaned`, a second `IconAction`
(`"btn-undo-clean"`, `IconName::RotateCcw`) whose label is
`"Undo cleanup"`. While `Running`, the wand is disabled and an
`InfoNote`-free muted `Label` "Cleaning up…" replaces the transient status line;
the existing `output_status` slot carries "Cleaned" / "Original restored".

The icon is added to the existing `icon_assets!` list in `lib.rs`. `WandSparkles`
maps to Lucide `wand-sparkles`; the macro validates the variant at compile time,
so a wrong name is a compile error, not a blank icon — if it does not exist, use
`Sparkles` and say so in the Implementation Notes.

`views/dialogs/settings.rs` — one more `SettingRow`, below the two model rows:

- **Cleanup Model** — *"Repairs typos and broken sentences after transcription"*,
  control: `ModelSelect` with id `field-settings-cleanup`, listing the worker's
  discovered language models.
- When the worker reports no language models, the row shows a `Label` with the
  resolved default id instead of a menu, so the setting still reads as
  configured.
- Placeholder: `"Use DEFAULT_LANGUAGE_MODEL"` while the stored field is empty.

`components/model_select.rs` adds the third state to `SettingsSelectFields`
(`cleanup`, one `SearchableVec<ModelOption>` with `mode: None`) and the matching
`SettingsSnapshot.cleanup_models` / `selected_cleanup_model` fields, so the
existing sync rule applies unchanged.

## TDD Tasks

- [ ] **T30.1 — Red: protocol.** Failing tests in `protocol.rs` for the
  `CleanTranscript` round-trip, the `Cleaned` event parse, `Error` with and
  without `request_id` (absent key stays absent), and a `Capabilities` payload
  with and without the two new fields. Then implement.
- [ ] **T30.2 — Red: settings.** Failing tests for `language_model` (default
  empty, JSON round-trip, `Debug` shows it, `validate()` does not require it).
  Then implement.
- [ ] **T30.3 — Red: worker prompt.** Failing `bun test` cases in `clean.spec.ts`
  with a stub `generate`:
  - the prompt carries the instructions and the transcript verbatim, and the
    transcript appears exactly once;
  - the prompt names all four defect categories (artefacts, fragment joins,
    hesitations, the speaker's own repetitions) and the fidelity rules (no added
    content, no summary, no style rewrite, keep the language);
  - the returned text is trimmed;
  - an empty answer returns the input unchanged;
  - a clean input comes back unchanged when the stub echoes the transcript
    (the idempotence clause);
  - `generate` is called with the documented `temperature` parameter.

  Then implement `clean.ts`.
- [ ] **T30.4 — Red: worker routing.** Failing `worker.spec.ts` cases: a
  `cleanTranscript` line emits `cleaned` with the same `requestId`; a missing key
  emits `missing_api_key` with that `requestId`; a thrown error emits
  `cleanup_failed` and never leaks the transcript in `message`; cleanup does not
  disturb an active session. Then route it in `worker.ts`.
- [ ] **T30.5 — Red: discovery and defaults.** Failing cases: `parseModels`
  yields language models from `type: "language"` entries while transcription
  models keep their modes; `defaultLanguageModel` prefers
  `DEFAULT_LANGUAGE_MODEL` then the fallback; `listCapabilities` reports both new
  fields. Then implement.
- [ ] **T30.6 — Red: controller.** Failing unit tests for `clean_status`
  transitions (unavailable → ready → running → cleaned → ready after undo), the
  stale-request rule, `undo` restoring the original pair, `new_take` clearing the
  state, and Copy/Save/auto-paste following the cleaned text. Then implement.
- [ ] **T30.7 — Green: app wiring and UI.** Add `clean_transcript`, route
  `Cleaned`/`Error` in the pump, render the wand + undo in `session_end.rs`, add
  the Settings row and the third select state, add the icon. Make a headless
  `widget.rs` test click `btn-clean-transcript`, feed `Cleaned`, and assert the
  displayed text, the Copy payload and the undo behaviour.
- [ ] **T30.8 — Docs and design.** `DESIGN.md`: the footer action order and the
  new Settings row; module headers and item docs per `AGENTS.md` §3 (callers
  named, present tense, no ticket ids).
- [ ] **T30.9 — Gates.** `cargo fmt -- --check`,
  `cargo clippy --all-targets -- -D warnings`, `cargo test`, `bun test`,
  `bun check src/stt`, `bun run design:validate`. Record exact counts in
  `## Implementation Notes`.

## Acceptance Criteria

- The finished-take footer shows a magic-wand action immediately left of Copy
  and Save; it is disabled on an empty take and when no cleanup model resolves.
- Clicking it sends the displayed transcript to the worker, shows a running
  state, then replaces the transcript with the repaired text.

  The repaired text removes transcription artefacts (glued or split words,
  stray spaces around apostrophes), duplicated fragments at window joins,
  hesitations and accidental repetitions, while keeping the speaker's words,
  order and register: no content added, no summary, no stylistic rewrite, and
  the original language preserved. An already-clean transcript comes back
  identical.
- A second action restores the original text exactly, and Copy, Save and the
  automatic paste all use whichever text is on screen.
- A failed cleanup leaves the original transcript untouched and surfaces a typed
  error; the transcript never appears in any error message or log.
- Settings has a **Cleanup Model** row listing the discovered language models,
  seeded from `DEFAULT_LANGUAGE_MODEL` when the stored value is empty.
- `DEFAULT_LANGUAGE_MODEL` alone is enough to enable the feature with no settings
  change.
- No view picks a colour, radius, spacing or type size; the wand is an existing
  component with an existing icon size.
- All six gates are green with zero warnings and no skipped tests.
- Final visual review is left to the user.
