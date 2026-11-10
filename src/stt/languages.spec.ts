import { describe, expect, test } from "bun:test";
import { LANGUAGE_OPTIONS, languageIsForwarded, languageProviderOptions } from "./languages";

/** Languages every supported provider publishes support for. */
const CORE_LABELS = [
	"Chinese",
	"Dutch",
	"English",
	"French",
	"German",
	"Italian",
	"Japanese",
	"Korean",
	"Polish",
	"Portuguese",
	"Russian",
	"Spanish"
];

function byLabel(labels: readonly string[]) {
	return [...labels].sort((left, right) => (left < right ? -1 : left > right ? 1 : 0));
}

describe("LANGUAGE_OPTIONS", () => {
	test("uses unique two-letter lowercase ISO-639-1 codes", () => {
		const codes = LANGUAGE_OPTIONS.map((option) => option.code);

		expect(new Set(codes).size).toBe(codes.length);
		for (const code of codes) {
			expect(code).toMatch(/^[a-z]{2}$/);
		}
	});

	test("gives every language a unique, non-empty English label", () => {
		const labels = LANGUAGE_OPTIONS.map((option) => option.label);

		expect(new Set(labels).size).toBe(labels.length);
		for (const label of labels) {
			expect(label.trim().length).toBeGreaterThan(0);
		}
	});

	test("contains the core languages every provider supports", () => {
		const labels = LANGUAGE_OPTIONS.map((option) => option.label);

		for (const label of CORE_LABELS) {
			expect(labels).toContain(label);
		}
	});

	test("offers no Auto entry: an empty setting is represented by empty", () => {
		for (const option of LANGUAGE_OPTIONS) {
			expect(option.code.length).toBe(2);
			expect(option.label.toLowerCase()).not.toBe("auto");
		}
	});

	test("is sorted by label", () => {
		const labels = LANGUAGE_OPTIONS.map((option) => option.label);

		expect(labels).toEqual(byLabel(labels));
	});
});

describe("languageProviderOptions", () => {
	test("returns undefined when no language is set", () => {
		expect(languageProviderOptions("vercel-ai-gateway", undefined)).toBeUndefined();
		expect(languageProviderOptions("openai", undefined)).toBeUndefined();
	});

	test("treats a blank language as unset", () => {
		expect(languageProviderOptions("vercel-ai-gateway", "")).toBeUndefined();
		expect(languageProviderOptions("vercel-ai-gateway", "   ")).toBeUndefined();
	});

	test("never targets the gateway package for the gateway and its aliases", () => {
		// `GatewayProviderOptions` documents routing and accounting fields only;
		// an unknown key makes the gateway reject the request, which surfaced as
		// a `stream_failed` take. The language is not sent at all.
		for (const provider of ["vercel-ai-gateway", "vercel", "gateway"]) {
			expect(languageProviderOptions(provider, "fr")).toBeUndefined();
		}
	});

	test("never targets a package it does not know", () => {
		expect(languageProviderOptions("some-future-provider", "fr")).toBeUndefined();
	});

	test("reports whether the language can reach the model", () => {
		expect(languageIsForwarded("openai")).toBe(true);
		expect(languageIsForwarded("xai")).toBe(true);
		expect(languageIsForwarded("vercel-ai-gateway")).toBe(false);
		expect(languageIsForwarded("some-future-provider")).toBe(false);
	});

	test("names the provider package for a direct provider", () => {
		expect(languageProviderOptions("openai", "fr")).toEqual({
			openai: { language: "fr" }
		});
		expect(languageProviderOptions("mistral", "fr")).toEqual({
			mistral: { language: "fr" }
		});
		expect(languageProviderOptions("xai", "fr")).toEqual({
			xai: { language: "fr" }
		});
		expect(languageProviderOptions("elevenlabs", "fr")).toEqual({
			elevenlabs: { language: "fr" }
		});
	});

	test("normalizes the code to a trimmed lowercase value", () => {
		expect(languageProviderOptions("openai", " FR ")).toEqual({
			openai: { language: "fr" }
		});
	});
});
