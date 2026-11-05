# T0030 — Clean the transcript with a language model, in a chosen language

- **Track**: Cross-cutting (Rust core protocol + Bun worker + Rust UI + design system)
- **Wave**: 1 (independent)
- **Depends on**: nothing (uses the T0028 dropdowns and the T0029 settings plumbing)
- **Blocks**: nothing
- **References**:
  - `AGENTS.md` §2 (component/view rules), §5 (gates), §6 (official documentation), §9 (secret safety)
  - `DESIGN.md` § "Action buttons", § "Icon buttons", § "Screen footer", § "Settings panel"
  - Vercel AI SDK `generateText`: <https://ai-sdk.dev/docs/reference/ai-sdk-core/generate-text.md>
  - Vercel AI SDK `transcribe`: <https://ai-sdk.dev/docs/reference/ai-sdk-core/transcribe.md>
  - Vercel AI SDK streaming transcription (`experimental_streamTranscribe`):
    <https://ai-sdk.dev/llms.txt> § "Streaming Transcription"
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

The same dictation also loses quality for a duller reason: the transcriber is
never told which language is spoken. Every model guesses, and a guess made from
two seconds of audio is worse than a declared language — accents, homophones and
proper nouns all suffer. This ticket therefore adds one **dictation language**
setting, applied to the transcription itself and reused by the cleanup pass.

Done means: one click cleans the transcript through the Bun worker, the cleaned
text is what the user sees and copies, a second click restores the original, the
cleanup model is configurable in Settings with `DEFAULT_LANGUAGE_MODEL` as its
built-in default, and the dictation language is chosen once in Settings and
forwarded to both the transcriber and the cleanup model.

## Scope Boundary

Allowed to create or modify:

- `crates/speechtt-core/src/protocol.rs` (new command, new event, error correlation)
- `crates/speechtt-core/src/settings.rs` (`language_model`, `language`)
- `src/stt/protocol.ts` (mirror of the Rust protocol)
- `src/stt/clean.ts` + `src/stt/clean.spec.ts` (new: prompt and call)
- `src/stt/worker.ts` + `src/stt/worker.spec.ts` (route `cleanTranscript`)
- `src/stt/discovery.ts` + `src/stt/discovery.spec.ts` (language models)
- `src/stt/registry.ts` + `src/stt/registry.spec.ts` (`DEFAULT_LANGUAGE_MODEL`, capabilities payload)
- `src/stt/adapters/vercel-ai-sdk.ts` + `.spec.ts` (forward the dictation language)
- `src/stt/adapters/mistral.ts` + `.spec.ts` (forward the dictation language)
- `src/stt/adapters/types.ts` (`language` on `SttSessionConfig`)
- `src/stt/languages.ts` + `src/stt/languages.spec.ts` (new: the offered language list)
- `crates/speechtt-ui/src/controller.rs`
- `crates/speechtt-ui/src/app.rs`
- `crates/speechtt-ui/src/components/model_select.rs`
- `crates/speechtt-ui/src/components/language_select.rs` (new)
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
- `src/stt/stream-controller.ts` and `src/stt/segmentation.ts` (segmentation policy is unchanged)
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
    /// ISO-639-1 language of the transcript, e.g. `fr`. `None` means "detect
    /// it yourself"; the prompt then omits the language clause instead of
    /// guessing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    language: Option<String>,
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

### 1b. Wire protocol: the dictation language on transcription commands

`Start` and `TranscribeFile` each gain the same optional field, so a persisted
choice reaches the transcriber without a new command:

```rust
Start {
    // …existing fields…
    /// ISO-639-1 language of the dictation, e.g. `fr`. `None` leaves detection
    /// to the provider.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    language: Option<String>,
},

TranscribeFile {
    // …existing fields…
    #[serde(default, skip_serializing_if = "Option::is_none")]
    language: Option<String>,
},
```

Absent stays absent on the wire: an older worker keeps parsing the command, and
a take with no language set produces exactly today's payload. The TypeScript
mirror adds `language?: string` to both command interfaces and keeps the
validator's rule that an unknown extra key is rejected — `language` becomes a
known key, so it is validated as "when present, a non-empty string".

### 1c. Worker: how the language reaches a model

The AI SDK exposes **no** top-level `language` on `transcribe` or
`experimental_streamTranscribe`; the documented path is a provider-specific
option:

```ts
// https://ai-sdk.dev/docs/reference/ai-sdk-core/transcribe.md (providerOptions)
// https://ai-sdk.dev/llms.txt § "Streaming Transcription"
experimental_streamTranscribe({
  model,
  audio,
  inputAudioFormat: { type: "audio/pcm", rate },
  providerOptions: { openai: { language: "fr" } },
});
```

