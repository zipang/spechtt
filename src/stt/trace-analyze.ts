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

export interface SessionHeader {
	readonly provider: string;
	readonly model: string;
	readonly language?: string;
	readonly sampleRate?: number;
	readonly kind: "live" | "file";
}

export interface TracedEvent {
	readonly seq: number;
	readonly type: string;
	readonly sessionId?: string;
	readonly segment?: string;
	readonly text?: string;
	readonly chars?: number;
	/**
	 * The take this event belongs to, from the dump's session header.
	 *
	 * A session id restarts at `take-1` on every launch, so two takes on two
	 * models look identical without it; the header is what keeps them apart.
	 */
	readonly session?: SessionHeader;
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

/**
 * Parses one dump line.
 *
 * A `[stt-partials] session …` line yields a [`SessionHeader`]; a record line
 * yields a [`TracedEvent`] carrying the header that was current when it was
 * emitted, so a file holding two takes keeps them apart even though both call
 * their session `take-1`.
 */
export function parseDumpLine(line: string): TracedEvent | SessionHeader | null {
	const start = line.indexOf("[stt-partials]");
	if (start < 0) {
		return null;
	}
	const body = line.slice(start + "[stt-partials]".length).trim();
	if (body.startsWith("session ")) {
		const details = new Map<string, string>();
		for (const pair of body.slice("session ".length).split(/\s+/u)) {
			const eq = pair.indexOf("=");
			if (eq > 0) {
				details.set(pair.slice(0, eq), pair.slice(eq + 1));
			}
		}
		const provider = details.get("provider");
		const model = details.get("model");
		if (provider === undefined || model === undefined) {
			return null;
		}
		const rate = details.get("rate");
		const language = details.get("language");
		return {
			provider,
			model,
			language: language === undefined || language === "-" ? undefined : language,
			sampleRate: rate === undefined ? undefined : Number(rate),
			kind: details.get("kind") === "file" ? "file" : "live",
		};
	}

	let parsed: unknown;
	try {
		parsed = JSON.parse(body);
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

/** Whether a parsed line is a session header. */
export function isSessionHeader(
	value: TracedEvent | SessionHeader,
): value is SessionHeader {
	return "provider" in value && "model" in value && !("seq" in value);
}

/**
 * Reads a whole dump, attaching each event to the take that produced it.
 *
 * Takes are keyed by header, so appending two launches to one file is the
 * intended way to compare models.
 */
export function parseDump(text: string): TracedEvent[] {
	const events: TracedEvent[] = [];
	let current: SessionHeader | undefined;
	for (const line of text.split("\n")) {
		const parsed = parseDumpLine(line);
		if (parsed === null) {
			continue;
		}
		if (isSessionHeader(parsed)) {
			current = parsed;
			continue;
		}
		events.push({ ...parsed, ...(current === undefined ? {} : { session: current }) });
	}
	return events;
}

export interface StreamReport {
	/** The take that produced this stream, from the dump's header. */
	readonly provider: string;
	readonly model: string;
	readonly language?: string;
	readonly sessionId: string;
	readonly segment: string;
	/**
	 * Which channel this report describes.
	 *
	 * A `partial` is a whole hypothesis and a `delta` is an increment; they mean
	 * different things, and a provider may use both. Lumping them would classify
	 * an increment against a hypothesis and call every pair a new window.
	 */
	readonly channel: "partial" | "delta";
	/** Number of events seen on this channel. */
	readonly hypotheses: number;
	readonly counts: Readonly<Record<Transition, number>>;
	/** The single most common transition, which names the channel's behaviour. */
	readonly dominant: Transition;
	/** Longest seam overlap seen, in words. */
	readonly maxOverlapWords: number;
	/** Characters the seam would show twice. */
	readonly duplicatedChars: number;
	/**
	 * Pairs whose seam only matched after folding whitespace: the provider
	 * re-spaced the same words, which a word-level buffer cannot detect.
	 */
	readonly retokenizedPairs: number;
	/**
	 * Duplication the shipped reducer would produce, as a multiple of the
	 * provider's authoritative `stopped` text: `1` is clean, `7.28` means seven
	 * times too much.
	 */
	readonly duplicatedFactor: number;
	/** Characters of the authoritative `stopped` text, `0` when none was seen. */
	readonly finalChars: number;
}

/**
 * Builds one report per stream and per channel.
 *
 * Partials and deltas are analysed apart on purpose: a partial is a whole
 * hypothesis and a delta is an increment, so comparing one against the other
 * describes nothing. A provider that uses both channels gets two reports, and
 * the fact that it uses both is itself the finding.
 *
 * The duplication factor divides the characters the shipped reducer would show
 * by the length of the provider's authoritative `stopped` text, which is the
 * only ground truth in a dump.
 */
export function analyzeEvents(events: readonly TracedEvent[]): StreamReport[] {
	const streams = new Map<string, TracedEvent[]>();
	const finals = new Map<string, number>();
	for (const event of events) {
		if (event.type === "stopped") {
			// A `stopped` carries no segment: the take is the whole session, so
			// the final is recorded per session and every segment reads it.
			finals.set(sessionKeyOf(event), event.text?.trim().length ?? 0);
			continue;
		}
		if (event.type !== "partial" && event.type !== "delta") {
			continue;
		}
		if (event.text === undefined) {
			continue;
		}
		const key = `${keyOf(event)}\u0000${event.type}`;
		const list = streams.get(key);
		if (list === undefined) {
			streams.set(key, [event]);
		} else {
			list.push(event);
		}
	}

	const reports: StreamReport[] = [];
	for (const [key, list] of streams) {
		const parts = key.split("\u0000");
		const channel = parts.pop() === "delta" ? "delta" : "partial";
		const [provider = "-", model = "-", language = "-", sessionId = "-", segment = "-"] =
			parts;
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
		// What the shipped reducer would display, accumulated the way its
		// `settle_live()` path does: a pair that is not a revision commits the
		// whole previous tail and starts the next one.
		let shownChars = 0;
		for (let index = 1; index < list.length; index += 1) {
			const previous = list[index - 1]?.text ?? "";
			const next = list[index]?.text ?? "";
			const classified = classifyPair(previous, next);
			counts[classified.kind] += 1;
			maxOverlapWords = Math.max(maxOverlapWords, classified.overlapWords);
			duplicatedChars += classified.overlapChars;
			if (classified.retokenized) {
				retokenizedPairs += 1;
			}
			if (classified.kind !== "extend" && classified.kind !== "identical") {
				shownChars += previous.trim().length;
			}
		}
		shownChars += list[list.length - 1]?.text?.trim().length ?? 0;
		const finalChars =
			finals.get([provider, model, language, sessionId].join("\u0000")) ?? 0;
		const dominant = (Object.entries(counts) as [Transition, number][]).reduce(
			(best, entry) => (entry[1] > best[1] ? entry : best),
			["identical", -1] as [Transition, number],
		)[0];
		reports.push({
			provider,
			model,
			...(language === "-" ? {} : { language }),
			sessionId,
			segment,
			channel,
			hypotheses: list.length,
			counts,
			dominant,
			maxOverlapWords,
			duplicatedChars,
			retokenizedPairs,
			duplicatedFactor:
				finalChars === 0 ? 1 : Math.round((shownChars / finalChars) * 100) / 100,
			finalChars,
		});
	}
	return reports;
}

/** The session identity of an event: provider, model, language and session. */
function sessionKeyOf(event: TracedEvent): string {
	return [
		event.session?.provider ?? "-",
		event.session?.model ?? "-",
		event.session?.language ?? "-",
		event.sessionId ?? "-",
	].join("\u0000");
}

/** The stream identity of an event, without the channel. */
function keyOf(event: TracedEvent): string {
	return [
		event.session?.provider ?? "-",
		event.session?.model ?? "-",
		event.session?.language ?? "-",
		event.sessionId ?? "-",
		event.segment ?? "-",
	].join("\u0000");
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

/**
 * A human-readable report, one block per model, then one line per channel.
 *
 * Grouped by model because the question the dump answers is "do the models
 * behave the same?", and a reader should not have to correlate segments by hand.
 */
export function formatReport(reports: readonly StreamReport[]): string {
	if (reports.length === 0) {
		return "no hypotheses found — was SPEECHTT_STT_PARTIALS=1 set?";
	}

	const models = new Map<string, StreamReport[]>();
	for (const report of reports) {
		const key = `${report.provider}\u0000${report.model}\u0000${report.language ?? "-"}`;
		const list = models.get(key);
		if (list === undefined) {
			models.set(key, [report]);
		} else {
			list.push(report);
		}
	}

	const blocks: string[] = [];
	for (const [key, list] of models) {
		const [provider = "-", model = "-", language = "-"] = key.split("\u0000");
		const lines = [`${model}${language === "-" ? "" : ` [${language}]`}`];
		const finalChars = Math.max(...list.map((report) => report.finalChars));
		lines.push(`  authoritative stopped text: ${finalChars} chars`);

		for (const channel of ["partial", "delta"] as const) {
			const channelReports = list.filter((report) => report.channel === channel);
			if (channelReports.length === 0) {
				continue;
			}
			const totals: Record<Transition, number> = {
				identical: 0,
				extend: 0,
				shrink: 0,
				slide: 0,
				new: 0,
			};
			let hypotheses = 0;
			let maxOverlapWords = 0;
			for (const report of channelReports) {
				for (const [kind, count] of Object.entries(report.counts)) {
					totals[kind as Transition] += count;
				}
				hypotheses += report.hypotheses;
				maxOverlapWords = Math.max(maxOverlapWords, report.maxOverlapWords);
			}
			const dominant = (Object.entries(totals) as [Transition, number][]).reduce(
				(best, entry) => (entry[1] > best[1] ? entry : best),
				["identical", -1] as [Transition, number],
			)[0];
			const merged: StreamReport = {
				provider,
				model,
				...(language === "-" ? {} : { language }),
				sessionId: "-",
				segment: "-",
				channel,
				hypotheses,
				counts: totals,
				dominant,
				maxOverlapWords,
				duplicatedChars: channelReports.reduce(
					(sum, report) => sum + report.duplicatedChars,
					0,
				),
				retokenizedPairs: channelReports.reduce(
					(sum, report) => sum + report.retokenizedPairs,
					0,
				),
				duplicatedFactor: Math.max(
					...channelReports.map((report) => report.duplicatedFactor),
				),
				finalChars,
			};
			const counts = (Object.entries(totals) as [Transition, number][])
				.filter(([, count]) => count > 0)
				.map(([kind, count]) => `${kind}=${count}`)
				.join(" ");
			lines.push(
				`  ${channel}s=${merged.hypotheses} behaviour=${behaviorOf(merged)} (${counts})`,
			);
			lines.push(
				`    maxOverlapWords=${merged.maxOverlapWords} retokenizedPairs=${merged.retokenizedPairs} shipped reducer=${merged.duplicatedFactor}x`,
			);
		}

		if (list.length > 1) {
			const segments = [...new Set(list.map((report) => report.segment))];
			lines.push(`  (${segments.length} segment(s): ${segments.join(", ")})`);
		}
		blocks.push(lines.join("\n"));
	}
	return blocks.join("\n\n");
}

if (import.meta.main) {
	const path = process.argv[2];
	const text =
		path === undefined
			? await Bun.stdin.text()
			: await Bun.file(path).text();
	console.log(formatReport(analyzeEvents(parseDump(text))));
}
