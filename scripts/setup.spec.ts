/**
 * Unit tests for the dependency bootstrap (`scripts/setup.ts`).
 *
 * Every test injects its environment: `has`, `probeVersion`, `exec`, `prompt`
 * and the two output sinks are fakes, so no test runs a package manager, reads
 * the network or touches this machine.
 *
 * Run: bun test
 */
import { describe, expect, test } from "bun:test";
import {
	type CliDeps,
	DEPENDENCIES,
	type DependencyId,
	type DependencyState,
	commandsFor,
	detectManager,
	evaluate,
	parseVersion,
	plan,
	runCli,
} from "./setup";

/** Build the three states from the version output each probe would print. */
function statesFrom(
	probe: Partial<Record<DependencyId, string | null>>,
): DependencyState[] {
	return DEPENDENCIES.map((spec) => {
		const versionOutput = probe[spec.id] ?? null;
		return evaluate(spec, { present: versionOutput !== null, versionOutput });
	});
}

const BUN = "1.4.4\n";
const FFMPEG = "ffmpeg version n9.0.2 Copyright (c) 2000-2026 the FFmpeg developers\n";
const FFPROBE = "ffprobe version n9.0.2 Copyright (c) 2007-2026 the FFmpeg developers\n";

/** A command table row: the exact argv lists and whether the step needs sudo. */
interface ManagerRow {
	bun: string[];
	ffmpeg: string[];
	bunSudo: boolean;
}

const CURL_BUN = ["sh", "-c", "curl -fsSL https://bun.sh/install | bash"];

/** The table of §1 of the ticket, asserted verbatim. */
const MANAGER_TABLE: Record<string, ManagerRow> = {
	apt: {
		bun: CURL_BUN,
		ffmpeg: ["sudo", "apt-get", "install", "-y", "ffmpeg"],
		bunSudo: false,
	},
	dnf: {
		bun: CURL_BUN,
		ffmpeg: ["sudo", "dnf", "install", "-y", "ffmpeg"],
		bunSudo: false,
	},
	pacman: {
		bun: CURL_BUN,
		ffmpeg: ["sudo", "pacman", "-S", "--needed", "--noconfirm", "ffmpeg"],
		bunSudo: false,
	},
	zypper: {
		bun: CURL_BUN,
		ffmpeg: ["sudo", "zypper", "install", "-y", "ffmpeg"],
		bunSudo: false,
	},
	apk: {
		bun: CURL_BUN,
		ffmpeg: ["sudo", "apk", "add", "ffmpeg"],
		bunSudo: false,
	},
	brew: {
		bun: ["brew", "install", "oven-sh/bun/bun"],
		ffmpeg: ["brew", "install", "ffmpeg"],
		bunSudo: false,
	},
	winget: {
		bun: ["powershell", "-NoProfile", "-Command", "irm bun.sh/install.ps1 | iex"],
		ffmpeg: [
			"winget",
			"install",
			"--id",
			"Gyan.FFmpeg",
			"-e",
			"--accept-package-agreements",
			"--accept-source-agreements",
		],
		bunSudo: false,
	},
};

/** The managers whose ffmpeg install needs root: the Linux package managers. */
const SUDO_MANAGERS: readonly string[] = ["apt", "dnf", "pacman", "zypper", "apk"];

describe("detectManager", () => {
	for (const [platform, candidates] of [
		["linux", ["apt", "dnf", "pacman", "zypper", "apk"]],
		["darwin", ["brew"]],
		["win32", ["winget"]],
	] as const) {
		for (const candidate of candidates) {
			test(`${platform} finds ${candidate}`, () => {
				const has = (bin: string) => bin === candidate;
				expect(detectManager(platform, has)).toBe(candidate as never);
			});
		}
	}

	test("linux prefers the first candidate of the probe order", () => {
		expect(detectManager("linux", () => true)).toBe("apt");
		expect(detectManager("linux", (bin) => bin === "dnf" || bin === "pacman")).toBe("dnf");
		expect(detectManager("linux", (bin) => bin === "pacman" || bin === "zypper")).toBe("pacman");
		expect(detectManager("linux", (bin) => bin === "zypper" || bin === "apk")).toBe("zypper");
	});

	test("macOS never reports a Linux manager, and the reverse", () => {
		expect(detectManager("darwin", (bin) => bin === "apt")).toBeNull();
		expect(detectManager("linux", (bin) => bin === "brew")).toBeNull();
		expect(detectManager("win32", (bin) => bin === "brew" || bin === "apt")).toBeNull();
	});

	test("no known manager on PATH is null", () => {
		expect(detectManager("linux", () => false)).toBeNull();
		expect(detectManager("darwin", () => false)).toBeNull();
		expect(detectManager("win32", () => false)).toBeNull();
	});

	test("an unknown platform is null even when every manager is present", () => {
		expect(detectManager("freebsd" as NodeJS.Platform, () => true)).toBeNull();
		expect(detectManager("aix" as NodeJS.Platform, () => true)).toBeNull();
	});

	test("probes its candidates in the documented order", () => {
		const seen: string[] = [];
		detectManager("linux", (bin) => {
			seen.push(bin);
			return false;
		});
		expect(seen).toEqual(["apt", "dnf", "pacman", "zypper", "apk"]);
	});
});

