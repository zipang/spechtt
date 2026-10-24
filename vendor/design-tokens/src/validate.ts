import { contrastRatio, parseHex } from "./color-contrast";
import { normalizeValue, resolveVariables, stripCssComments } from "./contract";
import { AA_NORMAL, CONTRAST_TOKENS, updateThemeContrastTokens } from "./contrast-tokens";
import { TOKENS, tokenByVariable } from "./design-system";
import { fontStackFamilies, googleFontName } from "./fonts";

/** One validation finding. */
export interface Issue {
	level: "error" | "warning";
	code: string;
	message: string;
	target?: string;
}

/** The two parsed halves of the contract plus front-matter bookkeeping. */
export interface ValidationInput {
	/** The raw `design-tokens.css` text, for checks that read at-rules the parser skips. */
	tokensCss: string;
	designValues: Record<string, string>;
	cssValues: Record<string, string>;
	declaredPaths: Set<string>;
	unknownPaths: string[];
	/** The theme's front-matter name. */
	name: string;
}

const FONT_FAMILY_TOKENS = TOKENS.filter((token) => token.kind === "font-family");

/** True when the stylesheet declares a font loader: an `@import` or a `@font-face` rule. */
const hasFontImport = (tokensCss: string): boolean =>
	/@(?:import|font-face)\b/i.test(stripCssComments(tokensCss));

/** The hosted web fonts named by one font stack, from the families the catalog knows. */
const hostedFontsIn = (stack: string): string[] =>
	fontStackFamilies(stack).filter((family) => googleFontName(family) !== undefined);

/**
 * Check a parsed contract against the section 10 rules of the design-system-tokens
 * skill. Returns one issue per violation; an empty array means the contract is valid.
 * @param input - The parsed halves and front-matter bookkeeping to validate.
 * @returns One issue per violation; empty when the contract is valid.
 */
export const validateContract = ({
	tokensCss,
	designValues,
	cssValues,
	declaredPaths,
	unknownPaths,
	name
}: ValidationInput): Issue[] => {
	const issues: Issue[] = [];

	// A theme's name is its identity across the editor and its saved files.
	if (name.trim() === "") {
		issues.push({
			level: "error",
			code: "theme-name",
			message: "DESIGN.md must declare a non-empty name."
		});
	}

	// Undocumented tokens are forbidden in the stylesheet.
	for (const variable of Object.keys(cssValues)) {
		if (!tokenByVariable(variable)) {
			issues.push({
				level: "error",
				code: "undocumented-token",
				target: variable,
				message: `${variable} is not a documented design token.`
			});
		}
	}

	// Every token — required and optional — must be present.
	for (const token of TOKENS) {
		if (!(token.variable in cssValues)) {
			issues.push({
				level: "error",
				code: "missing-token",
				target: token.variable,
				message: `${token.variable} is missing from design-tokens.css.`
			});
		}
	}

	// Front-matter paths must map to a token.
	for (const path of unknownPaths) {
		issues.push({
			level: "error",
			code: "undocumented-path",
			target: path,
			message: `${path} maps to no design token.`
		});
	}

	// Required front-matter tokens must be declared.
	for (const token of TOKENS) {
		if (token.path && token.required && !declaredPaths.has(token.path)) {
			issues.push({
				level: "error",
				code: "missing-required-path",
				target: token.path,
				message: `Required token ${token.path} is missing from DESIGN.md.`
			});
		}
	}

	// Declared values must match the stylesheet (after resolving `var()` aliases).
	for (const [variable, designValue] of Object.entries(designValues)) {
		const cssValue = cssValues[variable];

		if (cssValue === undefined) {
			continue;
		}
		const resolvedCss = normalizeValue(resolveVariables(cssValue, cssValues));
		const resolvedDesign = normalizeValue(resolveVariables(designValue, cssValues));

		if (resolvedCss !== resolvedDesign) {
			issues.push({
				level: "error",
				code: "value-mismatch",
				target: variable,
				message: `${variable} differs: DESIGN.md "${designValue}" vs stylesheet "${cssValue}".`
			});
		}
	}

	// Undeclared optional tokens must keep their documented fallback.
	for (const token of TOKENS) {
		if (token.required || !token.path || token.fallback === undefined) {
			continue;
		}

		if (declaredPaths.has(token.path)) {
			continue;
		}
		const cssValue = cssValues[token.variable];

		if (cssValue !== undefined && normalizeValue(cssValue) !== normalizeValue(token.fallback)) {
			issues.push({
				level: "error",
				code: "optional-fallback",
				target: token.variable,
				message: `Optional ${token.variable} is neither declared in DESIGN.md nor set to its fallback ${token.fallback}.`
			});
		}
	}

	// Hosted web fonts must be imported by the stylesheet, or they never load.
	const fontTokens = FONT_FAMILY_TOKENS.map((token) => ({
		variable: token.variable,
		hosted: hostedFontsIn(cssValues[token.variable] ?? "")
	})).filter((entry) => entry.hosted.length > 0);

	if (fontTokens.length > 0 && !hasFontImport(tokensCss)) {
		const families = [...new Set(fontTokens.flatMap((entry) => entry.hosted))];
		issues.push({
			level: "error",
			code: "missing-font-import",
			target: fontTokens[0]?.variable,
			message: `Hosted web fonts (${families.join(", ")}) need an @import or @font-face in design-tokens.css.`
		});
	}

	// Resolved contrast pairs must equal the value the derivation rule produces.
	const derived = updateThemeContrastTokens(cssValues);

	for (const variable of CONTRAST_TOKENS) {
		const stored = cssValues[variable];
		const computed = derived[variable];

		if (stored === undefined || computed === undefined) {
			continue;
		}

		if (normalizeValue(stored) !== normalizeValue(computed)) {
			issues.push({
				level: "error",
				code: "contrast-mismatch",
				target: variable,
				message: `${variable} is "${stored}" but the contrast rule derives "${computed}".`
			});
		}
	}

	// `--color-text` is the base surface foreground by rule, so the pair must reach AA.
	const textColor = parseHex(resolveVariables(cssValues["--color-text"] ?? "", cssValues));
	const surfaceColor = parseHex(resolveVariables(cssValues["--color-surface"] ?? "", cssValues));

	if (textColor && surfaceColor) {
		const ratio = contrastRatio(textColor, surfaceColor);

		if (ratio < AA_NORMAL) {
			issues.push({
				level: "error",
				code: "text-surface-contrast",
				target: "--color-text",
				message: `--color-text on --color-surface is ${ratio.toFixed(1)}:1, below 4.5:1.`
			});
		}
	}

	return issues;
};
