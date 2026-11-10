/**
 * Unit tests for the AppImage packaging pipeline (`scripts/package.ts`).
 *
 * Every test injects its environment: `exec`, `fetch`, `exists`, `readFile`, the filesystem sinks
 * and the output sinks are fakes, so no test runs `cargo`, `bun build`, `fetch` or `gh`, and no
 * test touches this machine's `dist/` directory.
 *
 * Run: bun test
 */
import { describe, expect, test } from "bun:test";
import {
	type CliDeps,
	appImageFileName,
	appimagetoolArgs,
	appRunScriptLinux,
	archFrom,
	checksumLine,
	desktopEntryLinux,
	releaseCommand,
	runCli,
	stagePlanLinux,
	versionFromMetadata,
	workerBundleArgs
} from "./package";

const VERSION_METADATA = JSON.stringify({
	packages: [
		{ name: "speechtt-core", version: "0.5.0" },
		{ name: "speechtt-ui", version: "0.5.0" }
	]
});

const DIGEST = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

describe("appImageFileName", () => {
	test("names the artifact after the version and architecture", () => {
		expect(appImageFileName("0.5.0", "x86_64")).toBe("SpeechTT-0.5.0-x86_64.AppImage");
		expect(appImageFileName("1.0.0", "aarch64")).toBe("SpeechTT-1.0.0-aarch64.AppImage");
	});

	test("does not put the platform in the file name", () => {
		expect(appImageFileName("0.5.0", "x86_64")).not.toContain("Linux");
	});
});

describe("archFrom", () => {
	test("maps the two supported uname -m spellings", () => {
		expect(archFrom("x86_64")).toBe("x86_64");
		expect(archFrom("aarch64")).toBe("aarch64");
	});

	test("accepts the Node process.arch spellings used when uname fails", () => {
		expect(archFrom("x64")).toBe("x86_64");
		expect(archFrom("arm64")).toBe("aarch64");
	});

	test("rejects an unsupported architecture with the value", () => {
		expect(() => archFrom("riscv64")).toThrow(/riscv64/);
		expect(() => archFrom("i686")).toThrow(/i686/);
	});
});

describe("versionFromMetadata", () => {
	test("reads the workspace version when every package agrees", () => {
		expect(versionFromMetadata(VERSION_METADATA)).toBe("0.5.0");
	});

	test("a single package is enough", () => {
		expect(versionFromMetadata(JSON.stringify({ packages: [{ version: "1.0.0" }] }))).toBe(
			"1.0.0"
		);
	});

	test("names both versions when the workspace disagrees", () => {
		const metadata = JSON.stringify({
			packages: [
				{ name: "speechtt-core", version: "0.5.0" },
				{ name: "speechtt-ui", version: "0.6.0" }
			]
		});

		expect(() => versionFromMetadata(metadata)).toThrow(/0\.5\.0/);
		expect(() => versionFromMetadata(metadata)).toThrow(/0\.6\.0/);
	});

	test("rejects metadata with no package version", () => {
		expect(() => versionFromMetadata(JSON.stringify({ packages: [] }))).toThrow(/version/i);
		expect(() => versionFromMetadata(JSON.stringify({ packages: [{}] }))).toThrow(/version/i);
	});

	test("rejects malformed JSON", () => {
		expect(() => versionFromMetadata("not json")).toThrow();
	});
});

describe("AppRun and desktop entry", () => {
	test("the AppRun script launches the payload next to itself", () => {
		expect(appRunScriptLinux()).toBe(
			[
				"#!/bin/sh",
				"# Resolve the AppImage mount point, not the caller's working directory:",
				"# the widget locates its worker relative to its own executable.",
				"set -eu",
				'HERE="$(dirname "$(readlink -f "$0")")"',
				'exec "$HERE/usr/bin/speechtt-ui" "$@"',
				""
			].join("\n")
		);
	});

	test("the desktop entry matches the icon and executable names", () => {
		expect(desktopEntryLinux()).toBe(
			[
				"[Desktop Entry]",
				"Type=Application",
				"Name=SpeechTT",
				"Comment=Floating speech-to-text widget",
				"Exec=speechtt-ui",
				"Icon=speechtt",
				"Terminal=false",
				"Categories=AudioVideo;Audio;Utility;",
				""
			].join("\n")
		);
	});
});

