/**
 * Builds the Linux AppImage release artifact for SpeechTT.
 *
 * The pipeline advances the workspace patch version in `Cargo.toml`, bundles the STT worker into
 * one file, compiles the release widget, stages a `SpeechTT.AppDir` exactly as the AppImage guide
 * describes, turns it into `dist/SpeechTT-<version>-<arch>.AppImage`, writes its `sha256sum`
 * companion and prints the `gh release create` command. It never publishes anything itself.
 *
 * The bump comes first because the version is compiled into the binary: the artifact name and the
 * `V<version>` the widget shows must be the same number. A failure after the write restores the
 * manifest and the lock file, so a retry does not consume a second version.
 *
 * # Callers
 *
 * - `package.json`'s `package` script (`bun run package`), the documented entry point.
 * - `scripts/package.spec.ts`, which drives every pure function and the CLI through injected fakes.
 *
 * # Purity
 *
 * Importing this module runs nothing: the CLI is entered from the `import.meta.main` guard at the
 * bottom, and every machine-facing capability (`has`, `exec`, `fetch`, the filesystem sinks, the
 * output sinks) arrives as a {@link CliDeps} value. The version is read from `cargo metadata`,
 * never hardcoded, and the only files the release writes outside `dist/` are `Cargo.toml` and its
 * lock file.
 */
import { createHash } from "node:crypto";
import * as fs from "node:fs";
import { dirname, join } from "node:path";

/** The architectures this pipeline packages. */
export type Arch = "x86_64" | "aarch64";

/** A failure the CLI reports with exit code `2` rather than `1`. */
export class UsageError extends Error {}

/** One file of a staged AppDir. */
export type StagedFile =
	| {
			readonly path: string;
			readonly kind: "copy";
			readonly source: string;
			readonly mode: number;
	  }
	| {
			readonly path: string;
			readonly kind: "generated";
			readonly contents: string;
			readonly mode: number;
	  };

/** The complete Linux AppDir staging description. */
export interface StagePlan {
	readonly appDir: string;
	readonly files: readonly StagedFile[];
}

/** The result of one spawned command. */
export interface ExecResult {
	readonly exitCode: number;
	readonly stdout: string;
	readonly stderr: string;
}

/** Optional settings for one spawned command. */
export interface ExecOptions {
	readonly env?: Readonly<Record<string, string>>;
}

/** The injectable machine surface of the CLI. */
export interface CliDeps {
	/** The host's `uname -m` answer. */
	readonly machine: string;
	/** Whether a binary is on `PATH`. */
	readonly has: (bin: string) => boolean;
	/** Runs one argv list and captures its streams. */
	readonly exec: (argv: readonly string[], options?: ExecOptions) => ExecResult;
	/** Downloads `url` to `destination`. */
	readonly fetch: (url: string, destination: string) => Promise<void>;
	/** Whether a path exists. */
	readonly exists: (path: string) => boolean;
	/** Reads a text file. */
	readonly readFile: (path: string) => string;
	/** Writes a text file. */
	readonly writeFile: (path: string, contents: string) => void;
	/** Copies a file. */
	readonly copyFile: (from: string, to: string) => void;
	/** Creates a directory and its parents. */
	readonly makeDir: (path: string) => void;
	/** Removes a file or directory tree. */
	readonly removePath: (path: string) => void;
	/** Sets a file mode. */
	readonly chmod: (path: string, mode: number) => void;
	/** The lowercase hex SHA-256 digest of a file. */
	readonly sha256: (path: string) => string;
	readonly out: (line: string) => void;
	readonly err: (line: string) => void;
}

/** Where the staged AppDir lives. */
export const APP_DIR = "dist/SpeechTT.AppDir";

/** Where the worker bundle lives. */
export const WORKER_BUNDLE = "dist/worker.js";

/** The release binary the staging plan copies. */
export const RELEASE_BINARY = "target/release/speechtt-ui";

