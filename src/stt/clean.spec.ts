import { describe, expect, test } from "bun:test";
import { CLEANUP_TEMPERATURE, type CleanDeps, type CleanRequest, cleanTranscript } from "./clean";

const TRANSCRIPT = "euh bonjour, je d 'une idée très claire";

/** Records every `generate` call and answers with a fixed text. */
function recorder(answer: string) {
	const calls: Array<{ model: string; prompt: string; temperature: number }> = [];
	const deps: CleanDeps = {
		async generate(args) {
			calls.push(args);
			return { text: answer };
		}
	};
	return { calls, deps };
}

function request(overrides: Partial<CleanRequest> = {}): CleanRequest {
	return {
		provider: "vercel-ai-gateway",
		model: "openai/gpt-4.1-mini",
		text: TRANSCRIPT,
		...overrides
	};
}

async function promptFor(overrides: Partial<CleanRequest> = {}): Promise<string> {
	const { calls, deps } = recorder("peu importe");
	await cleanTranscript(request(overrides), deps);
	return calls[0]!.prompt;
}

describe("cleanTranscript", () => {
	test("calls generate with the model and the documented temperature", async () => {
		const { calls, deps } = recorder("Bonjour.");

		await cleanTranscript(request({ model: "openai/gpt-4.1-mini" }), deps);

		expect(calls).toHaveLength(1);
		expect(calls[0]?.model).toBe("openai/gpt-4.1-mini");
		expect(calls[0]?.temperature).toBe(CLEANUP_TEMPERATURE);
		expect(CLEANUP_TEMPERATURE).toBeLessThan(0.5);
	});

	test("returns the model answer trimmed", async () => {
		const { deps } = recorder("  Bonjour, je fais une idée très claire.\n\n");

		await expect(cleanTranscript(request(), deps)).resolves.toBe(
			"Bonjour, je fais une idée très claire."
		);
	});

	test("returns the input unchanged when the model answers nothing", async () => {
		for (const answer of ["", "   ", "\n\n"]) {
			const { deps } = recorder(answer);

			await expect(
				cleanTranscript(request({ text: `  ${TRANSCRIPT}  ` }), deps)
			).resolves.toBe(TRANSCRIPT);
		}
	});

	test("is idempotent for clean input", async () => {
		const clean = "Bonjour, je fais une idée très claire.";
		const { deps } = recorder(clean);

		await expect(cleanTranscript(request({ text: clean }), deps)).resolves.toBe(clean);
	});

	test("passes the transcript verbatim, exactly once", async () => {
		const prompt = await promptFor();

		expect(prompt.split(TRANSCRIPT)).toHaveLength(2);
	});

	test("names the four defect categories", async () => {
		const prompt = await promptFor();
		const lowered = prompt.toLowerCase();

		expect(lowered).toContain("transcription artefacts");
		expect(lowered).toContain("fragment joins");
		expect(lowered).toContain("hesitations");
		expect(lowered).toContain("repetitions");
	});

	test("illustrates glued words and stray spaces around an apostrophe", async () => {
		const prompt = await promptFor();

		expect(prompt).toContain("d 'une");
		expect(prompt).toContain("d'une");
		expect(prompt).toContain("dict ée");
	});

	test("states the fidelity rules", async () => {
		const prompt = await promptFor();
		const lowered = prompt.toLowerCase();

		expect(lowered).toContain("never add content");
		expect(lowered).toContain("never summarise");
		expect(lowered).toContain("translate");
		expect(lowered).toContain("style");
		expect(lowered).toContain("keep the original language");
		expect(lowered).toContain("already clean");
	});

	test("forbids a preamble, quotes and markdown in the answer", async () => {
		const prompt = await promptFor();
		const lowered = prompt.toLowerCase();

		expect(lowered).toContain("no preamble");
		expect(lowered).toContain("no quotes");
		expect(lowered).toContain("no markdown");
	});

	test("names the declared language and its typography", async () => {
		const prompt = await promptFor({ language: "fr" });

		expect(prompt).toContain("French");
		expect(prompt).toContain("fr");
		expect(prompt).toContain("«");
		expect(prompt).toContain("»");
		expect(prompt).toContain("’");
		expect(prompt.toLowerCase()).toContain("never translate");
	});

	test("omits the language clause when no language is declared", async () => {
		const prompt = await promptFor();

		expect(prompt).not.toContain("French");
		expect(prompt).not.toContain("The transcript is in");
		expect(prompt.toLowerCase()).not.toContain("typography");
		expect(prompt).not.toContain("«");
	});

	test("falls back to the raw code for a language outside the list", async () => {
		const prompt = await promptFor({ language: "xx" });

		expect(prompt).toContain("xx");
		expect(prompt.toLowerCase()).toContain("never translate");
	});
});
