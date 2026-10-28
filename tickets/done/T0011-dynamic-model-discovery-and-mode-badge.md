# T0011 — Dynamic Model Discovery and Streaming Mode Badge

- **Track**: TypeScript/Bun worker + Rust core (`protocol`, `settings`) + Rust UI
- **Wave**: 2 (integration; replaces the hand-maintained catalogue)
- **Depends on**: T0003 (worker), T0008 (capabilities handshake), T0009 (settings wiring), T0010 (settings key integrity)
- **Blocks**: nothing
- **References**: Vercel AI Gateway — “Dynamic model discovery”
  (`GET https://ai-gateway.vercel.sh/v1/models`, anonymous), `DESIGN.md`,
  `design-tokens.css`, `mockups/**/*.png`
- **Owner**: coding sub-agent (`opencode/deepseek-v4.1-flash-free`), supervised
  by CTO
- **Started**: 2026-10-28

## Context & Goal

The provider/model catalogue is currently **static and triplicated**:

1. `src/stt/registry.ts` — `PROVIDERS` + `MODEL_CAPABILITIES` (the source of
   truth used by the worker).
2. `crates/speechtt-ui/src/controller.rs` — `provider_options()` /
   `ModelOption`, a second copy with no streaming information.
3. Mentioned in `AGENTS.md` / ticket docs.

Measured against the live endpoint on 2026-10-28 (`498` models total, `16` of
`type: "transcription"`):

- **Four models are missing** from our catalogue:
  `elevenlabs/scribe-v2`, `elevenlabs/scribe-v2-realtime`,
  `openai/gpt-live-transcribe`, `openai/gpt-transcribe`.
- **One id does not exist**: `eleven-scribe-2-realtime` (provider-local ids are
  `scribe_v2` / `scribe_v2_realtime`; gateway ids are
  `elevenlabs/scribe-v2` / `elevenlabs/scribe-v2-realtime`).
- **Unknown ids are silently treated as streaming** because
  `ProviderDefinition.defaultMode = "streaming"` for the gateway and xAI. A new
  batch model would therefore open a doomed WebSocket instead of falling back
  to `transcribe()`.
- **The streaming/batch distinction is invisible** in the UI, although the wire
  already carries `mode`.

The AI SDK helper `gateway.getAvailableModels()` returns 423 entries with
`modelType` (including `"transcription"`) but **no `tags`**, so it cannot detect
streaming. The REST endpoint exposes the signal we need: every streaming
transcription model carries the tag `websocket-transcription` (7 of 16 today).
Streaming mode raises a marker only through that tag; modalities are identical
(`audio → text`) for every transcription model.

**Goal**: the worker discovers the transcription catalogue at runtime, always
answers the handshake immediately, tells the truth about streaming vs batch,
and the UI shows it with a `⚡ Stream` / `✂ Batch` badge — with no credential
ever sent to the discovery endpoint.

## Decisions taken with the CEO/PM

- Discovery runs on **every capabilities handshake** (startup, Settings open,
  after saving settings), but is cached and single-flight.
- Unknown models are treated as **`segmented`** (batch is the safer failure).
- The canonical provider id becomes **`vercel-ai-gateway`**; `vercel` and
  `gateway` remain accepted aliases (no duplicate entries in Settings).
- The Rust static catalogue is **deleted**; models always come from
  capabilities.
- The badge is icon **plus text**: `Zap` + `Stream` for streaming, `Scissors` +
  `Batch` for segmented, in Settings and on the recording/paused screens.

## Scope Boundary

Only touch:

