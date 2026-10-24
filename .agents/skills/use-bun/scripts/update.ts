/**
 * Regenerates the `use-bun` skill reference indexes from the official Bun
 * documentation and keeps two derived files in sync:
 *
 * 1. `references/*.md` — the per-domain documentation sub-indexes.
 * 2. The target project's `AGENTS.md` — a small always-visible "Bun tooling"
 *    block (marker-managed), so every session knows to prefer Bun built-ins
 *    even before the skill is loaded.
 *
 * The script mirrors two sources and merges them, because neither is complete:
 * - `https://bun.com/docs/llms.txt` lists most pages with a title and summary,
 *   but omits a dozen pages (type checker, linter, formatter, module graph, ...).
 * - `https://bun.com/sitemap.xml` lists every published page, including the
 *   ones missing from `llms.txt`, but carries no title or summary.
 *
 * Every doc page is fetchable as Markdown by suffixing `.md`, which is the URL
 * shape the skill hands to the agent for on-demand reading.
 *
 * Run from the root of the project that hosts the skill:
 *
 *   bun .agents/skills/use-bun/scripts/update.ts           # rewrite the indexes
 *   bun .agents/skills/use-bun/scripts/update.ts --check   # fail when stale
 *
 * Content inside `<!-- curated:start -->` / `<!-- curated:end -->` markers is
 * never modified, so project-specific notes survive regeneration.
 */

import path from "node:path";
import { rm } from "node:fs/promises";

const SKILL_DIR = path.resolve(import.meta.dir, "..");
const SKILL_RELATIVE_DIR = ".agents/skills/use-bun";
const SKILL_PATH = path.join(SKILL_DIR, "SKILL.md");
const REFERENCE_DIR = path.join(SKILL_DIR, "references");
const AGENTS_SNIPPET_PATH = path.join(SKILL_DIR, "agents-snippet.md");
const AGENTS_PATH = path.join(process.cwd(), "AGENTS.md");

const DOCS_ORIGIN = "https://bun.com";
const LLMS_INDEX_URL = `${DOCS_ORIGIN}/docs/llms.txt`;
const SITEMAP_URL = `${DOCS_ORIGIN}/sitemap.xml`;
const REQUEST_CONCURRENCY = 8;

const CURATED_START = "<!-- curated:start -->";
const CURATED_END = "<!-- curated:end -->";
const GENERATED_START = "<!-- generated:start -->";
const GENERATED_END = "<!-- generated:end -->";

/** Markers around the always-visible block inside the project AGENTS.md. */
const AGENTS_START = "<!-- the-ai-lab:use-bun:start -->";
const AGENTS_END = "<!-- the-ai-lab:use-bun:end -->";

interface DocEntry {
	/** Site-relative Markdown path, e.g. `docs/runtime/file-io.md`. */
	path: string;
	title: string;
	description: string;
}

interface Bucket {
	id: string;
	file: string;
	title: string;
	/** Shown in the SKILL.md routing table. */
	summary: string;
	/** Shown at the top of the reference file. */
	blurb: string;
	/** Ordered path predicates; the first matching bucket wins. */
	matches: (path: string) => boolean;
	/** Split the index into headings by sub-directory (useful for guides). */
	groupByDirectory?: boolean;
}

/* -------------------------------------------------------------------------- */
/* Buckets                                                                    */
/* -------------------------------------------------------------------------- */

const startsWith =
	(prefix: string) =>
	(path: string): boolean =>
		path.startsWith(prefix);

const equals =
	(...paths: string[]) =>
	(path: string): boolean =>
		paths.includes(path);

const anyOf =
	(...predicates: ((path: string) => boolean)[]) =>
	(path: string): boolean =>
		predicates.some((predicate) => predicate(path));

/**
 * Pages that are not useful when writing code: licence, contribution process,
 * roadmap, and the 1,000 individual linter rule pages.
 */
const shouldSkip = (path: string): boolean =>
	path.startsWith("docs/project/") ||
	path === "docs/feedback.md" ||
	path.startsWith("docs/lint/rules");