const APPIMAGETOOL_BASE = "https://github.com/AppImage/appimagetool/releases/download/continuous";

const USAGE = "usage: bun run package [--dry-run] [--no-build] [--no-bump] [--arch <arch>]";

/** The workspace manifest whose `[workspace.package]` version names the release. */
const MANIFEST_PATH = "Cargo.toml";

/** The lock file `cargo build --locked` checks against the manifest. */
const LOCKFILE_PATH = "Cargo.lock";

const METADATA_ARGV = ["cargo", "metadata", "--format-version", "1", "--no-deps"];

/**
 * The resolving metadata query run after the manifest write.
 *
 * `--no-deps` never touches `Cargo.lock`, and bumping a workspace member's version invalidates it,
 * so `cargo build --locked` would refuse to run. Resolving after the write rewrites just the member
 * entries and returns the version the build will see.
 *
 * The query reads the registry, because that is where `Cargo.lock` is rewritten from. A cold cache
 * is a normal state, not an error: on a warm cache cargo answers locally, and on a cold one it
 * downloads the index that `cargo build` was about to fetch anyway. The build's own `--locked` flag
 * keeps it from silently re-resolving, so this step does not weaken the lock guarantee.
 */
const RESOLVE_METADATA_ARGV = ["cargo", "metadata", "--format-version", "1"];

const CARGO_BUILD_ARGV = ["cargo", "build", "--release", "--locked", "-p", "speechtt-ui"];

/**
 * Maps `uname -m` — or, when that fails, Node's `process.arch` — to an AppImage architecture.
 *
 * Both spellings of each supported architecture are accepted here, in one place: the kernel says
 * `x86_64` / `aarch64`, Node says `x64` / `arm64`, and the release file name and the AppImage
 * runtime must agree on the kernel spelling. Anything else throws a {@link UsageError} naming the
 * unsupported value, because the pipeline cannot package it.
 */
export function archFrom(machine: string): Arch {
	switch (machine) {
		case "x86_64":
		case "x64":
			return "x86_64";
		case "aarch64":
		case "arm64":
			return "aarch64";
		default:
			throw new UsageError(`unsupported architecture: ${machine}`);
	}
}

/**
 * The release file name for `version` on `arch`.
 *
 * The AppImage guide forbids the platform in the name: `.AppImage` already means Linux, so the name
 * carries only the product, version and architecture.
 */
export function appImageFileName(version: string, arch: Arch): string {
	return `SpeechTT-${version}-${arch}.AppImage`;
}

/**
 * Reads the single workspace version from `cargo metadata` output.
 *
 * Every workspace package must agree: the release name carries one version, so a mismatch would
 * silently ship a mislabelled artifact. A mismatch and a missing version both throw a
 * {@link UsageError}; malformed JSON does too.
 */
export function versionFromMetadata(raw: string): string {
	let metadata: unknown;
	try {
		metadata = JSON.parse(raw);
	} catch {
		throw new UsageError("could not parse cargo metadata as JSON");
	}

	const packages =
		typeof metadata === "object" &&
		metadata !== null &&
		Array.isArray((metadata as { packages?: unknown }).packages)
			? (metadata as { packages: unknown[] }).packages
			: [];
	const versions = packages
		.map((entry) =>
			typeof entry === "object" &&
			entry !== null &&
			typeof (entry as { version?: unknown }).version === "string"
				? (entry as { version: string }).version
				: null
		)
		.filter((version): version is string => version !== null);

	const [first, ...rest] = versions;
	if (first === undefined) {
		throw new UsageError("cargo metadata reported no package version");
	}
	const disagreement = rest.find((version) => version !== first);
	if (disagreement !== undefined) {
		throw new UsageError(
			`workspace packages disagree on the version: ${first} and ${disagreement}`
		);
	}
	return first;
}

