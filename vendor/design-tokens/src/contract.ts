import { parse as parseYaml, stringify as stringifyYaml } from "yaml";
import {
	isIgnoredPath,
	TOKENS,
	type TokenValues,
	tokenByPath,
	tokenByVariable
} from "./design-system";
import { fontStackFamilies, googleFontImport } from "./fonts";

/** Fields of a typographic style that reference a token variable. */
export type TypographyField = "fontWeight" | "lineHeight" | "letterSpacing";

/** Style → field → referenced CSS variable, e.g. `display.fontWeight` → `--font-weight-bold`. */
export type TypographyRefs = Record<string, Partial<Record<TypographyField, string>>>;

/** Result of parsing a `DESIGN.md` front matter block. */
export interface ParsedDesignMd {
	/** Variable → value, for every known token declared in the front matter. */
	values: TokenValues;
	/** Token paths actually declared in the front matter. */
	declaredPaths: Set<string>;
	/** Front-matter paths that match no token and are not ignored. */
	unknownPaths: string[];
	/** Style → field → token variable, for `typography.<style>.{fontWeight,lineHeight,letterSpacing}`. */
	typography: TypographyRefs;
	/** The theme's display name from the front matter, or `""` when absent. */
	name: string;
	/** The original parsed front-matter object, for lossless re-serialization. */
	frontMatter: Record<string, unknown>;
	/** The markdown body after the front matter. */
	body: string;
}

/**
 * Normalize a CSS or YAML scalar: collapse whitespace and trim.
 * @param value - The scalar to normalize.
 * @returns The value with runs of whitespace collapsed and ends trimmed.
 */
export const normalizeValue = (value: string): string => value.replace(/\s+/g, " ").trim();

const flatten = (value: unknown, prefix: string, out: Record<string, string>): void => {
	if (value === null || value === undefined) {
		return;
	}

	if (typeof value === "object" && !Array.isArray(value)) {
		for (const [key, child] of Object.entries(value)) {
			flatten(child, prefix ? `${prefix}.${key}` : key, out);
		}

		return;
	}
	out[prefix] = String(value);
};

const setPath = (root: Record<string, unknown>, path: string, value: string): void => {
	const segments = path.split(".");
	let cursor = root;

	for (const segment of segments.slice(0, -1)) {
		const next: unknown = cursor[segment];

		if (typeof next !== "object" || next === null) {
			cursor[segment] = {};
		}
		cursor = cursor[segment] as Record<string, unknown>;
	}
	cursor[segments.at(-1) ?? path] = value;
};

const deletePath = (root: Record<string, unknown>, path: string): void => {
	const segments = path.split(".");
	let cursor: Record<string, unknown> | undefined = root;

	for (const segment of segments.slice(0, -1)) {
		const next: unknown = cursor?.[segment];
		cursor =
			typeof next === "object" && next !== null
				? (next as Record<string, unknown>)
				: undefined;
	}
	const leaf = segments.at(-1);

	if (cursor && leaf) {
		delete cursor[leaf];
	}
};

const VARIABLE_REFERENCE = /var\(\s*(--[\w-]+)(?:\s*,\s*([^)]+))?\s*\)/g;

/**
 * Resolve `var(--x)` references against a value map so a front-matter literal and a
 * stylesheet alias compare equal. Depth is bounded to avoid reference cycles.
 * @param value - The value that may contain `var()` references.
 * @param values - The variable map to resolve references against.
 * @param depth - The current recursion depth, capped to break cycles.
 * @returns The value with resolvable `var()` references replaced.
 */
export const resolveVariables = (value: string, values: TokenValues, depth = 0): string => {
	if (depth > 8 || !value.includes("var(")) {
		return value;
	}
	const next = value.replace(VARIABLE_REFERENCE, (_match, name: string, fallback?: string) => {
		const referenced = values[name];

		if (referenced !== undefined) {
			return referenced;
		}

		return fallback?.trim() ?? "";
	});

	return next === value ? next : resolveVariables(next, values, depth + 1);
};

const FONT_FAMILY_TOKENS = TOKENS.filter((token) => token.kind === "font-family");
const FONT_WEIGHT_TOKENS = TOKENS.filter((token) => token.kind === "font-weight");

/** The theme's font families, resolved through `var()` aliases, for the font import. */
const fontFamilies = (values: TokenValues): string[] =>
	FONT_FAMILY_TOKENS.flatMap((token) =>
		fontStackFamilies(resolveVariables(values[token.variable] ?? "", values))
	);

