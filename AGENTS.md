# AGENTS.md — SpeechTT Agentic Workflow Guide

This file is the **source of truth for coding and reviewing agents** working on
SpeechTT. It describes the architecture, engineering rules, quality gates, and
scope conventions agents must follow. It does **not** duplicate the project
structure or module map — see `README.md` for those.

---

## 1. Project Identity

SpeechTT is a native, GPU-accelerated floating Speech-to-Text widget for Linux.
It is a **single-process Rust host** (`crates/speechtt-core` + `crates/speechtt-ui`
on `gpui-kit`) that spawns a **stateless TypeScript/Bun STT worker**
(`src/stt/worker.ts` on the Vercel AI SDK) over a full-duplex `stdin`/`stdout`
NDJSON bridge. Visual fidelity is governed by `DESIGN.md` + `design-tokens.css`.

---

## 2. Architecture & Module Boundaries

- **`crates/speechtt-core`** — framework-free domain logic (no GPUI dependency).
  Hermetic, headless-testable via injected traits/fakes. Owns: session machine,
  audio capture, FFT, settings store, hotkeys, clipboard/paste/file output,
  NDJSON bridge protocol, backend client.
- **`crates/speechtt-ui`** — native `gpui-kit` application. Owns: window shell,
  view model (`controller.rs`), screens (`views/`), reusable components,
  theme/token mirror, dialogs.
- **`src/stt/`** — TypeScript/Bun worker. Runs as a subprocess; `stdout` is
  strictly NDJSON events, `stderr` is strictly human diagnostics.
- **`scripts/` + `tests/` (TypeScript)** — design-system tooling only; run
  with Bun, never `npm`/`npx`/`tsc`/`vitest`.
- **Shared registration files** (append-only across parallel tickets):
  `crates/speechtt-core/Cargo.toml`, `crates/speechtt-core/src/lib.rs`.

**Boundary rule**: `speechtt-core` must never depend on GPUI or any UI crate.
`speechtt-ui` may depend on `speechtt-core`. The worker must never import Rust.

**View rule**: each file under `crates/speechtt-ui/src/views/` exports exactly
one screen renderer, named `render(app, cx)` and named for the screen it draws
(`welcome.rs`, `session_setup.rs`, `ready.rs`, `recording.rs`, `paused.rs`,
`stopped.rs`). Never add a second screen or a shared fragment to a view file —
fragments used by more than one screen are components.

**Component rule (design system)**: visual decisions live in
`crates/speechtt-ui/src/components/` (guided by `DESIGN.md`), never in a view.
Introducing new text sizes, weights, colors, spacing, radii, shadows, or ad-hoc
styles directly in application/view code without Designer validation and explicit
approval in `DESIGN.md` is strictly prohibited. A view (`crates/speechtt-ui/src/views/`)
composes approved UI components and passes data plus intent; it must not choose
a font size, a weight, a colour, a radius, a padding, or a shadow. When a screen
needs a visual element or variant that does not exist yet:
1. Discuss and get explicit approval from the Designer/CEO to document it in `DESIGN.md`.
2. Implement it as a reusable component under `crates/speechtt-ui/src/components/` (with its own tests) — never by styling a raw `div` at the view call site.

Concretely:

- **Text** — every string is rendered by `components::Title`
  (`--font-size-lg`), `components::Body` (`--font-size-md`) or
  `components::Label` (`--font-size-sm`). Those three roles are the whole type
  scale: `.text_size(..)` and `.font_weight(..)` outside `components/text.rs`
  and `tokens.rs` are gate failures, enforced by
  `crates/speechtt-ui/tests/design_system.rs`. `Title`/`Body`/`Label` take the
  copy plus optional `.muted()`, `.tone(..)`, `.mono()`, `.bold()` (the single
  emphasis step above the role's weight) and `.center()`; there is no other
  knob.
- **Actions** — `components::ActionButton` (labelled), `components::IconButton`
  (window chrome), `components::IconAction` (square/compact),
  `components::RoundButton` (round affordance and transport),
  `components::RippleRing` (press wave).
- **Surfaces** — `components::Card` (clickable card), `layout::SurfaceBox`
  (recessed well), `layout::ScreenShell` (three-band screen).
- **Chrome & Indicators** — `components::AppHeader`, `components::AppFooter`,
  `components::StatusBadge`, `components::StatusDot`, `components::InfoNote`,
  `components::KeyCap`, `components::TranscriptionModeBadge`,
  `components::SettingRow`, `components::TransportHeader`,
  `components::StopControl`.
- Tokens are read from `crate::tokens` (or the theme roles they map to). A raw
  hex, `px(..)` spacing, `rems(..)` type or `radius` literal in a view is a
  gate failure (`tests/design_system.rs`) and a review blocker.

---

## 3. Tech Stack

- **Rust** (edition 2024): `speechtt-core`, `speechtt-ui` (gpui-kit 0.8).
- **TypeScript / Bun**: worker + design tooling. All JS/TS tooling uses Bun.
- **NDJSON** over piped `stdin`/`stdout` for the Rust ↔ Bun bridge.
- **Vercel AI SDK** (`ai`, `@ai-sdk/*`) for transcription providers.
- **Design tokens**: `design-tokens.css` is the single source of truth; mirrored
  in `crates/speechtt-ui/src/tokens.rs`.

---

## 4. Quality Gates (run before staging any ticket to `review/`)

### Rust
- `cargo fmt -- --check`
- `cargo clippy --all-targets -- -D warnings`
- `cargo test`

### TypeScript / Bun
- `bun test`
- `bun check src/stt`

### Design (when tokens or theme files are touched)
- `bun run design:validate`

All gates must be green with **zero warnings**. Do not weaken, skip, or
`#[ignore]` a test to make a gate pass.

---

## 5. Engineering Rules & Do/Don'ts

### Do
- Follow strict **TDD**: write failing tests first, then implement, then
  refactor. Tests target the `Contract` section of the ticket.
- Keep tests **hermetic and deterministic**: inject traits/interfaces/fakes;
  never require a real microphone, audio server, display server, clipboard,
  real user config directory (`$HOME`/`$XDG_CONFIG_HOME`), or live network
  unless explicitly gated/skipped per the ticket.
- Treat shared registration files (`Cargo.toml`, `lib.rs`) as **append-only**
  across parallel tickets — never rewrite or delete sibling lines.
- Use `SecretString` (Rust) / `apiKey` (TS) for credentials; persist settings
  files with mode `0600` on Unix.
- Load the relevant local skills from `.agents/skills/` per the ticket's track
  (see §6).
- Hand off final visual and animation review to the user (CEO/Designer) after
  all deterministic headless tests and design-system gates pass.

### Don't
- Don't write or edit production feature code directly when a coding sub-agent
  can be dispatched — the CTO architects, specifies, delegates, audits.
- Don't conduct autonomous visual QA by launching the GUI window on the user's
  display to take screenshots, record video bursts, or tune animations via
  snapshot scripts — verify via headless tests (`cargo test`) and let the user
  perform the visual review.
- Don't write **UI placement/layout tests**. Never assert positions, offsets,
  alignment, baselines, centring, spacing, sizes, or geometry of rendered
  elements. Visual layout is reviewed by the user, not by automated tests;
  automated tests cover behaviour, state transitions, and contracts only.
- Don't touch files outside a ticket's `Scope Boundary`.
- Don't leak secrets in `Debug`, `Display`, error messages, logs, or tests.
- Don't use `npm`, `npx`, `tsc`, or `vitest` — Bun only for JS/TS.
- Don't hardcode colors, font sizes, or spacing in UI code — read from
  `crate::tokens` / the theme.
- Don't commit `.env*` or any untracked local secrets.
- Don't silently expand a ticket's scope mid-flight; create a follow-up ticket
  in `tickets/planned/` instead.

---

## 6. Skill Discovery

Installed skills live in `.agents/skills/`. Coding and reviewing sub-agents must
load the skills matching their ticket's track. Common mappings:

- **Rust work** (`crates/speechtt-core`, `crates/speechtt-ui`): `rust-code-review`
- **GPUI UI work** (`crates/speechtt-ui`): `gpui-kit`, `gpui-kit-design-guides`
- **TypeScript/Bun worker** (`src/stt/`): `use-bun`, `ai-sdk`
- **Design tokens** (`DESIGN.md`, `design-tokens.css`, `tokens.rs`): `design-system-tokens`

Always inspect `<available_skills>` / `.agents/skills/` at the start of a task;
new skills may be installed.

---

## 7. Kanban Board Conventions

See `tickets/README.md` for the full board, wave plan, and conventions. Summary:

- Tickets live in `tickets/{planned,in_progress,review,done}/` as
  `TNNNN-short-kebab-title.md`.
- IDs are monotonically increasing and never reused (scan all folders for the
  next ID).
- Commit subjects reference the ticket ID: `<emoji> <type>: <summary> (TNNNN)`.
- A ticket in `review/` is **not** done until validated (or autonomously closed
  under delegated trust — see CTO contract).

---

## 8. Secret Safety (Non-Negotiable)

- API keys use `SecretString` (Rust) / `apiKey` (TS). Their `Debug`/`Display`
  must print `[REDACTED]`.
- Persisted settings files use mode `0600` on Unix.
- No key material in error messages, logs, test output, or commits.
- The `masked_api_key` helper (first 3 + `…` + last 2) is the only
  representation ever shown in the UI.