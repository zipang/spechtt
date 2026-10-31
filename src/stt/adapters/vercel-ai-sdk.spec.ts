import { describe, expect, test } from "bun:test";
import type { StreamTranscriptionResult, TranscriptionStreamPart } from "ai";
import { MockTranscriptionModelV4, simulateReadableStream } from "ai/test";
import type { OutboundEvent } from "../protocol";
import type { SttSessionConfig } from "./types";
import {
	type StreamTranscribeFn,
	VercelAiSdkAdapter,
	isEmptyInputFailure,
} from "./vercel-ai-sdk";

const config: SttSessionConfig = {
	sessionId: "session-1",
	provider: "vercel",
	model: "openai/gpt-realtime-whisper",
	sampleRate: 24000,
	apiKey: "sk-test",
};

/** 40 ms of digital silence: what the capture layer sends when nothing is heard. */
const SILENT_CHUNK = Buffer.alloc(640).toString("base64");

interface FakeResultOptions {
	parts?: TranscriptionStreamPart[];
	finalText?: string;
	failWith?: Error;
}

function fakeResult({
	parts = [],
	finalText = "",
	failWith,
}: FakeResultOptions = {}): StreamTranscriptionResult {
	const fullStream = (async function* () {
		yield* parts;
		if (failWith !== undefined) {
			throw failWith;
		}
	})();

	return {
		fullStream,
		text: Promise.resolve(finalText),
		segments: Promise.resolve([]),
		language: Promise.resolve(undefined),
		durationInSeconds: Promise.resolve(undefined),
		warnings: Promise.resolve([]),
		responses: Promise.resolve([]),
		providerMetadata: Promise.resolve({}),
	} as unknown as StreamTranscriptionResult;
}

function collect() {
	const events: OutboundEvent[] = [];
	return { events, emit: (event: OutboundEvent) => events.push(event) };
}