describe("stagePlanLinux", () => {
	const plan = stagePlanLinux();

	test("stages exactly the AppDir tree of the contract", () => {
		expect(plan.appDir).toBe("dist/SpeechTT.AppDir");
		expect(plan.files.map((file) => file.path)).toEqual([
			"AppRun",
			"speechtt.desktop",
			"speechtt.png",
			"usr/bin/speechtt-ui",
			"usr/share/speechtt/worker.js",
			"usr/share/applications/speechtt.desktop",
			"usr/share/icons/hicolor/256x256/apps/speechtt.png",
			"usr/share/licenses/speechtt/LICENSE"
		]);
	});

	test("uses the contract's file modes", () => {
		const modes = Object.fromEntries(plan.files.map((file) => [file.path, file.mode]));
		expect(modes).toEqual({
			AppRun: 0o755,
			"speechtt.desktop": 0o644,
			"speechtt.png": 0o644,
			"usr/bin/speechtt-ui": 0o755,
			"usr/share/speechtt/worker.js": 0o644,
			"usr/share/applications/speechtt.desktop": 0o644,
			"usr/share/icons/hicolor/256x256/apps/speechtt.png": 0o644,
			"usr/share/licenses/speechtt/LICENSE": 0o644
		});
	});

	test("generates the launcher and desktop entry twice from one source", () => {
		const appRun = plan.files.find((file) => file.path === "AppRun");
		expect(appRun).toEqual({
			path: "AppRun",
			kind: "generated",
			contents: appRunScriptLinux(),
			mode: 0o755
		});

		const desktops = plan.files.filter(
			(file) => file.kind === "generated" && file.contents === desktopEntryLinux()
		);
		expect(desktops.map((file) => file.path)).toEqual([
			"speechtt.desktop",
			"usr/share/applications/speechtt.desktop"
		]);
	});

	test("copies the binary, the worker, the icon and the licence", () => {
		const copies = Object.fromEntries(
			plan.files
				.filter((file) => file.kind === "copy")
				.map((file) => [file.path, file.kind === "copy" ? file.source : ""])
		);
		expect(copies).toEqual({
			"speechtt.png": "packaging/speechtt.png",
			"usr/bin/speechtt-ui": "target/release/speechtt-ui",
			"usr/share/speechtt/worker.js": "dist/worker.js",
			"usr/share/icons/hicolor/256x256/apps/speechtt.png": "packaging/speechtt.png",
			"usr/share/licenses/speechtt/LICENSE": "LICENSE"
		});
	});
});

describe("command builders", () => {
	test("bundles the worker into one file for Bun", () => {
		expect(workerBundleArgs()).toEqual([
			"build",
			"src/stt/worker.ts",
			"--target=bun",
			"--outfile",
			"dist/worker.js"
		]);
	});

	test("extracts and runs appimagetool without AppStream metadata", () => {
		expect(
			appimagetoolArgs("dist/SpeechTT.AppDir", "dist/SpeechTT-0.5.0-x86_64.AppImage")
		).toEqual([
			"--appimage-extract-and-run",
			"--no-appstream",
			"dist/SpeechTT.AppDir",
			"dist/SpeechTT-0.5.0-x86_64.AppImage"
		]);
	});

	test("writes one sha256sum-compatible line", () => {
		const line = checksumLine(DIGEST, "SpeechTT-0.5.0-x86_64.AppImage");
		expect(line).toBe(`${DIGEST}  SpeechTT-0.5.0-x86_64.AppImage\n`);
		expect(line).toMatch(/^[0-9a-f]{64} {2}\S+\n$/);
	});
});

