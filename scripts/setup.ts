/**
 * Dependency bootstrap for the two runtime binaries SpeechTT needs outside the
 * Rust workspace: `bun` (the STT worker subprocess) and `ffmpeg` with its
 * `ffprobe` companion (file-mode decoding).
 *
 * It probes what is installed, prints a small table, builds an install plan for
 * the chosen package manager, shows the exact commands, asks before running
 * anything, and re-probes afterwards so the exit code reports the real state of
 * the machine.
 *
 * # Callers
 *
 * - `package.json`'s `setup` script (`bun run setup`), the documented entry
 *   point.
 * - `scripts/setup.sh` and `scripts/setup.ps1`, which install `bun` first when
 *   it is absent and then hand their arguments to this file unchanged.
 * - `scripts/setup.spec.ts`, which drives every function with injected fakes.
 *
 * # Purity
 *
 * Importing this module runs nothing: the CLI is entered from the
 * `import.meta.main` guard at the bottom, and every machine-facing capability
 * (`has`, `probeVersion`, `exec`, `prompt`, the two output sinks) arrives as a
 * {@link CliDeps} value. That is what lets the tests exercise the whole command
 * surface without a package manager, a network or a mutation.
 */
import { createInterface } from "node:readline/promises";

/** The package managers the install table speaks. */
export type PackageManager =
	| "apt"
	| "dnf"
	| "pacman"
	| "zypper"
	| "apk"
	| "brew"
	| "winget";

/** The binaries this bootstrap owns. */
export type DependencyId = "bun" | "ffmpeg" | "ffprobe";

/** What one dependency needs to be probed and judged. */
export interface DependencySpec {
	readonly id: DependencyId;
	/** argv used to read the version (`["--version"]` / `["-version"]`, matching reality). */
	readonly versionArgs: readonly string[];
	/** Lowest acceptable version. */
	readonly minimum: readonly [number, number, number];
}

/** The verdict on one dependency. */
export interface DependencyState {
	readonly id: DependencyId;
	readonly present: boolean;
	readonly version: readonly [number, number, number] | null;
	readonly status: "ok" | "missing" | "outdated";
}

/** One package-manager operation of the plan. */
export interface InstallStep {
	readonly id: DependencyId;
	/** argv lists executed in order; each runs through `exec`. */
	readonly commands: readonly (readonly string[])[];
	/**
	 * Whether the step's commands need root.
	 *
	 * Read by `runCli`, which annotates such a step in the `Commands:` block so
	 * the user knows a password is coming before answering the prompt. It is
	 * `true` only for the ffmpeg step under a Linux manager: the Bun installer
	 * writes into `$HOME`.
	 */
	readonly requiresSudo: boolean;
}

/** The whole plan: what the machine looks like, and what would fix it. */
export interface SetupPlan {
	/**
	 * The platform family the plan was built for.
	 *
	 * It is not printed by `runCli` — the table names what is installed and the
	 * `Commands:` block names the manager — so it exists for callers and tests
	 * that need to know which family a plan belongs to.
	 */
	readonly platform: {
		readonly os: "linux" | "macos" | "windows";
		readonly manager: PackageManager | null;
	};
	readonly states: readonly DependencyState[];
	readonly steps: readonly InstallStep[];
	/**
	 * Human-readable install lines for work no step can express, which is the
	 * unknown-manager case: the plan never guesses a package manager, so it
	 * prints what to do instead. Empty whenever {@link steps} carries the work.
	 */
	readonly instructions: readonly string[];
}

/** The runtime dependencies, in table order. */
export const DEPENDENCIES: readonly DependencySpec[] = [
	{ id: "bun", versionArgs: ["--version"], minimum: [1, 4, 0] },
	{ id: "ffmpeg", versionArgs: ["-version"], minimum: [6, 0, 0] },
	{ id: "ffprobe", versionArgs: ["-version"], minimum: [6, 0, 0] },
];

/**
 * The platform each manager belongs to, in {@link detectManager}'s probe order.
 *
 * The key order is the Linux probe order, and {@link LINUX_MANAGERS} is derived
 * from this table so the probe list and the platform mapping cannot disagree.
 */
