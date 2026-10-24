/**
 * Resolved color-contrast tokens. For each colored role, pick the theme text color with
 * the higher WCAG 2.1 contrast against the role's background, or a black/white ink when
 * neither text token reaches AA. The result is the `--color-on-*` contract tokens.
 */

import type { Rgb } from "./color-contrast";
import { contrastRatio, parseHex } from "./color-contrast";

/** A flat map of `--variable` to its current value. */
export type TokenList = Record<string, string>;

/** One contrast pair: the `--color-on-*` token and the background variable it answers for. */
interface ContrastRole {
	on: string;
	background: string;
}

/**
 * The eleven contrast pairs. Order defines the emitted order and matches the token tables
 * in the design-system-tokens skill. `tertiary` and `card` resolve through the background
 * variable, which already carries its own documented fallback.
 */
const ROLES: ContrastRole[] = [
	{ on: "--color-on-primary", background: "--color-brand-primary" },
	{ on: "--color-on-accent", background: "--color-brand-accent" },
	{ on: "--color-on-secondary", background: "--color-brand-secondary" },
	{ on: "--color-on-tertiary", background: "--color-brand-tertiary" },
	{ on: "--color-on-success", background: "--color-action-success" },
	{ on: "--color-on-info", background: "--color-action-info" },
	{ on: "--color-on-warning", background: "--color-action-warning" },
	{ on: "--color-on-danger", background: "--color-action-danger" },
	{ on: "--color-on-dark", background: "--color-surface-dark" },
	{ on: "--color-on-alt", background: "--color-surface-alt" },
	{ on: "--color-on-card", background: "--color-surface-card" }
];

const TEXT_BASE = "--color-text";
const TEXT_ON_DARK = "--color-text-ondark";

/** WCAG AA contrast for normal text. */
export const AA_NORMAL = 4.5;

/** The eleven `--color-on-*` variables, in emitted order. */
export const CONTRAST_TOKENS: string[] = ROLES.map((role) => role.on);

const BLACK: Rgb = { r: 0, g: 0, b: 0 };
const WHITE: Rgb = { r: 255, g: 255, b: 255 };

const VARIABLE_REFERENCE = /var\(\s*(--[\w-]+)(?:\s*,\s*([^)]+))?\s*\)/g;

/**
 * Resolve `var(--x)` references against a token list so an alias compares equal to the
 * literal it points to. Depth is bounded to avoid reference cycles.
 */
const resolve = (value: string, list: TokenList, depth = 0): string => {
	if (depth > 8 || !value.includes("var(")) {
		return value;
	}
	const next = value.replace(VARIABLE_REFERENCE, (_match, name: string, fallback?: string) => {
		const referenced = list[name];

		if (referenced !== undefined) {
			return referenced;
		}

		return fallback?.trim() ?? "";
	});

	return next === value ? next : resolve(next, list, depth + 1);
};

/** Parse one color from the list, resolving its `var()` aliases first. */
const colorOf = (list: TokenList, variable: string): Rgb | null =>
	parseHex(resolve(list[variable] ?? "", list));

/**
 * Pick the foreground for one background: the theme text token with the higher contrast,
 * a tie going to the on-dark token. When neither reaches AA, fall back to pure black or
 * white so the label stays legible.
 */
const resolveOn = (background: Rgb, textBase: Rgb, textOnDark: Rgb): string => {
	const baseRatio = contrastRatio(textBase, background);
	const onDarkRatio = contrastRatio(textOnDark, background);
	const preferOnDark = onDarkRatio >= baseRatio;
	const themeRatio = preferOnDark ? onDarkRatio : baseRatio;

	if (themeRatio >= AA_NORMAL) {
		return preferOnDark ? `var(${TEXT_ON_DARK})` : `var(${TEXT_BASE})`;
	}

	return contrastRatio(WHITE, background) >= contrastRatio(BLACK, background)
		? "#ffffff"
		: "#000000";
};

/**
 * Return a copy of `tokenList` with the eleven `--color-on-*` contrast tokens recomputed
 * from its color values. `var()` references resolve against the list itself. The input is
 * not mutated. A role whose background is not a parseable color falls back to the
 * documented default `var(--color-text)`; when either text color is unparseable the list
 * is returned unchanged.
 * @param tokenList - The token list whose color values drive the derivation.
 * @returns A copy with the eleven `--color-on-*` tokens recomputed.
 */
export const updateThemeContrastTokens = (tokenList: TokenList): TokenList => {
	const textBase = colorOf(tokenList, TEXT_BASE);
	const textOnDark = colorOf(tokenList, TEXT_ON_DARK);
	const next: TokenList = { ...tokenList };

	if (!textBase || !textOnDark) {
		return next;
	}

	for (const { on, background } of ROLES) {
		// A non-hex background (rgba(), color-mix(), …) has no measurable contrast, so the
		// role keeps the documented default rather than a stale or missing value.
		const color = colorOf(tokenList, background);
		next[on] = color ? resolveOn(color, textBase, textOnDark) : `var(${TEXT_BASE})`;
	}

	return next;
};
