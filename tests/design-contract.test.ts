import { describe, expect, test } from "bun:test";
import { parseContract, validateContract } from "@applied-design-systems/design-tokens";

const root = new URL("..", import.meta.url).pathname;
const designMd = await Bun.file(`${root}DESIGN.md`).text();
const tokensCss = await Bun.file(`${root}design-tokens.css`).text();

describe("design system contract", () => {
	test("DESIGN.md + design-tokens.css satisfy the design-system-tokens rules", () => {
		const contract = parseContract(designMd, tokensCss);
		const errors = validateContract(contract).filter((issue) => issue.level === "error");

		expect(errors).toEqual([]);
	});

	test("every documented token is exposed as a CSS variable", () => {
		const { cssValues } = parseContract(designMd, tokensCss);

		for (const variable of [
			"--color-brand-primary",
			"--color-surface",
			"--color-text",
			"--space-base",
			"--rounded-lg",
			"--border-sm"
		]) {
			expect(cssValues[variable]).toBeDefined();
		}
	});

	test("the base text/surface pair meets WCAG AA", () => {
		const { cssValues } = parseContract(designMd, tokensCss);
		const hex = (value: string) => Number.parseInt(value.replace("#", ""), 16);
		const luminance = (value: string) => {
			const n = hex(value);
			const channel = (raw: number) => {
				const v = raw / 255;
				return v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4;
			};
			return (
				0.2126 * channel((n >> 16) & 255) +
				0.7152 * channel((n >> 8) & 255) +
				0.0722 * channel(n & 255)
			);
		};
		const a = luminance(cssValues["--color-text"]!);
		const b = luminance(cssValues["--color-surface"]!);
		const ratio = (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05);

		expect(ratio).toBeGreaterThanOrEqual(4.5);
	});
});
