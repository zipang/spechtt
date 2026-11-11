/**
 * Builds the Linux AppImage release artifact for SpeechTT, and publishes it under `--release`.
 *
 * The pipeline advances the workspace patch version in `Cargo.toml` and `package.json`, bundles the
 * STT worker into one file, compiles the release widget, stages a `SpeechTT.AppDir` exactly as the
 * AppImage guide describes, turns it into `dist/SpeechTT-<version>-<arch>.AppImage`, writes its
 * `sha256sum` companion and prints the `gh release create` command. Only `--release` executes that
 * command; a plain `bun run package` builds and prints.
 *
 * A `--release` run commits the version bump and pushes it before publishing, so the tag
 * `gh release create` creates points at a commit whose `Cargo.toml` names the released version.
 *
 * The bump comes first because the version is compiled into the binary: the artifact name and the
 * `V<version>` the widget shows must be the same number. A failure after the write restores the
 * manifest, the lock file and `package.json`, so a retry does not consume a second version.
 *
 * Once the artifact and its checksum are written, the run removes the other `SpeechTT-*.AppImage`
 * files and their `.sha256` companions from `dist/`, so the directory names only the release just
 * produced. The cleanup runs last so a failed build keeps the previous release; the tool cache
 * (`tools/`), the worker bundle (`worker.js`) and the staged `SpeechTT.AppDir` stay in place.
 *
 * # Callers
 *
 * - `package.json`'s `package` script (`bun run package`) and its `release` script (`bun run
 *   release`, which passes `--release`), the documented entry points.
 * - `scripts/package.spec.ts`, which drives every pure function and the CLI through injected fakes.
 *
 * # Purity
 *
 * Importing this module runs nothing: the CLI is entered from the `import.meta.main` guard at the
 * bottom, and every machine-facing capability (`has`, `exec`, `fetch`, the filesystem sinks, the
 * output sinks) arrives as a {@link CliDeps} value. The version is read from `cargo metadata`,
 * never hardcoded, and the only files the run writes outside `dist/` are `Cargo.toml`, its lock
 * file and `package.json`. The `git` invocations are the read-only `git status --porcelain` guard,
 * and — under `--release` on a run that bumped — the `git add`, `git commit` and `git push` that
 * carry the bump to the remote before the tag is made.
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
	/** Lists the bare entry names of a directory; an empty list when it does not exist. */
	readonly listDir: (path: string) => string[];
	/** Sets a file mode. */
	readonly chmod: (path: string, mode: number) => void;
	/** The lowercase hex SHA-256 digest of a file. */
	readonly sha256: (path: string) => string;
	readonly out: (line: string) => void;
	readonly err: (line: string) => void;
}

/** Where the staged AppDir lives. */
export const APP_DIR = "dist/SpeechTT.AppDir";

/** The output directory a release lands in, holding every file {@link staleArtifacts} inspects. */
export const DIST_DIR = "dist";

/** Where the worker bundle lives. */
export const WORKER_BUNDLE = "dist/worker.js";

/** The release binary the staging plan copies. */
export const RELEASE_BINARY = "target/release/speechtt-ui";

const APPIMAGETOOL_BASE = "https://github.com/AppImage/appimagetool/releases/download/continuous";

const USAGE =
	"usage: bun run package [--dry-run] [--no-build] [--no-bump] [--release] [--allow-dirty] [--arch <arch>]";

/** The workspace manifest whose `[workspace.package]` version names the release. */
const MANIFEST_PATH = "Cargo.toml";

/** The lock file `cargo build --locked` checks against the manifest. */
const LOCKFILE_PATH = "Cargo.lock";

/** The npm manifest whose `version` follows the release version. */
const PACKAGE_JSON_PATH = "package.json";

/** The working-tree status listing the release guard reads. */
const GIT_STATUS_ARGV = ["git", "status", "--porcelain"];

/**
 * The files a release commits before publishing.
 *
 * The bump writes exactly these three, so they are staged by path and never with `git add -A`: a
 * release must not sweep an unrelated change into its commit. A workspace without a lock file
 * stages two.
 */