Support therefore varies per adapter, and the SDK reports an unsupported option
through `result.warnings`. The worker's rule is:

- `SttSessionConfig` gains `readonly language?: string`, filled from the command
  by `sessionConfigFromStart`.
- `src/stt/languages.ts` exports one helper used by both adapters:

  ```ts
  /** `providerOptions` carrying the dictation language, or `undefined` when no
   * language is set. The inner key is the provider package name the resolved
   * model belongs to (`openai`, `mistral`, `xai`, `elevenlabs`, `gateway`). */
  export function languageProviderOptions(
    provider: string,
    language: string | undefined,
  ): Record<string, { language: string }> | undefined;
  ```

- `VercelAiSdkAdapter` passes the result as `providerOptions` on
  `streamTranscribe`; `MistralAdapter` passes it on `transcribe`. Both merge it
  with any existing provider options rather than replacing them.
- A provider that rejects the option raises a `warnings` entry the adapter
  already ignores, or fails the call; a failure surfaces as today's typed
  `stream_failed` with a message that names the language, never the transcript.
  Best effort is the contract: the option is forwarded, and a provider that does
  not implement it still transcribes.

### 2. Worker: the cleanup call — `src/stt/clean.ts`

```ts
/** Default model when neither settings nor the environment names one. */
export const FALLBACK_LANGUAGE_MODEL = "openai/gpt-4.1-mini";
/** Environment variable naming the default cleanup model. */
export const LANGUAGE_MODEL_ENV = "DEFAULT_LANGUAGE_MODEL";
/** Low enough that the pass repairs rather than improvises. */
export const CLEANUP_TEMPERATURE = 0.1;

export interface CleanRequest {
  readonly provider: string;
  readonly model: string;
  readonly text: string;
  readonly apiKey?: string;
}

export interface CleanDeps {
  /** Injected for tests; production passes the AI SDK `generateText`. */
  generate(args: {
    model: string;
    prompt: string;
    temperature: number;
  }): Promise<{ text: string }>;
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

When the request carries a language, the instructions name it explicitly ("the
transcript is in French") and add one instruction: keep that language, use its
typography (French spacing before `:` `;` `!` `?` and inside « », apostrophes as
`’`), and never translate. When the request carries none, the clause is omitted
and the model follows the transcript's own language — it never guesses a
language from nothing and never switches.

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

const { text } = await generateText({
  model,
  prompt,
  temperature: CLEANUP_TEMPERATURE, // exported constant, 0.1
});
```

`model` is the gateway id string, exactly as the transcription path already
passes model ids to the AI SDK. The injected `generate` seam takes the same two
values plus the temperature the production call uses, so a test can assert it
was passed without depending on the AI SDK itself.

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

### 4. Settings: two persisted fields, one seeded default

```rust
// crates/speechtt-core/src/settings.rs
/// Cleanup model id. Empty means "use `DEFAULT_LANGUAGE_MODEL`, else the
/// worker's fallback".
pub language_model: String,
/// ISO-639-1 code of the dictation language, e.g. `fr`. Empty means
/// "let the provider detect it".
pub language: String,
```

- Both default to empty; `Debug` prints them (neither is a secret).
- Neither is part of `validate()`: a take must remain startable with no cleanup
  model and no declared language. The wand is disabled without a model, and the
  transcriber simply keeps its detection behaviour without a language.
- `SettingsChange` and `SettingsDraft` carry both. The Settings dialog shows the
  value the worker reported as `default_language_model` when the stored model is
  empty, so the user sees an actual model id rather than a blank row.
- `Settings::language()` normalizes the stored value: trimmed and lowercased;
  anything that is not a two-letter ISO-639-1 code is treated as empty, so a
  corrupt file cannot send an arbitrary string to a provider.

### 4b. The offered languages — `src/stt/languages.ts`

The list is data, not a UI decision, so it lives beside the registry and the UI
reuses it:

```ts
/** One selectable dictation language. */
export interface LanguageOption {
  /** ISO-639-1 code, e.g. `fr`. */
  readonly code: string;
  /** English name shown in the menu, e.g. `French`. */
  readonly label: string;
}

/** Languages the picker offers, sorted by label. Empty `code` is not in this
 * list: "Auto" is a UI entry the Rust side represents as an empty setting. */
export const LANGUAGE_OPTIONS: readonly LanguageOption[];
```

`LANGUAGE_OPTIONS` covers the languages the supported providers publish support
for, and always contains at least English, French, German, Spanish, Italian,
Portuguese, Dutch, Polish, Russian, Chinese, Japanese and Korean. The list is
the single authority: the Rust picker is built from it through the capabilities
handshake (`languages` field below), so adding a language never means editing
Rust.

