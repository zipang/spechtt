# T0041 — Version bump per release, and the version in the header

- **Track**: Tooling (TypeScript/Bun) + Rust UI (header) + docs
- **Wave**: 2 (integration — touches the release pipeline and a shared component)
- **Depends on**: T0038 (the packaging pipeline and the `## Download` section this ticket extends), T0040 (the header's current content and the dialog it must not disturb)
- **Blocks**: nothing
- **References**:
    - `AGENTS.md` §2 (component rule), §3 (documentation in code), §5 (quality gates), §6 (Do/Don't), §8 (working-tree hygiene)
    - `DESIGN.md` §"Settings panel" and the header/app-chrome section
    - `scripts/package.ts` — `versionFromMetadata`, `runCli`, `parseArguments`, `USAGE`
    - `Cargo.toml` — `[workspace.package] version`, the single source the artifact name and the compiled binary both read
    - `crates/speechtt-ui/src/components/app_header.rs` — `AppHeader`
    - `crates/speechtt-ui/src/views/dialogs/help.rs:64` — the existing `SPEECHTT V<version>` label the new one mirrors
    - Official documentation — Bun: <https://bun.com/docs/llms-full.txt> (`Bun.write`, `Bun.spawnSync`)

## Context & Goal

Two things are missing around the release identity.

**The version never moves.** The workspace sat at `0.1.0` for five weeks while
the product changed substantially, and it only moved because a human remembered
to edit `Cargo.toml` by hand. Every rebuild of the AppImage is a new release as
far as a user is concerned, so the version must advance with it — otherwise two
different artifacts carry the same number, and the checksum is the only thing
telling them apart.

**The version is invisible where the app is used.** The Help dialog footer shows
`SPEECHTT V0.5.0` beside the gateway status, but the widget's own header shows
only the product name. A user reporting a problem has to open Help to say which
build they run.

Done means: `bun run package` advances the minor version itself, the version it
writes is the one compiled into the artifact and printed in the file name, a
test build can opt out explicitly, and the widget header shows the version in
the same mono muted style the Help footer already uses.

### Why the bump must precede the build

The version is compiled into the binary: the Help footer reads
`env!("CARGO_PKG_VERSION")`. Bumping the manifest _after_ `cargo build` would
produce `SpeechTT-0.6.0-x86_64.AppImage` containing a binary that reports
`0.5.0`. The order is therefore part of the contract, not an implementation
detail, and it is what the tests pin.

### What counts as a test build — decided

| Case                                 | Flag                  | Bumps?  | Why                                                                                                          |
| ------------------------------------ | --------------------- | ------- | ------------------------------------------------------------------------------------------------------------ |
| Full build and package               | _(none)_              | **yes** | This is the release.                                                                                         |
| Build and package, but not a release | `--no-bump` **(new)** | no      | Verifying the pipeline after editing `package.ts` must not consume a version.                                |
| Repackage existing artifacts         | `--no-build`          | **no**  | It reuses a binary compiled earlier; a new number would name an artifact whose binary reports the old one.   |
| Print the plan only                  | `--dry-run`           | no      | Nothing is produced. It prints the version it _would_ produce, so the bump is predictable before it happens. |

A failed build **restores** the manifest, so retrying after a build error does
not advance the version twice.

## Scope Boundary

May create or modify:

- `scripts/package.ts` — **modified**: `nextMinorVersion`, `bumpedManifest`,
  the `--no-bump` flag, the bump-then-build order, the restore-on-failure path,
  the `--dry-run` line naming the next version, and the updated `USAGE`.
- `scripts/package.spec.ts` — **modified**: tests for every rule above, through
  the existing `CliDeps` seam. No test runs `cargo`, `bun build`, `fetch` or
  `gh`.
- `crates/speechtt-ui/src/components/app_header.rs` — **modified**: an opt-in
  version slot, off unless asked for.
- `crates/speechtt-ui/src/app.rs` — **modified**: pass the version to the widget
  window's header only.
- `DESIGN.md` — the header description, and the version treatment as a role.
- `README.md` — the release procedure: bump, build, checksum, publish.
- `AGENTS.md` — one rule recording that an AppImage build advances the minor
  version, and which flags opt out.
- `tickets/README.md` — board row.

Out of scope — do **not** touch anything else, in particular:

- No change to the release command shape, the `appimagetool` invocation, the
  AppDir layout, the worker bundle or the checksum format.
- No CI, no tag creation, no `gh` execution from the script.
- No new visual component or token: the header version is
  `components::Label` with `.mono().muted()`, the same treatment the Help footer
  already uses (Contract §3).
- No version display in dialog windows: the Help footer already carries it, and
  showing it in that window's header too would duplicate it.
- No `git` operations from the script: it edits the manifest and tells the user
  what to commit.

## Contract

### §1 — Bumping

```ts
/// The next minor version: `0.5.0` → `0.6.0`.
///
/// The patch component resets to zero because a minor bump starts a new
/// feature line. A value that is not `major.minor.patch` with numeric
/// components throws a `UsageError` rather than guessing.
export function nextMinorVersion(version: string): string;

/// `manifest` with the `[workspace.package]` version replaced by `version`.
///
/// Only that section's `version` key changes: a dependency version or a
/// profile setting in the same file must be untouched. Returns `null` when the
/// section or its key is absent, so the caller reports the problem instead of
/// writing a manifest it does not understand.
export function bumpedManifest(manifest: string, version: string): string | null;
```

Rules:

- The version read at the start of the run is the _current_ one; the bump
  produces the _release_ one. `cargo metadata` is re-read after the write, and
  the artifact name comes from that result — so the name cannot drift from the
  manifest.
- The bump happens **before** `cargo build`, and only in the full-build path.
- On any failure after the write (worker bundle, cargo build, staging,
  download, `appimagetool`, checksum), the original manifest text is written
  back before the error is reported. A failure therefore leaves the tree as it
  was found.
- `--no-bump`, `--no-build` and `--dry-run` never write the manifest.
- `--dry-run` prints the release version it would use, so the number is visible
  before it becomes real.

### §2 — Interaction with the existing flags

```text
usage: bun run package [--dry-run] [--no-build] [--no-bump] [--arch <arch>]
```

- `--no-build` with `--no-bump` is the normal pair: repackaging existing
  artifacts, no version change. Using `--no-build` **without** `--no-bump` is
  not an error, but the bump is skipped and the run says so on one line, because
  the binary was not recompiled.
- `--dry-run` combined with `--no-bump` simply prints the current version as the
  release version.

### §3 — The header

`AppHeader` gains one opt-in slot:

```rust
/// Shows the product version after the title, in the mono muted treatment.
///
/// Off unless called: dialog windows keep a single title, because the Help
/// footer already carries the version and repeating it there would say it
/// twice in one window. The widget window passes
/// `concat!("V", env!("CARGO_PKG_VERSION"))`.
pub fn version(mut self, version: impl Into<SharedString>) -> Self;
```

- Rendered as `components::Label` with `.mono().muted()`, exactly the Help
  footer's treatment (`help.rs:64`), so one version string looks the same in
  both places.
- It sits between the title and the trailing actions, and it never displaces
  them.
- The version comes from `env!("CARGO_PKG_VERSION")` at the call site in
  `app.rs`, not from a literal, so it is the compiled version by construction.

### §4 — Documentation

- `README.md`'s release procedure becomes explicit: `bun run package` advances
  the minor version and the commit carries `Cargo.toml` and `Cargo.lock`; a
  build that must not consume a version uses `--no-bump`; publishing stays the
  `gh release create` command the script prints.
- `AGENTS.md` records the rule as part of the release workflow, including which
  flags opt out and why `--no-build` cannot bump.
- `DESIGN.md` describes the header's version slot and the treatment it uses.

## TDD Tasks

- [ ] **T41.1 — Version specs first**: red tests for `nextMinorVersion`
      (`0.5.0`→`0.6.0`, `1.9.3`→`1.10.0`, `0.0.0`→`0.1.0`, patch reset, and a
      `UsageError` for `1.2`, `0.5.0-rc1`, `x.y.z`, empty) and for
      `bumpedManifest` (replaces the workspace version, leaves a dependency
      version and a profile line untouched, returns `null` when the section or
      key is missing, tolerates extra whitespace).
- [ ] **T41.2 — Version implementation**: the two pure functions per §1.
- [ ] **T41.3 — CLI specs first**: red tests for the flag matrix — default run
      writes the bumped manifest and builds with the new version; `--no-bump`
      writes nothing; `--no-build` writes nothing and says the bump was skipped;
      `--dry-run` writes nothing and prints the release version; a failing
      `cargo build` restores the original manifest byte for byte; a failing
      `appimagetool` does too. Each asserts the manifest content the fake
      `writeFile` received.
- [ ] **T41.4 — CLI implementation**: the `--no-bump` flag, the bump-before-
      build order, the restore path, and the `--dry-run` line, per §1–§2.
- [ ] **T41.5 — Header specs first**: a render test asserting the widget header
      carries the version and that a dialog header does not, driven by the
      existing headless test fixtures. Controller-level element assertions
      only — never layout, position or size (AGENTS.md §6).
- [ ] **T41.6 — Header implementation**: the `version` builder per §3 and the
      call site in `app.rs` that passes `env!("CARGO_PKG_VERSION")`.
- [ ] **T41.7 — Docs**: `README.md` per §4, `AGENTS.md`, `DESIGN.md`,
      `tickets/README.md`.
- [ ] **T41.8 — Gates**: `cargo fmt -- --check`,
      `cargo clippy --all-targets -- -D warnings`, `cargo test`, `bun test`,
      `bun check src/stt`, `bun check scripts/package.ts scripts/package.spec.ts`,
      `bun run design:validate`. Zero warnings; nothing skipped or `#[ignore]`d.
- [ ] **T41.9 — Real release**: run `bun run package` once and confirm the
      artifact name, the checksum line, and that `--version`-bearing output in
      the binary agrees with the file name. Then run `bun run package
    --no-bump` and confirm the version did not move. Record both, and restore
      the intended release version in a single commit.

## Acceptance Criteria

1. `bun run package` advances the minor version in `Cargo.toml` before building,
   and the produced artifact's name and its compiled-in version agree.
2. `--no-bump`, `--no-build` and `--dry-run` never modify `Cargo.toml`.
3. A build that fails after the bump leaves `Cargo.toml` byte-identical to what
   it was before the run, proven for both a compile failure and a packaging
   failure.
4. `--no-build` without `--no-bump` says on one line that the bump was skipped
   and why, rather than silently keeping the version.
5. `--dry-run` names the version the build would produce.
6. The widget window's header shows `V<version>` in the mono muted treatment; no
   dialog window's header shows it.
7. `DESIGN.md` documents the header slot, and the design-system gate stays
   green: no new component, token, size, weight or colour.
8. `AGENTS.md` and `README.md` state the release rule and the opting-out flags.
9. Every gate in T41.8 is green with zero warnings, and no test asserts layout,
   position, size or geometry.

## Implementation Notes

_(added during work: the decisions taken, any deviation from the contract with
its reason, the exact gate output, and the T41.9 result.)_