const BUCKETS: Bucket[] = [
	{
		id: "getting-started",
		file: "getting-started.md",
		title: "Getting started",
		summary:
			"Installing Bun, scaffolding a project (`bun init`, `bun create`).",
		blurb: "Installation, quickstart, and project templating.",
		matches: anyOf(
			equals(
				"docs.md",
				"docs/index.md",
				"docs/installation.md",
				"docs/quickstart.md",
			),
			startsWith("docs/runtime/templating/"),
		),
	},
	{
		id: "tooling",
		file: "tooling.md",
		title: "Type checking, linting, formatting",
		summary: "`bun check`, `bun lint`, `bun fmt`, and TypeScript 6/7 setup.",
		blurb:
			"Bun's native replacements for `tsc`, ESLint, and Prettier, plus TypeScript configuration.",
		matches: anyOf(
			equals("docs/lint.md", "docs/format.md"),
			startsWith("docs/lint/"),
			startsWith("docs/typescript"),
			equals("docs/runtime/check.md", "docs/runtime/typescript.md"),
		),
	},
	{
		id: "server",
		file: "server.md",
		title: "HTTP server and networking",
		summary: "`Bun.serve`, routes, TLS, WebSockets, TCP/UDP/DNS, `fetch`.",
		blurb: "HTTP servers, routing, WebSockets, and low-level networking.",
		matches: anyOf(
			startsWith("docs/runtime/http/"),
			startsWith("docs/runtime/networking/"),
		),
	},
	{
		id: "runtime",
		file: "runtime.md",
		title: "Runtime and built-in APIs",
		summary:
			"File I/O, shell, spawn, SQL/SQLite, Redis, S3, hashing, glob, streams, workers, ...",
		blurb:
			"Core runtime APIs: files, processes, databases, binary data, and utilities.",
		matches: anyOf(
			equals("docs/runtime.md", "docs/runtime/index.md"),
			startsWith("docs/runtime/"),
		),
	},
	{
		id: "package-manager",
		file: "package-manager.md",
		title: "Package manager",
		summary:
			"`bun install`/`add`/`remove`/`update`, workspaces, catalogs, lockfile, registries.",
		blurb: "Installing, updating, and publishing packages with `bun install`.",
		matches: startsWith("docs/pm/"),
	},
	{
		id: "bundler",
		file: "bundler.md",
		title: "Bundler and dev server",
		summary:
			"`Bun.build`, HMR, fullstack dev server, CSS, HTML, executables, minifier.",
		blurb:
			"Native bundling, the fullstack dev server, and standalone executables.",
		matches: startsWith("docs/bundler"),
	},
	{
		id: "test",
		file: "test.md",
		title: "Test runner",
		summary:
			"`bun test`, mocks, snapshots, DOM testing, coverage, parallel runs.",
		blurb: "The Jest-compatible test runner and its APIs.",
		matches: startsWith("docs/test"),
	},
	{
		id: "guides",
		file: "guides.md",
		title: "Guides",
		summary:
			"Task-oriented recipes: file conversions, HTTP patterns, ecosystems.",
		blurb: "Step-by-step recipes from the official guides section.",
		matches: startsWith("docs/guides/"),
		groupByDirectory: true,
	},
];

const FALLBACK_BUCKET: Bucket = {
	id: "misc",
	file: "misc.md",
	title: "Unclassified documentation",
	summary: "Pages the classifier has not seen yet (report them upstream).",
	blurb:
		"Pages that did not match any known documentation section. New sections appear here first.",
	matches: () => true,
};

const ALL_BUCKETS = [...BUCKETS, FALLBACK_BUCKET];

/* -------------------------------------------------------------------------- */
/* Fetching                                                                   */
/* -------------------------------------------------------------------------- */

/**
 * Runs an async mapper with a bounded number of concurrent requests.
 *
 * @param items Items to process.
 * @param limit Maximum number of in-flight calls.
 * @param mapper Async function applied to every item.
 * @returns Results in the same order as the input.
 */
async function mapLimit<T, R>(
	items: T[],
	limit: number,
	mapper: (item: T) => Promise<R>,
): Promise<R[]> {
	const results = Array.from<R>({ length: items.length });
	let cursor = 0;

	async function worker(): Promise<void> {
		while (cursor < items.length) {
			const index = cursor;
			cursor += 1;
			results[index] = await mapper(items[index]);
		}
	}

	const workers = Array.from({ length: Math.min(limit, items.length) }, worker);
	await Promise.all(workers);

	return results;
}

/**
 * Fetches a text resource and fails loudly on non-2xx responses.
 *
 * @param url Absolute URL to fetch.
 * @returns Response body as text.
 */
