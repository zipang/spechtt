import { describe, expect, test } from "bun:test";
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

const config: SttSessionConfig = {
	sessionId: "session-1",
	provider: "mistral",
	model: "voxtral-mini-transcribe-3-0",
	sampleRate: 16000,
	apiKey: "sk-test",
};

const pcm = new Uint8Array([0x01, 0x00, 0x02, 0x00, 0xff, 0x7f, 0x00, 0x80]);
const pcmBase64 = Buffer.from(pcm).toString("base64");

function collect() {
	const events: OutboundEvent[] = [];
	return { events, emit: (event: OutboundEvent) => events.push(event) };
}

function textAt(bytes: Uint8Array, start: number, length: number): string {
	return String.fromCharCode(...bytes.slice(start, start + length));
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
		const wav = pcm16ToWav(pcm, 16000);

		expect(wav).toHaveLength(44 + pcm.length);
		expect(wav.slice(44)).toEqual(pcm);
		expect(new DataView(wav.buffer).getUint32(40, true)).toBe(pcm.length);
	});

	test("decodes base64 PCM16 before framing", () => {
		const wav = pcm16Base64ToWav(pcmBase64, 24000);
		const view = new DataView(wav.buffer);

		expect(view.getUint32(24, true)).toBe(24000);
		expect(view.getUint32(28, true)).toBe(48000);
		expect(wav.slice(44)).toEqual(pcm);
	});
});

describe("MistralAdapter", () => {
	test("buffers audio and transcribes a WAV segment on stop", async () => {
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
		session.pushAudio(pcmBase64);
		session.pushAudio(pcmBase64);
		await session.stop();

		expect(calls).toHaveLength(1);
		expect(calls[0]?.model).toBe("test-model");
		expect(calls[0]?.audio).toEqual(
			pcm16ToWav(new Uint8Array([...pcm, ...pcm]), 16000),
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
		session.pushAudio(pcmBase64);
		session.pause();
		session.resume();
		session.pushAudio(pcmBase64);
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
		session.pushAudio(pcmBase64);
		session.resume();
		await session.stop();

		expect(calls).toBe(0);
		expect(events).toEqual([
			{ type: "started", sessionId: "session-1" },
			{ type: "stopped", sessionId: "session-1", fullText: "" },
		]);
	});

	test("flushes automatically when the segment buffer fills", async () => {
		let calls = 0;
		const transcribe: TranscribeFn = async () => {
			calls += 1;
			return { text: "auto" } as never;
		};
		const { events, emit } = collect();
		const adapter = new MistralAdapter({
			transcribe,
			resolveModel: () => "test-model",
			// 16000 Hz * 2 bytes * 0.000125 s = 4 bytes.
			segmentSeconds: 0.000125,
		});

		const session = await adapter.start(config, emit);
		session.pushAudio(pcmBase64);
		await session.stop();

		expect(calls).toBe(1);
		expect(events).toContainEqual({
			type: "committed",
			sessionId: "session-1",
			text: "auto",
		});
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
		session.pushAudio(pcmBase64);
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
		session.pushAudio(pcmBase64);
		await session.stop();

		expect(received).toHaveLength(1);
		expect(received[0]).toEqual(pcm16ToWav(pcm, 16000));
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
		session.pushAudio(pcmBase64);
		await session.abort(new Error("stdin closed"));

		expect(calls).toBe(0);
		expect(events).toEqual([{ type: "started", sessionId: "session-1" }]);
	});
});