/**
 * The next patch version: `0.5.0` → `0.5.1`.
 *
 * A packaging change is a patch-level change, and the minor number is reserved for a feature line a
 * human decides on. Major and minor are preserved and only the patch advances. A value that is not
 * `major.minor.patch` with numeric components throws a {@link UsageError} rather than guessing.
 */
export function nextPatchVersion(version: string): string {
	const match = /^(\d+)\.(\d+)\.(\d+)$/.exec(version);
	if (match === null) {
		throw new UsageError(`not a major.minor.patch version: ${version}`);
	}
	const major = BigInt(match[1] as string);
	const minor = BigInt(match[2] as string);
	const patch = BigInt(match[3] as string);
	return `${major}.${minor}.${patch + 1n}`;
}

/**
 * `manifest` with the `[workspace.package]` version replaced by `version`.
 *
 * Only that section's `version` key changes: a dependency version or a profile setting in the same
 * file must be untouched. Returns `null` when the section or its key is absent, so the caller
 * reports the problem instead of writing a manifest it does not understand.
 */
export function bumpedManifest(manifest: string, version: string): string | null {
	let inWorkspacePackage = false;
	let sawSection = false;
	let replaced = false;

	const updated = manifest.split("\n").map((line) => {
		const header = /^\s*\[([^\]]+)\]\s*(?:#.*)?$/.exec(line);
		if (header !== null) {
			inWorkspacePackage = (header[1] as string).trim() === "workspace.package";
			sawSection ||= inWorkspacePackage;
			return line;
		}
		if (!inWorkspacePackage || replaced) {
			return line;
		}
		const key = /^(\s*version\s*=\s*)("[^"]*")(.*)$/.exec(line);
		if (key === null) {
			return line;
		}
		replaced = true;
		return `${key[1]}"${version}"${key[3]}`;
	});

	return sawSection && replaced ? updated.join("\n") : null;
}

/** The `AppRun` launcher, byte for byte. */
export function appRunScriptLinux(): string {
	return [
		"#!/bin/sh",
		"# Resolve the AppImage mount point, not the caller's working directory:",
		"# the widget locates its worker relative to its own executable.",
		"set -eu",
		'HERE="$(dirname "$(readlink -f "$0")")"',
		'exec "$HERE/usr/bin/speechtt-ui" "$@"',
		""
	].join("\n");
}

/** The desktop entry, byte for byte. */
export function desktopEntryLinux(): string {
	return [
		"[Desktop Entry]",
		"Type=Application",
		"Name=SpeechTT",
		"Comment=Floating speech-to-text widget",
		"Exec=speechtt-ui",
		"Icon=speechtt",
		"Terminal=false",
		"Categories=AudioVideo;Audio;Utility;",
		""
	].join("\n");
}

/**
 * The Linux AppDir staging plan of the AppImage guide.
 *
 * Paths are relative to {@link APP_DIR}. The launcher and the desktop entry are generated once and
 * written to both their root and `usr/share` locations; the icon is copied twice for the same
 * reason. `speechtt.png` at the AppDir root must match `Icon=speechtt`.
 */
export function stagePlanLinux(): StagePlan {
	const desktop = desktopEntryLinux();
	return {
		appDir: APP_DIR,
		files: [
			{ path: "AppRun", kind: "generated", contents: appRunScriptLinux(), mode: 0o755 },
			{ path: "speechtt.desktop", kind: "generated", contents: desktop, mode: 0o644 },
			{ path: "speechtt.png", kind: "copy", source: "packaging/speechtt.png", mode: 0o644 },
			{ path: "usr/bin/speechtt-ui", kind: "copy", source: RELEASE_BINARY, mode: 0o755 },
			{
				path: "usr/share/speechtt/worker.js",
				kind: "copy",
				source: WORKER_BUNDLE,
				mode: 0o644
			},
			{
				path: "usr/share/applications/speechtt.desktop",
				kind: "generated",
				contents: desktop,
				mode: 0o644
			},
			{
				path: "usr/share/icons/hicolor/256x256/apps/speechtt.png",
				kind: "copy",
				source: "packaging/speechtt.png",
				mode: 0o644
			},
			{
				path: "usr/share/licenses/speechtt/LICENSE",
				kind: "copy",
				source: "LICENSE",
				mode: 0o644
			}
		]
	};
}