async function fetchText(url: string): Promise<string> {
	const response = await fetch(url);

	if (!response.ok) {
		throw new Error(`GET ${url} failed with ${response.status}`);
	}

	return response.text();
}

/**
 * Fetches a documentation page and extracts its title and summary from the
 * Markdown heading and blockquote that every Bun doc page starts with.
 *
 * @param path Site-relative Markdown path.
 * @returns Title, description, and effective path, or `null` when missing.
 */
async function fetchDocMeta(
	path: string,
): Promise<{ path: string; title: string; description: string } | null> {
	const response = await fetch(`${DOCS_ORIGIN}/${path}`);

	if (response.status === 404) {
		return null;
	}

	if (!response.ok) {
		throw new Error(`GET ${path} failed with ${response.status}`);
	}

	const markdown = await response.text();
	const lines = markdown.split("\n");
	const heading = lines.find((line) => line.startsWith("# "));
	const quote = lines.find((line) => line.startsWith("> "));

	return {
		path,
		title: heading ? heading.slice(2).trim() : path,
		description: quote ? quote.slice(2).trim() : "",
	};
}

/* -------------------------------------------------------------------------- */
/* Discovery                                                                  */
/* -------------------------------------------------------------------------- */

const LINK_PATTERN = /^- \[([^\]]+)\]\(([^)]+)\)(?::\s*(.*))?$/;

/**
 * Parses `llms.txt` into doc entries.
 *
 * @param markdown Contents of `https://bun.com/docs/llms.txt`.
 * @returns Entries keyed by site-relative path.
 */
function parseLlmsIndex(markdown: string): Map<string, DocEntry> {
	const entries = new Map<string, DocEntry>();

	for (const line of markdown.split("\n")) {
		const match = LINK_PATTERN.exec(line.trim());

		if (!match) {
			continue;
		}

		const [, title, url, description = ""] = match;

		if (!url.startsWith(`${DOCS_ORIGIN}/docs/`) || !url.endsWith(".md")) {
			continue;
		}

		const path = url.slice(DOCS_ORIGIN.length + 1);
		entries.set(path, { path, title, description });
	}

	return entries;
}

/**
 * Lists documentation pages advertised in the sitemap.
 *
 * @param xml Contents of `https://bun.com/sitemap.xml`.
 * @returns Site-relative Markdown paths, deduplicated.
 */
function parseSitemap(xml: string): string[] {
	const paths = new Set<string>();
	const pattern = /<loc>([^<]+)<\/loc>/g;

	for (const match of xml.matchAll(pattern)) {
		const [, url] = match;

		if (!url.startsWith(`${DOCS_ORIGIN}/docs/`)) {
			continue;
		}

		paths.add(`${url.slice(DOCS_ORIGIN.length + 1)}.md`);
	}

	return [...paths];
}

/**
 * Maps a dead `/index.md` URL to its canonical sibling, e.g.
 * `docs/runtime/index.md` becomes `docs/runtime.md`.
 *
 * @param path Site-relative Markdown path.
 * @returns The canonical alias, or `null` when none applies.
 */
function indexAlias(path: string): string | null {
	if (path === "docs/index.md") {
		return "docs.md";
	}

	if (path.endsWith("/index.md")) {
		return `${path.slice(0, -"/index.md".length)}.md`;
	}

	return null;
}

/**
 * Merges the `llms.txt` index with the sitemap, then verifies every page.
 *
 * `llms.txt` is not authoritative: it omits pages (type checker, linter,
 * formatter, module graph, ...) and keeps links to pages that were removed
 * (for example `runtime/index.md`). The sitemap recovers the former and a
 * fetch of every `.md` URL drops the latter.
 *
 * @returns Every live documentation page, deduplicated and sorted by path.
 */