describe("parseVersion", () => {
	test("reads the bun banner", () => {
		expect(parseVersion("1.4.4\n")).toEqual([1, 4, 4]);
		expect(parseVersion("bun 1.4.4")).toEqual([1, 4, 4]);
	});

	test("reads an ffmpeg banner with a static suffix", () => {
		expect(parseVersion("ffmpeg version 7.1.2-static")).toEqual([7, 1, 2]);
	});

	test("reads an ffprobe banner with a prefixed build number", () => {
		expect(parseVersion("ffprobe version n9.0.2")).toEqual([9, 0, 2]);
	});

	test("reads the first three-integer run of a long banner", () => {
		expect(parseVersion(FFMPEG)).toEqual([9, 0, 2]);
		expect(parseVersion(FFPROBE)).toEqual([9, 0, 2]);
	});

	test("keeps multi-digit components", () => {
		expect(parseVersion("ffmpeg version 10.11.12")).toEqual([10, 11, 12]);
	});

	test("garbage is null", () => {
		expect(parseVersion("")).toBeNull();
		expect(parseVersion("bun: command not found")).toBeNull();
		expect(parseVersion("ffmpeg version static")).toBeNull();
	});

	test("fewer than three components is null", () => {
		expect(parseVersion("bun 1")).toBeNull();
		expect(parseVersion("bun 1.4")).toBeNull();
		expect(parseVersion("version 2026.11")).toBeNull();
	});
});

describe("minimum versions", () => {
	test("bun 1.4.0 and ffmpeg/ffprobe 6.0.0", () => {
		const minimums = Object.fromEntries(DEPENDENCIES.map((spec) => [spec.id, spec.minimum]));
		expect(minimums).toEqual({ bun: [1, 4, 0], ffmpeg: [6, 0, 0], ffprobe: [6, 0, 0] });
	});

	test("the version argv matches each binary", () => {
		const args = Object.fromEntries(DEPENDENCIES.map((spec) => [spec.id, spec.versionArgs]));
		expect(args).toEqual({ bun: ["--version"], ffmpeg: ["-version"], ffprobe: ["-version"] });
	});
});

describe("evaluate", () => {
	const bun = DEPENDENCIES[0]!;

	test("an absent binary is missing", () => {
		expect(evaluate(bun, { present: false, versionOutput: null })).toEqual({
			id: "bun",
			present: false,
			version: null,
			status: "missing",
		});
	});

	test("exactly the minimum is ok", () => {
		expect(evaluate(bun, { present: true, versionOutput: "1.4.0" })).toEqual({
			id: "bun",
			present: true,
			version: [1, 4, 0],
			status: "ok",
		});
	});

	test("one patch above is ok", () => {
		expect(evaluate(bun, { present: true, versionOutput: "1.4.1" }).status).toBe("ok");
	});

	test("one patch below is outdated", () => {
		expect(evaluate(bun, { present: true, versionOutput: "1.3.9" })).toEqual({
			id: "bun",
			present: true,
			version: [1, 3, 9],
			status: "outdated",
		});
	});

	test("a lower major is outdated", () => {
		expect(evaluate(bun, { present: true, versionOutput: "0.8.0" }).status).toBe("outdated");
	});

	test("a higher major is ok", () => {
		expect(evaluate(bun, { present: true, versionOutput: "2.0.0" }).status).toBe("ok");
	});

	test("a present binary whose version cannot be read counts as ok", () => {
		// The gate only rejects a version it can read and finds too old: an
		// unreadable banner must not trigger an install over a working binary.
		expect(evaluate(bun, { present: true, versionOutput: "bun (development build)" })).toEqual({
			id: "bun",
			present: true,
			version: null,
			status: "ok",
		});
		expect(evaluate(bun, { present: true, versionOutput: null }).status).toBe("ok");
		expect(
			evaluate(bun, { present: true, versionOutput: null, probeFailed: false }).status,
		).toBe("ok");
	});

	test("a present binary whose probe failed is missing, not ok", () => {
		// A probe that exits non-zero says the binary cannot run, which is not
		// the same as a banner this build cannot parse: reinstalling is the fix,
		// so the status must produce a step.
		expect(
			evaluate(bun, { present: true, versionOutput: null, probeFailed: true }),
		).toEqual({
			id: "bun",
			present: true,
			version: null,
			status: "missing",
		});
	});

	test("a failed probe produces a plan step where a good probe does not", () => {
		const broken = plan([evaluate(bun, { present: true, versionOutput: null, probeFailed: true })], "brew");
		expect(broken.steps.map((step) => step.id)).toEqual(["bun"]);

		const unreadable = plan(
			[evaluate(bun, { present: true, versionOutput: "unknown build", probeFailed: false })],
			"brew",
		);
		expect(unreadable.steps).toEqual([]);
	});
});

