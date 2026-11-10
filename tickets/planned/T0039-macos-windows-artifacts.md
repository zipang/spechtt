# T0039 — macOS and Windows artifacts

- **Track**: Tooling (TypeScript/Bun) + cross-cutting platform
- **Wave**: 2 (integration)
- **Depends on**: T0038 (the worker bundle, the staging/checksum/release pattern and the resolver layouts this ticket builds on), T0016 (the setup and README vocabulary)
- **Blocks**: nothing
- **Status**: **blocked** — three CEO decisions in §10 must be answered before any task starts
- **References**:
    - `AGENTS.md` §5 (quality gates), §6 (Do/Don't)
    - Official documentation — GPUI Kit:
        - <https://gpui-kit.com/docs/packaging.md> — the macOS `.app`/DMG and Windows ZIP/Inno Setup procedures, and the rule that each target is built **and tested on its own platform**.
        - <https://gpui-kit.com/docs/installation.md> — the macOS (Xcode CLT) and Windows (MSVC + Windows SDK + CMake) toolchains.
    - Official documentation — Apple:
        - <https://developer.apple.com/documentation/bundleresources/placing-content-in-a-bundle> — the `.app` directory layout and `Info.plist`.
        - <https://developer.apple.com/documentation/xcode/creating-distribution-signed-code-for-the-mac/> — Developer ID signing and hardened runtime.
        - <https://developer.apple.com/documentation/security/customizing-the-notarization-workflow> — `notarytool submit --wait`, staple, validate.
    - Official documentation — Microsoft:
        - <https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/packaging/> — Windows packaging options.
    - Official documentation — Inno Setup:
        - <https://jrsoftware.org/isinfo.php> — the installer definition.
        - <https://jrsoftware.org/ishelp/topic_setup_signeduninstaller.htm> — why signing the setup `.exe` alone does not sign its uninstaller.
    - `crates/speechtt-core/src/backend.rs` — `worker_candidates`, whose macOS and beside-the-executable entries T0038 already adds and tests
    - `crates/speechtt-ui/tests/launch.rs` — the headless launch gate, whose display-server handling is Linux-specific today

## Context & Goal

An AppImage is a Linux format. After T0038 a Linux user downloads one file and
runs it; a macOS or Windows user has nothing, and the source build is the only
path — with no Xcode/MSVC instructions tested, no icon, no bundle identity and
no installer.

Done means: the same Releases page carries a macOS `.dmg` and a Windows `.zip`
(or installer), each built on its own platform, each carrying the same
`worker.js` bundle T0038 produces, and each documented with its own runtime
requirements — `bun` everywhere, `ffmpeg` for file transcription only.

### The hidden cost: the test suite is Linux-shaped

This is the part most packaging tickets miss. `crates/speechtt-ui/tests/launch.rs`
removes `DISPLAY` and `WAYLAND_DISPLAY` to keep `gpui` headless, and asserts a
Linux settings path. `crates/speechtt-core` depends on `cpal` and `enigo`,
whose platform backends differ. Before any artifact exists, each platform must
answer one question: **does `cargo test` pass there at all?** Task T39.1 exists
to answer it and to record every failure, because a green suite on Linux says
nothing about a macOS build.

## Scope Boundary

May create or modify:

- `scripts/package.ts` — extend the CLI with `--target <linux|macos|windows>`
  dispatching to per-target staging functions; the Linux path keeps behaving
  exactly as T0038 left it.
- `scripts/package.spec.ts` — append tests for the new pure functions.
- `scripts/package.macos.ts` / `scripts/package.windows.ts` — **new** only if the
  platform-specific staging does not fit in the shared module without a
  conditional tangle; state the choice in `## Implementation Notes`.
- `packaging/speechtt.icns` — **new**, generated from `packaging/icon.svg`.
- `packaging/speechtt.ico` — **new**, generated from the same source.
- `packaging/speechtt.iss` — **new**, the Inno Setup definition.
- `crates/speechtt-ui/tests/launch.rs` — **modified** so the gate is meaningful
  on each platform it runs on, without weakening the Linux assertions.
- `crates/speechtt-ui/src/diagnostics.rs` — only if the launch handshake needs a
  platform-specific detail; the handshake itself must not be removed.
- `README.md` — one download row per platform and the per-platform runtime notes.
- `tickets/README.md` — board row.

Out of scope:

- No change to the Linux AppImage, its layout or its release assets beyond what
  the shared refactor necessarily touches.
- No Mac App Store, Microsoft Store, `.pkg`, MSIX or `.deb`.
- No auto-update: <https://gpui-kit.com/docs/auto-update.md> is a separate
  product decision.
- No change to the worker, the protocol, the UI or the design system beyond the
  icon rasterisation.
- No CI unless §10.2 says so.

## Contract

### §1 — CLI

```text
bun run package [--target linux|macos|windows] [--dry-run] [--no-build] [--arch <arch>]
```

`--target` defaults to the host platform. An artifact is only produced for the
platform that runs the command: there is no cross-build, because GPUI Kit
requires packaging on the target platform. `--target` naming a different
platform exits `2` with that sentence, rather than failing halfway through.

Shared pure functions stay shared and single-sourced from T0038:
`versionFromMetadata`, `archFrom`, `checksumLine`, `workerBundleArgs`, and a
`releaseCommand(version, artifacts[])` generalised to take a list.

### §2 — macOS

Staged as `dist/SpeechTT.app/`:

```text
SpeechTT.app/Contents/
├── Info.plist                         0644
├── MacOS/speechtt-ui                  0755
├── Resources/worker.js                0644
├── Resources/speechtt.icns            0644
└── Resources/LICENSE                  0644
```

`Info.plist` carries, at minimum:

| Key                                    | Value                                   |
| -------------------------------------- | --------------------------------------- |
| `CFBundleName` / `CFBundleDisplayName` | `SpeechTT`                              |
| `CFBundleIdentifier`                   | the §10.3 decision                      |
| `CFBundleExecutable`                   | `speechtt-ui`                           |
| `CFBundlePackageType`                  | `APPL`                                  |
| `CFBundleShortVersionString`           | the workspace version                   |
| `CFBundleVersion`                      | the workspace version                   |
| `CFBundleIconFile`                     | `speechtt`                              |
| `NSMicrophoneUsageDescription`         | a sentence naming why capture is needed |
| `NSHighResolutionCapable`              | `true`                                  |

- `worker.js` lands in `Contents/Resources/`, which is exactly the
  `../Resources/worker.js` candidate T0038 already resolves and tests; the app
  needs no code change to find it.
- The DMG is produced with `hdiutil create` from a staging folder; the AppImage
  size of the artifact is not a target, the `.dmg` is.
- Apple Silicon and Intel: build both, then either ship two DMGs named
  `SpeechTT-<version>-macos-<arch>.dmg` or one universal binary via `lipo`.
  §10.4 decides; the checksum and release-command shape follow the choice.

### §3 — Windows

Staged as `dist/windows/SpeechTT/`:

```text
SpeechTT/
├── speechtt-ui.exe
├── worker.js
├── speechtt.ico
└── LICENSE.txt
```

- `worker.js` sits beside the executable, which is the `<exe_dir>/worker.js`
  candidate T0038 already resolves and tests.
- Portable artifact: `SpeechTT-<version>-windows-<arch>.zip` via
  `Compress-Archive`.
- Optional installer: `packaging/speechtt.iss`, `AppId` stable across releases,
  uninstaller signed per Inno's documented workflow when a certificate exists.

### §4 — Signing and notarization

Both are conditional on §10.1:

- **macOS, signed path**: `codesign --force --timestamp --options runtime --sign
"<Developer ID Application: …>"`, verify with `codesign --verify --deep
--strict`, then `notarytool submit … --wait`, `stapler staple`, `stapler
validate`. A rejected notarization fails the run and prints the submission
  log.
- **macOS, unsigned path**: the README documents the Gatekeeper bypass
  (right-click → Open, or `xattr -d com.apple.quarantine`), and the release note
  says the build is unsigned. No step pretends otherwise.
- **Windows, signed path**: `signtool` on `speechtt-ui.exe` **before** staging,
  and on the setup `.exe` after Inno runs.
- **Windows, unsigned path**: the README names the SmartScreen prompt.

The script never asks for a password or a keychain passphrase on the command
line; credentials are supplied by the platform's own tooling.

### §5 — macOS runtime permissions

These are product behaviour, not packaging trivia, and each needs a testable
outcome:

- **Microphone**: without `NSMicrophoneUsageDescription` the process is killed
  on the first capture. The permission prompt is a first-run experience; the
  controller must render `AppError::AudioUnavailable` (or the closest existing
  variant) instead of panicking when the user denies it.
- **Accessibility**: `enigo` cannot synthesise the paste keystroke until the
  user grants Accessibility to the app. The failure must reach the error strip
  with a message that names System Settings → Privacy & Security →
  Accessibility. No new visual component is allowed: an existing error surface
  carries it.
- Both paths are verified by hand on a real Mac, and the ticket records what
  was observed. They cannot be faked by a headless test.

### §6 — Test suite per platform

- `cargo test` must pass on both platforms, or the failing tests are changed to
  express the platform's reality rather than deleted. The launch gate keeps its
  meaning: on macOS and Windows it must still prove the binary opens a window
  and stays alive, using whatever headless mechanism those platforms provide —
  and it must never open a window on the developer's desktop.
- Any test that cannot be made hermetic on a platform is reported to the CEO as
  a gap, not silently skipped.

### §7 — Icons

`packaging/speechtt.icns` and `packaging/speechtt.ico` are generated from
`packaging/icon.svg` with `iconutil` (macOS) and `magick` (any). Both are
committed assets, like `packaging/speechtt.png`, and share its composition; no
new colours or glyphs.

### §8 — README

One download line per platform in `## Download`, each stating its own
requirements: `bun` everywhere; `ffmpeg`+`ffprobe` for file transcription only;
a Vulkan-capable graphical session on Linux; macOS 15+ and Windows 10+ as the
supported floors. The build-from-source section gains the Xcode CLT and MSVC
prerequisites with the GPUI Kit URLs.

### §9 — Gates

`cargo fmt -- --check`, `cargo clippy --all-targets -- -D warnings`,
`cargo test`, `bun test`, `bun check scripts/package.ts scripts/package.spec.ts`,
run **on each platform**: Linux (already green), macOS, Windows. Zero warnings.

### §10 — Open decisions (blocking)

| #    | Decision              | Options                                                                                                                      | Recommendation                                                                  |
| ---- | --------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------- |
| 10.1 | Code signing          | (a) Apple Developer Program + Windows certificate; (b) unsigned with documented bypasses; (c) macOS signed, Windows unsigned | (b) first: it ships now and costs nothing; (a) when downloads justify it        |
| 10.2 | Build machines        | (a) a Mac and a Windows PC; (b) GitHub Actions `macos-latest` + `windows-latest`; (c) both, script first                     | (b) for reproducibility, or (c) if a local machine exists for the first release |
| 10.3 | Bundle / app identity | Stable strings for `CFBundleIdentifier` and Inno `AppId`                                                                     | `com.github.zipang.speechtt` / `SpeechTT.SpeechTT` — no domain is claimed       |
| 10.4 | macOS architectures   | (a) two DMGs (`x86_64`, `aarch64`); (b) one universal `.dmg`                                                                 | (b): one download, `lipo` merge, same size class                                |

## TDD Tasks

- [ ] **T39.1 — Platform survey first**: on a real Mac and a real Windows
      machine (or the §10.2 runners), run `cargo build --release -p speechtt-ui`,
      `cargo test`, `bun test`. On Windows, additionally run
      `.\scripts\setup.ps1 -Check` and `.\scripts\setup.ps1 -DryRun`, confirm
      each hands its flag through to `scripts/setup.ts` and propagates its exit
      code, and record the output. The full install run
      (`.\scripts\setup.ps1 -Yes`) is exercised only in a throwaway VM or
      container, never on the machine you keep. Record every failure verbatim
      in `## Implementation Notes`. This task produces **no code** and blocks
      the rest: a ticket that writes packaging for a suite that does not compile
      is guessing, and T0016's PowerShell bootstrap has never been executed
      anywhere (its Deviation 6 hands that check to this task).
- [ ] **T39.2 — Shared refactor specs first**: red tests for `--target`
      parsing, the host-mismatch exit `2`, `releaseCommand(version, artifacts[])`
      with two and three assets, and `stagePlanLinux` unchanged by the refactor.
- [ ] **T39.3 — Shared refactor**: implement §1 without altering the Linux
      artifact byte-for-byte beyond the version string.
- [ ] **T39.4 — macOS specs first**: red tests for `plistXml(version, bundleId)`
      (every key in §2 present, values escaped), `stagePlanMacos`, the DMG name,
      and the lipo decision from §10.4.
- [ ] **T39.5 — macOS implementation**: `.app` staging, `.icns` generation, DMG,
      signing/notarization per §10.1.
- [ ] **T39.6 — Windows specs first**: red tests for `stagePlanWindows`, the ZIP
      name, and the `.iss` contents rendered from a `innoDefinition(version)`
      function.
- [ ] **T39.7 — Windows implementation**: folder staging, `.ico`, ZIP, optional
      Inno installer with the signed-uninstaller step.
- [ ] **T39.8 — Platform tests**: make `cargo test` pass on macOS and Windows
      per §6, including the launch gate, and record the exact commands and
      results.
- [ ] **T39.9 — Permissions**: §5, verified by hand on a real Mac; the denial
      paths render an existing error surface, with a test where the controller
      allows one.
- [ ] **T39.10 — Docs and wiring**: `README.md` per §8, `tickets/README.md`
      board row, `package.json` scripts only if a new entry is needed.
- [ ] **T39.11 — Real artifacts**: build and download each artifact on a clean
      machine or VM, install, launch, dictate, transcribe a file, uninstall.
      Record sizes, checksums and the release URL.

## Acceptance Criteria

1. `bun run package` on macOS produces `dist/SpeechTT-<version>-macos-<arch>.dmg`;
   on Windows, `SpeechTT-<version>-windows-<arch>.zip` (and the installer when
   §10.1 asks for it). Each carries the same `worker.js` bundle.
2. Launching the macOS app on a clean Mac shows the window, prompts for the
   microphone once, and dictates; a denied microphone and a missing
   Accessibility grant each produce a readable error instead of a crash or a
   silent no-op.
3. Launching the Windows artifact on a clean Windows 10+ machine shows the
   window and dictates.
4. On both platforms the app finds `worker.js` from the packaged layout — proven
   by the same fake-`bun` marker test T0038 introduces, adapted to each layout.
5. `cargo test` and `bun test` pass on macOS and Windows, or every remaining
   failure is documented in the ticket as an accepted platform gap with the
   CEO's approval — never skipped silently.
6. The Releases page carries the Linux, macOS and Windows assets for the same
   version, each with a published SHA-256.
7. The README states each platform's requirements, honestly, including the
   Gatekeeper or SmartScreen prompt when the build is unsigned.
8. Every gate in §9 is green on every platform it applies to, with zero
   warnings.
9. `.\scripts\setup.ps1 -Check` and `.\scripts\setup.ps1 -DryRun` have been
   executed on the Windows machine; each passes its flag to `scripts/setup.ts`,
   its exit code matches that script's verdict for the same state, and the
   output is recorded in the ticket.

## Implementation Notes

_(added during work: the §10 decisions as answered, the T39.1 survey output,
artifact sizes and checksums, and the release URL.)_
