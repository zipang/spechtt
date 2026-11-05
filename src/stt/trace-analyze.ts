/**
 * Analysis of a `SPEECHTT_STT_PARTIALS=1` dump.
 *
 * The live transcript problem is a question about provider behaviour, not about
 * our reducer: a hypothesis may grow cumulatively, be re-sent shorter, or slide
 * forward over an overlapping window. This module classifies every consecutive
 * pair of the same stream so the answer comes from a real take instead of a
 * guess, and so two models can be compared on the same vocabulary.
 *
 * Run a take with the dump on, then:
 *
 * ```sh
 * bun src/stt/trace-analyze.ts dump.log
 * grep '"type":"partial"' dump.log | bun src/stt/trace-analyze.ts
 * ```
 *
 * Nothing here is imported by `worker.ts`, so it never reaches the running
 * worker.
 */

/** How a hypothesis relates to the one before it in the same stream. */
export type Transition =
	/** The same text again: a duplicate the provider re-sent. */
	| "identical"
	/** The previous text plus more: cumulative growth. */
	| "extend"
	/** A shorter draft of the previous text: a re-punctuation. */
	| "shrink"
	/** Shares a suffix/prefix run: a window sliding forward. */
	| "slide"
	/** Shares nothing: a fresh window or a new utterance. */
	| "new";

export interface PairClassification {
	readonly kind: Transition;
	/** Words the two hypotheses share at the seam, for `slide`. */
	readonly overlapWords: number;
	/** Characters of `next` the seam already showed, i.e. shown twice. */
	readonly overlapChars: number;
	/** Words of `next` that `previous` did not already carry at its end. */
	readonly newWords: number;
	/**
	 * `true` when the seam only matched after folding whitespace away, which
	 * means the provider re-spaced the same words between two hypotheses
	 * (`MAI.` against `MA I.`). That is the shape a plain word comparison
	 * cannot see, and the reason a live buffer cannot rely on one.
	 */
	readonly retokenized: boolean;
}

export interface TracedEvent {
	readonly seq: number;
	readonly type: string;
	readonly sessionId?: string;
	readonly segment?: string;
	readonly text?: string;
	readonly chars?: number;
}

/** Shortest seam worth calling an overlap once whitespace is folded away. */
const MIN_COMPACT_OVERLAP = 10;

function words(text: string): string[] {
	const trimmed = text.trim();
	return trimmed.length === 0 ? [] : trimmed.split(/\s+/u);
}

/** Lowercases and strips edge punctuation, so a re-punctuation still matches. */
function normalize(word: string): string {
	return word.toLowerCase().replace(/^[^\p{L}\p{N}]+|[^\p{L}\p{N}]+$/gu, "");
}

/** Lowercases and removes every whitespace character. */
function compact(text: string): string {
	return text.toLowerCase().replace(/[\s]+/gu, "");
}

/** Length of the longest suffix of `left` that is also a prefix of `right`. */
function seamLength(left: string, right: string): number {
	const limit = Math.min(left.length, right.length);
	for (let length = limit; length >= MIN_COMPACT_OVERLAP; length -= 1) {
		if (left.endsWith(right.slice(0, length))) {
			return length;
		}
	}
	return 0;
}

/**
 * Classifies `next` against `previous`.
 *
 * Word-level first: an exact repeat, then a strict prefix relation in either
 * direction, then a word run shared at the seam. A single shared word is not an
 * overlap — two sentences really can start with the same word — so `slide` needs
 * at least two, the same threshold the segmented junction merge uses.
 *
 * When no word run matches, the pair is compared again with all whitespace
 * removed. A provider that re-spaces its own words between hypotheses produces
 * the same text twice, and only that second comparison sees it; a word-level
 * classifier would call it a fresh window and the live buffer would duplicate
 * everything before it.
 */