describe("plan", () => {
	test("emits nothing when every dependency is ok", () => {
		const result = plan(statesFrom({ bun: BUN, ffmpeg: FFMPEG, ffprobe: FFPROBE }), "pacman");
		expect(result.steps).toEqual([]);
		expect(result.instructions).toEqual([]);
	});

	test("emits one ffmpeg step when ffmpeg is missing", () => {
		const result = plan(statesFrom({ bun: BUN, ffprobe: FFPROBE }), "pacman");
		expect(result.steps).toEqual([
			{
				id: "ffmpeg",
				commands: [["sudo", "pacman", "-S", "--needed", "--noconfirm", "ffmpeg"]],
				requiresSudo: true,
			},
		]);
	});

	test("emits one ffmpeg step when only ffprobe is missing (deduplicated)", () => {
		const result = plan(statesFrom({ bun: BUN, ffmpeg: FFMPEG }), "pacman");
		expect(result.steps.map((step) => step.id)).toEqual(["ffmpeg"]);
	});

	test("keeps a single ffmpeg step when both are missing", () => {
		const result = plan(statesFrom({ bun: BUN }), "brew");
		expect(result.steps).toEqual([
			{ id: "ffmpeg", commands: [["brew", "install", "ffmpeg"]], requiresSudo: false },
		]);
	});

	test("orders bun before ffmpeg", () => {
		const result = plan(statesFrom({}), "dnf");
		expect(result.steps.map((step) => step.id)).toEqual(["bun", "ffmpeg"]);
	});

	test("an outdated dependency produces a step", () => {
		const result = plan(statesFrom({ bun: "1.3.9", ffmpeg: FFMPEG, ffprobe: FFPROBE }), "apk");
		expect(result.steps).toEqual([
			{ id: "bun", commands: [CURL_BUN], requiresSudo: false },
		]);
	});

	test("the step order is stable across input permutations", () => {
		const states = statesFrom({});
		const reversed = [...states].reverse();
		const forward = plan(states, "zypper").steps.map((step) => step.id);
		const backward = plan(reversed, "zypper").steps.map((step) => step.id);
		expect(forward).toEqual(["bun", "ffmpeg"]);
		expect(backward).toEqual(forward);
	});

	test("an unknown manager emits no step but manual instructions", () => {
		const result = plan(statesFrom({ bun: BUN }), null);
		expect(result.steps).toEqual([]);
		expect(result.instructions.length).toBeGreaterThan(0);
		const text = result.instructions.join("\n");
		expect(text).toContain("ffmpeg");
		expect(text).not.toContain("apt-get");
		expect(text).not.toContain("pacman");
		expect(text).not.toContain("brew");
	});

	test("an unknown manager with everything present has no instructions", () => {
		const result = plan(statesFrom({ bun: BUN, ffmpeg: FFMPEG, ffprobe: FFPROBE }), null);
		expect(result.steps).toEqual([]);
		expect(result.instructions).toEqual([]);
	});

	test("the platform names the manager family", () => {
		expect(plan(statesFrom({}), "brew").platform).toEqual({ os: "macos", manager: "brew" });
		expect(plan(statesFrom({}), "winget").platform).toEqual({
			os: "windows",
			manager: "winget",
		});
		expect(plan(statesFrom({}), "pacman").platform).toEqual({ os: "linux", manager: "pacman" });
	});

	test("the states are carried through unchanged", () => {
		const states = statesFrom({ bun: BUN });
		expect(plan(states, "apt").states).toEqual(states);
	});
});