/** The `bun build` argv, after the `bun` program itself. */
export function workerBundleArgs(): readonly string[] {
	return ["build", "src/stt/worker.ts", "--target=bun", "--outfile", WORKER_BUNDLE];
}

/** The `appimagetool` argv, after the tool program itself. */
export function appimagetoolArgs(appDir: string, out: string): readonly string[] {
	return ["--appimage-extract-and-run", "--no-appstream", appDir, out];
}

/** One `sha256sum`-compatible line: digest, two spaces, file name, newline. */
export function checksumLine(digest: string, fileName: string): string {
	return `${digest}  ${fileName}\n`;
}

/**
 * The release command the user runs after a successful build.
 *
 * The script prints this and never executes it: authentication, tagging and publishing stay
 * explicit human actions.
 */
export function releaseCommand(version: string, arch: Arch): string {
	const name = appImageFileName(version, arch);
	return `gh release create v${version} dist/${name} dist/${name}.sha256 --title "SpeechTT ${version}" --generate-notes`;
}

/** Command text with an optional environment prefix, for the terminal. */
function execLine(argv: readonly string[], env: Readonly<Record<string, string>> = {}): string {
	const prefix = Object.entries(env)
		.map(([key, value]) => `${key}=${value} `)
		.join("");
	return `${prefix}${argv.join(" ")}`;
}

/** Prints and runs the workspace `cargo metadata` query, returning its version. */
function readWorkspaceVersion(deps: CliDeps): string {
	deps.out(execLine(METADATA_ARGV));
	const metadata = deps.exec(METADATA_ARGV);
	if (metadata.exitCode !== 0) {
		throw new UsageError(`cargo metadata failed: ${metadata.stderr.trim()}`);
	}
	return versionFromMetadata(metadata.stdout);
}

/**
 * Resolves the workspace so `Cargo.lock` matches the manifest that was just written.
 *
 * Bumping a workspace member's version invalidates the lock file, and `cargo build --locked`
 * refuses to run against it. The resolving (dependency-including) query rewrites the member
 * entries, so it reads the registry: a cold cache is a normal state, and the index download it
 * costs is the one `cargo build` would have paid anyway. `--locked` on the build is what keeps the
 * build from silently re-resolving. Its package list is not read — the caller re-reads the
 * workspace version with the `--no-deps` query, which reports the members alone.
 */
function refreshLockfile(deps: CliDeps): void {
	deps.out(execLine(RESOLVE_METADATA_ARGV));
	const resolved = deps.exec(RESOLVE_METADATA_ARGV);
	if (resolved.exitCode !== 0) {
		throw new UsageError(`cargo metadata failed: ${resolved.stderr.trim()}`);
	}
}

/** Runs one step, printing it and its stderr, and reports success. */
function runStep(deps: CliDeps, argv: readonly string[], options?: ExecOptions): boolean {
	deps.out(execLine(argv, options?.env));
	const result = deps.exec(argv, options);
	if (result.stdout.trim() !== "") {
		deps.out(result.stdout.trimEnd());
	}
	if (result.exitCode !== 0) {
		deps.err(
			`package: command failed with exit code ${result.exitCode}: ${execLine(argv, options?.env)}`
		);
		if (result.stderr.trim() !== "") {
			deps.err(result.stderr.trimEnd());
		}
		return false;
	}
	return true;
}

