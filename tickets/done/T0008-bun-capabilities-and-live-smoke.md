# T0008 — Bun: Capability Handshake and Live Cross-Language Smoke Test

- **Track**: TypeScript/Bun worker (`src/stt/`) + Rust protocol types
- **Wave**: 1 (independent)
- **Depends on**: T0003 (worker + registry), T0002 (Rust NDJSON bridge)
- **Blocks**: T0009 (settings dropdowns and provider selection)
- **PRODUCT.md**: Phase 6 (capability detection) and Phase 8 (integration proof)
- **Owner**: coding sub-agent (deepseek-v4.1-flash), supervised by CTO
- **Started**: 2026-10-27

## Context & Goal

The UI currently ships a hard-coded provider/model catalogue and the Bun
registry can already answer "is this provider configured?", but nothing
connects the two. This ticket adds a **`capabilities` handshake** so the Rust
app can populate the Settings dialog from the worker's real registry, plus a
**live cross-language smoke test** proving `BunSubprocess` ↔
`src/stt/worker.ts` interoperate without a real STT provider.

## Scope Boundary

Only touch:

```text
src/stt/protocol.ts + src/stt/protocol.spec.ts
src/stt/registry.ts + src/stt/registry.spec.ts
src/stt/worker.ts + src/stt/worker.spec.ts
crates/speechtt-core/src/protocol.rs        # add 2 variants
crates/speechtt-core/src/backend.rs         # small capability helper (optional)
crates/speechtt-core/tests/bridge_smoke.rs  # new
Cargo.toml / crates/speechtt-core/Cargo.toml # dev-deps only, append-only
```

Do **not** touch `crates/speechtt-ui/`, `src/stt/adapters/*` internals beyond
reading their mode metadata, or the settings store.

## 1. Protocol extension (both languages, same bytes)

### Inbound (`Rust → Bun`)

```jsonc
{ "type": "capabilities", "requestId": "cap-1" }
```

- `requestId` is optional; when present it is echoed back so multiple
  requests can be correlated.

### Outbound (`Bun → Rust`)

```jsonc
{
  "type": "capabilities",
  "requestId": "cap-1",
  "providers": [
    {
      "id": "vercel",
      "label": "Vercel AI Gateway",
      "available": true,
      "models": [
        { "id": "openai/gpt-realtime-whisper", "label": "GPT Realtime Whisper", "mode": "streaming" },
        { "id": "openai/gpt-4o-transcribe", "label": "GPT-4o Transcribe", "mode": "segmented" }
      ]
    }
  ]
}
```

Rules:

- `available` is computed with the existing `isProviderAvailable(providerId,
  apiKey?)` logic against `process.env` **and** any key supplied in the
  request (`{ "type": "capabilities", "requestId": "...", "apiKey": "..." }`
  — needed for keys entered but not yet exported). The key is never echoed
  and never logged.
- Models are always listed (availability is a provider-level flag) so the UI
  can explain "add an API key" instead of hiding choices.
- `mode` is the existing `TranscriptionMode` (`"streaming" | "segmented"`).
- Deterministic ordering: providers sorted by `label`, models sorted by
  `label`, so snapshot tests are stable.
- The handshake is **read-only**: it must not create a session or disturb an
  active one.

### Rust side (`protocol.rs`)

Add to the `#[non_exhaustive]` enums:

```rust
SttCommand::Capabilities { request_id: Option<String>, api_key: Option<SecretString> }
SttEvent::Capabilities {
    request_id: Option<String>,
    providers: Vec<ProviderCapability>,
}

pub struct ProviderCapability {
    pub id: String,
    pub label: String,
    pub available: bool,
    pub models: Vec<ModelCapabilityInfo>,
}

pub struct ModelCapabilityInfo {
    pub id: String,
    pub label: String,
    pub mode: TranscriptionMode, // Streaming | Segmented
}
```

- All fields camelCase on the wire (`requestId`, `apiKey`,
  `fullText`-style consistency).
- `Debug` for the command redacts `api_key`; `secret_string_never_exposes…`
  style test must cover the new variant.
- `SttEvent::session_id()` returns `None` for capabilities.

## 2. Worker integration (`src/stt/`)

- `protocol.ts`: parse/validate `capabilities` with an optional string
  `requestId`, optional string `apiKey`; reject wrong types with
  `invalid_protocol` exactly like existing commands.