describe("commandsFor", () => {
	for (const [manager, row] of Object.entries(MANAGER_TABLE)) {
		test(`${manager} runs the documented commands`, () => {
			expect(commandsFor("bun", manager as never)).toEqual([row.bun]);
			expect(commandsFor("ffmpeg", manager as never)).toEqual([row.ffmpeg]);
			expect(commandsFor("ffprobe", manager as never)).toEqual([row.ffmpeg]);
		});

		test(`${manager} marks only the ffmpeg step as needing sudo`, () => {
			const steps = plan(statesFrom({}), manager as never).steps;
			expect(steps).toEqual([
				{ id: "bun", commands: [row.bun], requiresSudo: row.bunSudo },
				{ id: "ffmpeg", commands: [row.ffmpeg], requiresSudo: SUDO_MANAGERS.includes(manager) },
			]);
		});
	}

	test("the curl bootstrap is executed through sh -c", () => {
		expect(commandsFor("bun", "apt")[0]?.slice(0, 2)).toEqual(["sh", "-c"]);
	});
});

/** One injected CLI environment plus the records its fakes keep. */
interface Harness {
	readonly deps: CliDeps;
	/** The fake `PATH`: a binary is present when its version output is in here. */
	readonly versions: Map<string, string>;
	readonly execCalls: (readonly string[])[];
	readonly prompts: string[];
	readonly stdout: string[];
	readonly stderr: string[];
}

function harness(options: {
	platform?: NodeJS.Platform;
	versions?: Partial<Record<DependencyId, string | null>>;
	/** Binaries present on the fake `PATH` whose version probe fails. */
	brokenProbes?: DependencyId[];
	/** Package-manager binaries the fake `PATH` reports, on top of the versions. */
	managers?: string[];
	answer?: string | null;
	execExitCode?: (call: number) => number;
	onExec?: (argv: readonly string[]) => void;
} = {}): Harness {
	const versions = new Map<string, string>(
		Object.entries(options.versions ?? {}).flatMap(([bin, output]) =>
			output === null || output === undefined ? [] : [[bin, output] as [string, string]],
		),
	);
	const broken = new Set<string>(options.brokenProbes ?? []);
	const managers = new Set(options.managers ?? ["pacman"]);
	const execCalls: (readonly string[])[] = [];
	const prompts: string[] = [];
	const stdout: string[] = [];
	const stderr: string[] = [];

	const deps: CliDeps = {
		platform: options.platform ?? "linux",
		has: (bin) => versions.has(bin) || broken.has(bin) || managers.has(bin),
		probeVersion: (bin) =>
			broken.has(bin)
				? { output: null, failed: true }
				: { output: versions.get(bin) ?? null, failed: false },
		exec: (argv) => {
			execCalls.push(argv);
			options.onExec?.(argv);
			const exitCode = options.execExitCode?.(execCalls.length) ?? 0;
			return { exitCode };
		},
		prompt: (question) => {
			prompts.push(question);
			return options.answer ?? null;
		},
		out: (line) => {
			stdout.push(line);
		},
		err: (line) => {
			stderr.push(line);
		},
	};

	return { deps, versions, execCalls, prompts, stdout, stderr };
}

/** `bun`, `ffmpeg` and `ffprobe` all present and recent. */
const ALL_PRESENT = { bun: BUN, ffmpeg: FFMPEG, ffprobe: FFPROBE };

const lineMatches = (lines: readonly string[], pattern: RegExp) =>
	lines.some((line) => pattern.test(line));

