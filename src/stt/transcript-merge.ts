/**
 * Junction-safe transcript merging for segmented transcription.
 *
 * Silence-aligned segments may overlap by a few hundred milliseconds after a
 * forced cut, so the same words can be transcribed twice. `mergeTranscript`
 * removes a genuine suffix/prefix word run at the junction while preserving
 * legitimate repetitions such as "very very".
 */

export interface MergeOptions {
	/** Longest repeated word run removed at a junction; default 4. */
	readonly maxOverlapWords?: number;
	/** Allow a single-word overlap to be removed; default false. */
	readonly allowSingleWordOverlap?: boolean;
}

const DEFAULT_MAX_OVERLAP_WORDS = 4;
const EDGE_PUNCTUATION = /^[^\p{L}\p{N}]+|[^\p{L}\p{N}]+$/gu;

function words(text: string): string[] {
	const trimmed = text.trim();
	return trimmed.length === 0 ? [] : trimmed.split(/\s+/u);
}

/** Lowercases and strips edge punctuation; empty for punctuation-only tokens. */
function normalize(word: string): string {
	return word.toLowerCase().replace(EDGE_PUNCTUATION, "");
}

/**
 * Joins `next` to `previous`, removing the longest word run that appears both
 * at the end of `previous` and at the start of `next`.
 *
 * The comparison ignores case and edge punctuation, but the returned text keeps
 * the original spelling of both sides. A one-word overlap is only removed when
 * `allowSingleWordOverlap` is set, so ordinary repetitions survive.
 */
export function mergeTranscript(
	previous: string,
	next: string,
	options: MergeOptions = {},
): string {
	const nextText = next.trim();
	if (previous.trim().length === 0) {
		return nextText;
	}
	if (nextText.length === 0) {
		return previous;
	}

	const previousWords = words(previous);
	const nextWords = words(nextText);
	const maxOverlap = Math.max(
		1,
		options.maxOverlapWords ?? DEFAULT_MAX_OVERLAP_WORDS,
	);
	const minimum = options.allowSingleWordOverlap === true ? 1 : 2;

	let overlap = 0;
	for (
		let candidate = Math.min(maxOverlap, previousWords.length, nextWords.length);
		candidate >= minimum;
		candidate -= 1
	) {
		let matches = true;
		for (let index = 0; index < candidate; index += 1) {
			const left = normalize(previousWords[previousWords.length - candidate + index]);
			const right = normalize(nextWords[index]);
			if (left.length === 0 || right.length === 0 || left !== right) {
				matches = false;
				break;
			}
		}
		if (matches) {
			overlap = candidate;
			break;
		}
	}

	const remainder = nextWords.slice(overlap).join(" ");
	return remainder.length === 0 ? previous : `${previous} ${remainder}`;
}
