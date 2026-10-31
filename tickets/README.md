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

## Implementation Notes   (added during work)
Decisions, deviations, verification output.
```

## Board

### Done

| Ticket | Title | Track |
|---|---|---|
| [T0001](done/T0001-ui-layout-and-components.md) | UI layout, reusable components, behavior-first tests | Rust UI |
| [T0002](done/T0002-rust-duplex-bridge.md) | Rust duplex audio & transcript bridge over NDJSON | Rust core + UI |
| [T0003](done/T0003-bun-stt-worker.md) | Bun STT worker on the Vercel AI SDK over NDJSON | TypeScript/Bun |
| [T0004](done/T0004-ui-dialogs-help-and-settings.md) | Help / Shortcuts and Settings as floating OS windows | Rust UI |
| [T0005](done/T0005-core-audio-capture-and-fft.md) | Microphone capture and FFT waveform frames | Rust core |
| [T0006](done/T0006-core-settings-and-shortcuts.md) | Settings store and global hotkeys | Rust core |
| [T0007](done/T0007-core-output-clipboard-paste-save.md) | Clipboard, paste and save-to-file adapters | Rust core |
| [T0008](done/T0008-bun-capabilities-and-live-smoke.md) | Capability handshake and live cross-language smoke test | Bun + Rust protocol |
| [T0009](done/T0009-e2e-wiring-and-error-ux.md) | End-to-end wiring and error UX | Cross-cutting |
| [T0010](done/T0010-settings-key-integrity-and-reveal-ux.md) | Settings key integrity and stored-key reveal UX | Rust UI + core |
| [T0011](done/T0011-dynamic-model-discovery-and-mode-badge.md) | Dynamic model discovery and streaming mode badge | Bun worker + Rust core/UI |
| [T0012](done/T0012-silence-aligned-batch-segmentation.md) | Silence-aligned audio segmentation for batch transcription | Bun worker |
| [T0013](done/T0013-window-geometry-memory.md) | Remember the window size and position | Rust core + UI |
| [T0014](done/T0014-help-model-catalogue-tab.md) | Help dialog model-catalogue tab and a single gateway provider | Bun worker + Rust core/UI |
| [T0015](done/T0015-transcribe-local-media-file.md) | Transcribe a local audio or video file | Bun worker + Rust core/UI |
| [T0017](done/T0017-mode-chooser-and-nameless-sessions.md) | Mode chooser and nameless sessions | Rust core/UI + design system |
| [T0018](done/T0018-typography-scale-and-round-button-polish.md) | Three-size typography scale and round-button polish | Rust UI + design system |
| [T0019](done/T0019-one-view-per-file.md) | One view per file under `views/` | Rust UI (refactor) |
| [T0020](done/T0020-empty-takes-and-explicit-logged-errors.md) | Silent empty takes and explicit, logged backend failures | Cross-cutting |

### In Progress

_(none)_

### Review

_(none)_

### Planned

| Ticket | Title | Track | Depends on |
|---|---|---|---|
| [T0016](planned/T0016-dependency-bootstrap-script.md) | Dependency bootstrap for Bun and ffmpeg | Tooling (TypeScript/Bun) | — |
| [T0023](planned/T0023-file-mode-setup-and-segmented-model.md) | File sessions must choose a non-streaming model in the setup form | Rust UI + core + Bun worker | T0011, T0015, T0020 |
| [T0024](planned/T0024-media-only-file-picker.md) | Accept only media files for file transcription | Rust core + UI | T0015, T0023 |
| [T0021](planned/T0021-consistent-footer-navigation.md) | One consistent footer and a live-first navigation flow | Rust UI | T0017, T0019, T0020, T0023 |
| [T0022](planned/T0022-live-take-screen-record-control.md) | Live take screen: microphone record control, cancel and reset | Rust UI + core + design | T0020, T0021 |