/** Removes and recreates the AppDir from `plan`. */
function materialize(deps: CliDeps, plan: StagePlan): void {
	deps.removePath(plan.appDir);
	deps.makeDir(plan.appDir);
	for (const file of plan.files) {
		const destination = join(plan.appDir, file.path);
		deps.makeDir(dirname(destination));
		if (file.kind === "generated") {
			deps.writeFile(destination, file.contents);
		} else {
			deps.copyFile(file.source, destination);
		}
		deps.chmod(destination, file.mode);
	}
}

/** A worker bundle that still imports a package the AppImage does not ship. */
function hasBarePackageImport(bundle: string): boolean {
	return bundle.includes('from "ai"') || bundle.includes('from "@ai-sdk/');
}

/**
 * Checks that `path` holds a worker bundle the AppImage can ship.
 *
 * The AppImage carries no `node_modules` tree, so a bundle that is empty (the build wrote nothing)
 * or that still imports `ai` / `@ai-sdk/*` (the bundler externalised them) would ship a worker that
 * cannot start. Returns `false` after printing the reason.
 */
function checkWorkerBundle(deps: CliDeps, path: string): boolean {
	const bundle = deps.readFile(path);
	if (bundle.length === 0) {
		deps.err(`package: ${path} is empty`);
		return false;
	}
	if (hasBarePackageImport(bundle)) {
		deps.err(
			`package: ${path} still imports \`ai\` or \`@ai-sdk/*\`; the AppImage ships no node_modules`
		);
		return false;
	}
	return true;
}

interface ParsedArguments {
	readonly dryRun: boolean;
	readonly noBuild: boolean;
	readonly noBump: boolean;
	readonly arch: Arch | null;
}

/** Parses the CLI flags and rejects unknown or incomplete arguments. */
function parseArguments(argv: readonly string[]): ParsedArguments {
	let dryRun = false;
	let noBuild = false;
	let noBump = false;
	let arch: Arch | null = null;

	for (let index = 0; index < argv.length; index += 1) {
		const argument = argv[index];
		switch (argument) {
			case "--dry-run":
				dryRun = true;
				break;
			case "--no-build":
				noBuild = true;
				break;
			case "--no-bump":
				noBump = true;
				break;
			case "--arch": {
				const value = argv[index + 1];
				if (value === undefined) {
					throw new UsageError("--arch needs an architecture");
				}
				arch = archFrom(value);
				index += 1;
				break;
			}
			default:
				throw new UsageError(`unknown argument ${argument}`);
		}
	}

	return { dryRun, noBuild, noBump, arch };
}

/**
 * The `package` command itself.
 *
 * Exit codes are the contract: `0` a complete artifact (or a complete `--dry-run`), `1` a build or
 * packaging step failed — a failed `cargo build` or `bun build`, a staging copy, the `appimagetool`
 * download or run, the checksum — and `2` a usage or environment error such as a missing tool, a
 * workspace version mismatch or an unsupported architecture.
 *
 * A full build advances the `[workspace.package]` patch version in `Cargo.toml` before `cargo
 * build`, re-reads `cargo metadata`, and names the artifact from that answer. A failure after the
 * write restores the original manifest, so a retry does not consume a second version.
 *
 * `--dry-run` prints every command, the release version it would use and every staged path, and
 * runs only the read-only `cargo metadata` query. `--no-build` reuses `target/release/speechtt-ui`
 * and `dist/worker.js`, checks that bundle for bare imports, and fails with exit `1` when either
 * artifact is absent. `--no-bump` packages the current version without writing the manifest. The
 * script never runs `gh`.
 */
export async function runCli(argv: readonly string[], deps: CliDeps): Promise<number> {
	try {
		return await runPipeline(argv, deps);
	} catch (error) {
		if (error instanceof UsageError) {
			deps.err(`package: ${error.message}`);
			deps.err(USAGE);
			return 2;
		}
		deps.err(`package: ${error instanceof Error ? error.message : String(error)}`);
		// A filesystem, download or packaging call threw (a missing icon, an unwritable `dist/`, a
		// rejected download). The artifact is incomplete, so the run failed the way a failed build
		// does: exit `1`, not a usage or environment error.
		return 1;
	}
}