describe("releaseCommand", () => {
	test("prints the exact gh command and runs nothing", () => {
		expect(releaseCommand("0.5.0", "x86_64")).toBe(
			'gh release create v0.5.0 dist/SpeechTT-0.5.0-x86_64.AppImage dist/SpeechTT-0.5.0-x86_64.AppImage.sha256 --title "SpeechTT 0.5.0" --generate-notes'
		);
	});

	test("the tag carries the v prefix and the title does not", () => {
		const command = releaseCommand("1.0.0", "aarch64");
		expect(command).toContain("v1.0.0");
		expect(command).toContain('--title "SpeechTT 1.0.0"');
		expect(command).not.toContain('--title "SpeechTT v1.0.0"');
	});
});

interface ExecCall {
	readonly argv: readonly string[];
	readonly env: Readonly<Record<string, string>>;
}

interface Harness {
	deps: CliDeps;
	readonly execCalls: ExecCall[];
	readonly fetchCalls: { url: string; destination: string }[];
	readonly copied: { from: string; to: string }[];
	readonly written: { path: string; contents: string }[];
	readonly removed: string[];
	readonly chmodCalls: { path: string; mode: number }[];
	readonly stdout: string[];
	readonly stderr: string[];
	readonly files: Set<string>;
	bundle: string;
}

function harness(
	options: {
		machine?: string;
		has?: (bin: string) => boolean;
		metadata?: string;
		exists?: (path: string) => boolean;
		bundle?: string;
		digest?: string;
		onExec?: (argv: readonly string[]) => void;
		exitCode?: (argv: readonly string[]) => number;
		stderr?: (argv: readonly string[]) => string;
		copyError?: Error;
		fetchError?: Error;
		sha256Error?: Error;
		execThrows?: (argv: readonly string[]) => boolean;
	} = {}
): Harness {
	const execCalls: ExecCall[] = [];
	const fetchCalls: { url: string; destination: string }[] = [];
	const copied: { from: string; to: string }[] = [];
	const written: { path: string; contents: string }[] = [];
	const removed: string[] = [];
	const chmodCalls: { path: string; mode: number }[] = [];
	const stdout: string[] = [];
	const stderr: string[] = [];
	const files = new Set<string>();

	const state: Harness = {
		deps: undefined as unknown as CliDeps,
		execCalls,
		fetchCalls,
		copied,
		written,
		removed,
		chmodCalls,
		stdout,
		stderr,
		files,
		bundle: options.bundle ?? "export const ready = true;\n"
	};

	const deps: CliDeps = {
		machine: options.machine ?? "x86_64",
		has: options.has ?? (() => true),
		exec: (argv, execOptions) => {
			execCalls.push({ argv, env: execOptions?.env ?? {} });
			if (options.execThrows?.(argv)) {
				throw new Error(`cannot run ${argv[0]}`);
			}
			if (argv[0] === "cargo" && argv[1] === "metadata") {
				return { exitCode: 0, stdout: options.metadata ?? VERSION_METADATA, stderr: "" };
			}
			options.onExec?.(argv);
			return {
				exitCode: options.exitCode?.(argv) ?? 0,
				stdout: "",
				stderr: options.stderr?.(argv) ?? ""
			};
		},
		fetch: async (url, destination) => {
			fetchCalls.push({ url, destination });
			if (options.fetchError) {
				throw options.fetchError;
			}
			files.add(destination);
		},
		exists: (path) => (options.exists ? options.exists(path) : files.has(path)),
		readFile: (path) => (path === "dist/worker.js" ? state.bundle : ""),
		writeFile: (path, contents) => {
			written.push({ path, contents });
			files.add(path);
		},
		copyFile: (from, to) => {
			if (options.copyError) {
				throw options.copyError;
			}
			copied.push({ from, to });
			files.add(to);
		},
		makeDir: () => {},
		removePath: (path) => {
			removed.push(path);
		},
		chmod: (path, mode) => {
			chmodCalls.push({ path, mode });
		},
		sha256: () => {
			if (options.sha256Error) {
				throw options.sha256Error;
			}
			return options.digest ?? DIGEST;
		},
		out: (line) => {
			stdout.push(line);
		},
		err: (line) => {
			stderr.push(line);
		}
	};

	state.deps = deps;
	return state;
}