describe("VercelAiSdkAdapter", () => {
	test("passes base64 chunks through to the AI SDK audio stream", async () => {
		let received: Parameters<StreamTranscribeFn>[0] | undefined;
		const streamTranscribe: StreamTranscribeFn = (options) => {
			received = options;
			return fakeResult({ finalText: "" });
		};
		const { events, emit } = collect();
		const adapter = new VercelAiSdkAdapter({
			streamTranscribe,
			resolveModel: () => "test-model",
		});

		const session = await adapter.start(config, emit);
		expect(events).toEqual([{ type: "started", sessionId: "session-1" }]);
		expect(received?.inputAudioFormat).toEqual({
			type: "audio/pcm",
			rate: 24000,
		});
		expect(received?.model).toBe("test-model");

		const reader = received!.audio.getReader();
		session.pushAudio("QQ==");
		session.pushAudio("Qg==");

		expect(await reader.read()).toEqual({ value: "QQ==", done: false });
		expect(await reader.read()).toEqual({ value: "Qg==", done: false });

		await session.stop();
		expect(await reader.read()).toEqual({ value: undefined, done: true });
	});

	test("drops audio while paused and forwards audio after resume", async () => {
		let received: Parameters<StreamTranscribeFn>[0] | undefined;
		const streamTranscribe: StreamTranscribeFn = (options) => {
			received = options;
			return fakeResult();
		};
		const { emit } = collect();
		const adapter = new VercelAiSdkAdapter({
			streamTranscribe,
			resolveModel: () => "test-model",
		});

		const session = await adapter.start(config, emit);
		const reader = received!.audio.getReader();

		session.pause();
		session.pushAudio("dropped");
		session.resume();
		session.pushAudio("kept");

		expect(await reader.read()).toEqual({ value: "kept", done: false });

		await session.stop();
	});

	test("maps stream parts to partial, delta and committed events", async () => {
		const streamTranscribe: StreamTranscribeFn = () =>
			fakeResult({
				parts: [
					{ type: "transcript-partial", text: "hel" },
					{ type: "transcript-delta", delta: "lo" },
					{ type: "transcript-final", text: "hello" },
					{ type: "transcript-partial", text: "wor" },
					{ type: "transcript-final", text: "world" },
				],
				finalText: "",
			});
		const { events, emit } = collect();
		const adapter = new VercelAiSdkAdapter({
			streamTranscribe,
			resolveModel: () => "test-model",
		});

		const session = await adapter.start(config, emit);
		await session.stop();

		expect(events).toEqual([
			{ type: "started", sessionId: "session-1" },
			{ type: "partial", sessionId: "session-1", text: "hel" },
			{ type: "delta", sessionId: "session-1", delta: "lo" },
			{ type: "committed", sessionId: "session-1", text: "hello" },
			{ type: "partial", sessionId: "session-1", text: "wor" },
			{ type: "committed", sessionId: "session-1", text: "world" },
			{
				type: "stopped",
				sessionId: "session-1",
				fullText: "hello world",
			},
		]);
	});

	test("prefers the final result text over the accumulated segments", async () => {
		const streamTranscribe: StreamTranscribeFn = () =>
			fakeResult({
				parts: [{ type: "transcript-final", text: "hello" }],
				finalText: "hello from the provider",
			});
		const { events, emit } = collect();
		const adapter = new VercelAiSdkAdapter({
			streamTranscribe,
			resolveModel: () => "test-model",
		});

		const session = await adapter.start(config, emit);
		await session.stop();

		expect(events.at(-1)).toEqual({
			type: "stopped",
			sessionId: "session-1",
			fullText: "hello from the provider",
		});
	});

	test("emits stream_failed without throwing when the provider fails", async () => {
		const streamTranscribe: StreamTranscribeFn = () =>
			fakeResult({ failWith: new Error("provider down") });
		const { events, emit } = collect();
		const adapter = new VercelAiSdkAdapter({
			streamTranscribe,
			resolveModel: () => "test-model",
		});

		const session = await adapter.start(config, emit);
		session.pushAudio("QQ==");
		await expect(session.stop()).resolves.toBeUndefined();

		expect(events).toContainEqual({
			type: "error",
			sessionId: "session-1",
			code: "stream_failed",
			message: "provider down",
		});
		expect(events.filter((event) => event.type === "error")).toHaveLength(1);
		expect(events.at(-1)).toEqual({
			type: "stopped",
			sessionId: "session-1",
			fullText: "",
		});
	});

	test("a silent stop never surfaces a provider empty-input failure", async () => {
		const streamTranscribe: StreamTranscribeFn = () =>
			fakeResult({
				failWith: new Error("no audio to transcribe"),
			});
		const { events, emit } = collect();
		const adapter = new VercelAiSdkAdapter({
			streamTranscribe,
			resolveModel: () => "test-model",
		});

		const session = await adapter.start(config, emit);
		await expect(session.stop()).resolves.toBeUndefined();

		expect(events).toEqual([
			{ type: "started", sessionId: "session-1" },
			{ type: "stopped", sessionId: "session-1", fullText: "" },
		]);
		expect(events.some((event) => event.type === "error")).toBe(false);
	});

	test("a silent take never surfaces a provider empty-input failure", async () => {
		const streamTranscribe: StreamTranscribeFn = () =>
			fakeResult({ failWith: new Error("Nothing was recorded.") });
		const { events, emit } = collect();
		const adapter = new VercelAiSdkAdapter({
			streamTranscribe,
			resolveModel: () => "test-model",
		});

		const session = await adapter.start(config, emit);
		// Silence still travels as PCM16 chunks, so the session did receive
		// audio; the provider is the one reporting there was nothing to hear.
		for (let index = 0; index < 5; index += 1) {
			session.pushAudio(SILENT_CHUNK);
		}
		await expect(session.stop()).resolves.toBeUndefined();

		expect(events).toEqual([
			{ type: "started", sessionId: "session-1" },
			{ type: "stopped", sessionId: "session-1", fullText: "" },
		]);
	});

	test("a silent take still reports a genuine provider failure", async () => {
		const streamTranscribe: StreamTranscribeFn = () =>
			fakeResult({ failWith: new Error("provider down") });
		const { events, emit } = collect();
		const adapter = new VercelAiSdkAdapter({
			streamTranscribe,
			resolveModel: () => "test-model",
		});

		const session = await adapter.start(config, emit);
		session.pushAudio(SILENT_CHUNK);
		await session.stop();

		expect(events.filter((event) => event.type === "error")).toHaveLength(1);
		expect(events.at(-1)).toEqual({
			type: "stopped",
			sessionId: "session-1",
			fullText: "",
		});
	});

	test("an empty-input error after real text is still reported", async () => {
		const streamTranscribe: StreamTranscribeFn = () =>
			fakeResult({
				parts: [{ type: "transcript-final", text: "hello" }],
				failWith: new Error("Nothing was recorded."),
			});
		const { events, emit } = collect();
		const adapter = new VercelAiSdkAdapter({
			streamTranscribe,
			resolveModel: () => "test-model",
		});

		const session = await adapter.start(config, emit);
		session.pushAudio("UklGRg==");
		await session.stop();

		expect(events).toContainEqual({
			type: "error",
			sessionId: "session-1",
			code: "stream_failed",
			message: "Nothing was recorded.",
		});
	});

	test("isEmptyInputFailure recognises empty-input provider messages", () => {
		for (const message of [
			"Nothing was recorded.",
			"no speech detected",
			"audio is empty",
			"the request contained no audio",
			"There is nothing to transcribe",
		]) {
			expect(isEmptyInputFailure(message)).toBe(true);
		}
		for (const message of [
			"provider down",
			"401 invalid api key",
			"socket closed",
		]) {
			expect(isEmptyInputFailure(message)).toBe(false);
		}
	});

	test("emits stream_failed when the SDK throws synchronously", async () => {
		const streamTranscribe: StreamTranscribeFn = () => {
			throw new Error("model does not support streaming");
		};
		const { events, emit } = collect();
		const adapter = new VercelAiSdkAdapter({
			streamTranscribe,
			resolveModel: () => "test-model",
		});

		const session = await adapter.start(config, emit);
		expect(events).toEqual([
			{
				type: "error",
				sessionId: "session-1",
				code: "stream_failed",
				message: "model does not support streaming",
			},
		]);

		await session.stop();
		expect(events.at(-1)).toEqual({
			type: "stopped",
			sessionId: "session-1",
			fullText: "",
		});
	});

	test("works with the installed experimental_streamTranscribe implementation", async () => {
		const model = new MockTranscriptionModelV4({
			doStream: async () => ({
				stream: simulateReadableStream({
					chunks: [
						{ type: "stream-start", warnings: [] },
						{ type: "transcript-partial", text: "hel" },
						{ type: "transcript-delta", delta: "lo" },
						{ type: "transcript-final", text: "hello" },
						{ type: "finish", text: "hello", segments: [] },
					],
					initialDelayInMs: 0,
					chunkDelayInMs: 0,
				}),
			}),
		});
		const { events, emit } = collect();
		const adapter = new VercelAiSdkAdapter({
			resolveModel: () => model,
		});

		const session = await adapter.start(config, emit);
		await session.stop();

		expect(events).toEqual([
			{ type: "started", sessionId: "session-1" },
			{ type: "partial", sessionId: "session-1", text: "hel" },
			{ type: "delta", sessionId: "session-1", delta: "lo" },
			{ type: "committed", sessionId: "session-1", text: "hello" },
			{ type: "stopped", sessionId: "session-1", fullText: "hello" },
		]);
	});

	test("abort tears down the stream without emitting errors", async () => {
		const streamTranscribe: StreamTranscribeFn = () => {
			const fullStream = (async function* () {
				await new Promise(() => {});
			})();
			return {
				...fakeResult(),
				fullStream,
			} as unknown as StreamTranscriptionResult;
		};
		const { events, emit } = collect();
		const adapter = new VercelAiSdkAdapter({
			streamTranscribe,
			resolveModel: () => "test-model",
		});

		const session = await adapter.start(config, emit);
		await session.abort(new Error("stdin closed"));

		expect(events).toEqual([{ type: "started", sessionId: "session-1" }]);
	});
});