/** The pipeline behind {@link runCli}; only the exit-code mapping lives outside it. */
async function runPipeline(argv: readonly string[], deps: CliDeps): Promise<number> {
	const parsed = parseArguments(argv);
	const arch = parsed.arch ?? archFrom(deps.machine);

	const required = parsed.noBuild ? ["cargo"] : ["cargo", "bun"];
	for (const tool of required) {
		if (!deps.has(tool)) {
			throw new UsageError(`missing required tool: ${tool}`);
		}
	}

	const currentVersion = readWorkspaceVersion(deps);
	// A dry run writes nothing, but it names the version a release would produce. Repackaging and
	// `--no-bump` keep the version already compiled into the artifacts they reuse.
	const releaseVersion =
		parsed.noBump || parsed.noBuild ? currentVersion : nextPatchVersion(currentVersion);
	const plan = stagePlanLinux();

	if (parsed.dryRun) {
		const out = `dist/${appImageFileName(releaseVersion, arch)}`;
		deps.out(`release version: ${releaseVersion}`);
		deps.out(execLine(["bun", ...workerBundleArgs()]));
		deps.out(execLine(CARGO_BUILD_ARGV));
		for (const file of plan.files) {
			deps.out(`  ${file.mode.toString(8).padStart(3, "0")} ${join(plan.appDir, file.path)}`);
		}
		const tool = `${APPIMAGETOOL_BASE}/appimagetool-${arch}.AppImage`;
		deps.out(
			execLine(
				[`dist/tools/appimagetool-${arch}.AppImage`, ...appimagetoolArgs(plan.appDir, out)],
				{
					ARCH: arch
				}
			)
		);
		deps.out(`download: ${tool}`);
		deps.out(releaseCommand(releaseVersion, arch));
		return 0;
	}

	if (parsed.noBuild) {
		if (!parsed.noBump) {
			deps.out(
				"package: --no-build reuses the compiled binary, so the bump is skipped (nothing was recompiled)"
			);
		}
		for (const artifact of [RELEASE_BINARY, WORKER_BUNDLE]) {
			if (!deps.exists(artifact)) {
				deps.err(`package: --no-build needs ${artifact}; build it first`);
				return 1;
			}
		}
		if (!checkWorkerBundle(deps, WORKER_BUNDLE)) {
			return 1;
		}
	}

	let version = releaseVersion;
	let bumped = false;
	let originalManifest: string | null = null;
	let originalLockfile: string | null = null;
	let succeeded = false;

	try {
		if (!parsed.noBuild) {
			deps.makeDir("dist");
			if (!parsed.noBump) {
				originalManifest = deps.readFile(MANIFEST_PATH);
				originalLockfile = deps.exists(LOCKFILE_PATH) ? deps.readFile(LOCKFILE_PATH) : null;
				const bumpedManifestText = bumpedManifest(originalManifest, releaseVersion);
				if (bumpedManifestText === null) {
					throw new UsageError(
						`${MANIFEST_PATH} has no [workspace.package] version to bump`
					);
				}
				deps.writeFile(MANIFEST_PATH, bumpedManifestText);
				bumped = true;
				// The lock file follows the manifest, so the `--locked` build accepts it, and the
				// version is re-read after the write: the artifact name and the compiled-in version
				// both come from the manifest `cargo build` is about to see.
				refreshLockfile(deps);
				version = readWorkspaceVersion(deps);
				deps.out(`bumped: ${currentVersion} -> ${version}`);
			}

			if (!runStep(deps, ["bun", ...workerBundleArgs()])) {
				return 1;
			}
			if (!deps.exists(WORKER_BUNDLE)) {
				deps.err(`package: ${WORKER_BUNDLE} was not produced by bun build`);
				return 1;
			}
			if (!checkWorkerBundle(deps, WORKER_BUNDLE)) {
				return 1;
			}
			if (!runStep(deps, CARGO_BUILD_ARGV)) {
				return 1;
			}
			if (!deps.exists(RELEASE_BINARY)) {
				deps.err(`package: ${RELEASE_BINARY} was not produced by cargo build`);
				return 1;
			}
		}

		materialize(deps, plan);

		const name = appImageFileName(version, arch);
		const out = `dist/${name}`;
		const toolPath = `dist/tools/appimagetool-${arch}.AppImage`;
		if (!deps.exists(toolPath)) {
			const url = `${APPIMAGETOOL_BASE}/appimagetool-${arch}.AppImage`;
			deps.out(`download: ${url}`);
			await deps.fetch(url, toolPath);
		}
		// Unconditional: a cached tool left non-executable by an interrupted download must still run.
		deps.chmod(toolPath, 0o755);
		if (
			!runStep(deps, [toolPath, ...appimagetoolArgs(plan.appDir, out)], {
				env: { ARCH: arch }
			})
		) {
			return 1;
		}

		const checksum = checksumLine(deps.sha256(out), name);
		deps.writeFile(`${out}.sha256`, checksum);
		deps.out(`wrote: ${out}.sha256`);
		deps.out(releaseCommand(version, arch));
		succeeded = true;
		return 0;
	} finally {
		// Any failure after the write — a step returning non-zero or a call that throws — leaves the
		// tree as it was found, so retrying the build does not advance the version twice. The lock
		// file goes back with the manifest: a bumped lock against a restored manifest would break
		// the next `--locked` build.
		if (bumped && !succeeded) {
			if (originalManifest !== null) {
				deps.writeFile(MANIFEST_PATH, originalManifest);
			}
			if (originalLockfile !== null) {
				deps.writeFile(LOCKFILE_PATH, originalLockfile);
			}
		}
	}
}

