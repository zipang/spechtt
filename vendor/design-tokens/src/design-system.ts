/**
 * The fixed Design System token registry. Every token from sections 2–7 of the
 * `design-system-tokens` skill appears exactly once. Front-matter tokens carry a
 * `path`; stylesheet-only tokens (font sizes, weights, line heights, letter spacing)
 * have none. This registry is the single source of the path ↔ CSS-variable mapping.
 */

/** Top-level token category. */
export type TokenGroup = "typography" | "colors" | "spacing" | "rounded" | "elevation" | "border";

/** Value domain of a token, used by editors to pick the right control. */
export type TokenKind =
	| "font-family"
	| "font-size"
	| "font-weight"
	| "line-height"
	| "letter-spacing"
	| "color"
	| "space"
	| "radius"
	| "elevation"
	| "border-width";

/** One design token: its CSS variable, optional front-matter path, and default. */
export interface TokenDef {
	variable: string;
	path?: string;
	group: TokenGroup;
	kind: TokenKind;
	required: boolean;
	fallback?: string;
}

/** A flat map of `--variable` to its current value. */
export type TokenValues = Record<string, string>;

export const TOKENS: TokenDef[] = [
	// Typography — font families (front-matter tokens)
	{
		variable: "--font-family-base",
		path: "typography.base.fontFamily",
		group: "typography",
		kind: "font-family",
		required: true
	},
	{
		variable: "--font-family-display",
		path: "typography.display.fontFamily",
		group: "typography",
		kind: "font-family",
		required: true
	},
	{
		variable: "--font-family-mono",
		path: "typography.mono.fontFamily",
		group: "typography",
		kind: "font-family",
		required: false,
		fallback: "var(--font-family-base)"
	},

	// Typography — size scale (stylesheet-only)
	{ variable: "--font-size-base", group: "typography", kind: "font-size", required: true },
	{ variable: "--font-size-xs", group: "typography", kind: "font-size", required: true },
	{ variable: "--font-size-sm", group: "typography", kind: "font-size", required: true },
	{ variable: "--font-size-md", group: "typography", kind: "font-size", required: true },
	{ variable: "--font-size-lg", group: "typography", kind: "font-size", required: true },
	{ variable: "--font-size-xl", group: "typography", kind: "font-size", required: true },
	{
		variable: "--font-size-2xl",
		group: "typography",
		kind: "font-size",
		required: false,
		fallback: "var(--font-size-xl)"
	},
	{
		variable: "--font-size-display",
		group: "typography",
		kind: "font-size",
		required: false,
		fallback: "var(--font-size-xl)"
	},

	// Typography — weights (stylesheet-only)
	{ variable: "--font-weight-thin", group: "typography", kind: "font-weight", required: true },
	{ variable: "--font-weight-regular", group: "typography", kind: "font-weight", required: true },
	{ variable: "--font-weight-bold", group: "typography", kind: "font-weight", required: true },
	{
		variable: "--font-weight-extrabold",
		group: "typography",
		kind: "font-weight",
		required: false,
		fallback: "var(--font-weight-bold)"
	},

	// Typography — line heights (stylesheet-only)
	{ variable: "--line-height-tight", group: "typography", kind: "line-height", required: true },
	{ variable: "--line-height-regular", group: "typography", kind: "line-height", required: true },
	{
		variable: "--line-height-relaxed",
		group: "typography",
		kind: "line-height",
		required: false,
		fallback: "var(--line-height-regular)"
	},

	// Typography — letter spacing (stylesheet-only)
	{
		variable: "--letter-spacing-tight",
		group: "typography",
		kind: "letter-spacing",
		required: true
	},
	{
		variable: "--letter-spacing-regular",
		group: "typography",
		kind: "letter-spacing",
		required: true
	},
	{
		variable: "--letter-spacing-wide",
		group: "typography",
		kind: "letter-spacing",
		required: false,
		fallback: "var(--letter-spacing-regular)"
	},

	// Colors — brand
	{
		variable: "--color-brand-accent",
		path: "colors.brand.accent",
		group: "colors",
		kind: "color",
		required: true
	},
	{
		variable: "--color-brand-primary",
		path: "colors.brand.primary",
		group: "colors",
		kind: "color",
		required: true
	},
	{
		variable: "--color-brand-secondary",
		path: "colors.brand.secondary",
		group: "colors",
		kind: "color",
		required: false,
		fallback: "var(--color-brand-primary)"
	},
	{
		variable: "--color-brand-tertiary",
		path: "colors.brand.tertiary",
		group: "colors",
		kind: "color",
		required: false,
		fallback: "var(--color-brand-primary)"
	},

	// Colors — action
	{
		variable: "--color-action-success",
		path: "colors.action.success",
		group: "colors",
		kind: "color",
		required: true
	},
	{
		variable: "--color-action-info",
		path: "colors.action.info",
		group: "colors",
		kind: "color",
		required: true
	},
	{
		variable: "--color-action-warning",
		path: "colors.action.warning",
		group: "colors",
		kind: "color",
		required: true
	},
	{
		variable: "--color-action-danger",
		path: "colors.action.danger",
		group: "colors",
		kind: "color",
		required: true
	},

	// Colors — text (`.base` drops the segment)
	{
		variable: "--color-text",
		path: "colors.text.base",
		group: "colors",
		kind: "color",
		required: true
	},
	{
		variable: "--color-text-accent",
		path: "colors.text.accent",
		group: "colors",
		kind: "color",
		required: false,
		fallback: "var(--color-text)"
	},
	{
		variable: "--color-text-muted",
		path: "colors.text.muted",
		group: "colors",
		kind: "color",
		required: false,
		fallback: "var(--color-text)"
	},
	{
		variable: "--color-text-ondark",
		path: "colors.text.ondark",
		group: "colors",
		kind: "color",
		required: false,
		fallback: "var(--color-surface)"
	},

	// Colors — surface
	{
		variable: "--color-surface",
		path: "colors.surface.base",
		group: "colors",
		kind: "color",
		required: true
	},
	{
		variable: "--color-surface-alt",
		path: "colors.surface.alt",
		group: "colors",
		kind: "color",
		required: true
	},
	{
		variable: "--color-surface-dark",
		path: "colors.surface.dark",
		group: "colors",
		kind: "color",
		required: false,
		fallback: "var(--color-text)"
	},
	{
		variable: "--color-surface-card",
		path: "colors.surface.card",
		group: "colors",
		kind: "color",
		required: false,
		fallback: "var(--color-surface)"
	},

	// Colors — resolved contrast pairs (stylesheet-only, auto-calculated)
	{
		variable: "--color-on-primary",
		group: "colors",
		kind: "color",
		required: false,
		fallback: "var(--color-text)"
	},
	{
		variable: "--color-on-accent",
		group: "colors",
		kind: "color",
		required: false,
		fallback: "var(--color-text)"
	},
	{
		variable: "--color-on-secondary",
		group: "colors",
		kind: "color",
		required: false,
		fallback: "var(--color-text)"
	},
	{
		variable: "--color-on-tertiary",
		group: "colors",
		kind: "color",
		required: false,
		fallback: "var(--color-text)"
	},
	{
		variable: "--color-on-success",
		group: "colors",
		kind: "color",
		required: false,
		fallback: "var(--color-text)"
	},
	{
		variable: "--color-on-info",
		group: "colors",
		kind: "color",
		required: false,
		fallback: "var(--color-text)"
	},
	{
		variable: "--color-on-warning",
		group: "colors",
		kind: "color",
		required: false,
		fallback: "var(--color-text)"
	},
	{
		variable: "--color-on-danger",
		group: "colors",
		kind: "color",
		required: false,
		fallback: "var(--color-text)"
	},
	{
		variable: "--color-on-dark",
		group: "colors",
		kind: "color",
		required: false,
		fallback: "var(--color-text)"
	},
	{
		variable: "--color-on-alt",
		group: "colors",
		kind: "color",
		required: false,
		fallback: "var(--color-text)"
	},
	{
		variable: "--color-on-card",
		group: "colors",
		kind: "color",
		required: false,
		fallback: "var(--color-text)"
	},

	// Spacing
	{ variable: "--space-xs", path: "spacing.xs", group: "spacing", kind: "space", required: true },
	{ variable: "--space-sm", path: "spacing.sm", group: "spacing", kind: "space", required: true },
	{ variable: "--space-md", path: "spacing.md", group: "spacing", kind: "space", required: true },
	{
		variable: "--space-base",
		path: "spacing.base",
		group: "spacing",
		kind: "space",
		required: true
	},
	{ variable: "--space-lg", path: "spacing.lg", group: "spacing", kind: "space", required: true },
	{ variable: "--space-xl", path: "spacing.xl", group: "spacing", kind: "space", required: true },
	{
		variable: "--space-xxl",
		path: "spacing.xxl",
		group: "spacing",
		kind: "space",
		required: true
	},

	// Rounded
	{
		variable: "--rounded-none",
		path: "rounded.none",
		group: "rounded",
		kind: "radius",
		required: true
	},
	{
		variable: "--rounded-sm",
		path: "rounded.sm",
		group: "rounded",
		kind: "radius",
		required: true
	},
	{
		variable: "--rounded-md",
		path: "rounded.md",
		group: "rounded",
		kind: "radius",
		required: true
	},
	{
		variable: "--rounded-lg",
		path: "rounded.lg",
		group: "rounded",
		kind: "radius",
		required: true
	},
	{
		variable: "--rounded-full",
		path: "rounded.full",
		group: "rounded",
		kind: "radius",
		required: true
	},

	// Elevation
	{
		variable: "--elevation-sm",
		path: "elevation.sm",
		group: "elevation",
		kind: "elevation",
		required: false,
		fallback: "none"
	},
	{
		variable: "--elevation-md",
		path: "elevation.md",
		group: "elevation",
		kind: "elevation",
		required: false,
		fallback: "none"
	},
	{
		variable: "--elevation-lg",
		path: "elevation.lg",
		group: "elevation",
		kind: "elevation",
		required: false,
		fallback: "none"
	},

	// Border widths
	{
		variable: "--border-sm",
		path: "border.sm",
		group: "border",
		kind: "border-width",
		required: true
	},
	{
		variable: "--border-md",
		path: "border.md",
		group: "border",
		kind: "border-width",
		required: true
	},
	{
		variable: "--border-lg",
		path: "border.lg",
		group: "border",
		kind: "border-width",
		required: true
	}
];