describe("runCli --check", () => {
	test("exits 0 with three ok rows and touches nothing", async () => {
		const fake = harness({ versions: ALL_PRESENT });
		const code = await runCli(fake.deps, ["--check"]);

		expect(code).toBe(0);
		expect(fake.execCalls).toEqual([]);
		expect(fake.prompts).toEqual([]);
		expect(lineMatches(fake.stdout, /^bun\s+1\.4\.4\s+ok$/)).toBe(true);
		expect(lineMatches(fake.stdout, /^ffmpeg\s+9\.0\.2\s+ok$/)).toBe(true);
		expect(lineMatches(fake.stdout, /^ffprobe\s+9\.0\.2\s+ok$/)).toBe(true);
	});

	test("exits 1 when a dependency is missing, and never runs a command", async () => {
		const fake = harness({ versions: { bun: BUN, ffprobe: FFPROBE } });
		const code = await runCli(fake.deps, ["--check"]);

		expect(code).toBe(1);
		expect(fake.execCalls).toEqual([]);
		expect(fake.prompts).toEqual([]);
		expect(lineMatches(fake.stdout, /^ffmpeg\s+—\s+missing$/)).toBe(true);
	});

	test("exits 1 when a dependency is outdated", async () => {
		const fake = harness({ versions: { ...ALL_PRESENT, bun: "1.3.9" } });
		const code = await runCli(fake.deps, ["--check"]);

		expect(code).toBe(1);
		expect(fake.execCalls).toEqual([]);
		expect(lineMatches(fake.stdout, /^bun\s+1\.3\.9\s+outdated/)).toBe(true);
	});

	test("an unknown manager prints the manual instructions and exits 1", async () => {
		const fake = harness({ versions: { bun: BUN }, managers: [] });
		const code = await runCli(fake.deps, ["--check"]);

		expect(code).toBe(1);
		expect(fake.execCalls).toEqual([]);
		// The status row already names ffmpeg, so the heading and the linked
		// instruction are what prove the manual block printed.
		expect(fake.stdout).toContain("Manual install:");
		expect(lineMatches(fake.stdout, /ffmpeg\.org\/download\.html/)).toBe(true);
		expect(lineMatches(fake.stdout, /apt-get|pacman|brew|winget/)).toBe(false);
	});

	test("a binary whose probe failed is missing, not ok", async () => {
		const fake = harness({ versions: { ffmpeg: FFMPEG, ffprobe: FFPROBE }, brokenProbes: ["bun"] });
		const code = await runCli(fake.deps, ["--check"]);

		expect(code).toBe(1);
		expect(lineMatches(fake.stdout, /^bun\s+—\s+missing$/)).toBe(true);
	});
});

describe("runCli --dry-run", () => {
	test("prints the exact commands, changes nothing, exits 0", async () => {
		const fake = harness({ versions: { bun: BUN, ffprobe: FFPROBE } });
		const code = await runCli(fake.deps, ["--dry-run"]);

		expect(code).toBe(0);
		expect(fake.execCalls).toEqual([]);
		expect(fake.prompts).toEqual([]);
		expect(lineMatches(fake.stdout, /^ffmpeg\s+—\s+missing$/)).toBe(true);
		expect(lineMatches(fake.stdout, /sudo pacman -S --needed --noconfirm ffmpeg/)).toBe(true);
	});

	test("renders the bun bootstrap as the shell line it runs", async () => {
		const fake = harness({ versions: { ffmpeg: FFMPEG, ffprobe: FFPROBE } });
		const code = await runCli(fake.deps, ["--dry-run"]);

		expect(code).toBe(0);
		expect(fake.execCalls).toEqual([]);
		expect(lineMatches(fake.stdout, /curl -fsSL https:\/\/bun\.sh\/install \| bash/)).toBe(true);
	});

	test("marks a step that needs root before asking", async () => {
		const fake = harness({ versions: { bun: BUN } });
		await runCli(fake.deps, ["--dry-run"]);

		expect(
			lineMatches(fake.stdout, /^\s+sudo pacman -S --needed --noconfirm ffmpeg {2}\(needs root\)$/),
		).toBe(true);
	});

	test("does not mark a user-level install as needing root", async () => {
		const fake = harness({ versions: { bun: BUN }, platform: "darwin", managers: ["brew"] });
		await runCli(fake.deps, ["--dry-run"]);

		expect(lineMatches(fake.stdout, /^\s+brew install ffmpeg$/)).toBe(true);
		expect(lineMatches(fake.stdout, /needs root/)).toBe(false);
	});
});