/** The live machine surface behind the CLI. */
function systemDeps(): CliDeps {
	return {
		machine: (() => {
			try {
				const result = Bun.spawnSync(["uname", "-m"], { stdout: "pipe", stderr: "pipe" });
				return result.exitCode === 0 ? result.stdout.toString().trim() : process.arch;
			} catch {
				return process.arch;
			}
		})(),
		has: (bin) => Bun.which(bin) !== null,
		exec: (argv, options) => {
			try {
				const result = Bun.spawnSync([...argv], {
					stdout: "pipe",
					stderr: "pipe",
					env: { ...process.env, ...options?.env }
				});
				return {
					exitCode: result.exitCode ?? 1,
					stdout: result.stdout.toString(),
					stderr: result.stderr.toString()
				};
			} catch (error) {
				return {
					exitCode: 1,
					stdout: "",
					stderr: error instanceof Error ? error.message : String(error)
				};
			}
		},
		fetch: async (url, destination) => {
			const response = await fetch(url);
			if (!response.ok) {
				throw new Error(`download failed with ${response.status}: ${url}`);
			}
			await Bun.write(destination, response);
		},
		exists: (path) => fs.existsSync(path),
		readFile: (path) => fs.readFileSync(path, "utf8"),
		writeFile: (path, contents) => {
			fs.writeFileSync(path, contents);
		},
		copyFile: (from, to) => {
			fs.copyFileSync(from, to);
		},
		makeDir: (path) => {
			fs.mkdirSync(path, { recursive: true });
		},
		removePath: (path) => {
			fs.rmSync(path, { recursive: true, force: true });
		},
		chmod: (path, mode) => {
			fs.chmodSync(path, mode);
		},
		sha256: (path) => createHash("sha256").update(fs.readFileSync(path)).digest("hex"),
		out: (line) => {
			console.log(line);
		},
		err: (line) => {
			console.error(line);
		}
	};
}

if (import.meta.main) {
	process.exit(await runCli(process.argv.slice(2), systemDeps()));
}