- `registry.ts`: export `listCapabilities({ apiKeyFor }?)` (or an equivalent
  pure function) returning the deterministic structure above. Reuse
  `PROVIDERS`, `MODEL_CAPABILITIES`, `isProviderAvailable`; do not duplicate
  the model table.
- `worker.ts`: handle `case "capabilities"` **before** session dispatch so it
  works with no active session; emit the event and continue. It must also
  work while a session is recording.
- Keep `stdout` NDJSON-only and `stderr` diagnostics-only.

## 3. Live cross-language smoke test (`crates/speechtt-core/tests/bridge_smoke.rs`)

A real process test, not a mock:

- Skip (return early with a printed note) when `bun` is not on `PATH` or
  `src/stt/worker.ts` is missing, so CI without Bun stays green.
- Spawn `BunSubprocess::spawn_default()` with a scrubbed environment (no
  real API keys) and:
  1. read the initial `Ready` event;
  2. send `SttCommand::Capabilities { request_id: Some("smoke-1"), api_key:
     None }`;
  3. assert a `Capabilities` event with the same `requestId`, at least five
     providers, and a non-empty model list with valid modes;
  4. assert every `available` flag is `false` (no keys in the environment);
  5. send `Stop`/drop the child and assert clean EOF/exit.
- Add a second case that serialises a **malformed** line directly into the
  child's stdin and asserts the worker replies with an `error` event
  (`invalid_protocol`) and stays alive for a following `capabilities` request.
- The test must not hang: use a 10 s timeout per read and kill the child on
  timeout.

## 4. TDD Tasks

- [x] **T8.1 — TS protocol specs**: parse `capabilities` with/without
  `requestId`/`apiKey`; reject non-string `apiKey`; serialise the outbound
  event with exact camelCase keys.
- [x] **T8.2 — Registry specs**: deterministic provider/model ordering;
  `available` reflects env or supplied key; every listed model has a valid
  mode; adding a provider key flips only that provider.
- [x] **T8.3 — Worker specs**: `capabilities` before `start`; during an
  active session; bad request emits `invalid_protocol` and does not close the
  loop; no API key appears in any emitted payload or diagnostic.
- [x] **T8.4 — Rust protocol specs**: round-trip the new command/event,
  redaction in `Debug`, `session_id() == None`, unknown-field tolerance.
- [x] **T8.5 — Bridge smoke test**: the live process scenarios in §3, with
  skip-on-missing-bun.
- [x] **T8.6 — Implementation + gates**: `bun test`,
  `bun check src/stt`, `cargo fmt -- --check`,
  `cargo clippy --all-targets -- -D warnings`, `cargo test`.

## Acceptance Criteria

- Running `bun run stt:worker </dev/null` still emits only
  `{"type":"ready"}` (no handshake unless asked).
- A Rust client can request and parse capabilities from the real Bun worker
  in one round trip, with `requestId` correlation.
- Availability reflects the process environment without exposing key values.
- Malformed input yields an `error` event and the worker keeps serving.
- Smoke tests skip cleanly where Bun or the script is unavailable.

## Implementation Notes

### Design decisions

- **Wire contract (TS `protocol.ts` / Rust `protocol.rs`)**: added
  `CapabilitiesCommand` (`{ type: "capabilities", requestId?, apiKey? }`) and
  `CapabilitiesEvent` (`{ type: "capabilities", requestId?, providers[] }`).
  Optional fields are omitted from serialized output when absent, so both
  languages produce the same bytes. `requestId` must be a non-empty string when
  present; `apiKey` must be a string when present. Errors never echo the raw
  line or key value.
- **Registry (`registry.ts`)**: `ProviderDefinition` now carries a `label` and
  an explicit model list; the gateway providers (`vercel` and `gateway`) share
  one `GATEWAY_MODELS` table (`provider/model` ids), local providers list their
  provider-local ids. Modes still come only from `MODEL_CAPABILITIES` through
  `resolveModelCapability`, so the mode table is not duplicated.
  `listCapabilities({ apiKeyFor?, env? })` returns a fresh, read-only
  `ProviderCapabilityInfo[]`; providers and models are sorted with a
  locale-independent code-unit comparator (label, then id as tie-break), so
  snapshots are stable across hosts.
- **`available` semantics**: provider-level only; models are always listed.
  Availability uses the existing `isProviderAvailable(providerId, key, env)`.
  Because the handshake payload has no `provider` field, a key supplied in the
  request is passed to every provider (ticket §1 wording); `apiKeyFor` keeps
  the registry function itself provider-scoped so T0009 can pass a per-provider
  key lookup if the protocol grows a provider field.