const lineMatches = (lines: readonly string[], pattern: RegExp) =>
	lines.some((line) => pattern.test(line));

const METADATA_ARGV = ["cargo", "metadata", "--format-version", "1", "--no-deps"];

describe("runCli --dry-run", () => {
	test("prints every command and staged path, runs no build, exits 0", async () => {
		const fake = harness();

		const code = await runCli(["--dry-run"], fake.deps);

		expect(code).toBe(0);
		expect(fake.execCalls.map((call) => [...call.argv])).toEqual([METADATA_ARGV]);
		expect(fake.fetchCalls).toEqual([]);
		expect(fake.copied).toEqual([]);
		expect(fake.written).toEqual([]);
		expect(fake.removed).toEqual([]);
		expect(
			lineMatches(
				fake.stdout,
				/^bun build src\/stt\/worker\.ts --target=bun --outfile dist\/worker\.js$/
			)
		).toBe(true);
		expect(lineMatches(fake.stdout, /^cargo build --release --locked -p speechtt-ui$/)).toBe(
			true
		);
		expect(
			lineMatches(
				fake.stdout,
				/appimagetool-x86_64\.AppImage --appimage-extract-and-run --no-appstream/
			)
		).toBe(true);
		expect(
			lineMatches(fake.stdout, /dist\/SpeechTT\.AppDir\/usr\/share\/speechtt\/worker\.js/)
		).toBe(true);
		expect(lineMatches(fake.stdout, /dist\/SpeechTT\.AppDir\/usr\/bin\/speechtt-ui/)).toBe(
			true
		);
		expect(lineMatches(fake.stdout, /^gh release create v0\.5\.0 /)).toBe(true);
	});

	test("--arch overrides the detected architecture in every name", async () => {
		const fake = harness({ machine: "x86_64" });

		const code = await runCli(["--dry-run", "--arch", "aarch64"], fake.deps);

		expect(code).toBe(0);
		expect(lineMatches(fake.stdout, /SpeechTT-0\.5\.0-aarch64\.AppImage/)).toBe(true);
		expect(
			lineMatches(
				fake.stdout,
				/^gh release create v0\.5\.0 dist\/SpeechTT-0\.5\.0-aarch64\.AppImage/
			)
		).toBe(true);
	});
});