export const VERSION_FILES: readonly string[] = [MANIFEST_PATH, LOCKFILE_PATH, PACKAGE_JSON_PATH];

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

/**
 * `text` as a `package.json` whose `version` is `version`, formatting and key order preserved.
 *
 * The file is re-serialised with tab indentation, which is how the repository formats it, so the
 * bump produces the same diff as a hand edit of that one line. Returns `null` when the text is not
 * a JSON object, so the caller reports the problem instead of writing a file it does not
 * understand. Written by the packaging run after `Cargo.toml`, and restored with it on failure.
 */
export function bumpedPackageJson(text: string, version: string): string | null {
	let parsed: unknown;
	try {
		parsed = JSON.parse(text);
	} catch {
		return null;
	}
	if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed)) {
		return null;
	}
	// Spreading keeps the existing key order; assigning `version` updates it in place in the object
	// literal without moving it to the end.
	const updated = { ...(parsed as Record<string, unknown>), version };
	return `${JSON.stringify(updated, null, "\t")}\n`;
}

/**
 * The non-blank entries of a `git status --porcelain` listing.
 *
 * Clean means empty: a release tags the commit the build came from, so any uncommitted change would
 * make the tag describe code the artifact does not contain. Trailing whitespace is trimmed and
 * blank lines dropped, so the caller can report the paths as the listing wrote them.
 * `--allow-dirty` bypasses the check for a deliberate one-off.
 */
export function dirtyEntries(statusOutput: string): string[] {
	return statusOutput
		.split("\n")
		.map((line) => line.trimEnd())
		.filter((line) => line.trim() !== "");
}

/**
 * Artifacts in `entries` that belong to another version than `keep`.
 *
 * Only the two release shapes are candidates: `SpeechTT-*.AppImage` and
 * `SpeechTT-*.AppImage.sha256`. Everything else is left alone, so the cached `appimagetool`
 * download (`tools/`), the worker bundle (`worker.js`) and the staged `SpeechTT.AppDir` survive a
 * run.
 *
 * `entries` are bare names from {@link CliDeps.listDir}, never paths: an entry that contains a `/`
 * or a `\` is never returned, so a directory listing cannot escape `dist/`. `keep` is the artifact
 * just built, and its `.sha256` companion is kept with it. The result is sorted, so a run removes
 * files in a stable order. Called by the packaging pipeline once the checksum is written.
 */
