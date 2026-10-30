# T0019 — One view per file under `views/`

- **Track**: Rust UI (structural refactor, no behaviour change)
- **Wave**: 1 (independent)
- **Depends on**: T0017 (mode chooser, `Card`, `RoundButton`)
- **Blocks**: T0018 (it edits `welcome.rs` and `session_setup.rs`)
- **Owner**: CTO (sub-agent quotas exhausted), supervised by the board owner
- **Started**: 2026-10-30
- **References**: `AGENTS.md` §2, `crates/speechtt-ui/src/views/`

## Context & Goal

`views/setup.rs` holds two unrelated screens (the mode chooser and the
provider/model form) plus three private copy helpers, and `views/transport.rs`
holds three (ready, recording, paused) plus two shared fragments. Finding the
screen that is on display means knowing that "welcome" is spelled
`setup::render_mode`, which is exactly the kind of accidental coupling that
made T0017 slow to review.

Goal: every file under `views/` exports exactly one screen renderer, named for
the screen it draws. Shared fragments that two screens need become components.

## Scope Boundary

May create or modify:

- `crates/speechtt-ui/src/views/{mod,welcome,session_setup,ready,recording,paused,stopped}.rs`
- `crates/speechtt-ui/src/components/{mod,transport_header,stop_control}.rs`
- `crates/speechtt-ui/src/app.rs` (route table only)
- `README.md`, `AGENTS.md` (module map, if they name `views/setup.rs`)

Do **not** touch `crates/speechtt-core`, `src/stt`, `DESIGN.md`, or any test
expectation: this ticket changes file layout, not behaviour or pixels.

## Contract

- One screen renderer per file, all named `render(app, cx)`:
  `welcome`, `session_setup`, `ready`, `recording`, `paused`, `stopped`.
- `views/setup.rs` and `views/transport.rs` are deleted.
- Fragments used by more than one screen move to `components/`:
  - `TransportHeader::new(elapsed, progress, mode)` — elapsed/progress
    read-out plus the transcription-mode badge.
  - `StopControl::new(enabled).on_click(..)` — the square Stop action, inert
    while recording.
- `views/` contains no private copy helpers: the mode chooser's three copy
  helpers stay local to `welcome.rs` (T0018 replaces them with the text
  components).

## TDD Tasks

- [x] **T19.1 — Red: module surface**: a test asserts the view module list is
  exactly the six screens (guards against a second screen creeping back into
  a file).
- [x] **T19.2 — Split**: create the six files, move each renderer unchanged,
  rename to `render`.
- [x] **T19.3 — Components**: extract `TransportHeader` and `StopControl`,
  delete `views/transport.rs`.
- [x] **T19.4 — Rewire**: update `views/mod.rs` and the `app.rs` route table.
- [x] **T19.5 — Gates**: `cargo fmt -- --check`,
  `cargo clippy --all-targets -- -D warnings`, `cargo test`, `bun test`,
  `bun check src/stt`, `bun run design:validate`.
- [x] **T19.6 — Bare visual check**: the chooser and every transport screen
  still render identically (screenshot at 600x460).

## Acceptance Criteria

1. Each file under `views/` exports exactly one `render(app, cx)`.
2. The welcome screen is `views/welcome.rs`; session setup is
   `views/session_setup.rs`.
3. `views/setup.rs` and `views/transport.rs` no longer exist.
4. No behaviour, copy or pixel changes: the existing test suite passes
   untouched.
5. All gates green with zero warnings.

## Implementation Notes

Structure now:

```text
views/
├── mod.rs           # module list only
├── welcome.rs       # mode chooser
├── session_setup.rs # provider + model form
├── ready.rs         # mic / browse
├── recording.rs     # waveform + caption + pause/stop
├── paused.rs        # waveform + caption + resume/stop
├── stopped.rs       # finished take, copy/save/open
└── dialogs/         # one file per dialog window (+ shell)
```

- Every renderer is now `render(app, cx)`; `render_mode`, `render_form`,
  `render_ready`, `render_recording`, `render_paused`, `render_stopped` are
  gone. Routes in `app.rs` follow the screen names.
- Two fragments crossed screens, so they became components:
  `components::TransportHeader` (elapsed/progress read-out + mode badge) and
  `components::StopControl` (square Stop, inert while recording).
- `views/setup.rs` and `views/transport.rs` deleted. No behaviour, copy or
  pixel change; the whole existing test suite passed untouched.
- Docs: `AGENTS.md` §2 gained the "one screen per file" rule, `README.md`'s
  tree now lists the screens instead of "setup, transport, stopped".

Gates: `cargo fmt -- --check` clean, `cargo clippy --all-targets --
-D warnings` clean, `cargo test` 324 passed, `bun test` 206 passed,
`bun check src/stt` no type errors (23 files), `bun run design:validate`
66 tokens / 0 errors. Visual check at 600x460: chooser unchanged.