const MANAGER_OS: Readonly<Record<PackageManager, "linux" | "macos" | "windows">> = {
	apt: "linux",
	dnf: "linux",
	pacman: "linux",
	zypper: "linux",
	apk: "linux",
	brew: "macos",
	winget: "windows",
};

/** Linux managers, in the order {@link detectManager} probes them. */
const LINUX_MANAGERS: readonly PackageManager[] = (
	Object.keys(MANAGER_OS) as PackageManager[]
).filter((manager) => MANAGER_OS[manager] === "linux");

/**
 * Whether installing a package with `manager` needs root.
 *
 * Every Linux manager writes into the system prefix, so its install goes
 * through `sudo`; `brew` and `winget` install per user and never do. The Bun
 * installer is exempt on every platform — it writes into `$HOME`.
 */
const needsRoot = (manager: PackageManager): boolean => MANAGER_OS[manager] === "linux";

/** The official Bun installer, which writes into `$HOME` and needs no root. */
const BUN_INSTALL_SCRIPT = "curl -fsSL https://bun.sh/install | bash";

/** Bun's install command per manager; a pipe is only expressible through `sh -c`. */
const BUN_COMMANDS: Readonly<Record<PackageManager, readonly (readonly string[])[]>> = {
	apt: [["sh", "-c", BUN_INSTALL_SCRIPT]],
	dnf: [["sh", "-c", BUN_INSTALL_SCRIPT]],
	pacman: [["sh", "-c", BUN_INSTALL_SCRIPT]],
	zypper: [["sh", "-c", BUN_INSTALL_SCRIPT]],
	apk: [["sh", "-c", BUN_INSTALL_SCRIPT]],
	brew: [["brew", "install", "oven-sh/bun/bun"]],
	winget: [["powershell", "-NoProfile", "-Command", "irm bun.sh/install.ps1 | iex"]],
};

/** The ffmpeg package per manager; the same package provides `ffprobe`. */
const FFMPEG_COMMANDS: Readonly<Record<PackageManager, readonly (readonly string[])[]>> = {
	apt: [["sudo", "apt-get", "install", "-y", "ffmpeg"]],
	dnf: [["sudo", "dnf", "install", "-y", "ffmpeg"]],
	pacman: [["sudo", "pacman", "-S", "--needed", "--noconfirm", "ffmpeg"]],
	zypper: [["sudo", "zypper", "install", "-y", "ffmpeg"]],
	apk: [["sudo", "apk", "add", "ffmpeg"]],
	brew: [["brew", "install", "ffmpeg"]],
	winget: [
		[
			"winget",
			"install",
			"--id",
			"Gyan.FFmpeg",
			"-e",
			"--accept-package-agreements",
			"--accept-source-agreements",
		],
	],
};

/** Where to install by hand when no known manager is on `PATH`. */
const MANUAL_INSTRUCTIONS: Readonly<Record<"bun" | "ffmpeg", string>> = {
	bun: "bun — install it from https://bun.sh/docs/installation",
	ffmpeg:
		"ffmpeg and ffprobe — install them with your system package manager, or see https://ffmpeg.org/download.html",
};

/**
 * The package manager to plan for, or `null` when the platform is unknown here
 * or none of its managers is on `PATH`.
 *
 * Linux probes `apt`, `dnf`, `pacman`, `zypper`, `apk` in that order and reports
 * the first present one; macOS probes `brew`; Windows probes `winget`. A machine
 * matched by none gets `null`, and {@link plan} answers with instructions rather
 * than a guessed manager.
 */
export function detectManager(
	platform: NodeJS.Platform,
	has: (bin: string) => boolean,
): PackageManager | null {
	switch (platform) {
		case "linux": {
			for (const manager of LINUX_MANAGERS) {
				if (has(manager)) {
					return manager;
				}
			}
			return null;
		}
		case "darwin":
			return has("brew") ? "brew" : null;
		case "win32":
			return has("winget") ? "winget" : null;
		default:
			return null;
	}
}

/**
 * The first three dot-separated integers of a version banner.
 *
 * `1.4.4`, `ffmpeg version 7.1.2-static` and `ffprobe version n9.0.2` all
 * yield a triple; a banner with fewer than three components, or without any,
 * yields `null` rather than a partial version that would compare wrongly.
 */
