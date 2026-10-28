# T0014 — Help Dialog: Model Catalogue Tab and a Single Gateway Provider

- **Track**: TypeScript/Bun worker (`discovery`, `registry`, `protocol`) + Rust core (`protocol`, `settings`) + Rust UI (`help`, `controller`)
- **Wave**: 2 (product polish; follows T0011's catalogue work)
- **Depends on**: T0011 (discovery + mode badge), T0004 (Help dialog)
- **Blocks**: nothing
- **References**: live `GET https://ai-gateway.vercel.sh/v1/models` (anonymous), `mockups/dialogs/help.png`, `DESIGN.md`

## Context & Goal

The Help dialog only documents keyboard shortcuts, while the app now discovers a
rich model catalogue that is invisible to the user. The gateway payload carries,
for each transcription model: `name`, `description`, `type`, `tags`,
`pricing` (per-token **or** `transcription_duration_cost_per_second`),
`owned_by` and `released`.

Meanwhile only **Vercel AI Gateway** has been evaluated end to end. Shipping
four untested direct providers in Settings invites failed sessions, so the
provider list is restricted to the gateway until each candidate gets its own
evaluation ticket (API, auth, streaming support, model list).

**Goal**: a second Help tab lists the selected provider's models with the mode
badge, description and price, sourced from the live discovery; and the provider
list contains exactly one entry. Rows stay compact (three to four lines) so the
whole list is readable at the dialog's 600 × 520 size.

## Scope Boundary

```text
src/stt/discovery.ts                  # richer DiscoveredModel + parsing
src/stt/discovery.spec.ts
src/stt/registry.ts                   # single provider, richer model details
src/stt/registry.spec.ts
src/stt/protocol.ts                   # ModelCapabilityInfo metadata
src/stt/protocol.spec.ts
src/stt/worker.spec.ts                # only if fixtures need the new fields

crates/speechtt-core/src/protocol.rs  # ModelCapabilityInfo metadata (serde defaults)
crates/speechtt-core/src/settings.rs  # normalize unsupported providers
crates/speechtt-core/tests/bridge_smoke.rs # only if the payload shape is asserted

crates/speechtt-ui/src/controller.rs  # HelpTab state + model queries + pricing text
crates/speechtt-ui/src/views/dialogs/help.rs   # tabs + model list
crates/speechtt-ui/src/views/dialogs/mod.rs    # help dialog height (if needed)
crates/speechtt-ui/src/app.rs         # apply HelpTab intents from the dialog
crates/speechtt-ui/src/views/dialogs/window.rs # forward tab intents
crates/speechtt-ui/tests/dialogs.rs   # tab + list behaviour
crates/speechtt-ui/tests/end_to_end.rs # catalogue/help wiring
crates/speechtt-ui/tests/common/mod.rs # richer test catalogue

tickets/README.md
```

Do **not** touch `src/stt/adapters/**` (the direct-provider resolvers stay
dormant for future tickets), `crates/speechtt-core/src/{audio,fft,output}.rs`,
`tokens.rs`, `design-tokens.css`, `DESIGN.md`, or the T0012 segmentation code.

## Contract

### 1. One provider

- `PROVIDERS` contains only `vercel-ai-gateway` (label "Vercel AI Gateway",
  env vars `AI_GATEWAY_API_KEY`, `VERCEL_AI_GATEWAY_API_KEY`).
- `PROVIDER_ALIASES` still maps `vercel` and `gateway` to the canonical id.
- Delete the `mistral`, `openai`, `xai`, `elevenlabs` curated entries and their
  `MODEL_CAPABILITIES` rows. `MODEL_CAPABILITIES` remains derived from the
  curated catalogue.
- `listCapabilities()` returns exactly one provider; the gateway model list is
  unchanged (16 curated ids, 7 streaming) until discovery replaces it.
- Keep `defaultModelResolver`'s direct-provider branches and the `@ai-sdk/*`
  dependencies: they are dormant and will be re-enabled per evaluation ticket.
- Core `settings.rs`: add `SUPPORTED_PROVIDERS: [&str; 1] = ["vercel-ai-gateway"]`
  (documented as the sync point with the registry) and make
  `normalize_provider_ids()` map any **other** provider to `DEFAULT_PROVIDER`.
  Stored keys under removed providers are kept in `api_keys` (a future ticket
  must not force users to paste them again), but the active provider becomes
  the gateway.

### 2. Richer discovery (`DiscoveredModel`)

```ts
export interface DiscoveredModel {
  readonly id: string;
  readonly label: string;
  readonly mode: TranscriptionMode;
  /** Gateway `description`, or null. */
  readonly description: string | null;
  /** Raw gateway pricing map, values kept as strings. */
  readonly pricing: Readonly<Record<string, string>>;
}
```

- `tags` are still read to decide `mode` but are not transported: the badge
  already conveys streaming vs batch, and the compact row shows no tag chips.
- Parsing stays defensive: strings only, unknown pricing keys preserved,
  malformed entries skipped; `type === "transcription"` still the filter.
- Curated fallback entries carry `description: null` and an empty pricing map.
- Cache, TTL, single-flight and the anonymous/no-key guarantee are unchanged.

### 3. Wire (`ModelCapabilityInfo`)

```rust
pub struct ModelCapabilityInfo {
    pub id: String,
    pub label: String,
    pub mode: TranscriptionMode,
    #[serde(default)] pub description: Option<String>,
    #[serde(default)] pub pricing: BTreeMap<String, String>,
}
```

- Every new field defaults so an older worker or UI keeps working.
- `listCapabilities()` passes the discovered metadata through; curated models
  emit the defaults.

### 4. Help dialog tabs

- `HelpTab { Shortcuts, Models }` in the controller, with
  `help_tab()` / `set_help_tab(tab)`; opening Help resets to `Shortcuts`.
- A `gpui_kit::component::tab::TabBar` (`.segmented()`, `.selected_index(...)`,
  `.on_click(...)`) at the top of the dialog body with two `Tab`s:
  "Shortcuts" and "Models". The selected index comes from the controller, so
  the headless test can assert it without geometry.
- The footer ("Got it", version, gateway status) stays unchanged on both tabs.
- `DialogWindow` forwards a `select_help_tab(tab)` intent to `SpeechTtApp`,
  which updates the controller and notifies.

### 5. Models tab content

- Header line: provider label + catalogue source:
  `Live catalogue` (`CapabilitySource::Discovered`) or
  `Built-in catalogue` (`Builtin` / not loaded yet).
- A scrollable list (`overflow_y_scrollbar`, `min_h_0`) of the provider's
  models, sorted exactly as the capability payload (label, then id). Each row:
  1. `TranscriptionModeBadge` (`⚡ Stream` / `✂ Batch`),
  2. model label (medium) and id in the mono font, muted,
  3. description (muted, wraps, at most three lines),
  4. pricing line(s) from the helper below.
- Rows stay compact: no tags, owner or release columns.
- Empty/unknown: `No model metadata yet. Open Settings to refresh the catalogue.`
  when no catalogue has arrived, and the row simply omits missing details.
- All colors/sizes/spacing from `crate::tokens` / the theme.

### 6. Pricing formatter (pure, tested)

`controller::pricing_lines(pricing: &BTreeMap<String, String>) -> Vec<String>`
with deterministic order:

| Key | Rendering |
|---|---|
| `transcription_duration_cost_per_second` | `$0.000031 / second of audio` |
| `input` | `$2.00 / M input tokens` (value × 1 000 000) |
| `output` | `$10.00 / M output tokens` |
| any other key | `<key title-cased>: <raw value>` |
| missing/empty map | `Price not published` |
| zero values | `Free` |

- Costs use two significant decimals for per-million prices and up to six
  decimals for per-second prices; a value that cannot parse is shown raw.
- Keys are emitted in the table order, unknown keys afterwards sorted
  alphabetically, so the output is stable.

## TDD Tasks

- [x] **T14.1 — Registry specs**: exactly one provider; aliases still resolve;
  catalogue still 16 gateway ids with the 7/9 split; the removed ids are absent
  from `PROVIDERS` and `MODEL_CAPABILITIES`; `listCapabilities` metadata
  defaults are empty for curated models and pass through for discovered ones.
- [x] **T14.2 — Discovery specs**: description and pricing are parsed from a
  real-shaped fixture; malformed pricing values and missing keys degrade to
  null/empty; tags still decide the mode but are not returned; zero
  transcription models still returns `undefined`; sort, cache and single-flight
  unchanged.
- [x] **T14.3 — Protocol specs** (TS + Rust): metadata round-trips; missing
  fields default; old payloads still deserialize.
- [x] **T14.4 — Settings specs**: a stored `mistral`/`openai`/`xai`/`elevenlabs`
  provider normalizes to the gateway; its key is retained in `api_keys`; the
  gateway alias still migrates; idempotent.
- [x] **T14.5 — Controller/help specs**: default tab is Shortcuts; switching
  works and resets on open; `help_models()` returns the catalogue rows with
  modes/description/pricing; pricing formatter table above; empty catalogue
  state.
- [x] **T14.6 — Help view**: Shortcuts tab unchanged; Models tab renders the
  rows and the empty state (headless render + controller assertions); both
  tabs switch through the app.
- [x] **T14.7 — Implementation**: discovery metadata, registry restriction,
  protocol fields, settings normalization, `TabBar`, compact list, formatter,
  dialog height/scroll.
- [x] **T14.8 — Gates**: `cargo fmt -- --check`,
  `cargo clippy --all-targets -- -D warnings`, `cargo test`, `bun test`,
  `bun check src/stt`.

## Acceptance Criteria

- The Help dialog has two tabs; the Models tab lists every model of the
  Vercel AI Gateway with mode, description and pricing (or
  `Price not published`), from the live catalogue.
- The catalogue is labelled `Live catalogue` when discovery succeeded and
  `Built-in catalogue` otherwise; the list stays scrollable at 600 × 520.
- Settings offers exactly one provider (`Vercel AI Gateway`); old settings with
  another provider open with the gateway selected, without losing stored keys.
- No key material appears anywhere; model metadata is public.
- All existing gates stay green with zero warnings, and the T0011/T0012/T0013
  behaviour is unchanged.

## Follow-ups (explicitly out of scope)

- **Provider evaluation tickets**: re-introduce a direct provider only after its
  API, auth, streaming support and model discovery are validated.
- **Richer model metadata**: owner, release date, context window, modalities,
  supported specifications and zero-data-retention flags (available in the
  payload, hidden by the compact row decision above).
- **Catalogue refresh affordance**: a manual "Refresh" button in the Models tab.
- **Copy model id** action from the list.

## Implementation Notes

Implemented by the CTO directly (coder quotas still exhausted).

### Delivered

- **Single provider**: `PROVIDERS` now contains only `vercel-ai-gateway`;
  `vercel`/`gateway` still resolve as aliases, and the direct adapters, resolver
  branches and `@ai-sdk/*` dependencies stay dormant for future evaluation
  tickets. `settings.rs` gained `SUPPORTED_PROVIDERS` and normalises any other
  stored provider to the gateway **without dropping its key**, so re-enabling a
  provider later does not force a re-paste.
- **Metadata**: `DiscoveredModel` carries `description` (trimmed, null when
  empty) and a defensive `pricing` map (string values only; malformed entries
  ignored). Tags still decide `mode` and are not transported. Curated fallback
  models emit `description: null` and an empty pricing map.
- **Wire**: `ModelCapabilityInfo` gained `description` and `pricing`, both
  optional and skipped when empty, so curated payloads stay byte-identical and
  older peers keep working. Rust round-trip and defaulting are tested.
- **Help dialog**: `HelpTab { Shortcuts, Models }` in the controller (reset to
  Shortcuts on open), a `gpui_kit::component::tab::TabBar` (segmented) in the
  dialog, `DialogWindow::select_help_tab` forwarding to
  `SpeechTtApp::set_help_tab`. The Models tab shows the provider name, the
  catalogue source (`LIVE CATALOGUE` / `BUILT-IN CATALOGUE` / not loaded), and
  a scrolling list of compact rows: mode badge, label, mono id, description,
  pricing lines. Empty state: "No model metadata yet. Open Settings to refresh
  the catalogue."
- **Pricing formatter** (`controller::pricing_lines`, pure and tested):
  per-second `$0.000031 / second of audio`, per-token
  `$2.00 / M input tokens`, `Free` when every known price is zero,
  `Price not published` for an empty map, and unknown keys rendered as
  `Title case key: value` after the known ones in alphabetical order.
- Test catalogues across the UI suite were trimmed to the single provider, and
  the live-bridge smoke test now asserts one provider with its key env vars.

### Notes

- The dialog body already scrolls; the Models tab adds a bounded inner scroll
  so the tab bar and the provider header stay put while the list scrolls.
- The Models tab follows the Settings provider, falling back to the first
  provider in the catalogue (the only one today).
- Live verification against the gateway: 16 transcription models, 14 with both
  a description and pricing. Shapes vary (`input`/`output`, a per-second
  duration cost, and extra keys such as `audio_input_token_cost`), all handled
  by the formatter.
- Owner/release/tag chips were deliberately left out per the compact-row
  decision; the payload still exposes them if a future ticket wants them.

### Gates

```
cargo fmt -- --check                      clean
cargo clippy --all-targets -- -D warnings clean
cargo test                                290 passed, 0 failed
bun test                                  146 passed, 0 failed
bun check src/stt                         ✓ 19 files, no type errors
bun run design:validate                   ✓ 66 tokens, 0 errors
```
