# Tickets — SpeechTT Kanban Board

Issues and feature tickets, kept for historical reference. Tickets are written
by the CTO agents after an interview with the CEO (the user): they explain
**why** a change is needed, then break the work down into a detailed,
test-first TODO list. Large changes group tasks into **Phases** that are
committed on completion.

Everything follows **Test Driven Development**: each ticket first describes
the behaviour as specifications (unit/integration tests), then the
implementation tasks, then the gates (`cargo fmt`, `clippy`, `cargo test`,
`bun test`, `bun check`).

## Kanban layout

```text
tickets/
├── README.md        # this board
├── planned/         # specified, ready to pick up
├── in_progress/     # currently being implemented (one owner per ticket)
├── review/          # implementation + gates green, awaiting review
└── done/            # reviewed, merged and verified
```

### Workflow

1. **Pick** a ticket from `planned/` and `git mv` it to `in_progress/`.
   Add the owner/agent and start date to the ticket header.
2. **Implement** red → green → refactor. Check off `[x]` tasks as they land;
   append an **Implementation Notes** section with decisions, deviations and
   exact verification commands/results.
3. **Move to `review/`** when all gates pass. The reviewer verifies the
   acceptance criteria and the notes, not just the diff.
4. **Move to `done/`** after review (and manual QA where the ticket asks for
   it).

Never edit a ticket that is in `in_progress/` unless you own it. If scope
changes mid-flight, note it in the ticket and split the remainder into a new
`planned/` ticket instead of silently expanding the current one.

### Ticket file naming

```text
TNNNN-short-kebab-title.md
```

IDs are stable and never reused. `TNNNN` matches the commit subject, e.g.
`✨ feat: implement Rust duplex audio/transcript bridge over NDJSON (T0002)`.

### Ticket template

```markdown
# TNNNN — Title

- **Track**: Rust UI / Rust core / TypeScript-Bun / cross-cutting
- **Wave**: 1 (independent) or 2 (integration)
- **Depends on**: ticket ids
- **Blocks**: ticket ids
- **References**: PRODUCT.md phase, mockups

## Context & Goal

Why this exists and what "done" means.

## Scope Boundary

Exact files/directories that may change; everything else is out of scope.

## Contract

Types, protocols, behaviours (tests are written against this section).

## TDD Tasks

- [ ] Tn.1 — failing spec first ...
- [ ] Tn.2 — implementation ...
- [ ] Tn.3 — gates ...

## Acceptance Criteria

Observable outcomes, not implementation details.

## Implementation Notes (added during work)

Decisions, deviations, verification output.
```

## Board

### Done

