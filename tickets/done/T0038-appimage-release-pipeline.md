# T0038 — Build an installable AppImage and publish it on GitHub Releases

- **Track**: Tooling (TypeScript/Bun) + Rust core (worker resolution) + docs
- **Wave**: 2 (integration — touches the production spawn path, packaging and the README)
- **Depends on**: T0016 (both tickets edit `README.md`, `package.json` and `.gitignore`; T0016's implementation is committed, so the shared files are free, per AGENTS.md §8). T0016 is still in `review/`: if CEO validation changes `README.md` while this ticket holds it, that edit waits for this ticket's commit.
- **Owner**: coding sub-agent, supervised by CTO
- **Started**: 2026-11-10
- **Blocks**: T0039 (macOS and Windows artifacts reuse this ticket's worker bundle, staging pattern, checksums and release command)
- **References**:
    - `AGENTS.md` §3 (docs describe the code as it is), §5 (quality gates), §6 (Do/Don't), §8 (working-tree hygiene)
    - `PRODUCT.md` §1 (the product the artifact delivers)
    - `DESIGN.md` §2 (brand roles) — the icon reads `--color-brand-primary` / `--color-on-primary`
    - Official documentation — AppImage:
        - <https://docs.appimage.org/packaging-guide/manual.html> — the AppDir layout, `AppRun`, the `appimagetool` invocation, and the "no hard-coded paths" rule.
        - <https://docs.appimage.org/packaging-guide/distribution.html> — publishing, naming and checksums.
    - Official documentation — GPUI Kit packaging:
        - <https://gpui-kit.com/docs/packaging.md> — "resolve them from the bundle location rather than the caller's working directory"; `objdump -p` for ELF requirements; verify the **downloaded** artifact, not the build directory.
    - Official documentation — Bun:
        - <https://bun.com/docs/llms-full.txt>, section **Single-file executable** (canonical page <https://bun.com/docs/bundler/executables>, which Bun's own `llms.txt` lists; that URL currently answers 404, so `llms-full.txt` is the page to read).
    - Official documentation — GitHub CLI:
        - <https://cli.github.com/manual/gh_release_create> — `gh release create [<tag>] [<filename>...]`, `--title <string>`, `--generate-notes`.
    - `crates/speechtt-core/src/backend.rs` — `BunSubprocess`, `WORKER_SCRIPT`, `BackendError`
    - `crates/speechtt-ui/tests/launch.rs` — the headless launch gate this ticket extends to the packaged layout
    - `scripts/setup.ts` / `scripts/setup.spec.ts` — the `CliDeps` seam and pure-function style the packaging script follows

## Context & Goal

SpeechTT exists only as a source tree. To run it a user must clone the
repository, install the Rust toolchain and the build libraries, run
`./scripts/setup.sh`, `bun install`, `cargo build` — and even then the widget
only works when the process is started **from the repository root**, because
`BunSubprocess::spawn_default()` spawns `bun run src/stt/worker.ts`, a path
relative to the current working directory (`crates/speechtt-core/src/backend.rs:24`).

Done means: a user opens the project's GitHub **Releases** page, downloads
`SpeechTT-<version>-x86_64.AppImage`, marks it executable, runs it, and the
widget opens and dictates — with no clone and no build.

### What the AppImage does and does not contain

The CEO chose a **lean** artifact, and the ticket must say so out loud rather
than imply a self-containment the file does not have:

| Component                   | Inside the AppImage?                                         | Consequence                                                                   |
| --------------------------- | ------------------------------------------------------------ | ----------------------------------------------------------------------------- |
| `speechtt-ui` (Rust binary) | yes                                                          | the widget itself                                                             |
| STT worker                  | yes, as a single-file Bun bundle (`dist/worker.js`, ~1.5 MB) | no `node_modules` tree, no `bun run src/stt/worker.ts`                        |
| `bun` runtime               | **no**                                                       | must be installed on the machine — the worker is executed by the system `bun` |
| `ffmpeg` + `ffprobe`        | **no**                                                       | live dictation needs neither; **file transcription** needs both               |
| GPU driver / Vulkan loader  | **no**                                                       | a Linux GPUI window needs a working graphical session and a Vulkan driver     |

Verified before writing this ticket: `bun build src/stt/worker.ts --target=bun`
produces a **1.47 MB** single file that, run as `bun dist/worker.js`, answers the
capabilities handshake with `{"type":"ready"}` followed by the `capabilities`
event. `bun build --compile` (a 106 MB standalone binary) was considered and
rejected by the CEO: the AppImage requires `bun` on the machine either way, so
the extra 104 MB buys nothing.

## Scope Boundary

May create or modify:

- `scripts/package.ts` — **new**: build, stage, package, checksum, print the
  release command. Pure functions plus a `CliDeps` seam, in the shape of
  `scripts/setup.ts`.
- `scripts/package.spec.ts` — **new**: `bun test` unit tests with fake
  dependencies; no test builds, downloads or spawns anything.
- `packaging/icon.svg` — **new**: the 256×256 icon source, written from the
  design tokens (see Contract §7).
- `packaging/speechtt.png` — **new**: the 256×256 PNG rendered from that SVG.
- `LICENSE` — **new**: MIT, holder `Christophe Desguez`, year 2026.
- `crates/speechtt-core/src/backend.rs` — **modified**: worker resolution
  replaces the CWD-relative constant as the production path.
- `crates/speechtt-core/tests/worker_resolution.rs` — **new**: hermetic tests
  for the resolution rules, with fake `exists` predicates and path arithmetic;
  they create no directory and read no file.
- `crates/speechtt-ui/tests/launch.rs` — **modified**: **append one test** that
  launches the binary from a staged AppDir layout, proving the packaged path
  resolution end to end. Do not modify the existing launch assertions.
- `package.json` — **append-only**: one `"package": "bun run scripts/package.ts"`
  script.
- `.gitignore` — **append-only**: one `dist/` entry.
- `README.md` — a `## Download` section and the runtime-requirements table
  update (Contract §9).
- `tickets/README.md` — board row.

Out of scope — do **not** touch anything else, in particular:

- No CI: no `.github/` directory, no workflow. The CEO chose a local script plus
  `gh release`.
- No `.deb`, tarball or macOS/Windows packaging: those are T0039. The resolver
  still learns their worker layouts (Contract §1) so that ticket adds a target
  instead of rewriting this one.
- No change to the worker's TypeScript sources, the protocol, the UI, the design
  system tokens, or `DESIGN.md`.
- No change to `scripts/setup.ts` / `setup.spec.ts` / `setup.sh` / `setup.ps1`.
- No `bun.lock`, `Cargo.toml` or `Cargo.lock` change.
- The script never runs `gh` itself: publishing stays an explicit human command.

## Contract

### §1 — Worker resolution (Rust, `speechtt-core`)

The CWD-relative constant stops being the production lookup. Add to
`crates/speechtt-core/src/backend.rs`:

```rust
/// Candidate worker locations for a process whose executable lives in
/// `exe_dir`, most specific first.
///
/// Every supported layout is listed so one resolver serves all of them: the
/// Linux AppDir (`usr/bin/speechtt-ui` → `usr/share/speechtt/worker.js`), a
/// macOS app bundle (`Contents/MacOS/speechtt-ui` → `Contents/Resources/worker.js`),
/// a folder where the worker sits beside the executable (Windows portable, flat
/// Linux), and a Cargo build tree (`target/{debug,release}/speechtt-ui` →
/// `<repo>/src/stt/worker.ts`) so a developer run needs no install step.
pub fn worker_candidates(exe_dir: &Path) -> Vec<PathBuf>;

/// Picks the first candidate `exists` accepts. `exists` is a parameter, not
/// `Path::exists`, so the rule is testable without a filesystem. Returns
/// `None` when no candidate exists; it never panics and never guesses.
pub fn resolve_worker_in(
    exe_dir: &Path,
    exists: impl Fn(&Path) -> bool,
) -> Option<PathBuf>;
```

`worker_candidates` returns, in this order:

1. `<exe_dir>/../share/speechtt/worker.js` — Linux AppDir (`usr/bin/../share/...`)
2. `<exe_dir>/../Resources/worker.js` — macOS app bundle (`Contents/MacOS/../Resources/...`)
3. `<exe_dir>/worker.js` — beside the executable: a Windows portable folder, or a flat Linux layout
4. `<exe_dir>/../../src/stt/worker.ts` — Cargo build tree
5. `<exe_dir>/../../../src/stt/worker.ts` — `target/<profile>/deps/` variants

Candidates 2 and 3 have no consumer in this ticket — the artifact is Linux
only — but the resolver is the one function every future platform will share,
and adding a layout later means re-testing the whole ordering. They cost two
lines now and are tested with the same fake `exists` predicate as the rest.

`BunSubprocess::spawn_default()` becomes:

```rust
pub fn spawn_default() -> Result<Self, BackendError> {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf));
    let candidates = exe_dir
        .as_deref()
        .map(worker_candidates)
        .unwrap_or_default();
    let script = candidates
        .iter()
        .find(|path| path.exists())
        .cloned()
        .ok_or(BackendError::WorkerMissing { searched: candidates })?;
    Self::spawn(&script)
}
```

`WORKER_SCRIPT` stays public and keeps its current value and doc, because
`crates/speechtt-core/tests/bridge_smoke.rs` and the Cargo fallback both use
`src/stt/worker.ts`; it is no longer the spawn path by itself.

New error variant, inside the existing `#[non_exhaustive]` enum:

```rust
#[error("could not find the STT worker; looked in: {searched:?}")]
WorkerMissing { searched: Vec<PathBuf> },
```

The `Display` output carries paths only — no key material, no environment dump.

`views`/`app.rs` need no change: a missing worker already becomes
`AppError::BackendUnavailable`, and the widget still opens.

### §2 — Worker bundle

```sh
bun build src/stt/worker.ts --target=bun --outfile dist/worker.js
```

- Output is one file. After the build, the script asserts the file exists and
  is non-empty; **it also asserts no bare package import survived** by scanning
  the emitted file for `from "ai"` / `from "@ai-sdk/`. A failure here means the
  bundle silently depended on `node_modules`, which the AppImage does not ship.
- The bundled file is executed as `bun run <path>`, unchanged from today's
  worker contract: stdin/stdout NDJSON, stderr diagnostics.
- `bun` absent from `PATH` is a **spawn failure at runtime**, surfaced as
  `AppError::BackendUnavailable`; the script's own dependency check is a
  separate, earlier failure with a readable message.

### §3 — Linux AppDir layout

Staged at `dist/SpeechTT.AppDir/`, exactly:

```text
SpeechTT.AppDir/
├── AppRun                                      0755
├── speechtt.desktop                            0644
├── speechtt.png                                0644  (256×256)
└── usr/
    ├── bin/speechtt-ui                         0755
    ├── share/speechtt/worker.js                0644
    ├── share/applications/speechtt.desktop     0644
    ├── share/icons/hicolor/256x256/apps/speechtt.png  0644
    └── share/licenses/speechtt/LICENSE         0644
```

`AppRun`, byte for byte:

```sh
#!/bin/sh
# Resolve the AppImage mount point, not the caller's working directory:
# the widget locates its worker relative to its own executable.
set -eu
HERE="$(dirname "$(readlink -f "$0")")"
exec "$HERE/usr/bin/speechtt-ui" "$@"
```

`speechtt.desktop`, byte for byte:

```ini
[Desktop Entry]
Type=Application
Name=SpeechTT
Comment=Floating speech-to-text widget
Exec=speechtt-ui
Icon=speechtt
Terminal=false
Categories=AudioVideo;Audio;Utility;
```

The icon file name at the AppDir root must be exactly `speechtt.png` so it
matches `Icon=speechtt` (AppImage's icon rule).

### §4 — Version, file name, checksum

- Version comes from `cargo metadata --format-version 1 --no-deps`, the
  workspace package version. All workspace packages must agree; a mismatch
  exits `2` naming both versions. Never hardcode the version in the script;
  the workspace is at `0.5.0` when this ticket starts, and the script must
  keep working at `1.0.0` without an edit.
- `appImageFileName(version, arch)` → `SpeechTT-<version>-<arch>.AppImage`
  (`SpeechTT-0.5.0-x86_64.AppImage`).
- `arch` defaults to the host: `x86_64` → `x86_64`, `aarch64` → `aarch64`;
  any other `uname -m` exits `2` with the unsupported value.
- The checksum file is `<AppImage>.sha256` whose single line is
  `<64-hex>  <file name>` (two spaces), the `sha256sum` format, so
  `sha256sum -c` accepts it.

### §5 — `appimagetool`

- Cached at `dist/tools/appimagetool-<arch>.AppImage`; downloaded only when
  absent, from
  `https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-<arch>.AppImage`.
- Invoked as
  `appimagetool --appimage-extract-and-run --no-appstream <AppDir> <out>` with
  `ARCH=<arch>` in the environment, because the tool is itself an AppImage and
  FUSE is not guaranteed. `--appimage-extract-and-run` and `--no-appstream` were
  verified against the continuous release's own `--help` output; `ARCH` selects
  the runtime and the output architecture.
- Non-zero exit fails the run and prints the tool's own stderr.

### §6 — CLI surface (`scripts/package.ts`)

```text
bun run package [--dry-run] [--no-build] [--arch <arch>]
```

| Flag         | Effect                                                                                                         |
| ------------ | -------------------------------------------------------------------------------------------------------------- |
| none         | full run: bundle worker, `cargo build --release --locked -p speechtt-ui`, stage, package, checksum             |
| `--dry-run`  | prints every command and every staged path, runs nothing, exits `0`                                            |
| `--no-build` | reuses an existing `target/release/speechtt-ui` and `dist/worker.js`; fails with exit `1` if either is missing |
| `--arch`     | overrides the detected architecture                                                                            |

Exit codes: `0` success, `1` a build/packaging step failed, `2` usage or
environment error (missing `cargo`/`bun`, version mismatch, unsupported arch).

Selection of the architecture, the version, the staging plan and the release
command are **pure functions** with their own tests — `stagePlanLinux` and its
companions are named for the target so a macOS or Windows staging function can
sit beside them without a rename — and the CLI is a thin `runCli(argv, deps)`
like `scripts/setup.ts`, where `deps` supplies `exec`, `fetch`, `exists` and
`readFile`.

### §7 — Icon

`packaging/icon.svg` — a 256×256 canvas, square, no corner rounding:

- background: `--color-brand-primary` (`#ea5514`), filling the canvas;
- foreground glyph: a microphone, filled `--color-on-primary` (`#000000`),
  centered, occupying roughly 55 % of the canvas;
- no text, no gradient, no shadow, no second colour.

`packaging/speechtt.png` is rendered from it with
`rsvg-convert -w 256 -h 256`, and must be exactly 256×256 (assert with
`ffprobe`; the render is part of the build, not a shipped step).

The CEO validates the rendered PNG before it is committed. If they ask for a
different radius, glyph proportion or colour, the ticket records the decision
and the SVG changes — no component or token changes.

### §8 — Release command

After a successful build the script prints, and **does not run**:

```text
gh release create v<version> dist/SpeechTT-<version>-<arch>.AppImage dist/SpeechTT-<version>-<arch>.AppImage.sha256 --title "SpeechTT <version>" --generate-notes
```

`releaseCommand(version, arch)` is a pure function and is asserted verbatim in
`scripts/package.spec.ts`. The script never authenticates, never pushes and
never creates a tag.

### §9 — README

A `## Download` section directly after `## Demo`, and the requirements table
gains the AppImage row:

- the download link points at
  `https://github.com/zipang/spechtt/releases/latest`;
- the run instructions are `chmod +x SpeechTT-*-x86_64.AppImage` then
  `./SpeechTT-*-x86_64.AppImage`, with the version wildcard rather than a
  literal version that rots;
- **plainly state** what the AppImage still requires: `bun` on `PATH` (the
  worker runs on it), `ffmpeg` + `ffprobe` for file transcription only, and a
  working graphical session with a Vulkan driver. No sentence may imply the
  AppImage is self-contained.
- the existing "build from source" flow stays; the download is the short path,
  not a replacement.

## TDD Tasks

- [x] **T38.1 — Resolution specs first**: write `crates/speechtt-core/tests/worker_resolution.rs`
      red. The workspace has no `tempfile` dependency and does not gain one:
      exercise the rules with fake `exists` closures and path arithmetic only —
      no directory is created and no file is read. Cover the full order: the
      AppDir layout wins; the macOS `Resources` layout is second; a worker
      beside the executable is third; the Cargo fallback resolves; no candidate
      → `None`; a root-like `exe_dir` does not panic; the candidate list is
      ordered and joined onto `exe_dir`. Assert `WORKER_SCRIPT` is still
      `src/stt/worker.ts`.
- [x] **T38.2 — Resolution implementation**: add `worker_candidates`,
      `resolve_worker_in`, `BackendError::WorkerMissing`, and rewire
      `spawn_default` per Contract §1. Existing `spawn_with` tests keep passing
      untouched.
- [x] **T38.3 — Packaging specs first**: write `scripts/package.spec.ts` red for
      every pure function — `appImageFileName`, `archFrom`, `versionFromMetadata`
      (agreeing packages, mismatched packages, missing field),
      `desktopEntryLinux`, `appRunScriptLinux`, `stagePlanLinux`,
      `appimagetoolArgs`, `workerBundleArgs`, `releaseCommand`, `checksumLine`,
      plus `runCli` exit codes for `--dry-run`, `--no-build` with and without
      the artifacts, and the missing-tool path. No test may run `cargo`,
      `bun build`, `fetch` or `gh`. Linux is in each name on purpose: the
      macOS and Windows targets share the CLI and add their own staging
      function instead of rewriting this one.
- [x] **T38.4 — Packaging implementation**: `scripts/package.ts` implementing
      Contract §2–§8, with `runCli(argv, deps)` and the `import.meta.main`
      guard used by `scripts/setup.ts`.
- [x] **T38.5 — Icon and licence**: write `packaging/icon.svg` per §7, render
      `packaging/speechtt.png`, add `LICENSE` (MIT), and present the PNG to the
      CEO for approval **before** committing it.
- [x] **T38.6 — Packaged-layout launch test**: append to
      `crates/speechtt-ui/tests/launch.rs` a test that builds a temporary
      AppDir-shaped tree (a **copy** of the real binary at `usr/bin/speechtt-ui`
      and a copy of the real worker at `usr/share/speechtt/worker.js`; a symlink
      is not enough, because `current_exe()` resolves it and the AppDir lookup
      would never be exercised), launches the binary through the existing
      `Sandbox` helper so `DISPLAY`/`WAYLAND_DISPLAY` are removed and
      `HOME`/`XDG_CONFIG_HOME` point at a throwaway directory, and asserts the
      `status=window-open` line still appears. A window-open line alone does not
      prove the worker was found — a missing worker is non-fatal — so the test
      also puts a **fake `bun`** first on `PATH` that appends its arguments to a
      marker file, and asserts the recorded path is the AppDir's
      `usr/share/speechtt/worker.js`. The test must never open a window on the
      user's desktop, must not spawn a real `bun`, and must kill and reap the
      child on every path, mirroring the existing helpers in that file.
- [x] **T38.7 — Wiring**: `package.json` `"package"` script, `.gitignore`
      `dist/`, `README.md` `## Download` + requirements, `tickets/README.md`
      board row.
- [x] **T38.8 — Real artifact**: run `bun run package`, then verify from the
      artifact, not the build tree: extract it (`--appimage-extract`), inspect
      the tree against §3, `objdump -p` the payload binary for unexpected
      absolute build paths, and run the extracted `usr/bin/speechtt-ui` headless
      with a throwaway `HOME` until the `status=window-open` line appears. Record
      the AppImage size and the `sha256sum -c` result in `## Implementation Notes`.
- [x] **T38.9 — Gates**: `cargo fmt -- --check`,
      `cargo clippy --all-targets -- -D warnings`, `cargo test`, `bun test`,
      `bun check src/stt`, `bun check scripts/package.ts scripts/package.spec.ts`,
      `bun run design:validate`. Zero warnings; nothing skipped or `#[ignore]`d.

## Acceptance Criteria

1. `BunSubprocess::spawn_default()` finds the worker in the AppDir layout, the
   macOS `Resources` layout, the beside-the-executable layout and the Cargo
   build tree, in that precedence, and reports `WorkerMissing` with the searched
   paths when it finds none — all proven by hermetic tests that never touch the
   real filesystem layout of `$HOME`.
2. `bun run package --dry-run` prints every command and staged path, runs
   nothing, and exits `0`; `--no-build` fails with exit `1` when the binary or
   the worker bundle is absent.
3. `bun run package` produces `dist/SpeechTT-<version>-x86_64.AppImage` and its
   `.sha256`; `sha256sum -c` succeeds on the pair; the AppImage is under
   **150 MB** (record the real size).
4. The AppImage contains exactly the tree in Contract §3, with the modes stated
   there, and its payload binary prints `[speechtt] startup status=window-open`
   when launched headless from the extracted AppDir. Launching the same
   extracted binary with a fake `bun` first on `PATH` records the AppDir's
   `usr/share/speechtt/worker.js` as the spawned script — the packaged path, not
   a repository path.
5. `bun dist/worker.js` (the packaged bundle) answers the capabilities handshake
   with `{"type":"ready"}`, and the emitted bundle contains no unresolved
   `from "ai"` / `from "@ai-sdk/` import.
6. The printed release command, run by the CEO or CTO, creates a GitHub release
   `v<version>` carrying the AppImage and its `.sha256`, and the README's
   download link resolves to it.
7. The README states, without ambiguity, that the AppImage requires `bun`,
   needs `ffmpeg`+`ffprobe` for file transcription only, and needs a Vulkan-
   capable graphical session.
8. `packaging/speechtt.png` is exactly 256×256 and was approved by the CEO.
9. Every gate in T38.9 is green with zero warnings, and the pre-existing launch
   gate still passes.

## Implementation Notes

### What landed

- `crates/speechtt-core/tests/worker_resolution.rs` (new) — 11 hermetic specs for
  `worker_candidates` / `resolve_worker_in` / `BackendError::WorkerMissing`. Fake
  `exists` closures and path arithmetic only; no directory created, no file read,
  no `tempfile` dependency added.
- `crates/speechtt-core/src/backend.rs` — `worker_candidates`,
  `resolve_worker_in`, the `WorkerMissing { searched }` variant, and
  `spawn_default` rewired to the caller-independent lookup. `WORKER_SCRIPT` keeps
  its value and its role as the Cargo fallback. The module header now describes
  the relative-to-executable spawn instead of the CWD-relative one.
- `scripts/package.ts` (new) — the whole pipeline behind `runCli(argv, deps)`
  with the `CliDeps` (exec/fetch/exists/readFile/writeFile/copyFile/makeDir/
  removePath/chmod/sha256/machine) seam, plus the pure functions
  `archFrom`, `appImageFileName`, `versionFromMetadata`, `appRunScriptLinux`,
  `desktopEntryLinux`, `stagePlanLinux`, `workerBundleArgs`, `appimagetoolArgs`,
  `checksumLine`, `releaseCommand`.
- `scripts/package.spec.ts` (new) — 36 unit tests; the harness fakes every
  machine capability, so no test runs `cargo`, `bun build`, `fetch` or `gh`, and
  none touches `dist/`.
- `packaging/icon.svg` + `packaging/speechtt.png` — 256×256, `#ea5514`
  background, `#000000` microphone glyph at ~55 % of the canvas, no text, no
  gradient, no second colour.
- `LICENSE` — MIT, `Copyright (c) 2026 Christophe Desguez`.
- `crates/speechtt-ui/tests/launch.rs` — one appended test,
  `the_packaged_appdir_finds_the_worker_beside_the_binary`, plus a
  `wait_for_marker` helper. Existing assertions untouched.
- `package.json` (one appended script), `.gitignore` (one appended `dist/` line),
  `README.md` (`## Download` after `## Demo`, AppImage row in the requirements
  table, project-tree and gate lists).

### Red → green evidence

| Group | Red                                                                                                                                                                                                                                                                                  | Green                                                                                       |
| ----- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------- |
| T38.1 | Against the ticket's base tree (`667c9d1`) × the new spec: `error[E0432]: unresolved imports … no 'worker_candidates' in 'backend'` and `error[E0599]: no variant named 'WorkerMissing'`.                                                                                            | `cargo test -p speechtt-core --test worker_resolution` → 11 passed.                         |
| T38.3 | Spec × base tree: `error: Cannot find module './package' from …/scripts/package.spec.ts`, 0 pass / 1 fail.                                                                                                                                                                           | `bun test scripts/package.spec.ts` → 36 passed; full `bun test` → 459 passed.               |
| T38.6 | The appended launch test × base `backend.rs`: `assertion left == right failed: the packaged widget must spawn the AppDir worker, not a repository path` — `left: "src/stt/worker.ts"` (the old CWD-relative lookup), `right: …/SpeechTT.AppDir/usr/bin/../share/speechtt/worker.js`. | `cargo test -p speechtt-ui --test launch` → 5 passed, the new test records the AppDir path. |

### T38.8 artifact facts (`bun run package`, x86_64)

- Worker bundle: `dist/worker.js`, 1 472 364 bytes (1.47 MB), `bun build
src/stt/worker.ts --target=bun --outfile dist/worker.js` — 134 modules.
- AppImage: `dist/SpeechTT-0.5.0-x86_64.AppImage`, **19 286 520 bytes
  (18.4 MB)** — well under the 150 MB ceiling. Built with `appimagetool
--appimage-extract-and-run --no-appstream` and `ARCH=x86_64`. Rebuilt once
  after the icon was revised, so this is the digest of the artifact that ships.
- Checksum: `dist/SpeechTT-0.5.0-x86_64.AppImage.sha256` =
  `087adb2f7c7b36b29424cbde0c52d85ca4ef47bc5b07fb794f63b0a540c00b68  SpeechTT-0.5.0-x86_64.AppImage`;
  `sha256sum -c` → `SpeechTT-0.5.0-x86_64.AppImage: OK`.
- Extracted tree (`--appimage-extract`) matches §3 with the stated modes:
  `AppRun` 0755, `speechtt.desktop` 0644, `speechtt.png` 0644 (256×256),
  `usr/bin/speechtt-ui` 0755, `usr/share/speechtt/worker.js` 0644,
  `usr/share/applications/speechtt.desktop` 0644,
  `usr/share/icons/hicolor/256x256/apps/speechtt.png` 0644,
  `usr/share/licenses/speechtt/LICENSE` 0644. Both icon copies are byte-identical
  to `packaging/speechtt.png` (`7f2bff97…`).
- `objdump -p usr/bin/speechtt-ui`: no `RPATH`/`RUNPATH` entry at all; `NEEDED`
  is only the usual system set (`libasound.so.2`, `libxcb.so.1`,
  `libxkbcommon.so.0`, `libxkbcommon-x11.so.0`, `libgcc_s.so.1`, `libm.so.6`,
  `libc.so.6`, `ld-linux-x86-64.so.2`). `strings` still contains
  `~/.cargo/registry/src/**` and `~/.rustup/**` paths — they are compiler and
  panic-location strings from dependencies, not runtime lookups, and the binary
  resolves nothing through them.
- Headless launch of the extracted payload (throwaway `HOME`/`XDG_CONFIG_HOME`,
  `DISPLAY`/`WAYLAND_DISPLAY` removed, fake `bun` first on `PATH`): stderr carried
  `[speechtt] startup status=window-open`, and the fake `bun` recorded
  `run` +
  `/…/squashfs-root/usr/bin/../share/speechtt/worker.js` — the AppDir path, not a
  repository path. Child killed and reaped.
- `bun dist/worker.js` with a `capabilities` command answers
  `{"type":"ready"}` then the `capabilities` event; `grep -c 'from "ai"'` and
  `grep -c 'from "@ai-sdk/'` on the bundle both return `0`.

### Deviations from the contract

1. **`--dry-run` runs one read-only command.** §6 says "runs nothing"; the version
   must come from `cargo metadata` rather than a hardcoded number, so a dry run
   executes that single read-only query (and prints it) and runs nothing else —
   no `bun build`, no `cargo build`, no download, no staging. The spec pins
   exactly that single call.
2. **`--no-build` requires `cargo`, not `bun`.** It still needs `cargo metadata`
   for the version, and it never bundles. A full run requires both tools; both
   absences exit `2` with a readable message.
3. **`appimagetool` adds `.DirIcon`.** The produced AppImage carries the §3 tree
   plus a `.DirIcon → speechtt.png` symlink at the root, which `appimagetool`
   creates itself. Removing it is not part of the documented tool behaviour.
4. **README fence repair.** The Development gate block and the project tree were
   fenced with four backticks in the working copy, and adding the
   `bun check scripts/package.ts scripts/package.spec.ts` line inside it left the
   fences unbalanced. Both blocks are back to ` ``` ` now; this is a
   documentation-only change inside the ticket's README scope.
5. **`tickets/README.md` board row needed no edit** — T0038 already has its
   In Progress row (title, track) from the board commit, and the ticket stays in
   `in_progress/` for the CTO's review.

### Gates (final run on the working tree)

```text
$ cargo fmt -- --check
(no output)                                     # OK

$ cargo clippy --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.33s

$ cargo test
test result: ok. 229 passed; 0 failed   (speechtt-core lib)
test result: ok. 5 passed;   0 failed   (bridge_smoke)
test result: ok. 14 passed;  0 failed   (session_machine)
test result: ok. 5 passed;   0 failed   (transcript_captures)
test result: ok. 11 passed;  0 failed   (worker_resolution)
test result: ok. 128 passed; 0 failed   (speechtt-ui lib)
test result: ok. 3 passed;   0 failed   (design_system)
test result: ok. 13 passed;  0 failed   (dialogs)
test result: ok. 37 passed;  0 failed   (end_to_end)
test result: ok. 14 passed;  0 failed   (error_ux)
test result: ok. 5 passed;   0 failed   (launch)
test result: ok. 96 passed;  0 failed   (session_flow)
test result: ok. 23 passed;  0 failed   (shortcuts)
test result: ok. 5 passed;   0 failed   (startup)
test result: ok. 4 passed;   0 failed   (theme_contract)
test result: ok. 35 passed;  0 failed   (widget)
(0 failed, 0 ignored anywhere)

$ bun test
 459 pass
 0 fail
 1530 expect() calls
Ran 459 tests across 18 files. [659.00ms]

$ bun check src/stt
✓ No type errors in 32 files [71.04ms]

$ bun check scripts/package.ts scripts/package.spec.ts
✓ No type errors in 2 files [22.73ms]

$ bun run design:validate
✓ Design contract valid — 66 tokens, 0 errors.
```

### Review round 1

An independent reviewer audited the diff against the contract, `AGENTS.md` and
the local skills, and reproduced the artifact and the gates itself. Verdict was
`POLISH_REQUIRED` with no blocker and no incomplete wiring. Every finding was
applied:

1. **Exit codes** (`scripts/package.ts`) — the catch-all returned `2` for every
   non-`UsageError`, so a failing staging copy, a rejected `appimagetool`
   download or a throwing checksum reported a usage error instead of a build
   failure. Non-usage exceptions now return `1`; `UsageError` keeps `2`. Four
   specs pin it (`a staging copy that throws exits 1, not 2`, `a failed
appimagetool download exits 1, not 2`, `a checksum step that throws exits 1,
not 2`, `an exec that throws while building exits 1, not 2`).
2. **The tested resolver is the production path** (`backend.rs`) —
   `spawn_default` called `resolve_worker_in` only in tests while duplicating the
   find loop; it now calls `resolve_worker_in(exe_dir, |p| p.exists())`, so the
   hermetic precedence specs exercise what ships.
3. **Architecture fallback** — `uname -m` failing fell back to `process.arch`
   (`x64`/`arm64`), which `archFrom` rejected as an unsupported architecture.
   `archFrom` now accepts both spellings for each architecture.
4. **Cached `appimagetool` mode** — `chmod 0755` ran only after a fresh
   download, so a tool cached by an interrupted run could be non-executable. It
   now runs unconditionally after the existence check.
5. **`--no-build` self-containment** — the bare-`ai`/`@ai-sdk` import check ran
   only after a build, so a hand-made `dist/worker.js` could ship broken. The
   `--no-build` branch runs the same check.
6. **Test quality** — the tautological `arch type` block is gone, and the
   bare-import assertion pins the exact emitted message instead of an
   alternation that could pass for the wrong reason.
7. **Launch test name** — renamed to
   `the_packaged_appdir_resolves_the_worker_through_the_usr_share_layout`,
   which names candidate 1 rather than candidate 3.
8. **Documentation** — `worker_candidates` now names all five layouts including
   the `target/<profile>/deps/` variant, and `spawn_default`'s doc states the
   positive rule instead of defending against the removed working-directory
   behaviour.

The icon was revised by the CEO after the first artifact: the microphone stem
grew from 26 to 28 px in `packaging/icon.svg`, and `packaging/speechtt.png` was
re-rendered from it with `rsvg-convert -w 256 -h 256`. The artifact was rebuilt
afterwards, and all three icon entries inside it (`.DirIcon`, root
`speechtt.png`, `usr/share/icons/…/speechtt.png`) hash to the new
`7f2bff97…31bbfd`.

### CEO validation

The CEO ran the built AppImage on the target machine: it launches. A
configuration problem surfaced during that run and is out of this ticket's
scope — it is filed separately rather than reopening this one. The packaging
work itself is accepted.

### Follow-up found while validating

Launching from the desktop menu put Settings on an endless "Loading providers…"
with a misleading CONNECTED badge. The cause was not the API key: the launch
environment carrying no `~/.bun/bin` on `PATH` made `bun` unresolvable, so the
worker never started and no catalogue ever arrived. The artifact behaved exactly
as designed — it requires a system `bun` — but the failure was undiagnosable
from inside the app, which is a product defect rather than a packaging one.
Filed as `tickets/planned/T0040-bun-runtime-and-settings-recovery.md`; no code
in this ticket changes.