export function classifyPair(
	previous: string,
	next: string,
): PairClassification {
	const previousTrimmed = previous.trim();
	const nextTrimmed = next.trim();
	const previousWords = words(previous);
	const nextWords = words(next);

	if (previousTrimmed === nextTrimmed) {
		return {
			kind: "identical",
			overlapWords: 0,
			overlapChars: 0,
			newWords: 0,
			retokenized: false,
		};
	}
	if (nextTrimmed.startsWith(previousTrimmed)) {
		const added = nextTrimmed.slice(previousTrimmed.length);
		return {
			kind: "extend",
			overlapWords: 0,
			overlapChars: 0,
			newWords: words(added).length,
			retokenized: false,
		};
	}
	if (previousTrimmed.startsWith(nextTrimmed)) {
		return {
			kind: "shrink",
			overlapWords: 0,
			overlapChars: 0,
			newWords: 0,
			retokenized: false,
		};
	}

	const limit = Math.min(previousWords.length, nextWords.length);
	for (let candidate = limit; candidate >= 2; candidate -= 1) {
		let matches = true;
		for (let index = 0; index < candidate; index += 1) {
			const left = normalize(
				previousWords[previousWords.length - candidate + index] ?? "",
			);
			const right = normalize(nextWords[index] ?? "");
			if (left.length === 0 || right.length === 0 || left !== right) {
				matches = false;
				break;
			}
		}
		if (matches) {
			const overlap = nextWords.slice(0, candidate).join(" ");
			return {
				kind: "slide",
				overlapWords: candidate,
				overlapChars: overlap.length,
				newWords: nextWords.length - candidate,
				retokenized: false,
			};
		}
	}

	const seam = seamLength(compact(previous), compact(next));
	if (seam >= MIN_COMPACT_OVERLAP) {
		// Count the words of `next` the compact seam covers, so the threshold
		// keeps its word meaning even when the boundary falls mid-word.
		let covered = 0;
		let consumed = 0;
		for (const word of nextWords) {
			consumed += compact(word).length;
			if (consumed > seam) {
				break;
			}
			covered += 1;
		}
		if (covered >= 2) {
			return {
				kind: "slide",
				overlapWords: covered,
				overlapChars: nextWords.slice(0, covered).join(" ").length,
				newWords: nextWords.length - covered,
				retokenized: true,
			};
		}
	}

	return {
		kind: "new",
		overlapWords: 0,
		overlapChars: 0,
		newWords: nextWords.length,
		retokenized: false,
	};
}

/** One parsed dump line, or `null` for anything else. */
export function parseDumpLine(line: string): TracedEvent | null {
	const marker = line.indexOf('{"');
	if (marker < 0) {
		return null;
	}
	let parsed: unknown;
	try {
		parsed = JSON.parse(line.slice(marker));
	} catch {
		return null;
	}
	if (typeof parsed !== "object" || parsed === null) {
		return null;
	}
	const record = parsed as Record<string, unknown>;
	if (typeof record.seq !== "number" || typeof record.type !== "string") {
		return null;
	}
	return {
		seq: record.seq,
		type: record.type,
		sessionId:
			typeof record.sessionId === "string" ? record.sessionId : undefined,
		segment: typeof record.segment === "string" ? record.segment : undefined,
		text: typeof record.text === "string" ? record.text : undefined,
		chars: typeof record.chars === "number" ? record.chars : undefined,
	};
}

export interface StreamReport {
	readonly sessionId: string;
	readonly segment: string;
	/** Number of hypotheses seen, i.e. the pairs plus one. */
	readonly hypotheses: number;
	readonly counts: Readonly<Record<Transition, number>>;
	/** The single most common transition, which names the provider's behaviour. */
	readonly dominant: Transition;
	/** Longest seam overlap seen, in words. */
	readonly maxOverlapWords: number;
	/**
	 * Characters the naive accumulator would add, against the characters of the
	 * final text: the amount of duplication a live buffer would show.
	 */
	readonly duplicatedChars: number;
	/**
	 * Pairs whose seam only matched after folding whitespace: the provider
	 * re-spaced the same words, which a word-level buffer cannot detect.
	 */
	readonly retokenizedPairs: number;
}

