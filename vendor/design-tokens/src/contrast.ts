/** A WCAG conformance level for a contrast ratio. */
export type ContrastLevel = "AAA" | "AA" | "AA Large" | "Fail";

const toLinear = (channel: number): number =>
	channel <= 0.03928 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4;

const channel = (hex: string, start: number): number => {
	const parsed = Number.parseInt(hex.slice(start, start + 2), 16);

	return Number.isNaN(parsed) ? 0 : parsed / 255;
};

/**
 * Relative luminance of a `#rgb` or `#rrggbb` color. Returns 0 for invalid input.
 * @param hex - The color string to measure.
 * @returns The relative luminance, or 0 when the color is invalid.
 */
export const relativeLuminance = (hex: string): number => {
	const value = hex.trim().replace("#", "");
	const full =
		value.length === 3
			? value
					.split("")
					.map((c) => c + c)
					.join("")
			: value;

	if (full.length !== 6) {
		return 0;
	}

	return (
		0.2126 * toLinear(channel(full, 0)) +
		0.7152 * toLinear(channel(full, 2)) +
		0.0722 * toLinear(channel(full, 4))
	);
};

/**
 * WCAG contrast ratio between two colors, rounded to two decimals.
 * @param a - The first color string.
 * @param b - The second color string.
 * @returns The contrast ratio, rounded to two decimals.
 */
export const contrastRatio = (a: string, b: string): number => {
	const la = relativeLuminance(a);
	const lb = relativeLuminance(b);
	const ratio = (Math.max(la, lb) + 0.05) / (Math.min(la, lb) + 0.05);

	return Math.round(ratio * 100) / 100;
};

/**
 * Map a contrast ratio to its WCAG level.
 * @param ratio - The contrast ratio to classify.
 * @returns The highest WCAG level the ratio satisfies.
 */
export const contrastLevel = (ratio: number): ContrastLevel => {
	if (ratio >= 7) {
		return "AAA";
	}

	if (ratio >= 4.5) {
		return "AA";
	}

	if (ratio >= 3) {
		return "AA Large";
	}

	return "Fail";
};

/**
 * True when a color is light enough that dark text should sit on top of it.
 * @param hex - The color string to test.
 * @returns True when dark text should sit on top of the color.
 */
export const isLight = (hex: string): boolean => relativeLuminance(hex) > 0.35;