describe("runCli default mode", () => {
	test("installs nothing on non-interactive stdin and exits 1", async () => {
		const fake = harness({ versions: { bun: BUN }, answer: null });
		const code = await runCli(fake.deps, []);

		expect(code).toBe(1);
		expect(fake.execCalls).toEqual([]);
		expect(fake.prompts).toHaveLength(1);
	});

	test("installs nothing when the answer is not y", async () => {
		for (const answer of ["", "n", "no", "N", "  "]) {
			const fake = harness({ versions: { bun: BUN }, answer });
			const code = await runCli(fake.deps, []);
			expect(code).toBe(1);
			expect(fake.execCalls).toEqual([]);
		}
	});

	test("installs after an explicit y, then re-probes and reports ready", async () => {
		const fake = harness({
			versions: { bun: BUN },
			answer: "y",
			onExec: () => {
				fake.versions.set("ffmpeg", FFMPEG);
				fake.versions.set("ffprobe", FFPROBE);
			},
		});
		const code = await runCli(fake.deps, []);

		expect(code).toBe(0);
		expect(fake.execCalls).toEqual([["sudo", "pacman", "-S", "--needed", "--noconfirm", "ffmpeg"]]);
		expect(lineMatches(fake.stdout, /^ffmpeg\s+9\.0\.2\s+ok$/)).toBe(true);
		expect(lineMatches(fake.stdout, /All dependencies are ready/)).toBe(true);
	});

	test("--yes skips the confirmation prompt", async () => {
		const fake = harness({
			versions: { ffmpeg: FFMPEG, ffprobe: FFPROBE },
			onExec: () => {
				fake.versions.set("bun", BUN);
			},
		});
		const code = await runCli(fake.deps, ["--yes"]);

		expect(code).toBe(0);
		expect(fake.prompts).toEqual([]);
		expect(fake.execCalls).toEqual([CURL_BUN]);
	});

	test("a failing command stops the run and keeps the earlier install", async () => {
		const fake = harness({
			answer: "y",
			execExitCode: (call) => (call === 1 ? 0 : 1),
			onExec: (argv) => {
				if (argv.join(" ") === CURL_BUN.join(" ")) fake.versions.set("bun", BUN);
			},
		});
		const code = await runCli(fake.deps, []);

		expect(code).toBe(1);
		expect(fake.execCalls).toEqual([CURL_BUN, MANAGER_TABLE.pacman!.ffmpeg]);
		expect(lineMatches(fake.stderr, /exit code 1/)).toBe(true);
		expect(lineMatches(fake.stderr, /sudo pacman -S --needed --noconfirm ffmpeg/)).toBe(true);
		expect(lineMatches(fake.stdout, /^bun\s+1\.4\.4\s+ok$/)).toBe(true);
	});

	test("everything ok needs no prompt and no command", async () => {
		const fake = harness({ versions: ALL_PRESENT });
		const code = await runCli(fake.deps, []);

		expect(code).toBe(0);
		expect(fake.execCalls).toEqual([]);
		expect(fake.prompts).toEqual([]);
	});

	test("an unknown manager never installs anything", async () => {
		const fake = harness({ versions: { bun: BUN }, managers: [], answer: "y" });
		const code = await runCli(fake.deps, []);

		expect(code).toBe(1);
		expect(fake.execCalls).toEqual([]);
		expect(fake.prompts).toEqual([]);
		expect(fake.stdout).toContain("Manual install:");
		expect(lineMatches(fake.stdout, /ffmpeg\.org\/download\.html/)).toBe(true);
	});
});

describe("runCli usage", () => {
	test("an unknown flag exits 2 with the usage on stderr", async () => {
		const fake = harness({ versions: ALL_PRESENT });
		const code = await runCli(fake.deps, ["--nope"]);

		expect(code).toBe(2);
		expect(fake.execCalls).toEqual([]);
		expect(lineMatches(fake.stderr, /--check/)).toBe(true);
		expect(fake.stdout).toEqual([]);
	});

	test("--check wins over --yes: it reports and never installs", async () => {
		const fake = harness({ versions: { bun: BUN }, answer: "y" });
		const code = await runCli(fake.deps, ["--check", "--yes"]);

		expect(code).toBe(1);
		expect(fake.execCalls).toEqual([]);
		expect(fake.prompts).toEqual([]);
		expect(lineMatches(fake.stdout, /Not ready: ffmpeg, ffprobe/)).toBe(true);
	});

	test("--dry-run wins over --yes: it prints and never installs", async () => {
		const fake = harness({ versions: { bun: BUN }, answer: "y" });
		const code = await runCli(fake.deps, ["--dry-run", "--yes"]);

		expect(code).toBe(0);
		expect(fake.execCalls).toEqual([]);
		expect(fake.prompts).toEqual([]);
		expect(lineMatches(fake.stdout, /sudo pacman -S --needed --noconfirm ffmpeg/)).toBe(true);
	});
});