export function parseVersion(raw: string): readonly [number, number, number] | null {
	const match = /(\d+)\.(\d+)\.(\d+)/.exec(raw);
	if (match === null) {
		return null;
	}
	const [, major, minor, patch] = match;
	return [Number(major), Number(minor), Number(patch)];
}

/** Negative when `left` is older than `right`, zero when equal, positive when newer. */
function compareVersions(
	left: readonly [number, number, number],
	right: readonly [number, number, number],
): number {
	for (let index = 0; index < 3; index += 1) {
		const difference = (left[index] ?? 0) - (right[index] ?? 0);
		if (difference !== 0) {
			return difference;
		}
	}
	return 0;
}

/**
 * What one version probe observed.
 *
 * `failed` is separate from `output: null` because they mean different things:
 * a probe that could not run the binary (non-zero exit, spawn error) says
 * nothing about which version is installed, while a banner that simply carries
 * no version is a build we do not recognise.
 */
export interface VersionProbe {
	readonly output: string | null;
	readonly failed: boolean;
}

/**
 * Judges one dependency from what the probe saw.
 *
 * `missing` and `outdated` both need an install, and they are separate statuses
 * so the table can say which one it is.
 *
 * `status` reports whether the binary is *usable*, not merely whether its name
 * is on `PATH`, which is why `present` and `missing` can coexist: a binary the
 * probe could not run is reported as `missing` so the plan reinstalls it. A
 * banner that parses to nothing is the other case — the binary ran and only the
 * version string is unfamiliar, so it counts as `ok` and its version is left
 * `null`. The version gate therefore never rejects what it cannot read.
 */
export function evaluate(
	spec: DependencySpec,
	probe: { present: boolean; versionOutput: string | null; probeFailed?: boolean },
): DependencyState {
	if (!probe.present || probe.probeFailed === true) {
		return { id: spec.id, present: probe.present, version: null, status: "missing" };
	}

	const version = probe.versionOutput === null ? null : parseVersion(probe.versionOutput);
	if (version === null) {
		return { id: spec.id, present: true, version: null, status: "ok" };
	}

	const status = compareVersions(version, spec.minimum) < 0 ? "outdated" : "ok";
	return { id: spec.id, present: true, version, status };
}

/**
 * The argv lists that install `id` with `manager`.
 *
 * `ffprobe` shares ffmpeg's package, so it resolves to the same list. The Bun
 * installer is a shell pipeline, which `exec` receives as one `sh -c` argv.
 */
export function commandsFor(
	id: DependencyId,
	manager: PackageManager,
): readonly (readonly string[])[] {
	return id === "bun" ? BUN_COMMANDS[manager] : FFMPEG_COMMANDS[manager];
}

/** The platform a manager's commands target. */
function osForManager(manager: PackageManager | null): "linux" | "macos" | "windows" {
	// Linux is the primary platform, and it is also the answer when no manager
	// was found: no command is emitted in that case anyway.
	return manager === null ? "linux" : MANAGER_OS[manager];
}

/** A unit of work: the dependency whose package covers one or two of them. */
interface WorkItem {
	readonly id: "bun" | "ffmpeg";
}

/**
 * Builds the plan from the probe results and the manager.
 *
 * Steps are ordered `bun` → `ffmpeg` and deduplicated: the ffmpeg package
 * provides ffprobe, so either one being missing produces a single ffmpeg step.
 * When `manager` is `null` nothing can be installed by the script, so the plan
 * carries no step and puts the manual lines in {@link SetupPlan.instructions}
 * instead.
 *
 * `requiresSudo` marks the ffmpeg step under the five Linux managers, whose
 * package install needs root; the Bun installer writes into `$HOME` and never
 * does.
 */