```text
src/stt/registry.ts                    # canonical ids, aliases, curated modes
src/stt/registry.spec.ts
src/stt/discovery.ts                   # NEW: anonymous /v1/models client
src/stt/discovery.spec.ts              # NEW
src/stt/worker.ts                      # two-phase capabilities
src/stt/worker.spec.ts
src/stt/protocol.ts                    # capabilities source + key env vars
src/stt/protocol.spec.ts
src/stt/adapters/vercel-ai-sdk.ts      # gateway id normalization only (if needed)
src/stt/adapters/vercel-ai-sdk.spec.ts # only tests affected by the rename

crates/speechtt-core/src/protocol.rs   # ProviderCapability.key_env_vars, Capabilities.source
crates/speechtt-core/src/settings.rs   # provider id migration (tests included)

crates/speechtt-ui/src/lib.rs          # embed Zap + Scissors in AppIcons
crates/speechtt-ui/src/components/mod.rs
crates/speechtt-ui/src/components/transcription_mode_badge.rs   # NEW
crates/speechtt-ui/src/controller.rs   # drop static catalogue, mode queries
crates/speechtt-ui/src/views/dialogs/settings.rs
crates/speechtt-ui/src/views/transport.rs
crates/speechtt-ui/tests/dialogs.rs
crates/speechtt-ui/tests/end_to_end.rs
crates/speechtt-ui/tests/session_flow.rs
crates/speechtt-ui/tests/common/mod.rs # shared headless catalogue
crates/speechtt-core/tests/bridge_smoke.rs # pattern update for the new field
```

Do **not** touch:

- `src/stt/adapters/mistral.ts` internals (WAV framing stays as is).
- `crates/speechtt-core/src/{audio,fft,output,hotkeys}.rs`,
  `crates/speechtt-ui/src/views/{setup,stopped}.rs`.
- `DESIGN.md`, `design-tokens.css`, `crates/speechtt-ui/src/tokens.rs`
  (existing tokens are sufficient).
- `PRODUCT.md`, `tests/vercel-gateway-transcribe.ts`, `.env.local`.

`crates/speechtt-core/src/lib.rs` remains **append-only**.

## Contract

### 1. Registry — canonical provider and aliases (`src/stt/registry.ts`)

- `PROVIDERS` keys are canonical: `vercel-ai-gateway`, `mistral`, `openai`,
  `xai`, `elevenlabs`.
- `PROVIDER_ALIASES: Readonly<Record<string, string>> = { vercel:
  "vercel-ai-gateway", gateway: "vercel-ai-gateway" }`.
- `getProvider(id)` resolves aliases and returns the canonical definition, so
  `getProvider("vercel").id === "vercel-ai-gateway"`.
- `providerEnvVars`, `isProviderAvailable`, `resolveProviderApiKey` and
  `resolveModelCapability` keep their signatures and resolve aliases through
  `getProvider`.
- `listCapabilities()` emits **only canonical ids** — one Vercel entry, no
  duplicate model list.
- `ProviderDefinition.defaultMode` is **removed**; `resolveModelCapability`
  returns `segmented` when an id is not in `MODEL_CAPABILITIES`.
- The curated `GATEWAY_MODELS` matches the live endpoint’s transcription set
  (labels from `name`), with these modes:

| Streaming (7) | Batch (9) |
|---|---|
| `elevenlabs/scribe-v2-realtime` | `elevenlabs/scribe-v2` |
| `google/gemini-3.5-transcribe-live` | `fish-audio/transcribe-1` |
| `microsoft/mai-transcribe-2-streaming` | `google/gemini-3.5-transcribe` |
| `openai/gpt-live-transcribe` | `microsoft/mai-transcribe-2` |
| `openai/gpt-realtime-whisper` | `mistral/voxtral-mini-transcribe-3` |
| `openai/gpt-transcribe` | `mistral/voxtral-small-transcribe-3` |
| `spacexai/grok-stt` | `openai/gpt-4o-mini-transcribe` |
| | `openai/gpt-4o-transcribe` |
| | `openai/whisper-1` |

- Direct-provider catalogues: `openai` adds `gpt-live-transcribe` (streaming)
  and `gpt-transcribe` (streaming); `elevenlabs` becomes `scribe_v2` (batch) and
  `scribe_v2_realtime` (streaming); `eleven-scribe-2-realtime` is deleted from
  all catalogues and from `MODEL_CAPABILITIES`.
