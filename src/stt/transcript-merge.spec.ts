import { describe, expect, test } from "bun:test";
import { mergeTranscript } from "./transcript-merge";

describe("mergeTranscript", () => {
	test("joins plain segments with a single space", () => {
		expect(mergeTranscript("hello world", "  good morning ")).toBe(
			"hello world good morning",
		);
		expect(mergeTranscript("", "  hello ")).toBe("hello");
		expect(mergeTranscript("hello", "   ")).toBe("hello");
	});

	test("removes a two-word overlap", () => {
		expect(mergeTranscript("alpha beta gamma", "beta gamma delta")).toBe(
			"alpha beta gamma delta",
		);
		expect(mergeTranscript("the end of it", "of it all")).toBe(
			"the end of it all",
		);
	});

	test("removes a four-word overlap", () => {
		expect(mergeTranscript("one two three four five", "two three four five six")).toBe(
			"one two three four five six",
		);
	});

	test("caps the overlap at four words", () => {
		expect(mergeTranscript("a b c d e", "b c d e f")).toBe("a b c d e f");
	});

	test("keeps a single-word overlap unless explicitly allowed", () => {
		expect(mergeTranscript("we saw the", "the end")).toBe(
			"we saw the the end",
		);
		expect(
			mergeTranscript("we saw the", "the end", {
				allowSingleWordOverlap: true,
			}),
		).toBe("we saw the end");
	});

	test("preserves legitimate repetitions", () => {
		expect(mergeTranscript("it was very", "very good")).toBe(
			"it was very very good",
		);
	});

	test("matches case and punctuation but emits the original spelling", () => {
		expect(
			mergeTranscript("Hello, world.", "World again", {
				allowSingleWordOverlap: true,
			}),
		).toBe("Hello, world. again");
		expect(
			mergeTranscript("fin de phrase.", "Phrase suivante", {
				allowSingleWordOverlap: true,
			}),
		).toBe("fin de phrase. suivante");
	});

	test("returns the previous text when the next one is contained", () => {
		expect(mergeTranscript("one two three", "two three")).toBe(
			"one two three",
		);
	});

	test("never matches punctuation-only tokens", () => {
		expect(
			mergeTranscript("wait ...", "... go on", {
				allowSingleWordOverlap: true,
			}),
		).toBe("wait ... ... go on");
	});

	test("is deterministic for the same inputs", () => {
		const first = mergeTranscript("a b c", "b c d", { maxOverlapWords: 2 });
		const second = mergeTranscript("a b c", "b c d", { maxOverlapWords: 2 });
		expect(first).toBe(second);
		expect(first).toBe("a b c d");
	});
});