/** The numeric weights the theme uses, for the font import's `wght` axis. */
const fontWeights = (values: TokenValues): number[] =>
	FONT_WEIGHT_TOKENS.map((token) =>
		Number.parseFloat(resolveVariables(values[token.variable] ?? "", values))
	).filter((weight) => Number.isFinite(weight));

const FRONT_MATTER = /^---\r?\n([\s\S]*?)\r?\n---\r?\n?([\s\S]*)$/;

const TYPOGRAPHY_PATH = /^typography\.([^.]+)\.(fontWeight|lineHeight|letterSpacing)$/;
const VARIABLE_NAME = /var\(\s*(--[\w-]+)/;

/** Collect the token variable each typographic style references for weight/line-height/etc. */
const collectTypography = (flat: Record<string, string>): TypographyRefs => {
	const refs: TypographyRefs = {};

	for (const [path, value] of Object.entries(flat)) {
		const match = TYPOGRAPHY_PATH.exec(path);
		const variable = VARIABLE_NAME.exec(value)?.[1];
		const style = match?.[1];
		const field = match?.[2] as TypographyField | undefined;

		if (!style || !field || !variable) {
			continue;
		}
		refs[style] = { ...refs[style], [field]: variable };
	}

	return refs;
};

/** Read the front-matter `name`, trimmed; a non-string or empty value yields `""`. */
const readName = (frontMatter: Record<string, unknown>): string =>
	typeof frontMatter.name === "string" ? frontMatter.name.trim() : "";

/**
 * Parse a `DESIGN.md` file into token values and preserved metadata.
 * @param text - The full `DESIGN.md` content, front matter included.
 * @returns The token values, declared and unknown paths, typography refs, and body.
 */
export const parseDesignMd = (text: string): ParsedDesignMd => {
	const match = FRONT_MATTER.exec(text);

	if (!match) {
		throw new Error("DESIGN.md: missing YAML front matter");
	}
	const frontMatter = (parseYaml(match[1] ?? "") ?? {}) as Record<string, unknown>;

	const flat: Record<string, string> = {};
	flatten(frontMatter, "", flat);

	const values: TokenValues = {};
	const declaredPaths = new Set<string>();
	const unknownPaths: string[] = [];

	for (const [path, value] of Object.entries(flat)) {
		if (isIgnoredPath(path)) {
			continue;
		}
		const token = tokenByPath(path);

		if (!token) {
			unknownPaths.push(path);
			continue;
		}
		values[token.variable] = normalizeValue(value);
		declaredPaths.add(path);
	}

	return {
		values,
		declaredPaths,
		unknownPaths,
		typography: collectTypography(flat),
		name: readName(frontMatter),
		frontMatter,
		body: match[2] ?? ""
	};
};

/**
 * Rebuild the `DESIGN.md` front matter from token values, preserving metadata and prose.
 * Optional tokens that still equal their documented fallback are omitted, per the contract.
 * The `typography` map writes each style's token references back as `var(...)`. A `name`
 * argument rewrites the theme name; `undefined` leaves the original name untouched.
 * @param values - The token values to write into the front matter.
 * @param original - The current `DESIGN.md` text to preserve metadata and prose from.
 * @param typography - Style token references to write back; omitted to keep the original.
 * @param name - A replacement theme name; `undefined` keeps the original.
 * @returns The rebuilt `DESIGN.md` text.
 */
export const serializeDesignMd = (
	values: TokenValues,
	original: string,
	typography?: TypographyRefs,
	name?: string
): string => {
	const parsed = parseDesignMd(original);
	const frontMatter = structuredClone(parsed.frontMatter);

	if (name !== undefined) {
		const trimmed = name.trim();

		if (trimmed) {
			frontMatter.name = trimmed;
		} else {
			delete frontMatter.name;
		}
	}

	for (const token of TOKENS) {
		if (!token.path) {
			continue;
		}
		const value = values[token.variable];

		if (value === undefined || value === "") {
			continue;
		}

		if (
			!token.required &&
			token.fallback !== undefined &&
			normalizeValue(value) === normalizeValue(token.fallback)
		) {
			deletePath(frontMatter, token.path);
			continue;
		}
		setPath(frontMatter, token.path, value);
	}

	for (const [style, fields] of Object.entries(typography ?? {})) {
		for (const [field, variable] of Object.entries(fields)) {
			if (variable) {
				setPath(frontMatter, `typography.${style}.${field}`, `var(${variable})`);
			}
		}
	}

	const yamlText = stringifyYaml(frontMatter, { lineWidth: 0 });

	return `---\n${yamlText}---\n${parsed.body}`;
};

const CSS_VARIABLE = /--([\w-]+)\s*:\s*([^;]+);/g;

/**
 * Remove CSS comments so parsers never read commented-out declarations or at-rules.
 * @param css - The stylesheet text to strip.
 * @returns The stylesheet with its comments removed.
 */
export const stripCssComments = (css: string): string => css.replace(/\/\*[\s\S]*?\*\//g, "");

/**
 * Parse every custom property from a stylesheet into a variable → value map.
 * @param text - The stylesheet text to read.
 * @returns Every custom property mapped to its normalized value.
 */
export const parseTokensCss = (text: string): TokenValues => {
	const clean = stripCssComments(text);
	const values: TokenValues = {};

	for (;;) {
		const match = CSS_VARIABLE.exec(clean);

		if (match === null) {
			break;
		}
		const name = match[1];
		const raw = match[2];

		if (name !== undefined && raw !== undefined) {
			values[`--${name}`] = normalizeValue(raw);
		}
	}

	return values;
};

/**
 * Serialize the full token set into one scoped block. Optional tokens keep their fallbacks.
 * @param values - The token values to emit.
 * @param selector - The CSS selector to scope the block to.
 * @returns The stylesheet block, with a leading font import when one is needed.
 */
export const serializeTokensCss = (values: TokenValues, selector = ":root"): string => {
	// Web fonts must be imported before any rule or the font-family tokens fall back to
	// the local stack; system-only themes produce no import.
	const fontImport = googleFontImport(fontFamilies(values), fontWeights(values));
	const lines = fontImport ? [fontImport, ""] : [];
	lines.push(`${selector} {`);
	let currentGroup = "";

	for (const token of TOKENS) {
		if (token.group !== currentGroup) {
			if (currentGroup !== "") {
				lines.push("");
			}
			lines.push(`\t/* ${token.group} */`);
			currentGroup = token.group;
		}
		const value = values[token.variable] || token.fallback || "";
		lines.push(`\t${token.variable}: ${value};`);
	}
	lines.push("}");

	return lines.join("\n");
};

/**
 * Rewrite a stylesheet's root selector so the tokens apply only inside a preview scope.
 * @param css - The stylesheet text whose `:root` selector is rewritten.
 * @param scope - The selector that replaces `:root`.
 * @returns The stylesheet with its root selector scoped.
 */
export const scopeTokensCss = (css: string, scope: string): string => css.replace(/:root\b/, scope);

/** A contract is the two parsed files plus their merged view. */
export interface Contract {
	/** The raw `design-tokens.css` text, for checks that read at-rules the parser skips. */
	tokensCss: string;
	designValues: TokenValues;
	cssValues: TokenValues;
	declaredPaths: Set<string>;
	unknownPaths: string[];
	body: string;
	/** The theme's front-matter name, or `""`. */
	name: string;
}

/**
 * Parse both halves of the contract. CSS values win for keys absent from the front matter.
 * @param designMd - The `DESIGN.md` text to parse.
 * @param tokensCss - The `design-tokens.css` text to parse.
 * @returns The two parsed halves plus their merged bookkeeping.
 */
export const parseContract = (designMd: string, tokensCss: string): Contract => {
	const design = parseDesignMd(designMd);
	const cssValues = parseTokensCss(tokensCss);

	return {
		tokensCss,
		designValues: design.values,
		cssValues,
		declaredPaths: design.declaredPaths,
		unknownPaths: design.unknownPaths,
		body: design.body,
		name: design.name
	};
};

/**
 * Merge both halves into one value map, preferring the front matter.
 * @param contract - The parsed contract whose halves are merged.
 * @returns The design values layered over the stylesheet values.
 */
export const mergeValues = (contract: Contract): TokenValues => ({
	...contract.cssValues,
	...contract.designValues
});

/**
 * True when a variable is a registered token.
 * @param variable - The `--variable` to test.
 * @returns True when the variable is a documented token.
 */
export const isKnownVariable = (variable: string): boolean => Boolean(tokenByVariable(variable));