async function collectDocs(): Promise<DocEntry[]> {
	const [llmsText, sitemapXml] = await Promise.all([
		fetchText(LLMS_INDEX_URL),
		fetchText(SITEMAP_URL),
	]);

	const listed = parseLlmsIndex(llmsText);
	const candidates = new Set(listed.keys());

	for (const path of parseSitemap(sitemapXml)) {
		if (!shouldSkip(path)) {
			candidates.add(path);
		}
	}

	const paths = [...candidates].filter((path) => !shouldSkip(path));
	const resolved = await mapLimit(
		paths,
		REQUEST_CONCURRENCY,
		async (path): Promise<DocEntry | null> => {
			const listedEntry = listed.get(path);
			const direct = await fetchDocMeta(path);
			const alias = direct ? null : indexAlias(path);
			const meta = direct ?? (alias ? await fetchDocMeta(alias) : null);

			if (!meta) {
				return null;
			}

			return {
				path: meta.path,
				title: listedEntry?.title || meta.title,
				description: listedEntry?.description || meta.description,
			};
		},
	);

	const byPath = new Map<string, DocEntry>();

	for (const entry of resolved) {
		if (entry && !byPath.has(entry.path)) {
			byPath.set(entry.path, entry);
		}
	}

	const entries = [...byPath.values()];
	const dead = paths.filter((path, index) => resolved[index] === null);

	console.log(
		`Checked ${paths.length} pages: ${entries.length} live, ${dead.length} dead links dropped`,
	);

	for (const path of dead) {
		console.warn(`  dead: ${path}`);
	}

	return entries.sort((a, b) => a.path.localeCompare(b.path));
}

/* -------------------------------------------------------------------------- */
/* Rendering                                                                  */
/* -------------------------------------------------------------------------- */

const DIRECTORY_LABELS: Record<string, string> = {
	"html-rewriter": "HTMLRewriter",
	"read-file": "Read a file",
	"write-file": "Write a file",
	websocket: "WebSockets",
};

/**
 * Turns a directory slug into a readable heading.
 *
 * @param slug Directory name, e.g. `read-file`.
 * @returns Human-readable label.
 */
function directoryLabel(slug: string): string {
	const known = DIRECTORY_LABELS[slug];

	if (known) {
		return known;
	}

	const label = slug.replaceAll("-", " ");

	return label.charAt(0).toUpperCase() + label.slice(1);
}

/**
 * Groups entries by the first directory segment below the bucket root.
 *
 * @param entries Entries assigned to a bucket.
 * @param root Directory prefix shared by the entries, e.g. `docs/guides/`.
 * @returns Sections in first-seen order.
 */
function groupByTopDirectory(
	entries: DocEntry[],
	root: string,
): { label: string; entries: DocEntry[] }[] {
	const sections = new Map<string, DocEntry[]>();

	for (const entry of entries) {
		const rest = entry.path.slice(root.length);
		const separator = rest.indexOf("/");
		const label =
			separator === -1 ? "Overview" : directoryLabel(rest.slice(0, separator));
		const section = sections.get(label) ?? [];
		section.push(entry);
		sections.set(label, section);
	}

	return [...sections].map(([label, sectionEntries]) => ({
		label,
		entries: sectionEntries,
	}));
}

/**
 * Renders one bullet list line for a doc page.
 *
 * @param entry Documentation entry.
 * @returns Markdown list item.
 */
function renderEntry(entry: DocEntry): string {
	const url = `${DOCS_ORIGIN}/${entry.path}`;

	if (!entry.description) {
		return `- [${entry.title}](${url})`;
	}

	return `- [${entry.title}](${url}): ${entry.description}`;
}

/**
 * Extracts the curated block from an existing file, if any.
 *
 * @param existing Previous file contents, or `null` for a new file.
 * @returns Preserved text, without the marker lines.
 */
function readCuratedBlock(existing: string | null): string {
	if (!existing) {
		return "";
	}

	const start = existing.indexOf(CURATED_START);
	const end = existing.indexOf(CURATED_END);

	if (start === -1 || end === -1 || end < start) {
		return "";
	}

	return existing.slice(start + CURATED_START.length, end).trim();
}

/**
 * Renders the generated region of a reference file.
 *
 * @param bucket Bucket being rendered.
 * @param entries Entries assigned to the bucket.
 * @param version Bun version stamped in the header.
 * @param updated ISO date of the update.
 * @returns Markdown between the generated markers.
 */
function renderReferenceBody(
	bucket: Bucket,
	entries: DocEntry[],
	version: string,
	updated: string,
): string {
	const lines = [
		`_${entries.length} pages · Bun v${version} · updated ${updated}. Regenerate with \`bun ${SKILL_RELATIVE_DIR}/scripts/update.ts\`._`,
		"",
	];

	const sections = bucket.groupByDirectory
		? groupByTopDirectory(entries, "docs/guides/")
		: [{ label: "", entries }];

	for (const section of sections) {
		if (section.label) {
			lines.push(`## ${section.label}`, "");
		}

		for (const entry of section.entries) {
			lines.push(renderEntry(entry));
		}

		lines.push("");
	}

	return lines.join("\n").trimEnd();
}

