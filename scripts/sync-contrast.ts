/**
 * Recompute the derived `--color-on-*` contrast tokens in design-tokens.css
 * from the authored theme colors, using the WCAG 2.1 derivation rule.
 *
 * Run: bun run design:contrast
 */
import {
	parseTokensCss,
	serializeTokensCss,
	updateThemeContrastTokens
} from "@applied-design-systems/design-tokens";

const root = new URL("..", import.meta.url).pathname;
const cssPath = `${root}design-tokens.css`;
const tokensCss = await Bun.file(cssPath).text();

const values = parseTokensCss(tokensCss);
const next = updateThemeContrastTokens(values);

const changed = Object.keys(next).filter((variable) => next[variable] !== values[variable]);

if (changed.length === 0) {
	console.log("✓ Contrast tokens already up to date.");
	process.exit(0);
}

console.log("Updated:");
for (const variable of changed) {
	console.log(`  ${variable}: ${values[variable]} → ${next[variable]}`);
}
console.log(
	"\nRewrite design-tokens.css by hand with these values (the serializer drops comments)."
);
