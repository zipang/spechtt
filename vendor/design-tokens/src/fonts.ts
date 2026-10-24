/** A font family entry in the picker catalog. */
export interface FontEntry {
	family: string;
	category: "serif" | "sans-serif" | "display" | "monospace" | "handwriting";
}

/** Category filter for the picker. `all` shows every family. */
export type FontCategory = "all" | FontEntry["category"];

/** Provider tab in the picker. Adobe is shown but disabled. */
export type FontProvider = "google" | "system" | "adobe";

const entry = (family: string, category: FontEntry["category"]): FontEntry => ({
	family,
	category
});

/** Google Fonts catalog offered by the picker. */
export const GOOGLE_FONTS: FontEntry[] = [
	entry("DM Serif Display", "serif"),
	entry("DM Serif Text", "serif"),
	entry("Playfair Display", "serif"),
	entry("Playfair Display SC", "serif"),
	entry("Merriweather", "serif"),
	entry("Lora", "serif"),
	entry("PT Serif", "serif"),
	entry("Crimson Text", "serif"),
	entry("Crimson Pro", "serif"),
	entry("EB Garamond", "serif"),
	entry("Libre Baskerville", "serif"),
	entry("Cormorant Garamond", "serif"),
	entry("Cormorant", "serif"),
	entry("Vollkorn", "serif"),
	entry("Alegreya", "serif"),
	entry("Spectral", "serif"),
	entry("Fraunces", "serif"),
	entry("Bodoni Moda", "serif"),
	entry("Cardo", "serif"),
	entry("Domine", "serif"),
	entry("Frank Ruhl Libre", "serif"),
	entry("Literata", "serif"),
	entry("Source Serif 4", "serif"),
	entry("Noto Serif", "serif"),
	entry("Arvo", "serif"),
	entry("Bitter", "serif"),
	entry("Rokkitt", "serif"),
	entry("Zilla Slab", "serif"),
	entry("Josefin Slab", "serif"),
	entry("Libre Caslon Text", "serif"),
	entry("Unna", "serif"),
	entry("Cinzel", "serif"),
	entry("Vidaloka", "serif"),
	entry("Neuton", "serif"),
	entry("Tinos", "serif"),
	entry("Plus Jakarta Sans", "sans-serif"),
	entry("Inter", "sans-serif"),
	entry("Roboto", "sans-serif"),
	entry("Roboto Condensed", "sans-serif"),
	entry("Open Sans", "sans-serif"),
	entry("Lato", "sans-serif"),
	entry("Montserrat", "sans-serif"),
	entry("Nunito", "sans-serif"),
	entry("Nunito Sans", "sans-serif"),
	entry("Raleway", "sans-serif"),
	entry("Poppins", "sans-serif"),
	entry("Outfit", "sans-serif"),
	entry("DM Sans", "sans-serif"),
	entry("Figtree", "sans-serif"),
	entry("Sora", "sans-serif"),
	entry("Manrope", "sans-serif"),
	entry("Work Sans", "sans-serif"),
	entry("Source Sans 3", "sans-serif"),
	entry("IBM Plex Sans", "sans-serif"),
	entry("Jost", "sans-serif"),
	entry("Urbanist", "sans-serif"),
	entry("Rubik", "sans-serif"),
	entry("Karla", "sans-serif"),
	entry("Mulish", "sans-serif"),
	entry("Quicksand", "sans-serif"),
	entry("Cabin", "sans-serif"),
	entry("Barlow", "sans-serif"),
	entry("Barlow Condensed", "sans-serif"),
	entry("Noto Sans", "sans-serif"),
	entry("Ubuntu", "sans-serif"),
	entry("Exo 2", "sans-serif"),
	entry("Fira Sans", "sans-serif"),
	entry("Assistant", "sans-serif"),
	entry("Heebo", "sans-serif"),
	entry("Titillium Web", "sans-serif"),
	entry("Josefin Sans", "sans-serif"),
	entry("Varela Round", "sans-serif"),
	entry("Maven Pro", "sans-serif"),
	entry("Be Vietnam Pro", "sans-serif"),
	entry("Lexend", "sans-serif"),
	entry("Lexend Deca", "sans-serif"),
	entry("Albert Sans", "sans-serif"),
	entry("Onest", "sans-serif"),
	entry("Bricolage Grotesque", "sans-serif"),
	entry("Hanken Grotesk", "sans-serif"),
	entry("Schibsted Grotesk", "sans-serif"),
	entry("Instrument Sans", "sans-serif"),
	entry("Geist", "sans-serif"),
	entry("Bebas Neue", "display"),
	entry("Anton", "display"),
	entry("Oswald", "display"),
	entry("Righteous", "display"),
	entry("Bungee", "display"),
	entry("Alfa Slab One", "display"),
	entry("Abril Fatface", "display"),
	entry("Black Ops One", "display"),
	entry("Teko", "display"),
	entry("Russo One", "display"),
	entry("Squada One", "display"),
	entry("Big Shoulders Display", "display"),
	entry("Big Shoulders Text", "display"),
	entry("Syne", "display"),
	entry("Instrument Serif", "display"),
	entry("Cormorant SC", "display"),
	entry("Yeseva One", "display"),
	entry("Ultra", "display"),
	entry("Boogaloo", "display"),
	entry("JetBrains Mono", "monospace"),
	entry("Fira Code", "monospace"),
	entry("Source Code Pro", "monospace"),
	entry("IBM Plex Mono", "monospace"),
	entry("Space Mono", "monospace"),
	entry("DM Mono", "monospace"),
	entry("Roboto Mono", "monospace"),
	entry("Cousine", "monospace"),
	entry("Inconsolata", "monospace"),
	entry("Noto Sans Mono", "monospace"),
	entry("Ubuntu Mono", "monospace"),
	entry("Anonymous Pro", "monospace"),
	entry("Overpass Mono", "monospace"),
	entry("Share Tech Mono", "monospace"),
	entry("Chivo Mono", "monospace"),
	entry("Azeret Mono", "monospace"),
	entry("Dancing Script", "handwriting"),
	entry("Pacifico", "handwriting"),
	entry("Caveat", "handwriting"),
	entry("Satisfy", "handwriting"),
	entry("Kalam", "handwriting"),
	entry("Patrick Hand", "handwriting"),
	entry("Permanent Marker", "handwriting"),
	entry("Shadows Into Light", "handwriting"),
	entry("Amatic SC", "handwriting"),
	entry("Indie Flower", "handwriting"),
	entry("Gloria Hallelujah", "handwriting"),
	entry("Architects Daughter", "handwriting"),
	entry("Yellowtail", "handwriting"),
	entry("Bad Script", "handwriting")
];