/**
 * Renders a complete reference file, preserving any curated notes.
 *
 * @param bucket Bucket being rendered.
 * @param entries Entries assigned to the bucket.
 * @param existing Previous file contents, or `null`.
 * @param version Bun version stamped in the header.
 * @param updated ISO date of the update.
 * @returns File contents.
 */
function renderReference(
	bucket: Bucket,
	entries: DocEntry[],
	existing: string | null,
	version: string,
	updated: string,
): string {
	const curated = readCuratedBlock(existing);
	const body = renderReferenceBody(bucket, entries, version, updated);

	return [
		`# Bun — ${bucket.title}`,
		"",
		`> ${bucket.blurb} Every page is also available as raw Markdown: append \`.md\` to the URL.`,
		"",
		CURATED_START,
		curated,
		CURATED_END,
		"",
		GENERATED_START,
		body,
		GENERATED_END,
		"",
	].join("\n");
}

/**
 * Renders the routing table that lives in `SKILL.md`.
 *
 * @param counts Number of indexed pages per bucket id.
 * @param total Total number of indexed pages.
 * @param version Bun version stamped in the header.
 * @param updated ISO date of the update.
 * @returns Markdown between the generated markers of `SKILL.md`.
 */
function renderSkillBody(
	counts: Map<string, number>,
	total: number,
	version: string,
	updated: string,
): string {
	const lines = [
		`Bun **v${version}** · ${total} official doc pages indexed · updated ${updated}`,
		"",
		"| Task | Read | Pages |",
		"| --- | --- | --- |",
	];

	for (const bucket of BUCKETS) {
		lines.push(
			`| ${bucket.summary} | [references/${bucket.file}](references/${bucket.file}) | ${counts.get(bucket.id) ?? 0} |`,
		);
	}

	const unclassified = counts.get(FALLBACK_BUCKET.id) ?? 0;

	if (unclassified > 0) {
		lines.push(
			`| Not yet classified | [references/${FALLBACK_BUCKET.file}](references/${FALLBACK_BUCKET.file}) | ${unclassified} |`,
		);
	}

	return lines.join("\n");
}

/**
 * Replaces the generated region of a document between two markers.
 *
 * @param contents Existing document.
 * @param startMarker Opening marker line.
 * @param endMarker Closing marker line.
 * @param body New generated body.
 * @returns Updated document.
 */
function replaceBetweenMarkers(
	contents: string,
	startMarker: string,
	endMarker: string,
	body: string,
): string {
	const start = contents.indexOf(startMarker);
	const end = contents.indexOf(endMarker);

	if (start === -1 || end === -1 || end < start) {
		throw new Error(`Missing ${startMarker} / ${endMarker} markers`);
	}

	return (
		contents.slice(0, start + startMarker.length) +
		"\n" +
		body +
		"\n" +
		contents.slice(end)
	);
}

/**
 * Builds the marker-wrapped block inserted into the project `AGENTS.md`.
 *
 * @param snippet Snippet text from `agents-snippet.md`.
 * @param version Bun version stamped in the block.
 * @param updated ISO date of the update.
 * @returns Block including the marker lines.
 */
function renderAgentsBlock(
	snippet: string,
	version: string,
	updated: string,
): string {
	const footer = `_Maintained by the \`use-bun\` skill · Bun v${version} · updated ${updated}. Run \`bun ${SKILL_RELATIVE_DIR}/scripts/update.ts\` to refresh._`;

	return [AGENTS_START, snippet.trim(), "", footer, AGENTS_END].join("\n");
}

/**
 * Inserts or replaces the managed block inside the project `AGENTS.md`.
 *
 * @param contents Existing `AGENTS.md`, or `null` when the file is missing.
 * @param block Marker-wrapped block to insert.
 * @returns Updated `AGENTS.md` contents.
 */
function upsertAgentsBlock(contents: string | null, block: string): string {
	if (!contents) {
		return `# Agent Instructions\n\n${block}\n`;
	}

	const start = contents.indexOf(AGENTS_START);
	const end = contents.indexOf(AGENTS_END);

	if (start !== -1 && end !== -1 && end > start) {
		return (
			contents.slice(0, start) + block + contents.slice(end + AGENTS_END.length)
		);
	}

	const separator = contents.endsWith("\n\n")
		? ""
		: contents.endsWith("\n")
			? "\n"
			: "\n\n";

	return contents.trimEnd() + separator + "\n" + block + "\n";
}

