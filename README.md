# OPENCODE / LINUX SPEECH TO TEXT WITH ANY BACKEND

A Rust widget that should integrate with any Linux system to capture the audio stream, send it to a local or remote Model, and paste the resul in the clipboard or inside a text input.

## PROJECT STRUCTURE

```
speechtt/
├── Cargo.toml                  # Cargo workspace (edition 2024, dev profile tuned for gpui)
├── package.json                # Bun scripts: design checks + UI run/build/test
├── README.md                   # This file
├── PRODUCT.md                  # Product plan, phases, session workflow and open decisions
├── DESIGN.md                   # Design-system contract consumed by the token validator
├── design-tokens.css           # Source of truth for colors, typography, spacing, radii
├── color-variants.css          # Derived color variants (hover/active/foreground pairs)
├── reset.css                   # Base reset applied before the token stylesheet
│
├── crates/
│   ├── speechtt-core/          # Framework-free domain logic (no UI, no gpui)
│   │   ├── src/lib.rs          # Public module surface
│   │   ├── src/session.rs      # Session machine: Blank/Ready/Recording/Paused
│   │   └── tests/
│   │       └── session_machine.rs  # Transition rules, invariants, invalid transitions
│   │
│   └── speechtt-ui/            # Native gpui-kit application (single process)
│       ├── src/main.rs         # Binary entry point → speechtt_ui::run()
│       ├── src/lib.rs          # `run()`, window options, embedded icons and assets
│       ├── src/app.rs          # Window shell, workflow state and public commands
│       ├── src/controller.rs   # Pure session/view-model state (no GPUI window)
│       ├── src/layout.rs       # ScreenShell (3-band screen) + SurfaceBox card
│       ├── src/views/          # Screen composition: setup, transport, stopped
│       ├── src/components/     # Reusable RenderOnce controls (badge, pills, transport…)
│       ├── src/theme.rs        # Token → gpui-kit Theme mapping + bundled font loading
│       ├── src/tokens.rs       # Rust mirror of design-tokens.css
│       ├── assets/fonts/       # Embedded Inter + JetBrains Mono faces and OFL licenses
│       └── tests/
│           ├── common/mod.rs       # Shared headless-window helpers
│           ├── session_flow.rs     # Controller behavior tests (no window)
│           ├── theme_contract.rs   # CSS ↔ tokens ↔ Theme value contract
│           └── widget.rs           # Render smoke test across screens and window sizes
│
├── mockups/                    # Visual ground truth exported from Paper
│   ├── app-window/             # idle / recording / paused frames (.png + .tsx value dumps)
│   └── dialogs/                # settings / help frames (.png + .tsx value dumps)
│
├── scripts/                    # TypeScript design-system tooling (run with Bun)
│   ├── validate-design.ts      # Validates DESIGN.md + design-tokens.css
│   └── sync-contrast.ts        # Regenerates color variants / contrast pairs
├── tests/
│   └── design-contract.test.ts # Bun test wrapping the token contract
├── vendor/
│   └── design-tokens/          # Vendored design-system package (validator + contrast helpers)
│
├── .agents/skills/             # Installed agent skills (gpui-kit guides, design tokens, Bun…)
├── bun.lock                    # Bun dependency lockfile
└── Cargo.lock                  # Rust dependency lockfile
```

### What lives where

- **`crates/speechtt-core`** — the behavioral backbone. Pure Rust with no
  GPUI dependency, so the rules (session parameters, transport transitions,
  finished-take hand-off) are testable headlessly. Audio capture, FFT,
  clipboard, hotkeys and file save will land here in later phases.
- **`crates/speechtt-ui`** — everything native and visual. `controller.rs` owns
  the pure view model (session machine, setup draft, finished take); `app.rs`
  owns the window shell, state and command surface; `views/` composes each
  screen from `layout.rs` primitives and the reusable `components/` controls.
  `theme.rs` maps the token contract onto gpui-kit and embeds the fonts, so the
  binary carries its own typography.
- **Root design files** — `design-tokens.css` is the single source of truth
  for visual values; `DESIGN.md` describes their intent; `color-variants.css`
  and `reset.css` support the reference renderings. The UI never hardcodes
  colors or type sizes: it reads the mirrored tokens.
- **`mockups/`** — reference only, never imported at runtime. `.png` files are
  the visual ground truth; `.tsx` exports are value dumps consulted when a
  measurement is ambiguous.
- **`scripts/` + `tests/` (TypeScript)** — tooling around the design system,
  run with Bun (`bun run design:validate`, `bun run design:contrast`,
  `bun test`). They do not ship with the widget.
- **`.agents/skills/`** — instructions loaded by coding agents working on this
  repository; not part of the product.

Build artifacts (`target/`, `node_modules/`) are generated and git-ignored.
The workspace deliberately stays **one process**: the UI and the core live in
the same binary, with no background daemon.

## LOCAL MODELS

## REMOTE MODELS PROVIDERS

### VERCEL AI GATEWAY

Vercel AI Gateway offers several streaming Speech transcription models with a unified API through their Vercel AI SDK (for Typescript): 

* mistral/voxtral-mini-transcribe-3
* spacexai/grok-stt

The list of models is activated on detection of the VERCEL_AI_GATEWAY_API_KEY environment variable. 

### MISTRAL AI

Mistral offers
