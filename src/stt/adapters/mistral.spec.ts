import { afterEach, beforeEach, describe, expect, spyOn, test } from "bun:test";
import { MockTranscriptionModelV4 } from "ai/test";
import type { OutboundEvent } from "../protocol";
import type { SttSessionConfig } from "./types";
import {
	MistralAdapter,
	type TranscribeFn,
	createWavHeader,
	pcm16Base64ToWav,
	pcm16ToWav,
} from "./mistral";

const RATE = 16_000;

const config: SttSessionConfig = {
	sessionId: "session-1",
	provider: "mistral",
	model: "voxtral-mini-transcribe-3-0",
	sampleRate: RATE,
	apiKey: "sk-test",
};

function collect() {
	const events: OutboundEvent[] = [];
	return { events, emit: (event: OutboundEvent) => events.push(event) };
}

function textAt(bytes: Uint8Array, start: number, length: number): string {
	return String.fromCharCode(...bytes.slice(start, start + length));
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

function base64(bytes: Uint8Array): string {
	return Buffer.from(bytes).toString("base64");
}

/** Feeds audio the way Rust does: 40 ms (640 byte) chunks. */
function pushPcm(
	session: { pushAudio(pcm16Base64: string): void },
	bytes: Uint8Array,
	chunkSize = 640,
): void {
	for (let offset = 0; offset < bytes.length; offset += chunkSize) {
		session.pushAudio(base64(bytes.subarray(offset, offset + chunkSize)));
	}
}

function committedTexts(events: OutboundEvent[]): string[] {
	return events.flatMap((event) =>
		event.type === "committed" ? [event.text] : [],
	);
}

describe("WAV framing", () => {
	test("writes a 44-byte PCM16 mono header", () => {
		const header = createWavHeader(3200, 16000);
		const view = new DataView(header.buffer, header.byteOffset, 44);

		expect(header).toHaveLength(44);
		expect(textAt(header, 0, 4)).toBe("RIFF");
		expect(view.getUint32(4, true)).toBe(36 + 3200);
		expect(textAt(header, 8, 4)).toBe("WAVE");
		expect(textAt(header, 12, 4)).toBe("fmt ");
		expect(view.getUint32(16, true)).toBe(16);
		expect(view.getUint16(20, true)).toBe(1);
		expect(view.getUint16(22, true)).toBe(1);
		expect(view.getUint32(24, true)).toBe(16000);
		expect(view.getUint32(28, true)).toBe(32000);
		expect(view.getUint16(32, true)).toBe(2);
		expect(view.getUint16(34, true)).toBe(16);
		expect(textAt(header, 36, 4)).toBe("data");
		expect(view.getUint32(40, true)).toBe(3200);
	});

	test("appends PCM16 bytes after the header", () => {
		const pcm = new Uint8Array([0x01, 0x00, 0x02, 0x00, 0xff, 0x7f, 0x00, 0x80]);
		const wav = pcm16ToWav(pcm, 16000);

		expect(wav).toHaveLength(44 + pcm.length);
		expect(wav.slice(44)).toEqual(pcm);
		expect(new DataView(wav.buffer).getUint32(40, true)).toBe(pcm.length);
	});

	test("decodes base64 PCM16 before framing", () => {
		const pcm = new Uint8Array([0x01, 0x00, 0x02, 0x00]);
		const wav = pcm16Base64ToWav(base64(pcm), 24000);
		const view = new DataView(wav.buffer);

		expect(view.getUint32(24, true)).toBe(24000);
		expect(view.getUint32(28, true)).toBe(48000);
		expect(wav.slice(44)).toEqual(pcm);
	});
});

describe("MistralAdapter", () => {
	let errorSpy: ReturnType<typeof spyOn>;

	beforeEach(() => {
		// Segment diagnostics are informational; keep the test output readable.
		errorSpy = spyOn(console, "error").mockImplementation(() => {});
	});

	afterEach(() => {
		errorSpy.mockRestore();
	});

	test("transcribes a silence-padded WAV segment on stop", async () => {
		const calls: Parameters<TranscribeFn>[0][] = [];
		const transcribe: TranscribeFn = async (options) => {
			calls.push(options);
			return { text: "hello world" } as never;
		};
		const { events, emit } = collect();
		const adapter = new MistralAdapter({
			transcribe,
			resolveModel: () => "test-model",
		});

		const session = await adapter.start(config, emit);
		pushPcm(session, tone(1500));
		await session.stop();

		expect(calls).toHaveLength(1);
		expect(calls[0]?.model).toBe("test-model");
		const audio = calls[0]?.audio as Uint8Array;
		expect(textAt(audio, 0, 4)).toBe("RIFF");
		// 1500 ms of speech plus the default 200 ms tail pad.
		expect(new DataView(audio.buffer, audio.byteOffset).getUint32(40, true)).toBe(
			(1500 + 200) * 32,
		);
		expect(events).toEqual([
			{ type: "started", sessionId: "session-1" },
			{ type: "committed", sessionId: "session-1", text: "hello world" },
			{ type: "stopped", sessionId: "session-1", fullText: "hello world" },
		]);
	});

	test("flushes on pause and again on stop, joining the segments", async () => {
		let call = 0;
		const transcribe: TranscribeFn = async () => {
			call += 1;
			return { text: call === 1 ? "first" : "second" } as never;
		};
		const { events, emit } = collect();
		const adapter = new MistralAdapter({
			transcribe,
			resolveModel: () => "test-model",
		});

		const session = await adapter.start(config, emit);
		pushPcm(session, tone(900));
		session.pause();
		session.resume();
		pushPcm(session, tone(900));
		await session.stop();

		expect(call).toBe(2);
		expect(events).toEqual([
			{ type: "started", sessionId: "session-1" },
			{ type: "committed", sessionId: "session-1", text: "first" },
			{ type: "committed", sessionId: "session-1", text: "second" },
			{
				type: "stopped",
				sessionId: "session-1",
				fullText: "first second",
			},
		]);
	});

	test("never calls the model for silence", async () => {
		let calls = 0;
		const transcribe: TranscribeFn = async () => {
			calls += 1;
			return { text: "unexpected" } as never;
		};
		const { events, emit } = collect();
		const adapter = new MistralAdapter({
			transcribe,
			resolveModel: () => "test-model",
		});

		const session = await adapter.start(config, emit);
		pushPcm(session, silence(3000));
		session.pause();
		session.resume();
		await session.stop();

		expect(calls).toBe(0);
		expect(events).toEqual([
			{ type: "started", sessionId: "session-1" },
			{ type: "stopped", sessionId: "session-1", fullText: "" },
		]);
	});

	test("an empty stop stays silent even if the provider would reject", async () => {
		const transcribe: TranscribeFn = async () => {
			throw new Error("nothing to transcribe");
		};
		const { events, emit } = collect();
		const adapter = new MistralAdapter({
			transcribe,
			resolveModel: () => "test-model",
		});

		const session = await adapter.start(config, emit);
		await expect(session.stop()).resolves.toBeUndefined();

		expect(events).toEqual([
			{ type: "started", sessionId: "session-1" },
			{ type: "stopped", sessionId: "session-1", fullText: "" },
		]);
	});

	test("drops audio pushed while paused", async () => {
		let calls = 0;
		const transcribe: TranscribeFn = async () => {
			calls += 1;
			return { text: "unexpected" } as never;
		};
		const { events, emit } = collect();
		const adapter = new MistralAdapter({
			transcribe,
			resolveModel: () => "test-model",
		});

		const session = await adapter.start(config, emit);
		session.pause();
		pushPcm(session, tone(500));
		session.resume();
		await session.stop();

		expect(calls).toBe(0);
		expect(events).toEqual([
			{ type: "started", sessionId: "session-1" },
			{ type: "stopped", sessionId: "session-1", fullText: "" },
		]);
	});

	test("cuts a long monologue at the configured cap", async () => {
		const calls: Parameters<TranscribeFn>[0][] = [];
		const transcribe: TranscribeFn = async (options) => {
			calls.push(options);
			return { text: "auto" } as never;
		};
		const { emit } = collect();
		const adapter = new MistralAdapter({
			transcribe,
			resolveModel: () => "test-model",
			segmentation: { maxSegmentMs: 1000, minSpeechMs: 200 },
		});

		const session = await adapter.start(config, emit);
		pushPcm(session, tone(3000));
		await session.stop();

		expect(calls.length).toBeGreaterThanOrEqual(2);
	});

	test("removes a repeated junction word after an overlap cut", async () => {
		let call = 0;
		const transcribe: TranscribeFn = async () => {
			call += 1;
			if (call === 1) {
				return { text: "one two three" } as never;
			}
			if (call === 2) {
				return { text: "three four" } as never;
			}
			return { text: "" } as never;
		};
		const { events, emit } = collect();
		const adapter = new MistralAdapter({
			transcribe,
			resolveModel: () => "test-model",
			segmentation: { maxSegmentMs: 1000, minSpeechMs: 200 },
		});

		const session = await adapter.start(config, emit);
		pushPcm(session, tone(3000));
		await session.stop();

		const stopped = events.at(-1);
		expect(stopped).toEqual({
			type: "stopped",
			sessionId: "session-1",
			fullText: "one two three four",
		});
		expect(committedTexts(events).join(" ")).toBe("one two three four");
	});

	test("writes segment diagnostics to stderr without transcript text", async () => {
		const transcribe: TranscribeFn = async () =>
			({ text: "private words" }) as never;
		const { emit } = collect();
		const adapter = new MistralAdapter({
			transcribe,
			resolveModel: () => "test-model",
		});

		const session = await adapter.start(config, emit);
		pushPcm(session, tone(1500));
		pushPcm(session, silence(700));
		await session.stop();

		const lines = errorSpy.mock.calls.map((call: unknown[]) => String(call[0]));
		expect(
			lines.some((line: string) =>
				/^\[stt-worker\] segment index=0 reason=silence durationMs=\d+ speechMs=\d+ rmsDb=-?\d+(\.\d+)? overlap=false$/.test(
					line,
				),
			),
		).toBe(true);
		expect(lines.some((line: string) => line.includes("private words"))).toBe(
			false,
		);
	});

	test("emits stream_failed and still stops when transcription fails", async () => {
		const transcribe: TranscribeFn = async () => {
			throw new Error("mistral unavailable");
		};
		const { events, emit } = collect();
		const adapter = new MistralAdapter({
			transcribe,
			resolveModel: () => "test-model",
		});

		const session = await adapter.start(config, emit);
		pushPcm(session, tone(900));
		await expect(session.stop()).resolves.toBeUndefined();

		expect(events).toContainEqual({
			type: "error",
			sessionId: "session-1",
			code: "stream_failed",
			message: "mistral unavailable",
		});
		expect(events.at(-1)).toEqual({
			type: "stopped",
			sessionId: "session-1",
			fullText: "",
		});
	});

	test("works with the installed transcribe implementation", async () => {
		const received: Uint8Array[] = [];
		const model = new MockTranscriptionModelV4({
			doGenerate: async ({ audio }) => {
				received.push(audio as Uint8Array);
				return {
					text: "hello via the SDK",
					segments: [],
					language: undefined,
					durationInSeconds: undefined,
					warnings: [],
					response: {
						timestamp: new Date(0),
						modelId: "mock-model",
					},
				};
			},
		});
		const { events, emit } = collect();
		const adapter = new MistralAdapter({
			resolveModel: () => model,
		});

		const session = await adapter.start(config, emit);
		pushPcm(session, tone(1500));
		await session.stop();

		expect(received).toHaveLength(1);
		expect(textAt(received[0] ?? new Uint8Array(), 0, 4)).toBe("RIFF");
		expect(events).toEqual([
			{ type: "started", sessionId: "session-1" },
			{
				type: "committed",
				sessionId: "session-1",
				text: "hello via the SDK",
			},
			{
				type: "stopped",
				sessionId: "session-1",
				fullText: "hello via the SDK",
			},
		]);
	});

	test("abort discards buffered audio without transcribing", async () => {
		let calls = 0;
		const transcribe: TranscribeFn = async () => {
			calls += 1;
			return { text: "unexpected" } as never;
		};
		const { events, emit } = collect();
		const adapter = new MistralAdapter({
			transcribe,
			resolveModel: () => "test-model",
		});

		const session = await adapter.start(config, emit);
		pushPcm(session, tone(900));
		await session.abort(new Error("stdin closed"));

		expect(calls).toBe(0);
		expect(events).toEqual([{ type: "started", sessionId: "session-1" }]);
	});
});
