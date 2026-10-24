/**
 * WCAG 2.1 color contrast helpers. The theme front matter stores role and text colors
 * as six-digit hex values, so this module parses hex only and rejects anything else.
 */

/** An sRGB color with 0-255 channels. */
export interface Rgb {
	r: number;
	g: number;
	b: number;
}

const HEX = /^#([0-9a-f]{3}|[0-9a-f]{6})$/i;

/**
 * Parse `#rgb` or `#rrggbb` into channels. Return null for any other format.
 * @param value - The color string to parse.
 * @returns The parsed channels, or `null` when the input is not hex.
 */
export const parseHex = (value: string): Rgb | null => {
	const match = value.trim().match(HEX);

	if (!match?.[1]) {
		return null;
	}

	const hex = match[1].length === 3 ? match[1].replace(/./g, (char) => char + char) : match[1];

	return {
		r: Number.parseInt(hex.slice(0, 2), 16),
		g: Number.parseInt(hex.slice(2, 4), 16),
		b: Number.parseInt(hex.slice(4, 6), 16)
	};
};

/** Linearize one sRGB channel, as WCAG relative luminance requires. */
const linearize = (value: number): number => {
	const channel = value / 255;

	return channel <= 0.03928 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4;
};

/**
 * WCAG 2.1 relative luminance of an sRGB color, from 0 (black) to 1 (white).
 * @param color - The sRGB color to measure.
 * @returns The relative luminance, from 0 (black) to 1 (white).
 */
export const relativeLuminance = ({ r, g, b }: Rgb): number =>
	0.2126 * linearize(r) + 0.7152 * linearize(g) + 0.0722 * linearize(b);

/**
 * WCAG 2.1 contrast ratio between two colors, from 1 to 21.
 * @param a - The first sRGB color.
 * @param b - The second sRGB color.
 * @returns The contrast ratio, from 1 to 21.
 */
export const contrastRatio = (a: Rgb, b: Rgb): number => {
	const luminanceA = relativeLuminance(a);
	const luminanceB = relativeLuminance(b);
	const lighter = Math.max(luminanceA, luminanceB);
	const darker = Math.min(luminanceA, luminanceB);

	return (lighter + 0.05) / (darker + 0.05);
};