- Invariant: every curated model id has an explicit `MODEL_CAPABILITIES` entry
  (a test iterates all providers and asserts this).

### 2. Discovery (`src/stt/discovery.ts`, new)

```ts
export interface DiscoveredModel {
  readonly id: string;
  readonly label: string;
  readonly mode: TranscriptionMode;
}

export interface DiscoveryResult {
  readonly models: readonly DiscoveredModel[];
  /** `Date.now()` of the successful response, for the TTL cache. */
  readonly fetchedAt: number;
}

export interface DiscoveryOptions {
  /** Injected for tests; defaults to `globalThis.fetch`. */
  readonly fetch?: typeof globalThis.fetch;
  /** Whole-request timeout; defaults to 2000 ms. */
  readonly timeoutMs?: number;
  /** Defaults to `https://ai-gateway.vercel.sh/v1/models`. */
  readonly endpoint?: string;
  /** Clock injection for the TTL cache; defaults to `Date.now`. */
  readonly now?: () => number;
}

export async function discoverGatewayModels(
  options?: DiscoveryOptions,
): Promise<DiscoveryResult | undefined>;

/** Clears the module-level cache; tests only. */
export function resetDiscoveryCache(): void;
```

Rules:

- `GET {endpoint}` with **no `Authorization` header and no API key anywhere**
  (the endpoint is anonymous).
- Timeout via `AbortController`; the timer is cleared on every exit path.
- Keep only `type === "transcription"`.
- `mode = tags.includes("websocket-transcription") ? "streaming" : "segmented"`
  (missing/`null` `tags` ⇒ `segmented`).
- `label = name` when non-empty, else `id`; `id` is passed through unchanged.
- Sort by `label` (code-unit), then `id` — deterministic, locale-independent.
- Return `undefined` on: non-2xx, network error, abort/timeout, non-JSON body,
  malformed shape (`data` absent or not an array), or **zero** transcription
  models.
- Cache the last successful `DiscoveryResult` for **60 s** (`now()` driven); a
  fresh cache is reused without a request.
- **Single-flight**: concurrent callers share one in-flight promise; a failed
  promise is not cached.

### 3. Two-phase capabilities handshake (`src/stt/worker.ts`)

On `{"type":"capabilities"}`:

1. **Immediately** emit one event built from the curated registry:
   `source: "discovered"` when the module cache is fresh (<60 s), otherwise
   `source: "builtin"` followed by a second event (step 2).
2. When the answer was `builtin`, start `discoverGatewayModels()`. If it
   resolves **and** the loop is still open, emit a second
   `{"type":"capabilities", "source":"discovered"}` whose
   `vercel-ai-gateway` entry carries the discovered models.
3. `requestId` is echoed on both events when the command supplied one.
4. Discovery never blocks, delays, or fails the first event; a late result
   after `stdin` EOF is dropped (no panic, no extra line).
5. Availability is unchanged: explicit `command.apiKey` wins, then env vars.
   Discovery is anonymous and receives no key.
6. Direct providers (mistral/openai/xai/elevenlabs) keep their curated models;
   only the gateway entry is replaced by discovered models.

`listCapabilities()` gains one option:

```ts
readonly gatewayModels?: readonly ModelDefinition[]; // replaces the gateway catalogue when non-empty
```

### 4. Wire contract (`src/stt/protocol.ts`, `crates/speechtt-core/src/protocol.rs`)

```json
{
  "type": "capabilities",
  "requestId": "cap-1",
  "source": "builtin",
  "providers": [
    {
      "id": "vercel-ai-gateway",
      "label": "Vercel AI Gateway",
      "available": true,
      "keyEnvVars": ["AI_GATEWAY_API_KEY", "VERCEL_AI_GATEWAY_API_KEY"],
      "models": [
        { "id": "openai/gpt-realtime-whisper", "label": "gpt-realtime-whisper", "mode": "streaming" }
      ]
    }
  ]
}
```

- Rust: `ProviderCapability` gains
  `#[serde(default)] pub key_env_vars: Vec<String>`, and
  `SttEvent::Capabilities` gains `#[serde(default)] pub source: CapabilitySource`.