export function plan(
	states: readonly DependencyState[],
	manager: PackageManager | null,
): SetupPlan {
	const status = new Map(states.map((state) => [state.id, state.status] as const));
	const needs = (id: DependencyId) => status.has(id) && status.get(id) !== "ok";

	const work: WorkItem[] = [];
	if (needs("bun")) {
		work.push({ id: "bun" });
	}
	if (needs("ffmpeg") || needs("ffprobe")) {
		work.push({ id: "ffmpeg" });
	}

	const steps: InstallStep[] =
		manager === null
			? []
			: work.map((item) => ({
					id: item.id,
					commands: commandsFor(item.id, manager),
					requiresSudo: item.id === "ffmpeg" && needsRoot(manager),
				}));

	const instructions =
		manager === null ? work.map((item) => MANUAL_INSTRUCTIONS[item.id]) : [];

	return {
		platform: { os: osForManager(manager), manager },
		states,
		steps,
		instructions,
	};
}

/** How one argv list reads on the terminal, with an `sh -c` pipeline unwrapped. */
export function displayCommand(argv: readonly string[]): string {
	if (argv.length === 3 && argv[0] === "sh" && argv[1] === "-c") {
		return argv[2] ?? "";
	}
	return argv.join(" ");
}

/** The injectable machine surface of the CLI. */
export interface CliDeps {
	/** Host platform, as `process.platform` reports it. */
	readonly platform: NodeJS.Platform;
	/** Whether a binary is on `PATH`. */
	readonly has: (bin: string) => boolean;
	/** Runs the binary's version argv and reports what came back. */
	readonly probeVersion: (bin: string, args: readonly string[]) => VersionProbe;
	/** Runs one argv list and reports its exit code. */
	readonly exec: (argv: readonly string[]) => { exitCode: number };
	/**
	 * Asks the user a question and returns the answer, or `null` when there is
	 * nobody to ask (non-interactive stdin, EOF). A `null` answer is never
	 * treated as consent.
	 */
	readonly prompt: (question: string) => string | null | Promise<string | null>;
	readonly out: (line: string) => void;
	readonly err: (line: string) => void;
}

const USAGE = "usage: bun run setup [--check] [--dry-run] [--yes]";

/** The aligned rows of the status table. */
function tableLines(states: readonly DependencyState[]): string[] {
	const width = Math.max(...states.map((state) => state.id.length));
	return states.map((state) => {
		const version = state.version === null ? "—" : state.version.join(".");
		const minimum = DEPENDENCIES.find((spec) => spec.id === state.id)?.minimum.join(".") ?? "";
		const status = state.status === "outdated" ? `outdated (needs >= ${minimum})` : state.status;
		return `${state.id.padEnd(width)}  ${version.padEnd(7)}  ${status}`.trimEnd();
	});
}

/** Whether every dependency of `states` is `ok`. */
function allReady(states: readonly DependencyState[]): boolean {
	return states.every((state) => state.status === "ok");
}

/** The dependencies that still need work, in table order. */
function pendingIds(states: readonly DependencyState[]): string {
	return states
		.filter((state) => state.status !== "ok")
		.map((state) => state.id)
		.join(", ");
}

/**
 * The `setup` command itself: probe, print, ask, install, re-probe.
 *
 * Exit codes are the contract: `0` everything is ready, `1` something is still
 * missing or an install failed, `2` a usage error.
 *
 * `--check` reports and stops before any prompt or command; `--dry-run` prints
 * the plan and stops the same way, with exit `0`; `--yes` skips the question.
 * Nothing runs without `--yes` or an explicit `y`, and `--check`/`--dry-run`
 * never prompt, so a script that asks a question and a script that would change
 * the machine can never be the same invocation. A step that needs root is marked
 * `(needs root)` in the `Commands:` block, so the user sees it before answering.
 */
