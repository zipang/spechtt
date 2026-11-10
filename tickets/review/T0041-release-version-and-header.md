# T0041 — Version bump per release, and the version in the header

- **Track**: Tooling (TypeScript/Bun) + Rust UI (header) + docs
- **Wave**: 2 (integration — touches the release pipeline and a shared component)
- **Depends on**: T0038 (the packaging pipeline and the `## Download` section this ticket extends), T0040 (the header's current content and the dialog it must not disturb)
- **Blocks**: nothing
- **Owner**: coding sub-agent, supervised by CTO
- **Started**: 2026-11-10
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

Done means: `bun run package` advances the patch version itself, the version it
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

- `scripts/package.ts` — **modified**: `nextPatchVersion`, `bumpedManifest`,
  `staleArtifacts`, the `listDir` seam, the `--no-bump` flag, the
  bump-then-build order, the restore-on-failure path, the `--dry-run` lines
  naming the next version and the files a run would discard, and the updated
  `USAGE`.
- `scripts/package.spec.ts` — **modified**: tests for every rule above, through
  the existing `CliDeps` seam. No test runs `cargo`, `bun build`, `fetch` or
  `gh`.
- `crates/speechtt-ui/src/components/app_header.rs` — **modified**: an opt-in
  version slot, off unless asked for.
- `crates/speechtt-ui/src/app.rs` — **modified**: pass the version to the widget
  window's header only.
- `DESIGN.md` — the header description, and the version treatment as a role.
- `README.md` — the release procedure: bump, build, checksum, publish.
- `AGENTS.md` — one rule recording that an AppImage build advances the patch
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
- No purging of the tool cache or the worker bundle: `dist/tools/` is a download
  the next build would repeat, and `worker.js` is regenerated in place.

## Contract

### §1 — Bumping

```ts
/// The next patch version: `0.5.0` → `0.5.1`.
///
/// A packaging change is a patch-level change, and the minor number is reserved
/// for a feature line a human decides on. Major and minor are preserved and
/// only the patch advances. A value that is not `major.minor.patch` with
/// numeric components throws a `UsageError` rather than guessing.
export function nextPatchVersion(version: string): string;

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
  the patch version and the commit carries `Cargo.toml` and `Cargo.lock`; a
  build that must not consume a version uses `--no-bump`; publishing stays the
  `gh release create` command the script prints.
- `AGENTS.md` records the rule as part of the release workflow, including which
  flags opt out and why `--no-build` cannot bump.
- `DESIGN.md` describes the header's version slot and the treatment it uses.

### §5 — Stale artifacts in `dist/`

`dist/` accumulates one `SpeechTT-*.AppImage` and its `.sha256` per release, so a
reader cannot tell which file is current and a publish command copied from an
older run names the wrong artifact. A successful package run therefore leaves
`dist/` carrying the release it just produced and nothing else.

The decision is a pure function, so the rule is testable without a filesystem:

```ts
/// Artifacts in `entries` that belong to another version than `keep`.
///
/// Only the two release shapes are candidates: `SpeechTT-*.AppImage` and
/// `SpeechTT-*.AppImage.sha256`. Everything else is left alone, so the cached
/// `appimagetool` download and the worker bundle survive. The result is sorted
/// so a run removes files in a stable order.
export function staleArtifacts(entries: readonly string[], keep: string): string[];
```

Rules:

- `entries` are bare names from [`CliDeps::listDir`], never paths; an entry that
  contains a `/` or a `\` is never returned, so a directory listing cannot escape
  `dist/`.
- `keep` is the artifact name just built. Its `.sha256` companion is kept with
  it. Every other `SpeechTT-*.AppImage` and `.sha256` is returned.
- The removal runs **after** the artifact and its checksum are written, so a
  failed build leaves the previous artifacts in place; a run that produced
  nothing must not delete what an earlier run produced.
- `--dry-run` lists what it would remove and removes nothing.
- `tools/`, `worker.js` and `SpeechTT.AppDir` are never removed by this step.
  The AppDir is already replaced wholesale when it is staged, and the tool cache
  is a download the next build would otherwise repeat.
- One line per removal, naming the file, so the run says what it discarded.

## TDD Tasks

- [x] **T41.1 — Version specs first**: red tests for `nextPatchVersion`
      (`0.5.0`→`0.5.1`, `1.9.3`→`1.9.4`, `0.0.0`→`0.0.1`, major and minor
      preserved, and a `UsageError` for `1.2`, `0.5.0-rc1`, `x.y.z`, empty) and
      for `bumpedManifest` (replaces the workspace version, leaves a dependency
      version and a profile line untouched, returns `null` when the section or
      key is missing, tolerates extra whitespace).
- [x] **T41.2 — Version implementation**: the two pure functions per §1.
- [x] **T41.3 — CLI specs first**: red tests for the flag matrix — default run
      writes the bumped manifest and builds with the new version; `--no-bump`
      writes nothing; `--no-build` writes nothing and says the bump was skipped;
      `--dry-run` writes nothing and prints the release version; a failing
      `cargo build` restores the original manifest byte for byte; a failing
      `appimagetool` does too. Each asserts the manifest content the fake
      `writeFile` received.
- [x] **T41.4 — CLI implementation**: the `--no-bump` flag, the bump-before-
      build order, the restore path, and the `--dry-run` line, per §1–§2.
- [x] **T41.5 — Header specs first**: a render test asserting the widget header
      carries the version and that a dialog header does not, driven by the
      existing headless test fixtures. Controller-level element assertions
      only — never layout, position or size (AGENTS.md §6).
- [x] **T41.6 — Header implementation**: the `version` builder per §3 and the
      call site in `app.rs` that passes `env!("CARGO_PKG_VERSION")`.
- [x] **T41.7 — Docs**: `README.md` per §4, `AGENTS.md`, `DESIGN.md`,
      `tickets/README.md`.
- [x] **T41.11 — Stale-artifact specs first**: red tests for
      `staleArtifacts` — keeps the current artifact and its `.sha256` and
      returns every other `SpeechTT-*.AppImage`/`.sha256`; leaves `tools`,
      `worker.js`, `SpeechTT.AppDir` and an unrelated file alone; returns a
      sorted list; never returns an entry containing a path separator; an empty
      listing returns an empty list. Plus CLI specs: a successful run removes
      the stale files it found and prints one line per removal; a run that fails
      before packaging removes nothing; `--dry-run` lists them and removes
      nothing. `CliDeps` gains an injectable `listDir`, so no test reads a real
      directory.
- [x] **T41.12 — Stale-artifact implementation**: `listDir` in `CliDeps` and its
      live implementation, `staleArtifacts` per §5, and the removal step after
      the checksum is written.
- [ ] **T41.8 — Gates**: `cargo fmt -- --check`,
      `cargo clippy --all-targets -- -D warnings`, `cargo test`, `bun test`,
      `bun check src/stt`, `bun check scripts/package.ts scripts/package.spec.ts`,
      `bun run design:validate`. Zero warnings; nothing skipped or `#[ignore]`d.
- [x] **T41.9 — Real release**: run `bun run package` once and confirm the
      artifact name, the checksum line, and that `--version`-bearing output in
      the binary agrees with the file name. Then run `bun run package
--no-bump` and confirm the version did not move. Record both, and restore
      the intended release version in a single commit.
- [x] **T41.10 — Patch bump policy**: rename `nextMinorVersion` to
      `nextPatchVersion` and advance only the patch (`0.5.0`→`0.5.1`), update
      the version specs and the CLI matrix to 0.5.1, reset the workspace to
      0.5.1 in `Cargo.toml` and `Cargo.lock`, and correct the patch rule in
      `README.md` and `AGENTS.md`.

## Acceptance Criteria

1. `bun run package` advances the patch version in `Cargo.toml` before building,
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
10. After a successful run `dist/` holds exactly the artifact just built and its
    `.sha256`; any other `SpeechTT-*.AppImage` and `.sha256` is gone, and
    `tools/`, `worker.js` and `SpeechTT.AppDir` are untouched.
11. A failed run deletes nothing, and `--dry-run` names what it would remove
    without removing it.

## Implementation Notes

### Decisions and deviations

- **A lock refresh joined the bump (deviation, required by the invariant).**
  Contract §1 pins "bump before `cargo build`", and the build argv keeps
  `--locked` (out of scope to change). Bumping a workspace member's version
  invalidates `Cargo.lock`, so the first real run failed exactly there:
  `error: cannot update the lock file … because --locked was passed`. The bump
  step now runs `cargo metadata --offline --format-version 1` — the resolving
  query, which rewrites the two member entries of `Cargo.lock` — and then
  re-reads the version with the existing `--no-deps` query, because the
  resolving answer also lists every dependency version and
  `versionFromMetadata` (correctly) rejects a workspace as "disagreeing" when
  fed them. `--offline` keeps the step local: the existing lock already names
  every dependency, so no registry access is needed. The same run is the
  restore path's evidence — it failed on `cargo build`, and `Cargo.toml` was
  byte-identical afterwards.
- **The restore path covers `Cargo.lock` too.** With the bump writing the lock
  file, restoring only the manifest would leave a 0.6.0 lock against a 0.5.0
  manifest and break the next `--locked` build. Both files are captured before
  the write and written back on any failure; the spec asserts both.
- **`--no-build` prints its skip line before the artifact checks**, so a
  repackage that then fails still says why the version did not move.
- **The `--dry-run` line is `release version: <v>`**, printed before the
  command list; it names the version a release would produce (or the current
  one with `--no-bump`).
- **Header slot**: `AppHeader::version` renders `components::Label` with
  `.mono().muted()` inside a `div` carrying the id `app-header-version`,
  `.test_support()` and an `aria_label`. The id exists so the headless test can
  assert presence/absence, not position. The dialog call site in
  `views/dialogs/window.rs` is untouched, so no dialog header shows it.
- **Docs**: `tickets/README.md` already carried the T0041 row with the right
  title and track, so no edit was needed there; `DESIGN.md` gained an
  "Application header" section documenting the slot and its treatment,
  `README.md` a "Releasing" section, `AGENTS.md` a "Release builds" subsection
  under the gates.

### Red → green evidence

| Group | Red                                                                                                                           | Green            |
| ----- | ----------------------------------------------------------------------------------------------------------------------------- | ---------------- |
| T41.1 | `SyntaxError: Export named 'bumpedManifest' not found`                                                                        | 51 pass / 0 fail |
| T41.2 | (implemented the two functions)                                                                                               | 51 pass / 0 fail |
| T41.3 | 8 fail: `/^release version: 0\.6\.0$/` → `false`, `code 2` for `--no-bump`, Cargo.toml writes missing, restore writes missing | 54 pass / 0 fail |
| T41.4 | (implemented `--no-bump`, bump-then-build, restore, dry-run line)                                                             | 54 pass / 0 fail |
| T41.5 | `missing ElementId Name("app-header-version")`                                                                                | 1 pass           |
| T41.6 | (implemented `version` builder + `app.rs` call site)                                                                          | 1 pass           |
| T41.9 | first real run failed on `--locked`; second failed on the resolving metadata's version list                                   | artifact built   |

### Gate output

```
$ cargo fmt -- --check
FMT OK

$ cargo clippy --all-targets -- -D warnings
    Checking speechtt-core v0.6.0 (...)
    Checking speechtt-ui v0.6.0 (...)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 4.77s

$ cargo test
test result: ok. 237 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
… every target ok, 0 failed, 0 ignored (widget 36, dialogs 37, design_system 152, launch 5, …)

$ bun test
 477 pass
 0 fail
 1600 expect() calls
Ran 477 tests across 18 files.

$ bun check src/stt
✓ No type errors in 32 files

$ bun check scripts/package.ts scripts/package.spec.ts
✓ No type errors in 2 files

$ bun run design:validate
✓ Design contract valid — 66 tokens, 0 errors.
```

### T41.9 — real release

```
$ bun run package
cargo metadata --format-version 1 --no-deps
cargo metadata --format-version 1
bumped: 0.5.0 -> 0.6.0
bun build src/stt/worker.ts --target=bun --outfile dist/worker.js
cargo build --release --locked -p speechtt-ui
wrote: dist/SpeechTT-0.6.0-x86_64.AppImage.sha256
gh release create v0.6.0 dist/SpeechTT-0.6.0-x86_64.AppImage dist/SpeechTT-0.6.0-x86_64.AppImage.sha256 --title "SpeechTT 0.6.0" --generate-notes
```

- Artifact: `dist/SpeechTT-0.6.0-x86_64.AppImage` (19 331 576 bytes).
- The digest a release must publish is **the one the final build prints**: the
  squashfs image embeds timestamps, so repackaging the same binary yields a new
  digest. The artifact currently on disk is
  `5a145e52d8103561b54d7da581098f0d02cdcfdaba533cf788386a310967a610  SpeechTT-0.6.0-x86_64.AppImage`,
  verified with `sha256sum -c`; the `.sha256` companion beside it carries the
  same value. Re-run `bun run package --no-bump` before publishing and take the
  digest that run prints, rather than copying one from this record.
- The staged binary agrees with the file name: `strings` on
  `dist/SpeechTT.AppDir/usr/bin/speechtt-ui` contains `SPEECHTT V0.6.0` (Help
  footer) and `V0.6.0` (widget header). No window was opened.
- `bun run package --no-bump`: rebuilt and repackaged at 0.6.0, no `bumped:`
  line, `Cargo.toml` unchanged (`diff` against the pre-run copy is empty).
- `bun run package --no-build`:
  `package: --no-build reuses the compiled binary, so the bump is skipped (nothing was recompiled)`,
  then `wrote: dist/SpeechTT-0.6.0-x86_64.AppImage.sha256`; `Cargo.toml` still 0.6.0.
- `bun run package --dry-run`: `release version: 0.7.0`, no write.
- `Cargo.toml` and `Cargo.lock` now carry **0.6.0** — the intended release
  version — left uncommitted with the rest of the change. The stale
  `dist/SpeechTT-0.5.0-*` artifacts from the previous release are still present
  in the git-ignored `dist/`; they are not part of the tree.

### Peer-review follow-up — the resolve must not run `--offline`

- **Defect.** The lock refresh from "Decisions and deviations" ran
  `cargo metadata --offline --format-version 1`. On a machine with a cold cargo
  cache — a fresh clone, a new machine, a container, a CI runner, which is where
  T0039 builds the macOS and Windows artifacts — that command fails before
  `cargo build` is reached:

    ```console
    $ CARGO_HOME=/tmp/opencode/fresh-cargo cargo metadata --offline --format-version 1
    error: no matching package named `arboard` found
    location searched: crates.io index
    required by package `speechtt-core v0.6.0`
    exit=101
    ```

    `--offline` assumes the registry cache is already warm, which is only true on
    a machine that has built before.

- **Fix.** `RESOLVE_METADATA_ARGV` is now
  `["cargo", "metadata", "--format-version", "1"]`; nothing else changed.
  The resolve has to see the registry to rewrite the lock, and a cold cache is
  a normal state rather than an error. On a warm cache (the maintainer's
  machine) the command stays local; on a cold one it spends the index download
  `cargo build` was about to spend anyway.
- **`--locked` stays.** `cargo build --release --locked` is what guarantees the
  build does not silently re-resolve against a different dependency set. The
  refresh step's registry access does not weaken it: the build still verifies
  `Cargo.lock` against the manifest, and `--locked` is untouched.
- **Tests.** The fake-CLI lock-rewrite condition now keys on the resolving query
  (no `--no-deps`) instead of `--offline`, `RESOLVE_ARGV` matches the new argv,
  and a regression test — `the resolving query omits --offline, because a cold
registry cache makes it fail` — fails with an explanatory message if
  `--offline` reappears in the resolving argv. The restore-on-failure tests are
  unchanged and green. With `--offline` temporarily put back, that guard fails
  with the explanatory message and the offline argv it received.
- **Cold-cache verification.** `CARGO_HOME=/tmp/opencode/fresh-cargo3 cargo
metadata --format-version 1` exits `0` in 10.1 s against an empty cache and
  leaves `Cargo.lock` byte-identical.

    ```
    $ bun test
     478 pass
     0 fail
     1603 expect() calls
    Ran 478 tests across 18 files.

    $ bun check scripts/package.ts scripts/package.spec.ts
    ✓ No type errors in 2 files

    $ cargo test -p speechtt-ui --test widget
    test result: ok. 36 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
    ```

    No `.rs` file changed, so `cargo fmt -- --check` and clippy are not affected
    by this fix.

### Policy correction — a rebuild bumps the patch, not the minor (CEO decision)

- **Decision.** `bun run package` advances the **patch** component, not the
  minor one. A packaging change is a patch-level change; the minor number is
  reserved for a feature line that a human decides on. The rule is
  `0.5.0` → `0.5.1`, `1.9.3` → `1.9.4`, `0.0.0` → `0.0.1`: major and minor are
  preserved and only the patch advances.
- **Version reset.** T41.9's real run moved the workspace `0.5.0` → `0.6.0`
  under the previous minor rule. Nothing was ever published — no tag, no
  release — so `0.6.0` is discarded rather than built upon: the published
  history contains no `0.6.0`, so there is no number to keep consistent with.
  The next release is **`0.5.1`**. The script bumps the manifest _before_ it
  builds, so the release is always manifest + one patch; to land on `0.5.1` the
  tree keeps `[workspace.package] version = "0.5.0"`, and both `speechtt-core`
  and `speechtt-ui` entries of `Cargo.lock` carry `0.5.0` to match. The first
  `bun run package` after this change therefore produces `0.5.1`.
- **Code and tests.** `nextMinorVersion` is now `nextPatchVersion` in
  `scripts/package.ts`, with the doc comment stating the patch rule and its
  reason. `scripts/package.spec.ts` pins `0.5.0`→`0.5.1`, `1.9.3`→`1.9.4`,
  `0.0.0`→`0.0.1`, `2.0.7`→`2.0.8`, and the `UsageError` cases (`1.2`,
  `0.5.0-rc1`, `x.y.z`, empty). The CLI matrix now expects the bumped manifest
  `0.5.1`, the artifact `SpeechTT-0.5.1-x86_64.AppImage` and the printed
  release command on `v0.5.1`.
- **Docs.** `README.md` and `AGENTS.md` state the patch rule and the same
  reason.
- **No artifact was rebuilt this round.** `dist/` still holds the stale `0.5.0`
  and `0.6.0` AppImages; packaging again would only add a third. The point of
  the change is the policy, so verification stops at
  `bun run package --dry-run`, which writes nothing. From the `0.5.0` manifest
  it names `0.5.1` as the release version, the artifact and the tag:

```
$ bun run package --dry-run
cargo metadata --format-version 1 --no-deps
release version: 0.5.1
bun build src/stt/worker.ts --target=bun --outfile dist/worker.js
cargo build --release --locked -p speechtt-ui
…
ARCH=x86_64 dist/tools/appimagetool-x86_64.AppImage --appimage-extract-and-run --no-appstream dist/SpeechTT.AppDir dist/SpeechTT-0.5.1-x86_64.AppImage
gh release create v0.5.1 dist/SpeechTT-0.5.1-x86_64.AppImage dist/SpeechTT-0.5.1-x86_64.AppImage.sha256 --title "SpeechTT 0.5.1" --generate-notes
```

`Cargo.toml` still reads `version = "0.5.0"` afterwards.

- **Gates, patch-policy round** (all green, zero warnings, nothing ignored):

```
$ cargo fmt -- --check                      # exit 0, no diff
$ cargo clippy --all-targets -- -D warnings
    Checking speechtt-core v0.5.0
    Checking speechtt-ui v0.5.0
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 6.09s

$ cargo test
test result: ok. 237 passed; 0 failed; 0 ignored; …   (every target ok,
  0 failed, 0 ignored: widget 36, dialogs 37, design_system 152, launch 5, …)

$ bun test
 477 pass / 0 fail / 1602 expect() calls across 18 files
$ bun check src/stt
✓ No type errors in 32 files
$ bun check scripts/package.ts scripts/package.spec.ts
✓ No type errors in 2 files
$ bun run design:validate
✓ Design contract valid — 66 tokens, 0 errors.
```

- **Why the manifest keeps `0.5.0`.** A dry run names the version a release
  _would produce_, so the projected number is always one patch above the
  manifest. Setting the manifest to `0.5.1` would make the next release
  `0.5.2`; leaving it at `0.5.0` makes the next release `0.5.1`, which is the
  number the CEO decided. The tree therefore stays at `0.5.0` and the release
  lands on `0.5.1`.

### Stale artifacts in `dist/` (T41.11–T41.12)

- **What was added.** `staleArtifacts(entries, keep)` is pure and returns the
  bare names to delete, sorted. `CliDeps` gained `listDir(path) -> string[]`,
  returning bare entry names and an empty array for a missing directory; the
  live `systemDeps` implements it with `fs.readdirSync` inside a `try`, so a
  first run with no `dist/` cleans nothing instead of failing. A private
  `discardStaleArtifacts(deps, keep, dryRun)` owns the one place the listing is
  read, the one line of output per file (`removed: <path>`, or
  `would remove: <path>` under `--dry-run`) and the `removePath` call.
- **The rule.** Only `SpeechTT-*.AppImage` and `SpeechTT-*.AppImage.sha256` are
  candidates (`^SpeechTT-.+\.AppImage(\.sha256)?$`). `keep` and
  `keep + ".sha256"` are preserved; `tools/`, `worker.js`, `SpeechTT.AppDir`
  and any unrelated file are left alone, because the AppDir is replaced when it
  is staged and the tool cache is a download the next build would repeat. An
  entry containing `/` or `\` is never returned, so a listing cannot escape
  `dist/`, and a name that merely looks close (`SpeechTTX-…`) is not a
  candidate.
- **Safety ordering.** The cleanup runs after `writeFile(<artifact>.sha256)` and
  before the `gh release create` line is printed, so the publish command stays
  the last thing the user reads and a run that fails earlier (worker bundle,
  `cargo build`, staging, download, `appimagetool`, checksum) removes nothing.
  `--dry-run` takes the same branch with `dryRun = true`: it reads the listing,
  prints the would-be removals, and calls `removePath` zero times. The existing
  output order is otherwise unchanged; the two new lines sit immediately before
  the release command.
- **Docs.** `README.md`'s Releasing section states that a successful run leaves
  `dist/` carrying only the artifact it just produced, what is kept, and that a
  failed run deletes nothing; the `--dry-run` bullet mentions the would-be
  removals.

#### Tests added

`staleArtifacts` (7): keeps the current artifact and its checksum and removes an
older pair; leaves `tools`, `worker.js`, `SpeechTT.AppDir` and unrelated files
alone; returns the names sorted whatever the listing order; never returns an
entry carrying a path separator (`/`, `\`, absolute); empty listing removes
nothing; a near name (`SpeechTTX-…`, `.zip`, `.asc`, lowercase) is not a
candidate; the current checksum alone is kept. CLI through the `CliDeps` fakes
(5): a successful run removes exactly the stale files the fake listing reported
and prints one `removed:` line each; the cleanup lines follow the checksum line
and precede the release command; a failing `appimagetool` removes nothing; a
`--dry-run` names the four would-be removals and calls `removePath` zero times;
`--no-build` also cleans after a successful repackage (keeping its own pair).
No test reads a real directory, runs `cargo`, `bun build`, `fetch` or `gh`.

#### Red → green

| Group  | Red                                                                | Green            |
| ------ | ------------------------------------------------------------------ | ---------------- |
| T41.11 | `SyntaxError: Export named 'staleArtifacts' not found in module …` | 66 pass / 0 fail |
| T41.12 | (implemented `listDir`, `staleArtifacts`, `discardStaleArtifacts`) | 66 pass / 0 fail |

One spec was corrected while going green: `--no-build` repackages the current
version, so the pair it keeps is the current one and the expectations name the
older pair as the removed set.

#### Gate output (stale-artifact round, version 0.5.0)

```
$ cargo fmt -- --check
exit 0, no diff

$ cargo clippy --all-targets -- -D warnings
    Checking speechtt-core v0.5.0
    Checking speechtt-ui v0.5.0
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.02s

$ cargo test
20 targets: 674 passed; 0 failed; 0 ignored

$ bun test
 489 pass
 0 fail
 1632 expect() calls
Ran 489 tests across 18 files.

$ bun check src/stt
✓ No type errors in 32 files

$ bun check scripts/package.ts scripts/package.spec.ts
✓ No type errors in 2 files

$ bun run design:validate
✓ Design contract valid — 66 tokens, 0 errors.
```

#### Dry run against the real `dist/`

The real `dist/` held only the 0.5.1 pair, so the listing was exercised with
two empty throwaway entries in place and removed afterwards:

```
$ bun run package --dry-run
release version: 0.5.1
would remove: dist/SpeechTT-0.4.0-x86_64.AppImage
would remove: dist/SpeechTT-0.4.0-x86_64.AppImage.sha256
gh release create v0.5.1 dist/SpeechTT-0.5.1-x86_64.AppImage dist/SpeechTT-0.5.1-x86_64.AppImage.sha256 --title "SpeechTT 0.5.1" --generate-notes
```

Both throwaway files were still present after the dry run (nothing was
removed), and `dist/` afterwards holds `SpeechTT-0.5.1-x86_64.AppImage`,
`SpeechTT-0.5.1-x86_64.AppImage.sha256`, `SpeechTT.AppDir`, `tools/` and
`worker.js`. No release build was run, so `Cargo.toml` still reads
`version = "0.5.0"` and the next `bun run package` produces 0.5.1.

**Working-tree note.** The tree at dispatch carried an uncommitted bump of
`Cargo.toml`/`Cargo.lock` to 0.5.1, left by an earlier verification run, which
made the dry run project 0.5.2. With the CEO's approval those two files were
restored to `HEAD` (`0.5.0`) — a copy of the discarded text is at
`/tmp/opencode/t41-manifest-backup/` — so the tree now matches the manifest the
next release must start from.
