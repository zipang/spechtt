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
  audio capture, FFT, settings store, the keyboard command vocabulary
  (`actions`), clipboard/paste/file output, NDJSON bridge protocol, backend
  client.
- **`crates/speechtt-ui`** — native `gpui-kit` application. Owns: window shell,
  view model (`controller.rs`), screens (`views/`), reusable components,
  theme/token mirror, dialogs, and the startup seam (`startup.rs`).
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
(`welcome.rs`, `session_setup.rs`, `recording.rs`, `stopped.rs`; dialogs under
`views/dialogs/`). Never add a second screen or a shared fragment to a view
file — fragments used by more than one screen are components.

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
  (window chrome), `components::IconAction` (square/compact of any glyph),
  `components::RoundButton` (round affordance), `components::ToggleRoundButton`
  (a round button as an on/off switch, with pulse and wave while on),
  `components::RippleRing` (press wave).
- **Surfaces** — `components::Card` (clickable card), `layout::SurfaceBox`
  (recessed well), `layout::ScreenShell` (three-band screen).
- **Chrome & Indicators** — `components::AppHeader`, `components::ScreenFooter`
  (the three-zone screen footer: back left, primary centred, secondaries
  right), `components::AppFooter` (window and dialog chrome), `components::Card`,
  `components::StatusBadge`, `components::StatusDot`,
  `components::ElapsedTimer`, `components::Waveform`, `components::CaptionWell`,
  `components::InfoNote`, `components::KeyCap`,
  `components::TranscriptionModeBadge` (only where a model is chosen),
  `components::SettingRow`, `components::TransportHeader`.
- Tokens are read from `crate::tokens` (or the theme roles they map to). A raw
  hex, `px(..)` spacing, `rems(..)` type or `radius` literal in a view is a
  gate failure (`tests/design_system.rs`) and a review blocker.

---

## 3. Documentation in Code

Module and item docs describe the code **as it is**: what a piece is, what it
does, what its options are, and what a caller must know to use it. They are the
contract for the next reader, not a diary for this one.

### Write docs that answer "what is this and how do I use it?"

| Question the reader has           | What the doc covers                                           |
| --------------------------------- | ------------------------------------------------------------- |
| What is this thing?               | One sentence naming it, in the vocabulary of the domain.      |
| What does it do?                  | The behaviour, its states, its guarantees.                    |
| What can I pass?                  | Every builder/setter, what each one changes, and the default. |
| What must I not do?               | Constraints a caller would get wrong silently.                |
| Where does it live in the design? | The `DESIGN.md` section or token role it implements.          |

Rules:

- **Lead with the purpose in one line.** The first `//!` or `///` line is a
  complete sentence a reader can stop after.
- **Document the public surface, not the private one.** A `pub fn` gets a doc
  comment; a `fn` private to its module gets one only when the _why_ is not
  obvious from the code.
- **State invariants where they can be violated.** "Clamped to `0..=1`",
  "sends exactly one `Stop`", "never returns `NaN`" — these are contracts, and
  tests may pin them.
- **Name the visual source for anything themed.** A component points at the
  `DESIGN.md` section and the token roles it reads, so a reader can verify it
  against the design instead of guessing.
- **Component docs cover: purpose, states, tones/roles, motion, layout band.**
  If the component has an on/off or enabled/disabled distinction, both halves
  are described explicitly — that is what a caller most often gets wrong.
- **View docs cover: which screen, its bands (header/body/footer), which
  component owns each decision, and the transitions leaving it.** A view is a
  composition, so its doc is the composition, not the styling.

### Every file and every component says where it is used

The codebase grows by screens, components and layers, and the hardest question a
reader has is no longer "what is this?" but **"who calls this?"**. Answer it in
the file:

- **Every module header names its callers.** After the one-line purpose, list
  the screens and components that render, construct or depend on what the module
  defines — names, not vague prose: _"rendered by `views::recording` and
  `views::session_end`, wrapped by `layout::SurfaceBox`"_. A module nobody uses
  is either new or dead, and the header must make that visible.
- **Every public item states its role in the design.** A component says which
  band of which screen it fills; a helper says which call site depends on which
  invariant; a token reader says which `DESIGN.md` role it mirrors.
- **Every non-obvious decision states its reason.** Why a flag defaults to
  `true`, why an id must stay stable across frames, why a value is clamped —
  the reader cannot recover these from the code, and the next change breaks them
  silently if they are unstated.
- **Prefer the concrete noun to the abstraction.** "The scroll offset of the
  caption well" beats "the relevant state"; "the live tail, in brand accent"
  beats "the text".

A file whose header cannot name its callers, or a public item whose purpose is
only clear from its body, is a documentation defect and a review finding.

