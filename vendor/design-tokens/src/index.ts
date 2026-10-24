/**
 * Public API of the Applied Design Systems token contract.
 *
 * This entry is pure: it imports no React, no DOM, and no filesystem. Consumers parse,
 * serialize, validate, and derive contrast for a `DESIGN.md` + `design-tokens.css` pair
 * with it. Every module here is also the single source of truth the Theme Editor uses.
 */

// Contrast: the Rgb API is the validator's; the hex helpers are a display convenience.
export { contrastRatio, parseHex, type Rgb, relativeLuminance } from "./color-contrast";
// Contract: parse and serialize both halves of the dual-file contract.
export {
	type Contract,
	isKnownVariable,
	mergeValues,
	normalizeValue,
	type ParsedDesignMd,
	parseContract,
	parseDesignMd,
	parseTokensCss,
	resolveVariables,
	scopeTokensCss,
	serializeDesignMd,
	serializeTokensCss,
	stripCssComments,
	type TypographyField,
	type TypographyRefs
} from "./contract";
export {
	type ContrastLevel,
	contrastLevel,
	contrastRatio as hexContrastRatio,
	isLight,
	relativeLuminance as hexRelativeLuminance
} from "./contrast";
export {
	AA_NORMAL,
	CONTRAST_TOKENS,
	type TokenList,
	updateThemeContrastTokens
} from "./contrast-tokens";
// Token registry: the fixed list and its path ↔ CSS-variable mapping.
export {
	createDefaultValues,
	frontMatterTokens,
	GROUP_LABELS,
	groupTokens,
	isIgnoredPath,
	TOKENS,
	type TokenDef,
	type TokenGroup,
	type TokenKind,
	type TokenValues,
	tokenByPath,
	tokenByVariable
} from "./design-system";
// Fonts: the catalog the picker, the parser, and the validator read.
export {
	CATEGORY_BADGE,
	FONT_CATEGORIES,
	type FontCategory,
	type FontEntry,
	type FontProvider,
	fontStackFamilies,
	GOOGLE_FONTS,
	googleFontImport,
	googleFontName,
	SYSTEM_FONTS
} from "./fonts";
// Type scale and spacing math over the token contract.
export {
	formatRem,
	geometricSpacingScaleRem,
	linearSpacingScaleRem,
	SPACE_MULTIPLIER_DEFAULT,
	SPACE_SEED_DEFAULT,
	SPACE_STEP_OFFSETS,
	SPACE_STEP_UNITS,
	SPACE_UNIT_DEFAULT,
	type SpaceStepName,
	TYPE_STEP_OFFSETS,
	type TypeStepName,
	typeScaleRem
} from "./scale";
// Validation: the section 10 rules of the design-system-tokens skill.
export { type Issue, type ValidationInput, validateContract } from "./validate";