/** Human labels for the groups, in display order. */
export const GROUP_LABELS: { group: TokenGroup; label: string }[] = [
	{ group: "typography", label: "Typography" },
	{ group: "colors", label: "Colors" },
	{ group: "spacing", label: "Spacing" },
	{ group: "rounded", label: "Rounded" },
	{ group: "elevation", label: "Elevation" },
	{ group: "border", label: "Border" }
];

const byVariable = new Map<string, TokenDef>(TOKENS.map((token) => [token.variable, token]));
const byPath = new Map<string, TokenDef>();

for (const token of TOKENS) {
	if (token.path) {
		byPath.set(token.path, token);
	}
}

/**
 * Look up a token by its CSS variable.
 * @param variable - The `--variable` to look up.
 * @returns The matching token, or `undefined` when none is registered.
 */
export const tokenByVariable = (variable: string): TokenDef | undefined => byVariable.get(variable);

/**
 * Look up a token by its front-matter path.
 * @param path - The dotted front-matter path to look up.
 * @returns The matching token, or `undefined` when none is registered.
 */
export const tokenByPath = (path: string): TokenDef | undefined => byPath.get(path);

/**
 * Tokens with a front-matter path, in registry order.
 * @returns The path-bearing tokens, in registry order.
 */
export const frontMatterTokens = (): TokenDef[] => TOKENS.filter((token) => Boolean(token.path));

/**
 * Tokens grouped for display, in {@link GROUP_LABELS} order.
 * @returns One entry per group, each with its tokens, in display order.
 */
export const groupTokens = (): { group: TokenGroup; label: string; tokens: TokenDef[] }[] =>
	GROUP_LABELS.map(({ group, label }) => ({
		group,
		label,
		tokens: TOKENS.filter((token) => token.group === group)
	}));

/**
 * Paths the validator ignores: document metadata, components, and typography presets.
 * @param path - The front-matter path to test.
 * @returns True when the path carries no token and is exempt from validation.
 */
export const isIgnoredPath = (path: string): boolean =>
	path === "version" ||
	path === "name" ||
	path.startsWith("components") ||
	/^typography\.\w+\.(fontWeight|lineHeight|letterSpacing)$/.test(path);

/**
 * A fresh value map with documented fallbacks and empty required tokens.
 * @returns Every token variable mapped to its fallback, or `""` when none exists.
 */
export const createDefaultValues = (): TokenValues => {
	const values: TokenValues = {};

	for (const token of TOKENS) {
		values[token.variable] = token.fallback ?? "";
	}

	return values;
};