/* -------------------------------------------------------------------------- */
/* Main                                                                       */
/* -------------------------------------------------------------------------- */

/**
 * Removes blank lines so that formatter whitespace differences do not make a
 * managed block look stale.
 *
 * @param text Markdown text.
 * @returns Text with every blank line removed.
 */
function compressBlankLines(text: string): string {
	return text
		.split("\n")
		.filter((line) => line.trim() !== "")
		.join("\n");
}

/**
 * Rewrites (or verifies) the skill reference indexes and the AGENTS.md block.
 *
 * @returns Nothing.
 */
async function main(): Promise<void> {
	const checkOnly = process.argv.includes("--check");
	const version = Bun.version;
	const updated = new Date().toISOString().slice(0, 10);
	const docs = await collectDocs();

	const assigned = new Map<string, DocEntry[]>(
		ALL_BUCKETS.map((bucket) => [bucket.id, []]),
	);

	for (const entry of docs) {
		const bucket = ALL_BUCKETS.find((candidate) =>
			candidate.matches(entry.path),
		);

		assigned.get(bucket!.id)!.push(entry);
	}

	const stale: string[] = [];
	const counts = new Map<string, number>();

	for (const bucket of ALL_BUCKETS) {
		const entries = assigned.get(bucket.id) ?? [];

		if (entries.length === 0 && bucket === FALLBACK_BUCKET) {
			// Keep the working tree clean when every page is classified.
			await rm(`${REFERENCE_DIR}/${bucket.file}`, { force: true });
			continue;
		}

		counts.set(bucket.id, entries.length);

		const destination = `${REFERENCE_DIR}/${bucket.file}`;
		const previous = Bun.file(destination);
		const existing = (await previous.exists()) ? await previous.text() : null;
		const next = renderReference(bucket, entries, existing, version, updated);

		if (next !== existing) {
			if (checkOnly) {
				stale.push(destination);
			} else {
				await Bun.write(destination, next);
			}
		}
	}

	const skillFile = Bun.file(SKILL_PATH);

	if (!(await skillFile.exists())) {
		throw new Error(`Missing skill file: ${SKILL_PATH}`);
	}

	const skillContents = await skillFile.text();

	// The version is stamped in the frontmatter so agents can see how fresh
	// the index is without reading the generated table.
	const stampedSkill = skillContents.replace(
		/^bun-version: .*$/m,
		`bun-version: "${version}"`,
	);
	const nextSkill = replaceBetweenMarkers(
		stampedSkill,
		GENERATED_START,
		GENERATED_END,
		renderSkillBody(counts, docs.length, version, updated),
	);

	if (nextSkill !== skillContents) {
		if (checkOnly) {
			stale.push(SKILL_PATH);
		} else {
			await Bun.write(SKILL_PATH, nextSkill);
		}
	}

	const snippetFile = Bun.file(AGENTS_SNIPPET_PATH);

	if (!(await snippetFile.exists())) {
		throw new Error(`Missing AGENTS.md snippet: ${AGENTS_SNIPPET_PATH}`);
	}

	const agentsFile = Bun.file(AGENTS_PATH);
	const agentsExisting = (await agentsFile.exists())
		? await agentsFile.text()
		: null;
	const nextAgents = upsertAgentsBlock(
		agentsExisting,
		renderAgentsBlock(await snippetFile.text(), version, updated),
	);

	if (
		compressBlankLines(nextAgents) !== compressBlankLines(agentsExisting ?? "")
	) {
		if (checkOnly) {
			stale.push(AGENTS_PATH);
		} else {
			await Bun.write(AGENTS_PATH, nextAgents);
		}
	}

	if (checkOnly) {
		if (stale.length > 0) {
			console.error(
				`Stale Bun skill files (Bun v${version}):\n${stale.map((file) => `  - ${file}`).join("\n")}\nRun: bun ${SKILL_RELATIVE_DIR}/scripts/update.ts`,
			);
			process.exit(1);
		}

		console.log(`Bun skill files are up to date (v${version}).`);

		return;
	}

	console.log(
		`Bun skill updated: ${docs.length} pages across ${counts.size} references, AGENTS.md block synced (v${version}).`,
	);
}

await main();