### Do not write change history into the code

Source docs are not a changelog. The Kanban ticket and the commit message carry
the history; a reader of the file only has the present.

**Never write:**

- what the code used to do — _"this used to render a pause glyph"_, _"the old
  screen lacked an escape hatch"_, _"an earlier version animated something
  nobody could see"_;
- what was removed, moved or renamed — _"the chip that used to ride along here
  was gone"_, _"the header no longer carries a New take button"_;
- why a _previous_ draft was wrong, or a defence against a misreading of the
  past — _"it is not a record control, it is a switch"_, _"rather than
  restyling it"_;
- ticket IDs, ticket titles or ticket paths — `(T0023)`, _"see
  `tickets/done/T…md`"_, _"T0009 will replace this"_;
- dated decisions and review history — _"CEO decision of 2026-10-31"_,
  _"approved during review"_;
- placeholder future tense for work that has since landed — _"T0006 will drive
  this"_, _"the store arrives later"_.

If a fact is only true in the past tense, it does not belong in the file. If a
reader needs it, it belongs in the ticket, in `DESIGN.md` (for a design rule) or
in `PRODUCT.md` (for a product decision).

**Write instead:** the rule itself, in the present tense, with the reason that
still applies — _"back always means one step towards the mode chooser"_ rather
than _"this used to be in the header"_.

### Scope of this rule

Applies to every doc comment and module header in `crates/**/*.rs`,
`src/stt/**/*.ts` and `scripts/**/*.ts`, and to the narrative sections of
`DESIGN.md` and `PRODUCT.md`. It does **not** apply to `tickets/**` (that is the
audit trail, and `Implementation Notes` / `Deviations` / `Review` exist to hold
exactly this history) or to `git` history.

### Reviewing documentation

A reviewer rejects a diff that adds any sentence of the forbidden kinds above,
even when the sentence is accurate. The fix is to delete it, not to rephrase
it: the code, the tests and the ticket already hold the truth.

---

## 4. Tech Stack

- **Rust** (edition 2024): `speechtt-core`, `speechtt-ui` (gpui-kit 0.9.1).
- **TypeScript / Bun**: worker + design tooling. All JS/TS tooling uses Bun.
- **NDJSON** over piped `stdin`/`stdout` for the Rust ↔ Bun bridge.
- **Vercel AI SDK** (`ai`, `@ai-sdk/*`) for transcription providers.
- **Design tokens**: `design-tokens.css` is the single source of truth; mirrored
  in `crates/speechtt-ui/src/tokens.rs`.

---

## 5. Quality Gates (run before staging any ticket to `review/`)

### Rust

- `cargo fmt -- --check`
- `cargo clippy --all-targets -- -D warnings`
- `cargo test` — includes the **startup gate** (`crates/speechtt-ui/tests/startup.rs`),
  which runs `startup::init_app` headlessly (so every keymap registration is
  parsed), asserts that `lib.rs::run` still calls the seam and registers nothing
  itself, and that the UI test fixture calls the seam instead of copying it.
- `cargo test` also includes the **launch gate**
  (`crates/speechtt-ui/tests/launch.rs`), which runs the built `speechtt-ui`
  binary **headless** — `DISPLAY` and `WAYLAND_DISPLAY` removed, so `gpui` opens
  no window — in a throwaway `$HOME`/`$XDG_CONFIG_HOME`, waits for the
  `[speechtt] startup status=window-open` line `run()` writes once its window is
  open, and fails if the widget exits before that line or within the settle
  window after it. It needs no display server, no audio server and no user
  configuration, so it runs on every machine and in CI.

### Release builds

`bun run package` is the release command. It advances the minor version in the
`[workspace.package]` table of `Cargo.toml` **before** `cargo build`, then
re-reads `cargo metadata`, so the artifact name and the version compiled into the
binary (`env!("CARGO_PKG_VERSION")`) agree. A failure after the write restores
the original manifest, so a retry does not consume a second version.

- `--no-bump` builds and packages the current version without writing the
  manifest; use it when verifying the pipeline rather than cutting a release.
- `--no-build` never bumps: it repackages the binary compiled earlier, and a new
  number would name an artifact whose binary reports the old one. Without
  `--no-bump` it prints one line saying the bump was skipped.
- `--dry-run` prints the release version and the full plan and writes nothing.

### TypeScript / Bun

- `bun test`
- `bun check src/stt`

### Design (when tokens or theme files are touched)

- `bun run design:validate`

All gates must be green with **zero warnings**. Do not weaken, skip, or
`#[ignore]` a test to make a gate pass.

---

## 6. Engineering Rules & Do/Don'ts

### Do

- Follow strict **TDD**: write failing tests first, then implement, then
  refactor. Tests target the `Contract` section of the ticket.
