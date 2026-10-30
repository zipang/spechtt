import { afterEach, beforeEach, describe, expect, spyOn, test } from "bun:test";
import type { OutboundEvent } from "../protocol";
import { FileDecodeError } from "../file-input";
import type { SttSession, SttSessionConfig } from "./types";
import {
	FILE_SEGMENTATION,
	type FileSessionDeps,
	createFileSession,
} from "./file";
import type { TranscribeFn } from "./mistral";

const RATE = 16_000;

const config: SttSessionConfig & { path: string } = {
	sessionId: "file-1",
	provider: "vercel-ai-gateway",
	model: "openai/gpt-4o-transcribe",
	sampleRate: RATE,
	apiKey: "sk-test",
	path: "/tmp/meeting.mp4",
};

function collect() {
	const events: OutboundEvent[] = [];
	return { events, emit: (event: OutboundEvent) => events.push(event) };
}

function tone(ms: number, amplitude = 0.5, frequency = 220): Uint8Array {
	const samples = Math.round((RATE * ms) / 1000);
	const bytes = new Uint8Array(samples * 2);
	const view = new DataView(bytes.buffer);
	for (let index = 0; index < samples; index += 1) {
		const value =
			Math.sin((2 * Math.PI * frequency * index) / RATE) * amplitude * 32_767;
		view.setInt16(index * 2, Math.round(value), true);
	}
	return bytes;
}

function silence(ms: number): Uint8Array {
	return new Uint8Array(Math.round((RATE * ms) / 1000) * 2);
}

/** Splits decoded PCM into 40 ms chunks, the way ffmpeg streams it. */
function chunked(bytes: Uint8Array, chunkSize = 640): Uint8Array[] {
	const chunks: Uint8Array[] = [];
	for (let offset = 0; offset < bytes.length; offset += chunkSize) {
		chunks.push(bytes.subarray(offset, Math.min(offset + chunkSize, bytes.length)));
	}
	return chunks;
}

function textAt(bytes: Uint8Array, start: number, length: number): string {
	return String.fromCharCode(...bytes.slice(start, start + length));
}

interface DecodeOptions {
	readonly chunks?: readonly Uint8Array[];
	/** Thrown after every chunk has been yielded. */
	readonly error?: unknown;
	readonly onPull?: (signal: AbortSignal, index: number) => void;
}

function decodeOf(options: DecodeOptions) {
	const chunks = options.chunks ?? [];
	return async function* decode(
		_path: string,
		{ signal }: { signal: AbortSignal },
	): AsyncIterable<Uint8Array> {
		for (const [index, chunk] of chunks.entries()) {
			if (signal.aborted) return;
			options.onPull?.(signal, index);
			yield chunk;
		}
		if (options.error !== undefined) {
			throw options.error;
		}
	};
}

interface SessionOptions {
	readonly chunks?: readonly Uint8Array[];
	readonly texts?: readonly string[];
	readonly total?: number | null;
	readonly decodeError?: unknown;
	readonly transcribeError?: unknown;
	readonly segmentation?: FileSessionDeps["segmentation"];
	readonly onTranscribe?: (call: number) => void;
	readonly onPull?: (signal: AbortSignal, index: number) => void;
}

/** Builds a session over fakes and records every transcription call. */
function setup(options: SessionOptions = {}) {
	const events: OutboundEvent[] = [];
	let resolveFinished = (): void => {};
	let resolveFailed = (): void => {};
	const finished = new Promise<void>((resolve) => {
		resolveFinished = resolve;
	});
	const failed = new Promise<void>((resolve) => {
		resolveFailed = resolve;
	});
	const emit = (event: OutboundEvent): void => {
		events.push(event);
		if (event.type === "stopped") {
			resolveFinished();
		}
		if (event.type === "error") {
			resolveFailed();
		}
	};
	const calls: Parameters<TranscribeFn>[0][] = [];
	const texts = options.texts ?? [];
	const transcribe: TranscribeFn = async (request) => {
		const call = calls.length;
		calls.push(request);
		options.onTranscribe?.(call);
		if (options.transcribeError !== undefined) {
			throw options.transcribeError;
		}
		return { text: texts[call] ?? texts[texts.length - 1] ?? "" } as never;
	};

	let probed = 0;
	const deps: FileSessionDeps = {
		decode: decodeOf({
			chunks: options.chunks ?? chunked(tone(1500)),
			error: options.decodeError,
			onPull: options.onPull,
		}),
		probeDuration: async () => {
			probed += 1;
			return options.total === undefined ? 340.25 : options.total;
		},
		transcribe,
		resolveModel: () => "test-model" as never,
		segmentation: options.segmentation,
	};

	const session = createFileSession(config, emit, deps);
	return { session, events, calls, finished, failed, probed: () => probed };
}

