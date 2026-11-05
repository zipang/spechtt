/**
 * Opt-in dump of every transcript event the providers produce.
 *
 * This is a diagnosis tool, not part of the running app's output, and it is the
 * one place where transcript text is written to `stderr`: the analysis it
 * enables is impossible without the text itself. It stays off unless
 * `SPEECHTT_STT_PARTIALS` is exactly `1`, and it is deliberately a different
 * variable from `SPEECHTT_STT_TRACE`, whose lines carry lengths only.
 *
 * Used by `worker.ts`, which wraps its `emit` so every event an adapter produces
 * is traced in order. `stdout` remains NDJSON-only.
 */

/** Environment variable that switches the partial dump on. */
export const PARTIALS_ENV = "SPEECHTT_STT_PARTIALS";

/** Every transcript-bearing event, i.e. the ones worth analysing. */
const TRACED_TYPES = new Set([
	"started",
	"partial",
	"delta",
	"committed",
	"stopped",
]);

/** Longest text kept in one line; a window hypothesis stays far below this. */
const MAX_TEXT = 4000;

/** Prefix every dump line carries, so a log can be filtered with one grep. */
export const PARTIALS_TAG = "[stt-partials]";

/** Whether the dump is enabled, i.e. `SPEECHTT_STT_PARTIALS=1`. */
export function partialsTraceEnabled(
	env: Record<string, string | undefined> = process.env,
): boolean {
	return env[PARTIALS_ENV] === "1";
}

function isRecord(value: unknown): value is Record<string, unknown> {
	return typeof value === "object" && value !== null;
}

/**
 * The one-line JSON record for `event`, or `null` when it carries no transcript.
 *
 * `seq` is monotonic per process, so a log can be replayed in order even when
 * two events share a millisecond. Field names match the wire protocol, with
 * `chars` added because the interesting comparison is length against content.
 */
export function formatPartialRecord(
	event: unknown,
	seq: number,
): string | null {
	if (!isRecord(event)) {
		return null;
	}
	const type = event.type;
	if (typeof type !== "string" || !TRACED_TYPES.has(type)) {
		return null;
	}

	const record: Record<string, unknown> = { seq, type };
	for (const key of ["sessionId", "segment", "requestId"] as const) {
		const value = event[key];
		if (typeof value === "string") {
			record[key] = value;
		}
	}

	// The text field is named after the event: `text` for a partial, a final and
	// a stop, `delta` for an incremental token.
	const text =
		typeof event.text === "string"
			? event.text
			: typeof event.delta === "string"
				? event.delta
				: typeof event.fullText === "string"
					? event.fullText
					: undefined;
	if (text !== undefined) {
		record.chars = text.length;
		record.text =
			text.length > MAX_TEXT ? `${text.slice(0, MAX_TEXT)}…` : text;
	}

	return JSON.stringify(record);
}

/**
 * Writes the record for `event` when the dump is enabled.
 *
 * Returns the line it wrote, or `null` when it wrote nothing, so a test can
 * assert the shape without capturing `stderr`.
 */
export function tracePartial(event: unknown, seq: number): string | null {
	const record = formatPartialRecord(event, seq);
	if (record === null) {
		return null;
	}
	const line = `${PARTIALS_TAG} ${record}`;
	console.error(line);
	return line;
}

/**
 * The header line describing the session a dump belongs to.
 *
 * Two takes from different models land in one log, so every dump needs to say
 * which provider, model and language produced it; that is the whole point of
 * comparing models.
 */
export function formatSessionHeader(details: {
	provider: string;
	model: string;
	language?: string;
	sampleRate?: number;
	kind: "live" | "file";
}): string {
	const pairs = [
		`kind=${details.kind}`,
		`provider=${details.provider}`,
		`model=${details.model}`,
		`language=${details.language ?? "-"}`,
		details.sampleRate === undefined ? "" : `rate=${details.sampleRate}`,
	]
		.filter((pair) => pair.length > 0)
		.join(" ");
	return `${PARTIALS_TAG} session ${pairs}`;
}