- **Read the official documentation of a library, never its source.** For any
  imported crate or package (`gpui-kit`, `gpui`, `gpui-base`, `gpui-component`,
  `ai`, `@ai-sdk/*`, …), the authority is its published documentation: docs.rs,
  the crate's own `docs/` tree, the installed project skill for it, or the
  upstream guide. Reading `~/.cargo/registry/src/**` or `node_modules/**` to
  work out an API is not a diagnosis — it is guessing at an implementation that
  may change under us. If the documentation does not answer the question, say
  so out loud instead of inferring from the code.
- **Report an API gap to the CEO immediately.** When a documented capability we
  need is missing (no context-menu hook, no clipboard entry point, no selection
  accessor, no event), stop and report what is missing, what we tried in the
  documentation, and which alternatives exist — before spending more time or
  inventing a workaround around a library boundary.
- Keep tests **hermetic and deterministic**: inject traits/interfaces/fakes;
  never require a real microphone, audio server, display server, clipboard,
  real user config directory (`$HOME`/`$XDG_CONFIG_HOME`), or live network
  unless explicitly gated/skipped per the ticket.
- **Every startup registration lives behind `crates/speechtt-ui/src/startup.rs`'s
  `init_app`**, `run()` wires and never registers, and the UI test fixture calls
  that seam instead of re-listing its steps. `tests/startup.rs` enforces the
  split, because a startup step reachable only from `gpui_kit::application().run`'s
  closure is parsed for the first time on the user's machine.
- **Startup keeps the launch handshake.** `run()` writes
  `diagnostics::log_launched()` once `open_window` has returned, and
  `tests/launch.rs` launches the built binary headless and fails when it exits
  before or just after that line. A change that opens the window by another route
  keeps the handshake. That gate must never open a window on the developer's
  desktop: it removes `DISPLAY` and `WAYLAND_DISPLAY` so `gpui` starts headless,
  and it is never `#[ignore]`d or made conditional on a display server.
- Treat shared registration files (`Cargo.toml`, `lib.rs`) as **append-only**
  across parallel tickets — never rewrite or delete sibling lines.
- Use `SecretString` (Rust) / `apiKey` (TS) for credentials; persist settings
  files with mode `0600` on Unix.
- Load the relevant local skills from `.agents/skills/` per the ticket's track
  (see §7).
- Hand off final visual and animation review to the user (CEO/Designer) after
  all deterministic headless tests and design-system gates pass.

### Don't

- Don't write or edit production feature code directly when a coding sub-agent
  can be dispatched — the CTO architects, specifies, delegates, audits.
- Don't grep, read or reverse-engineer the source of an imported dependency
  (`~/.cargo/registry/src/**`, `node_modules/**`) to learn how to use it. That
  is the one thing the official documentation exists for, and code-derived
  usage is a guess that the next dependency upgrade invalidates (§6 Do).
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
- Don't write change history, ticket IDs or past-draft reasoning into doc
  comments and module headers (§3). Describe the code as it is.
- Don't commit `.env*` or any untracked local secrets.
- Don't silently expand a ticket's scope mid-flight; create a follow-up ticket
  in `tickets/planned/` instead.

### Official documentation sources — the registry

A library's API is learned from one of these URLs and from nowhere else. The CTO
owns this table: adding a dependency adds its row, and a documentation link that
moves is corrected in the same change that notices it. A row is written in the
LLM-friendly form when the project publishes one (`llms.txt`, `llms-full.txt`,
page-level `.md`), because it is Markdown all the way down and costs a fraction
of the rendered HTML.

