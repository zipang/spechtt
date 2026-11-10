# T0016 — Dependency bootstrap for Bun and ffmpeg, and a welcome README

- **Track**: Tooling (TypeScript/Bun) + docs
- **Wave**: 1 (independent)
- **Depends on**: nothing (T0015 consumes the result: an installed ffmpeg)
- **Blocks**: nothing
- **Owner**: coding sub-agent, supervised by CTO
- **Started**: 2026-11-09
- **References**:
    - `AGENTS.md` §3 (Bun only), §5 (quality gates), §6 (Don'ts)
    - `PRODUCT.md` §1–§3 (vision, users, functional requirements)
    - `DESIGN.md` (the design-system contract the README points at)
    - `https://gpui-kit.com/docs/installation.md` — the per-platform build
      requirements the README summarises, and the one URL it cites for them
    - `tickets/done/T0015-transcribe-local-media-file.md` — the platform matrix
      behind the `ffmpeg` requirement
    - `tickets/done/T0035-window-scoped-keyboard-shortcuts.md` — the shortcut
      table the README documents (`crates/speechtt-ui/src/shortcuts.rs`)

## Context & Goal

SpeechTT needs two binaries that are not part of the Rust workspace:

| Binary               | Used by                                         | Present by default              |
| -------------------- | ----------------------------------------------- | ------------------------------- |
| `bun`                | the STT worker subprocess (`src/stt/worker.ts`) | no — user installs              |
| `ffmpeg` + `ffprobe` | file decoding (T0015)                           | Linux: often; macOS/Windows: no |

Today a missing binary surfaces as an opaque failure (`spawn bun: No such file`,
or T0015's `ffmpeg_missing`), leaving the user to work out the install command
themselves. Done means a single documented command verifies and, after explicit
confirmation, installs exactly what is missing — idempotently, without ever
running `sudo` silently, and with the same logic on Linux, macOS and Windows.

**Scope is runtime dependencies only** (`bun`, `ffmpeg`, `ffprobe`). The Rust
toolchain and build libraries (ALSA, xkbcommon, Wayland/X11, fontconfig) stay
out: developers already have them to build, and a `--dev` matrix would double
the distro surface for no end-user benefit.

### Second half: the README

`README.md` is the repository's front door and it no longer describes the
repository. It stops mid-sentence ("Mistral offers"), never says how to install
or run anything, and its tree has drifted: `startup.rs`, `shortcuts.rs`,
`diagnostics.rs`, `session_transcription.rs`, `media.rs`, `src/stt/` and seven
UI test files are missing, while `views/ready.rs`, `paused.rs` and `stopped.rs`
no longer exist. It also documents three separate providers (Vercel, Mistral,
…), but the worker knows exactly one: `vercel-ai-gateway` in
`src/stt/registry.ts`, through which Mistral is a _model_, not a provider.

Done means a README a newcomer can follow end to end — install, build, first
run, keys, shortcuts — that is factually true of the code in this repository,
and that hands off to `PRODUCT.md`, `DESIGN.md` and `AGENTS.md` for depth
instead of duplicating them. A recorded demo of the widget leads it.

## Scope Boundary

May create or modify:

- `scripts/setup.ts` — **new**: cross-platform plan/confirm/apply logic.
- `scripts/setup.spec.ts` — **new**: `bun test` unit tests with a fake executor.
- `scripts/setup.sh` — **new**: POSIX bootstrap (~15 lines).
- `scripts/setup.ps1` — **new**: PowerShell bootstrap (~15 lines).
- `package.json` — **append-only**: one `"setup": "bun run scripts/setup.ts"` script.
- `README.md` — **rewritten** as a welcome document: demo, what it does,
  requirements, getting started, first run, shortcuts, project layout,
  development gates, documentation map (see §5).
- `demo-launcher-vignette.png` — **new**: the committed poster frame (753×495) the
  README's `## Demo` section wraps in a link to the video.
- `.gitignore` — **append-only**: one `recording.mp4` entry, so the demo video
  cannot be committed by accident. Touched outside the original scope on the
  CEO's decision that the video is a GitHub attachment, not a repository file.

The video is **not** a repository file: the CEO uploads it in GitHub's README
editor and supplies the resulting `user-attachments` URL, and `recording.mp4`
therefore sits in `.gitignore`, never appears in `git status`, and no `docs/`
tree exists for it. The poster frame **is** a repository file:
`demo-launcher-vignette.png` is committed at the root and is what the README
shows, linked to that URL.

Do **not** touch: anything under `src/`, `crates/`, `tests/`, `DESIGN.md`,
`design-tokens.css`, `.agents/`, or the T0015 ticket. No new npm dependency,
no `npm`/`npx`/`tsc` invocation. The script never writes into the repository.

`README.md` is documentation, so the rules of `AGENTS.md` §3 apply to it in the
present tense: it describes what the repository **is**, with no changelog, no
ticket IDs, no "used to". Where a fact lives in another document, the README
links to it rather than restating it.

## Contract

### 1. Core (`scripts/setup.ts`)

Pure, injectable, no top-level side effects when imported (so tests never run a
package manager):

```ts
export type PackageManager =
	| "apt"
	| "dnf"
	| "pacman"
	| "zypper"
	| "apk" // Linux
	| "brew" // macOS
	| "winget"; // Windows

export type DependencyId = "bun" | "ffmpeg" | "ffprobe";

export interface DependencySpec {
	readonly id: DependencyId;
	/** Argv used to read the version (`["--version"]` / `["-version"]`, matching reality). */
	readonly versionArgs: readonly string[];
	/** Lowest acceptable version. */
	readonly minimum: readonly [number, number, number];
}

export interface DependencyState {
	readonly id: DependencyId;
	readonly present: boolean;
	readonly version: readonly [number, number, number] | null;
	readonly status: "ok" | "missing" | "outdated";
}

export interface InstallStep {
	readonly id: DependencyId;
	/** Argv lists executed in order; each runs through `exec`. */
	readonly commands: readonly (readonly string[])[];
	readonly requiresSudo: boolean;
}

export interface SetupPlan {
	readonly platform: {
		os: "linux" | "macos" | "windows";
		manager: PackageManager | null;
	};
	readonly states: readonly DependencyState[];
	readonly steps: readonly InstallStep[];
}

export function detectManager(
	platform: NodeJS.Platform,
	has: (bin: string) => boolean
): PackageManager | null;

export function parseVersion(raw: string): readonly [number, number, number] | null;

export function evaluate(
	spec: DependencySpec,
	probe: { present: boolean; versionOutput: string | null }
): DependencyState;

export function plan(states: readonly DependencyState[], manager: PackageManager | null): SetupPlan;

export function commandsFor(
	id: DependencyId,
	manager: PackageManager
): readonly (readonly string[])[];
```

Rules:

- `detectManager` probes in a fixed documented order and is a pure function of
  `platform` + `has` (tests inject both). Unknown platform ⇒ `null`.
- `parseVersion` accepts `bun 1.4.4`, `ffmpeg version 7.1.2-static`, and
  `ffprobe version n9.0.2` — the first three dot-separated integers found;
  `null` when nothing parses.
- `outdated` is a separate status from `missing`, but both produce a step.
- Minimum versions: **bun 1.4.0** (worker uses `Bun.stdin.stream` and Bun 1.4
  behaviour), **ffmpeg/ffprobe 6.0.0**.
- Steps are ordered `bun` → `ffmpeg` → `ffprobe`, deduplicated: installing
  ffmpeg also provides ffprobe, so **one** package step covers both when either
  is missing, and no step is emitted when both are `ok`.
- Unknown manager with a missing dependency ⇒ **no step**, but an
  `instructions` string is returned so the CLI can print the manual command
  (never guess a package manager).

Command table (asserted verbatim in tests):

| Manager | bun                                                              | ffmpeg (+ffprobe)                                                                           |
| ------- | ---------------------------------------------------------------- | ------------------------------------------------------------------------------------------- |
| apt     | `curl -fsSL https://bun.sh/install \| bash`                      | `sudo apt-get install -y ffmpeg`                                                            |
| dnf     | same curl line                                                   | `sudo dnf install -y ffmpeg`                                                                |
| pacman  | same curl line                                                   | `sudo pacman -S --needed --noconfirm ffmpeg`                                                |
| zypper  | same curl line                                                   | `sudo zypper install -y ffmpeg`                                                             |
| apk     | same curl line                                                   | `sudo apk add ffmpeg`                                                                       |
| brew    | `brew install oven-sh/bun/bun`                                   | `brew install ffmpeg`                                                                       |
| winget  | `powershell -NoProfile -Command "irm bun.sh/install.ps1 \| iex"` | `winget install --id Gyan.FFmpeg -e --accept-package-agreements --accept-source-agreements` |

The curl line is executed through `sh -c` (pipes are not expressible as argv);
`requiresSudo` is `true` only for the apt/dnf/pacman/zypper/apk rows.

### 2. CLI

```text
bun run setup              # probe, print the plan, ask, install, re-probe
bun run setup --check      # probe only; exit 1 when anything is missing/outdated
bun run setup --dry-run    # print the plan and the exact commands; change nothing
bun run setup --yes        # skip the confirmation prompt (for scripts/CI)
```

- Default mode **always asks** before running any command, and shows the exact
  argv first. Non-interactive stdin without `--yes` ⇒ prints the plan and exits
  `1` without installing (never hangs, never installs unattended).
- After applying, the CLI re-probes and prints a final verdict. Exit codes:
  `0` everything ok, `1` still missing/failed, `2` usage error.
- Output is a small aligned table (`bun 1.4.4 ok`, `ffmpeg — missing`) followed
  by the commands; nothing else goes to stdout. Errors go to stderr.
- `--check` never mutates the system and never invokes `sudo`.
- A failing command stops the run, prints the exit code and the surviving
  manual instruction, and leaves earlier successful installs in place
  (idempotent re-run picks up where it stopped).

### 3. Bootstraps

`scripts/setup.sh` (POSIX `sh`, `set -eu`):

1. If `bun` is absent: print the exact command, ask for confirmation, run
   `curl -fsSL https://bun.sh/install | bash`, then re-`exec` itself so the new
   `$HOME/.bun/bin` lands on `PATH`.
2. `exec bun run scripts/setup.ts "$@"`.

`scripts/setup.ps1` (PowerShell 5.1+ / 7+):

1. If `bun` is absent: `irm bun.sh/install.ps1 | iex`, refresh `PATH` from the
   registry/user environment.
2. `bun run scripts/setup.ts @args` and propagate the exit code.

Both are thin: **no dependency logic lives in them**. They honour `--check`,
`--dry-run` and `--yes` by passing arguments through unchanged. Neither writes
outside `$HOME` except through the package manager the user approved.

### 4. Docs

`README.md` gains a short "Requirements" block:

```text
Linux (primary), macOS and Windows (best effort)
  bun >= 1.4.0, ffmpeg >= 6.0 (provides ffprobe)

Verify:      ./scripts/setup.sh --check      (Windows: .\scripts\setup.ps1 -Check?)
Install:     ./scripts/setup.sh
```

(Exact flag spelling for PowerShell is documented in the script header; args
are passed through verbatim.)

### 5. README

`README.md` is rewritten as a welcome document. It is **the only place a
newcomer starts**, so it must be followable top to bottom with no other file
open, and every factual claim in it must be verifiable against the code. Its
sections, in order:

| Section                | Content                                                                                                                                                                                                                                                                    | Source of truth it must agree with                                                               |
| ---------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------ |
| Title + one-line pitch | What SpeechTT is, in two sentences.                                                                                                                                                                                                                                        | `PRODUCT.md` §1                                                                                  |
| Demo                   | A linked poster: the committed `demo-launcher-vignette.png` wrapped in a Markdown link to the GitHub attachment URL, so GitHub shows a clickable thumbnail that opens the player. GitHub renders an `user-attachments` URL as a player, which a relative `<video>` is not. | the committed PNG plus the attachment URL the CEO supplied                                       |
| What it does           | Five bullets: live dictation, file transcription, gateway models, window-scoped shortcuts, single process.                                                                                                                                                                 | `PRODUCT.md` §2–§3                                                                               |
| Requirements           | The runtime table: platform, Rust, `bun`, `ffmpeg`+`ffprobe`, gateway key. `ffmpeg` is marked file-mode-only.                                                                                                                                                              | `src/stt/file-input.ts`, `crates/speechtt-core/src/backend.rs` (`WORKER_SCRIPT`), `AGENTS.md` §4 |
| Getting started        | Three steps: Linux system packages (the `apt` line, credited and linked to <https://gpui-kit.com/docs/installation.md>), the `./scripts/setup.sh` bootstrap, then `bun install` + `bun run ui` **from the repository root**.                                               | this ticket's scripts; `package.json`                                                            |
| First run — an API key | One key (`AI_GATEWAY_API_KEY` / `VERCEL_AI_GATEWAY_API_KEY`) or Settings (`F2`); the settings path and its `0600` mode; the masked-key rule.                                                                                                                               | `src/stt/registry.ts` (`envVars`), `crates/speechtt-core/src/settings.rs`, `AGENTS.md` §9        |
| Keyboard shortcuts     | The five-row table, plus the note that Help generates its rows from the same table.                                                                                                                                                                                        | `crates/speechtt-ui/src/shortcuts.rs` `BINDINGS`                                                 |
| Project layout         | The tree, current to the file, plus the "what lives where" notes.                                                                                                                                                                                                          | `ls crates/*/src`, `ls src/stt`, `ls scripts`, `ls tests`                                        |
| Development            | The six gate commands, and what the headless launch gate proves.                                                                                                                                                                                                           | `AGENTS.md` §5                                                                                   |
| Documentation map      | A table linking `PRODUCT.md`, `DESIGN.md`, `AGENTS.md`, `tickets/README.md`.                                                                                                                                                                                               | the files                                                                                        |
| Status                 | One honest paragraph: functional end to end, remaining work tracked in `tickets/`.                                                                                                                                                                                         | `tickets/README.md`                                                                              |

Rules:

- **One provider, not three.** The worker resolves exactly one provider id,
  `vercel-ai-gateway` (`src/stt/registry.ts`), and Mistral, OpenAI and the rest
  are _models_ behind it. The old README's separate "Mistral AI" provider
  section is wrong and does not survive.
- **No unverifiable promise.** Do not repeat the Settings dialog's "falls back
  to a local model and keeps working fully offline" line: no local model exists
  in the worker. If a local provider is wanted, it is a product decision for
  the CEO, not a sentence in the README.
- **No licence claim.** The repository has no `LICENSE` file; the README must
  not assert one.
- **No duplication of depth.** The catalogue of models, the design tokens, the
  module boundaries and the engineering rules stay in `PRODUCT.md`,
  `DESIGN.md` and `AGENTS.md`; the README links to them.
- **The tree is checked, not remembered.** Build the tree from `ls`/`find`
  output at the time of writing, and verify each listed path exists before the
  ticket is staged.
- **The video is an attachment; the poster is committed.** The video lives as a
  GitHub attachment that the CEO uploads while editing the README on the web, so
  its local file `recording.mp4` is listed in `.gitignore`, never appears in
  `git status`, and no `docs/` tree exists for a single media file — a 104 KB
  video is not repository content. The `## Demo` section shows the committed
  poster `demo-launcher-vignette.png`, linked to that attachment URL: GitHub
  renders an `user-attachments` URL as a player, which a relative `<video>` is
  not.

## TDD Tasks

- [x] **T16.1 — Pure-function specs (red first)**: `setup.spec.ts` for
      `detectManager` (each platform, manager precedence, unknown), `parseVersion`
      (bun/ffmpeg/ffprobe real-world strings, garbage, single-number), `evaluate`
      (`ok`/`missing`/`outdated` boundaries at exactly the minimum, one patch above).
- [x] **T16.2 — Plan specs**: `plan()` emits no steps when all `ok`; one
      `ffmpeg` step when either ffmpeg or ffprobe is missing (deduped); `bun` before
      `ffmpeg`; no step + manual instructions when `manager` is `null`; stable
      ordering across input permutations.
- [x] **T16.3 — Command table specs**: one test per manager asserting the exact
      argv lists and `requiresSudo`, including the winget/brew rows.
- [x] **T16.4 — Implement** `scripts/setup.ts` until T16.1–T16.3 are green,
      with an injected `exec`/`has`/`prompt` seam (`Bun.spawn`, `Bun.which`).
- [x] **T16.5 — CLI specs**: `--check` exits 1 without invoking `exec` when a
      dependency is missing; `--dry-run` prints commands and never calls `exec`;
      non-TTY without `--yes` exits 1; `--yes` runs the plan; a failing command
      exits 1 and prints the manual fallback. Drive `runCli(deps, argv)` with fakes.
- [x] **T16.6 — Bootstraps**: `setup.sh`, `setup.ps1`, the `package.json`
      script.
- [x] **T16.7 — README**: lead the demo with the committed poster
      `demo-launcher-vignette.png`, linked to the GitHub attachment URL the CEO
      supplies, and git-ignore the local `recording.mp4`; rewrite `README.md` per §5,
      building the tree from live `ls`/`find` output and verifying every listed path.
      Cross-check the requirements table, the shortcut table and the gate commands
      against their sources of truth.
- [x] **T16.8 — Gates**: `bun test`, `bun check scripts/setup.ts` (or the repo
      type-check scope), plus a manual matrix run: `sh scripts/setup.sh --check` on
      this Arch machine (ffmpeg present ⇒ exit 0), `--dry-run` with a fake `PATH`
      lacking ffmpeg (plan printed, exit 0), and a shellcheck-free syntax check
      (`sh -n scripts/setup.sh`, `pwsh -NoProfile -File scripts/setup.ps1 -Check`
      when `pwsh` exists).

## Acceptance Criteria

1. On a machine with `bun` and `ffmpeg` installed, `bun run setup --check`
   prints all three rows `ok` and exits `0` **without touching the system**.
2. On a machine missing `ffmpeg`, the default mode prints the plan and the exact
   command, and installs only after an explicit `y`; re-running immediately
   reports everything `ok` and does nothing.
3. `--dry-run` and `--check` never invoke `sudo`, never install, and never
   prompt.
4. Piping the default mode into a non-interactive context (no `--yes`) exits
   `1` with the manual instructions instead of hanging or installing.
5. On a machine whose package manager is unknown to the script, nothing is
   guessed: the plan prints the manual install commands and exits `1`.
6. `./scripts/setup.sh` and `.\scripts\setup.ps1` both end up running
   `bun run scripts/setup.ts` with the user's flags; the bootstraps contain no
   dependency logic of their own.
7. Unit tests cover every platform/manager combination with injected fakes;
   no test ever runs a real package manager or requires network access.
8. `README.md` can be followed top to bottom by someone who has never seen the
   repository: it states the requirements, installs them, builds, runs, and
   explains the first run. Every path it lists exists, the shortcut table
   matches `shortcuts.rs`, the requirements match the code, and it names exactly
   one provider. The demo leads with the committed poster
   `demo-launcher-vignette.png`, linked to the GitHub attachment URL the CEO
   supplies; the video itself is never in the repository, `git status` never
   shows it because `recording.mp4` is git-ignored, and no `docs/` directory
   exists.
9. All gates green: `bun test`, type-check clean, zero warnings.

## Implementation Notes

### Files

| File                         | Change                                                                                                                                                                                |
| ---------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `scripts/setup.ts`           | **new** — probe, plan and install logic plus the CLI (`runCli`), the `systemDeps()` seam and the `import.meta.main` guard.                                                            |
| `scripts/setup.spec.ts`      | **new** — 75 `bun test` cases covering T16.1–T16.3 and T16.5 with injected fakes.                                                                                                     |
| `scripts/setup.sh`           | **new** — POSIX `sh`, `set -eu`; installs `bun` when absent (after confirmation), then `exec bun run scripts/setup.ts "$@"`.                                                          |
| `scripts/setup.ps1`          | **new** — PowerShell; installs `bun` via `irm bun.sh/install.ps1 \| iex`, refreshes `PATH` from the user environment, then `bun run scripts/setup.ts` and propagates `$LASTEXITCODE`. |
| `package.json`               | appended `"setup": "bun run scripts/setup.ts"`; nothing else touched.                                                                                                                 |
| `README.md`                  | rewritten as the welcome document (§5 section order).                                                                                                                                 |
| `.gitignore`                 | appended one `recording.mp4` entry (outside the original scope, at the CEO's direction).                                                                                              |
| `recording.mp4`              | **not committed** — the video is a GitHub attachment, and the local copy is `.gitignore`d.                                                                                            |
| `demo-launcher-vignette.png` | **new, committed** — the poster frame the demo section links to the attachment URL.                                                                                                   |

### Shape of `scripts/setup.ts`

- Contract types verbatim (`PackageManager`, `DependencyId`, `DependencySpec`,
  `DependencyState`, `InstallStep`, `SetupPlan`) plus `DEPENDENCIES` (the three
  specs in table order), `CliDeps` and `displayCommand`.
- Pure half: `detectManager` (linux probes `apt`, `dnf`, `pacman`, `zypper`,
  `apk` in that order; `darwin` → `brew`; `win32` → `winget`; anything else or
  nothing found → `null`), `parseVersion` (first `\d+\.\d+\.\d+` run),
  `evaluate`, `plan`, `commandsFor`.
- `plan()` builds `WorkItem`s (`bun`, then `ffmpeg` — the ffmpeg package covers
  ffprobe, so either one missing yields one step), maps them to steps when a
  manager is known and to `instructions` when it is not.
- CLI half: `runCli(deps, argv)` parses `--check` / `--dry-run` / `--yes`,
  rejects anything else with exit 2, prints the aligned table, then either the
  `Commands:` block, the `Manual install:` block, or `All dependencies are
ready.`; then check/dry-run/prompt/apply/re-probe.
- Machine seam: `systemDeps()` — `Bun.which` for `has`, `Bun.spawnSync` with
  piped streams for `probeVersion`, `Bun.spawnSync` with inherited streams for
  `exec` (so `sudo` keeps the terminal), `node:readline/promises` for `prompt`
  behind an `isTTY` guard, `console.log` / `console.error` for the sinks.
  Nothing runs on import: the CLI is entered only from `import.meta.main`.

### Fakes used in the specs

`harness()` builds a `CliDeps` whose `has` reports a fake `PATH` (`versions` map
plus a `managers` set), `probeVersion` reads that map, `exec` records the argv
and returns a per-call exit code (optionally mutating the fake `PATH`, which is
how "install then re-probe" is exercised), `prompt` records the question and
returns a scripted answer (or `null` for non-interactive stdin), and `out`/`err`
collect lines. No test spawns a process, opens a socket or writes a file.

### Deviations from the Contract, with reasons

1. **`SetupPlan` gained `instructions: readonly string[]`.** §1's interface
   omits it, but the same section's rules require the plan to return manual
   instructions when the manager is unknown. It is empty whenever `steps`
   carries the work.
2. **`requiresSudo` is per step, not per manager.** It is `true` only for the
   ffmpeg step under apt/dnf/pacman/zypper/apk. The bun step is never sudo: the
   `curl … | bash` installer writes into `$HOME`. Marking it `true` would print
   (and imply) a root install that does not happen.
3. **A present binary whose banner does not parse is `ok`, not `outdated`.**
   The version gate only rejects a version it can read and finds too low;
   otherwise an unrecognised build would trigger an install over a working
   binary.
4. **`--dry-run` always exits `0`**, including when something is missing, which
   is what T16.8's manual matrix asserts. `--check` is the exit-code gate: `1`
   when anything is missing or outdated.
5. **PowerShell flag spelling.** `.\scripts\setup.ps1 -Check` (and `-DryRun`,
   `-Yes`) are translated to the `--…` form; every other argument, including
   the GNU spelling, is forwarded unchanged. §4 says the exact spelling belongs
   in the script header, and it is documented there.
6. **`plan()` derives `platform.os` from the manager** (`brew` → macOS,
   `winget` → Windows, Linux managers → Linux, `null` → Linux, the primary
   platform, where no command is emitted anyway), so the documented two-argument
   signature is kept exactly.
7. **Extra exports** (`DEPENDENCIES`, `CliDeps`, `runCli`, `displayCommand`).
   T16.5 requires `runCli(deps, argv)`; the others are what the CLI and the
   specs share.

### Gate results

| Gate                                               | Result                                                                               |
| -------------------------------------------------- | ------------------------------------------------------------------------------------ |
| `cargo fmt -- --check`                             | clean (no diff)                                                                      |
| `cargo clippy --all-targets -- -D warnings`        | `Finished dev profile` — zero warnings                                               |
| `cargo test`                                       | 18 binaries, **614 passed, 0 failed, 0 ignored** (incl. startup and launch gates)    |
| `bun test`                                         | 17 files, **423 pass, 0 fail**, 1442 `expect()` (75 of them `scripts/setup.spec.ts`) |
| `bun check src/stt`                                | `✓ No type errors in 32 files`                                                       |
| `bun check scripts/setup.ts scripts/setup.spec.ts` | `✓ No type errors in 2 files`                                                        |
| `bun run design:validate`                          | `✓ Design contract valid — 66 tokens, 0 errors.`                                     |

There is no repository-wide TypeScript scope (`bun check` with no path would
also type-check `vendor/` and the `.tsx` mockup dumps), so the new scripts are
type-checked explicitly, as recorded in the last two rows.

### Manual matrix (this machine: CachyOS/Arch, bun 1.4.4, ffmpeg/ffprobe n9.0.2)

```text
$ sh -n scripts/setup.sh
sh -n OK

$ ./scripts/setup.sh --check        # through the exec bit, as the README documents
bun      1.4.4    ok
ffmpeg   9.0.2    ok
ffprobe  9.0.2    ok
All dependencies are ready.
exit=0

$ sh scripts/setup.sh --check
bun      1.4.4    ok
ffmpeg   9.0.2    ok
ffprobe  9.0.2    ok
All dependencies are ready.
exit=0

$ bun run setup --check
$ bun run scripts/setup.ts --check
bun      1.4.4    ok
ffmpeg   9.0.2    ok
ffprobe  9.0.2    ok
All dependencies are ready.
exit=0

$ env PATH=/tmp/opencode/bin-no-ffmpeg /bin/sh scripts/setup.sh --dry-run
# fake PATH: bun + dirname only (no ffmpeg, no package manager)
bun      1.4.4    ok
ffmpeg   —        missing
ffprobe  —        missing

Manual install:
  ffmpeg and ffprobe — install them with your system package manager, or see https://ffmpeg.org/download.html
exit=0

$ env PATH=/tmp/opencode/bin-no-ffmpeg /bin/sh scripts/setup.sh --dry-run
# same fake PATH, plus a symlinked pacman
bun      1.4.4    ok
ffmpeg   —        missing
ffprobe  —        missing

Commands:
  sudo pacman -S --needed --noconfirm ffmpeg  (needs root)
exit=0

$ env PATH=/tmp/opencode/bin-no-ffmpeg /bin/sh scripts/setup.sh --check
... Not ready: ffmpeg, ffprobe
exit=1

$ bun run setup < /dev/null
bun      1.4.4    ok
ffmpeg   9.0.2    ok
ffprobe  9.0.2    ok
All dependencies are ready.
exit=0

$ env PATH=<bun absent> /bin/sh scripts/setup.sh --check < /dev/null
bun is missing. Install it with:
  curl -fsSL https://bun.sh/install | bash
Not ready: bun
exit=1

$ env PATH=<bun absent> /bin/sh scripts/setup.sh --dry-run < /dev/null
bun is missing. Install it with:
  curl -fsSL https://bun.sh/install | bash
exit=0

$ env PATH=<bun absent> /bin/sh scripts/setup.sh < /dev/null
bun is not installed. Install it with:
  curl -fsSL https://bun.sh/install | bash
Run that now? [y/N] Install bun first, then re-run this script.
exit=1
```

`scripts/setup.sh` is committed with mode `100755` (`git add -N` reports
`100755`), so `./scripts/setup.sh` runs as the README says; `scripts/setup.ps1`
stays at `644`.

`pwsh -NoProfile -File scripts/setup.ps1 -Check` was **not** run: `pwsh` is not
installed on this machine (`which pwsh` → not found). The file was written and
reviewed by hand; it must be exercised on a Windows host before release (see
Deviation 6).

### Deviations

Recorded so the next reader does not have to re-derive them.

1. **`SetupPlan.instructions` was added.** §1's `SetupPlan` has no field for the
   manual instructions its own rules require when the manager is unknown, so the
   interface carries `readonly instructions: readonly string[]`. It is empty
   whenever `steps` carries the work, and it is what the CLI prints under
   `Manual install:`.
2. **`requiresSudo` is per step, not per manager.** The `curl … | bash` bun step
   writes into `$HOME` and must not imply root, so only the ffmpeg step under a
   Linux manager sets it. `runCli` reads it and annotates that command line with
   `(needs root)` before asking.
3. **`--dry-run` always exits 0; `--check` is the exit-code gate.** This
   contradicts a literal reading of §2 ("exit 1 when anything is missing") and
   follows T16.8's manual matrix instead ("plan printed, exit 0"). The residual
   tension with AC5 is deliberate: AC5 is about the unknown-manager case, which
   `--dry-run` still reports on stdout while exiting 0, and `--check` reports with
   exit 1.
4. **`--check`/`--yes` take precedence over the bootstrap's ask-before-install
   prompt.** AC3 forbids prompting under `--check`/`--dry-run`, which an absent
   `bun` used to violate. `scripts/setup.sh` now reads the three flags before it
   decides: `--check` prints the exact command and exits 1, `--dry-run` prints it
   and exits 0, `--yes` installs without asking, and only the plain interactive
   case asks. The arguments still reach `scripts/setup.ts` unchanged.
5. **"Nothing else goes to stdout" (§2) is not literal.** `Running: <command>`,
   `Nothing was installed.`, `Not ready: …`, the `Manual install:`/`Commands:`
   blocks and the status table all go to stdout, so a caller can read the plan
   from one stream. Only the two `setup: …` failure lines go to stderr.
6. **The PowerShell bootstrap has not been executed.** There is no `pwsh` on
   this machine, so `scripts/setup.ps1` is reviewed by hand only and needs one
   real run on a Windows host (`.\scripts\setup.ps1 -Check`, then
   `.\scripts\setup.ps1 -DryRun`) before release.
7. **`evaluate()` gained an optional `probeFailed` field.** A present binary whose
   probe exits non-zero is reported `missing` (the plan reinstalls it) while a
   present binary whose banner merely carries no version stays `ok` with
   `version: null`. Without the flag the two cases were indistinguishable and a
   broken `bun` printed `bun  —  ok`. `probe: { present, versionOutput }` remains
   a valid call shape, so §1's signature still holds.

### Red-checks

Both repaired assertions were reverted and restored to prove they fail for the
reason they claim:

```text
# 1 — `deps.out("Manual install:")` renamed to "Install by hand:"
error: expect(received).toContain(expected)
Expected to contain: "Manual install:"
Received: [ "bun      1.4.4    ok", "ffmpeg   —        missing", "ffprobe  —        missing", …
(fail) runCli --check > an unknown manager prints the manual instructions and exits 1
(fail) runCli default mode > an unknown manager never installs anything
 73 pass

# 2 — the instruction URL changed from ffmpeg.org/download.html to ffmpeg.org/
Expected: true
(fail) runCli --check > an unknown manager prints the manual instructions and exits 1
Expected: true
(fail) runCli default mode > an unknown manager never installs anything
 73 pass

# 3 — the `(needs root)` annotation removed from the Commands block
(fail) runCli --dry-run > marks a step that needs root before asking
 74 pass
```

All three were restored afterwards (75 pass, 0 fail).

### README

- Every one of the 67 entries of the tree was checked against the filesystem with
  a script walk of the code block, and every tree entry exists; the tree was
  built from live `ls` output. It lists `tickets/`, `skills-lock.json` and
  `adapters/types.ts`, and says in one sentence that `*.spec.ts` files sit beside
  their sources instead of listing thirty of them.
- Exactly one provider is named: `vercel-ai-gateway`, with Mistral, OpenAI,
  Google and ElevenLabs presented as _models_ behind it.
- Requirements table matches `src/stt/registry.ts` (`envVars`),
  `crates/speechtt-core/src/backend.rs` (`WORKER_SCRIPT`),
  `crates/speechtt-core/src/settings.rs` (`0o600`, `~/.config/speechtt`),
  `src/stt/file-input.ts` (ffmpeg/ffprobe, file mode only) and `AGENTS.md` §4.
- The shortcut table matches `crates/speechtt-ui/src/shortcuts.rs` `BINDINGS`
  row for row (Space, Enter, Ctrl+S/Cmd+S, F1, F2) and notes that `bind_app_keys`
  adds `Ctrl`/`Cmd`+`C` and that the Help tab is generated from the same table.
- No licence claim; the local-model fallback sentence is not repeated.
- The `## Demo` section is a linked poster: an image reference to the committed
  `demo-launcher-vignette.png` wrapped in a link to the GitHub attachment URL.
  GitHub shows the PNG as a clickable thumbnail that opens the player, so the
  section needs neither a relative `<video>` (which GitHub does not render
  reliably) nor a bare link.
- The video is a GitHub attachment, not a repository file: the CEO uploads it in
  GitHub's README editor, which yields an
  `https://github.com/user-attachments/assets/<uuid>` URL, and that URL is what the
  poster links to. The poster is what a reader clicks.
- Because the video is not a repository file, `recording.mp4` is listed in
  `.gitignore` and never appears in `git status`, and no `docs/` tree exists for a
  single media file. The poster `demo-launcher-vignette.png` **is** committed at
  the repository root and appears in the tree.
- Because the video is not a repository file, `recording.mp4` is listed in
  `.gitignore` and never appears in `git status`, and no `docs/` tree exists for
  a single media file. The local file stays on disk for the upload.

### Not done here

- The ticket is in `tickets/review/`; no commit was made.
- Visual review of the demo/README rendering on GitHub belongs to the CEO.
