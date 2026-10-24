/** Type-scale step names from the fixed token contract. */
export type TypeStepName = "xs" | "sm" | "md" | "lg" | "xl" | "2xl" | "display";

/** Step offsets from the `1rem` base (step 0, usually `md`). */
export const TYPE_STEP_OFFSETS: Record<TypeStepName, number> = {
	xs: -2,
	sm: -1,
	md: 0,
	lg: 1,
	xl: 2,
	"2xl": 3,
	display: 4
};

/** Format a `rem` value, trimming trailing zeros. */
const remString = (rem: number): string => `${Number(rem.toFixed(4))}rem`;

/**
 * Format a pixel value as a `rem` string, trimming trailing zeros.
 * @param px - The pixel value to convert.
 * @returns The equivalent `rem` string.
 */
export const formatRem = (px: number): string => remString(px / 16);

/**
 * Compute the type scale from a geometric ratio. The base step is always `1rem`, so
 * `md` is `1rem` and each other step is `ratio ** offset`.
 * @param ratio - The geometric ratio between adjacent steps.
 * @returns Each step name mapped to its `rem` value.
 */
export const typeScaleRem = (ratio: number): Record<TypeStepName, string> => {
	const scale = {} as Record<TypeStepName, string>;

	for (const [name, offset] of Object.entries(TYPE_STEP_OFFSETS) as [TypeStepName, number][]) {
		scale[name] = `${Number(ratio ** offset)}rem`;
	}

	return scale;
};

/** Spacing step names from the fixed token contract. */
export type SpaceStepName = "xs" | "sm" | "md" | "base" | "lg" | "xl" | "xxl";

/** Linear multipliers from the skill: xs=1, sm=2, md=3, base=4, lg=5, xl=8, xxl=12. */
export const SPACE_STEP_UNITS: Record<SpaceStepName, number> = {
	xs: 1,
	sm: 2,
	md: 3,
	base: 4,
	lg: 5,
	xl: 8,
	xxl: 12
};

/**
 * Step offsets from `base` for a geometric scale. The linear multipliers are not a
 * geometric progression, so a geometric scale spaces the steps evenly around `base`.
 */
export const SPACE_STEP_OFFSETS: Record<SpaceStepName, number> = {
	xs: -3,
	sm: -2,
	md: -1,
	base: 0,
	lg: 1,
	xl: 2,
	xxl: 3
};

/** Default linear unit in `rem`; four units make the `1rem` base. */
export const SPACE_UNIT_DEFAULT = 0.25;

/** Default geometric seed in `rem`; the seed occupies the `base` step. */
export const SPACE_SEED_DEFAULT = 1;

/** Default geometric multiplier: a wide enough spread for layout spacing. */
export const SPACE_MULTIPLIER_DEFAULT = 1.5;

/**
 * Compute the linear spacing scale from a unit in `rem`; `base` is four units.
 * @param unitRem - The linear unit in `rem`; defaults to {@link SPACE_UNIT_DEFAULT}.
 * @returns Each spacing step name mapped to its `rem` value.
 */
export const linearSpacingScaleRem = (
	unitRem = SPACE_UNIT_DEFAULT
): Record<SpaceStepName, string> => {
	const scale = {} as Record<SpaceStepName, string>;

	for (const [name, units] of Object.entries(SPACE_STEP_UNITS) as [SpaceStepName, number][]) {
		scale[name] = remString(unitRem * units);
	}

	return scale;
};

/**
 * Compute the geometric spacing scale from a seed and a multiplier. The seed lands on
 * `base`, so the other steps grow outward as `seed * multiplier ** offset`.
 * @param seedRem - The `rem` value at `base`; defaults to {@link SPACE_SEED_DEFAULT}.
 * @param multiplier - The ratio between steps; defaults to {@link SPACE_MULTIPLIER_DEFAULT}.
 * @returns Each spacing step name mapped to its `rem` value.
 */
export const geometricSpacingScaleRem = (
	seedRem = SPACE_SEED_DEFAULT,
	multiplier = SPACE_MULTIPLIER_DEFAULT
): Record<SpaceStepName, string> => {
	const scale = {} as Record<SpaceStepName, string>;

	for (const [name, offset] of Object.entries(SPACE_STEP_OFFSETS) as [SpaceStepName, number][]) {
		scale[name] = remString(seedRem * multiplier ** offset);
	}

	return scale;
};