export async function runCli(deps: CliDeps, argv: readonly string[]): Promise<number> {
	let check = false;
	let dryRun = false;
	let assumeYes = false;

	for (const argument of argv) {
		switch (argument) {
			case "--check":
				check = true;
				break;
			case "--dry-run":
				dryRun = true;
				break;
			case "--yes":
				assumeYes = true;
				break;
			default:
				deps.err(`setup: unknown argument ${argument}`);
				deps.err(USAGE);
				return 2;
		}
	}

	const probe = (): DependencyState[] =>
		DEPENDENCIES.map((spec) => {
			const present = deps.has(spec.id);
			const result = present ? deps.probeVersion(spec.id, spec.versionArgs) : null;
			return evaluate(spec, {
				present,
				versionOutput: result?.output ?? null,
				probeFailed: result?.failed ?? false,
			});
		});

	const manager = detectManager(deps.platform, deps.has);
	const initial = plan(probe(), manager);

	for (const line of tableLines(initial.states)) {
		deps.out(line);
	}

	if (initial.steps.length > 0) {
		deps.out("");
		deps.out("Commands:");
		for (const step of initial.steps) {
			const commands = step.commands.map(displayCommand);
			commands.forEach((command, index) => {
				const root = step.requiresSudo && index === commands.length - 1 ? "  (needs root)" : "";
				deps.out(`  ${command}${root}`);
			});
		}
	} else if (initial.instructions.length > 0) {
		deps.out("");
		deps.out("Manual install:");
		for (const instruction of initial.instructions) {
			deps.out(`  ${instruction}`);
		}
	}

	if (allReady(initial.states)) {
		deps.out("All dependencies are ready.");
		return 0;
	}

	if (check) {
		deps.out(`Not ready: ${pendingIds(initial.states)}`);
		return 1;
	}

	if (dryRun) {
		return 0;
	}

	if (manager === null) {
		deps.out(`Not ready: ${pendingIds(initial.states)}`);
		return 1;
	}

	if (!assumeYes) {
		const answer = await deps.prompt("Install these dependencies now? [y/N] ");
		if (answer === null || !/^y(es)?$/i.test(answer.trim())) {
			deps.out("Nothing was installed.");
			deps.out(`Not ready: ${pendingIds(initial.states)}`);
			return 1;
		}
	}

	for (const step of initial.steps) {
		for (const command of step.commands) {
			const display = displayCommand(command);
			deps.out(`Running: ${display}`);
			const result = deps.exec(command);
			if (result.exitCode !== 0) {
				deps.err(`setup: command failed with exit code ${result.exitCode}: ${display}`);
				deps.err(`setup: run it yourself: ${display}`);

				const after = probe();
				deps.out("");
				for (const line of tableLines(after)) {
					deps.out(line);
				}
				deps.out(`Not ready: ${pendingIds(after)}`);
				return 1;
			}
		}
	}

	const after = probe();
	deps.out("");
	for (const line of tableLines(after)) {
		deps.out(line);
	}

	if (allReady(after)) {
		deps.out("All dependencies are ready.");
		return 0;
	}

	deps.out(`Not ready: ${pendingIds(after)}`);
	return 1;
}

/** The live machine surface behind the CLI. */
function systemDeps(): CliDeps {
	return {
		platform: process.platform,
		has: (bin) => Bun.which(bin) !== null,
		probeVersion: (bin, args) => {
			try {
				const result = Bun.spawnSync([bin, ...args], { stdout: "pipe", stderr: "pipe" });
				return result.exitCode === 0
					? { output: result.stdout.toString(), failed: false }
					: { output: null, failed: true };
			} catch {
				// A binary that is on `PATH` but cannot be executed — a broken
				// symlink, a missing interpreter — reports a failed probe rather
				// than throwing out of the status table.
				return { output: null, failed: true };
			}
		},
		exec: (argv) => {
			try {
				// Inherited streams keep `sudo`'s password prompt on the terminal.
				const result = Bun.spawnSync([...argv], {
					stdio: ["inherit", "inherit", "inherit"],
				});
				return { exitCode: result.exitCode ?? 1 };
			} catch {
				// `Bun.spawnSync` throws when the program itself cannot be run —
				// `sudo` absent on a manager that needs it, for instance. Reporting
				// a failed exit code keeps the run on `runCli`'s failure path,
				// which prints the command and the manual fallback.
				return { exitCode: 1 };
			}
		},
		prompt: async (question) => {
			if (!process.stdin.isTTY) {
				return null;
			}
			const terminal = createInterface({ input: process.stdin, output: process.stdout });
			try {
				return await terminal.question(question);
			} finally {
				terminal.close();
			}
		},
		out: (line) => {
			console.log(line);
		},
		err: (line) => {
			console.error(line);
		},
	};
}

if (import.meta.main) {
	process.exit(await runCli(systemDeps(), process.argv.slice(2)));
}
