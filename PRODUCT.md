# SpeechTT — Floating Speech-to-Text Widget

## 1. Vision

A small floating widget for Linux (cross-platform-minded) that lets a user
record their voice, see a live waveform and live transcription, and paste or
save the resulting text — backed by a pluggable local or remote
Speech-to-Text (STT) provider.

The product is split in two cooperating parts:

- **Rust core**: audio capture (mic input), frequency analysis for the
  waveform, global keyboard shortcuts, clipboard/paste integration, file
  save, and communication with STT provider backends (local or remote).
- **Rust UI layer — native, GPU-accelerated (gpui-kit)**: the floating
  widget UI and dialogs, built with `gpui-kit` on **`gpui-component`
  styled components over a custom theme generated from
  `design-tokens.css`**. Custom pieces (record button, waveform, status
  pills, keycaps) are gpui-kit `RenderOnce` components. The UI must be
  fully renderable from mock state before any backend is wired in.
- **TypeScript/Bun adapters**: STT provider integrations (e.g. Vercel AI
  SDK for streaming transcription) consumed by the Rust layer — bridge
  topology is an open decision (see §7).

The UI must be built from a **Design System contract** (`DESIGN.md` +
`design-tokens.css`) using the `design-system-tokens` skill, so that the
Paper MCP mockups and the implemented UI stay visually consistent and do not
drift.

## 2. Users & use case

A developer/power user who wants to dictate text anywhere on their desktop
(any input field, any app) via a global shortcut, see what is being
transcribed in real time, and either have it auto-pasted or copy/save it
manually.

## 3. Functional requirements

### 3.1 Widget UI states

| State | Description |
|---|---|
| **Idle** | Widget visible, big record button, no audio activity, shortcuts reminder visible. |
| **Recording** | Record button turns into a stop/pause affordance, live waveform animates from mic frequency data, transcript streams in and auto-scrolls, clear "Recording" status indicator. |
| **Paused** | Waveform freezes/flattens, clear "Paused" status indicator (distinct color from Recording), transcript-so-far remains visible and scrollable, resume affordance shown. |
| **Settings** | Separate larger panel/modal: STT service/provider choice, API key management (per provider), default model choice, save/close actions. |

### 3.2 Core interactions

- **Record / Pause / Resume** toggle via the big button or a global keyboard
  shortcut.
- **Stop** recording (ends the session, keeps transcript in widget).
- **Live waveform**: bars/visualization reactive to live input frequency
  bins (not just raw amplitude).
- **Live transcription**: text appended to the visible transcript as soon as
  each chunk/partial result arrives from the STT backend (streaming).
- **Copy to clipboard**: one action/shortcut copies the full transcript.
- **Paste to focused input**: one action/shortcut types/pastes the
  transcript into whatever text field currently has OS focus.
- **Save to file**: exports the transcript as `.txt` or `.md` (user choice
  or configured default), with a default filename pattern
  (e.g. `speechtt-YYYYMMDD-HHMMSS.md`).
- **Settings**:
  - Choose active STT provider (local model vs. remote: Vercel AI Gateway,
    Mistral AI, others as added).
  - Enter/store API key(s) per provider (securely, not in plain config when
    avoidable).
  - Choose default model per provider (e.g.
    `mistral/voxtral-mini-transcribe-3`, `spacexai/grok-stt`).
  - Remote providers/models are only listed/enabled when their required
    environment variable or stored key is present (e.g.
    `VERCEL_AI_GATEWAY_API_KEY`).

### 3.3 Keyboard shortcuts (cross-platform generic notation)

| Action | Shortcut (example, configurable) |
|---|---|
| Start / Pause / Resume recording | `Ctrl+Shift+R` |
| Stop recording | `Ctrl+Shift+S` |
| Copy transcript | `Ctrl+Shift+C` |
| Paste transcript to focused field | `Ctrl+Shift+V` |
| Save transcript to file | `Ctrl+Shift+F` |
| Open settings | `Ctrl+Shift+,` |

Exact bindings are configurable in Settings; the widget's top info strip
always reflects the currently bound shortcuts.

### 3.4 Non-functional requirements

- Widget stays on top, is draggable, lightweight (idle CPU/mem footprint
  near zero).
- Works without a remote provider configured (local model fallback) —
  remote providers are optional, additive.
- No data persisted outside explicit user action (save) except the
  transcript buffer of the current session, until cleared.