export function staleArtifacts(entries: readonly string[], keep: string): string[] {
	const kept = new Set([keep, `${keep}.sha256`]);
	return entries
		.filter(
			(entry) =>
				!entry.includes("/") &&
				!entry.includes("\\") &&
				!kept.has(entry) &&
				(/^SpeechTT-.+\.AppImage$/.test(entry) ||
					/^SpeechTT-.+\.AppImage\.sha256$/.test(entry))
		)
		.sort();
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
 * The `git add` command staging the version files a bump wrote, as an argv list.
 *
 * Explicit paths, never `git add -A`: a release commits its own three files and nothing else, so an
 * unrelated change in the working tree stays where the author left it.
 */
export function stageVersionFilesArgs(files: readonly string[]): readonly string[] {
	return ["git", "add", ...files];
}

/**
 * The commit that carries a release bump, as an argv list.
 *
 * The subject matches the repository convention, `<emoji> <type>: <summary>`, and names the version
 * rather than a ticket: the release is what the commit is about.
 */
export function releaseCommitArgs(version: string): readonly string[] {
	return ["git", "commit", "-m", `🔖 release: ${version}`];
}

/** The push that puts the release commit on the remote before the tag is created. */
export const GIT_PUSH_ARGV: readonly string[] = ["git", "push"];

/**
 * The `gh release create` command that publishes `version`, as an argv list.
 *
 * The single source of the publish command: {@link releaseCommand} prints it and a `--release` run
 * executes it, so what the user reads and what runs cannot drift apart.
 */
export function releaseArgs(version: string, arch: Arch): readonly string[] {
	const name = appImageFileName(version, arch);
	return [
		"gh",
		"release",
		"create",
		`v${version}`,
		`${DIST_DIR}/${name}`,
		`${DIST_DIR}/${name}.sha256`,
		"--title",
		`SpeechTT ${version}`,
		"--generate-notes"
	];
}

/**
 * The release command as the terminal shows it.
 *
 * Printed by every run. `bun run package` stops there, so a build never publishes; `--release` goes
 * on to execute the same argv through {@link releaseArgs}. `gh release create v<version>` creates
 * the tag as part of publishing, so the script never tags anything itself.
 */
export function releaseCommand(version: string, arch: Arch): string {
	return argvLine(releaseArgs(version, arch));
}

/**
 * One argument as a shell would need it typed.
 *
 * An argument carrying a space is quoted, so a printed command stays copy-pasteable: the release
 * commit's subject is `🔖 release: 0.5.2`, and printing it bare would split into two arguments.
 */
function shellArg(argument: string): string {
	return argument.includes(" ") ? `"${argument}"` : argument;
}

/** An argv list as terminal text. */
function argvLine(argv: readonly string[]): string {
	return argv.map(shellArg).join(" ");
}

/** Command text with an optional environment prefix, for the terminal. */
function execLine(argv: readonly string[], env: Readonly<Record<string, string>> = {}): string {
	const prefix = Object.entries(env)
		.map(([key, value]) => `${key}=${value} `)
		.join("");
	return `${prefix}${argvLine(argv)}`;
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

/**
 * Leaves `dist/` carrying only the release `keep` just produced.
 *
 * A successful run writes the artifact and its checksum first and cleans afterwards, so a build
 * that failed produced nothing to keep and the previous release survives. Every removal is named on
 * its own line. Under `--dry-run` the same names are printed and nothing is removed.
 */
function discardStaleArtifacts(deps: CliDeps, keep: string, dryRun: boolean): void {
	for (const entry of staleArtifacts(deps.listDir(DIST_DIR), keep)) {
		const path = `${DIST_DIR}/${entry}`;
		deps.out(dryRun ? `would remove: ${path}` : `removed: ${path}`);
		if (!dryRun) {
			deps.removePath(path);
		}
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
	readonly release: boolean;
	readonly allowDirty: boolean;
	readonly arch: Arch | null;
}

/** Parses the CLI flags and rejects unknown or incomplete arguments. */
function parseArguments(argv: readonly string[]): ParsedArguments {
	let dryRun = false;
	let noBuild = false;
	let noBump = false;
	let release = false;
	let allowDirty = false;
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
			case "--release":
				release = true;
				break;
			case "--allow-dirty":
				allowDirty = true;
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

	return { dryRun, noBuild, noBump, release, allowDirty, arch };
}

/**
 * The uncommitted paths that would block a release.
 *
 * A release tags the commit the build came from, so an uncommitted change would make the tag
 * describe code the artifact does not contain. This is the only `git` invocation the script makes,
 * and it only reads. Returns the entries rather than throwing, so a `--dry-run` can report what a
 * real release would refuse instead of aborting the projection. A failed `git status` itself is a
 * {@link UsageError}, because the guard cannot answer at all.
 */
function dirtyTree(deps: CliDeps): string[] {
	const status = deps.exec(GIT_STATUS_ARGV);
	if (status.exitCode !== 0) {
		throw new UsageError(`git status failed: ${status.stderr.trim()}`);
	}
	return dirtyEntries(status.stdout);
}

/**
 * Commits the version bump and pushes it, so the tag `gh release create` makes points at a commit
 * that carries the released number.
 *
 * A release tags the commit on the remote's default branch, so a bump left uncommitted would tag a
 * tree that still names the previous version: the artifact reports `0.5.2` while `v0.5.2` points at
 * a `Cargo.toml` saying `0.5.1`. Committing and pushing first removes that gap.
 *
 * Only the files the bump wrote are staged, by path, so an unrelated change is never swept in.
 * Returns whether all three steps succeeded; a failure leaves the bump on disk and uncommitted,
 * which the caller reports rather than rolls back — the artifact is already built at that number.
 */
function commitAndPushVersion(deps: CliDeps, version: string): boolean {
	const staged = stageVersionFilesArgs(VERSION_FILES);
	deps.out(execLine(staged));
	if (!runStep(deps, staged)) {
		return false;
	}

	const commit = releaseCommitArgs(version);
	deps.out(execLine(commit));
	if (!runStep(deps, commit)) {
		return false;
	}

	deps.out(execLine(GIT_PUSH_ARGV));
	if (!runStep(deps, GIT_PUSH_ARGV)) {
		return false;
	}

	deps.out(`committed and pushed: v${version}`);
	return true;
}

/**
 * Publishes the artifact `version` just produced, through the command every run prints.
 *
 * Only `--release` reaches this. A failure keeps the artifact and its checksum on disk: the build
 * succeeded, and deleting them would punish the user for a network or authentication problem.
 * Returns whether `gh` succeeded.
 */
function publishRelease(deps: CliDeps, version: string, arch: Arch): boolean {
	deps.out(releaseCommand(version, arch));
	const result = deps.exec(releaseArgs(version, arch));
	if (result.stdout.trim() !== "") {
		deps.out(result.stdout.trimEnd());
	}
	if (result.exitCode !== 0) {
		deps.err(`package: gh release create failed with exit code ${result.exitCode}`);
		if (result.stderr.trim() !== "") {
			deps.err(result.stderr.trimEnd());
		}
		return false;
	}
	deps.out(`published: v${version}`);
	return true;
}

/**
 * The `package` command itself.
 *
 * Exit codes are the contract: `0` a complete artifact (or a complete `--dry-run`), `1` a build or
 * packaging step failed — a failed `cargo build` or `bun build`, a staging copy, the `appimagetool`
 * download or run, the checksum, a failed `gh release create` — and `2` a usage or environment
 * error such as a missing tool, a dirty working tree, a workspace version mismatch or an
 * unsupported architecture.
 *
 * A full build advances the `[workspace.package]` patch version in `Cargo.toml` and the `version`
 * in `package.json` before `cargo build`, re-reads `cargo metadata`, and names the artifact from
 * that answer. A failure after the write restores the original files, so a retry does not consume a
 * second version.
 *
 * `--dry-run` prints every command, the release version it would use, every staged path and the
 * files a run would discard from `dist/`, and runs only the read-only `cargo metadata` query, the
 * `dist/` listing and — under `--release` — the `git status --porcelain` guard, whose blocking
 * paths it reports instead of aborting. `--no-build` reuses `target/release/speechtt-ui` and
 * `dist/worker.js`, checks that bundle for bare imports, and fails with exit `1` when either
 * artifact is absent. `--no-bump` packages the current version without writing the manifest.
 * `--release` on a run that bumped commits the version files and pushes before it executes `gh`; a
 * plain run prints the commands and stops.
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
	// `gh` and `git` are checked with the other tools so a release never fails at the last step for
	// a missing tool, after the whole build has already been paid for.
	if (parsed.release) {
		required.push("gh", "git");
	}
	for (const tool of required) {
		if (!deps.has(tool)) {
			throw new UsageError(`missing required tool: ${tool}`);
		}
	}

	if (parsed.release && parsed.allowDirty) {
		deps.out("package: --allow-dirty given, so the clean-tree check is skipped");
	}
	// Publishing tags the commit the artifact was built from, so the tree must carry no
	// uncommitted change. A dry run publishes nothing, but it exists to predict the real run, so
	// it reports what would block the release and carries on with the plan.
	if (parsed.release && !parsed.allowDirty) {
		const blocked = dirtyTree(deps);
		if (blocked.length > 0) {
			const listing = blocked.map((entry) => `  ${entry}`);
			if (parsed.dryRun) {
				deps.out(
					`package: a real release would refuse: ${blocked.length} uncommitted path(s)`
				);
				for (const line of listing) {
					deps.out(line);
				}
			} else {
				throw new UsageError(
					[
						"refusing to publish from a dirty working tree; commit or stash these, or pass --allow-dirty:",
						...listing
					].join("\n")
				);
			}
		}
	}

	const currentVersion = readWorkspaceVersion(deps);
	// A dry run writes nothing, but it names the version a release would produce. Repackaging and
	// `--no-bump` keep the version already compiled into the artifacts they reuse.
	const releaseVersion =
		parsed.noBump || parsed.noBuild ? currentVersion : nextPatchVersion(currentVersion);
	const plan = stagePlanLinux();

	if (parsed.dryRun) {
		const name = appImageFileName(releaseVersion, arch);
		const out = `dist/${name}`;
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
		discardStaleArtifacts(deps, name, true);
		if (parsed.release && !parsed.noBump && !parsed.noBuild) {
			deps.out(execLine(stageVersionFilesArgs(VERSION_FILES)));
			deps.out(execLine(releaseCommitArgs(releaseVersion)));
			deps.out(execLine(GIT_PUSH_ARGV));
		}
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
	let originalPackageJson: string | null = null;
	let succeeded = false;

	try {
		if (!parsed.noBuild) {
			deps.makeDir("dist");
			if (!parsed.noBump) {
				originalManifest = deps.readFile(MANIFEST_PATH);
				originalLockfile = deps.exists(LOCKFILE_PATH) ? deps.readFile(LOCKFILE_PATH) : null;
				originalPackageJson = deps.exists(PACKAGE_JSON_PATH)
					? deps.readFile(PACKAGE_JSON_PATH)
					: null;
				const bumpedManifestText = bumpedManifest(originalManifest, releaseVersion);
				if (bumpedManifestText === null) {
					throw new UsageError(
						`${MANIFEST_PATH} has no [workspace.package] version to bump`
					);
				}
				deps.writeFile(MANIFEST_PATH, bumpedManifestText);
				bumped = true;
				// `package.json` is not a workspace member, so it does not affect `cargo metadata`;
				// it follows the same number so the repository carries one version.
				if (originalPackageJson !== null) {
					const bumpedPackageJsonText = bumpedPackageJson(
						originalPackageJson,
						releaseVersion
					);
					if (bumpedPackageJsonText === null) {
						throw new UsageError(
							`${PACKAGE_JSON_PATH} is not a JSON object, so its version cannot be bumped`
						);
					}
					deps.writeFile(PACKAGE_JSON_PATH, bumpedPackageJsonText);
				}
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
		// The release exists on disk now, so anything older in `dist/` is superseded. Cleaning after
		// the checksum keeps a failed build from destroying the previous release.
		discardStaleArtifacts(deps, name, false);
		// The artifact is complete and the tree carries its version, so a publish failure below
		// must not roll the bump back: the retry is a republish at this number, and rolling back
		// would leave `--release --no-build` naming a version no artifact carries.
		succeeded = true;
		if (parsed.release) {
			// The commit must come before the publish: `gh release create` tags the remote's default
			// branch, so a bump still sitting in the working tree would leave the tag on a commit
			// that names the previous version. `--no-bump` and `--no-build` write no version, so
			// there is nothing to commit and the existing artifact is republished as it is.
			if (bumped && !commitAndPushVersion(deps, version)) {
				deps.err(
					"package: the version bump is on disk but not committed; commit and push it, then publish"
				);
				return 1;
			}
			return publishRelease(deps, version, arch) ? 0 : 1;
		}
		deps.out(releaseCommand(version, arch));
		return 0;
	} finally {
		// Any failure after the write — a step returning non-zero or a call that throws — leaves the
		// tree as it was found, so retrying the build does not advance the version twice. The lock
		// file goes back with the manifest: a bumped lock against a restored manifest would break
		// the next `--locked` build. `package.json` follows both.
		if (bumped && !succeeded) {
			if (originalManifest !== null) {
				deps.writeFile(MANIFEST_PATH, originalManifest);
			}
			if (originalLockfile !== null) {
				deps.writeFile(LOCKFILE_PATH, originalLockfile);
			}
			if (originalPackageJson !== null) {
				deps.writeFile(PACKAGE_JSON_PATH, originalPackageJson);
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
		listDir: (path) => {
			try {
				return fs.readdirSync(path);
			} catch {
				// A missing `dist/` on a first run lists as empty, so the cleanup step has nothing
				// to do instead of failing the release it just built.
				return [];
			}
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