/** System fonts offered by the picker. */
export const SYSTEM_FONTS: FontEntry[] = [
	entry("Arial", "sans-serif"),
	entry("Helvetica Neue", "sans-serif"),
	entry("Verdana", "sans-serif"),
	entry("Trebuchet MS", "sans-serif"),
	entry("Georgia", "serif"),
	entry("Times New Roman", "serif"),
	entry("Palatino", "serif"),
	entry("Garamond", "serif"),
	entry("Courier New", "monospace"),
	entry("Lucida Console", "monospace"),
	entry("Impact", "display"),
	entry("Comic Sans MS", "handwriting")
];

/** Category filter options, in display order. */
export const FONT_CATEGORIES: { key: FontCategory; label: string }[] = [
	{ key: "all", label: "All" },
	{ key: "serif", label: "Serif" },
	{ key: "sans-serif", label: "Sans" },
	{ key: "display", label: "Display" },
	{ key: "monospace", label: "Mono" },
	{ key: "handwriting", label: "Script" }
];

/** Short badge label for a category. */
export const CATEGORY_BADGE: Record<FontEntry["category"], string> = {
	serif: "serif",
	"sans-serif": "sans",
	display: "display",
	monospace: "mono",
	handwriting: "script"
};

/** Generic CSS font-family keywords that never name a loadable web font. */
const GENERIC_FAMILIES = new Set([
	"serif",
	"sans-serif",
	"monospace",
	"cursive",
	"fantasy",
	"system-ui",
	"ui-serif",
	"ui-sans-serif",
	"ui-monospace",
	"ui-rounded",
	"math",
	"emoji",
	"fangsong",
	"-apple-system",
	"blinkmacsystemfont"
]);

const googleFontNames = new Map(
	GOOGLE_FONTS.map((font) => [font.family.toLowerCase(), font.family])
);

/**
 * The families named in a CSS font stack, in order, with quotes and generic keywords
 * removed. Unresolved `var()` fragments are dropped, so resolve the value first.
 * @param stack - The CSS font stack to split.
 * @returns The non-generic family names, in stack order.
 */
export const fontStackFamilies = (stack: string): string[] =>
	stack
		.split(",")
		.map((part) =>
			part
				.trim()
				.replace(/^["']|["']$/g, "")
				.trim()
		)
		.filter(
			(family) =>
				family.length > 0 &&
				!family.includes("var(") &&
				!GENERIC_FAMILIES.has(family.toLowerCase())
		);

/**
 * The canonical Google Font name for `family`, or undefined when it is not in the catalog.
 * @param family - The family name to look up, matched case-insensitively.
 * @returns The catalog spelling of the family, or `undefined` when unknown.
 */
export const googleFontName = (family: string): string | undefined =>
	googleFontNames.get(family.trim().toLowerCase());

/**
 * Build the Google Fonts `@import` for the catalog families among `families`, loading
 * the given weights. Returns `""` when none is a Google Font, so system-only themes stay
 * import-free. Google serves a family's available faces for any requested weight set, so
 * a shared list is safe even for single-weight display faces.
 * @param families - The family names to import; unknown families are skipped.
 * @param weights - The requested weights, filtered to the 100-900 range.
 * @returns The `@import` rule, or `""` when no family is hosted.
 */
export const googleFontImport = (families: string[], weights: number[] = [400, 700]): string => {
	const canonical = [
		...new Set(
			families
				.map((family) => googleFontName(family))
				.filter((family): family is string => family !== undefined)
		)
	];

	if (canonical.length === 0) {
		return "";
	}
	const axis = [...new Set(weights)]
		.filter((weight) => Number.isInteger(weight) && weight >= 100 && weight <= 900)
		.sort((a, b) => a - b);
	const suffix = axis.length > 0 ? `:wght@${axis.join(";")}` : "";
	const query = canonical
		.map((family) => `family=${family.replace(/\s+/g, "+")}${suffix}`)
		.join("&");

	return `@import url("https://fonts.googleapis.com/css2?${query}&display=swap");`;
};