describe("runCli --no-build", () => {
	test("reuses the existing binary and bundle, then packages them", async () => {
		const fake = harness({
			exists: (path) =>
				path === "target/release/speechtt-ui" ||
				path === "dist/worker.js" ||
				path === "dist/tools/appimagetool-x86_64.AppImage"
		});

		const code = await runCli(["--no-build"], fake.deps);

		expect(code).toBe(0);
		expect(fake.execCalls.map((call) => [...call.argv])).toEqual([
			METADATA_ARGV,
			[
				"dist/tools/appimagetool-x86_64.AppImage",
				"--appimage-extract-and-run",
				"--no-appstream",
				"dist/SpeechTT.AppDir",
				"dist/SpeechTT-0.5.0-x86_64.AppImage"
			]
		]);
		expect(fake.execCalls[1]?.env).toEqual({ ARCH: "x86_64" });
		expect(fake.fetchCalls).toEqual([]);
		expect(fake.written.filter((file) => file.path.endsWith(".sha256"))).toEqual([
			{
				path: "dist/SpeechTT-0.5.0-x86_64.AppImage.sha256",
				contents: `${DIGEST}  SpeechTT-0.5.0-x86_64.AppImage\n`
			}
		]);
		expect(fake.stdout.some((line) => line.startsWith("gh release create v0.5.0"))).toBe(true);
	});

	test("fails with exit 1 when the release binary is absent", async () => {
		const fake = harness({ exists: (path) => path === "dist/worker.js" });

		const code = await runCli(["--no-build"], fake.deps);

		expect(code).toBe(1);
		expect(lineMatches(fake.stderr, /target\/release\/speechtt-ui/)).toBe(true);
		expect(fake.execCalls.map((call) => [...call.argv])).toEqual([METADATA_ARGV]);
	});

	test("fails with exit 1 when the worker bundle is absent", async () => {
		const fake = harness({ exists: (path) => path === "target/release/speechtt-ui" });

		const code = await runCli(["--no-build"], fake.deps);

		expect(code).toBe(1);
		expect(lineMatches(fake.stderr, /dist\/worker\.js/)).toBe(true);
		expect(fake.execCalls.map((call) => [...call.argv])).toEqual([METADATA_ARGV]);
	});

	test("a reused bundle that still imports ai fails before packaging", async () => {
		const fake = harness({
			bundle: 'import { generateText } from "ai";\n',
			exists: (path) =>
				path === "target/release/speechtt-ui" ||
				path === "dist/worker.js" ||
				path === "dist/tools/appimagetool-x86_64.AppImage"
		});

		const code = await runCli(["--no-build"], fake.deps);

		expect(code).toBe(1);
		expect(fake.stderr.join("\n")).toContain(
			"still imports `ai` or `@ai-sdk/*`; the AppImage ships no node_modules"
		);
		expect(fake.execCalls.map((call) => [...call.argv])).toEqual([METADATA_ARGV]);
	});

	test("a cached appimagetool is made executable before it runs", async () => {
		const tool = "dist/tools/appimagetool-x86_64.AppImage";
		const fake = harness({
			exists: (path) =>
				path === "target/release/speechtt-ui" || path === "dist/worker.js" || path === tool
		});

		const code = await runCli(["--no-build"], fake.deps);

		expect(code).toBe(0);
		expect(fake.fetchCalls).toEqual([]);
		expect(fake.chmodCalls).toContainEqual({ path: tool, mode: 0o755 });
	});
});

describe("runCli environment errors", () => {
	test("missing cargo exits 2 with a readable message", async () => {
		const fake = harness({ has: (bin) => bin !== "cargo" });

		const code = await runCli([], fake.deps);

		expect(code).toBe(2);
		expect(lineMatches(fake.stderr, /cargo/)).toBe(true);
		expect(fake.execCalls).toEqual([]);
	});

	test("missing bun exits 2 for a build run", async () => {
		const fake = harness({ has: (bin) => bin !== "bun" });

		const code = await runCli([], fake.deps);

		expect(code).toBe(2);
		expect(lineMatches(fake.stderr, /bun/)).toBe(true);
		expect(fake.execCalls).toEqual([]);
	});

	test("a workspace version mismatch exits 2 naming both versions", async () => {
		const fake = harness({
			metadata: JSON.stringify({
				packages: [
					{ name: "speechtt-core", version: "0.5.0" },
					{ name: "speechtt-ui", version: "0.6.0" }
				]
			})
		});

		const code = await runCli([], fake.deps);

		expect(code).toBe(2);
		expect(lineMatches(fake.stderr, /0\.5\.0/)).toBe(true);
		expect(lineMatches(fake.stderr, /0\.6\.0/)).toBe(true);
	});

	test("an unsupported machine exits 2 with the value", async () => {
		const fake = harness({ machine: "riscv64" });

		const code = await runCli([], fake.deps);

		expect(code).toBe(2);
		expect(lineMatches(fake.stderr, /riscv64/)).toBe(true);
	});

	test("an unknown flag exits 2 with the usage", async () => {
		const fake = harness();

		const code = await runCli(["--nope"], fake.deps);

		expect(code).toBe(2);
		expect(lineMatches(fake.stderr, /--dry-run/)).toBe(true);
		expect(fake.execCalls).toEqual([]);
	});
});