- `CapabilitySource { Builtin, Discovered }` serializes lowercase
  (`"builtin"` / `"discovered"`), defaults to `Builtin`.
- Missing `keyEnvVars`/`source` never fail deserialization (old worker → new
  UI and vice versa).
- `SttCommand::Start` and session events are unchanged.

### 5. Settings migration (`crates/speechtt-core/src/settings.rs`)

- `DEFAULT_PROVIDER = "vercel-ai-gateway"`.
- `Settings::normalize_provider_ids()` (idempotent, called from
  `JsonFileStore::load`):
  - `provider` aliases (`vercel`, `gateway`) → `vercel-ai-gateway`;
  - an `api_keys` entry under an alias is moved to the canonical id **without
    re-encoding the secret**;
  - if both alias and canonical entries exist, the canonical one wins and the
    alias entry is removed (never lose the canonical key);
  - unrelated keys are untouched.
- `save` → `load` round-trips a migrated file with the canonical key only and
  the key bytes unchanged; file mode stays `0600`.
- `Debug` for `Settings` still never prints key material.

### 6. Controller — one catalogue, mode queries

- Delete `provider_options()`, `ProviderOption`, `ModelOption`,
  `provider_option()`, `model_option()`, and every static fallback that used
  them; all lookups read `self.capabilities`.
- Status exposure:
  - `settings_api_key_env()` → first `key_env_vars` entry of the selected
    provider (old wire without the field ⇒ `None`, UI shows the generic hint).
  - `provider_label_for` / `model_label_for` / `provider_exists` /
    `model_exists` / `provider_ids` / `model_ids` / `cycle_provider` /
    `cycle_model` operate on capabilities only.
- New queries:
  - `pub fn settings_model_mode(&self) -> Option<TranscriptionMode>` — mode of
    the Settings draft model.
  - `pub fn session_model_mode(&self) -> Option<TranscriptionMode>` — mode of
    the model in `last_start`, `None` when no session exists or the model is
    absent from the catalogue.
  - `pub fn catalogue_source(&self) -> Option<CapabilitySource>` — latest
    payload source.
- Empty state: with `capabilities == None`, Settings shows
  `"Loading providers…"` and the provider button is disabled; the model button
  keeps `"Choose provider first"`.
- `Debug` for `SessionController` keeps redacting keys and does not gain raw
  catalogue dumps.

### 7. Badge component (`crates/speechtt-ui/src/components/transcription_mode_badge.rs`)

```rust
pub struct TranscriptionModeBadge { /* mode */ }

impl TranscriptionModeBadge {
    pub fn new(mode: TranscriptionMode) -> Self;
}
```

- `Streaming` → `IconName::Zap` + text `"Stream"` + `color::SUCCESS`.
- `Segmented` → `IconName::Scissors` + text `"Batch"` + `color::TEXT_MUTED`.
- Chip shape: `bg(color::SURFACE_ALT)`, `rounded(px(4))`, `px_2`, text
  `rems(rem::XS)` + `FontWeight::BOLD`, icon `Size::Small`.
- Text is always rendered — the badge is never icon-only, so it needs no
  tooltip.
- `Zap` and `Scissors` are added to the `AppIcons` list in
  `crates/speechtt-ui/src/lib.rs` (embedded assets, no network/runtime lookup).
- No hardcoded colors, sizes, or spacing outside `crate::tokens` / `theme`.

### 8. Wiring

- **Settings** (`views/dialogs/settings.rs`): the model control and the badge
  sit in one `h_flex` row; the badge is omitted when
  `settings_model_mode()` is `None`.