The capabilities event therefore gains one more optional field, next to the two
cleanup ones:

```rust
Capabilities {
    // …existing fields…
    /// Offered dictation languages; `None` when an older worker does not
    /// report them, in which case the UI offers the built-in default list.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    languages: Option<Vec<LanguageOption>>,
}

pub struct LanguageOption { pub code: String, pub label: String }
```

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

### 7b. UI: the dictation language row

One more `SettingRow`, **above** the two model rows because it applies to the
transcription itself:

- **Dictation Language** — *"The language you speak; improves accuracy"*, control:
  a `LanguageSelect` component (same shape as `ProviderSelect`, non-searchable,
  ids `field-settings-language`).
- Its first entry is the **Auto** option: label `"Auto (detect)"`, value the
  empty string. The remaining entries come from the capabilities `languages`
  field, or from a built-in fallback list when an older worker reports none.
- The selected `Auto` entry maps to `Settings::language == ""`, so an unset
  language renders as Auto rather than as a blank field.

`components/language_select.rs` (new, exported from `components/mod.rs`) owns the
`SelectState<SearchableVec<LanguageItem>>` plus the `Auto` entry, so no view
builds it. `SettingsSelectFields` gains a fourth state for it, and
`SettingsSnapshot` gains `languages` / `selected_language`.

`views/session_setup.rs` stays untouched: the language is a Settings-level
choice, not a per-take field.

## TDD Tasks

- [ ] **T30.1 — Red: protocol.** Failing tests in `protocol.rs` for the
  `CleanTranscript` round-trip (with and without `language`), the `Cleaned` event
  parse, `Error` with and without `request_id` (absent key stays absent), the
  `language` field on `Start` and `TranscribeFile` (absent key stays absent), and
  a `Capabilities` payload with and without the three new fields (`languages`,
  `language_models`, `default_language_model`). Then implement.
- [ ] **T30.2 — Red: settings.** Failing tests for `language_model` and
  `language` (defaults empty, JSON round-trip, `Debug` shows both,
  `validate()` requires neither, `Settings::language()` lowercases and trims and
  rejects a non-ISO-639-1 value). Then implement.
- [ ] **T30.2b — Red: languages list.** Failing `languages.spec.ts` cases:
  `LANGUAGE_OPTIONS` codes are unique, lowercase and two letters; labels are
  non-empty and unique; the required core languages are present; the list is
  sorted by label. Then implement `languages.ts`.
- [ ] **T30.2c — Red: language forwarding.** Failing adapter specs: a session
  with a language passes `providerOptions.<provider>.language` to
  `streamTranscribe` (streaming) and to `transcribe` (batch); a session without
  one passes no `providerOptions`; an existing `providerOptions` is merged, not
  replaced; `sessionConfigFromStart` copies the command's language. Then
  implement `languageProviderOptions` and wire both adapters.
- [ ] **T30.2d — Red: capabilities and start payload.** Failing `registry.spec.ts`
  cases: `listCapabilities` reports `languages`; a `start` built from settings
  with a language carries it, and one without omits the key. Then implement.
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
  - a declared language puts the language name and its typography rules in the
    prompt; no declared language omits that clause entirely;
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
  the two Settings rows (Cleanup Model, Dictation Language), the third and fourth
  select states, `components/language_select.rs`, the icon, and the language
  forwarding from `start_configuration` into `SttCommand::Start` /
  `TranscribeFile`. Make headless `widget.rs` tests:
  - click `btn-clean-transcript`, feed `Cleaned`, assert the displayed text, the
    Copy payload and the undo behaviour;
  - choose a language in Settings, save, start a take, assert the sent `Start`
    carries `language: Some("fr")`, and that Auto sends `None`.
- [ ] **T30.8 — Docs and design.** `DESIGN.md`: the footer action order, the
  `Dictation Language` row (Auto first) and the `Cleanup Model` row; module
  headers and item docs per `AGENTS.md` §3 (callers named, present tense, no
  ticket ids).
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
- Settings has a **Dictation Language** row whose first entry is **Auto (detect)**;
  choosing a language forwards its ISO-639-1 code to the transcription command
  and to the cleanup request, and choosing Auto sends no language at all.
- A take started after choosing French sends `"language": "fr"` on `start` (and
  on `transcribeFile`), and the adapters forward it inside `providerOptions`;
  a take with Auto has no `language` key on the wire.
- The cleanup prompt names the declared language and its typography, and omits
  that clause when Auto is selected.
- No view picks a colour, radius, spacing or type size; the wand is an existing
  component with an existing icon size.
- All six gates are green with zero warnings and no skipped tests.
- Final visual review is left to the user.
