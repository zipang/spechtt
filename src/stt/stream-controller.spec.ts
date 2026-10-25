import { describe, expect, test } from "bun:test";
import { AudioStreamBridge } from "./stream-controller";

function readerFor(bridge: AudioStreamBridge) {
	return bridge.readable.getReader();
}

describe("AudioStreamBridge", () => {
	test("enqueues base64 chunks in arrival order", async () => {
		const bridge = new AudioStreamBridge();
		const reader = readerFor(bridge);

		expect(bridge.pushAudio("QQ==")).toBe(true);
		expect(bridge.pushAudio("Qg==")).toBe(true);

		expect(await reader.read()).toEqual({ value: "QQ==", done: false });
		expect(await reader.read()).toEqual({ value: "Qg==", done: false });

		bridge.close();
		expect(await reader.read()).toEqual({ value: undefined, done: true });
	});

	test("drops chunks while paused and re-enables them on resume", async () => {
		const bridge = new AudioStreamBridge();
		const reader = readerFor(bridge);

		bridge.pause();
		expect(bridge.paused).toBe(true);
		expect(bridge.pushAudio("dropped-1")).toBe(false);

		bridge.resume();
		expect(bridge.paused).toBe(false);

		expect(bridge.pushAudio("kept")).toBe(true);
		expect(await reader.read()).toEqual({ value: "kept", done: false });

		bridge.close();
		await reader.read();
	});

	test("pause and resume are idempotent", async () => {
		const bridge = new AudioStreamBridge();
		const reader = readerFor(bridge);

		bridge.pause();
		bridge.pause();
		bridge.resume();
		bridge.resume();

		expect(bridge.pushAudio("kept")).toBe(true);
		expect(await reader.read()).toEqual({ value: "kept", done: false });

		bridge.close();
	});

	test("close ends the stream and drops later chunks", async () => {
		const bridge = new AudioStreamBridge();
		const reader = readerFor(bridge);

		bridge.close();
		bridge.close();

		expect(bridge.closed).toBe(true);
		expect(bridge.pushAudio("late")).toBe(false);
		expect(await reader.read()).toEqual({ value: undefined, done: true });
	});

	test("abort aborts the signal and fails pending reads", async () => {
		const bridge = new AudioStreamBridge();
		const reader = readerFor(bridge);
		const reason = new Error("torn down");
		const pending = reader.read();

		bridge.abort(reason);

		expect(bridge.aborted).toBe(true);
		expect(bridge.closed).toBe(true);
		expect(bridge.abortSignal.aborted).toBe(true);
		expect(bridge.abortSignal.reason).toBe(reason);
		expect(bridge.pushAudio("late")).toBe(false);
		await expect(pending).rejects.toThrow("torn down");
	});

	test("abort after close is a no-op for the stream", async () => {
		const bridge = new AudioStreamBridge();
		const reader = readerFor(bridge);

		bridge.close();
		bridge.abort(new Error("ignored"));

		expect(bridge.aborted).toBe(false);
		expect(await reader.read()).toEqual({ value: undefined, done: true });
	});
});