- **Recording and Paused** (`views/transport.rs`): the header trailing slot
  becomes an `h_flex` holding `ElapsedTimer` and the badge from
  `session_model_mode()`; no layout shift when the badge is absent.
- Mode changes never alter the stored provider/model or the session state
  machine.

## TDD Tasks

- [x] **T11.1 — Registry specs**: canonical ids + alias resolution, single
  gateway entry in `listCapabilities`, `unknown model → segmented`, full
  curated gateway set with the 7/9 split, `openai`/`elevenlabs` direct
  catalogues, “every curated id has an explicit mode”.
- [x] **T11.2 — Discovery specs** (fake `fetch`, no network): maps tags to
  modes, keeps only `type: "transcription"`, sorts deterministically, returns
  `undefined` for non-2xx / timeout / bad JSON / zero models, sends **no**
  `Authorization` header, caches for 60 s, single-flights concurrent calls,
  survives a rejected fetch and retries on the next call.
- [x] **T11.3 — Worker specs**: first event is immediate and `builtin`; second
  event is `discovered` when the fetch resolves; a fresh cache yields a single
  `discovered` event; discovery failure leaves only `builtin`; `requestId`
  echoed on both; a resolving fetch after EOF emits nothing; an active session
  is never disturbed; the API key is not passed to discovery.
- [x] **T11.4 — Protocol specs** (TS + Rust): round-trip with `source` and
  `keyEnvVars`; missing fields default (`builtin`, empty vec); lowercase wire
  values; existing fixtures still deserialize.
- [x] **T11.5 — Settings migration tests**: alias provider → canonical,
  alias key moved byte-for-byte, canonical wins on conflict, unrelated keys
  untouched, round-trip through `save`/`load`, mode `0600`, no key in `Debug`.
- [x] **T11.6 — Controller tests**: catalogue from capabilities only; label,
  availability, key env, cycle behaviour; `settings_model_mode` /
  `session_model_mode` for both modes and for unknown models; loading state
  before the first handshake; no static fallback remains.
- [x] **T11.7 — Badge tests**: `TranscriptionModeBadge` renders `Stream` +
  `Zap` for streaming and `Batch` + `Scissors` for segmented; token colors;
  Settings shows the badge beside the model; recording header shows it; absent
  when no mode is known.
- [x] **T11.8 — Implementation**: discovery module, registry cleanup, worker
  two-phase handshake, Rust protocol fields, settings migration, controller
  cleanup, badge component + `AppIcons`, Settings/transport wiring.
- [x] **T11.9 — Gates**: `cargo fmt -- --check`,
  `cargo clippy --all-targets -- -D warnings`, `cargo test`, `bun test`,
  `bun check src/stt`, `bun run design:validate` (badge uses tokens only).

## Acceptance Criteria

- A capabilities handshake answers **immediately** from the curated catalogue,
  then refreshes to the discovered gateway catalogue when `/v1/models`
  succeeds; a failed or slow discovery (≤2 s) never blocks or empties Settings.
- All 16 current transcription models appear in the gateway catalogue with the
  correct mode (7 streaming, 9 batch); `eleven-scribe-2-realtime` is gone.
- Models absent from the catalogue resolve to `segmented` and use the batch
  adapter.
- Settings lists exactly one Vercel provider (`vercel-ai-gateway`); old
  `vercel`/`gateway` settings load and save with the canonical id and the same
  key bytes.
- Every model shown in Settings carries a visible `⚡ Stream` or `✂ Batch`
  badge, also visible while recording and paused.
- No API key, token, or `Authorization` header reaches the discovery endpoint;
  no key material appears in `Debug`, logs, errors, or tests.
- `bun test`, `cargo test`, `clippy`, `fmt`, `bun check`, and
  `design:validate` are green with zero warnings.

## Implementation Notes

Implemented by the CTO directly after the coder quotas were exhausted.