- **Worker (`worker.ts`)**: `case "capabilities"` is the first dispatch arm, so
  it works before `start` and while a session is recording. It only reads the
  registry and emits; it never creates, stops or pauses a session.
- **Rust types**: `TranscriptionMode` (lowercase `"streaming"`/`"segmented"`),
  `ProviderCapability` and `ModelCapabilityInfo` use `camelCase` serde renames.
  `SttCommand::Capabilities` omits absent optional fields; its derived `Debug`
  prints `SecretString("[REDACTED]")`, covered by
  `capabilities_command_serializes_its_key_but_redacts_debug`.
  `SttEvent::Capabilities` returns `session_id() == None` and tolerates unknown
  fields (serde default behaviour).
- **`backend.rs`**: added `BunSubprocess::send_raw_line` (typed commands still
  go through `send`). The malformed-NDJSON smoke scenario needs to put an
  invalid line on `stdin`, which the typed API cannot express.
- **Smoke test (`tests/bridge_smoke.rs`)**: spawns the real worker with an
  absolute script path computed from `CARGO_MANIFEST_DIR` (equivalent to
  `spawn_default`, but independent of the integration-test working directory).
  Provider environment variables are removed under a `Mutex` guard for the
  duration of each test (edition-2024 `unsafe { env::remove_var }`, safe because
  this test binary owns the environment in that window) and restored on drop.
  Reads use `try_recv` with a 10 s deadline and panic on timeout; `Drop` on
  `BunSubprocess` closes `stdin` and reaps the child. Both tests print a skip
  note and return early when `bun` is absent or `src/stt/worker.ts` is missing.
- **Scope note**: adding a `SttEvent` variant required a one-line new arm in
  the in-crate exhaustive match in `crates/speechtt-core/src/transcript.rs`
  (`| SttEvent::Capabilities { .. }`). Without it the crate does not compile.
  No other out-of-scope file was modified (`Cargo.toml`, `lib.rs`, `Cargo.lock`
  and `audio.rs`/`fft.rs` changes in the worktree belong to T0005).

### Test coverage

- `src/stt/protocol.spec.ts`: 9 new tests (16 → 25) — parse with/without
  optional fields, wrong types, key non-echo, exact camelCase serialization,
  `requestId` omission, redaction of the capabilities key.
- `src/stt/registry.spec.ts`: 6 new tests (14 → 20) — deterministic ordering,
  every model explicitly mapped to a valid mode, gateway vs local ids,
  env-driven availability, provider-scoped `apiKeyFor`, no key material in the
  payload.
- `src/stt/worker.spec.ts`: 5 new tests (10 → 15) — before `start`, during a
  live session, `invalid_protocol` recovery, `requestId` correlation/omission,
  no key in any emitted event. `bun test` total: **87 pass, 0 fail**.
- `crates/speechtt-core/src/protocol.rs`: 7 new unit tests (10 → 17 in the
  `protocol` filter) — command/event round trips with exact JSON, absent-field
  omission, `Debug` redaction, `session_id() == None`, unknown-field tolerance,
  lowercase mode values.
- `crates/speechtt-core/tests/bridge_smoke.rs`: 2 live tests — capabilities
  round trip over the real Bun process (`Ready` → `Capabilities` with
  `requestId` `smoke-1`, ≥ 5 providers, sorted models, all `available == false`,
  clean EOF) and malformed-line recovery (`invalid_protocol`, no key echoed,
  then `capabilities` with `smoke-2`).

### Gate results

| Gate | Result |
| --- | --- |
| `bun test` | `87 pass`, `0 fail`, 413 `expect()` calls, 7 files |
| `bun check src/stt` | `✓ No type errors in 13 files` |
| `cargo fmt -- --check` | clean (no diff) |
| `cargo clippy -p speechtt-core --test bridge_smoke -- -D warnings` | `Finished` with no warnings |
| `cargo clippy --all-targets -- -D warnings` | `Finished` with no warnings |
| `cargo test -p speechtt-core` | lib 66 passed, bridge_smoke 2 passed, session_machine 11 passed, doc-tests 0 |
| `cargo test -p speechtt-core --test bridge_smoke` | 2 passed in 0.15 s |
| `cargo test -p speechtt-core protocol` | 17 passed (49 filtered) |
| `cargo test` (workspace) | core 66/2/11 plus UI 14/0/8/17/4/2 — all passed |
| `bun run stt:worker </dev/null` | exactly `{"type":"ready"}` (no unsolicited handshake) |
