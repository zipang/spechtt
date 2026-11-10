# T0032 — Say honestly what the dictation language can reach

- **Track**: Cross-track (Bun worker capabilities → Rust protocol → Rust UI)
- **Wave**: 1 (independent)
- **Depends on**: T0030 (the setting and its wiring)
- **Blocks**: nothing
- **References**:
    - `AGENTS.md` §2 (component rule), §3 (documentation), §5 (gates)
    - `DESIGN.md` § "Settings panel"
    - AI Gateway provider options (the reason a gateway gets no language):
      <https://ai-sdk.dev/providers/ai-sdk-providers/ai-gateway.md> § "Gateway Provider Options"
    - Streaming transcription provider options:
      <https://ai-sdk.dev/docs/ai-sdk-core/transcription.md> § "Streaming Transcription"

## Context & Goal

T0030 added a **Dictation Language** setting and forwards it through
`providerOptions[<package>].language`. That option exists only for direct
provider packages (`openai`, `mistral`, `xai`, `elevenlabs`). The AI Gateway's
documented options are routing and accounting fields with no `language`, and
sending one makes the gateway reject the whole request — which is exactly the
`stream_failed` a real take hit before it was fixed by refusing to send it.

The shipped catalogue is gateway-only, so today the setting **cannot reach the
transcription model at all**. It still genuinely feeds the cleanup prompt, but
the Settings row reads _"The language you speak; improves accuracy"_, which
promises the user a transcription gain that does not arrive.

Done means: the dialog says which of the two is true, from the worker's own
knowledge rather than a hardcoded list.

## Scope Boundary

Allowed to create or modify:

- `src/stt/languages.ts` + `src/stt/languages.spec.ts`
- `src/stt/registry.ts` + `src/stt/registry.spec.ts`
- `src/stt/worker.ts` + `src/stt/worker.spec.ts`
- `crates/speechtt-core/src/protocol.rs`
- `crates/speechtt-ui/src/controller.rs`
- `crates/speechtt-ui/src/views/dialogs/settings.rs`
- `crates/speechtt-core/tests/bridge_smoke.rs`
- `crates/speechtt-ui/tests/{common/mod.rs,widget.rs,dialogs.rs}`
- `DESIGN.md`
- `tickets/README.md` and this ticket

Out of scope — do **not** touch:

- `src/stt/adapters/**` (forwarding is already correct: the packages that
  document the option receive it, the gateway does not)
- `crates/speechtt-ui/src/components/**` (`SettingRow` and `InfoNote` are enough;
  no new component is needed)
- `crates/speechtt-ui/src/views/{welcome,session_setup,recording,session_transcription,session_end}.rs`
- `crates/speechtt-ui/src/tokens.rs`, `design-tokens.css`, `tests/design_system.rs`

## Contract

### 1. Worker: report what the language reaches

The capabilities payload gains one boolean, computed from the same rule the
adapters apply:

```ts
// src/stt/registry.ts — CapabilitiesPayload
/**
 * `true` when some offered provider package documents a `language` option, so the dictation
 * language can reach a transcription model. Derived from `languageIsForwarded` over the offered
 * providers, never hardcoded.
 */
languageReachesTranscription: boolean;
```

`languageIsForwarded(provider)` already exists in `languages.ts` for that
purpose; this ticket gives it its production caller.

On the wire, optional so an older worker still parses:

```rust
Capabilities {
    // …existing fields…
    #[serde(default, skip_serializing_if = "Option::is_none")]
    language_reaches_transcription: Option<bool>,
}
```

### 2. Controller: expose it

```rust
/// Whether the dictation language can reach a transcription model.
///
/// `false` when the worker reports that no offered provider documents the
/// option, and `false` when no catalogue has arrived (nothing is claimed
/// before it is known). The setting still feeds the cleanup prompt either way.
#[must_use]
pub fn language_reaches_transcription(&self) -> bool;
```

### 3. Settings: one honest line

The **Dictation Language** row keeps its label, its select and its Auto entry.
Its description becomes conditional:

- reachable — _"The language you speak; improves accuracy"_ (unchanged);
- not reachable — _"The language you speak; used by the cleanup pass. The
  transcription model detects it"_.

When it is not reachable, the row also renders an `InfoNote` below the select
with the same fact in one sentence, because a muted description alone is easy to
miss. `InfoNote` is the existing component: no new tone, size or spacing.

No other row changes, and the setting is never disabled — it is still a real
input for the cleanup prompt.

## TDD Tasks

- [ ] **T32.1 — Red: worker.** Failing cases: a gateway-only catalogue reports
      `languageReachesTranscription: false`; a catalogue containing a direct
      language-aware provider reports `true`; the value comes from
      `languageIsForwarded`, so a new provider package that documents the option
      flips it without touching the flag. Then implement.
- [ ] **T32.2 — Red: protocol.** Failing cases: a capabilities payload with and
      without `languageReachesTranscription` parses; absent stays absent on the
      wire. Then implement.
- [ ] **T32.3 — Red: controller.** Failing cases: `false` before any catalogue,
      `false` for a gateway-only catalogue, `true` when a direct provider is
      present, and the cleanup model resolution unaffected either way. Then
      implement.
- [ ] **T32.4 — Green: the row.** Conditional description plus the `InfoNote`
      branch; headless test asserting both branches render without panicking and
      that the select is enabled in both.
- [ ] **T32.5 — Docs and gates.** `DESIGN.md` § "Settings panel" states the two
      forms of the row and the reason. Then `cargo fmt -- --check`,
      `cargo clippy --all-targets -- -D warnings`, `cargo test`, `bun test`,
      `bun check src/stt`, `bun run design:validate`, recorded in
      `## Implementation Notes`.

## Acceptance Criteria

- With a gateway-only catalogue, the Dictation Language row states that the
  setting applies to the cleanup pass and that the transcription model detects
  the language — no promise of an accuracy gain it cannot deliver.
- With a catalogue offering a direct provider that documents the option, the row
  reads as before.
- The setting stays usable in both cases and still reaches the cleanup prompt.
- The flag comes from the worker's capability report, not from a list duplicated
  in Rust or in a view.
- All six gates are green with zero warnings and no skipped tests.