| Library                                                                                              | Official documentation                                                                                                                                            | Use                                                               |
| ---------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------- |
| `gpui-kit` (UI framework)                                                                            | index <https://gpui-kit.com/llms.txt> · full text <https://gpui-kit.com/llms-full.txt> · one page as Markdown, e.g. <https://gpui-kit.com/base/text-selection.md> | Start at `llms.txt`, then fetch the single page the ticket needs. |
| `gpui-kit::base`, `::component`, `::assets` (layers of the same framework)                           | <https://docs.rs/gpui-kit> · <https://docs.rs/gpui-base> · <https://docs.rs/gpui-component>                                                                       | Exact signatures and types when the guide is prose only.          |
| `ai` (Vercel AI SDK)                                                                                 | <https://ai-sdk.dev/llms.txt>                                                                                                                                     | The whole documentation set as Markdown.                          |
| `@ai-sdk/elevenlabs`                                                                                 | <https://ai-sdk.dev/providers/ai-sdk-providers/elevenlabs.md>                                                                                                     | Provider page.                                                    |
| `@ai-sdk/mistral`                                                                                    | <https://ai-sdk.dev/providers/ai-sdk-providers/mistral.md>                                                                                                        | Provider page.                                                    |
| `@ai-sdk/openai`                                                                                     | <https://ai-sdk.dev/providers/ai-sdk-providers/openai.md>                                                                                                         | Provider page.                                                    |
| `@ai-sdk/xai`                                                                                        | <https://ai-sdk.dev/providers/ai-sdk-providers/xai.md>                                                                                                            | Provider page.                                                    |
| Bun (runtime, test runner, bundler)                                                                  | <https://bun.com/docs/llms.txt>                                                                                                                                   | `bun test`, `Bun.*` APIs, tooling.                                |
| AppImage (spec + `appimagetool`)                                                                     | <https://docs.appimage.org/packaging-guide/manual.html> · <https://docs.appimage.org/packaging-guide/distribution.html>                                           | AppDir layout, `AppRun`, `appimagetool` flags, publishing.        |
| GitHub CLI (`gh`)                                                                                    | <https://cli.github.com/manual/gh_release_create>                                                                                                                 | Publishing the release that carries the AppImage.                 |
| `cpal`, `enigo`, `arboard`, `rustfft`, `base64`, `async-channel`, `serde`, `serde_json`, `thiserror` | `https://docs.rs/<crate>/latest/<crate>/`                                                                                                                         | docs.rs is where these crates publish their API.                  |

Rules that go with the table:

- **Copy the signature, not the impression.** A ticket quotes the documented
  signature, its argument and return types and the URL it came from, so the
  implementing agent types what the documentation says instead of inferring it.
- **A gap is a report, not a workaround.** If the documented surface does not
  give what we need, say which URL was consulted and which alternatives remain,
  and let the CEO/PM decide.
- **A version bump re-reads the documentation.** The row is the contract; the
  installed source is an implementation detail that changes without notice.

---

## 7. Skill Discovery

Installed skills live in `.agents/skills/`. Coding and reviewing sub-agents must
load the skills matching their ticket's track. Common mappings:

- **Rust work** (`crates/speechtt-core`, `crates/speechtt-ui`): `rust-code-review`
- **GPUI UI work** (`crates/speechtt-ui`): `gpui-kit`, `gpui-kit-design-guides`
- **TypeScript/Bun worker** (`src/stt/`): `use-bun`, `ai-sdk`
- **Design tokens** (`DESIGN.md`, `design-tokens.css`, `tokens.rs`): `design-system-tokens`

Always inspect `<available_skills>` / `.agents/skills/` at the start of a task;
new skills may be installed.

---

## 8. Kanban Board Conventions

See `tickets/README.md` for the full board, wave plan, and conventions. Summary:

- Tickets live in `tickets/{planned,in_progress,review,done}/` as
  `TNNNN-short-kebab-title.md`.
- IDs are monotonically increasing and never reused (scan all folders for the
  next ID).
- Commit subjects reference the ticket ID: `<emoji> <type>: <summary> (TNNNN)`.
- A ticket in `review/` is **not** done until validated (or autonomously closed
  under delegated trust — see CTO contract).

### Working-tree hygiene between tickets

The working tree is the only copy of uncommitted work, so nothing is worth
rewriting it for.

- **A file is committed before a new ticket modifies it.** Check
  `git status --short` for every file a ticket is about to touch. If one carries
  another ticket's uncommitted changes, commit that work **first, in the state it
  is in** — a ticket in `review/` counts, and a work-in-progress commit is fine.
  Only then start the new ticket.
- **Never rewrite a file to manufacture a commit boundary.** Splitting two
  entangled tickets by restoring, editing or regenerating the shared file risks
  the only copy of the work for a purely cosmetic gain in history. An imperfect
  commit costs nothing; a lost working tree costs the work.
- **When two tickets already share a file, commit them together.** Name both IDs
  in the subject (`(T0036, T0037)`) and say in the body which change belongs to
  which ticket, so the history still reads.
- **Ask before any destructive git operation** on uncommitted work — `restore`,
  `checkout -- <path>`, `stash`, `reset`, `clean`. State what would be lost and
  offer a single combined commit as the alternative.

The CTO enforces this before dispatching a ticket: a dirty file that belongs to
another ticket is a reason to stop and ask the CEO/PM, not to edit around it.

---

## 9. Secret Safety (Non-Negotiable)

- API keys use `SecretString` (Rust) / `apiKey` (TS). Their `Debug`/`Display`
  must print `[REDACTED]`.
- Persisted settings files use mode `0600` on Unix.
- No key material in error messages, logs, test output, or commits.
- The `masked_api_key` helper (first 3 + `…` + last 2) is the only
  representation ever shown in the UI.
