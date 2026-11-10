/**
 * Failure descriptions for the transcription adapters and the worker loop.
 *
 * Used by `adapters/vercel-ai-sdk.ts`, `adapters/mistral.ts`, `adapters/file.ts` and `worker.ts`,
 * each of which turns a rejection into one short bridge event plus one diagnostic line.
 *
 * The two are deliberately different sizes. The bridge event is user-facing and short, so the
 * screen can show it; the `stderr` line is the only place the full cause fits, and it is what makes
 * a provider failure diagnosable. Both are bounded, and the line is redacted.
 *
 * A rejection is not always an `Error`: the AI SDK can reject with a plain object, a wrapper around
 * another error, or a string, so [`describeError`] reads every documented shape before it gives up
 * on a fallback sentence.
 *
 * `stdout` stays NDJSON-only: nothing here writes to it.
 */

/** Longest diagnostic kept; the bridge message is clamped separately. */
const MAX_DIAGNOSTIC_LENGTH = 300;

/**
 * Collapses whitespace, redacts credentials and bounds the length.
 *
 * Applied to every string that reaches `stderr`, so a provider echoing an `Authorization` header
 * cannot leak a key into a log.
 */
export function sanitizeDiagnostic(raw: string): string {
	const collapsed = raw
		.replace(/[\u0000-\u001f\u007f]+/g, " ")
		.replace(/\s+/g, " ")
		.trim();
	const redacted = collapsed
		.replace(/authorization:[^,;]*/gi, "[REDACTED]")
		.replace(/bearer\s+\S+/gi, "[REDACTED]")
		.replace(/sk-[a-z0-9_-]+/gi, "[REDACTED]")
		.replace(/(api_key|key)=[^\s,;]+/gi, "[REDACTED]");
	return redacted.length > 120 ? `${redacted.slice(0, 119)}…` : redacted;
}

function isRecord(value: unknown): value is Record<string, unknown> {
	return typeof value === "object" && value !== null;
}

/**
 * A short, user-facing description of a rejection.
 *
 * Reads, in order: an `Error`'s `message`, a string, then any object's `message`/`cause`/`error`
 * field, and finally a bounded JSON rendering. The last resort is a serialization rather than a
 * fixed sentence, so a future provider that rejects with an unexpected shape still produces a
 * message that names it.
 *
 * `fallback` is used only when nothing at all can be read from `error`.
 */
export function describeError(error: unknown, fallback: string): string {
	if (error instanceof Error && error.message.trim().length > 0) {
		const cause = error.cause;
		const suffix =
			cause === undefined || cause === error
				? ""
				: ` (cause: ${describeError(cause, "")}`.replace(/(, )?\)$/, ")");
		return suffix.length > 0 ? `${error.message}${suffix}` : error.message;
	}
	if (typeof error === "string" && error.trim().length > 0) {
		return error.trim();
	}
	if (isRecord(error)) {
		for (const key of ["message", "error", "cause", "detail", "reason"] as const) {
			const nested: unknown = error[key];
			if (nested === undefined || nested === error) {
				continue;
			}
			const described = describeError(nested, "");
			if (described.length > 0) {
				return described;
			}
		}
		let json: string;
		try {
			json = JSON.stringify(error);
		} catch {
			json = "";
		}
		if (typeof json === "string" && json.length > 0 && json !== "{}") {
			return json;
		}
	}
	return fallback;
}

/**
 * Writes one redacted diagnostic line for a failure.
 *
 * `context` is a short tag such as `stream`, `batch` or `file`; `details` are `key=value` pairs the
 * caller already knows (provider, model, language). The raw rejection is included so the next
 * failure is diagnosable from `stderr` alone, without a rebuild.
 */
export function logFailure(
	context: string,
	details: Record<string, string | number | undefined>,
	error: unknown,
	fallback: string
): void {
	const pairs = Object.entries(details)
		.filter(([, value]) => value !== undefined && value !== "")
		.map(([key, value]) => `${key}=${value}`)
		.join(" ");
	const cause = sanitizeDiagnostic(describeError(error, fallback));
	const line = `[stt-worker] error context=${context}${pairs.length > 0 ? ` ${pairs}` : ""} cause=${cause}`;
	console.error(line.slice(0, MAX_DIAGNOSTIC_LENGTH));
}