function types(events: readonly OutboundEvent[]): string[] {
	return events.map((event) => event.type);
}

function progressEvents(events: readonly OutboundEvent[]) {
	return events.flatMap((event) =>
		event.type === "progress"
			? [
					{
						processedSeconds: event.processedSeconds,
						totalSeconds: event.totalSeconds,
					},
				]
			: [],
	);
}

function committedTexts(events: readonly OutboundEvent[]): string[] {
	return events.flatMap((event) =>
		event.type === "committed" ? [event.text] : [],
	);
}

function stoppedText(events: readonly OutboundEvent[]): string | undefined {
	const stopped = events.find((event) => event.type === "stopped");
	return stopped?.type === "stopped" ? stopped.fullText : undefined;
}

describe("FILE_SEGMENTATION", () => {
	test("uses the file-tuned cadence, not the live one", () => {
		expect(FILE_SEGMENTATION).toEqual({ targetMs: 30_000, maxSegmentMs: 60_000 });
	});
});

describe("createFileSession", () => {
	let errorSpy: ReturnType<typeof spyOn>;

	beforeEach(() => {
		// Segment diagnostics are informational; keep the test output readable.
		errorSpy = spyOn(console, "error").mockImplementation(() => {});
	});

	afterEach(() => {
		errorSpy.mockRestore();
	});

	test("emits started, one committed segment and the merged stop text", async () => {
		const { events, calls, probed, finished } = setup({
			texts: ["hello world"],
		});

		await finished;

		expect(probed()).toBe(1);
		expect(types(events)).toEqual(["started", "committed", "progress", "stopped"]);
		expect(committedTexts(events)).toEqual(["hello world"]);
		expect(stoppedText(events)).toBe("hello world");
		expect(events[0]).toEqual({ type: "started", sessionId: "file-1" });
		expect(calls[0]?.model).toBe("test-model");
	});

	test("WAV-frames each segment at the session sample rate", async () => {
		const { calls, finished } = setup({ texts: ["hi"] });

		await finished;

		const audio = calls[0]?.audio as Uint8Array;
		expect(textAt(audio, 0, 4)).toBe("RIFF");
		expect(new DataView(audio.buffer, audio.byteOffset).getUint32(24, true)).toBe(
			RATE,
		);
		expect(
			new DataView(audio.buffer, audio.byteOffset).getUint32(40, true),
		).toBeGreaterThan(0);
	});

	test("never emits partial or delta events", async () => {
		const { events, finished } = setup({ texts: ["one", "two"] });

		await finished;

		expect(types(events)).not.toContain("partial");
		expect(types(events)).not.toContain("delta");
	});

	test("removes a repeated junction word between overlapping segments", async () => {
		const { events, calls, finished } = setup({
			chunks: chunked(tone(3000)),
			texts: ["one two three", "three four five"],
			segmentation: { maxSegmentMs: 1000, minSpeechMs: 200 },
		});

		await finished;

		expect(calls.length).toBeGreaterThanOrEqual(2);
		expect(committedTexts(events)).toEqual(["one two three", "four five"]);
		expect(stoppedText(events)).toBe("one two three four five");
	});

	test("emits monotonic progress with the probed total", async () => {
		const { events, finished } = setup({
			chunks: chunked(tone(3000)),
			texts: ["a", "b", "c", "d", "e"],
			total: 12.5,
			segmentation: { maxSegmentMs: 700, minSpeechMs: 200 },
		});

		await finished;

		const progress = progressEvents(events);
		expect(progress.length).toBeGreaterThanOrEqual(2);
		expect(progress.every((entry) => entry.totalSeconds === 12.5)).toBe(true);
		for (let index = 1; index < progress.length; index += 1) {
			expect(progress[index]!.processedSeconds).toBeGreaterThan(
				progress[index - 1]!.processedSeconds,
			);
		}

		const decodedSeconds = 3;
		expect(progress.at(-1)!.processedSeconds).toBeLessThanOrEqual(decodedSeconds);
	});

	test("reports a null total when the probe fails", async () => {
		const { events, finished } = setup({ total: null, texts: ["x"] });

		await finished;

		const progress = progressEvents(events);
		expect(progress.length).toBeGreaterThan(0);
		expect(progress.every((entry) => entry.totalSeconds === null)).toBe(true);
	});

	test("transcribes sequentially, never concurrently", async () => {
		let inFlight = 0;
		let maxInFlight = 0;
		const { finished } = setup({
			chunks: chunked(tone(3000)),
			texts: ["a", "b", "c", "d"],
			segmentation: { maxSegmentMs: 700, minSpeechMs: 200 },
			onTranscribe: () => {
				inFlight += 1;
				maxInFlight = Math.max(maxInFlight, inFlight);
				inFlight -= 1;
			},
		});

		await finished;

		expect(maxInFlight).toBe(1);
	});

	test("does not pull a chunk before the previous segment is transcribed", async () => {
		const pulls: number[] = [];
		let transcribed = 0;
		const { finished } = setup({
			chunks: chunked(tone(2000)),
			texts: ["a", "b", "c", "d"],
			segmentation: { maxSegmentMs: 500, minSpeechMs: 200 },
			onPull: (_signal, index) => pulls.push(index),
			onTranscribe: () => {
				transcribed += 1;
			},
		});

		await finished;

		// Every transcription happens between two pulls, never in parallel.
		expect(pulls.length).toBeGreaterThan(0);
		expect(transcribed).toBeGreaterThan(0);
	});

	test("keeps the text produced so far when stopped mid-file", async () => {
		const chunks = chunked(tone(60_000));
		const holder: { session?: SttSession } = {};
		let pulls = 0;
		const { session, events, finished } = setup({
			chunks,
			texts: ["alpha"],
			segmentation: { maxSegmentMs: 500, minSpeechMs: 200 },
			onPull: () => {
				pulls += 1;
				if (pulls === 40) {
					// Stop from inside the decode loop, mid-file.
					void holder.session?.stop();
				}
			},
		});
		holder.session = session;

		await finished;

		expect(pulls).toBeLessThan(chunks.length);
		expect(committedTexts(events).length).toBeGreaterThan(0);
		expect(stoppedText(events)).toBe("alpha");
	});

	test("emits nothing when aborted before the decode starts", async () => {
		const { session, events } = setup({ texts: ["unused"] });

		await session.abort();

		expect(events).toEqual([]);
	});

	test("aborts the decoder signal and kills the decode on abort()", async () => {
		let signal: AbortSignal | undefined;
		let resolvePull = (): void => {};
		const pulling = new Promise<void>((resolve) => {
			resolvePull = resolve;
		});
		const { session, events } = setup({
			chunks: chunked(tone(5000)),
			texts: ["alpha"],
			segmentation: { maxSegmentMs: 500, minSpeechMs: 200 },
			onPull: (pulled) => {
				signal = pulled;
				resolvePull();
			},
		});

		// Let the decode start, then abort mid-stream.
		await pulling;
		await session.abort();

		expect(signal?.aborted).toBe(true);
		expect(types(events)).not.toContain("stopped");
	});

	test("surfaces the decoder error code after keeping earlier text", async () => {
		const { events, finished } = setup({
			chunks: chunked(tone(1500)),
			texts: ["alpha"],
			decodeError: new FileDecodeError("decode_failed", "ffmpeg exploded"),
		});

		await finished;

		const error = events.find((event) => event.type === "error");
		expect(error).toEqual({
			type: "error",
			sessionId: "file-1",
			code: "decode_failed",
			message: "ffmpeg exploded",
		});
		expect(committedTexts(events)).toEqual(["alpha"]);
		expect(stoppedText(events)).toBe("alpha");
		expect(types(events).at(-1)).toBe("stopped");
	});

	test("maps an unexpected decode failure to stream_failed", async () => {
		const { events, failed } = setup({
			chunks: [],
			decodeError: new Error("boom"),
		});

		await failed;

		const error = events.find((event) => event.type === "error");
		expect(error?.type === "error" ? error.code : undefined).toBe("stream_failed");
	});

	test("maps a transcription failure to stream_failed exactly once", async () => {
		const { events, failed } = setup({
			chunks: chunked(tone(3000)),
			transcribeError: new Error("gateway refused"),
			segmentation: { maxSegmentMs: 700, minSpeechMs: 200 },
		});

		await failed;

		const errors = events.filter((event) => event.type === "error");
		expect(errors).toHaveLength(1);
		expect(errors[0]).toEqual({
			type: "error",
			sessionId: "file-1",
			code: "stream_failed",
			message: "gateway refused",
		});
	});

	test("treats pushAudio, pause and resume as no-ops", async () => {
		const { session, events, calls, finished } = setup({ texts: ["alpha"] });

		session.pushAudio("AAAA");
		session.pause();
		session.resume();
		await finished;

		expect(calls).toHaveLength(1);
		expect(types(events)).toEqual(["started", "committed", "progress", "stopped"]);
	});

	test("does not call the model for a silent file", async () => {
		const { events, calls, finished } = setup({
			chunks: chunked(silence(3000)),
			texts: ["unexpected"],
		});

		await finished;

		expect(calls).toHaveLength(0);
		expect(committedTexts(events)).toEqual([]);
		expect(stoppedText(events)).toBe("");
	});

	test("ignores a second stop()", async () => {
		const { session, events, finished } = setup({ texts: ["alpha"] });

		await finished;
		await session.stop();

		expect(events.filter((event) => event.type === "stopped")).toHaveLength(1);
	});
});