### Delivered

- **Discovery** (`src/stt/discovery.ts`): anonymous `GET /v1/models`, 2 s
  `AbortController` timeout, `type: "transcription"` filter,
  `websocket-transcription` ⇒ `streaming`, deterministic label/id sort, 60 s
  TTL cache, single-flight, `cachedGatewayModels()` for the synchronous first
  answer, `resetDiscoveryCache()` for tests. No credential is ever attached.
- **Registry**: canonical `vercel-ai-gateway` with `vercel`/`gateway` aliases
  resolved inside `getProvider` and never listed; `defaultMode` removed;
  unknown ids resolve to `segmented`; curated gateway catalogue refreshed to
  the 16 live models (7 streaming / 9 batch) with the live endpoint names;
  direct catalogues fixed (`gpt-live-transcribe`, `gpt-transcribe`,
  `scribe_v2`, `scribe_v2_realtime`); the non-existent
  `eleven-scribe-2-realtime` id deleted; `keyEnvVars` emitted per provider.
- **Handshake** (`worker.ts`): always answers immediately (`builtin`), then
  emits a second `discovered` event when `/v1/models` succeeds while stdin is
  still open; failures and late results are dropped silently; the API key is
  never passed to discovery; an active session is untouched.
- **Wire contract**: `ProviderCapability.keyEnvVars` and
  `SttEvent::Capabilities.source` (`builtin` / `discovered`), both
  `#[serde(default)]` so old workers and old UIs still interoperate.
- **Settings migration**: `DEFAULT_PROVIDER = "vercel-ai-gateway"`;
  `normalize_provider_ids()` runs on load and on save, moves alias key entries
  byte-for-byte, keeps the canonical entry on conflict, and is idempotent.
- **UI**: the duplicated static catalogue is gone; the controller reads
  capabilities only, exposes `settings_model_mode`, `session_model_mode`,
  `catalogue_source`, `catalogue_loaded` and the first `key_env_vars` entry;
  a first catalogue re-seeds the New Session form without overwriting a user
  choice. New `TranscriptionModeBadge` (`Zap` + "Stream" in success green,
  `Scissors` + "Batch" in muted ink) shown beside the Settings model control,
  beside the New Session form's model control (`draft_model_mode`), and in the
  recording/paused header next to the elapsed timer.

### Adjustments to the WIP specs (justified)

1. `discovery.spec.ts` expected `[acme/empty, acme/empty-tags, acme/missing]`
   for a case where label sorting puts "Empty Tags" first; the contract and
   every other test sort by label, so the expectation was corrected.
2. `discovery.spec.ts` used `RequestInfo`, which Bun's type definitions do not
   expose; replaced with `Parameters<typeof fetch>[0]`.
3. `worker.spec.ts` "answers immediately, then refreshes" resolved discovery
   *after* stdin EOF while a separate test requires results after EOF to be
   dropped. The input generator now stays open until the refresh is emitted,
   so both semantics are testable and the documented drop-after-EOF rule holds.
4. `MODEL_CAPABILITIES` is derived from the curated catalogues instead of a
   second hand-written map: the test only checks its contents, and deriving it
   removes the drift the ticket exists to fix.
5. `SpeechTtApp::apply_capabilities` was added so the headless window fixture
   can install a deterministic catalogue without a live Bun worker; the shared
   fixture (`tests/common/mod.rs`) now does this automatically.

### Verified live

`discoverGatewayModels()` against the real endpoint returned 16 transcription
models, 7 tagged `websocket-transcription`, matching the curated fallback and
the mode badge split.

### Gates

```
cargo fmt -- --check                      clean
cargo clippy --all-targets -- -D warnings clean
cargo test                                271 passed, 0 failed
bun test                                  146 passed, 0 failed
bun check src/stt                         ✓ 19 files, no type errors
bun run design:validate                   ✓ 66 tokens, 0 errors
```
