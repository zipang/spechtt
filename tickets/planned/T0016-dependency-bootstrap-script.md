# T0016 — Dependency bootstrap for Bun and ffmpeg

- **Track**: Tooling (TypeScript/Bun) + docs
- **Wave**: 1 (independent)
- **Depends on**: nothing (T0015 consumes the result: an installed ffmpeg)
- **Blocks**: nothing
- **References**: `AGENTS.md` §3 (Bun only), §5 (Don'ts), T0015 platform matrix

## Context & Goal

SpeechTT needs two binaries that are not part of the Rust workspace:

| Binary | Used by | Present by default |
|---|---|---|
| `bun` | the STT worker subprocess (`src/stt/worker.ts`) | no — user installs |
| `ffmpeg` + `ffprobe` | file decoding (T0015) | Linux: often; macOS/Windows: no |

Today a missing binary surfaces as an opaque failure (`spawn bun: No such file`,
or T0015's `ffmpeg_missing`), leaving the user to work out the install command
themselves. Done means a single documented command verifies and, after explicit
confirmation, installs exactly what is missing — idempotently, without ever
running `sudo` silently, and with the same logic on Linux, macOS and Windows.

**Scope is runtime dependencies only** (`bun`, `ffmpeg`, `ffprobe`). The Rust
toolchain and build libraries (ALSA, xkbcommon, Wayland/X11, fontconfig) stay
out: developers already have them to build, and a `--dev` matrix would double
the distro surface for no end-user benefit.

## Scope Boundary

May create or modify:

- `scripts/setup.ts` — **new**: cross-platform plan/confirm/apply logic.
- `scripts/setup.spec.ts` — **new**: `bun test` unit tests with a fake executor.
- `scripts/setup.sh` — **new**: POSIX bootstrap (~15 lines).
- `scripts/setup.ps1` — **new**: PowerShell bootstrap (~15 lines).
- `package.json` — **append-only**: one `"setup": "bun run scripts/setup.ts"` script.
- `README.md` — a short "Requirements / Setup" section pointing at both entry
  points.

Do **not** touch: anything under `src/`, `crates/`, `tests/`, `DESIGN.md`,
`design-tokens.css`, `.agents/`, or the T0015 ticket. No new npm dependency,
no `npm`/`npx`/`tsc` invocation. The script never writes into the repository.

## Contract

### 1. Core (`scripts/setup.ts`)

Pure, injectable, no top-level side effects when imported (so tests never run a
package manager):

```ts
export type PackageManager =
  | "apt" | "dnf" | "pacman" | "zypper" | "apk"   // Linux
  | "brew"                                        // macOS
  | "winget";                                     // Windows

export type DependencyId = "bun" | "ffmpeg" | "ffprobe";

export interface DependencySpec {
  readonly id: DependencyId;
  /** argv used to read the version (`["--version"]` / `["-version"]`, matching reality). */
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
  /** argv lists executed in order; each runs through `exec`. */
  readonly commands: readonly (readonly string[])[];
  readonly requiresSudo: boolean;
}

export interface SetupPlan {
  readonly platform: { os: "linux" | "macos" | "windows"; manager: PackageManager | null };
  readonly states: readonly DependencyState[];
  readonly steps: readonly InstallStep[];
}

export function detectManager(platform: NodeJS.Platform, has: (bin: string) => boolean): PackageManager | null;

export function parseVersion(raw: string): readonly [number, number, number] | null;

export function evaluate(
  spec: DependencySpec,
  probe: { present: boolean; versionOutput: string | null },
): DependencyState;

export function plan(states: readonly DependencyState[], manager: PackageManager | null): SetupPlan;

export function commandsFor(id: DependencyId, manager: PackageManager): readonly (readonly string[])[];
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

| Manager | bun | ffmpeg (+ffprobe) |
|---|---|---|
| apt | `curl -fsSL https://bun.sh/install \| bash` | `sudo apt-get install -y ffmpeg` |
| dnf | same curl line | `sudo dnf install -y ffmpeg` |
| pacman | same curl line | `sudo pacman -S --needed --noconfirm ffmpeg` |
| zypper | same curl line | `sudo zypper install -y ffmpeg` |
| apk | same curl line | `sudo apk add ffmpeg` |
| brew | `brew install oven-sh/bun/bun` | `brew install ffmpeg` |
| winget | `powershell -NoProfile -Command "irm bun.sh/install.ps1 \| iex"` | `winget install --id Gyan.FFmpeg -e --accept-package-agreements --accept-source-agreements` |

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

## TDD Tasks

- [ ] **T16.1 — Pure-function specs (red first)**: `setup.spec.ts` for
  `detectManager` (each platform, manager precedence, unknown), `parseVersion`
  (bun/ffmpeg/ffprobe real-world strings, garbage, single-number), `evaluate`
  (`ok`/`missing`/`outdated` boundaries at exactly the minimum, one patch above).
- [ ] **T16.2 — Plan specs**: `plan()` emits no steps when all `ok`; one
  `ffmpeg` step when either ffmpeg or ffprobe is missing (deduped); `bun` before
  `ffmpeg`; no step + manual instructions when `manager` is `null`; stable
  ordering across input permutations.
- [ ] **T16.3 — Command table specs**: one test per manager asserting the exact
  argv lists and `requiresSudo`, including the winget/brew rows.
- [ ] **T16.4 — Implement** `scripts/setup.ts` until T16.1–T16.3 are green,
  with an injected `exec`/`has`/`prompt` seam (`Bun.spawn`, `Bun.which`).
- [ ] **T16.5 — CLI specs**: `--check` exits 1 without invoking `exec` when a
  dependency is missing; `--dry-run` prints commands and never calls `exec`;
  non-TTY without `--yes` exits 1; `--yes` runs the plan; a failing command
  exits 1 and prints the manual fallback. Drive `runCli(deps, argv)` with fakes.
- [ ] **T16.6 — Bootstraps + docs**: `setup.sh`, `setup.ps1`, `package.json`
  script, README section.
- [ ] **T16.7 — Gates**: `bun test`, `bun check scripts/setup.ts` (or the repo
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
8. All gates green: `bun test`, type-check clean, zero warnings.

## Implementation Notes

_(added during work)_
