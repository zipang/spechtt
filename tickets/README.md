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

### Planned — Wave 1 (independent, parallelisable)

| Ticket | Title | Track | Primary files |
|---|---|---|---|
| [T0005](planned/T0005-core-audio-capture-and-fft.md) | Microphone capture and FFT waveform frames | Rust core | `crates/speechtt-core/src/audio.rs`, `fft.rs` |
| [T0006](planned/T0006-core-settings-and-shortcuts.md) | Settings store and global hotkeys | Rust core | `crates/speechtt-core/src/settings.rs`, `hotkeys.rs` |
| [T0007](planned/T0007-core-output-clipboard-paste-save.md) | Clipboard, paste and save-to-file adapters | Rust core | `crates/speechtt-core/src/output.rs` |
| [T0008](planned/T0008-bun-capabilities-and-live-smoke.md) | Capability handshake and live cross-language smoke test | Bun + Rust protocol | `src/stt/`, `crates/speechtt-core/src/protocol.rs`, `tests/bridge_smoke.rs` |

### Planned — Wave 2 (integration)

| Ticket | Title | Depends on |
|---|---|---|
| [T0009](planned/T0009-e2e-wiring-and-error-ux.md) | End-to-end wiring and error UX | T0004–T0008 |

## Parallel execution plan

```text
Wave 1
  Track A (Rust UI)   : T0004 ✓ ────────────┐
  Track B (Rust core) : T0005 → T0006 → T0007 ─┤
  Track C (Bun)       : T0008 ─────────────┤
                                           ▼
Wave 2                : T0009 (integration) → review → done
```

- **T0005, T0006 and T0007 are conflict-free in substance** (separate
  modules) but all append one line to `crates/speechtt-core/src/lib.rs` and
  one dependency block to `crates/speechtt-core/Cargo.toml`. Run them
  sequentially on the core track, or merge those two files last if they run
  in parallel. Never rewrite an existing line in either file from those
  tickets.
- **T0004** (UI) is done; **T0008** (Bun + `protocol.rs`) does not overlap
  with the core track, so both can run concurrently.
- **T0009** is the only ticket allowed to edit the UI composition root,
  `views/stopped.rs` and `views/dialogs/settings.rs` after Wave 1; start it
  only when T0004–T0008 are in `review/` or `done/`.