- API keys never logged or displayed in full (masked in UI after entry).

## 4. Design system

- Follow the **Applied Design Systems** approach
  (https://github.com/zipang/applied-design-systems):
  - Install `skills/design-system-tokens/` into `.agents/skills/` of this
    repo.
  - Generate `DESIGN.md` (YAML front matter + prose) and
    `design-tokens.css` as the single source of truth for tokens.
  - Only use the fixed token categories: `colors` (brand/action/text/surface),
    `typography` (families/sizes/weights/line-heights/letter-spacing),
    `spacing` (`xs`→`xxl`), `shapes` (radii/border widths), `elevation`
    (`sm`/`md`/`lg`).
  - Visual direction: bold/branded, dark charcoal `surface`, deep
    "Agfa Sensor orange" as `brand.primary`/`action` accent for the record
    button and waveform.
- Paper MCP mockups (Idle / Recording / Paused / Settings / Help frames) are
  produced from these tokens, not invented ad hoc.
- **Exported reference assets** (Phase 1 output, under `mockups/`):
  - `mockups/app-window/` — Idle, Recording, Paused widget states.
  - `mockups/dialogs/` — Settings, Help/Shortcuts modals.
  - Each artboard exists as a **`.png` screenshot** and an auto-exported
    **`.tsx` JSX dump**.
  - **The `.png` files are the visual ground truth** for the Phase 7 UI:
    the implementation is built semantically (real HTML elements +
    `design-tokens.css`) and validated against them pixel by pixel.
  - **The `.tsx` files are NOT usable as code**: the exporter emits
    non-semantic, accessibility-flat `<div>` soup with inline styles. They
    serve one purpose only — a lookup table for exact values (colors, px
    sizes, spacing, radii, shadows) when a `DESIGN.md` token value is
    ambiguous. Never copy/paste them into the implementation.

## 5. Technical stack

- **Rust — UI layer (gpui-kit, native GPU-accelerated; replaces the
  former webview/tauri plan)**:
  - Framework: `gpui-kit = "0.8"` — one dependency bundling GPUI,
    `gpui-base`, `gpui-component` (75+ styled components), the theme
    system, default **Lucide** icon assets, and headless UI integration
    testing.
  - Built on **`gpui-component` styled components over a custom SpeechTT
    theme** mapped 1:1 from `design-tokens.css` (dark charcoal surfaces,
    deep Agfa-Sensor orange, type scale ×1.25, radii, elevation).
  - Custom visual pieces written as `RenderOnce` components:
    `HeaderStrip`, `StatusBadge`, `BigAction` (92px record/stop/resume
    circle), `KeyCap`, `Waveform` (bars from FFT bins), `CaptionWell`.
  - **Typography — DECIDED: bundle our own fonts** (the
    `design-tokens.css` "system fonts only" rule was the web-mockup
    default, irrelevant to the native app). Inter (400/500/700) + JetBrains
    Mono (400/700) are embedded via `include_bytes!` +
    `text_system().add_fonts()` — deterministic rendering on every
    machine, no network, ~1.6 MB binary size; SIL-OFL license texts are
    kept next to the `.ttf` files in `crates/speechtt-ui/assets/fonts/`.
  - Settings & Help/Shortcuts shown as gpui-kit dialogs (window
    overlays); global styling comes from the installed agent skills
    (`longbridge/gpui-kit`).
  - UI integration tests via `#[gpui_kit::test]` (headless windows:
    state, focus, layout, accessibility).
  - **Works without a backend**: state fed by a mock (fake timer, fake
    waveform bins, fake streaming transcript) until the core is wired.
- **Rust — core**:
  - Audio capture: `cpal`.
  - FFT / frequency analysis for waveform: `rustfft` (or `realfft`).
  - Global shortcuts: platform-appropriate crate (e.g. `global-hotkey`).
  - Clipboard: `arboard`.
  - Synthetic keyboard paste (type into focused field):
    `enigo` or platform input simulation.
- **TypeScript + Bun** (provider adapter layer — topology open, §7):
  - STT provider integrations: Vercel AI SDK (streaming transcription)
    for Vercel AI Gateway-hosted models; direct Mistral AI API client
    for Mistral-hosted models; interface abstracted so new providers
    can be added.
  - Bun used as the runtime/test runner for the TS layer.
- **Testing**: TDD for both sides.
  - **UI test policy — test the function, not the pixels (decided).**
    Headless UI tests assert *behavior*: state transitions
    (idle/recording/paused), modal open/close (settings, help), and
    button wiring. They must NOT assert element placement, relative
    geometry, bounds comparisons, or pixel comparisons — the UI must
    stay free to evolve (add/remove/rearrange controls) without breaking
    tests. Mockups (`mockups/**/*.png`) are checked by *manual* visual
    QA only, never by automated tests. **Stable `ElementId`s identify
    controls in tests — never accessibility labels or visible wording**:
    a control's label may change freely for copy/UX reasons without
    breaking any test, and its position likewise must not be asserted.
  - Rust: `cargo test` (unit tests for state transitions, audio
    chunking, FFT bin aggregation, hotkey event mapping, clipboard/paste
    adapters mocked behind traits; gpui-kit headless UI tests for the
    behaviors above).
  - TypeScript: `bun test` (unit tests for provider adapters, settings
    store, transcript buffer/streaming reducer, shortcut-to-action
    mapping).

## 6. Task list (TDD-first)

Each feature task starts with writing the failing test(s), then the
implementation, following red → green → refactor.

### Session workflow (state machine, as implemented)

The machine lives in `crates/speechtt-core/src/session.rs`; the UI derives
every screen from it (plus one UI-owned result state).

| State | Entered when | Screen / controls |
|---|---|---|
| `Blank` | at least one of the five parameters is missing | New Session prompt (`+`) → setup form |
| `Ready` | all five parameters present | READY badge + mic circle |
| `Recording` | `start` from `Ready` | RECORDING badge + timer + waveform + caption; **Pause + Stop**, Stop **disabled** |
| `Paused` | `pause` from `Recording` | PAUSED badge + timer + muted waveform + caption; **Resume + Stop**, Stop **enabled** |
| *Stopped (UI)* | `stop` from `Paused` | result: memory → transcript + Copy/Save; file → written path + Open File |

Rules:
- **Five parameters are required before any recording**: audio source
  (defaults to the system mic in the mock), provider, model, session name,
  output mode (`Memory` or `File`). Setup changes are frozen during a take.
- **Transport controls never move between Recording and Paused**: both show
  Play/Pause + Stop in the same positions. Stop is inert while recording and
  live only while paused.
- **Stop semantics**: the machine hands back a `FinishedSession` (source,
  provider, model, name, output) and resets to `Blank`, keeping audio source
  / provider / model and clearing session name + output — the next take must
  be named. The UI keeps the finished take to show the `Stopped` screen until
  **New take** dismisses it (machine is already `Blank` underneath).
- **Two Stopped layouts, one per output mode**:
  - *To memory*: transcript well + **Copy** + **Save**, no file path.
  - *To file*: `TRANSCRIPT WRITTEN TO` + path + **Open File**; no transcript
    well, no Copy, no Save.
- Copy/Save/Open File handlers are stubs until the Phase 5 adapters exist
  (clipboard, file write, editor launch).
- File path defaults to `~/Documents/<session-name>.md` until a file picker
  exists.

### Phase 0 — Design system foundation
- [x] Install `design-system-tokens` skill into `.agents/skills/`.
- [x] Install the Vercel AI SDK skill (`bunx skills add vercel/ai --skill ai-sdk`).
- [x] Use the `use-bun` skill: all TS tooling runs on Bun (`bunx`, `bun test`,
      `bun check`, `bun fmt`, `bun lint`), never `npx`/`vitest`/`tsc`.
- [x] Draft `DESIGN.md` front matter (colors, typography, spacing, shapes,
      elevation) for the orange/charcoal branded direction.
- [x] Generate `design-tokens.css` from `DESIGN.md` (+ `color-variants.css`,
      `reset.css`).
- [x] Vendor the (unpublished) `@applied-design-systems/design-tokens`
      validator under `vendor/design-tokens` and expose
      `bun run design:validate` / `bun run design:contrast`.
- [x] Test the contract in `tests/design-contract.test.ts` (validation,
      token presence, AA text/surface contrast).

### Phase 1 — Paper MCP mockups (done)
- [x] Idle state frame.
- [x] Recording state frame (waveform + live caption).
- [x] Paused state frame.
- [x] Settings panel frame.
- [x] Help / Shortcuts modal frame (per header redesign: app title in the
      top strip, `?` button next to the settings cog, shortcuts shown only
      in the help dialog, never permanently).
- [x] Review with screenshots, adjust spacing/contrast against tokens.
- [x] Export reference assets to `mockups/` (`.png` visual ground truth +
      auto-exported `.tsx` value dumps — see §4 for their roles).

### Phase 2 — GPUI UI shell (no backend, mock state)
- [x] Workspace scaffold: workspace `Cargo.toml`, `crates/speechtt-ui`,
      `crates/speechtt-core` placeholder; `gpui-kit = "0.8"` dependency;
      app opens a 480×280 always-on-top, draggable window
      (`application()` → `init(cx)` → `open_window`).
- [x] Test: headless (`#[gpui_kit::test]`) render shows the header with
      the app title and the `?` + cog icon buttons, found by their stable
      ElementIds (presence + visibility only — never placement, never
      labels).
- [x] Custom theme: map every `design-tokens.css` token (colors, radii,
      type scale ×1.25, spacing, elevation) into a gpui-kit dark Theme.
- [x] Test: theme contract — Rust theme values equal the CSS token values
      (cargo-test extension of the design-token contract).
- [x] Session state machine (pure Rust in `speechtt-core`): five required
      session parameters (audio source, provider, model, session name,
      output target) drive `Blank ⇄ Ready`; then `Ready → Recording ⇄
      Paused`; `Stop` returns the finished session to the caller and resets
      to `Blank` keeping audio source/provider/model (name + output clear
      for the next take). Setup changes freeze during a take. Invalid
      transitions return an error and never mutate state.
- [x] Test: session machine transitions (unit, no UI) — 10 cases in
      `crates/speechtt-core/tests/session_machine.rs`.
- [x] UI wiring: `SpeechTtApp` owns a `SessionMachine`; the New Session
      form fills the parameters; clicking the main action / pause / resume /
      stop buttons transitions the state (headless click, assert the
      resulting state — no geometry, find by stable ids). 7 behavior tests in
      `crates/speechtt-ui/tests/session_flow.rs`.
- [x] Blank state screen (inferred — no mockup frame): `NO SESSION` pill,
      74px `+` circle labelled "New Session", hint line; opens the form.
- [x] New Session form (inferred — no mockup frame): provider + model
      (mock registry, dependent cycling), session title input, output
      Memory/File segment, Cancel / Create session (inert until all five
      parameters are set). Body view, not a dialog (480×280 has no room).
- [x] Core visual components (`crates/speechtt-ui/src/components/`):
      `StatusBadge`, `ElapsedTimer`, `Waveform`, `CaptionWell`, plus the
      reusable `TransportButton`, `IconAction`, `SettingRow` and `ChoicePill`
      controls. Behavior is covered through `tests/session_flow.rs`.
- [x] Idle/Ready view per `mockups/app-window/idle.png`: READY badge,
      74px mic circle, "Press to start dictating".
- [x] Recording / Paused views per `mockups/app-window/*.png`: badge +
      `mm:ss` timer row, waveform (active bars / muted dots when paused),
      caption well (muted committed text + brand live words). Fed by static
      mock constants (`MOCK_ELAPSED_SECONDS`, `MOCK_BARS`, transcript
      lines); live animation + real bins arrive with the core wiring.
- [x] Transport controls by state (decided): Recording and Paused share the
      **same two controls** (Play/Pause toggle + Stop) so nothing shifts
      between states. Stop is **disabled/inert while Recording** and only
      becomes live when Paused. Copy/Save appear only on the Stopped
      screen, and only for memory takes.
- [x] Stopped screen (result), split by output mode: **file takes** show
      only the written path + **Open File** (no transcript, no Copy/Save);
      **memory takes** show the transcript + **Copy/Save**; both offer
      **New take** → Blank (name + output required again). Copy/Save/Open
      File handlers are stubs until the Phase 5 adapters exist.
- [ ] Settings dialog per `mockups/dialogs/settings.png` (provider select,
      model select, masked API key field, connected badge, local fallback
      note, Cancel/Save). Test: cog click opens, Cancel/Save close —
      behavior only.
- [ ] Help / Shortcuts dialog per `mockups/dialogs/help.png` (keycap rows,
      global-scope note, version/footer, Got it). Test: `?` click opens,
      Got it/Escape closes — behavior only.
- [ ] Visual QA (manual, not automated): side-by-side against all
      `mockups/**/*.png`.

### Phase 3 — Rust core: audio capture & waveform
- [ ] Test: audio input stream yields fixed-size PCM chunks at expected
      sample rate.
- [ ] Implement `cpal`-based capture producing chunks.
- [ ] Test: FFT of a known synthetic signal (e.g. 440Hz sine) produces
      expected dominant frequency bin.
- [ ] Implement FFT + bin-to-bar-amplitude mapping for the waveform.
- [ ] Test: silence (near-zero amplitude) maps to a flat/minimal waveform.

### Phase 4 — Rust core: hotkeys & lifecycle events
- [ ] Test: global hotkey events map to the correct session actions
      (start, pause, resume, finish).
- [ ] Implement hotkey listener + dispatch into `SessionMachine`.
- [ ] Test: a finished take (memory or file mode) is routed to the right
      consumer — transcript buffer vs file writer.

### Phase 5 — Rust core: clipboard, paste, save
- [ ] Test: transcript buffer copy produces exact clipboard content
      (clipboard backend mocked behind a trait).
- [ ] Implement clipboard adapter (`arboard`).
- [ ] Test: paste adapter sends correct keystroke/text sequence
      (input backend mocked).
- [ ] Implement paste adapter (`enigo`).
- [ ] Test: save-to-file writes `.txt`/`.md` with correct content and
      filename pattern; rejects unsupported extensions.
- [ ] Implement file save.

### Phase 6 — TypeScript: provider abstraction
- [ ] Test: provider registry only lists a remote provider when its env
      var/key is present.
- [ ] Implement provider registry + capability detection.
- [ ] Test: Vercel AI Gateway adapter streams partial transcripts and
      emits incremental text events (mock SDK/stream).
- [ ] Implement Vercel AI Gateway adapter.
- [ ] Test: Mistral AI adapter streams/returns transcripts correctly
      (mocked API).
- [ ] Implement Mistral AI adapter.
- [ ] Test: switching provider/model in settings updates the active
      adapter used by the recording session.

### Phase 7 — TypeScript: settings store & UI state
- [ ] Test: settings store persists provider choice, masked API keys, and
      default model; round-trips correctly.
- [ ] Implement settings store (Bun-backed, e.g. local config file).
- [ ] Test: API keys are masked in any serialized UI state/log output.
- [ ] Implement masking.
- [ ] Test: transcript streaming reducer appends partial chunks in order
      and exposes the full transcript + "is streaming" flag.
- [ ] Implement transcript reducer.
- [ ] Test: shortcut-to-action mapper resolves configured bindings to the
      correct UI/IPC action, including rebinding.
- [ ] Implement shortcut mapper.

### Phase 8 — UI ↔ core & provider wiring
- [ ] Test (Rust): core state-machine events drive UI state in the gpui
      views (replace the mock feed); wiring follows the §7 topology
      decision (single process or daemon + IPC).
- [ ] Wire live waveform bins and elapsed timer from the core into the
      `Waveform`/`CaptionWell` components.
- [ ] Wire global hotkeys → state machine → UI updates.
- [ ] Settings dialog writes to the settings store; the selected
      provider adapter streams into the transcript reducer.
- [ ] End-to-end manual test: record → live waveform + live text → stop →
      copy / paste / save all work.

### Phase 9 — Polish & packaging
- [ ] Error states (mic permission denied, provider auth failure, network
      loss during streaming) surfaced in UI.
- [ ] Idle resource usage check (CPU/mem).
- [ ] Packaging for Linux (binary + bundled webview assets).

## 7. Open questions / future scope

- **UI ↔ core topology — DECIDED: single process.** Core (audio, hotkeys,
  clipboard, save) and the gpui-kit UI live in one program and talk
  directly. We never wanted a background daemon; if hotkeys must later
  survive a closed window, that is a new requirement to be scoped.
- **Provider adapters**: stay in TypeScript/Bun (as currently scoped) or
  move into Rust (now that the UI is native) — affects Phases 6/7.
- Local model support (which runtime/engine — e.g. whisper.cpp) is listed as
  a placeholder in the README and not yet specified in detail.
- Multi-language transcription support not yet scoped.
- Auto-paste-on-stop vs. manual paste as a configurable default — TBD with
  user testing.