describe("runCli full run", () => {
	const buildAddsArtifacts = (argv: readonly string[]) => {
		if (argv[0] === "bun" && argv[1] === "build") return true;
		if (argv[0] === "cargo" && argv[1] === "build") return true;
		return false;
	};

	test("bundles, builds, stages, packages and checksums in order", async () => {
		const fake = harness({
			onExec: (argv) => {
				if (buildAddsArtifacts(argv)) {
					if (argv[0] === "bun") fake.files.add("dist/worker.js");
					if (argv[0] === "cargo") fake.files.add("target/release/speechtt-ui");
				}
			}
		});

		const code = await runCli([], fake.deps);

		expect(code).toBe(0);
		expect(fake.execCalls.map((call) => [...call.argv])).toEqual([
			METADATA_ARGV,
			["bun", "build", "src/stt/worker.ts", "--target=bun", "--outfile", "dist/worker.js"],
			["cargo", "build", "--release", "--locked", "-p", "speechtt-ui"],
			[
				"dist/tools/appimagetool-x86_64.AppImage",
				"--appimage-extract-and-run",
				"--no-appstream",
				"dist/SpeechTT.AppDir",
				"dist/SpeechTT-0.5.0-x86_64.AppImage"
			]
		]);
		expect(fake.fetchCalls).toEqual([
			{
				url: "https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage",
				destination: "dist/tools/appimagetool-x86_64.AppImage"
			}
		]);
		expect(fake.copied).toHaveLength(5);
		expect(fake.written.filter((file) => file.path.endsWith(".sha256"))).toEqual([
			{
				path: "dist/SpeechTT-0.5.0-x86_64.AppImage.sha256",
				contents: `${DIGEST}  SpeechTT-0.5.0-x86_64.AppImage\n`
			}
		]);
	});

	test("a bundle with a bare ai import fails the run before cargo build", async () => {
		const fake = harness({
			bundle: 'import { generateText } from "ai";\n',
			onExec: (argv) => {
				if (argv[0] === "bun" && argv[1] === "build") fake.files.add("dist/worker.js");
			}
		});

		const code = await runCli([], fake.deps);

		expect(code).toBe(1);
		expect(fake.stderr.join("\n")).toContain(
			"still imports `ai` or `@ai-sdk/*`; the AppImage ships no node_modules"
		);
		expect(
			fake.execCalls.some((call) => call.argv[0] === "cargo" && call.argv[1] === "build")
		).toBe(false);
	});

	test("a non-empty bundle without a bare import passes the self-containment check", async () => {
		const fake = harness({
			bundle: "const ready = true;\n",
			onExec: (argv) => {
				if (argv[0] === "bun" && argv[1] === "build") fake.files.add("dist/worker.js");
				if (argv[0] === "cargo" && argv[1] === "build")
					fake.files.add("target/release/speechtt-ui");
			}
		});

		expect(await runCli([], fake.deps)).toBe(0);
	});
});