/**
 * Builds the per-stream report from parsed dump events.
 *
 * Only `partial` and `delta` events are hypotheses; a `committed` or `stopped`
 * event closes the stream and is reported separately by the caller.
 */
export function analyzeEvents(events: readonly TracedEvent[]): StreamReport[] {
	const streams = new Map<string, TracedEvent[]>();
	for (const event of events) {
		if (event.type !== "partial" && event.type !== "delta") {
			continue;
		}
		if (event.text === undefined) {
			continue;
		}
		const key = `${event.sessionId ?? "-"}\u0000${event.segment ?? "-"}`;
		const list = streams.get(key);
		if (list === undefined) {
			streams.set(key, [event]);
		} else {
			list.push(event);
		}
	}

	const reports: StreamReport[] = [];
	for (const [key, list] of streams) {
		const [sessionId = "-", segment = "-"] = key.split("\u0000");
		const counts: Record<Transition, number> = {
			identical: 0,
			extend: 0,
			shrink: 0,
			slide: 0,
			new: 0,
		};
		let maxOverlapWords = 0;
		let duplicatedChars = 0;
		let retokenizedPairs = 0;
		for (let index = 1; index < list.length; index += 1) {
			const previous = list[index - 1]?.text ?? "";
			const next = list[index]?.text ?? "";
			const classified = classifyPair(previous, next);
			counts[classified.kind] += 1;
			maxOverlapWords = Math.max(maxOverlapWords, classified.overlapWords);
			// A slide shows the seam twice: that overlap is exactly the
			// duplication a naive accumulator would append.
			duplicatedChars += classified.overlapChars;
			if (classified.retokenized) {
				retokenizedPairs += 1;
			}
		}
		const dominant = (Object.entries(counts) as [Transition, number][]).reduce(
			(best, entry) => (entry[1] > best[1] ? entry : best),
			["identical", -1] as [Transition, number],
		)[0];
		reports.push({
			sessionId,
			segment,
			hypotheses: list.length,
			counts,
			dominant,
			maxOverlapWords,
			duplicatedChars,
			retokenizedPairs,
		});
	}
	return reports;
}

/** The name the observed behaviour earns, from the dominant transition. */
export function behaviorOf(report: StreamReport): string {
	switch (report.dominant) {
		case "extend":
			return "cumulative";
		case "slide":
			return "sliding window";
		case "shrink":
			return "re-punctuating window";
		case "identical":
			return "duplicate re-sends";
		case "new":
			return "disjoint windows";
	}
}

/** A human-readable report, one block per stream. */
export function formatReport(reports: readonly StreamReport[]): string {
	if (reports.length === 0) {
		return "no hypotheses found — was SPEECHTT_STT_PARTIALS=1 set?";
	}
	return reports
		.map((report) => {
			const counts = (
				Object.entries(report.counts) as [Transition, number][]
			)
				.filter(([, count]) => count > 0)
				.map(([kind, count]) => `${kind}=${count}`)
				.join(" ");
			return [
				`stream session=${report.sessionId} segment=${report.segment}`,
				`  hypotheses=${report.hypotheses} behaviour=${behaviorOf(report)}`,
				`  ${counts}`,
				`  maxOverlapWords=${report.maxOverlapWords} duplicatedChars=${report.duplicatedChars} retokenizedPairs=${report.retokenizedPairs}`,
			].join("\n");
		})
		.join("\n\n");
}

if (import.meta.main) {
	const path = process.argv[2];
	const text =
		path === undefined
			? await Bun.stdin.text()
			: await Bun.file(path).text();
	const events = text
		.split("\n")
		.map(parseDumpLine)
		.filter((event): event is TracedEvent => event !== null);
	console.log(formatReport(analyzeEvents(events)));
}
