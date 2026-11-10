# T0038 — Build an installable AppImage and publish it on GitHub Releases

- **Track**: Tooling (TypeScript/Bun) + Rust core (worker resolution) + docs
- **Wave**: 2 (integration — touches the production spawn path, packaging and the README)
- **Depends on**: T0016 (both tickets edit `README.md`, `package.json` and `.gitignore`; T0016 must be committed and validated first, per AGENTS.md §8)
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
  exits `2` naming both versions. Never hardcode `0.1.0` in the script.
- `appImageFileName(version, arch)` → `SpeechTT-<version>-<arch>.AppImage`
  (`SpeechTT-0.1.0-x86_64.AppImage`).
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

- [ ] **T38.1 — Resolution specs first**: write `crates/speechtt-core/tests/worker_resolution.rs`
      red. The workspace has no `tempfile` dependency and does not gain one:
      exercise the rules with fake `exists` closures and path arithmetic only —
      no directory is created and no file is read. Cover the full order: the
      AppDir layout wins; the macOS `Resources` layout is second; a worker
      beside the executable is third; the Cargo fallback resolves; no candidate
      → `None`; a root-like `exe_dir` does not panic; the candidate list is
      ordered and joined onto `exe_dir`. Assert `WORKER_SCRIPT` is still
      `src/stt/worker.ts`.
- [ ] **T38.2 — Resolution implementation**: add `worker_candidates`,
      `resolve_worker_in`, `BackendError::WorkerMissing`, and rewire
      `spawn_default` per Contract §1. Existing `spawn_with` tests keep passing
      untouched.
- [ ] **T38.3 — Packaging specs first**: write `scripts/package.spec.ts` red for
      every pure function — `appImageFileName`, `archFrom`, `versionFromMetadata`
      (agreeing packages, mismatched packages, missing field),
      `desktopEntryLinux`, `appRunScriptLinux`, `stagePlanLinux`,
      `appimagetoolArgs`, `workerBundleArgs`, `releaseCommand`, `checksumLine`,
      plus `runCli` exit codes for `--dry-run`, `--no-build` with and without
      the artifacts, and the missing-tool path. No test may run `cargo`,
      `bun build`, `fetch` or `gh`. Linux is in each name on purpose: the
      macOS and Windows targets share the CLI and add their own staging
      function instead of rewriting this one.
- [ ] **T38.4 — Packaging implementation**: `scripts/package.ts` implementing
      Contract §2–§8, with `runCli(argv, deps)` and the `import.meta.main`
      guard used by `scripts/setup.ts`.
- [ ] **T38.5 — Icon and licence**: write `packaging/icon.svg` per §7, render
      `packaging/speechtt.png`, add `LICENSE` (MIT), and present the PNG to the
      CEO for approval **before** committing it.
- [ ] **T38.6 — Packaged-layout launch test**: append to
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
- [ ] **T38.7 — Wiring**: `package.json` `"package"` script, `.gitignore`
      `dist/`, `README.md` `## Download` + requirements, `tickets/README.md`
      board row.
- [ ] **T38.8 — Real artifact**: run `bun run package`, then verify from the
      artifact, not the build tree: extract it (`--appimage-extract`), inspect
      the tree against §3, `objdump -p` the payload binary for unexpected
      absolute build paths, and run the extracted `usr/bin/speechtt-ui` headless
      with a throwaway `HOME` until the `status=window-open` line appears. Record
      the AppImage size and the `sha256sum -c` result in `## Implementation Notes`.
- [ ] **T38.9 — Gates**: `cargo fmt -- --check`,
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

_(added during work: decisions, deviations, exact gate output, artifact size,
checksum, and the release URL.)_