describe("runCli failures", () => {
	test("a failing appimagetool exits 1 and prints the tool's stderr", async () => {
		const fake = harness({
			exists: (path) =>
				path === "target/release/speechtt-ui" ||
				path === "dist/worker.js" ||
				path === "dist/tools/appimagetool-x86_64.AppImage",
			exitCode: (argv) => (argv[0]?.includes("appimagetool") ? 3 : 0),
			stderr: (argv) =>
				argv[0]?.includes("appimagetool") ? "appimagetool: cannot build" : ""
		});

		const code = await runCli(["--no-build"], fake.deps);

		expect(code).toBe(1);
		expect(lineMatches(fake.stderr, /appimagetool: cannot build/)).toBe(true);
		expect(fake.written.filter((file) => file.path.endsWith(".sha256"))).toEqual([]);
	});

	test("a failing cargo build exits 1 and stops before staging", async () => {
		const fake = harness({
			exitCode: (argv) => (argv[0] === "cargo" && argv[1] === "build" ? 101 : 0),
			stderr: (argv) =>
				argv[0] === "cargo" && argv[1] === "build"
					? "error: could not compile speechtt-ui"
					: "",
			onExec: (argv) => {
				if (argv[0] === "bun" && argv[1] === "build") fake.files.add("dist/worker.js");
			}
		});

		const code = await runCli([], fake.deps);

		expect(code).toBe(1);
		expect(lineMatches(fake.stderr, /could not compile/)).toBe(true);
		expect(fake.copied).toEqual([]);
		expect(fake.written).toEqual([]);
	});

	test("a failing bun build exits 1 and never reaches cargo build", async () => {
		const fake = harness({
			exitCode: (argv) => (argv[0] === "bun" && argv[1] === "build" ? 1 : 0),
			stderr: (argv) =>
				argv[0] === "bun" && argv[1] === "build" ? "error: bundling failed" : ""
		});

		const code = await runCli([], fake.deps);

		expect(code).toBe(1);
		expect(lineMatches(fake.stderr, /bundling failed/)).toBe(true);
		expect(
			fake.execCalls.some((call) => call.argv[0] === "cargo" && call.argv[1] === "build")
		).toBe(false);
		expect(fake.copied).toEqual([]);
	});

	test("a staging copy that throws exits 1, not 2", async () => {
		const fake = harness({
			exists: (path) =>
				path === "target/release/speechtt-ui" ||
				path === "dist/worker.js" ||
				path === "dist/tools/appimagetool-x86_64.AppImage",
			copyError: new Error("ENOENT: packaging/speechtt.png missing")
		});

		const code = await runCli(["--no-build"], fake.deps);

		expect(code).toBe(1);
		expect(fake.stderr.join("\n")).toContain("ENOENT: packaging/speechtt.png missing");
		expect(fake.stderr.join("\n")).not.toContain("usage: bun run package");
		expect(fake.written.filter((file) => file.path.endsWith(".sha256"))).toEqual([]);
	});

	test("a failed appimagetool download exits 1, not 2", async () => {
		const fake = harness({
			exists: (path) => path === "target/release/speechtt-ui" || path === "dist/worker.js",
			fetchError: new Error("download failed with 404: appimagetool")
		});

		const code = await runCli(["--no-build"], fake.deps);

		expect(code).toBe(1);
		expect(fake.stderr.join("\n")).toContain("download failed with 404: appimagetool");
		expect(fake.stderr.join("\n")).not.toContain("usage: bun run package");
		expect(fake.written.filter((file) => file.path.endsWith(".sha256"))).toEqual([]);
	});

	test("a checksum step that throws exits 1, not 2", async () => {
		const fake = harness({
			exists: (path) =>
				path === "target/release/speechtt-ui" ||
				path === "dist/worker.js" ||
				path === "dist/tools/appimagetool-x86_64.AppImage",
			sha256Error: new Error("cannot read the built artifact")
		});

		const code = await runCli(["--no-build"], fake.deps);

		expect(code).toBe(1);
		expect(fake.stderr.join("\n")).toContain("cannot read the built artifact");
	});

	test("an exec that throws while building exits 1, not 2", async () => {
		const fake = harness({
			execThrows: (argv) => argv[0] === "bun" && argv[1] === "build"
		});

		const code = await runCli([], fake.deps);

		expect(code).toBe(1);
		expect(fake.stderr.join("\n")).toContain("cannot run bun");
		expect(fake.copied).toEqual([]);
	});
});