| Ticket                                                          | Title                                                                           | Track                                                     |
| --------------------------------------------------------------- | ------------------------------------------------------------------------------- | --------------------------------------------------------- |
| [T0001](done/T0001-ui-layout-and-components.md)                 | UI layout, reusable components, behavior-first tests                            | Rust UI                                                   |
| [T0002](done/T0002-rust-duplex-bridge.md)                       | Rust duplex audio & transcript bridge over NDJSON                               | Rust core + UI                                            |
| [T0003](done/T0003-bun-stt-worker.md)                           | Bun STT worker on the Vercel AI SDK over NDJSON                                 | TypeScript/Bun                                            |
| [T0004](done/T0004-ui-dialogs-help-and-settings.md)             | Help / Shortcuts and Settings as floating OS windows                            | Rust UI                                                   |
| [T0005](done/T0005-core-audio-capture-and-fft.md)               | Microphone capture and FFT waveform frames                                      | Rust core                                                 |
| [T0006](done/T0006-core-settings-and-shortcuts.md)              | Settings store and global hotkeys                                               | Rust core                                                 |
| [T0007](done/T0007-core-output-clipboard-paste-save.md)         | Clipboard, paste and save-to-file adapters                                      | Rust core                                                 |
| [T0008](done/T0008-bun-capabilities-and-live-smoke.md)          | Capability handshake and live cross-language smoke test                         | Bun + Rust protocol                                       |
| [T0009](done/T0009-e2e-wiring-and-error-ux.md)                  | End-to-end wiring and error UX                                                  | Cross-cutting                                             |
| [T0010](done/T0010-settings-key-integrity-and-reveal-ux.md)     | Settings key integrity and stored-key reveal UX                                 | Rust UI + core                                            |
| [T0011](done/T0011-dynamic-model-discovery-and-mode-badge.md)   | Dynamic model discovery and streaming mode badge                                | Bun worker + Rust core/UI                                 |
| [T0012](done/T0012-silence-aligned-batch-segmentation.md)       | Silence-aligned audio segmentation for batch transcription                      | Bun worker                                                |
| [T0013](done/T0013-window-geometry-memory.md)                   | Remember the window size and position                                           | Rust core + UI                                            |
| [T0014](done/T0014-help-model-catalogue-tab.md)                 | Help dialog model-catalogue tab and a single gateway provider                   | Bun worker + Rust core/UI                                 |
| [T0015](done/T0015-transcribe-local-media-file.md)              | Transcribe a local audio or video file                                          | Bun worker + Rust core/UI                                 |
| [T0017](done/T0017-mode-chooser-and-nameless-sessions.md)       | Mode chooser and nameless sessions                                              | Rust core/UI + design system                              |
| [T0018](done/T0018-typography-scale-and-round-button-polish.md) | Three-size typography scale and round-button polish                             | Rust UI + design system                                   |
| [T0019](done/T0019-one-view-per-file.md)                        | One view per file under `views/`                                                | Rust UI (refactor)                                        |
| [T0020](done/T0020-empty-takes-and-explicit-logged-errors.md)   | Silent empty takes and explicit, logged backend failures                        | Cross-cutting                                             |
| [T0021](done/T0021-consistent-footer-navigation.md)             | One consistent footer and a live-first navigation flow                          | Rust UI                                                   |
| [T0022](done/T0022-live-take-screen-record-control.md)          | Live take screen: microphone record control, cancel and reset                   | Rust UI + core + design                                   |
| [T0023](done/T0023-file-mode-setup-and-segmented-model.md)      | File sessions must choose a non-streaming model in the setup form               | Rust UI + core + Bun worker                               |
| [T0024](done/T0024-media-only-file-picker.md)                   | Accept only media files for file transcription                                  | Rust core + UI                                            |
| [T0025](done/T0025-file-transcription-view.md)                  | Dedicated transcription screen for file takes                                   | Rust UI (controller + views + components + design system) |
| [T0027](done/T0027-selectable-transcript-text.md)               | Selectable, copyable transcript text                                            | Rust UI (text + caption well + design)                    |
| [T0028](done/T0028-real-dropdown-transcription-selects.md)      | Real dropdown menus for the transcription provider and model                    | Rust UI (components + views + controller + design)        |
| [T0029](done/T0029-default-model-per-mode.md)                   | One default model per transcription mode                                        | Rust core + Rust UI + design                              |
| [T0030](done/T0030-clean-transcript-with-language-model.md)     | Clean the transcript with a language model, in a chosen language                | Cross-cutting (core + Bun worker + UI)                    |
| [T0033](done/T0033-single-cleanup-toggle.md)                    | One cleanup button that toggles between the repaired and original text          | Rust UI (controller + view + design)                      |
| [T0034](done/T0034-live-take-header-model-and-timer.md)         | Show the model and the timer on the live take screen from the start             | Rust UI (components + view + controller + design)         |
| [T0031](done/T0031-live-transcript-window-overlap.md)           | Split the partial and delta channels in the live transcript                     | Rust core (transcript) + Bun worker (diagnosis)           |
| [T0035](done/T0035-window-scoped-keyboard-shortcuts.md)         | Window- and state-scoped keyboard shortcuts, and a Help tab generated from them | Cross-cutting (core + UI + docs)                          |
| [T0036](done/T0036-gpui-kit-0-9-migration.md)                   | Migrate the UI to gpui-kit 0.9.1                                                | Rust UI (framework bump)                                  |
| [T0037](done/T0037-launch-smoke-gate.md)                        | A gate that launches the built binary                                           | Rust UI (test infrastructure)                             |
| [T0038](done/T0038-appimage-release-pipeline.md)                | Build an installable AppImage and publish it on GitHub Releases                 | Tooling (TypeScript/Bun) + Rust core + docs               |
| [T0041](done/T0041-release-version-and-header.md)              | Version bump per release, the version in the header, and `bun run release`      | Tooling (TypeScript/Bun) + Rust UI + docs                 |

### In Progress

_(none)_

### Review

| Ticket                                                     | Title                                                         | Track                           |
| ---------------------------------------------------------- | ------------------------------------------------------------- | ------------------------------- |
| [T0016](review/T0016-dependency-bootstrap-script.md)       | Dependency bootstrap for Bun and ffmpeg, and a welcome README | Tooling (TypeScript/Bun) + docs |
| [T0040](review/T0040-bun-runtime-and-settings-recovery.md) | Find `bun`, and never leave Settings as a dead end            | Rust core + Rust UI + docs      |

### Planned

| Ticket                                            | Title                                              | Track                                             | Depends on |
| ------------------------------------------------- | -------------------------------------------------- | ------------------------------------------------- | ---------- |
| [T0032](planned/T0032-language-reach-honesty.md)  | Say honestly what the dictation language can reach | Cross-cutting (worker → core → UI)                | T0030      |
| [T0039](planned/T0039-macos-windows-artifacts.md) | macOS and Windows artifacts                        | Tooling (TypeScript/Bun) + cross-cutting platform | T0038      |
| [T0042](planned/T0042-settings-density-and-order.md) | Settings fits, leads with what matters, and drops the empty choice | Rust UI + design system | T0040      |

**T0039 is blocked**: its contract §10 lists four decisions (signing, build
machines, bundle identity, macOS architectures) that the CEO must answer before
T39.1 starts.
