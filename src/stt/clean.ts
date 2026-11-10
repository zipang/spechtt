/**
 * Language-model cleanup of a finished transcript.
 *
 * The prompt asks a text model to repair the artefacts a streaming transcriber leaves behind
 * without rewriting what the speaker said. `worker.ts` calls [`cleanTranscript`] for a
 * `cleanTranscript` command and emits the result as a `cleaned` event; the model call itself is
 * injected so the worker owns credentials and tests stay hermetic.
 */

import { languageName } from "./languages";

export { FALLBACK_LANGUAGE_MODEL, LANGUAGE_MODEL_ENV } from "./registry";

/** Low enough that the pass repairs rather than improvises. */
export const CLEANUP_TEMPERATURE = 0.1;

/**
 * Fence around the transcript.
 *
 * A line of angle brackets and capitals is not something dictation produces, so the model cannot
 * mistake the transcript for the instructions that precede it.
 */
const TRANSCRIPT_START = "<<<TRANSCRIPT>>>";
const TRANSCRIPT_END = "<<<END TRANSCRIPT>>>";

const INSTRUCTIONS = `Repair a dictated transcript. The text between the markers below is the transcript exactly as it was transcribed; correct only the defects listed here and answer with the corrected text.

Defects to repair:
1. Transcription artefacts — words glued together or split apart, stray spaces inside a word or around an apostrophe or hyphen (\`d 'une\` → \`d'une\`, \`dict ée\` → \`dictée\`), letter sequences that spell a word the speaker clearly said (\`Trans cribe to Stream\` → \`Transcribe to Streaming\`), doubled punctuation, and missing capitalisation at the start of a sentence.
2. Fragment joins — a live transcriber commits windows that overlap, so a phrase can appear twice end to end or be cut mid-sentence. Produce one continuous, correctly punctuated text.
3. Hesitations — filler and false starts the speaker did not mean to keep (\`euh\`, \`bah\`, \`ben\`, \`voilà\`, \`en fait\` used as padding), a word restarted two or three times, an abandoned sentence fragment. Remove them; do not replace them with other words.
4. The speaker's own repetitions — a word or short phrase repeated because the speaker stumbled (\`beaucoup, beaucoup de doublons\`). Keep a repetition that carries meaning (emphasis, a genuine list, a quoted repetition) and drop the one that does not.

Fidelity rules — they bound the four categories:
- Keep the speaker's words, order and register. Do not rewrite for style, do not merge or split sentences to improve them, do not harmonise the tone.
- Correct a sentence's grammar only where it is broken as transcribed (mis-glued words, a dropped article at a window boundary), not where it is merely informal. Spoken language stays spoken.
- Never add content: no new facts, no transitions, no connective paragraph opening, no closing sentence. Never summarise, never expand, never translate.
- Keep the original language.
- When a passage is already clean, leave it untouched: a clean transcript must come back identical.
- Answer with the corrected text only: no preamble, no explanation, no quotes, no markdown, no code fence.`;

export interface CleanRequest {
	/** Gateway provider id, the same one the transcription used. */
	readonly provider: string;
	/** Language model id, e.g. `openai/gpt-4.1-mini`. */
	readonly model: string;
	/** The transcript to repair, exactly as displayed. */
	readonly text: string;
	/**
	 * ISO-639-1 language of the transcript. Absent means "follow the transcript's own language":
	 * the prompt then omits the language clause.
	 */
	readonly language?: string;
	/**
	 * Provider key resolved by the caller. The injected `generate` decides how to authenticate, so
	 * this module never sends it anywhere itself.
	 */
	readonly apiKey?: string;
}

export interface CleanDeps {
	/** Injected for tests; production passes the AI SDK `generateText`. */
	generate(args: {
		model: string;
		prompt: string;
		temperature: number;
	}): Promise<{ text: string }>;
}

/**
 * The clause that pins the transcript's language, or an empty string when no language is declared.
 *
 * With no language the model follows the transcript instead of guessing one from nothing, so the
 * instruction is omitted rather than reworded.
 */
function languageClause(language: string | undefined): string {
	const code = language?.trim().toLowerCase();
	if (code === undefined || code.length === 0) {
		return "";
	}
	const name = languageName(code) ?? code;
	return `\nThe transcript is in ${name} (${code}). Keep that language, use its typography — a non-breaking space before \`:\` \`;\` \`!\` \`?\`, no space inside « », apostrophes as ’ — and never translate.\n`;
}

/** Builds the one prompt the cleanup pass sends. */
function buildPrompt(text: string, language: string | undefined): string {
	return `${INSTRUCTIONS}\n${languageClause(language)}\n${TRANSCRIPT_START}\n${text}\n${TRANSCRIPT_END}\n`;
}

/**
 * Repairs typos and fragment joins. Never returns an empty string: when the model answers with
 * nothing usable, the input is returned unchanged.
 *
 * The caller decides what to do with the text; this function performs no I/O beyond the injected
 * `generate`. A transcript that is already clean comes back identical (trailing whitespace aside).
 */
export async function cleanTranscript(request: CleanRequest, deps: CleanDeps): Promise<string> {
	const { text } = await deps.generate({
		model: request.model,
		prompt: buildPrompt(request.text, request.language),
		temperature: CLEANUP_TEMPERATURE
	});
	const cleaned = typeof text === "string" ? text.trim() : "";
	return cleaned.length > 0 ? cleaned : request.text.trim();
}
